//! One connection's post-handshake observations; not permission to launch or send a prompt.
use super::{claude_approval, claude_events, claude_init, BoundedObservation, ProviderError};
use crate::provider::{PROVIDER_INTERNAL_EVENTS_MAX, PROVIDER_OBSERVATION_TOTAL_MAX_BYTES};
use serde::Deserialize;

pub enum Event {
    Initialized {
        receipt: InitReceipt,
        interrupt_receipt: bool,
        interrupt_cancel_queued: bool,
    },
    Permission(claude_approval::Observation),
    Output(claude_events::Projection),
}

/// Constructed only by a successful system/init observation. Not launch authority.
pub struct InitReceipt { session_id: String, cwd: String, model: String, organization_guard: bool }
impl InitReceipt {
    pub fn organization_guard(&self) -> bool { self.organization_guard }
    pub fn session_id(&self) -> &str { &self.session_id }
    pub fn matches(&self, cwd: &str, model: &str) -> bool { self.cwd == cwd && self.model == model }
}

pub struct Session {
    cwd: String,
    model: String,
    permission_mode: String,
    tools: Vec<String>,
    projection: Option<claude_events::SessionProjection>,
    permissions: claude_approval::Permissions,
    events: usize,
    bytes: usize,
    failed: bool,
    diagnostic_stage: &'static str,
    allow_organization_guard: bool,
}

fn invalid() -> ProviderError {
    ProviderError("Claude session observation did not match".into())
}

impl Session {
    pub fn diagnostic_stage(&self) -> &'static str { self.diagnostic_stage }
    pub fn is_permission_pending(&self, request_id: &str) -> bool {
        !self.failed && self.permissions.is_pending(request_id)
    }

    pub fn matches_expected(&self, expected: &claude_init::ExpectedInit<'_>) -> bool {
        !self.failed && self.cwd == expected.cwd && self.model == expected.model
            && self.permission_mode == expected.permission_mode
            && self.tools.iter().map(String::as_str).eq(expected.tools.iter().copied())
    }

    pub fn is_closed(&self) -> bool {
        self.failed
    }

    pub fn new(expected: &claude_init::ExpectedInit<'_>) -> Self {
        Self {
            cwd: expected.cwd.into(),
            model: expected.model.into(),
            permission_mode: expected.permission_mode.into(),
            tools: expected.tools.iter().map(|s| (*s).into()).collect(),
            projection: None,
            permissions: Default::default(),
            events: 0,
            bytes: 0,
            failed: false,
            diagnostic_stage: "session state",
            allow_organization_guard: false,
        }
    }

    /// For an owner that separately authenticates the native provider process.
    /// Does not authenticate provider registration strings by itself.
    pub fn for_native_organization_guard(expected: &claude_init::ExpectedInit<'_>) -> Self {
        Self { allow_organization_guard: true, ..Self::new(expected) }
    }

