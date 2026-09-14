//! Offline public-item projection for app-server notifications.
//! https://learn.chatgpt.com/docs/app-server#events
//! Not a transport, lifecycle validator, permission gate, or process-exit proof.
//! The owner must correlate request responses and enforce turn/item ordering before
//! publishing these projections. Unknown notifications require owner handling, never
//! silent acceptance. Raw reasoning, tools, errors and credentials are not projected.

use super::{BoundedObservation, ProviderError};
use crate::provider::{
    PROVIDER_INTERNAL_EVENTS_MAX, PROVIDER_OBSERVATION_TOTAL_MAX_BYTES, PROVIDER_PUBLIC_EVENTS_MAX,
    PROVIDER_PUBLIC_TEXT_MAX_BYTES, PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES,
};
use serde::Deserialize;
use serde_json::value::RawValue;

#[derive(Debug, PartialEq, Eq)]
pub enum PublicKind {
    AgentMessage,
    Plan,
}

// Deliberately no Debug: even public model output may contain user information.
pub enum Projection {
    Delta {
        kind: PublicKind,
        item_id: String,
        text: String,
    },
    /// Replace the item's accumulated text, do not append it a second time.
    Completed {
        kind: PublicKind,
        item_id: String,
        text: String,
    },
    /// Not public chat. The owner still has to validate/handle this notification.
    RequiresHandling,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Notification {
    method: String,
    params: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scope {
    thread_id: String,
    turn_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Delta {
    item_id: String,
    delta: String,
}

#[derive(Deserialize)]
struct Completed {
    item: Box<RawValue>,
}

#[derive(Deserialize)]
struct ItemTag {
    id: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct TextItem {
    text: String,
}

/// One bounded projection budget per expected turn. Failure is sticky.
pub struct TurnProjection {
    thread_id: String,
    turn_id: String,
    events: usize,
    bytes: usize,
    public_events: usize,
    public_bytes: usize,
    failed: bool,
}

fn invalid() -> ProviderError {
    ProviderError("invalid Codex public event observation".into())
}
fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, ProviderError> {
    serde_json::from_str(raw).map_err(|_| invalid())
}

impl TurnProjection {
    pub fn new(thread_id: &str, turn_id: &str) -> Result<Self, ProviderError> {
        if !identifier(thread_id) || !identifier(turn_id) {
            return Err(invalid());
        }
        Ok(Self {
            thread_id: thread_id.into(),
            turn_id: turn_id.into(),
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
        let frame: Notification = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        // No JSON-RPC requests/responses can be mistaken for public notifications.
        if frame.method == "model/rerouted" {
            return Err(invalid());
        }
        let kind = match frame.method.as_str() {
            "item/agentMessage/delta" => Some(PublicKind::AgentMessage),
            "item/plan/delta" => Some(PublicKind::Plan),
            "item/completed" => None,
            _ => return Ok(Projection::RequiresHandling),
        };
        let scope: Scope = decode(frame.params.get())?;
        if scope.thread_id != self.thread_id || scope.turn_id != self.turn_id {
            return Err(invalid());
        }
        let (kind, item_id, text, completed) = if let Some(kind) = kind {
            let delta: Delta = decode(frame.params.get())?;
            (kind, delta.item_id, delta.delta, false)
        } else {
            let completed: Completed = decode(frame.params.get())?;
            let item: ItemTag = decode(completed.item.get())?;
            let kind = match item.kind.as_str() {
                "agentMessage" => PublicKind::AgentMessage,
                "plan" => PublicKind::Plan,
                _ => return Ok(Projection::RequiresHandling),
            };
            let body: TextItem = decode(completed.item.get())?;
            (kind, item.id, body.text, true)
        };
        if !identifier(&item_id) || text.len() > PROVIDER_PUBLIC_TEXT_MAX_BYTES {
            return Err(invalid());
        }
        self.public_events += 1;
        self.public_bytes += text.len();
        if self.public_events > PROVIDER_PUBLIC_EVENTS_MAX
            || self.public_bytes > PROVIDER_PUBLIC_TEXT_TOTAL_MAX_BYTES
        {
            return Err(invalid());
        }
        Ok(if completed {
            Projection::Completed {
                kind,
                item_id,
                text,
            }
        } else {
            Projection::Delta {
                kind,
                item_id,
                text,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(method: &str, params: serde_json::Value) -> BoundedObservation {
        BoundedObservation::new(
            serde_json::to_vec(&json!({"method":method,"params":params})).unwrap(),
        )
        .unwrap()
    }
    fn delta() -> BoundedObservation {
        frame(
            "item/agentMessage/delta",
            json!({"threadId":"t","turnId":"r","itemId":"i","delta":"hello"}),
        )
    }
    #[test]
    fn projects_only_public_text_and_distinguishes_authoritative_replacement() {
        let mut p = TurnProjection::new("t", "r").unwrap();
        assert!(
            matches!(p.project(&delta()).unwrap(), Projection::Delta { text, .. } if text == "hello")
        );
        let final_plan = frame(
            "item/completed",
            json!({"threadId":"t","turnId":"r","item":{"type":"plan","id":"p","text":"revised plan","secret":"not copied"}}),
        );
        assert!(
            matches!(p.project(&final_plan).unwrap(), Projection::Completed { kind:PublicKind::Plan, text, .. } if text == "revised plan")
        );
        for method in [
            "item/reasoning/textDelta",
            "item/reasoning/summaryTextDelta",
            "item/commandExecution/outputDelta",
            "unknown",
        ] {
            assert!(matches!(
                p.project(&frame(method, json!({"delta":"SECRET"})))
                    .unwrap(),
                Projection::RequiresHandling
            ));
        }
        let reasoning = frame(
            "item/completed",
            json!({"threadId":"t","turnId":"r","item":{"type":"reasoning","id":"x","text":"SECRET","content":["SECRET"]}}),
        );
        assert!(matches!(
            p.project(&reasoning).unwrap(),
            Projection::RequiresHandling
        ));
    }
    #[test]
    fn rejects_cross_turn_duplicate_fields_requests_and_reroutes_permanently() {
        for raw in [
            r#"{"method":"item/agentMessage/delta","params":{"threadId":"other","turnId":"r","itemId":"i","delta":"SECRET"}}"#,
            r#"{"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"other","itemId":"i","delta":"SECRET"}}"#,
            r#"{"method":"item/agentMessage/delta","params":{"threadId":"t","threadId":"t","turnId":"r","itemId":"i","delta":"SECRET"}}"#,
            r#"{"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"r","itemId":"i","delta":"a","delta":"SECRET"}}"#,
            r#"{"method":"item/completed","params":{"threadId":"t","turnId":"r","item":{"id":"i","type":"reasoning","type":"agentMessage","text":"SECRET"}}}"#,
            r#"{"id":1,"method":"item/agentMessage/delta","params":{}}"#,
            r#"{"method":"model/rerouted","params":{}}"#,
        ] {
            let mut p = TurnProjection::new("t", "r").unwrap();
            let error = p
                .project(&BoundedObservation::new(raw.as_bytes().to_vec()).unwrap())
                .err()
                .unwrap();
            assert!(!format!("{error:?}").contains("SECRET"));
            assert!(p.project(&delta()).is_err());
        }
    }
    #[test]
    fn enforces_public_and_private_stream_budgets() {
        let mut p = TurnProjection::new("t", "r").unwrap();
        for _ in 0..PROVIDER_PUBLIC_EVENTS_MAX {
            assert!(p.project(&delta()).is_ok());
        }
        assert!(p.project(&delta()).is_err());
        let mut p = TurnProjection::new("t", "r").unwrap();
        let private = frame("item/reasoning/textDelta", json!({"delta":"secret"}));
        for _ in 0..PROVIDER_INTERNAL_EVENTS_MAX {
            assert!(p.project(&private).is_ok());
        }
        assert!(p.project(&private).is_err());
        let mut p = TurnProjection::new("t", "r").unwrap();
        let oversized = frame(
            "item/plan/delta",
            json!({"threadId":"t","turnId":"r","itemId":"i","delta":"x".repeat(PROVIDER_PUBLIC_TEXT_MAX_BYTES+1)}),
        );
        assert!(p.project(&oversized).is_err());
        let mut p = TurnProjection::new("t", "r").unwrap();
        let large_private = frame(
            "item/reasoning/textDelta",
            json!({"delta":"x".repeat(64 * 1024)}),
        );
        let accepted = PROVIDER_OBSERVATION_TOTAL_MAX_BYTES / large_private.bytes.len();
        for _ in 0..accepted {
            assert!(p.project(&large_private).is_ok());
        }
        assert!(p.project(&large_private).is_err());
        assert!(p.project(&delta()).is_err());
    }
}
