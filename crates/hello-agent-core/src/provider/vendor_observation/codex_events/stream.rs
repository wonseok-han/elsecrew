//! Offline ordering guard layered on the bounded public projection.
//! The transport owner must dispatch all notifications for this turn here and handle
//! `RequiresHandling` separately. This does not authorize tools or prove process exit.

use super::super::codex_approval::{Action, ApprovalReply, Decision, PendingApproval, RequestId};
use super::*;
use std::collections::{BTreeMap, BTreeSet};

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
    /// Validated user echo/reasoning metadata; payload is discarded, never public.
    Private,
    Finished(TurnOutcome),
    /// No public content; not an authorization to ignore a tool/request/policy event.
    RequiresHandling,
}

struct ItemState {
    kind: String,
    completed: bool,
    command: Option<(String, String)>,
}

pub struct TurnStream {
    projection: TurnProjection,
    started: bool,
    outcome: Option<TurnOutcome>,
    items: BTreeMap<String, ItemState>,
    failed: bool,
    approvals: BTreeMap<RequestId, Option<String>>,
    prepared_replies: BTreeSet<RequestId>,
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
            approvals: BTreeMap::new(),
            prepared_replies: BTreeSet::new(),
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

    /// Observe an approval only for this stream's still-live tool item. No reply
    /// or execution authority is produced; an owner/UI must decide separately.
    pub fn pending_approval(
        &mut self,
        input: &BoundedObservation,
    ) -> Result<PendingApproval, ProviderError> {
        let result = (|| {
            if self.failed || self.outcome.is_some() || !self.started {
                return Err(invalid());
            }
            self.projection.charge(input)?;
            let mut request = PendingApproval::parse(
                input,
                &self.projection.thread_id,
                &self.projection.turn_id,
            )?;
            if self.approvals.contains_key(&request.request_id) {
                return Err(invalid());
            }
            let item = self.items.get(&request.item_id).ok_or_else(invalid)?;
            if item.completed {
                return Err(invalid());
            }
            match &mut request.action {
                Action::Command { command, cwd, .. } if item.kind == "commandExecution" => {
                    let (expected_command, expected_cwd) =
                        item.command.as_ref().ok_or_else(invalid)?;
                    if command.as_ref().is_some_and(|v| v != expected_command)
                        || cwd.as_ref().is_some_and(|v| v != expected_cwd)
                    {
                        return Err(invalid());
                    }
                    *command = Some(expected_command.clone());
                    *cwd = Some(expected_cwd.clone());
                }
                Action::FileChange { .. } if item.kind == "fileChange" => {}
                _ => return Err(invalid()),
            }
            self.approvals
                .insert(request.request_id.clone(), Some(request.item_id.clone()));
            Ok(request)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Prepare exactly one response, not a delivery acknowledgement or completion.
    /// Only an admitted runtime bound to a live user decision may call this.
    /// Transport failure must terminate that runtime; never retry on a new session.
    pub fn prepare_approval_reply(
        &mut self,
        request_id: &RequestId,
        decision: Decision,
    ) -> Result<ApprovalReply, ProviderError> {
        let result = (|| {
            if self.failed
                || self.outcome.is_some()
                || !self.started
                || self.prepared_replies.contains(request_id)
            {
                return Err(invalid());
            }
            let item_id = self
                .approvals
                .get(request_id)
                .and_then(Option::as_ref)
                .ok_or_else(invalid)?;
            if self.items.get(item_id).is_none_or(|item| item.completed) {
                return Err(invalid());
            }
            let reply = ApprovalReply::new(request_id, decision)?;
            self.prepared_replies.insert(request_id.clone());
            Ok(reply)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn observe_inner(&mut self, input: &BoundedObservation) -> Result<Observed, ProviderError> {
        // Charge all notifications, including non-public ones, exactly once.
        let projected = self.projection.project(input)?;
        let frame: Notification = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if frame.method == "serverRequest/resolved" {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Resolved {
                thread_id: String,
                request_id: RequestId,
            }
            let resolved: Resolved = decode(frame.params.get())?;
            if !self.started || resolved.thread_id != self.projection.thread_id {
                return Err(invalid());
            }
            let pending = self
                .approvals
                .get_mut(&resolved.request_id)
                .ok_or_else(invalid)?;
            if pending.take().is_none() {
                return Err(invalid());
            }
            return Ok(Observed::Private);
        }
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
            if outcome == TurnOutcome::Completed && self.approvals.values().any(Option::is_some) {
                return Err(invalid());
            }
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
            let private = matches!(item.kind.as_str(), "userMessage" | "reasoning");
            if private {
                super::private::validate_item(wire.item.get())?;
            }
            if frame.method == "item/started" {
                if self.items.contains_key(&item.id) {
                    return Err(invalid());
                }
                let public = matches!(item.kind.as_str(), "agentMessage" | "plan");
                let command = if item.kind == "commandExecution" {
                    #[derive(Deserialize)]
                    struct Command {
                        command: String,
                        cwd: String,
                        status: String,
                    }
                    let value: Command = decode(wire.item.get())?;
                    if value.status != "inProgress"
                        || value.command.len() > 64 * 1024
                        || value.command.contains('\0')
                        || value.cwd.len() > 16 * 1024
                        || value.cwd.chars().any(char::is_control)
                    {
                        return Err(invalid());
                    }
                    Some((value.command, value.cwd))
                } else {
                    None
                };
                self.items.insert(
                    item.id,
                    ItemState {
                        kind: item.kind,
                        completed: false,
                        command,
                    },
                );
                return Ok(if public {
                    Observed::ItemStarted
                } else if private {
                    Observed::Private
                } else {
                    Observed::RequiresHandling
                });
            }
            if self
                .approvals
                .values()
                .any(|v| v.as_deref() == Some(&item.id))
            {
                return Err(invalid());
            }
            let existing = self.items.get_mut(&item.id).ok_or_else(invalid)?;
            if existing.completed || existing.kind != item.kind {
                return Err(invalid());
            }
            existing.completed = true;
            return Ok(match projected {
                Projection::Completed { .. } => Observed::Public(projected),
                _ if private => Observed::Private,
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
        if expected_kind == "reasoning" {
            super::private::validate_delta(&frame.method, frame.params.get())?;
            return Ok(Observed::Private);
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
            json!({"threadId":"t","turnId":"r","item":{"id":"i","type":kind,"text":"public","command":"echo test","cwd":"/snapshot","status":"inProgress"}}),
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
        for kind in ["commandExecution", "fileChange", "futureItem"] {
            let mut stream = started();
            assert!(matches!(
                stream.observe(&item("item/started", kind)).unwrap(),
                Observed::RequiresHandling
            ));
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

    #[test]
    fn private_items_are_validated_without_public_projection() {
        for kind in ["userMessage", "reasoning"] {
            let mut stream = started();
            let value = if kind == "userMessage" {
                json!({"type":kind,"id":"i","clientId":null,"content":[{"type":"text","text":"PRIVATE","text_elements":[]}]})
            } else {
                json!({"type":kind,"id":"i","summary":[],"content":["PRIVATE"]})
            };
            let params = json!({"threadId":"t","turnId":"r","item":value});
            assert!(matches!(
                stream
                    .observe(&frame("item/started", params.clone()))
                    .unwrap(),
                Observed::Private
            ));
            if kind == "reasoning" {
                for (method, index) in [
                    ("item/reasoning/textDelta", "contentIndex"),
                    ("item/reasoning/summaryTextDelta", "summaryIndex"),
                    ("item/reasoning/summaryPartAdded", "summaryIndex"),
                ] {
                    let mut body = json!({"threadId":"t","turnId":"r","itemId":"i"});
                    body[index] = json!(0);
                    if !method.ends_with("Added") {
                        body["delta"] = json!("PRIVATE");
                    }
                    assert!(matches!(
                        stream.observe(&frame(method, body)).unwrap(),
                        Observed::Private
                    ));
                }
            }
            assert!(matches!(
                stream.observe(&frame("item/completed", params)).unwrap(),
                Observed::Private
            ));
            stream
                .observe(&turn("turn/completed", "completed"))
                .unwrap();
            assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);
        }
    }

    #[test]
    fn malformed_or_cross_scoped_private_events_fail_permanently() {
        for case in [
            "missing",
            "wrong-type",
            "cross-thread",
            "negative-index",
            "late",
            "request",
        ] {
            let mut stream = started();
            let params = json!({"threadId":"t","turnId":"r","item":{"type":"reasoning","id":"i","summary":[],"content":[]}});
            stream
                .observe(&frame("item/started", params.clone()))
                .unwrap();
            if case == "late" {
                stream.observe(&frame("item/completed", params)).unwrap();
            }
            let mut body = json!({"threadId":"t","turnId":"r","itemId":"i","contentIndex":0,"delta":"PRIVATE"});
            match case {
                "missing" => {
                    body.as_object_mut().unwrap().remove("delta");
                }
                "wrong-type" => body["delta"] = json!({}),
                "cross-thread" => body["threadId"] = json!("other"),
                "negative-index" => body["contentIndex"] = json!(-1),
                _ => {}
            }
            let mut event = json!({"method":"item/reasoning/textDelta","params":body});
            if case == "request" {
                event["id"] = json!(9);
            }
            let error = stream
                .observe(&BoundedObservation::new(event.to_string().into_bytes()).unwrap())
                .err()
                .unwrap();
            assert!(!error.to_string().contains("PRIVATE"));
            assert!(stream.finish().is_err());
        }
    }

    fn approval(request_id: u64) -> BoundedObservation {
        BoundedObservation::new(json!({"id":request_id,"method":"item/commandExecution/requestApproval","params":{"threadId":"t","turnId":"r","itemId":"i","startedAtMs":1,"kind":"command","environmentId":null}}).to_string().into_bytes()).unwrap()
    }

    #[test]
    fn approval_matches_live_tool_context_and_resolves_once() {
        let mut stream = started();
        assert!(matches!(
            stream
                .observe(&item("item/started", "commandExecution"))
                .unwrap(),
            Observed::RequiresHandling
        ));
        let request = stream.pending_approval(&approval(10)).unwrap();
        assert!(
            matches!(request.action,Action::Command {command:Some(ref c),cwd:Some(ref p),..} if c=="echo test"&&p=="/snapshot")
        );
        assert!(matches!(
            stream
                .observe(&frame(
                    "serverRequest/resolved",
                    json!({"threadId":"t","requestId":10})
                ))
                .unwrap(),
            Observed::Private
        ));
        stream
            .observe(&item("item/completed", "commandExecution"))
            .unwrap();
        stream
            .observe(&turn("turn/completed", "completed"))
            .unwrap();
        assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);
    }

    #[test]
    fn approval_wrong_item_scope_duplicate_context_and_resolution_fail_closed() {
        for case in [
            "missing-item",
            "closed-item",
            "duplicate",
            "resolved-reuse",
            "wrong-command",
            "wrong-cwd",
            "wrong-type",
            "pending-completion",
            "unknown-resolution",
            "duplicate-resolution",
        ] {
            let mut stream = started();
            if case != "missing-item" {
                stream
                    .observe(&item(
                        "item/started",
                        if case == "wrong-type" {
                            "agentMessage"
                        } else {
                            "commandExecution"
                        },
                    ))
                    .unwrap();
            }
            if case == "closed-item" {
                stream
                    .observe(&item("item/completed", "commandExecution"))
                    .unwrap();
            }
            let mut event: serde_json::Value = serde_json::from_slice(&approval(10).bytes).unwrap();
            if case == "wrong-command" {
                event["params"]["command"] = json!("other");
            }
            if case == "wrong-cwd" {
                event["params"]["cwd"] = json!("/other");
            }
            let event = BoundedObservation::new(event.to_string().into_bytes()).unwrap();
            let resolved = frame(
                "serverRequest/resolved",
                json!({"threadId":"t","requestId":10}),
            );
            let rejected = match case {
                "duplicate" => {
                    stream.pending_approval(&event).unwrap();
                    stream.pending_approval(&event).is_err()
                }
                "resolved-reuse" => {
                    stream.pending_approval(&event).unwrap();
                    stream.observe(&resolved).unwrap();
                    stream.pending_approval(&event).is_err()
                }
                "pending-completion" => {
                    stream.pending_approval(&event).unwrap();
                    stream
                        .observe(&item("item/completed", "commandExecution"))
                        .is_err()
                }
                "unknown-resolution" => stream.observe(&resolved).is_err(),
                "duplicate-resolution" => {
                    stream.pending_approval(&event).unwrap();
                    stream.observe(&resolved).unwrap();
                    stream.observe(&resolved).is_err()
                }
                _ => stream.pending_approval(&event).is_err(),
            };
            assert!(rejected, "{case}");
            assert!(stream.finish().is_err(), "{case}");
        }
    }

    #[test]
    fn approval_decisions_are_single_use_and_do_not_complete_the_request() {
        for (decision, wire) in [(Decision::Decline, "decline"), (Decision::Cancel, "cancel")] {
            let mut stream = started();
            stream
                .observe(&item("item/started", "commandExecution"))
                .unwrap();
            stream.pending_approval(&approval(10)).unwrap();
            let reply = stream
                .prepare_approval_reply(&RequestId::Number(10), decision)
                .unwrap();
            let value: serde_json::Value = serde_json::from_str(reply.as_str()).unwrap();
            assert_eq!(value, json!({"id":10,"result":{"decision":wire}}));
            assert!(reply.as_str().ends_with('\n'));
            // Preparing a response cannot stand in for the server's resolution.
            assert!(stream
                .observe(&item("item/completed", "commandExecution"))
                .is_err());
            assert!(stream.finish().is_err());
        }
    }

    #[test]
    fn late_unknown_duplicate_and_cross_id_decisions_fail_closed() {
        for case in ["unknown", "duplicate", "resolved", "interrupted", "text-id"] {
            let mut stream = started();
            stream
                .observe(&item("item/started", "commandExecution"))
                .unwrap();
            if case != "unknown" {
                stream.pending_approval(&approval(10)).unwrap();
            }
            match case {
                "duplicate" => {
                    stream
                        .prepare_approval_reply(&RequestId::Number(10), Decision::Decline)
                        .unwrap();
                }
                "resolved" => {
                    stream
                        .observe(&frame(
                            "serverRequest/resolved",
                            json!({"threadId":"t","requestId":10}),
                        ))
                        .unwrap();
                }
                "interrupted" => {
                    stream
                        .observe(&turn("turn/completed", "interrupted"))
                        .unwrap();
                }
                _ => {}
            }
            let id = if case == "text-id" {
                RequestId::Text("10".into())
            } else {
                RequestId::Number(10)
            };
            assert!(
                stream
                    .prepare_approval_reply(&id, Decision::Decline)
                    .is_err(),
                "{case}"
            );
            assert!(stream.finish().is_err());
        }
    }

    #[test]
    fn live_user_accept_cannot_expand_the_unadmitted_sandbox_boundary() {
        let mut stream = started();
        stream
            .observe(&item("item/started", "commandExecution"))
            .unwrap();
        stream.pending_approval(&approval(10)).unwrap();
        assert!(stream
            .prepare_approval_reply(&RequestId::Number(10), Decision::AcceptOnce)
            .is_err());
        assert!(stream
            .prepare_approval_reply(&RequestId::Number(10), Decision::Decline)
            .is_err());
        assert!(stream.finish().is_err());
    }
}
