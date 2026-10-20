//! Bounded app-profile configuration parsing, not launch authorization.
//! Trust records are recognized separately so callers cannot mistake parsing
//! success for permission to load project-local configuration.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path};

pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_PROJECTS: usize = 128;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    cli_auth_credentials_store: String,
    #[serde(default)]
    projects: BTreeMap<String, ProjectTrust>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectTrust {
    trust_level: TrustLevel,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum TrustLevel {
    Trusted,
    Untrusted,
}

impl ProfileConfig {
    /// No credential contents, raw parser errors, or project paths escape on error.
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err("profile configuration exceeds size limit");
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "profile configuration is not UTF-8")?;
        let config: Self = toml::from_str(text).map_err(|_| "unsupported profile configuration")?;
        if config.cli_auth_credentials_store != "file" || config.projects.len() > MAX_PROJECTS {
            return Err("unsupported profile configuration");
        }
        for (path, project) in &config.projects {
            if path.len() > 4096
                || path.chars().any(char::is_control)
                || !Path::new(path).is_absolute()
                || path.split('/').any(|part| part == ".")
                || Path::new(path)
                    .components()
                    .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
            {
                return Err("unsupported profile project identity");
            }
            // Exhaustive typed validation; no arbitrary TOML subtree survives.
            match project.trust_level {
                TrustLevel::Trusted | TrustLevel::Untrusted => {}
            }
        }
        Ok(config)
    }

    /// Until a launcher proves project-layer isolation, only auth-only profiles
    /// may be admitted. Whitespace/comments are not an authorization boundary.
    pub fn is_auth_only(&self) -> bool {
        self.projects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_equivalent_auth_configuration_without_rewriting() {
        for input in [
            "cli_auth_credentials_store=\"file\"",
            "# vendor comment\r\ncli_auth_credentials_store = 'file'\r\n",
            "cli_auth_credentials_store = \"file\"\n[projects]\n",
        ] {
            assert!(ProfileConfig::parse(input.as_bytes())
                .unwrap()
                .is_auth_only());
        }
    }

    #[test]
    fn recognizes_trust_but_does_not_admit_it_as_auth_only() {
        for level in ["trusted", "untrusted"] {
            let input = format!(
                "cli_auth_credentials_store='file'\n[projects.'/snapshot']\ntrust_level='{level}'"
            );
            assert!(!ProfileConfig::parse(input.as_bytes())
                .unwrap()
                .is_auth_only());
        }
    }

    #[test]
    fn rejects_unknown_settings_malformed_trust_and_budget_excess() {
        for suffix in [
            "\nnotify=['command']",
            "\n[hooks]",
            "\ncli_auth_credentials_store='file'",
            "\n[projects.'/snapshot']\ntrust_level='trusted'\nextra=true",
            "\n[projects.'/snapshot']\ntrust_level='other'",
            "\n[projects.'/snapshot']",
            "\n[projects.'relative']\ntrust_level='trusted'",
            "\n[projects.'/a/../b']\ntrust_level='trusted'",
            "\n[projects.'/a/./b']\ntrust_level='trusted'",
        ] {
            let text = format!("cli_auth_credentials_store='file'{suffix}");
            assert!(ProfileConfig::parse(text.as_bytes()).is_err(), "{suffix}");
        }
        assert!(ProfileConfig::parse(b"cli_auth_credentials_store='keyring'").is_err());
        assert!(ProfileConfig::parse(&vec![b' '; MAX_CONFIG_BYTES + 1]).is_err());
        assert!(ProfileConfig::parse(&[0xff]).is_err());
        let mut text = "cli_auth_credentials_store='file'\n".to_owned();
        for index in 0..=MAX_PROJECTS {
            text.push_str(&format!("[projects.'/p{index}']\ntrust_level='trusted'\n"));
        }
        assert!(ProfileConfig::parse(text.as_bytes()).is_err());
    }
}
