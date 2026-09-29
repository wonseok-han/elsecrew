//! Single-flight, model-free Codex app-server exchange. Not a launch/permission gate.
//! No arbitrary methods, thread creation, authentication mutation or user input are expressible.
//! An owner must still enforce process isolation/timeouts and validate each returned observation.

use super::super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::value::RawValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationKind {
    Initialize,
    Config,
    Account,
    Models,
    Requirements,
}

#[derive(Debug, Default)]
pub struct ModelFreeExchange {
    next: usize,
    pending: bool,
    failed: bool,
    include_config_layers: bool,
}

fn error() -> ProviderError {
    ProviderError("invalid or unexpected Codex RPC response".into())
}

impl ModelFreeExchange {
    pub(super) fn awaiting_reply(&self) -> bool {
        self.pending && !self.failed
    }

    pub(super) fn awaiting_account_reply(&self) -> bool {
        self.awaiting_reply() && self.next == 2
    }
    pub fn with_config_layers() -> Self {
        Self {
            include_config_layers: true,
            ..Self::default()
        }
    }
    // Keep the existing thread/turn IDs 5/6 unchanged on the same connection.
    fn request_id(&self) -> u64 {
        if self.next == 4 {
            50
        } else {
            self.next as u64 + 1
        }
    }

    /// Returns one fixed request. Layered preflight adds a fifth requirements read.
    /// The second request includes the required initialized notification first.
    pub fn next_request(&mut self) -> Result<Option<String>, ProviderError> {
        if self.failed || self.pending {
            return Err(error());
        }
        let (method, mut params) = match self.next {
            0 => (
                "initialize",
                serde_json::json!({"clientInfo":{"name":"hello_agent","version":"0.1.0"}}),
            ),
            1 => (
                "config/read",
                serde_json::json!({"includeLayers":self.include_config_layers}),
            ),
            2 => ("account/read", serde_json::json!({"refreshToken":false})),
            3 => (
                "model/list",
                serde_json::json!({"limit":256,"includeHidden":false}),
            ),
            4 if self.include_config_layers => ("configRequirements/read", serde_json::json!({})),
            _ => return Ok(None),
        };
        if self.next == 0 && self.include_config_layers {
            // The pinned native CLI emits this unrelated startup notification.
            // Suppress only this method; unexpected frames still fail closed.
            params["capabilities"] = serde_json::json!({
                "experimentalApi": false,
                "requestAttestation": false,
                "optOutNotificationMethods": ["remoteControl/status/changed"]
            });
        }
        self.pending = true;
        let prefix = if self.next == 1 {
            "{\"method\":\"initialized\",\"params\":{}}\n"
        } else {
            ""
        };
        Ok(Some(format!(
            "{prefix}{}\n",
            serde_json::json!({"id":self.request_id(),"method":method,"params":params})
        )))
    }

