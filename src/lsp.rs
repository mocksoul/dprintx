use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{self, MconfConfig};
use crate::matcher::ProfileMatcher;

/// Timeout for reading LSP responses from backends.
const READ_TIMEOUT: Duration = Duration::from_secs(1);

/// LSP proxy: spawns dprint lsp per profile, routes requests by file URI.
pub struct LspProxy {
    dprint_bin: PathBuf,
    matcher: ProfileMatcher,
    config: MconfConfig,
}

/// A running dprint lsp backend.
struct Backend {
    _child: Child,
    stdin: std::process::ChildStdin,
    responses: mpsc::Receiver<String>,
}

impl LspProxy {
    pub fn new(dprint_bin: PathBuf, matcher: ProfileMatcher, config: MconfConfig) -> Self {
        Self {
            dprint_bin,
            matcher,
            config,
        }
    }

    /// Run the LSP proxy. Blocks forever (until stdin closes).
    pub fn run(&self) -> Result<()> {
        eprintln!(
            "mconf: lsp proxy starting (timeout={}ms)",
            READ_TIMEOUT.as_millis()
        );

        // Map: profile config path -> backend.
        let backends: Arc<Mutex<HashMap<PathBuf, Backend>>> = Arc::new(Mutex::new(HashMap::new()));

        // Shared stdout lock for writing responses.
        let stdout = Arc::new(Mutex::new(io::stdout()));

        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());

        // Track initialize state for lazy backend spawning.
        let mut _initialized = false;
        let mut last_init_params: Option<serde_json::Value> = None;
        // Hold merged config guards alive for the lifetime of LSP backends.
        let mut _merged_guards: Vec<config::TempConfig> = Vec::new();

        loop {
            // Read LSP message (Content-Length header + body).
            let msg = match read_lsp_message(&mut reader) {
                Ok(msg) => msg,
                Err(_) => break, // EOF or error, exit.
            };

            // Parse as JSON.
            let parsed: serde_json::Value = match serde_json::from_str(&msg) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let method = parsed.get("method").and_then(|m| m.as_str());

            match method {
                Some("initialize") => {
                    // Start all backends with initialize.
                    let id = parsed.get("id").cloned();
                    let params = parsed.get("params").cloned();
                    last_init_params = params.clone();

                    // Spawn backends for each unique profile.
                    let mut profile_configs = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for (_pattern, profile_name) in self.config.match_rules_iter() {
                        if seen.insert(profile_name.to_string())
                            && let Some(config_path) =
                                self.config.profile_config_path(profile_name)
                        {
                            profile_configs.push(config_path);
                        }
                    }

                    // Spawn all backends.
                    let mut first_response = None;
                    for config_path in &profile_configs {
                        let backend = self.spawn_backend(config_path)?;
                        let mut backends_lock = backends.lock().unwrap();
                        backends_lock.insert(config_path.clone(), backend);
                        drop(backends_lock);

                        // Send initialize to this backend.
                        // Override rootUri to the config file's directory so dprint
                        // knows which workspace this backend serves.
                        let mut init_params = params.clone().unwrap_or(serde_json::json!({}));
                        if let Some(config_dir) = config_path.parent() {
                            let root_uri = format!("file://{}", config_dir.display());
                            init_params["rootUri"] = serde_json::Value::String(root_uri.clone());
                            // Also set rootPath for older LSP compat.
                            init_params["rootPath"] =
                                serde_json::Value::String(config_dir.display().to_string());
                        }
                        let init_msg = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "method": "initialize",
                            "params": init_params,
                        });

                        self.send_to_backend(&backends, config_path, &init_msg)?;

                        // Read response from this backend.
                        if let Ok(resp) = self.read_from_backend(&backends, config_path, &stdout)
                            && first_response.is_none()
                        {
                            first_response = Some(resp);
                        }
                    }

                    // Send first backend's response as our response.
                    if let Some(resp) = first_response {
                        write_lsp_message(&stdout, &resp)?;
                    }

                    _initialized = true;
                }

                Some("initialized") => {
                    // Forward to all backends.
                    let backends_lock = backends.lock().unwrap();
                    let keys: Vec<PathBuf> = backends_lock.keys().cloned().collect();
                    drop(backends_lock);

                    for config_path in &keys {
                        let _ = self.send_to_backend(&backends, config_path, &parsed);
                    }
                }

