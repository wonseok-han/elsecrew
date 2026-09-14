//! Model-free vendor observations, never launch authority or fixture manifest evidence.
//! Inputs are transient; outputs contain no account identifiers, tokens or raw config.

use super::{BoundedObservation, ProviderError};
use serde::Deserialize;

pub mod codex_events;
pub mod codex_login;
pub mod codex_preflight;
pub mod codex_rpc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Codex01491,
    Claude21269,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthObservation {
    CredentialsPresent,
    NotAuthenticated,
}

impl Vendor {
    pub fn verify_version(self, input: &BoundedObservation) -> Result<(), ProviderError> {
        let expected = match self {
            Self::Codex01491 => "codex-cli 0.149.1",
            Self::Claude21269 => "2.1.269 (Claude Code)",
        };
        if input.text()? != expected {
            return Err(ProviderError(
                "unsupported vendor version observation".into(),
            ));
        }
        Ok(())
    }

    /// Codex input is the result of a correlated account/read(refreshToken:false) response.
    /// Claude input is auth status --json stdout with its observed exit code.
    /// Presence does not establish request validity, model access or config isolation.
    pub fn observe_auth(
        self,
        input: &BoundedObservation,
        exit_code: i32,
    ) -> Result<AuthObservation, ProviderError> {
        let malformed = || ProviderError("invalid vendor auth observation".into());
        let present = match self {
            Self::Codex01491 => {
                #[derive(Deserialize)]
                struct Account {
                    #[serde(rename = "type")]
                    kind: String,
                }
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct ResultWire {
                    account: Option<Account>,
                    requires_openai_auth: bool,
                }
                // Require the account key: absent is malformed, explicit null is logged out.
                let value = input.json()?;
                if value.get("account").is_none() || exit_code != 0 {
                    return Err(malformed());
                }
                let wire: ResultWire =
                    serde_json::from_slice(&input.bytes).map_err(|_| malformed())?;
                if !wire.requires_openai_auth {
                    return Err(malformed());
                }
                match wire.account {
                    None => false,
                    Some(account)
                        if matches!(
                            account.kind.as_str(),
                            "chatgpt" | "apiKey" | "chatgptAuthTokens"
                        ) =>
                    {
                        true
                    }
                    Some(_) => return Err(malformed()),
                }
            }
            Self::Claude21269 => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct AuthWire {
                    logged_in: bool,
                    auth_method: Option<String>,
                }
                let wire: AuthWire =
                    serde_json::from_slice(&input.bytes).map_err(|_| malformed())?;
                if wire.logged_in {
                    if exit_code != 0
                        || !matches!(wire.auth_method.as_deref(), Some("claude.ai" | "api_key"))
                    {
                        return Err(malformed());
                    }
                } else if exit_code != 1 {
                    return Err(malformed());
                }
                wire.logged_in
            }
        };
        Ok(if present {
            AuthObservation::CredentialsPresent
        } else {
            AuthObservation::NotAuthenticated
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(value: &str) -> BoundedObservation {
        BoundedObservation::new(value.as_bytes().to_vec()).unwrap()
    }

    #[test]
    fn exact_versions_do_not_accept_fake_versions_or_other_vendors() {
        for (vendor, version) in [
            (Vendor::Codex01491, "codex-cli 0.149.1"),
            (Vendor::Claude21269, "2.1.269 (Claude Code)"),
        ] {
            assert!(vendor.verify_version(&observation(version)).is_ok());
            for invalid in [
                "codex-cli 0.150.0",
                "Claude Code 2.1.241",
                "0.149.1",
                "unknown",
            ] {
                assert!(vendor.verify_version(&observation(invalid)).is_err());
            }
        }
    }

    #[test]
    fn authentication_output_is_redacted_and_not_execution_authority() {
        let codex = observation(
            r#"{"account":{"type":"chatgpt","email":"PRIVATE","token":"SECRET"},"requiresOpenaiAuth":true}"#,
        );
        let claude = observation(
            r#"{"loggedIn":true,"authMethod":"claude.ai","email":"PRIVATE","token":"SECRET"}"#,
        );
        for (vendor, input) in [(Vendor::Codex01491, codex), (Vendor::Claude21269, claude)] {
            let result = vendor.observe_auth(&input, 0).unwrap();
            assert_eq!(result, AuthObservation::CredentialsPresent);
            assert!(!format!("{input:?} {result:?}").contains("SECRET"));
            assert!(!format!("{input:?} {result:?}").contains("PRIVATE"));
        }
    }

    #[test]
    fn absent_malformed_duplicate_and_cross_vendor_auth_are_rejected() {
        for wire in [
            r#"{}"#,
            r#"{"loggedIn":"yes"}"#,
            r#"{"loggedIn":false,"loggedIn":true,"authMethod":"claude.ai"}"#,
            "SECRET",
        ] {
            for vendor in [Vendor::Codex01491, Vendor::Claude21269] {
                let error = vendor.observe_auth(&observation(wire), 0).unwrap_err();
                assert!(!error.to_string().contains("SECRET"));
            }
        }
        assert!(Vendor::Codex01491
            .observe_auth(
                &observation(r#"{"loggedIn":true,"authMethod":"claude.ai"}"#),
                0
            )
            .is_err());
        assert!(Vendor::Claude21269
            .observe_auth(
                &observation(r#"{"account":null,"requiresOpenaiAuth":true}"#),
                0
            )
            .is_err());
    }

    #[test]
    fn logged_out_and_exit_mismatch_remain_distinct() {
        let codex = observation(r#"{"account":null,"requiresOpenaiAuth":true}"#);
        let claude = observation(r#"{"loggedIn":false}"#);
        assert_eq!(
            Vendor::Codex01491.observe_auth(&codex, 0).unwrap(),
            AuthObservation::NotAuthenticated
        );
        assert_eq!(
            Vendor::Claude21269.observe_auth(&claude, 1).unwrap(),
            AuthObservation::NotAuthenticated
        );
        assert!(Vendor::Codex01491.observe_auth(&codex, 1).is_err());
        assert!(Vendor::Claude21269.observe_auth(&claude, 0).is_err());
    }

    #[test]
    fn codex_duplicate_recognized_fields_are_rejected() {
        for wire in [
            r#"{"account":null,"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}"#,
            r#"{"account":null,"requiresOpenaiAuth":false,"requiresOpenaiAuth":true}"#,
            r#"{"account":{"type":"unknown","type":"chatgpt"},"requiresOpenaiAuth":true}"#,
        ] {
            assert!(Vendor::Codex01491
                .observe_auth(&observation(wire), 0)
                .is_err());
        }
    }

    #[test]
    fn observation_whitespace_and_auth_method_boundaries_are_explicit() {
        // CLI line framing may contain leading/trailing whitespace, not extra content.
        assert!(Vendor::Codex01491
            .verify_version(&observation(" \tcodex-cli 0.149.1\r\n"))
            .is_ok());
        assert!(Vendor::Codex01491
            .verify_version(&observation("codex-cli 0.149.1\nextra"))
            .is_err());
        assert_eq!(
            Vendor::Claude21269
                .observe_auth(&observation(r#"{"loggedIn":false,"authMethod":null}"#), 1)
                .unwrap(),
            AuthObservation::NotAuthenticated
        );
        for method in ["null", "\"unknown\""] {
            let wire = format!(r#"{{"loggedIn":true,"authMethod":{method}}}"#);
            assert!(Vendor::Claude21269
                .observe_auth(&observation(&wire), 0)
                .is_err());
        }
    }
}
