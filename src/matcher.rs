use anyhow::{Context, Result};
use globset::{Glob, GlobMatcher};
use std::path::Path;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_first_wins() {
        let config_json = r#"{
            "dprint": "/usr/bin/dprint",
            "profiles": {
                "strict": "/config/strict.jsonc",
                "default": "/config/default.jsonc"
            },
            "match": {
                "**/noc/cmdb/**": "strict",
                "**": "default"
            },
            "fallback": "/config/default.jsonc"
        }"#;

        let config: MconfConfig = serde_json::from_str(config_json).unwrap();
        let matcher = ProfileMatcher::from_config(&config).unwrap();

        assert_eq!(
            matcher.match_profile(Path::new("/home/user/workspace/noc/cmdb/main.go")),
            Some("strict")
        );
        assert_eq!(
            matcher.match_profile(Path::new("/home/user/other/file.go")),
            Some("default")
        );
    }
}