    /// The raw result is preserved so the typed vendor parsers can reject duplicate fields.
    /// Any unexpected frame permanently fails this exchange; restarting requires a new owner.
    pub fn accept(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<(ObservationKind, BoundedObservation), ProviderError> {
        let result = self.accept_inner(input);
        if result.is_err() {
            self.failed = true;
            self.pending = false;
        }
        result
    }

    fn accept_inner(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<(ObservationKind, BoundedObservation), ProviderError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            id: u64,
            result: Box<RawValue>,
        }
        if self.failed || !self.pending {
            return Err(error());
        }
        let reply: Reply = serde_json::from_slice(&input.bytes).map_err(|_| error())?;
        if reply.id != self.request_id() || !reply.result.get().trim_start().starts_with('{') {
            return Err(error());
        }
        let result =
            BoundedObservation::new(reply.result.get().as_bytes().to_vec()).map_err(|_| error())?;
        let kind = match self.next {
            0 => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Init {
                    user_agent: String,
                }
                let init: Init = serde_json::from_slice(&result.bytes).map_err(|_| error())?;
                if init.user_agent.is_empty() || init.user_agent.len() > 1024 {
                    return Err(error());
                }
                ObservationKind::Initialize
            }
            1 => ObservationKind::Config,
            2 => ObservationKind::Account,
            3 => ObservationKind::Models,
            4 if self.include_config_layers => ObservationKind::Requirements,
            _ => return Err(error()),
        };
        self.next += 1;
        self.pending = false;
        Ok((kind, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(text: &str) -> BoundedObservation {
        BoundedObservation::new(text.as_bytes().to_vec()).unwrap()
    }
    #[test]
    fn layered_probe_suppresses_only_unrelated_remote_status() {
        let request: serde_json::Value = serde_json::from_str(
            &ModelFreeExchange::with_config_layers()
                .next_request()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            request["params"]["capabilities"],
            serde_json::json!({
                "experimentalApi":false,"requestAttestation":false,
                "optOutNotificationMethods":["remoteControl/status/changed"]
            })
        );
        let original: serde_json::Value = serde_json::from_str(
            &ModelFreeExchange::default()
                .next_request()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(original["params"].get("capabilities").is_none());
        let mut exchange = ModelFreeExchange::with_config_layers();
        exchange.next_request().unwrap();
        assert!(exchange
            .accept(&input(
                r#"{"method":"remoteControl/status/changed","params":{}}"#
            ))
            .is_err());
        assert!(exchange.next_request().is_err());
    }
    #[test]
    fn request_order_never_creates_a_thread_or_sends_a_prompt() {
        let mut exchange = ModelFreeExchange::default();
        for (index, (method, kind)) in [
            ("initialize", ObservationKind::Initialize),
            ("config/read", ObservationKind::Config),
            ("account/read", ObservationKind::Account),
            ("model/list", ObservationKind::Models),
        ]
        .into_iter()
        .enumerate()
        {
            let request = exchange.next_request().unwrap().unwrap();
            let lines: Vec<serde_json::Value> = request
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(lines.len(), if index == 1 { 2 } else { 1 });
            if index == 1 {
                assert_eq!(
                    lines[0],
                    serde_json::json!({"method":"initialized","params":{}})
                );
            }
            assert_eq!(lines.last().unwrap()["method"], method);
            assert_eq!(lines.last().unwrap()["id"], index + 1);
            if index == 2 {
                assert_eq!(lines[0]["params"]["refreshToken"], false);
            }
            assert!(exchange.next_request().is_err());
            let result = if index == 0 {
                r#"{"userAgent":"test"}"#
            } else {
                "{}"
            };
            let reply = format!(r#"{{"id":{},"result":{result}}}"#, index + 1);
            assert_eq!(exchange.accept(&input(&reply)).unwrap().0, kind);
        }
        assert!(exchange.next_request().unwrap().is_none());
    }

    #[test]
    fn unsolicited_wrong_duplicate_and_error_frames_fail_permanently() {
        for wire in [
            r#"{"id":2,"result":{}}"#,
            r#"{"id":1,"id":1,"result":{}}"#,
            r#"{"id":1,"result":{},"error":{"message":"SECRET"}}"#,
            r#"{"method":"account/updated","params":{}}"#,
            r#"{"id":1,"result":null}"#,
            r#"{"id":1,"result":{"userAgent":""}}"#,
        ] {
            let mut exchange = ModelFreeExchange::default();
            exchange.next_request().unwrap();
            let error = exchange.accept(&input(wire)).unwrap_err();
            assert!(!format!("{error:?} {exchange:?}").contains("SECRET"));
            assert!(exchange.next_request().is_err());
        }
        let mut exchange = ModelFreeExchange::default();
        assert!(exchange
            .accept(&input(r#"{"id":1,"result":{"userAgent":"test"}}"#))
            .is_err());
    }

    #[test]
    fn nested_duplicates_survive_for_the_vendor_validator() {
        let mut exchange = ModelFreeExchange::default();
        exchange.next_request().unwrap();
        exchange
            .accept(&input(r#"{"id":1,"result":{"userAgent":"test"}}"#))
            .unwrap();
        exchange.next_request().unwrap();
        let (_, result) = exchange
            .accept(&input(
                r#"{"id":2,"result":{"config":{"model":"SECRET","model":"test"}}}"#,
            ))
            .unwrap();
        assert_eq!(
            std::str::from_utf8(&result.bytes).unwrap(),
            r#"{"config":{"model":"SECRET","model":"test"}}"#
        );
        assert!(!format!("{result:?} {exchange:?}").contains("SECRET"));
        let profile = super::super::codex_preflight::ExpectedProfile {
            model: "test",
            reasoning_effort: "medium",
        };
        assert!(super::super::codex_preflight::match_read_only_config(&result, &profile).is_err());
    }
}
