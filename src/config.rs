use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Map;
use std::path::{Path, PathBuf};

/// mconf.jsonc configuration.
///
/// Format:
/// ```jsonc
/// {
///   "dprint": "~/.cargo/bin/dprint",
///   "profiles": {
///     "maintainer": "~/.config/dprint/dprint-maintainer.jsonc",
///     "default": "~/.config/dprint/dprint-default.jsonc",
///   },
///   "match": {
///     "**/noc/cmdb/**": "maintainer",
///     "**/noc/invapi/**": "maintainer",
///     "**": "default",
///   },
/// }
/// ```
#[derive(Debug, Deserialize)]
pub struct MconfConfig {
    /// Path to real dprint binary.
    pub dprint: String,

    /// Named profiles: name → config path.
    pub profiles: Map<String, serde_json::Value>,

    /// Ordered match rules: glob pattern → profile name.
    /// Uses serde_json::Map with preserve_order for first-match semantics.
    #[serde(rename = "match")]
    pub match_rules: Map<String, serde_json::Value>,

    /// Optional diff pager command for `dprint check` (e.g. "delta -s").
    /// When set, check produces unified diff output:
    /// - stdout is TTY → pipe through pager
    /// - stdout is pipe/redirect → raw unified diff
    #[serde(default)]
    pub diff_pager: Option<String>,
}

impl MconfConfig {
    /// Try to load config from the default location (~/.config/dprint/mconf.jsonc).
    /// Returns Ok(None) if the file doesn't exist.
    /// Returns Err if the file exists but is invalid.
    pub fn try_load_default() -> Result<Option<Self>> {
        let config_dir = dirs::config_dir().context("cannot determine config directory")?;
        let path = config_dir.join("dprint").join("mconf.jsonc");
        if !path.exists() {
            return Ok(None);
        }
        Self::load(&path).map(Some)
    }

    /// Load config from a specific path.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read config: {}", path.display()))?;

        // Strip JSONC comments (// and /* */) before parsing.
        let json = strip_jsonc_comments(&content);

        let config: MconfConfig =
            serde_json::from_str(&json).with_context(|| "invalid mconf.jsonc format")?;

        Ok(config)
    }

    /// Resolve dprint binary path (expand ~).
    pub fn dprint_path(&self) -> PathBuf {
        expand_tilde(&self.dprint)
    }

    /// Resolve a profile name to its config file path.
    pub fn profile_config_path(&self, profile_name: &str) -> Option<PathBuf> {
        self.profiles
            .get(profile_name)
            .and_then(|v| v.as_str().map(|s| expand_tilde(s)))
    }

    /// Get ordered match rules as (glob_pattern, profile_name) pairs.
    pub fn match_rules_iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.match_rules.iter().filter_map(|(pattern, value)| {
            value.as_str().map(|profile| (pattern.as_str(), profile))
        })
    }
}

/// Expand ~ to home directory in a path string.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Strip JSONC-style comments from a string.
/// Handles // line comments and /* */ block comments.
/// Does not strip inside strings.
fn strip_jsonc_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_string = false;

    while i < len {
        if in_string {
            result.push(chars[i]);
            if chars[i] == '\\' && i + 1 < len {
                i += 1;
                result.push(chars[i]);
            } else if chars[i] == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // Line comment
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '/' {
            // Skip until end of line
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Block comment
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            continue;
        }

        result.push(chars[i]);
        i += 1;
    }

    // Strip trailing commas before } and ] (JSONC allows them, JSON doesn't).
    strip_trailing_commas(&result)
}

/// Remove trailing commas before } and ].
fn strip_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_string = false;

    while i < len {
        if in_string {
            result.push(chars[i]);
            if chars[i] == '\\' && i + 1 < len {
                i += 1;
                result.push(chars[i]);
            } else if chars[i] == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            result.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == ',' {
            // Look ahead for } or ] (skipping whitespace)
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                // Skip the trailing comma
                i += 1;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_jsonc_comments() {
        let input = r#"{
  // line comment
  "key": "value", // inline comment
  /* block comment */
  "key2": "val with // not a comment"
}"#;
        let result = strip_jsonc_comments(input);
        assert!(result.contains("\"key\": \"value\""));
        assert!(result.contains("\"key2\": \"val with // not a comment\""));
        assert!(!result.contains("line comment"));
        assert!(!result.contains("inline comment"));
        assert!(!result.contains("block comment"));
    }

    #[test]
    fn test_strip_trailing_commas() {
        let input = r#"{"a": 1, "b": 2,}"#;
        let result = strip_trailing_commas(input);
        assert_eq!(result, r#"{"a": 1, "b": 2}"#);
    }

    #[test]
    fn test_expand_tilde() {
        let home = dirs::home_dir().unwrap();
        let result = expand_tilde("~/foo/bar");
        assert_eq!(result, home.join("foo/bar"));

        let result = expand_tilde("/absolute/path");
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_parse_full_mconf_jsonc() {
        let input = r#"{
  // Path to real dprint binary
  "dprint": "~/.cargo/bin/dprint",
  "profiles": {
    "maintainer": "~/.config/dprint/dprint-maintainer.jsonc",
    "default": "~/.config/dprint/dprint-default.jsonc",
  },
  "match": {
    "**/noc/cmdb/**": "maintainer",
    "**/noc/invapi/**": "maintainer",
    /* catch-all */
    "**": "default",
  },
}"#;
        let json = strip_jsonc_comments(input);
        let config: MconfConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.dprint, "~/.cargo/bin/dprint");
        assert_eq!(config.profiles.len(), 2);

        // Verify match rules preserve order (first match semantics).
        let rules: Vec<(&str, &str)> = config.match_rules_iter().collect();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0], ("**/noc/cmdb/**", "maintainer"));
        assert_eq!(rules[1], ("**/noc/invapi/**", "maintainer"));
        assert_eq!(rules[2], ("**", "default"));
    }
}
