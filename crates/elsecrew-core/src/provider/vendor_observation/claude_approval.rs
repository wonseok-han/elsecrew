//! Claude SDK control permissions. Runtime admission is separate from wire replies.
//! Wire: anthropics/claude-agent-sdk-python/_internal/query.py (2026-10-06).
//! One instance belongs to one connection. Raw tool inputs are never public output.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const PRE_TOOL_CALLBACK: &str = "elsecrew_pre_tool";

#[derive(Default)]
pub struct Permissions {
    seen: BTreeSet<String>,
    pending: BTreeMap<String, Pending>,
    hook_scope: Option<(String, String)>,
    failed: bool,
}

struct Pending {
    hook: bool,
    token: Arc<()>,
    tool_name: String,
    tool_use_id: Option<String>,
}

/// Private inspection, not authorization. Never serialize into chat or run metadata.
pub struct Request {
    pub request_id: String,
    pub tool_name: String,
    pub tool_use_id: Option<String>,
    target: ToolTarget,
    tool_input: String,
    token: Arc<()>,
}

/// Paths are private inspection data, never public chat/status metadata.
pub enum ToolTarget {
    File { path: String, writes: bool },
    Search { path: String },
    Shell,
    Unsupported,
}
impl Request {
    /// Only expose following an explicit, live approval inspection request.
    pub fn inspect_input(&self) -> &str {
        &self.tool_input
    }
    pub fn target(&self) -> &ToolTarget {
        &self.target
    }
}
fn tool_target(tool: &str, input: &Value, cwd: &str) -> Result<ToolTarget, ProviderError> {
    fn path(value: &Value) -> Result<String, ProviderError> {
        let path = value.as_str().ok_or_else(invalid)?;
        if path.is_empty() || path.len() > 16 * 1024 || path.chars().any(char::is_control) {
            return Err(invalid());
        }
        Ok(path.into())
    }
    match tool {
        "Read" | "Write" | "Edit" => Ok(ToolTarget::File {
            path: path(input.get("file_path").ok_or_else(invalid)?)?,
            writes: tool != "Read",
        }),
        "Glob" | "Grep" => {
            if !input.get("pattern").is_some_and(Value::is_string) {
                return Err(invalid());
            }
            let root = match input.get("path") {
                Some(value) => path(value)?,
                None => cwd.into(),
            };
            Ok(ToolTarget::Search { path: root })
        }
        "Bash" => Ok(ToolTarget::Shell),
        _ => Ok(ToolTarget::Unsupported),
    }
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

/// A single response to the exact inspected request, not a persistent permission.
pub struct AllowOnce(String);
impl AllowOnce {
    pub fn as_str(&self) -> &str { &self.0 }
}

fn invalid() -> ProviderError {
    ProviderError("Claude permission request did not match".into())
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

impl Permissions {
    pub fn is_pending(&self, request_id: &str) -> bool {
        !self.failed && self.pending.contains_key(request_id)
    }

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
            agent_id: Option<String>,
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
        let (tool_name, tool_use_id, target, tool_input) = if hook {
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
            let target = tool_target(&data.tool_name, &data.tool_input, cwd)?;
            (data.tool_name, Some(data.tool_use_id), target, data.tool_input)
        } else {
            let tool_name = request.tool_name.ok_or_else(invalid)?;
            let target = match self.hook_scope.as_ref() {
                Some((_, cwd)) => tool_target(&tool_name, &request.input, cwd)?,
                None => ToolTarget::Unsupported,
            };
            (
                tool_name,
                request.tool_use_id,
                target,
                request.input.clone(),
            )
        };
        if wire.kind != "control_request"
            || (!hook && request.subtype != "can_use_tool")
            || request.agent_id.is_some()
            || !valid_id(&tool_name)
            || !request.input.is_object()
            || tool_use_id.as_deref().is_some_and(|id| !valid_id(id))
            || self.seen.len() >= 1024
            || !self.seen.insert(wire.request_id.clone())
        {
            return Err(invalid());
        }
        let tool_input = tool_input.to_string();
        if tool_input.len() > 128 * 1024 {
            return Err(invalid());
        }
        let token = Arc::new(());
        self.pending.insert(wire.request_id.clone(), Pending {
            hook, token: token.clone(), tool_name: tool_name.clone(), tool_use_id: tool_use_id.clone(),
        });
        let request = Request {
            request_id: wire.request_id,
            tool_name,
            tool_use_id,
            target,
            tool_input,
            token,
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
        let pending = self.pending.remove(request_id).ok_or_else(invalid)?;
        let response = if pending.hook {
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

    /// Caller must first admit the tool against the execution boundary and user decision.
    /// No caller-supplied replacement input or persistent permission updates are accepted.
    pub fn allow_once(&mut self, request: &Request) -> Result<AllowOnce, ProviderError> {
        let pending = self.pending.get(&request.request_id).ok_or_else(invalid)?;
        if self.failed || self.hook_scope.is_none()
            || !Arc::ptr_eq(&pending.token, &request.token)
            || pending.tool_name != request.tool_name || pending.tool_use_id != request.tool_use_id
            || matches!(request.target, ToolTarget::Unsupported)
        { return Err(invalid()); }
        let input: Value = serde_json::from_str(&request.tool_input).map_err(|_| invalid())?;
        let response = if pending.hook {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse",
                "permissionDecision":"allow","updatedInput":input}})
        } else {
            json!({"behavior":"allow","updatedInput":input})
        };
        let frame = format!("{}\n", json!({"type":"control_response","response":{
            "subtype":"success","request_id":request.request_id,"response":response
        }}));
        if frame.len() > crate::provider::PROVIDER_OBSERVATION_LINE_MAX_BYTES { return Err(invalid()); }
        self.pending.remove(&request.request_id);
        Ok(AllowOnce(frame))
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
    fn allow_once_binds_connection_request_and_original_input() {
        let mut first = Permissions::default();
        let mut second = Permissions::default();
        first.bind_hook_scope("s", "/snapshot");
        second.bind_hook_scope("s", "/snapshot");
        let Observation::Requested(mut a) = first.observe(&request("a")).unwrap() else { panic!() };
        let Observation::Requested(b) = second.observe(&request("a")).unwrap() else { panic!() };
        assert!(first.allow_once(&b).is_err());
        a.tool_name = "Read".into();
        assert!(first.allow_once(&a).is_err());
        a.tool_name = "Bash".into();
        let reply = first.allow_once(&a).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["response"], json!({"behavior":"allow","updatedInput":{"command":"SECRET"}}));
        assert!(first.allow_once(&a).is_err());
        assert!(first.deny("a", false).is_err());
        second.observe(&input(json!({"type":"control_cancel_request","request_id":"a"}))).unwrap();
        assert!(second.allow_once(&b).is_err());
    }
    #[test]
    fn hook_allow_keeps_input_and_does_not_add_persistent_permissions() {
        let mut permissions = Permissions::default();
        permissions.bind_hook_scope("s", "/snapshot");
        let hook = input(json!({"type":"control_request","request_id":"h","request":{
            "subtype":"hook_callback","callback_id":PRE_TOOL_CALLBACK,"tool_use_id":"t",
            "input":{"session_id":"s","cwd":"/snapshot","hook_event_name":"PreToolUse",
                "tool_name":"Read","tool_use_id":"t","tool_input":{"file_path":"src/lib.rs"}}
        }}));
        let Observation::ToolCheck(request) = permissions.observe(&hook).unwrap() else { panic!() };
        let reply = permissions.allow_once(&request).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["response"], json!({"hookSpecificOutput":{
            "hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"file_path":"src/lib.rs"}
        }}));
        assert!(permissions.allow_once(&request).is_err());
    }
    #[test]
    fn explicit_inspection_retains_input_without_echoing_in_denial() {
        let mut permissions = Permissions::default();
        permissions.bind_hook_scope("s", "/snapshot");
        let wire = input(json!({"type":"control_request","request_id":"edit","request":{
            "subtype":"can_use_tool","tool_name":"Edit","input":{
                "file_path":"src/main.rs","old_string":"OLD_PRIVATE","new_string":"NEW_PRIVATE"
            },"tool_use_id":"tool1"
        }}));
        let Observation::Requested(request) = permissions.observe(&wire).unwrap() else { panic!() };
        assert!(matches!(request.target(), ToolTarget::File { path, writes:true } if path == "src/main.rs"));
        let content: Value = serde_json::from_str(request.inspect_input()).unwrap();
        assert_eq!(content["new_string"], "NEW_PRIVATE");
        assert!(!permissions.deny("edit", false).unwrap().as_str().contains("PRIVATE"));
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
