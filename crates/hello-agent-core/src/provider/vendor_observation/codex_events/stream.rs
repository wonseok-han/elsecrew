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
    /// Validated lifecycle metadata, not tool permission or public tool output.
    Tool(ToolEvent),
    /// Scoped to a live tool. The owner must obtain a decision; never auto-accept.
    Approval(Box<PendingApproval>),
    /// No public content; not an authorization to ignore a tool/request/policy event.
    RequiresHandling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Command,
    FileChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPhase {
    Started,
    Output,
    Completed,
    Failed,
    Declined,
}

pub struct ToolEvent {
    pub item_id: String,
    pub kind: ToolKind,
    pub phase: ToolPhase,
}

fn tool_kind(kind: &str) -> Option<ToolKind> {
    match kind {
        "commandExecution" => Some(ToolKind::Command),
        "fileChange" => Some(ToolKind::FileChange),
        _ => None,
    }
}

fn tool_phase(raw: &str, started: bool) -> Result<ToolPhase, ProviderError> {
    #[derive(Deserialize)]
    struct Status {
        status: String,
    }
    let status: Status = decode(raw)?;
    match (started, status.status.as_str()) {
        (true, "inProgress") => Ok(ToolPhase::Started),
        (false, "completed") => Ok(ToolPhase::Completed),
        (false, "failed") => Ok(ToolPhase::Failed),
        (false, "declined") => Ok(ToolPhase::Declined),
        _ => Err(invalid()),
    }
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
    diagnostic_stage: &'static str,
    retained_item_bytes: usize,
    budget: crate::provider::ProviderEventBudget,
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
        Self::with_budget(thread_id, turn_id, crate::provider::ProviderEventBudget::LegacyV1)
    }

    pub fn with_budget(thread_id: &str, turn_id: &str, budget: crate::provider::ProviderEventBudget) -> Result<Self, ProviderError> {
        Ok(Self {
            projection: TurnProjection::with_budget(thread_id, turn_id, budget)?,
            started: false,
            outcome: None,
            items: BTreeMap::new(),
            failed: false,
            approvals: BTreeMap::new(),
            prepared_replies: BTreeSet::new(),
            diagnostic_stage: "turn stream state",
            retained_item_bytes: 0,
            budget,
        })
    }

    pub fn observe(&mut self, input: &BoundedObservation) -> Result<Observed, ProviderError> {
        if self.failed || self.outcome.is_some() {
            self.failed = true;
            return Err(invalid());
        }
        // A closed local vocabulary, never an arbitrary method or payload in logs.
        self.diagnostic_stage = match input.json().ok().as_ref()
            .and_then(|value| value.get("method")).and_then(|value| value.as_str()) {
            Some("thread/status/changed") => "turn stream thread status",
            Some("thread/tokenUsage/updated") => "turn stream token usage",
            Some("turn/started") => "turn stream started",
            Some("turn/completed") => "turn stream completed",
            Some("item/started") => "turn stream item started",
            Some("item/completed") => "turn stream item completed",
            Some("item/agentMessage/delta") => "turn stream message delta",
            Some("item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" | "item/reasoning/summaryPartAdded") => "turn stream reasoning",
            Some("serverRequest/resolved") => "turn stream request resolved",
            Some("error") => "turn stream provider error",
            Some("turn/diff/updated") => "turn stream diff metadata",
            Some("turn/plan/updated" | "item/plan/delta") => "turn stream plan metadata",
            Some("thread/name/updated") => "turn stream thread name",
            Some("thread/settings/updated") => "turn stream thread settings",
            Some("hook/started" | "hook/completed") => "turn stream hook",
            Some("model/rerouted" | "model/verification" | "model/safetyBuffering/updated") => "turn stream model metadata",
            Some("warning" | "guardianWarning" | "deprecationNotice" | "configWarning") => "turn stream warning",
            Some("account/updated" | "account/rateLimits/updated") => "turn stream account metadata",
            Some("mcpServer/startupStatus/updated" | "mcpServer/event/stream/notification") => "turn stream mcp metadata",
            Some("turn/moderationMetadata") => "turn stream moderation metadata",
            _ => "turn stream other frame",
        };
        let result = self.observe_inner(input);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub fn diagnostic_stage(&self) -> &'static str { self.diagnostic_stage }

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
        let reply = ApprovalReply::new(request_id, decision).inspect_err(|_| {
            self.failed = true;
        })?;
        self.reserve_approval_response(request_id)?;
        Ok(reply)
    }

    /// Reserve response order only; produces no wire response or permission.
    /// Accept encoding belongs exclusively to the admitted transport owner.
    pub fn reserve_approval_response(
        &mut self,
        request_id: &RequestId,
    ) -> Result<(), ProviderError> {
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
            self.prepared_replies.insert(request_id.clone());
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// A UI poll must discard a request cleared by the server or already answered.
    pub fn awaits_approval_decision(&self, request_id: &RequestId) -> bool {
        !self.failed
            && self.outcome.is_none()
            && !self.prepared_replies.contains(request_id)
            && self
                .approvals
                .get(request_id)
                .and_then(Option::as_ref)
                .and_then(|id| self.items.get(id))
                .is_some_and(|item| !item.completed)
    }

    fn observe_inner(&mut self, input: &BoundedObservation) -> Result<Observed, ProviderError> {
        let item_start = self.diagnostic_stage == "turn stream item started";
        if item_start { self.diagnostic_stage = "turn item start envelope"; }
        #[derive(Deserialize)]
        struct Envelope {
            id: Option<Box<RawValue>>,
        }
        let envelope: Envelope = decode(std::str::from_utf8(&input.bytes).map_err(|_| invalid())?)?;
        if envelope.id.is_some() {
            // Server requests use their own parser and charge the same turn budget.
            return self
                .pending_approval(input)
                .map(Box::new)
                .map(Observed::Approval);
        }
        // Charge all notifications, including non-public ones, exactly once.
        if item_start { self.diagnostic_stage = "turn item start projection"; }
        let projected = self.projection.project(input)?;
        if super::thread_status(input, &self.projection.thread_id)? {
            return Ok(Observed::Private);
        }
        let frame: Notification = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if frame.method == "account/rateLimits/updated" {
            // Global advisory event has no thread/turn identity and may precede
            // turn/started. It consumes the same budget and confers no authority.
            super::rate_limits::validate(frame.params.get())?;
            return Ok(Observed::Private);
        }
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
        if item_start { self.diagnostic_stage = "turn item start before turn"; }
        if !self.started {
            return Err(invalid());
        }
        if frame.method == "thread/tokenUsage/updated" {
            self.check_scope(frame.params.get())?;
            super::usage::validate(frame.params.get())?;
            return Ok(Observed::Private);
        }
        if frame.method == "item/started" || frame.method == "item/completed" {
            if item_start { self.diagnostic_stage = "turn item start scope"; }
            self.check_scope(frame.params.get())?;
            if item_start { self.diagnostic_stage = "turn item start body"; }
            let wire: Completed = decode(frame.params.get())?;
            if item_start { self.diagnostic_stage = "turn item start tag"; }
            let item: ItemTag = decode(wire.item.get())?;
            if item_start { self.diagnostic_stage = "turn item start identity"; }
            if !identifier(&item.id) || !identifier(&item.kind) {
                return Err(invalid());
            }
            let private = matches!(item.kind.as_str(), "userMessage" | "reasoning");
            if item_start { self.diagnostic_stage = "turn item start tool phase"; }
            let tool = tool_kind(&item.kind)
                .map(|kind| {
                    Ok::<_, ProviderError>(ToolEvent {
                        item_id: item.id.clone(),
                        kind,
                        phase: tool_phase(wire.item.get(), frame.method == "item/started")?,
                    })
                })
                .transpose()?;
            if private {
                if item_start {
                    self.diagnostic_stage = if item.kind == "userMessage" {
                        "turn item start user echo shape"
                    } else { "turn item start reasoning shape" };
                }
                super::private::validate_item(wire.item.get())?;
            }
            if frame.method == "item/started" {
                self.diagnostic_stage = "turn item start duplicate";
                if self.items.contains_key(&item.id) {
                    return Err(invalid());
                }
                let public = matches!(item.kind.as_str(), "agentMessage" | "plan");
                let command = if item.kind == "commandExecution" {
                    self.diagnostic_stage = "turn item start command shape";
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
                // Keep lifecycle tombstones for duplicate rejection, but bound their
                // retained strings independently of the larger raw stream budget.
                self.diagnostic_stage = "turn item start retained budget";
                let bytes = item.id.len() + item.kind.len()
                    + command.as_ref().map_or(0, |(command, cwd)| command.len() + cwd.len());
                self.retained_item_bytes = self.retained_item_bytes.checked_add(bytes).ok_or_else(invalid)?;
                if self.budget == crate::provider::ProviderEventBudget::DurableV1
                    && self.retained_item_bytes > 4 * 1024 * 1024 {
                    return Err(invalid());
                }
                // Fixed categories only: never interpolate the untrusted item type.
                self.diagnostic_stage = match item.kind.as_str() {
                    "dynamicToolCall" => "turn item start unsupported dynamic tool",
                    "mcpToolCall" => "turn item start unsupported mcp tool",
                    "collabAgentToolCall" | "collabToolCall" | "subAgentActivity" => "turn item start unsupported collaboration",
                    "hookPrompt" => "turn item start unsupported hook",
                    _ if !public && !private && tool.is_none() => "turn item start unsupported other",
                    _ => "turn stream item started",
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
                } else if let Some(tool) = tool {
                    Observed::Tool(tool)
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
            if let Some((expected_command, expected_cwd)) = &existing.command {
                #[derive(Deserialize)]
                struct Command {
                    command: String,
                    cwd: String,
                }
                let command: Command = decode(wire.item.get())?;
                if &command.command != expected_command || &command.cwd != expected_cwd {
                    return Err(invalid());
                }
            }
            existing.completed = true;
            return Ok(match projected {
                Projection::Completed { .. } => Observed::Public(projected),
                _ if private => Observed::Private,
                _ if tool.is_some() => Observed::Tool(tool.unwrap()),
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
        if let Some(kind) = tool_kind(expected_kind) {
            // Validate the payload shape, but never expose raw stdout/diff here.
            let _: Delta = decode(frame.params.get())?;
            return Ok(Observed::Tool(ToolEvent {
                item_id: reference.item_id,
                kind,
                phase: ToolPhase::Output,
            }));
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
            json!({"threadId":"t","turnId":"r","item":{"id":"i","type":kind,"text":"public","command":"echo test","cwd":"/snapshot","status":if method == "item/started" {"inProgress"} else {"completed"}}}),
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
    fn durable_stream_keeps_ordering_for_three_thousand_deltas() {
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&turn("turn/started", "inProgress")).unwrap();
        stream.observe(&item("item/started", "agentMessage")).unwrap();
        for _ in 0..3000 {
            assert!(matches!(stream.observe(&delta()).unwrap(), Observed::Public(_)));
        }
        stream.observe(&item("item/completed", "agentMessage")).unwrap();
        assert!(stream.observe(&turn("turn/completed", "completed")).is_ok());
        assert!(stream.observe(&delta()).is_err());
    }

    #[test]
    fn durable_stream_retained_item_limit_is_sticky() {
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&turn("turn/started", "inProgress")).unwrap();
        stream.retained_item_bytes = 4 * 1024 * 1024;
        assert!(stream.observe(&item("item/started", "commandExecution")).is_err());
        assert!(stream.observe(&delta()).is_err());
    }
    #[test]
    fn account_limits_are_bounded_advisory_not_auth_or_completion() {
        let body = json!({"rateLimits":{"limitId":"codex","primary":{"usedPercent":31,
            "windowDurationMins":15,"resetsAt":1730948100},"secondary":null,
            "credits":{"hasCredits":false,"unlimited":false,"balance":null},
            "individualLimit":{"limit":"10","used":"2","remainingPercent":80,"resetsAt":10},
            "planType":"future-display-plan","spendControlReached":false}});
        let mut stream = TurnStream::new("t", "r").unwrap();
        for update in [json!({"rateLimits":{}}), body.clone()] {
            assert!(matches!(stream.observe(&frame("account/rateLimits/updated", update)).unwrap(), Observed::Private));
            assert!(!stream.started);
            assert!(stream.outcome.is_none());
            assert!(stream.approvals.is_empty());
        }
        assert!(stream.finish().is_err());
        let mut stream = started();
        assert!(matches!(stream.observe(&frame("account/rateLimits/updated", body.clone())).unwrap(), Observed::Private));
        assert!(matches!(stream.observe(&frame("account/updated", json!({"authMode":null}))).unwrap(), Observed::RequiresHandling));
        for update in [json!({}), json!({"rateLimits":null}), json!({"rateLimits":[]}),
            json!({"rateLimits":{"primary":{}}}),
            json!({"rateLimits":{"primary":{"usedPercent":"31"}}}),
            json!({"rateLimits":{"primary":{"usedPercent":2147483648_i64}}}),
            json!({"rateLimits":{"credits":{"hasCredits":true}}}),
            json!({"rateLimits":{"spendControlReached":"false"}})] {
            let mut stream = started();
            assert!(stream.observe(&frame("account/rateLimits/updated", update)).is_err());
            assert!(stream.observe(&turn("turn/completed", "completed")).is_err());
        }
        for case in ["budget", "after"] {
            let mut stream = started();
            if case == "budget" { stream.projection.events = PROVIDER_INTERNAL_EVENTS_MAX; }
            else { stream.observe(&turn("turn/completed", "completed")).unwrap(); }
            assert!(stream.observe(&frame("account/rateLimits/updated", body.clone())).is_err());
            assert!(stream.finish().is_err());
        }
    }

    #[test]
    fn usage_updates_are_scoped_private_metadata_not_completion() {
        let counts = json!({"totalTokens":12,"inputTokens":10,"cachedInputTokens":2,
            "outputTokens":2,"reasoningOutputTokens":1});
        let body = json!({"threadId":"t","turnId":"r",
            "tokenUsage":{"total":counts,"last":counts,"modelContextWindow":null}});
        let mut stream = started();
        for window in [Value::Null, json!(200000)] {
            let mut update = body.clone();
            update["tokenUsage"]["modelContextWindow"] = window;
            update["tokenUsage"]["last"]["cacheWriteInputTokens"] = json!(0);
            assert!(matches!(stream.observe(&frame("thread/tokenUsage/updated", update)).unwrap(), Observed::Private));
            assert!(stream.outcome.is_none());
            assert!(stream.approvals.is_empty());
        }
        assert!(matches!(stream.observe(&turn("turn/completed", "completed")).unwrap(), Observed::Finished(TurnOutcome::Completed)));
        for case in ["thread", "turn", "missing", "negative", "overflow", "type", "before", "after", "budget"] {
            let mut stream = if case == "before" { TurnStream::new("t", "r").unwrap() } else { started() };
            let mut update = body.clone();
            match case {
                "thread" => update["threadId"] = json!("other"),
                "turn" => update["turnId"] = json!("other"),
                "missing" => { update["tokenUsage"]["last"].as_object_mut().unwrap().remove("inputTokens"); }
                "negative" => update["tokenUsage"]["total"]["totalTokens"] = json!(-1),
                "overflow" => update["tokenUsage"]["modelContextWindow"] = json!(u64::MAX),
                "type" => update["tokenUsage"]["last"]["inputTokens"] = json!("10"),
                "after" => { stream.observe(&turn("turn/completed", "completed")).unwrap(); }
                "budget" => stream.projection.events = PROVIDER_INTERNAL_EVENTS_MAX,
                _ => {}
            }
            assert!(stream.observe(&frame("thread/tokenUsage/updated", update)).is_err(), "{case}");
            assert!(stream.observe(&turn("turn/completed", "completed")).is_err(), "{case}");
        }
    }

    #[test]
    fn diagnostic_labels_do_not_echo_unknown_frames_or_reopen_failures() {
        let mut stream = started();
        assert!(matches!(stream.observe(&frame("PRIVATE_TOKEN", json!({}))).unwrap(), Observed::RequiresHandling));
        assert_eq!(stream.diagnostic_stage(), "turn stream other frame");
        assert!(stream.observe(&frame("item/started", json!({"private":"SECRET"}))).is_err());
        assert_eq!(stream.diagnostic_stage(), "turn item start scope");
        assert!(stream.observe(&turn("turn/completed", "completed")).is_err());
        assert_eq!(stream.diagnostic_stage(), "turn item start scope");
    }

    #[test]
    fn item_start_checkpoints_identify_rejection_without_private_values() {
        for (item, expected) in [
            (json!({"id":"i","type":"userMessage","content":[{"type":"text","text":42}]}), "turn item start user echo shape"),
            (json!({"id":"i","type":"reasoning","content":null}), "turn item start reasoning shape"),
            (json!({"id":"i","type":"commandExecution","status":"SECRET"}), "turn item start tool phase"),
            (json!({"id":"i","type":"commandExecution","status":"inProgress","command":42,"cwd":"SECRET"}), "turn item start command shape"),
            (json!({"id":"","type":"SECRET"}), "turn item start identity"),
            (json!({"id":"i"}), "turn item start tag"),
        ] {
            let mut stream = started();
            let event = frame("item/started", json!({"threadId":"t","turnId":"r","item":item}));
            assert!(stream.observe(&event).is_err());
            assert_eq!(stream.diagnostic_stage(), expected);
            assert!(stream.observe(&delta()).is_err());
            assert_eq!(stream.diagnostic_stage(), expected, "failure checkpoint stays sticky");
        }
        let mut stream = TurnStream::new("t", "r").unwrap();
        assert!(stream.observe(&item("item/started", "agentMessage")).is_err());
        assert_eq!(stream.diagnostic_stage(), "turn item start before turn");
        let mut stream = started();
        stream.observe(&item("item/started", "agentMessage")).unwrap();
        assert!(stream.observe(&item("item/started", "agentMessage")).is_err());
        assert_eq!(stream.diagnostic_stage(), "turn item start duplicate");
    }

    #[test]
    fn unsupported_item_checkpoints_never_authorize_tools_or_echo_unknown_types() {
        for (kind, expected) in [
            ("dynamicToolCall", "turn item start unsupported dynamic tool"),
            ("mcpToolCall", "turn item start unsupported mcp tool"),
            ("collabAgentToolCall", "turn item start unsupported collaboration"),
            ("hookPrompt", "turn item start unsupported hook"),
            ("SECRET_UNKNOWN_TYPE", "turn item start unsupported other"),
        ] {
            let mut stream = started();
            assert!(matches!(stream.observe(&item("item/started", kind)).unwrap(), Observed::RequiresHandling));
            assert_eq!(stream.diagnostic_stage(), expected);
        }
    }

    #[test]
    fn thread_status_neither_finishes_turn_nor_grants_approval() {
        let mut stream = started();
        for status in [json!({"type":"active","activeFlags":["waitingOnApproval"]}),
            json!({"type":"idle"}), json!({"type":"systemError"}), json!({"type":"notLoaded"})] {
            assert!(matches!(stream.observe(&frame("thread/status/changed",
                json!({"threadId":"t","status":status}))).unwrap(), Observed::Private));
            assert!(stream.outcome.is_none());
            assert!(stream.approvals.is_empty());
        }
        assert!(matches!(stream.observe(&turn("turn/completed", "completed")).unwrap(), Observed::Finished(TurnOutcome::Completed)));
        let mut stream = started();
        assert!(stream.observe(&frame("thread/status/changed", json!({"threadId":"other","status":{"type":"idle"}}))).is_err());
        assert!(stream.observe(&turn("turn/completed", "completed")).is_err());
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
    fn tools_are_typed_and_unknown_items_still_require_owner_handling() {
        for kind in ["commandExecution", "fileChange", "futureItem"] {
            let mut stream = started();
            for (method, phase) in [
                ("item/started", ToolPhase::Started),
                ("item/completed", ToolPhase::Completed),
            ] {
                match stream.observe(&item(method, kind)).unwrap() {
                    Observed::Tool(tool) => {
                        assert_eq!(Some(tool.kind), tool_kind(kind));
                        assert_eq!(tool.phase, phase);
                        assert_eq!(tool.item_id, "i");
                    }
                    Observed::RequiresHandling => assert_eq!(kind, "futureItem"),
                    _ => panic!("unexpected public tool projection"),
                }
            }
            stream
                .observe(&turn("turn/completed", "completed"))
                .unwrap();
            assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);
        }
    }

    #[test]
    fn tool_output_is_metadata_only_and_terminal_status_is_preserved() {
        for (kind, method) in [
            ("commandExecution", "item/commandExecution/outputDelta"),
            ("fileChange", "item/fileChange/outputDelta"),
        ] {
            for (status, phase) in [
                ("completed", ToolPhase::Completed),
                ("failed", ToolPhase::Failed),
                ("declined", ToolPhase::Declined),
            ] {
                let mut stream = started();
                stream.observe(&item("item/started", kind)).unwrap();
                let output = frame(
                    method,
                    json!({"threadId":"t","turnId":"r","itemId":"i","delta":"PRIVATE output"}),
                );
                assert!(matches!(
                    stream.observe(&output).unwrap(),
                    Observed::Tool(ToolEvent {
                        phase: ToolPhase::Output,
                        ..
                    })
                ));
                let mut completed: Value =
                    serde_json::from_slice(&item("item/completed", kind).bytes).unwrap();
                completed["params"]["item"]["status"] = json!(status);
                let event = BoundedObservation::new(completed.to_string().into_bytes()).unwrap();
                match stream.observe(&event).unwrap() {
                    Observed::Tool(value) => assert_eq!(value.phase, phase),
                    _ => panic!("tool output entered public stream"),
                }
                assert!(stream.observe(&output).is_err());
                assert!(stream.finish().is_err());
            }
        }
    }

    #[test]
    fn malformed_tool_lifecycle_permanently_fails_the_stream() {
        for case in [
            "start-status",
            "end-status",
            "changed-command",
            "changed-cwd",
            "missing-delta",
            "wrong-item",
            "wrong-kind",
            "wrong-turn",
        ] {
            let mut stream = started();
            let mut event: Value =
                serde_json::from_slice(&item("item/started", "commandExecution").bytes).unwrap();
            if case == "start-status" {
                event["params"]["item"]["status"] = json!("completed");
            } else {
                stream
                    .observe(&item("item/started", "commandExecution"))
                    .unwrap();
                event = serde_json::from_slice(&item("item/completed", "commandExecution").bytes)
                    .unwrap();
                match case {
                    "end-status" => event["params"]["item"]["status"] = json!("inProgress"),
                    "changed-command" => event["params"]["item"]["command"] = json!("PRIVATE"),
                    "changed-cwd" => event["params"]["item"]["cwd"] = json!("/other"),
                    _ => {
                        event = json!({"method":"item/commandExecution/outputDelta","params":{"threadId":"t","turnId":"r","itemId":"i","delta":"PRIVATE"}});
                        match case {
                            "missing-delta" => {
                                event["params"].as_object_mut().unwrap().remove("delta");
                            }
                            "wrong-item" => event["params"]["itemId"] = json!("other"),
                            "wrong-kind" => event["method"] = json!("item/fileChange/outputDelta"),
                            _ => event["params"]["turnId"] = json!("other"),
                        }
                    }
                }
            }
            let error = stream
                .observe(&BoundedObservation::new(event.to_string().into_bytes()).unwrap())
                .err()
                .unwrap();
            assert!(!error.to_string().contains("PRIVATE"));
            assert!(stream
                .observe(&turn("turn/completed", "completed"))
                .is_err());
            assert!(stream.finish().is_err());
        }
    }

    #[test]
    fn omitted_text_elements_echo_remains_private_and_allows_public_reply() {
        let mut stream = started();
        let params = json!({"threadId":"t","turnId":"r","item":{
            "id":"user_echo", "type":"userMessage", "startedAtMs":1793343295000i64,"completedAtMs":null,
            "content":[{"type":"text","text":"PRIVATE"}]
        }});
        for method in ["item/started", "item/completed"] {
            assert!(matches!(stream.observe(&frame(method, params.clone())).unwrap(), Observed::Private));
        }
        assert!(matches!(stream.observe(&item("item/started", "agentMessage")).unwrap(), Observed::ItemStarted));
        assert!(matches!(stream.observe(&delta()).unwrap(), Observed::Public(_)));
        stream.observe(&item("item/completed", "agentMessage")).unwrap();
        stream.observe(&turn("turn/completed", "completed")).unwrap();
        stream.finish().unwrap();
    }

    #[test]
    fn lifecycle_timestamps_are_not_turn_completion_evidence() {
        for kind in ["userMessage", "reasoning"] {
            let mut stream = started();
            let content = if kind == "userMessage" { json!([{ "type":"text", "text":"PRIVATE" }]) } else { json!(["PRIVATE"]) };
            let params = json!({"threadId":"t","turnId":"r","item":{
                "id":"i","type":kind,"content":content,
                "startedAtMs":1793343295000i64,"completedAtMs":1793343295001i64
            }});
            assert!(matches!(stream.observe(&frame("item/started", params)).unwrap(), Observed::Private));
            assert!(stream.finish().is_err());
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
            Observed::Tool(ToolEvent {
                kind: ToolKind::Command,
                phase: ToolPhase::Started,
                ..
            })
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
    fn unified_dispatch_preserves_approval_identity_and_single_use() {
        let mut stream = started();
        stream
            .observe(&item("item/started", "commandExecution"))
            .unwrap();
        let request = match stream.observe(&approval(10)).unwrap() {
            Observed::Approval(request) => request,
            _ => panic!("approval was not dispatched"),
        };
        assert!(request.request_id == RequestId::Number(10));
        assert!(stream.awaits_approval_decision(&request.request_id));
        let reply = stream
            .prepare_approval_reply(&request.request_id, Decision::Decline)
            .unwrap();
        assert!(!stream.awaits_approval_decision(&request.request_id));
        assert_eq!(
            serde_json::from_str::<Value>(reply.as_str()).unwrap(),
            json!({"id":10,"result":{"decision":"decline"}})
        );
        stream
            .observe(&frame(
                "serverRequest/resolved",
                json!({"threadId":"t","requestId":10}),
            ))
            .unwrap();
        stream
            .observe(&item("item/completed", "commandExecution"))
            .unwrap();
        stream
            .observe(&turn("turn/completed", "completed"))
            .unwrap();
        assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);

        for case in ["duplicate", "unknown", "response", "missing-tool"] {
            let mut stream = started();
            if case != "missing-tool" {
                stream
                    .observe(&item("item/started", "commandExecution"))
                    .unwrap();
            }
            if case == "duplicate" {
                stream.observe(&approval(10)).unwrap();
            }
            let mut event: Value = serde_json::from_slice(&approval(10).bytes).unwrap();
            if case == "unknown" {
                event["method"] = json!("future/requestApproval");
            }
            if case == "response" {
                event["result"] = json!({});
            }
            assert!(stream
                .observe(&BoundedObservation::new(event.to_string().into_bytes()).unwrap())
                .is_err());
            assert!(stream.finish().is_err());
        }
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
    fn confined_accept_is_single_request_and_still_needs_server_resolution() {
        let mut stream = started();
        stream
            .observe(&item("item/started", "commandExecution"))
            .unwrap();
        stream.pending_approval(&approval(10)).unwrap();
        stream
            .reserve_approval_response(&RequestId::Number(10))
            .unwrap();
        assert!(!stream.awaits_approval_decision(&RequestId::Number(10)));
        assert!(stream
            .reserve_approval_response(&RequestId::Number(10))
            .is_err());
        assert!(stream.finish().is_err());
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
