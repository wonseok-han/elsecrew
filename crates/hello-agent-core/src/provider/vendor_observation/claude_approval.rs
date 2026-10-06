//! Claude SDK control permissions. Denials only until the runtime admits tool grants.
//! Wire: anthropics/claude-agent-sdk-python/_internal/query.py (2026-10-06).
//! One instance belongs to one connection. Raw tool inputs are never public output.
use super::{BoundedObservation, ProviderError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct Permissions {
    seen: BTreeSet<String>,
    pending: BTreeSet<String>,
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
            tool_name: String,
            input: Value,
            tool_use_id: Option<String>,
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
        if wire.kind != "control_request"
            || request.subtype != "can_use_tool"
            || !valid_id(&request.tool_name)
            || !request.input.is_object()
            || request
                .tool_use_id
                .as_deref()
                .is_some_and(|id| !valid_id(id))
            || self.seen.len() >= 1024
            || !self.seen.insert(wire.request_id.clone())
        {
            return Err(invalid());
        }
        self.pending.insert(wire.request_id.clone());
        Ok(Observation::Requested(Request {
            request_id: wire.request_id,
            tool_name: request.tool_name,
            tool_use_id: request.tool_use_id,
        }))
    }

    /// Consumes exactly one outstanding request. No permission update or allow path.
    pub fn deny(&mut self, request_id: &str, interrupt: bool) -> Result<Denial, ProviderError> {
        if self.failed || !self.pending.remove(request_id) {
            return Err(invalid());
        }
        let frame = json!({"type":"control_response","response":{
            "subtype":"success","request_id":request_id,
            "response":{"behavior":"deny","message":"Tool request declined by the application user.","interrupt":interrupt}
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