    pub fn observe(&mut self, input: &BoundedObservation) -> Result<Event, ProviderError> {
        let result = self.observe_inner(input);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn observe_inner(&mut self, input: &BoundedObservation) -> Result<Event, ProviderError> {
        #[derive(Deserialize)]
        struct Tag {
            #[serde(rename = "type")]
            kind: String,
            subtype: Option<String>,
        }
        if self.failed {
            return Err(invalid());
        }
        self.diagnostic_stage = "session budget";
        self.events = self.events.checked_add(1).ok_or_else(invalid)?;
        self.bytes = self
            .bytes
            .checked_add(input.bytes.len())
            .ok_or_else(invalid)?;
        if self.events > PROVIDER_INTERNAL_EVENTS_MAX
            || self.bytes > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES
        {
            return Err(invalid());
        }
        self.diagnostic_stage = "session tag";
        let tag: Tag = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if tag.kind == "system" && tag.subtype.as_deref() == Some("init") {
            if self.projection.is_some() {
                return Err(invalid());
            }
            let tools: Vec<_> = self.tools.iter().map(String::as_str).collect();
            let init = claude_init::observe_with_guard_policy(
                input,
                &claude_init::ExpectedInit {
                    cwd: &self.cwd,
                    model: &self.model,
                    permission_mode: &self.permission_mode,
                    tools: &tools,
                },
                &mut self.diagnostic_stage,
                self.allow_organization_guard,
            )?;
            let event = Event::Initialized {
                receipt: InitReceipt { session_id:init.session_id().into(), cwd:self.cwd.clone(), model:self.model.clone(), organization_guard:init.organization_guard },
                interrupt_receipt: init.interrupt_receipt,
                interrupt_cancel_queued: init.interrupt_cancel_queued,
            };
            self.permissions
                .bind_hook_scope(init.session_id(), &self.cwd);
            self.projection = Some(init.into_projection()?);
            return Ok(event);
        }
        // Do not infer a session or quietly accept tool requests before reported init.
        self.diagnostic_stage = "session before init";
        let projection = self.projection.as_mut().ok_or_else(invalid)?;
        self.diagnostic_stage = "session event";
        match tag.kind.as_str() {
            "control_request" | "control_cancel_request" => {
                self.permissions.observe(input).map(Event::Permission)
            }
            "control_response" => Err(invalid()), // No outstanding response after consumed handshake.
            _ => projection.project(input).map(Event::Output),
        }
    }

    pub fn deny(
        &mut self,
        request_id: &str,
        interrupt: bool,
    ) -> Result<claude_approval::Denial, ProviderError> {
        if self.failed || self.projection.is_none() {
            return Err(invalid());
        }
        self.permissions.deny(request_id, interrupt)
    }
    pub fn allow_once(&mut self, request: &claude_approval::Request) -> Result<claude_approval::AllowOnce, ProviderError> {
        if self.failed || self.projection.is_none() { return Err(invalid()); }
        self.permissions.allow_once(request)
    }

    pub fn close(&mut self) {
        self.failed = true;
        self.permissions.close();
        self.projection = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn session_binding_rejects_role_changes_and_closed_session() {
        let mut session = Session::new(&expected());
        assert!(session.matches_expected(&expected()));
        let mut other = expected();
        other.model = "other-model";
        assert!(!session.matches_expected(&other));
        other = expected();
        other.permission_mode = "bypassPermissions";
        assert!(!session.matches_expected(&other));
        other = expected();
        other.tools = &["Read", "Write"];
        assert!(!session.matches_expected(&other));
        session.close();
        assert!(!session.matches_expected(&expected()));
    }

    fn expected() -> claude_init::ExpectedInit<'static> {
        claude_init::ExpectedInit {
            cwd: "/snapshot",
            model: "model",
            permission_mode: "default",
            tools: &["Read"],
        }
    }
    fn input(value: serde_json::Value) -> BoundedObservation {
        BoundedObservation::new(value.to_string().into_bytes()).unwrap()
    }
    fn init() -> BoundedObservation {
        input(
            json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":"/snapshot","model":"model","permissionMode":"default","tools":["Read"],"mcp_servers":[],"skills":[],"plugins":[]}),
        )
    }
    #[test]
    fn native_guard_receipt_is_explicit_and_generic_session_still_rejects() {
        let mut wire: serde_json::Value = serde_json::from_slice(&init().bytes).unwrap();
        wire["plugins"] = json!([{"name":"cc-plugin-sec-default","path":"builtin","source":"cc-plugin-sec-default@builtin"}]);
        let observation = input(wire);
        assert!(Session::new(&expected()).observe(&observation).is_err());
        let mut session = Session::for_native_organization_guard(&expected());
        let Event::Initialized { receipt, .. } = session.observe(&observation).unwrap() else { panic!("expected init") };
        assert!(receipt.organization_guard());
        assert!(receipt.matches("/snapshot", "model"));
        assert!(session.observe(&observation).is_err());
        let Event::Initialized { receipt, .. } = Session::for_native_organization_guard(&expected()).observe(&init()).unwrap() else { panic!("expected init") };
        assert!(!receipt.organization_guard());
    }

    #[test]
    fn init_is_required_and_duplicate_init_stops_session() {
        let mut session = Session::new(&expected());
        assert!(session
            .observe(&input(json!({"type":"assistant"})))
            .is_err());
        assert!(session.observe(&init()).is_err());
        let mut session = Session::new(&expected());
        assert!(matches!(
            session.observe(&init()).unwrap(),
            Event::Initialized { .. }
        ));
        assert!(session.observe(&init()).is_err());
        assert!(session.deny("a", false).is_err());
    }
    #[test]
    fn public_output_and_permission_share_one_initialized_session() {
        let mut session = Session::new(&expected());
        session.observe(&init()).unwrap();
        let output = input(
            json!({"type":"assistant","uuid":"a","session_id":"s","parent_tool_use_id":null,"message":{"id":"m","role":"assistant","content":[{"type":"text","text":"hello"}]}}),
        );
        assert!(matches!(
            session.observe(&output).unwrap(),
            Event::Output(claude_events::Projection::Assistant { .. })
        ));
        let request = input(
            json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Read","input":{"file_path":"hello.txt"}}}),
        );
        assert!(matches!(
            session.observe(&request).unwrap(),
            Event::Permission(claude_approval::Observation::Requested(_))
        ));
        assert!(session.deny("r", false).is_ok());
        assert!(session.deny("r", false).is_err());
    }

    #[test]
    fn read_permission_without_target_closes_initialized_session() {
        let mut session = Session::new(&expected());
        session.observe(&init()).unwrap();
        let request = input(json!({"type":"control_request","request_id":"r","request":{
            "subtype":"can_use_tool","tool_name":"Read","input":{}
        }}));
        assert!(session.observe(&request).is_err());
        assert!(session.is_closed());
        assert!(session.deny("r", false).is_err());
    }
}
