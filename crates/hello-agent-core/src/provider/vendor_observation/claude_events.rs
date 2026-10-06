//! Public assistant projection for Claude Code stream-json, not launch authority.
//! Source: https://code.claude.com/docs/en/agent-sdk/typescript#sdkassistantmessage
//! The owner must separately handle initialization, tools, result, and process exit.
//! Partial stream events are not emitted here, avoiding duplicate text delivery.
use super::{BoundedObservation, ProviderError};
use crate::provider::{
    PROVIDER_INTERNAL_EVENTS_MAX, PROVIDER_OBSERVATION_TOTAL_MAX_BYTES, PROVIDER_PUBLIC_EVENTS_MAX,
    PROVIDER_PUBLIC_TEXT_MAX_BYTES, PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES,
};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::collections::BTreeSet;

// No Debug: model text may contain user information.
pub enum Projection {
    Assistant {
        item_id: String,
        text: String,
        /// The original frame still needs non-public block handling by the owner.
        has_private_blocks: bool,
    },
    RequiresHandling,
}

#[derive(Deserialize)]
struct Tag {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Assistant {
    uuid: String,
    session_id: String,
    parent_tool_use_id: Option<String>,
    message: Message,
    error: Option<String>,
    aborted: Option<bool>,
}

#[derive(Deserialize)]
struct Message {
    id: String,
    role: String,
    content: Vec<Box<RawValue>>,
}

fn invalid() -> ProviderError {
    ProviderError("invalid Claude public event observation".into())
}
fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

/// One session's bounded public projection. Failure is sticky; no raw frames retained.
pub struct SessionProjection {
    session_id: String,
    seen: BTreeSet<String>,
    events: usize,
    bytes: usize,
    public_events: usize,
    public_bytes: usize,
    failed: bool,
}

impl SessionProjection {
    pub fn new(session_id: &str) -> Result<Self, ProviderError> {
        if !identifier(session_id) {
            return Err(invalid());
        }
        Ok(Self {
            session_id: session_id.into(),
            seen: BTreeSet::new(),
            events: 0,
            bytes: 0,
            public_events: 0,
            public_bytes: 0,
            failed: false,
        })
    }

    pub fn project(&mut self, input: &BoundedObservation) -> Result<Projection, ProviderError> {
        if self.failed {
            return Err(invalid());
        }
        let result = self.project_inner(input);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn project_inner(&mut self, input: &BoundedObservation) -> Result<Projection, ProviderError> {
        self.events += 1;
        self.bytes += input.bytes.len();
        if self.events > PROVIDER_INTERNAL_EVENTS_MAX
            || self.bytes > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES
        {
            return Err(invalid());
        }
        let tag: Tag = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if tag.kind != "assistant" {
            return Ok(Projection::RequiresHandling);
        }
        let wire: Assistant = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        // Missing parent scope must not be interpreted as an explicit main-thread null.
        if input.json()?.get("parent_tool_use_id").is_none()
            || wire.session_id != self.session_id
            || !identifier(&wire.uuid)
            || !identifier(&wire.message.id)
            || wire.message.role != "assistant"
            || !self.seen.insert(wire.uuid.clone())
        {
            return Err(invalid());
        }
        if wire.parent_tool_use_id.is_some() || wire.error.is_some() || wire.aborted == Some(true) {
            return Ok(Projection::RequiresHandling);
        }
        let mut text = String::new();
        let mut has_private_blocks = false;
        for raw in wire.message.content {
            let tag: Tag = serde_json::from_str(raw.get()).map_err(|_| invalid())?;
            if tag.kind != "text" {
                has_private_blocks = true;
                continue;
            }
            #[derive(Deserialize)]
            struct Text {
                text: String,
            }
            let block: Text = serde_json::from_str(raw.get()).map_err(|_| invalid())?;
            if text.len() + block.text.len() > PROVIDER_PUBLIC_TEXT_MAX_BYTES {
                return Err(invalid());
            }
            text.push_str(&block.text);
        }
        if text.is_empty() {
            return Ok(Projection::RequiresHandling);
        }
        self.public_events += 1;
        self.public_bytes += text.len();
        if self.public_events > PROVIDER_PUBLIC_EVENTS_MAX
            || self.public_bytes > PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES
        {
            return Err(invalid());
        }
        // message.id can repeat across assistant frames; uuid is their delivery identity.
        Ok(Projection::Assistant {
            item_id: wire.uuid,
            text,
            has_private_blocks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn frame(uuid: &str) -> Value {
        json!({"type":"assistant","uuid":uuid,"session_id":"session-a","parent_tool_use_id":null,
        "message":{"id":"message-shared","role":"assistant","content":[
            {"type":"thinking","thinking":"PRIVATE"},
            {"type":"text","text":"공개 답변"},
            {"type":"tool_use","input":{"token":"SECRET"}}
        ]}})
    }
    fn observe(value: &Value) -> BoundedObservation {
        BoundedObservation::new(value.to_string().into_bytes()).unwrap()
    }
    #[test]
    fn projects_only_main_assistant_text_and_uses_delivery_identity() {
        let mut parser = SessionProjection::new("session-a").unwrap();
        for uuid in ["uuid-a", "uuid-b"] {
            match parser.project(&observe(&frame(uuid))).unwrap() {
                Projection::Assistant {
                    item_id,
                    text,
                    has_private_blocks,
                } => {
                    assert_eq!(item_id, uuid);
                    assert_eq!(text, "공개 답변");
                    assert!(has_private_blocks);
                }
                _ => panic!("expected public assistant text"),
            }
        }
        assert!(parser.project(&observe(&frame("uuid-a"))).is_err());
        assert!(parser.project(&observe(&frame("uuid-c"))).is_err());
    }
    #[test]
    fn does_not_publish_partial_result_error_or_subagent_frames() {
        for (key, value) in [
            ("type", json!("stream_event")),
            ("type", json!("result")),
            ("parent_tool_use_id", json!("tool-a")),
            ("error", json!("authentication_failed")),
            ("aborted", json!(true)),
        ] {
            let mut input = frame("uuid-a");
            input[key] = value;
            assert!(matches!(
                SessionProjection::new("session-a")
                    .unwrap()
                    .project(&observe(&input))
                    .unwrap(),
                Projection::RequiresHandling
            ));
        }
    }
    #[test]
    fn scope_and_text_budget_fail_without_echoing_private_input() {
        for case in ["session", "parent", "oversize", "duplicate"] {
            let mut input = frame("uuid-a");
            match case {
                "session" => input["session_id"] = json!("PRIVATE"),
                "parent" => {
                    input.as_object_mut().unwrap().remove("parent_tool_use_id");
                }
                "oversize" => {
                    input["message"]["content"] =
                        json!([{"type":"text","text":"x".repeat(PROVIDER_PUBLIC_TEXT_MAX_BYTES+1)}])
                }
                _ => {}
            }
            let observation = if case == "duplicate" {
                BoundedObservation::new(
                    input
                        .to_string()
                        .replacen(
                            "\"session_id\":",
                            "\"session_id\":\"PRIVATE\",\"session_id\":",
                            1,
                        )
                        .into_bytes(),
                )
                .unwrap()
            } else {
                observe(&input)
            };
            let mut parser = SessionProjection::new("session-a").unwrap();
            let error = parser.project(&observation).err().expect("must reject");
            assert!(!error.to_string().contains("PRIVATE"));
            assert!(parser.project(&observe(&frame("uuid-b"))).is_err());
        }
    }
}
