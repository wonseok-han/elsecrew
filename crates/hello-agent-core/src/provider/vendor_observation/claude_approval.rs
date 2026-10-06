//! Claude SDK control permissions. Denials only until the runtime admits tool grants.
//! Wire: anthropics/claude-agent-sdk-python/_internal/query.py (2026-10-06).
//! One instance belongs to one connection. Raw tool inputs are never public output.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const PRE_TOOL_CALLBACK: &str = "hello_agent_pre_tool";

#[derive(Default)]
pub struct Permissions {
    seen: BTreeSet<String>,
    pending: BTreeMap<String, bool>, // true: PreToolUse, false: can_use_tool
    hook_scope: Option<(String, String)>,
    failed: bool,
}

/// Display metadata, not authorization. No raw input or permission suggestions.
pub struct Request {
    pub request_id: String,
    pub tool_name: String,
    pub tool_use_id: Option<String>,
}

pub enum Observation {
    Requested(Request),
    ToolCheck(Request),
    Cancelled,
}

pub struct Denial(String);
impl Denial {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn invalid() -> ProviderError {
    ProviderError("Claude permission request did not match".into())
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

impl Permissions {
    pub(super) fn bind_hook_scope(&mut self, session: &str, cwd: &str) {
        self.hook_scope = Some((session.into(), cwd.into()));
    }
    pub fn observe(&mut self, input: &BoundedObservation) -> Result<Observation, ProviderError> {
        let result = self.observe_inner(input);
        if result.is_err() {
            self.failed = true;
            self.pending.clear();
        }
        result
    }

