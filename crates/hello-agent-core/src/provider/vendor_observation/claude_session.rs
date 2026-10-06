//! One connection's post-handshake observations; not permission to launch or send a prompt.
use super::{claude_approval, claude_events, claude_init, BoundedObservation, ProviderError};
use crate::provider::{PROVIDER_INTERNAL_EVENTS_MAX, PROVIDER_OBSERVATION_TOTAL_MAX_BYTES};
use serde::Deserialize;

pub enum Event {
    Initialized {
        interrupt_receipt: bool,
        interrupt_cancel_queued: bool,
    },
    Permission(claude_approval::Observation),
    Output(claude_events::Projection),
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
}

fn invalid() -> ProviderError {
    ProviderError("Claude session observation did not match".into())
}

impl Session {
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
        }
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
        let tag: Tag = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if tag.kind == "system" && tag.subtype.as_deref() == Some("init") {
            if self.projection.is_some() {
                return Err(invalid());
            }
            let tools: Vec<_> = self.tools.iter().map(String::as_str).collect();
            let init = claude_init::observe(
                input,
                &claude_init::ExpectedInit {
                    cwd: &self.cwd,
                    model: &self.model,
                    permission_mode: &self.permission_mode,
                    tools: &tools,
                },
            )?;
            let event = Event::Initialized {
                interrupt_receipt: init.interrupt_receipt,
                interrupt_cancel_queued: init.interrupt_cancel_queued,
            };
            self.projection = Some(init.into_projection()?);
            return Ok(event);
        }
        // Do not infer a session or quietly accept tool requests before reported init.
        let projection = self.projection.as_mut().ok_or_else(invalid)?;
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
            json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Read","input":{}}}),
        );
        assert!(matches!(
            session.observe(&request).unwrap(),
            Event::Permission(claude_approval::Observation::Requested(_))
        ));
        assert!(session.deny("r", false).is_ok());
        assert!(session.deny("r", false).is_err());
    }
}
