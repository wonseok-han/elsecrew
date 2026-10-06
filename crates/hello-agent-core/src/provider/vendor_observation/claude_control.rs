//! Model-free control initialization. No prompt, launch, or permission authority.
//! Wire source: anthropics/claude-agent-sdk-python, _internal/query.py (2026-10-06).
//! System init and launch-policy observations remain separate from this handshake.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::{json, value::RawValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    New,
    AwaitingResponse,
    Ready,
    Failed,
    Closed,
}

pub struct InitializeExchange {
    request_id: String,
    state: State,
    models: Vec<ModelEntry>,
    hooks_applied: bool,
    default_permission_mode: bool,
}

#[derive(Deserialize)]
struct ModelEntry {
    value: String,
    #[serde(rename = "resolvedModel")]
    resolved_model: Option<String>,
    #[serde(rename = "supportsEffort")]
    supports_effort: Option<bool>,
    #[serde(rename = "supportedEffortLevels")]
    efforts: Option<Vec<String>>,
}

/// Only fixed initialization can construct this wire frame. No caller command payload.
pub struct InitializeRequest(String);
impl InitializeRequest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn invalid() -> ProviderError {
    ProviderError("Claude control initialization did not match".into())
}

impl InitializeExchange {
    pub fn new(request_id: &str) -> Result<Self, ProviderError> {
        if request_id.is_empty()
            || request_id.len() > 80
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(invalid());
        }
        Ok(Self {
            request_id: request_id.into(),
            state: State::New,
            models: Vec::new(),
            hooks_applied: false,
            default_permission_mode: false,
        })
    }
    pub fn state(&self) -> State {
        self.state
    }

    pub fn request(&mut self) -> Result<InitializeRequest, ProviderError> {
        if self.state != State::New {
            return Err(invalid());
        }
        self.state = State::AwaitingResponse;
        // SDK callback only, never an on-disk command hook. Runtime admission
        // must establish callback failure/timeout semantics before activation.
        let frame = json!({"type":"control_request","request_id":self.request_id,
            "request":{"subtype":"initialize","hooks":{"PreToolUse":[{
                "matcher":null,"hookCallbackIds":[super::claude_approval::PRE_TOOL_CALLBACK]
            }]},"skills":[]}});
        Ok(InitializeRequest(format!("{frame}\n")))
    }

    /// The transport owner routes non-response frames separately and enforces
    /// deadlines/output budgets. This method accepts exactly the pending response.
    pub fn accept_response(&mut self, input: &BoundedObservation) -> Result<(), ProviderError> {
        if self.state != State::AwaitingResponse {
            self.state = State::Failed;
            return Err(invalid());
        }
        let result = self.accept_inner(input);
        self.state = if result.is_ok() {
            State::Ready
        } else {
            State::Failed
        };
        result
    }

    fn accept_inner(&mut self, input: &BoundedObservation) -> Result<(), ProviderError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            #[serde(rename = "type")]
            kind: String,
            response: Response,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Response {
            subtype: String,
            request_id: String,
            response: Box<RawValue>,
        }
        let wire: Envelope = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if wire.kind != "control_response"
            || wire.response.subtype != "success"
            || wire.response.request_id != self.request_id
            || !serde_json::from_str::<serde_json::Value>(wire.response.response.get())
                .map_err(|_| invalid())?
                .is_object()
        {
            return Err(invalid());
        }
        #[derive(Deserialize)]
        struct Capabilities {
            #[serde(default)]
            models: Vec<ModelEntry>,
            hooks_applied: Option<bool>,
            current_permission_mode: Option<String>,
        }
        let capabilities: Capabilities = serde_json::from_str(wire.response.response.get()).map_err(|_| invalid())?;
        let valid_model = |s: &str| !s.is_empty() && s.len() <= 128 && !s.starts_with('-')
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.'));
        let mut names = std::collections::BTreeSet::new();
        if capabilities.models.len() > 128 || capabilities.models.iter().any(|entry| {
            !valid_model(&entry.value) || !names.insert(entry.value.as_str())
                || entry.resolved_model.as_deref().is_some_and(|s| !valid_model(s))
                || entry.efforts.as_ref().is_some_and(|levels| levels.len() > 5
                    || levels.iter().any(|s| !matches!(s.as_str(), "low"|"medium"|"high"|"xhigh"|"max"))
                    || levels.iter().collect::<std::collections::BTreeSet<_>>().len() != levels.len())
        }) { return Err(invalid()); }
        self.models = capabilities.models;
        self.hooks_applied = capabilities.hooks_applied == Some(true);
        self.default_permission_mode = capabilities.current_permission_mode.as_deref() == Some("default");
        // Keep only bounded capability/mode evidence, never account or raw response data.
        Ok(())
    }

    /// Provider-reported catalog and mode, not account entitlement or confinement.
    pub fn profile_model(&self, model: &str, effort: &str) -> Result<&str, ProviderError> {
        if self.state != State::Ready || !self.hooks_applied || !self.default_permission_mode { return Err(invalid()); }
        let entry = self.models.iter().find(|entry| entry.value == model).ok_or_else(invalid)?;
        if entry.supports_effort != Some(true) || !entry.efforts.as_ref().is_some_and(|levels| levels.iter().any(|level| level == effort)) {
            return Err(invalid());
        }
        Ok(entry.resolved_model.as_deref().unwrap_or(&entry.value))
    }

    pub fn close(&mut self) {
        self.state = State::Closed;
        self.models.clear();
        self.hooks_applied = false;
        self.default_permission_mode = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response() -> serde_json::Value {
        json!({"type":"control_response","response":{"subtype":"success","request_id":"init_a","response":{"account":{"private":"SECRET"}}}})
    }
    fn input(value: &serde_json::Value) -> BoundedObservation {
        BoundedObservation::new(value.to_string().into_bytes()).unwrap()
    }
    #[test]
    fn profile_requires_reported_model_effort_and_registered_hooks() {
        let base = json!({"type":"control_response","response":{"subtype":"success","request_id":"init_a","response":{
            "hooks_applied":true,"current_permission_mode":"default","models":[{"value":"opus","resolvedModel":"claude-opus-test",
                "supportsEffort":true,"supportedEffortLevels":["low","high"]}],"account":{"private":"SECRET"}
        }}});
        let mut exchange = InitializeExchange::new("init_a").unwrap();
        exchange.request().unwrap();
        exchange.accept_response(&input(&base)).unwrap();
        assert_eq!(exchange.profile_model("opus", "high").unwrap(), "claude-opus-test");
        assert!(exchange.profile_model("opus", "max").is_err());
        assert!(exchange.profile_model("other", "high").is_err());
        exchange.close();
        assert!(exchange.profile_model("opus", "high").is_err());
        for field in ["hooks_applied", "models", "current_permission_mode"] {
            let mut wire = base.clone();
            wire["response"]["response"].as_object_mut().unwrap().remove(field);
            let mut exchange = InitializeExchange::new("init_a").unwrap();
            exchange.request().unwrap();
            exchange.accept_response(&input(&wire)).unwrap();
            assert!(exchange.profile_model("opus", "high").is_err());
        }
        for mode in ["bypassPermissions", "acceptEdits", "auto", "plan", "unknown"] {
            let mut wire = base.clone();
            wire["response"]["response"]["current_permission_mode"] = json!(mode);
            let mut exchange = InitializeExchange::new("init_a").unwrap();
            exchange.request().unwrap();
            exchange.accept_response(&input(&wire)).unwrap();
            assert!(exchange.profile_model("opus", "high").is_err());
        }
        let mut duplicate = base.clone();
        let entry = duplicate["response"]["response"]["models"][0].clone();
        duplicate["response"]["response"]["models"].as_array_mut().unwrap().push(entry);
        let mut exchange = InitializeExchange::new("init_a").unwrap();
        exchange.request().unwrap();
        assert!(exchange.accept_response(&input(&duplicate)).is_err());
        assert!(exchange.profile_model("opus", "high").is_err());
    }
    #[test]
    fn initializes_once_without_a_user_prompt_and_cannot_reuse_a_closed_connection() {
        let mut exchange = InitializeExchange::new("init_a").unwrap();
        let frame = exchange.request().unwrap();
        assert_eq!(frame.as_str().lines().count(), 1);
        let wire: serde_json::Value = serde_json::from_str(frame.as_str()).unwrap();
        assert_eq!(
            wire,
            json!({"type":"control_request","request_id":"init_a","request":{"subtype":"initialize","hooks":{"PreToolUse":[{"matcher":null,"hookCallbackIds":[super::super::claude_approval::PRE_TOOL_CALLBACK]}]},"skills":[]}})
        );
        assert!(exchange.request().is_err());
        exchange.accept_response(&input(&response())).unwrap();
        assert_eq!(exchange.state(), State::Ready);
        exchange.close();
        assert!(exchange.request().is_err());
        assert!(exchange.accept_response(&input(&response())).is_err());
    }
    #[test]
    fn wrong_correlation_errors_and_malformed_envelopes_fail_without_private_details() {
        for case in ["id", "error", "missing", "type", "duplicate", "unsolicited"] {
            let mut wire = response();
            match case {
                "id" => wire["response"]["request_id"] = json!("other"),
                "error" => {
                    wire["response"]["subtype"] = json!("error");
                    wire["response"]["error"] = json!("SECRET");
                }
                "missing" => {
                    wire["response"].as_object_mut().unwrap().remove("response");
                }
                "type" => wire["type"] = json!("control_request"),
                _ => {}
            }
            let mut exchange = InitializeExchange::new("init_a").unwrap();
            if case != "unsolicited" {
                exchange.request().unwrap();
            }
            let observation = if case == "duplicate" {
                BoundedObservation::new(
                    wire.to_string()
                        .replacen(
                            "\"request_id\":",
                            "\"request_id\":\"other\",\"request_id\":",
                            1,
                        )
                        .into_bytes(),
                )
                .unwrap()
            } else {
                input(&wire)
            };
            let error = exchange.accept_response(&observation).unwrap_err();
            assert!(!error.to_string().contains("SECRET"));
            assert_eq!(exchange.state(), State::Failed);
            assert!(exchange.accept_response(&input(&response())).is_err());
        }
    }
}