    fn observe_inner(&mut self, input: &BoundedObservation) -> Result<Observation, ProviderError> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(rename = "type")]
            kind: String,
            request_id: String,
            request: Option<ToolRequest>,
        }
        #[derive(Deserialize)]
        struct ToolRequest {
            subtype: String,
            tool_name: Option<String>,
            input: Value,
            tool_use_id: Option<String>,
            callback_id: Option<String>,
        }
        if self.failed {
            return Err(invalid());
        }
        let wire: Envelope = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if !valid_id(&wire.request_id) {
            return Err(invalid());
        }
        if wire.kind == "control_cancel_request" {
            // Cancellation can race a completed reply. Never reply to cancellation.
            if wire.request.is_some() || !self.seen.contains(&wire.request_id) {
                return Err(invalid());
            }
            self.pending.remove(&wire.request_id);
            return Ok(Observation::Cancelled);
        }
        let request = wire.request.ok_or_else(invalid)?;
        let hook = request.subtype == "hook_callback";
        let (tool_name, tool_use_id) = if hook {
            #[derive(Deserialize)]
            struct HookInput {
                session_id: String,
                cwd: String,
                hook_event_name: String,
                tool_name: String,
                tool_use_id: String,
                tool_input: Value,
                agent_id: Option<String>,
            }
            let data: HookInput =
                serde_json::from_value(request.input.clone()).map_err(|_| invalid())?;
            let (session, cwd) = self.hook_scope.as_ref().ok_or_else(invalid)?;
            if request.callback_id.as_deref() != Some(PRE_TOOL_CALLBACK)
                || data.hook_event_name != "PreToolUse"
                || &data.session_id != session
                || &data.cwd != cwd
                || data.agent_id.is_some()
                || !data.tool_input.is_object()
                || request.tool_use_id.as_deref() != Some(data.tool_use_id.as_str())
            {
                return Err(invalid());
            }
            (data.tool_name, Some(data.tool_use_id))
        } else {
            (request.tool_name.ok_or_else(invalid)?, request.tool_use_id)
        };
        if wire.kind != "control_request"
            || (!hook && request.subtype != "can_use_tool")
            || !valid_id(&tool_name)
            || !request.input.is_object()
            || tool_use_id.as_deref().is_some_and(|id| !valid_id(id))
            || self.seen.len() >= 1024
            || !self.seen.insert(wire.request_id.clone())
        {
            return Err(invalid());
        }
        self.pending.insert(wire.request_id.clone(), hook);
        let request = Request {
            request_id: wire.request_id,
            tool_name,
            tool_use_id,
        };
        Ok(if hook {
            Observation::ToolCheck(request)
        } else {
            Observation::Requested(request)
        })
    }

    /// Consumes exactly one outstanding request. No permission update or allow path.
    pub fn deny(&mut self, request_id: &str, interrupt: bool) -> Result<Denial, ProviderError> {
        if self.failed {
            return Err(invalid());
        }
        let hook = self.pending.remove(request_id).ok_or_else(invalid)?;
        let response = if hook {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny",
                "permissionDecisionReason":"Tool execution has not passed the application boundary check."}})
        } else {
            json!({"behavior":"deny","message":"Tool request declined by the application user.","interrupt":interrupt})
        };
        let frame = json!({"type":"control_response","response":{
            "subtype":"success","request_id":request_id,
            "response":response
        }});
        Ok(Denial(format!("{frame}\n")))
    }

    pub fn close(&mut self) {
        self.failed = true;
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(value: Value) -> BoundedObservation {
        BoundedObservation::new(value.to_string().into_bytes()).unwrap()
    }
    fn request(id: &str) -> BoundedObservation {
        input(json!({"type":"control_request","request_id":id,"request":{
            "subtype":"can_use_tool","tool_name":"Bash","input":{"command":"SECRET"},"tool_use_id":"tool1"
        }}))
    }
    #[test]
    fn pre_tool_checks_require_bound_scope_and_use_hook_denials() {
        let hook = json!({"type":"control_request","request_id":"hook-r","request":{
            "subtype":"hook_callback","callback_id":PRE_TOOL_CALLBACK,"tool_use_id":"tool1",
            "input":{"session_id":"s","cwd":"/snapshot","hook_event_name":"PreToolUse",
                "tool_name":"Read","tool_use_id":"tool1","tool_input":{"file_path":"SECRET"}}
        }});
        assert!(Permissions::default()
            .observe(&input(hook.clone()))
            .is_err());
        let mut permissions = Permissions::default();
        permissions.bind_hook_scope("s", "/snapshot");
        assert!(matches!(
            permissions.observe(&input(hook.clone())).unwrap(),
            Observation::ToolCheck(_)
        ));
        let reply = permissions.deny("hook-r", false).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(
            wire["response"]["response"]["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert!(wire["response"]["response"].get("behavior").is_none());
        assert!(!reply.as_str().contains("SECRET"));
        for field in ["session_id", "cwd", "tool_use_id", "hook_event_name"] {
            let mut permissions = Permissions::default();
            permissions.bind_hook_scope("s", "/snapshot");
            let mut wrong = hook.clone();
            wrong["request"]["input"][field] = json!("other");
            assert!(permissions.observe(&input(wrong)).is_err());
            assert!(permissions.deny("hook-r", false).is_err());
        }
    }
    #[test]
    fn denial_is_correlated_single_use_and_does_not_echo_private_input() {
        let mut permissions = Permissions::default();
        permissions.observe(&request("a")).unwrap();
        permissions.observe(&request("b")).unwrap();
        assert!(permissions.deny("unknown", false).is_err());
        let reply = permissions.deny("a", false).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["request_id"], "a");
        assert_eq!(wire["response"]["response"]["behavior"], "deny");
        assert!(!reply.as_str().contains("SECRET"));
        assert!(permissions.deny("a", false).is_err());
        assert!(permissions.deny("b", true).is_ok());
    }
    #[test]
    fn cancel_close_and_duplicate_prevent_replies() {
        let mut permissions = Permissions::default();
        permissions.observe(&request("a")).unwrap();
        permissions
            .observe(&input(
                json!({"type":"control_cancel_request","request_id":"a"}),
            ))
            .unwrap();
        assert!(permissions.deny("a", false).is_err());
        assert!(permissions.observe(&request("a")).is_err());
        assert!(permissions.observe(&request("b")).is_err());
        let mut permissions = Permissions::default();
        permissions.observe(&request("b")).unwrap();
        permissions.close();
        assert!(permissions.deny("b", false).is_err());
    }
}
