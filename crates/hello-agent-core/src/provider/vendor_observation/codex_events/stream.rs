//! Offline ordering guard layered on the bounded public projection.
//! The transport owner must dispatch all notifications for this turn here and handle
//! `RequiresHandling` separately. This does not authorize tools or prove process exit.

use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed,
    Interrupted,
    Failed,
}

pub enum Observed {
    Started,
    Public(Projection),
    ItemStarted,
    Finished(TurnOutcome),
    /// No public content; not an authorization to ignore a tool/request/policy event.
    RequiresHandling,
}

struct ItemState {
    kind: String,
    completed: bool,
}

pub struct TurnStream {
    projection: TurnProjection,
    started: bool,
    outcome: Option<TurnOutcome>,
    items: BTreeMap<String, ItemState>,
    failed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnFrame {
    thread_id: String,
    turn: Turn,
}

#[derive(Deserialize)]
struct Turn {
    id: String,
    status: String,
    items: Vec<serde::de::IgnoredAny>,
    error: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemReference {
    item_id: String,
}

impl TurnStream {
    /// Identity comes from the owner's correlated start response, never a notification.
    pub fn new(thread_id: &str, turn_id: &str) -> Result<Self, ProviderError> {
        Ok(Self {
            projection: TurnProjection::new(thread_id, turn_id)?,
            started: false,
            outcome: None,
            items: BTreeMap::new(),
            failed: false,
        })
    }

