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
    Usage,
    Requirements,
    PermissionProfiles,
}

#[derive(Debug, Default)]
pub struct ModelFreeExchange {
    next: usize,
    usage_pending: bool,
    pending: bool,
    failed: bool,
    include_config_layers: bool,
    experimental_permissions: bool,
    permission_cwd: Option<String>,
}

fn error() -> ProviderError {
    ProviderError("invalid or unexpected Codex RPC response".into())
}

impl ModelFreeExchange {
    pub fn request_usage_before_models(&mut self) { self.usage_pending = true; }

    /// Fixed diagnostic label only; never exposes an observed account/config value.
    pub(super) fn diagnostic_stage(&self) -> &'static str {
        if self.next == 3 && self.usage_pending { return "account/rateLimits/read"; }
        match self.next {
            0 => "initialize",
            1 => "config/read",
            2 => "account/read",
            3 => "model/list",
            4 => "configRequirements/read",
            5 => "permissionProfile/list",
            _ => "complete",
        }
    }
    pub(super) fn awaiting_reply(&self) -> bool {
        self.pending && !self.failed
    }

    pub(super) fn awaiting_initialized_reply(&self) -> bool {
        self.awaiting_reply() && self.next > 0
    }

    pub(super) fn awaiting_configuration_reply(&self) -> bool {
        self.awaiting_reply() && self.next == 1
    }
    pub fn with_config_layers() -> Self {
        Self {
            include_config_layers: true,
            ..Self::default()
        }
    }
    /// Explicit beta handshake for the named-permission adapter. This does not
    /// validate permission profiles or grant thread/turn authority.
    pub fn with_named_permission_config(cwd: &std::path::Path) -> Result<Self, ProviderError> {
        let text = cwd.to_str().ok_or_else(error)?;
        if !cwd.is_absolute()
            || text.len() > 16 * 1024
            || text.chars().any(char::is_control)
            || cwd
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(error());
        }
        Ok(Self {
            include_config_layers: true,
            experimental_permissions: true,
            permission_cwd: Some(text.into()),
            ..Self::default()
        })
    }
    // Keep the existing thread/turn IDs 5/6 unchanged on the same connection.
    fn request_id(&self) -> u64 {
        if self.next == 3 && self.usage_pending { return 52; }
        if self.next >= 4 {
            50 + (self.next - 4) as u64
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
                serde_json::json!({"clientInfo":{"name":"elsecrew","version":"0.1.0"}}),
            ),
            1 => (
                "config/read",
                serde_json::json!({"includeLayers":self.include_config_layers}),
            ),
            2 => ("account/read", serde_json::json!({"refreshToken":false})),
            3 if self.usage_pending => ("account/rateLimits/read", serde_json::json!({})),
            3 => (
                "model/list",
                serde_json::json!({"limit":256,"includeHidden":false}),
            ),
            4 if self.include_config_layers => ("configRequirements/read", serde_json::json!({})),
            5 if self.permission_cwd.is_some() => (
                "permissionProfile/list",
                serde_json::json!({"cwd":self.permission_cwd,"limit":256}),
            ),
            _ => return Ok(None),
        };
        if self.next == 1 {
            if let Some(cwd) = &self.permission_cwd {
                params["cwd"] = serde_json::json!(cwd);
            }
        }
        if self.next == 0 && self.include_config_layers {
            // The pinned native CLI emits this unrelated startup notification.
            // Suppress only this method; unexpected frames still fail closed.
            params["capabilities"] = serde_json::json!({
                "experimentalApi": self.experimental_permissions,
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
        // Validate the entire raw tree before typed parsing or Value projection
        // can erase duplicate keys in nested permission/config objects.
        let value = input.json().map_err(|_| error())?;
        // Unsupported display-only quota reads must not disable the model catalog.
        if self.next == 3 && self.usage_pending && value.get("id").and_then(serde_json::Value::as_u64) == Some(52)
            && value.get("error").is_some_and(serde_json::Value::is_object) && value.get("result").is_none() {
            self.usage_pending = false; self.pending = false;
            return Ok((ObservationKind::Usage, BoundedObservation::new(b"{}".to_vec())?));
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
            3 if self.usage_pending => ObservationKind::Usage,
            3 => ObservationKind::Models,
            4 if self.include_config_layers => ObservationKind::Requirements,
            5 if self.permission_cwd.is_some() => ObservationKind::PermissionProfiles,
            _ => return Err(error()),
        };
        if kind == ObservationKind::Usage { self.usage_pending = false; } else { self.next += 1; }
        self.pending = false;
        Ok((kind, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_usage_read_has_fixed_id_and_failure_preserves_model_catalog() {
        for unsupported in [false, true] {
            let mut flow = ModelFreeExchange::with_config_layers();
            flow.request_usage_before_models();
            for id in [1, 2, 3, 52, 4, 50] {
                let text = flow.next_request().unwrap().unwrap();
                let request: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
                assert_eq!(request["id"], id);
                if id == 52 { assert_eq!(request["method"], "account/rateLimits/read"); }
                let value = if id == 52 && unsupported { serde_json::json!({"id":id,"error":{"code":-32601,"message":"unsupported"}}) }
                    else { serde_json::json!({"id":id,"result":{"userAgent":"test"}}) };
                let (kind, _) = flow.accept(&BoundedObservation::new(serde_json::to_vec(&value).unwrap()).unwrap()).unwrap();
                if id == 52 { assert_eq!(kind, ObservationKind::Usage); }
                if id == 4 { assert_eq!(kind, ObservationKind::Models); }
            }
            assert!(flow.next_request().unwrap().is_none());
        }
    }
    #[test]
    fn named_permissions_opt_in_does_not_change_stable_handshake() {
        let mut named = ModelFreeExchange::with_named_permission_config(std::path::Path::new(
            test_path!("/snapshot"),
        ))
        .unwrap();
        let beta: serde_json::Value =
            serde_json::from_str(&named.next_request().unwrap().unwrap()).unwrap();
        let stable: serde_json::Value = serde_json::from_str(
            &ModelFreeExchange::with_config_layers()
                .next_request()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(beta["params"]["capabilities"]["experimentalApi"], true);
        assert_eq!(stable["params"]["capabilities"]["experimentalApi"], false);
        assert!(named.next_request().is_err());
    }

    #[test]
    fn named_profile_query_binds_cwd_without_reusing_thread_ids() {
        let mut exchange = ModelFreeExchange::with_named_permission_config(std::path::Path::new(
            test_path!("/snapshot"),
        ))
        .unwrap();
        for id in [1, 2, 3, 4, 50, 51] {
            let request = exchange.next_request().unwrap().unwrap();
            let wire: serde_json::Value =
                serde_json::from_str(request.lines().last().unwrap()).unwrap();
            assert_eq!(wire["id"], id);
            assert_eq!(wire["method"], exchange.diagnostic_stage());
            if id == 2 {
                assert_eq!(
                    wire["params"],
                    serde_json::json!({"cwd":test_path!("/snapshot"),"includeLayers":true})
                );
            }
            if id == 51 {
                assert_eq!(wire["method"], "permissionProfile/list");
                assert_eq!(
                    wire["params"],
                    serde_json::json!({"cwd":test_path!("/snapshot"),"limit":256})
                );
            }
            let body = if id == 1 {
                r#"{"userAgent":"test"}"#
            } else {
                "{}"
            };
            let (kind, _) = exchange
                .accept(&input(&format!(r#"{{"id":{id},"result":{body}}}"#)))
                .unwrap();
            if id == 51 {
                assert_eq!(kind, ObservationKind::PermissionProfiles);
            }
        }
        assert!(exchange.next_request().unwrap().is_none());
        for cwd in [
            "relative",
            test_path!("/snapshot/../source"),
            test_path!("/snapshot\n"),
        ] {
            assert!(
                ModelFreeExchange::with_named_permission_config(std::path::Path::new(cwd)).is_err()
            );
        }
    }
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
    fn nested_duplicates_fail_before_value_projection() {
        let mut exchange = ModelFreeExchange::default();
        exchange.next_request().unwrap();
        exchange
            .accept(&input(r#"{"id":1,"result":{"userAgent":"test"}}"#))
            .unwrap();
        exchange.next_request().unwrap();
        let error = exchange
            .accept(&input(
                r#"{"id":2,"result":{"config":{"model":"SECRET","model":"test"}}}"#,
            ))
            .unwrap_err();
        assert!(!format!("{error:?} {exchange:?}").contains("SECRET"));
        assert!(exchange.next_request().is_err());
    }
}
