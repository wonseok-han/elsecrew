//! Public assistant projection for Claude Code stream-json, not launch authority.
//! Source: https://code.claude.com/docs/en/agent-sdk/typescript#sdkassistantmessage
//! The owner must separately handle initialization, tools, and process exit.
//! Result observations are not proof of completion, empty queues, or child cleanup.
//! Partial stream events are not emitted here, avoiding duplicate text delivery.
use super::{BoundedObservation, ProviderError};
use crate::provider::PROVIDER_PUBLIC_TEXT_MAX_BYTES;
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
    Result(ResultObservation),
    /// How the main thread's tool uses ended, for the work history only.
    /// Read leniently: an odd frame is `RequiresHandling`, never a failure.
    ToolResults(Vec<ToolResult>),
    RequiresHandling,
}

/// Display-only facts about one finished tool use; never authority. What the
/// tool printed or read is not kept (review PR #11 B3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolResult {
    pub tool_use_id: String,
    /// None when the result does not say (missing or not a boolean).
    pub is_error: Option<bool>,
    /// The tool was stopped before it finished.
    pub interrupted: bool,
    /// The exit code a failed command reported ("Exit code N").
    pub exit_code: Option<i64>,
    /// Lines added and removed by a file edit (structured patch).
    pub lines: Option<(u64, u64)>,
}