                Some("shutdown") => {
                    // Forward to all backends.
                    let backends_lock = backends.lock().unwrap();
                    let keys: Vec<PathBuf> = backends_lock.keys().cloned().collect();
                    drop(backends_lock);

                    for config_path in &keys {
                        let _ = self.send_to_backend(&backends, config_path, &parsed);
                        // Read response (with timeout).
                        let _ = self.read_from_backend(&backends, config_path, &stdout);
                    }

                    // Respond with null result.
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": parsed.get("id"),
                        "result": null,
                    });
                    write_lsp_message(&stdout, &serde_json::to_string(&response)?)?;
                }

                Some("exit") => {
                    // Forward to all backends and exit.
                    let backends_lock = backends.lock().unwrap();
                    let keys: Vec<PathBuf> = backends_lock.keys().cloned().collect();
                    drop(backends_lock);

                    for config_path in &keys {
                        let _ = self.send_to_backend(&backends, config_path, &parsed);
                    }
                    break;
                }

                Some(method) if method.starts_with("textDocument/") => {
                    // Route by file URI.
                    let uri = extract_uri(&parsed);
                    let method_name = method.to_string();
                    let has_id = parsed.get("id").is_some();
                    eprintln!(
                        "mconf: recv {} ({})",
                        method_name,
                        if has_id { "request" } else { "notification" }
                    );
                    if let Some(uri) = uri {
                        let file_path = uri_to_path(&uri);
                        let profile_config =
                            match self.matcher.resolve_config(&file_path, &self.config) {
                                Ok(Some(p)) => p,
                                _ => {
                                    // No profile matched — respond with null result if it's a request.
                                    if let Some(id) = parsed.get("id").cloned() {
                                        let null_resp = serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "result": null,
                                        });
                                        write_lsp_message(
                                            &stdout,
                                            &serde_json::to_string(&null_resp)?,
                                        )?;
                                    }
                                    continue;
                                }
                            };

                        // Resolve effective config (merged local + profile, or just profile).
                        let effective_config = if let Some(parent) = file_path.parent() {
                            match config::build_merged_config(parent, &profile_config) {
                                Ok(Some(tc)) => {
                                    let p = tc.path().to_path_buf();
                                    _merged_guards.push(tc);
                                    p
                                }
                                Ok(None) => profile_config,
                                Err(e) => {
                                    eprintln!("mconf: warning: build_merged_config failed: {e}");
                                    profile_config
                                }
                            }
                        } else {
                            profile_config
                        };

                        // Ensure backend is spawned (lazily for merged configs).
                        {
                            let backends_lock = backends.lock().unwrap();
                            if !backends_lock.contains_key(&effective_config) {
                                drop(backends_lock);
                                let backend = self.spawn_backend(&effective_config)?;
                                let mut backends_lock = backends.lock().unwrap();
                                backends_lock.insert(effective_config.clone(), backend);
                                drop(backends_lock);

                                // Send initialize to the new backend.
                                if let Some(init_params) = &last_init_params {
                                    let mut params = init_params.clone();
                                    if let Some(config_dir) = effective_config.parent() {
                                        let root_uri = format!("file://{}", config_dir.display());
                                        params["rootUri"] =
                                            serde_json::Value::String(root_uri.clone());
                                        params["rootPath"] = serde_json::Value::String(
                                            config_dir.display().to_string(),
                                        );
                                    }
                                    let init_msg = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": 1,
                                        "method": "initialize",
                                        "params": params,
                                    });
                                    let _ = self.send_to_backend(
                                        &backends,
                                        &effective_config,
                                        &init_msg,
                                    );
                                    // Read and discard initialize response.
                                    let _ = self.read_from_backend(
                                        &backends,
                                        &effective_config,
                                        &stdout,
                                    );

                                    // Send initialized notification.
                                    let initialized_msg = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "method": "initialized",
                                        "params": {},
                                    });
                                    let _ = self.send_to_backend(
                                        &backends,
                                        &effective_config,
                                        &initialized_msg,
                                    );
                                }
                            }
                        }

                        // Send request to the right backend.
                        self.send_to_backend(&backends, &effective_config, &parsed)?;

                        // If it's a request (has id), read response.
                        if let Some(id) = parsed.get("id").cloned() {
                            let t0 = std::time::Instant::now();
                            match self.read_from_backend(&backends, &effective_config, &stdout) {
                                Ok(resp) => {
                                    eprintln!(
                                        "mconf: {} responded in {:?}",
                                        method_name,
                                        t0.elapsed()
                                    );
                                    write_lsp_message(&stdout, &resp)?;
                                }
                                Err(e) => {
                                    eprintln!(
                                        "mconf: {} timeout/error in {:?}: {}",
                                        method_name,
                                        t0.elapsed(),
                                        e
                                    );
                                    let error_resp = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": null,
                                    });
                                    write_lsp_message(
                                        &stdout,
                                        &serde_json::to_string(&error_resp)?,
                                    )?;
                                }
                            }
                        }
                    }
                }

                _ => {
                    // Unknown method — forward to all backends.
                    let backends_lock = backends.lock().unwrap();
                    let keys: Vec<PathBuf> = backends_lock.keys().cloned().collect();
                    drop(backends_lock);

                    for config_path in &keys {
                        let _ = self.send_to_backend(&backends, config_path, &parsed);
                    }

                    // If it's a request, respond from first backend.
                    if let Some(id) = parsed.get("id").cloned()
                        && let Some(config_path) = keys.first()
                    {
                        match self.read_from_backend(&backends, config_path, &stdout) {
                            Ok(resp) => {
                                write_lsp_message(&stdout, &resp)?;
                            }
                            Err(_) => {
                                let error_resp = serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": null,
                                });
                                write_lsp_message(
                                    &stdout,
                                    &serde_json::to_string(&error_resp)?,
                                )?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn spawn_backend(&self, config_path: &PathBuf) -> Result<Backend> {
        let mut child = Command::new(&self.dprint_bin)
            .args(["lsp", "--config"])
            .arg(config_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("spawning dprint lsp --config {}", config_path.display()))?;

        let stdin = child.stdin.take().context("no stdin on dprint lsp")?;
        let child_stdout = child.stdout.take().context("no stdout on dprint lsp")?;

        // Spawn a reader thread that reads LSP messages and sends them via channel.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(child_stdout);
            while let Ok(msg) = read_lsp_message(&mut reader) {
                if tx.send(msg).is_err() {
                    break; // Receiver dropped.
                }
            }
        });

        Ok(Backend {
            _child: child,
            stdin,
            responses: rx,
        })
    }

    fn send_to_backend(
        &self,
        backends: &Arc<Mutex<HashMap<PathBuf, Backend>>>,
        config_path: &PathBuf,
        msg: &serde_json::Value,
    ) -> Result<()> {
        let json = serde_json::to_string(msg)?;
        let mut backends_lock = backends.lock().unwrap();
        if let Some(backend) = backends_lock.get_mut(config_path) {
            let header = format!("Content-Length: {}\r\n\r\n", json.len());
            backend.stdin.write_all(header.as_bytes())?;
            backend.stdin.write_all(json.as_bytes())?;
            backend.stdin.flush()?;
        }
        Ok(())
    }

    /// Read a response from a backend, skipping notifications.
    /// Notifications (messages without "id") are forwarded to the editor.
    fn read_from_backend(
        &self,
        backends: &Arc<Mutex<HashMap<PathBuf, Backend>>>,
        config_path: &PathBuf,
        stdout: &Arc<Mutex<io::Stdout>>,
    ) -> Result<String> {
        let backends_lock = backends.lock().unwrap();
        let backend = backends_lock
            .get(config_path)
            .context("backend not found")?;

        let deadline = std::time::Instant::now() + READ_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                bail!("backend read timeout");
            }

            let msg = backend
                .responses
                .recv_timeout(remaining)
                .context("backend read timeout")?;

            // Check if this is a response (has "id") or a notification.
            let parsed: serde_json::Value = serde_json::from_str(&msg)?;
            if parsed.get("id").is_some() {
                // It's a response — return it.
                return Ok(msg);
            }

            // It's a notification — forward to editor and keep waiting.
            let _ = write_lsp_message(stdout, &msg);
        }
    }
}

