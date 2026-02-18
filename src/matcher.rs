use anyhow::{bail, Context, Result};
use globset::{Glob, GlobMatcher};
use std::path::{Path, PathBuf};

use crate::config::MconfConfig;

/// A compiled match rule: glob matcher + profile name.
struct Rule {
    matcher: GlobMatcher,
    profile: String,
}

/// Matches file paths to profiles using ordered glob rules.
pub struct ProfileMatcher {
    rules: Vec<Rule>,
}

impl ProfileMatcher {
    /// Build a matcher from config match rules.
    pub fn from_config(config: &MconfConfig) -> Result<Self> {
        let mut rules = Vec::new();

        for (pattern, profile) in config.match_rules_iter() {
            let glob =
                Glob::new(pattern).with_context(|| format!("invalid glob pattern: {pattern}"))?;
            rules.push(Rule {
                matcher: glob.compile_matcher(),
                profile: profile.to_string(),
            });
        }

        Ok(Self { rules })
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

    /// Resolve file path to dprint config path.
    ///
    /// Logic:
    /// 1. Match file against rules → get profile name → resolve to config path
    /// 2. If no rule matches → use fallback config
    pub fn resolve_config(&self, file_path: &Path, config: &MconfConfig) -> Result<PathBuf> {
        if let Some(profile_name) = self.match_profile(file_path) {
            if let Some(config_path) = config.profile_config_path(profile_name) {
                return Ok(config_path);
            }
            bail!(
                "profile '{}' referenced in match rules but not defined in profiles",
                profile_name
            );
        }
        Ok(config.fallback_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MconfConfig {
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
            },
            "fallback": "/config/dprint-default.jsonc"
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
            "match": { "**/noc/cmdb/**": "strict" },
            "fallback": "/config/default.jsonc"
        }"#;
        let config: MconfConfig = serde_json::from_str(config_json).unwrap();
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
        assert_eq!(result, PathBuf::from("/config/dprint-maintainer.jsonc"));
    }

    #[test]
    fn test_resolve_config_fallback() {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": { "strict": "/config/strict.jsonc" },
            "match": { "**/noc/cmdb/**": "strict" },
            "fallback": "/config/default.jsonc"
        }"#;
        let config: MconfConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher
            .resolve_config(Path::new("/other/file.go"), &config)
            .unwrap();
        assert_eq!(result, PathBuf::from("/config/default.jsonc"));
    }

    #[test]
    fn test_resolve_config_unknown_profile_errors() {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {},
            "match": { "**": "nonexistent" },
            "fallback": "/config/default.jsonc"
        }"#;
        let config: MconfConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        let result = matcher.resolve_config(Path::new("/any/file.go"), &config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not defined in profiles"));
    }
}