fn tool_results(frame: &serde_json::Value) -> Option<Vec<ToolResult>> {
    if !frame.get("parent_tool_use_id")?.is_null() { return None; }
    let blocks = frame.get("message")?.get("content")?.as_array()?;
    // Claude adds what the tool returned in its own shape next to the message;
    // it is about the (single) tool use the frame answers.
    let extra = frame.get("tool_use_result").filter(|_| blocks.len() == 1);
    let lines = extra.and_then(|extra| extra.get("structuredPatch")).and_then(|v| v.as_array()).map(|hunks| {
        hunks.iter().filter_map(|hunk| hunk.get("lines")?.as_array()).flatten().filter_map(|line| line.as_str())
            .fold((0, 0), |(added, removed), line| if line.starts_with('+') { (added + 1, removed) } else if line.starts_with('-') { (added, removed + 1) } else { (added, removed) })
    }).filter(|counts| *counts != (0, 0)).or_else(|| {
        // A new file written whole has no hunks: every line is added.
        let extra = extra?;
        if extra.get("type")?.as_str()? != "create" { return None; }
        Some((extra.get("content")?.as_str()?.lines().count() as u64, 0))
    });
    let interrupted = extra.and_then(|extra| extra.get("interrupted")).and_then(|v| v.as_bool()).unwrap_or(false);
    let results: Vec<ToolResult> = blocks.iter().filter(|block| block.get("type").and_then(|v| v.as_str()) == Some("tool_result")).filter_map(|block| {
        let tool_use_id = block.get("tool_use_id")?.as_str()?;
        if !identifier(tool_use_id) { return None; }
        // Claude leaves `is_error` out of a clean result; anything but a
        // boolean is unknown (review PR #11 I3).
        let is_error = match block.get("is_error") { None => Some(false), Some(value) => value.as_bool() };
        let first = match block.get("content") {
            Some(serde_json::Value::String(text)) => text.lines().next().unwrap_or(""),
            Some(serde_json::Value::Array(parts)) => parts.first().and_then(|part| part.get("text")?.as_str()).and_then(|text| text.lines().next()).unwrap_or(""),
            _ => "",
        };
        let exit_code = (is_error == Some(true)).then(|| first.strip_prefix("Exit code ")?.trim().parse().ok()).flatten();
        Some(ToolResult { tool_use_id: tool_use_id.into(), is_error, interrupted, exit_code, lines })
    }).collect();
    (!results.is_empty()).then_some(results)
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReportedOutcome {
    Success,
    Failure,
    /// A non-completion reason is not promoted to successful work.
    RequiresHandling,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResultObservation {
    pub outcome: ReportedOutcome,
    /// None means the provider did not report queue state, not an empty queue.
    pub queued_turn_count: Option<u64>,
    pub permission_denied: bool,
    /// Tool uses the result lists as denied; None when an entry carries no id,
    /// so the denial cannot be matched to whoever refused it.
    pub denied_tool_use_ids: Option<BTreeSet<String>>,
}

#[derive(Deserialize)]
struct ResultWire {
    uuid: String,
    session_id: String,
    subtype: String,
    is_error: bool,
    terminal_reason: Option<String>,
    queued_turn_count: Option<u64>,
    permission_denials: Vec<Box<RawValue>>,
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
    budget: crate::provider::ProviderEventBudget,
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
        Self::with_budget(session_id, crate::provider::ProviderEventBudget::LegacyV1)
    }

    pub fn with_budget(session_id: &str, budget: crate::provider::ProviderEventBudget) -> Result<Self, ProviderError> {
        if !identifier(session_id) {
            return Err(invalid());
        }
        Ok(Self {
            budget,
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
        if self.events > self.budget.internal_events()
            || self.bytes > self.budget.observation_bytes()
        {
            return Err(invalid());
        }
        let tag: Tag = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
        if tag.kind == "result" {
            let wire: ResultWire = serde_json::from_slice(&input.bytes).map_err(|_| invalid())?;
            if wire.session_id != self.session_id
                || !identifier(&wire.uuid)
                || !self.seen.insert(wire.uuid)
                || wire
                    .queued_turn_count
                    .is_some_and(|n| n > 9_007_199_254_740_991)
            {
                return Err(invalid());
            }
            let outcome = match wire.subtype.as_str() {
                "success" if wire.is_error => ReportedOutcome::Failure,
                "success"
                    if wire
                        .terminal_reason
                        .as_deref()
                        .is_none_or(|r| r == "completed") =>
                {
                    ReportedOutcome::Success
                }
                "success" => ReportedOutcome::RequiresHandling,
                "error_max_turns"
                | "error_during_execution"
                | "error_max_budget_usd"
                | "error_max_structured_output_retries" => ReportedOutcome::Failure,
                _ => return Err(invalid()),
            };
            #[derive(Deserialize)]
            struct Denial { tool_use_id: String }
            let denied_tool_use_ids = wire.permission_denials.iter()
                .map(|raw| serde_json::from_str::<Denial>(raw.get()).ok()
                    .map(|denial| denial.tool_use_id).filter(|id| identifier(id)))
                .collect::<Option<BTreeSet<_>>>();
            return Ok(Projection::Result(ResultObservation {
                outcome,
                queued_turn_count: wire.queued_turn_count,
                permission_denied: !wire.permission_denials.is_empty(),
                denied_tool_use_ids,
            }));
        }
        if tag.kind == "user" {
            // Bound to this session and each frame once, like the assistant
            // and result frames (review PR #11 I3); otherwise unhandled.
            let frame = input.json().ok();
            let bound = frame.as_ref().is_some_and(|frame| frame.get("session_id").and_then(|v| v.as_str()) == Some(self.session_id.as_str()))
                && frame.as_ref().and_then(|frame| frame.get("uuid")?.as_str().map(String::from)).is_some_and(|uuid| identifier(&uuid) && self.seen.insert(uuid));
            return Ok(frame.filter(|_| bound).and_then(|frame| tool_results(&frame)).map_or(Projection::RequiresHandling, Projection::ToolResults));
        }
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
        if self.public_events > self.budget.public_events()
            || self.public_bytes > self.budget.public_bytes()
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
    #[test]
    fn durable_session_exceeds_legacy_count_without_retaining_raw_frames() {
        let mut parser = super::SessionProjection::with_budget("session", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        for i in 0..3000 {
            let mut value = frame(&format!("uuid-{i}"));
            value["session_id"] = serde_json::Value::String("session".into());
            assert!(parser.project(&observe(&value)).is_ok());
        }
        assert_eq!(parser.seen.len(), 3000);
    }
    use super::*;
    use serde_json::{json, Value};
    fn result() -> Value {
        json!({"type":"result","uuid":"result-a","session_id":"session-a",
            "subtype":"success","is_error":false,"permission_denials":[],
            "result":"DO_NOT_APPEND_AGAIN","errors":["PRIVATE"]})
    }
    #[test]
    fn result_is_a_report_not_public_text_or_queue_and_exit_proof() {
        let mut parser = SessionProjection::new("session-a").unwrap();
        match parser.project(&observe(&result())).unwrap() {
            Projection::Result(observation) => assert_eq!(
                observation,
                ResultObservation {
                    outcome: ReportedOutcome::Success,
                    queued_turn_count: None,
                    permission_denied: false,
                    denied_tool_use_ids: Some(BTreeSet::new()),
                }
            ),
            _ => panic!("expected result observation"),
        }
        // A session may have another queued turn; observing a result does not close it.
        assert!(matches!(
            parser.project(&observe(&frame("uuid-next"))).unwrap(),
            Projection::Assistant { .. }
        ));
        assert!(parser.project(&observe(&result())).is_err());
    }
    #[test]
    fn result_names_each_denied_tool_use_or_none_when_one_is_unnamed() {
        let observe_denials = |denials: Value| {
            let mut value = result();
            value["permission_denials"] = denials;
            let mut parser = SessionProjection::new("session-a").unwrap();
            match parser.project(&observe(&value)).unwrap() {
                Projection::Result(observation) => observation,
                _ => panic!("expected result observation"),
            }
        };
        let named = observe_denials(json!([{"tool_name":"Read","tool_use_id":"tool_a","tool_input":{}}]));
        assert!(named.permission_denied);
        assert_eq!(named.denied_tool_use_ids, Some(BTreeSet::from(["tool_a".to_string()])));
        for unnamed in [json!([{"tool_name":"Read"}]), json!([{"tool_use_id":""}]), json!([{"tool_use_id":7}])] {
            let observation = observe_denials(unnamed);
            assert!(observation.permission_denied);
            assert_eq!(observation.denied_tool_use_ids, None);
        }
    }
    #[test]
    fn result_preserves_failure_attention_and_pending_queue_distinctions() {
        for (patch, expected) in [
            (json!({"is_error":true}), ReportedOutcome::Failure),
            (
                json!({"subtype":"error_max_turns"}),
                ReportedOutcome::Failure,
            ),
            (
                json!({"subtype":"error_during_execution"}),
                ReportedOutcome::Failure,
            ),
            (
                json!({"subtype":"error_max_budget_usd"}),
                ReportedOutcome::Failure,
            ),
            (
                json!({"subtype":"error_max_structured_output_retries"}),
                ReportedOutcome::Failure,
            ),
            (
                json!({"terminal_reason":"tool_deferred"}),
                ReportedOutcome::RequiresHandling,
            ),
            (
                json!({"terminal_reason":"new_reason"}),
                ReportedOutcome::RequiresHandling,
            ),
            (
                json!({"terminal_reason":"completed"}),
                ReportedOutcome::Success,
            ),
        ] {
            let mut wire = result();
            wire.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            wire["queued_turn_count"] = json!(2);
            wire["permission_denials"] = json!([{"tool_input":"SECRET"}]);
            match SessionProjection::new("session-a")
                .unwrap()
                .project(&observe(&wire))
                .unwrap()
            {
                Projection::Result(observation) => {
                    assert_eq!(observation.outcome, expected);
                    assert_eq!(observation.queued_turn_count, Some(2));
                    assert!(observation.permission_denied);
                    assert!(!format!("{observation:?}").contains("SECRET"));
                }
                _ => panic!("expected result observation"),
            }
        }
        for patch in [
            json!({"session_id":"other"}),
            json!({"subtype":"unknown"}),
            json!({"queued_turn_count":-1}),
            json!({"queued_turn_count":9007199254740992_u64}),
            json!({"permission_denials":null}),
            json!({"is_error":null}),
        ] {
            let mut wire = result();
            wire.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            let mut parser = SessionProjection::new("session-a").unwrap();
            assert!(parser.project(&observe(&wire)).is_err());
            assert!(parser.project(&observe(&result())).is_err());
        }
    }
    fn frame(uuid: &str) -> Value {
        json!({"type":"assistant","uuid":uuid,"session_id":"session-a","parent_tool_use_id":null,
        "message":{"id":"message-shared","role":"assistant","content":[
            {"type":"thinking","thinking":"PRIVATE"},
            {"type":"text","text":"공개 답변"},
            {"type":"tool_use","input":{"token":"SECRET"}}
        ]}})
    }
    #[test]
    fn tool_results_say_how_main_thread_tools_ended_for_display_only() {
        let mut parser = SessionProjection::new("session-a").unwrap();
        let user = |uuid: &str, content: Value, extra: Value| json!({"type":"user","uuid":uuid,"session_id":"session-a","parent_tool_use_id":null,
            "message":{"role":"user","content":content},"tool_use_result":extra});
        let bash = user("u1", json!([{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"Exit code 3\nFAKE output line"}]),
            json!({"stdout":"","stderr":"FAKE output line","interrupted":false}));
        match parser.project(&observe(&bash)).unwrap() {
            Projection::ToolResults(results) => assert_eq!(results, vec![ToolResult { tool_use_id: "toolu_1".into(), is_error: Some(true), interrupted: false, exit_code: Some(3), lines: None }]),
            _ => panic!("expected tool results"),
        }
        // Nothing the tool printed is kept (B3).
        assert!(!format!("{:?}", tool_results(&bash)).contains("FAKE output"));
        let failed_text = user("u4", json!([{"type":"tool_result","tool_use_id":"toolu_4","is_error":true,"content":"Exit code 1\nls: missing"}]), json!("Error: Exit code 1\nls: missing"));
        match parser.project(&observe(&failed_text)).unwrap() {
            Projection::ToolResults(results) => assert_eq!(results[0].exit_code, Some(1)),
            _ => panic!("expected tool results"),
        }
        let edit = user("u2", json!([{"type":"tool_result","tool_use_id":"toolu_2","content":[{"type":"text","text":"ok"}]}]),
            json!({"filePath":"/w/a.ts","structuredPatch":[{"oldStart":1,"oldLines":2,"newStart":1,"newLines":3,"lines":[" a","-b","+c","+d"]}]}));
        match parser.project(&observe(&edit)).unwrap() {
            Projection::ToolResults(results) => assert_eq!((results[0].is_error, results[0].lines), (Some(false), Some((2, 1)))),
            _ => panic!("expected tool results"),
        }
        let create = user("u3", json!([{"type":"tool_result","tool_use_id":"toolu_3","content":"done"}]),
            json!({"type":"create","filePath":"/w/n.md","content":"1\n2\n3","structuredPatch":[]}));
        match parser.project(&observe(&create)).unwrap() {
            Projection::ToolResults(results) => assert_eq!(results[0].lines, Some((3, 0)), "a new file: every line added"),
            _ => panic!("expected tool results"),
        }
        // Stopped, or an error flag that is not a boolean: said as such (I3).
        let stopped = user("u5", json!([{"type":"tool_result","tool_use_id":"toolu_5","is_error":false,"content":"x"}]), json!({"stdout":"","interrupted":true}));
        match parser.project(&observe(&stopped)).unwrap() {
            Projection::ToolResults(results) => assert!(results[0].interrupted),
            _ => panic!("expected tool results"),
        }
        let odd = user("u6", json!([{"type":"tool_result","tool_use_id":"toolu_6","is_error":"no","content":"x"}]), json!(null));
        match parser.project(&observe(&odd)).unwrap() {
            Projection::ToolResults(results) => assert_eq!(results[0].is_error, None),
            _ => panic!("expected tool results"),
        }
        // Another session, a repeated frame, subagent results, odd frames and
        // plain user echoes stay unhandled and never fail.
        let mut foreign = user("u7", json!([{"type":"tool_result","tool_use_id":"toolu_1","content":"x"}]), json!(null));
        foreign["session_id"] = json!("session-b");
        for frame in [
            foreign,
            bash.clone(),
            json!({"type":"user","uuid":"u8","session_id":"session-a","parent_tool_use_id":"toolu_9","message":{"content":[{"type":"tool_result","tool_use_id":"x","content":"a"}]}}),
            json!({"type":"user","uuid":"u9","session_id":"session-a","message":"text"}),
            json!({"type":"user","uuid":"u10","session_id":"session-a","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"hi"}]}}),
            json!({"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"x","content":"a"}]}}),
        ] {
            assert!(matches!(parser.project(&observe(&frame)).unwrap(), Projection::RequiresHandling), "{frame}");
        }
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