/// Read an LSP message from a buffered reader.
/// Format: "Content-Length: N\r\n\r\n" followed by N bytes.
fn read_lsp_message<R: BufRead>(reader: &mut R) -> Result<String> {
    let mut content_length: Option<usize> = None;

    // Read headers.
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            bail!("EOF while reading LSP headers");
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            break; // End of headers.
        }

        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse().context("invalid Content-Length")?);
        }
    }

    let length = content_length.context("missing Content-Length header")?;

    // Read body.
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    String::from_utf8(body).context("invalid UTF-8 in LSP message body")
}

/// Write an LSP message to stdout.
fn write_lsp_message(stdout: &Arc<Mutex<io::Stdout>>, body: &str) -> Result<()> {
    let mut out = stdout.lock().unwrap();
    write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    out.flush()?;
    Ok(())
}

/// Extract file URI from LSP params.
/// Looks for params.textDocument.uri.
fn extract_uri(msg: &serde_json::Value) -> Option<String> {
    msg.get("params")?
        .get("textDocument")?
        .get("uri")?
        .as_str()
        .map(|s| s.to_string())
}

/// Convert file:// URI to a filesystem path.
fn uri_to_path(uri: &str) -> PathBuf {
    if let Some(path) = uri.strip_prefix("file://") {
        // URL-decode percent-encoded characters.
        let decoded = percent_decode(path);
        PathBuf::from(decoded)
    } else {
        PathBuf::from(uri)
    }
}

/// Simple percent-decoding for file URIs.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16)
        {
            result.push(byte as char);
            i += 3;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_to_path() {
        assert_eq!(
            uri_to_path("file:///home/user/file.go"),
            PathBuf::from("/home/user/file.go")
        );
        assert_eq!(
            uri_to_path("file:///home/user/my%20file.go"),
            PathBuf::from("/home/user/my file.go")
        );
    }

    #[test]
    fn test_extract_uri() {
        let msg = serde_json::json!({
            "method": "textDocument/formatting",
            "params": {
                "textDocument": {
                    "uri": "file:///home/user/file.go"
                }
            }
        });
        assert_eq!(
            extract_uri(&msg),
            Some("file:///home/user/file.go".to_string())
        );
    }

    #[test]
    fn test_extract_uri_missing() {
        let msg = serde_json::json!({
            "method": "shutdown"
        });
        assert_eq!(extract_uri(&msg), None);
    }
}
