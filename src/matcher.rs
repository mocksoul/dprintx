use anyhow::{Context, Result, bail};
use globset::{Glob, GlobMatcher};
use std::path::Path;

use crate::config::{self, ContentMatcher, DprintxConfig, ProfileResolution};

/// Block size for reading file content during content matching.
/// File is read in line-aligned blocks of approximately this size.
const CONTENT_MATCH_BLOCK_BYTES: usize = 8192;

/// A compiled match rule: glob matcher + profile name.
struct Rule {
    matcher: GlobMatcher,
    profile: String,
}

/// Matches file paths to profiles using ordered glob rules,
/// with optional content-based override.
pub struct ProfileMatcher {
    rules: Vec<Rule>,
    content_matcher: Option<ContentMatcher>,
}

impl ProfileMatcher {
    /// Build a matcher from config match rules and content patterns.
    pub fn from_config(config: &DprintxConfig) -> Result<Self> {
        let mut rules = Vec::new();

        for (pattern, profile) in config.match_rules_iter() {
            // Expand ~ to home directory so globs like ~/workspace/** work.
            let expanded = config::expand_tilde(pattern);
            let expanded_str = expanded.to_string_lossy();
            let glob = Glob::new(&expanded_str)
                .with_context(|| format!("invalid glob pattern: {pattern}"))?;
            rules.push(Rule {
                matcher: glob.compile_matcher(),
                profile: profile.to_string(),
            });
        }

        let content_matcher = config.compile_content_patterns()?;

        Ok(Self {
            rules,
            content_matcher,
        })
    }

    /// Find the first matching profile for a file path.
    /// Returns None if no rule matches.
    pub fn match_profile(&self, path: &Path) -> Option<&str> {
        for rule in &self.rules {
            if rule.matcher.is_match(path) {
                return Some(&rule.profile);
            }
        }
        None
    }

    /// Resolve file path to profile resolution (path match only).
    ///
    /// Returns None if no match rule applies (file is skipped).
    /// Returns Some(Ignore) if matched profile is null.
    /// Returns Some(Config(path)) if matched profile has a config path.
    /// Errors if a matched profile name is not defined in profiles.
    ///
    /// This applies both path matching and content-based matching (if configured).
    /// Content match scans the entire file in line-aligned blocks and can
    /// override the path-based result.
    pub fn resolve_config(
        &self,
        file_path: &Path,
        config: &DprintxConfig,
    ) -> Result<Option<ProfileResolution>> {
        let path_resolution = self.resolve_config_by_path(file_path, config)?;

        // If no path match, file is unknown — skip without checking content.
        if path_resolution.is_none() {
            return Ok(None);
        }

        // If no content matcher configured, return path result as-is.
        let content_matcher = match &self.content_matcher {
            Some(cm) => cm,
            None => return Ok(path_resolution),
        };

        // Read file in blocks and check content patterns.
        match match_file_content(file_path, content_matcher) {
            Ok(Some(profile_name)) => {
                if let Some(resolution) = config.resolve_profile(&profile_name) {
                    return Ok(Some(resolution));
                }
                bail!(
                    "profile '{}' referenced in match_content but not defined in profiles",
                    profile_name
                );
            }
            Ok(None) => {} // No content match — keep path result.
            Err(_) => {}   // Can't read file — keep path result.
        }

        Ok(path_resolution)
    }

    /// Resolve file path by path matching only (no content check).
    fn resolve_config_by_path(
        &self,
        file_path: &Path,
        config: &DprintxConfig,
    ) -> Result<Option<ProfileResolution>> {
        if let Some(profile_name) = self.match_profile(file_path) {
            if let Some(resolution) = config.resolve_profile(profile_name) {
                return Ok(Some(resolution));
            }
            bail!(
                "profile '{}' referenced in match rules but not defined in profiles",
                profile_name
            );
        }
        Ok(None)
    }
}