    pub fn observe(&mut self, input: &BoundedObservation) -> Result<Observed, ProviderError> {
        if self.failed || self.outcome.is_some() {
            self.failed = true;
            return Err(invalid());
        }
        let result = self.observe_inner(input);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// EOF without a terminal event is never success. Even successful return says
    /// only what the vendor reported; process-tree cleanup remains mandatory.
    pub fn finish(self) -> Result<TurnOutcome, ProviderError> {
        if self.failed {
            return Err(invalid());
        }
        self.outcome.ok_or_else(invalid)
    }

    fn observe_inner(&mut self, input: &BoundedObservation) -> Result<Observed, ProviderError> {
        // Charge all notifications, including non-public ones, exactly once.
        let projected = self.projection.project(input)?;
        let frame: Notification = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if matches!(frame.method.as_str(), "turn/started" | "turn/completed") {
            let wire: TurnFrame = decode(frame.params.get())?;
            if wire.thread_id != self.projection.thread_id
                || wire.turn.id != self.projection.turn_id
            {
                return Err(invalid());
            }
            if frame.method == "turn/started" {
                if self.started
                    || wire.turn.status != "inProgress"
                    || !wire.turn.items.is_empty()
                    || wire.turn.error.get() != "null"
                {
                    return Err(invalid());
                }
                self.started = true;
                return Ok(Observed::Started);
            }
            if !self.started {
                return Err(invalid());
            }
            let outcome = match wire.turn.status.as_str() {
                "completed"
                    if wire.turn.error.get() == "null"
                        && self.items.values().all(|item| item.completed) =>
                {
                    TurnOutcome::Completed
                }
                "interrupted" if wire.turn.error.get() == "null" => TurnOutcome::Interrupted,
                "failed" => {
                    #[derive(Deserialize)]
                    struct Failure {
                        message: String,
                    }
                    let error: Failure = decode(wire.turn.error.get())?;
                    if error.message.is_empty() {
                        return Err(invalid());
                    }
                    TurnOutcome::Failed
                }
                _ => return Err(invalid()),
            };
            self.outcome = Some(outcome);
            return Ok(Observed::Finished(outcome));
        }
        if !self.started {
            return Err(invalid());
        }
        if frame.method == "item/started" || frame.method == "item/completed" {
            self.check_scope(frame.params.get())?;
            let wire: Completed = decode(frame.params.get())?;
            let item: ItemTag = decode(wire.item.get())?;
            if !identifier(&item.id) || !identifier(&item.kind) {
                return Err(invalid());
            }
            if frame.method == "item/started" {
                if self.items.contains_key(&item.id) {
                    return Err(invalid());
                }
                let public = matches!(item.kind.as_str(), "agentMessage" | "plan");
                self.items.insert(
                    item.id,
                    ItemState {
                        kind: item.kind,
                        completed: false,
                    },
                );
                return Ok(if public {
                    Observed::ItemStarted
                } else {
                    Observed::RequiresHandling
                });
            }
            let existing = self.items.get_mut(&item.id).ok_or_else(invalid)?;
            if existing.completed || existing.kind != item.kind {
                return Err(invalid());
            }
            existing.completed = true;
            return Ok(match projected {
                Projection::Completed { .. } => Observed::Public(projected),
                _ => Observed::RequiresHandling,
            });
        }
        let expected_kind = match frame.method.as_str() {
            "item/agentMessage/delta" => "agentMessage",
            "item/plan/delta" => "plan",
            "item/reasoning/textDelta"
            | "item/reasoning/summaryTextDelta"
            | "item/reasoning/summaryPartAdded" => "reasoning",
            "item/commandExecution/outputDelta" => "commandExecution",
            "item/fileChange/outputDelta" => "fileChange",
            _ => return Ok(Observed::RequiresHandling),
        };
        self.check_scope(frame.params.get())?;
        let reference: ItemReference = decode(frame.params.get())?;
        let item = self.items.get(&reference.item_id).ok_or_else(invalid)?;
        if item.completed || item.kind != expected_kind {
            return Err(invalid());
        }
        Ok(match projected {
            Projection::Delta { .. } => Observed::Public(projected),
            _ => Observed::RequiresHandling,
        })
    }

    fn check_scope(&self, raw: &str) -> Result<(), ProviderError> {
        let scope: Scope = decode(raw)?;
        if scope.thread_id != self.projection.thread_id || scope.turn_id != self.projection.turn_id
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn frame(method: &str, params: Value) -> BoundedObservation {
        BoundedObservation::new(
            serde_json::to_vec(&json!({"method":method,"params":params})).unwrap(),
        )
        .unwrap()
    }
    fn turn(method: &str, status: &str) -> BoundedObservation {
        frame(
            method,
            json!({"threadId":"t","turn":{"id":"r","status":status,"items":[],"error":null}}),
        )
    }
    fn item(method: &str, kind: &str) -> BoundedObservation {
        frame(
            method,
            json!({"threadId":"t","turnId":"r","item":{"id":"i","type":kind,"text":"public"}}),
        )
    }
    fn delta() -> BoundedObservation {
        frame(
            "item/agentMessage/delta",
            json!({"threadId":"t","turnId":"r","itemId":"i","delta":"public"}),
        )
    }
    fn started() -> TurnStream {
        let mut stream = TurnStream::new("t", "r").unwrap();
        assert!(matches!(
            stream.observe(&turn("turn/started", "inProgress")).unwrap(),
            Observed::Started
        ));
        stream
    }
    #[test]
    fn complete_stream_requires_both_item_and_turn_completion() {
        let mut stream = started();
        stream
            .observe(&item("item/started", "agentMessage"))
            .unwrap();
        assert!(matches!(
            stream.observe(&delta()).unwrap(),
            Observed::Public(Projection::Delta { .. })
        ));
        assert!(matches!(
            stream
                .observe(&item("item/completed", "agentMessage"))
                .unwrap(),
            Observed::Public(Projection::Completed { .. })
        ));
        assert!(matches!(
            stream
                .observe(&turn("turn/completed", "completed"))
                .unwrap(),
            Observed::Finished(TurnOutcome::Completed)
        ));
        assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);
        assert!(started().finish().is_err());
    }
    #[test]
    fn invalid_order_kind_identity_or_late_input_poison_stream() {
        let bad_sequences = vec![
            vec![delta()],
            vec![item("item/completed", "agentMessage")],
            vec![item("item/started", "reasoning"), delta()],
            vec![
                item("item/started", "agentMessage"),
                item("item/started", "agentMessage"),
            ],
            vec![
                item("item/started", "agentMessage"),
                item("item/completed", "plan"),
            ],
            vec![
                item("item/started", "agentMessage"),
                turn("turn/completed", "completed"),
            ],
            vec![
                item("item/started", "agentMessage"),
                item("item/completed", "agentMessage"),
                delta(),
            ],
            vec![turn("turn/started", "inProgress")],
            vec![turn("turn/completed", "completed"), delta()],
            vec![frame(
                "item/started",
                json!({"threadId":"other","turnId":"r","item":{"id":"i","type":"agentMessage"}}),
            )],
        ];
        for sequence in bad_sequences {
            let mut stream = started();
            let mut failed = false;
            for event in sequence {
                failed |= stream.observe(&event).is_err();
            }
            assert!(failed);
            assert!(stream
                .observe(&turn("turn/completed", "completed"))
                .is_err());
            assert!(stream.finish().is_err());
        }
    }
    #[test]
    fn interrupted_and_failed_turns_are_not_success_and_errors_stay_private() {
        let mut stream = started();
        stream
            .observe(&item("item/started", "agentMessage"))
            .unwrap();
        stream
            .observe(&turn("turn/completed", "interrupted"))
            .unwrap();
        assert_eq!(stream.finish().unwrap(), TurnOutcome::Interrupted);
        let mut stream = started();
        let failure = frame(
            "turn/completed",
            json!({"threadId":"t","turn":{"id":"r","status":"failed","items":[],"error":{"message":"SECRET"}}}),
        );
        assert!(matches!(
            stream.observe(&failure).unwrap(),
            Observed::Finished(TurnOutcome::Failed)
        ));
        assert_eq!(stream.finish().unwrap(), TurnOutcome::Failed);
        let mut stream = TurnStream::new("t", "r").unwrap();
        assert!(stream
            .observe(&turn("turn/completed", "completed"))
            .is_err());
    }

    #[test]
    fn private_and_unknown_items_require_owner_handling_throughout() {
        for kind in ["reasoning", "commandExecution", "fileChange", "futureItem"] {
            let mut stream = started();
            assert!(matches!(
                stream.observe(&item("item/started", kind)).unwrap(),
                Observed::RequiresHandling
            ));
            if kind == "reasoning" {
                let private = frame(
                    "item/reasoning/textDelta",
                    json!({"threadId":"t","turnId":"r","itemId":"i","delta":"SECRET"}),
                );
                assert!(matches!(
                    stream.observe(&private).unwrap(),
                    Observed::RequiresHandling
                ));
            }
            assert!(matches!(
                stream.observe(&item("item/completed", kind)).unwrap(),
                Observed::RequiresHandling
            ));
            stream
                .observe(&turn("turn/completed", "completed"))
                .unwrap();
            assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);
        }
    }
}