/// Read a file in line-aligned blocks and match against content patterns.
/// Returns the profile name of the first matching pattern, or None.
/// Scans the entire file, matching each block independently.
fn match_file_content(path: &Path, matcher: &ContentMatcher) -> Result<Option<String>> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path)
        .with_context(|| format!("reading file for content match: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut block = String::with_capacity(CONTENT_MATCH_BLOCK_BYTES);

    loop {
        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .with_context(|| format!("reading file for content match: {}", path.display()))?;

        if bytes_read == 0 {
            // EOF — match remaining block.
            if !block.is_empty()
                && let Some(profile) = matcher.match_content(&block)
            {
                return Ok(Some(profile.to_string()));
            }
            return Ok(None);
        }

        block.push_str(&line);

        if block.len() >= CONTENT_MATCH_BLOCK_BYTES {
            if let Some(profile) = matcher.match_content(&block) {
                return Ok(Some(profile.to_string()));
            }
            block.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_config() -> DprintxConfig {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "maintainer": "/config/dprint-maintainer.jsonc",
                "default": "/config/dprint-default.jsonc"
            },
            "match": {
                "**/noc/cmdb/**": "maintainer",
                "**/noc/invapi/**": "maintainer",
                "**/mocksoul/gostern/**": "maintainer",
                "**": "default"
            }
        }"#;
        serde_json::from_str(config_json).unwrap()
    }

    #[test]
    fn test_match_first_wins() {
        let config = test_config();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        assert_eq!(
            matcher.match_profile(Path::new("/home/user/workspace/noc/cmdb/main.go")),
            Some("maintainer")
        );
        assert_eq!(
            matcher.match_profile(Path::new("/home/user/workspace/noc/invapi/server.go")),
            Some("maintainer")
        );
        assert_eq!(
            matcher.match_profile(Path::new("/home/user/other/file.go")),
            Some("default")
        );
    }

    #[test]
    fn test_no_match_returns_none() {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": { "strict": "/config/strict.jsonc" },
            "match": { "**/noc/cmdb/**": "strict" }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // No catch-all "**" rule, so non-matching paths return None.
        assert_eq!(
            matcher.match_profile(Path::new("/home/user/other/file.go")),
            None
        );
    }

    #[test]
    fn test_resolve_config_matched() {
        let config = test_config();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher
            .resolve_config(Path::new("/workspace/noc/cmdb/main.go"), &config)
            .unwrap();
        assert_eq!(
            result,
            Some(ProfileResolution::Config(PathBuf::from(
                "/config/dprint-maintainer.jsonc"
            )))
        );
    }

    #[test]
    fn test_resolve_config_no_match() {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": { "strict": "/config/strict.jsonc" },
            "match": { "**/noc/cmdb/**": "strict" }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher
            .resolve_config(Path::new("/other/file.go"), &config)
            .unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_config_ignore_profile() {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc",
                "ignore": null
            },
            "match": {
                "**/generated/**": "ignore",
                "**": "default"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher
            .resolve_config(Path::new("/workspace/generated/types.go"), &config)
            .unwrap();
        assert_eq!(result, Some(ProfileResolution::Ignore));

        let result = matcher
            .resolve_config(Path::new("/workspace/main.go"), &config)
            .unwrap();
        assert_eq!(
            result,
            Some(ProfileResolution::Config(PathBuf::from(
                "/config/default.jsonc"
            )))
        );
    }

    #[test]
    fn test_tilde_expansion_in_match_rules() {
        let home = dirs::home_dir().unwrap();
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "special": "/config/special.jsonc",
                "default": "/config/default.jsonc"
            },
            "match": {
                "~/workspace/myproject/**": "special",
                "**": "default"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // ~/workspace/myproject/foo.lua should match "special" after tilde expansion.
        let test_path = home.join("workspace/myproject/foo.lua");
        assert_eq!(matcher.match_profile(&test_path), Some("special"));

        // Other paths should fall through to "default".
        let other_path = home.join("workspace/other/bar.go");
        assert_eq!(matcher.match_profile(&other_path), Some("default"));
    }

    #[test]
    fn test_resolve_config_unknown_profile_errors() {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {},
            "match": { "**": "nonexistent" }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher.resolve_config(Path::new("/any/file.go"), &config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not defined in profiles")
        );
    }

    #[test]
    fn test_resolve_with_content_skips_generated() {
        let dir = std::env::temp_dir().join("dprintx-test-content-match");
        let _ = std::fs::create_dir_all(&dir);

        // Write a generated file.
        let gen_file = dir.join("generated.go");
        std::fs::write(
            &gen_file,
            "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage pb\n",
        )
        .unwrap();

        // Write a normal file.
        let normal_file = dir.join("main.go");
        std::fs::write(&normal_file, "package main\n\nfunc main() {}\n").unwrap();

        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc",
                "ignore": null
            },
            "match": { "**": "default" },
            "match_content": {
                "// Code generated .+ DO NOT EDIT\\.": "ignore"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // Generated file → ignore.
        let result = matcher.resolve_config(&gen_file, &config).unwrap();
        assert_eq!(result, Some(ProfileResolution::Ignore));

        // Normal file → default config.
        let result = matcher.resolve_config(&normal_file, &config).unwrap();
        assert_eq!(
            result,
            Some(ProfileResolution::Config(PathBuf::from(
                "/config/default.jsonc"
            )))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_with_content_no_path_match_skips_content_check() {
        let dir = std::env::temp_dir().join("dprintx-test-content-nopath");
        let _ = std::fs::create_dir_all(&dir);

        let gen_file = dir.join("generated.go");
        std::fs::write(
            &gen_file,
            "// Code generated by protoc. DO NOT EDIT.\npackage pb\n",
        )
        .unwrap();

        // No catch-all match rule — file won't match by path.
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "special": "/config/special.jsonc",
                "ignore": null
            },
            "match": { "**/specific-dir/**": "special" },
            "match_content": {
                "DO NOT EDIT": "ignore"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // No path match → None, content not checked.
        let result = matcher.resolve_config(&gen_file, &config).unwrap();
        assert_eq!(result, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_with_content_overrides_to_different_profile() {
        let dir = std::env::temp_dir().join("dprintx-test-content-override");
        let _ = std::fs::create_dir_all(&dir);

        let file = dir.join("strict_file.go");
        std::fs::write(&file, "// @format:strict\npackage main\n").unwrap();

        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc",
                "strict": "/config/strict.jsonc"
            },
            "match": { "**": "default" },
            "match_content": {
                "@format:strict": "strict"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // Content match overrides path match from "default" to "strict".
        let result = matcher.resolve_config(&file, &config).unwrap();
        assert_eq!(
            result,
            Some(ProfileResolution::Config(PathBuf::from(
                "/config/strict.jsonc"
            )))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_with_content_undefined_profile_errors() {
        let dir = std::env::temp_dir().join("dprintx-test-content-undef");
        let _ = std::fs::create_dir_all(&dir);

        let file = dir.join("test.go");
        std::fs::write(&file, "// DO NOT EDIT\npackage main\n").unwrap();

        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc"
            },
            "match": { "**": "default" },
            "match_content": {
                "DO NOT EDIT": "nonexistent_profile"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher.resolve_config(&file, &config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not defined in profiles")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_with_content_unreadable_file_fallback() {
        // File that doesn't exist → can't read → fallback to path match result.
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc",
                "ignore": null
            },
            "match": { "**": "default" },
            "match_content": {
                "DO NOT EDIT": "ignore"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // Non-existent file — path matches "default", content can't be read → keeps "default".
        let result = matcher
            .resolve_config(std::path::Path::new("/nonexistent/file.go"), &config)
            .unwrap();
        assert_eq!(
            result,
            Some(ProfileResolution::Config(PathBuf::from(
                "/config/default.jsonc"
            )))
        );
    }

    #[test]
    fn test_content_match_scans_entire_file() {
        let dir = std::env::temp_dir().join("dprintx-test-full-scan");
        let _ = std::fs::create_dir_all(&dir);

        // Marker is far beyond the first block (~20KB into the file).
        let file = dir.join("late_marker.go");
        let mut content = String::new();
        for i in 0..1500 {
            content.push_str(&format!("// line {i:04} padding padding padding\n"));
        }
        content.push_str("// DO NOT EDIT\n");
        content.push_str("package main\n");
        assert!(content.len() > 40_000); // Ensure it spans multiple 8KB blocks.

        std::fs::write(&file, &content).unwrap();

        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc",
                "ignore": null
            },
            "match": { "**": "default" },
            "match_content": {
                "DO NOT EDIT": "ignore"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // Marker deep in the file → still matched (full file scan).
        let result = matcher.resolve_config(&file, &config).unwrap();
        assert_eq!(result, Some(ProfileResolution::Ignore));

        // File without marker → keeps path result.
        let file2 = dir.join("normal.go");
        std::fs::write(&file2, "package main\n\nfunc main() {}\n").unwrap();

        let result = matcher.resolve_config(&file2, &config).unwrap();
        assert_eq!(
            result,
            Some(ProfileResolution::Config(PathBuf::from(
                "/config/default.jsonc"
            )))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_content_match_caret_multiline_mode() {
        let dir = std::env::temp_dir().join("dprintx-test-caret-ml");
        let _ = std::fs::create_dir_all(&dir);

        // ^ should match at start of any line (multi_line mode), not just start of file.
        let file = dir.join("caret.go");
        std::fs::write(
            &file,
            "package main\n\nimport \"fmt\"\n// Code generated by tool. DO NOT EDIT\n",
        )
        .unwrap();

        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc",
                "ignore": null
            },
            "match": { "**": "default" },
            "match_content": {
                "^// Code generated .+ DO NOT EDIT": "ignore"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // ^ matches at line start, not just file start → found on line 4.
        let result = matcher.resolve_config(&file, &config).unwrap();
        assert_eq!(result, Some(ProfileResolution::Ignore));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_with_content_first_match_wins_files() {
        let dir = std::env::temp_dir().join("dprintx-test-content-fmw");
        let _ = std::fs::create_dir_all(&dir);

        let file = dir.join("both.go");
        std::fs::write(&file, "// DO NOT EDIT\n// @format:strict\npackage main\n").unwrap();

        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "default": "/config/default.jsonc",
                "ignore": null,
                "strict": "/config/strict.jsonc"
            },
            "match": { "**": "default" },
            "match_content": {
                "DO NOT EDIT": "ignore",
                "@format:strict": "strict"
            }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        // Both patterns match, but "ignore" is first in config order → wins.
        let result = matcher.resolve_config(&file, &config).unwrap();
        assert_eq!(result, Some(ProfileResolution::Ignore));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_without_content_matcher() {
        // No match_content in config → content matching not applied.
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": { "default": "/config/default.jsonc" },
            "match": { "**": "default" }
        }"#;
        let config: DprintxConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher
            .resolve_config(Path::new("/any/file.go"), &config)
            .unwrap();
        assert_eq!(
            result,
            Some(ProfileResolution::Config(PathBuf::from(
                "/config/default.jsonc"
            )))
        );
    }
}
