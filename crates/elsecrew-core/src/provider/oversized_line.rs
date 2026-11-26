//! A session's provider stdout line over the observation line bound
//! (2026-11-26 D2). An agent that reads a large image or file gets it back as
//! one stream-json/JSON-RPC line (a 55 KB PNG became a ~148 KB line), which
//! used to end the session. In session mode the transport reads such a line up
//! to `PROVIDER_SESSION_LINE_RAW_MAX_BYTES` and this module shortens its long
//! strings so the parsers see an ordinary bounded observation.
//!
//! Shortening is for observation and display only. Every value an approval
//! uses (what the user is shown, the approval fingerprint, the input sent back
//! to the provider) must stay the provider's original text (PR #17 review B1):
//! - Claude: approvals come only from `control_request` frames, which are never
//!   shortened (the allow reply echoes their input as `updatedInput`). Only
//!   frames the session reads for the work history are shortened: tool results
//!   (`user`) and the private blocks of an assistant frame; public text stays.
//! - Codex: an approval is completed from the command and working folder its
//!   `item/started` notification carried, and the completion is compared with
//!   them. So only fields named here as discardable are shortened, per method;
//!   every other field, and every request or response, stays exact.
//!
//! A frame that cannot be brought under the bound this way still fails, as
//! before.
use super::{unique_json, BoundedObservation, PROVIDER_OBSERVATION_LINE_MAX_BYTES};
use serde_json::{Map, Value};

/// The largest raw line a session connection buffers before shortening it.
/// Anything larger still fails. How many such lines may be held at once
/// across all sessions is the transport's budget, not this module's.
pub const PROVIDER_SESSION_LINE_RAW_MAX_BYTES: usize = 8 * 1024 * 1024;
/// What one session connection may stream in each direction in total. It
/// matches the durable session output budget (`ProviderEventBudget::DurableV1`),
/// which the session parsers still enforce on what they observe. The 1 MiB
/// connection total fits a login or preflight exchange, not an agent session
/// that reads several images. It counts bytes; it does not hold them.
pub const PROVIDER_SESSION_STREAM_MAX_BYTES: usize = 256 * 1024 * 1024;
/// Most JSON values (objects, arrays, scalars) an oversized line may contain.
/// A line of many small values costs far more to parse than one long string
/// of the same size, so it is refused before any tree is built (review I1).
pub const PROVIDER_SESSION_LINE_MAX_VALUES: usize = 64 * 1024;
/// Strings up to this size are kept whole.
const KEEP_STRING_BYTES: usize = 4 * 1024;
/// How much of a shortened string stays, so activity can still name a target.
const KEPT_PREFIX_BYTES: usize = 256;
/// Most lines whose count can be kept when the text itself is dropped.
const KEPT_LINE_COUNT_MAX: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OversizedLinePolicy {
    ClaudeStreamJson,
    CodexAppServer,
}

/// Why a line could not be shortened. Fixed names for diagnostics: never the
/// provider's text (review M2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OversizedLineRefusal {
    OverRawBound,
    TooManyValues,
    NotJson,
    ProtectedFrame,
    UnknownShape,
    StillOverBound,
}

impl OversizedLineRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OverRawBound => "over the session line bound",
            Self::TooManyValues => "too many JSON values",
            Self::NotJson => "not unique-key JSON",
            Self::ProtectedFrame => "a frame that is never shortened",
            Self::UnknownShape => "no discardable field for this frame",
            Self::StillOverBound => "still over the line bound after shortening",
        }
    }
}

use OversizedLineRefusal as Refusal;

/// `line` is one complete frame, with or without its trailing newline.
pub fn reduce_oversized_line(
    line: &[u8],
    policy: OversizedLinePolicy,
) -> Result<BoundedObservation, OversizedLineRefusal> {
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    if body.len() > PROVIDER_SESSION_LINE_RAW_MAX_BYTES || body.contains(&b'\n') {
        return Err(Refusal::OverRawBound);
    }
    if count_values(body) > PROVIDER_SESSION_LINE_MAX_VALUES {
        return Err(Refusal::TooManyValues);
    }
    // Duplicate keys are refused before anything is rewritten, as for any frame.
    let mut root = unique_json::parse(body).map_err(|_| Refusal::NotJson)?;
    let object = root.as_object_mut().ok_or(Refusal::UnknownShape)?;
    match policy {
        OversizedLinePolicy::ClaudeStreamJson => reduce_claude(object)?,
        OversizedLinePolicy::CodexAppServer => reduce_codex(object)?,
    }
    let mut bytes = serde_json::to_vec(&root).map_err(|_| Refusal::NotJson)?;
    if bytes.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
        return Err(Refusal::StillOverBound);
    }
    bytes.push(b'\n');
    BoundedObservation::new(bytes).map_err(|_| Refusal::StillOverBound)
}

/// An upper bound on the JSON values in `body`, counted without parsing:
/// every `{`, `[`, `,` and `:` outside a string starts at most one value. A
/// value is also counted for the root. Strings are skipped with their escapes.
fn count_values(body: &[u8]) -> usize {
    let mut count = 1;
    let mut in_string = false;
    let mut escaped = false;
    for byte in body {
        if in_string {
            if escaped { escaped = false; } else if *byte == b'\\' { escaped = true; } else if *byte == b'"' { in_string = false; }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' | b',' | b':' => count += 1,
            _ => {}
        }
    }
    count
}

fn reduce_claude(object: &mut Map<String, Value>) -> Result<(), OversizedLineRefusal> {
    let kind = object.get("type").and_then(Value::as_str).ok_or(Refusal::UnknownShape)?;
    let subtype = object.get("subtype").and_then(Value::as_str);
    match (kind, subtype) {
        // Authority and handshake frames are read exactly or not at all.
        ("control_request" | "control_cancel_request" | "control_response" | "result", _)
        | ("system", Some("init")) => Err(Refusal::ProtectedFrame),
        ("assistant", _) => {
            for (key, value) in object.iter_mut() {
                if key != "message" {
                    shorten(value);
                    continue;
                }
                let Some(message) = value.as_object_mut() else { return Err(Refusal::UnknownShape) };
                for (key, value) in message.iter_mut() {
                    if key != "content" {
                        shorten(value);
                        continue;
                    }
                    let Some(blocks) = value.as_array_mut() else { return Err(Refusal::UnknownShape) };
                    for block in blocks {
                        let is_text = block.get("type").and_then(Value::as_str) == Some("text");
                        match block.as_object_mut() {
                            // Public text is kept exactly; its own bound applies later.
                            Some(fields) if is_text => {
                                for (key, value) in fields.iter_mut() {
                                    if key != "text" { shorten(value); }
                                }
                            }
                            _ => shorten(block),
                        }
                    }
                }
            }
            Ok(())
        }
        // Tool results (`user`) and frames the session ignores.
        _ => {
            let created = created_line_count(object);
            for value in object.values_mut() { shorten(value); }
            if let Some(result) = object.get_mut("tool_use_result").and_then(Value::as_object_mut) {
                match created {
                    // Only the count survives: one empty line each, no content.
                    Some(Some(lines)) => { result.insert("content".into(), Value::String("\n".repeat(lines as usize))); }
                    // Too many to keep: no count is better than a wrong one (review M1).
                    Some(None) => { result.remove("content"); }
                    None => {}
                }
            }
            Ok(())
        }
    }
}

/// The work history counts the lines of a file written whole from the tool
/// result's `content`. `Some(None)`: that content is dropped and its count
/// cannot be kept.
fn created_line_count(object: &Map<String, Value>) -> Option<Option<u64>> {
    let extra = object.get("tool_use_result")?.as_object()?;
    if extra.get("type")?.as_str()? != "create" { return None; }
    let content = extra.get("content")?.as_str()?;
    if content.len() <= KEEP_STRING_BYTES { return None; }
    Some(Some(content.lines().count() as u64).filter(|lines| *lines <= KEPT_LINE_COUNT_MAX))
}

fn reduce_codex(object: &mut Map<String, Value>) -> Result<(), OversizedLineRefusal> {
    // Requests (approvals) and responses carry an id: never shortened.
    if object.contains_key("id") {
        return Err(Refusal::ProtectedFrame);
    }
    let method = object.get("method").and_then(Value::as_str).ok_or(Refusal::UnknownShape)?.to_owned();
    let params = object.get_mut("params").and_then(Value::as_object_mut).ok_or(Refusal::UnknownShape)?;
    match method.as_str() {
        // What a command printed. Its command and working folder are what an
        // approval and the completion check use, so they are never touched.
        "item/started" | "item/completed" => {
            let item = params.get_mut("item").and_then(Value::as_object_mut).ok_or(Refusal::UnknownShape)?;
            match item.get("type").and_then(Value::as_str) {
                Some("commandExecution") => {
                    if let Some(output) = item.get_mut("aggregatedOutput") { shorten_string(output); }
                }
                Some("fileChange") => shorten_change_diffs(item),
                _ => return Err(Refusal::UnknownShape),
            }
        }
        "turn/diff/updated" => shorten_string(params.get_mut("diff").ok_or(Refusal::UnknownShape)?),
        // Tool output deltas are never shown; message and plan deltas are
        // public text and stay exact (review I4).
        "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
            shorten_string(params.get_mut("delta").ok_or(Refusal::UnknownShape)?)
        }
        _ => return Err(Refusal::UnknownShape),
    }
    Ok(())
}

/// The work history counts each file change's added and removed lines from
/// its `diff`. A long diff is replaced by one that counts the same, or, when
/// that would not fit, removed so the count reads as unknown (review M1).
fn shorten_change_diffs(item: &mut Map<String, Value>) {
    let Some(changes) = item.get_mut("changes").and_then(Value::as_array_mut) else { return };
    for change in changes {
        let Some(fields) = change.as_object_mut() else { continue };
        let Some(diff) = fields.get("diff").and_then(Value::as_str) else { continue };
        if diff.len() <= KEEP_STRING_BYTES { continue; }
        let kind = fields.get("kind").and_then(|kind| kind.get("type")).and_then(Value::as_str).unwrap_or("");
        let (added, removed) = crate::provider::vendor_observation::codex_events::stream::diff_counts(kind, diff);
        if added + removed <= KEPT_LINE_COUNT_MAX {
            // One hunk of bare `+`/`-` lines counts the same for every change kind.
            let compact = format!("@@\n{}{}", "+\n".repeat(added as usize), "-\n".repeat(removed as usize));
            fields.insert("diff".into(), Value::String(compact));
        } else {
            fields.remove("diff");
        }
    }
}

fn shorten_string(value: &mut Value) {
    if let Value::String(text) = value {
        if text.len() > KEEP_STRING_BYTES {
            let mut end = KEPT_PREFIX_BYTES;
            while !text.is_char_boundary(end) { end -= 1; }
            let omitted = text.len() - end;
            text.truncate(end);
            text.push_str(&format!("…[{omitted} bytes omitted]"));
        }
    }
}

fn shorten(value: &mut Value) {
    match value {
        Value::String(_) => shorten_string(value),
        Value::Array(values) => values.iter_mut().for_each(shorten),
        Value::Object(fields) => fields.values_mut().for_each(shorten),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::vendor_observation::{claude_events, claude_session};
    use serde_json::json;

    fn line(value: &Value) -> Vec<u8> {
        let mut bytes = value.to_string().into_bytes();
        bytes.push(b'\n');
        bytes
    }

    fn image_result(bytes: usize) -> Value {
        json!({"type":"user","uuid":"u1","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1",
                "content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"A".repeat(bytes)}}]}]},
            "tool_use_result":{"type":"image","file":{"base64":"A".repeat(bytes),"type":"image/png"}}})
    }

    #[test]
    fn a_large_image_tool_result_is_shortened_to_a_bounded_observation() {
        let raw = line(&image_result(148 * 1024));
        assert!(raw.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        assert!(BoundedObservation::new(raw.clone()).is_err(), "the old bound refuses it");
        let reduced = reduce_oversized_line(&raw, OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let value: Value = reduced.json().unwrap();
        let data = value["message"]["content"][0]["content"][0]["source"]["data"].as_str().unwrap();
        assert!(data.len() < 1024 && data.ends_with("bytes omitted]"), "{data}");
        assert_eq!(value["message"]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(value["type"], "user");
    }

    #[test]
    fn a_large_new_file_keeps_its_line_count_and_an_error_keeps_its_exit_code() {
        let file = "line of the new file\n".repeat(10_000);
        let frame = json!({"type":"user","uuid":"u2","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_3","content":"done"}]},
            "tool_use_result":{"type":"create","filePath":"big.txt","content":file}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let mut projection = claude_events::SessionProjection::new("s").unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::ToolResults(results) => assert_eq!(results[0].lines, Some((10_000, 0))),
            _ => panic!("expected tool results"),
        }
        let output = format!("Exit code 3\n{}", "noise ".repeat(40_000));
        let frame = json!({"type":"user","uuid":"u3","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_4","is_error":true,"content":output}]},
            "tool_use_result":{"stdout":"","stderr":output,"interrupted":false}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::ToolResults(results) => assert_eq!(results[0].exit_code, Some(3)),
            _ => panic!("expected tool results"),
        }
    }

    #[test]
    fn assistant_public_text_is_kept_exactly_and_only_private_blocks_shrink() {
        let text = "보고 ".repeat(5000);
        let frame = json!({"type":"assistant","uuid":"a1","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m1","role":"assistant","content":[
                {"type":"text","text":text},
                {"type":"tool_use","id":"toolu_2","name":"Write","input":{"file_path":"big.txt","content":"x".repeat(200_000)}}]}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let mut projection = claude_events::SessionProjection::new("s").unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::Assistant { text: projected, has_private_blocks, .. } => {
                assert_eq!(projected, text);
                assert!(has_private_blocks);
            }
            _ => panic!("expected the assistant text"),
        }
    }

    #[test]
    fn permission_requests_handshake_and_results_are_never_shortened() {
        let big = "x".repeat(200_000);
        for frame in [
            json!({"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"a","content":big}}}),
            json!({"type":"control_cancel_request","request_id":"r1","pad":big}),
            json!({"type":"control_response","response":{"pad":big}}),
            json!({"type":"result","subtype":"success","result":big}),
            json!({"type":"system","subtype":"init","pad":big}),
        ] {
            assert!(reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).is_err(), "{}", frame["type"]);
        }
    }

    #[test]
    fn duplicate_keys_bad_json_and_huge_lines_still_fail() {
        let big = "A".repeat(200_000);
        let duplicate = format!(r#"{{"type":"user","type":"user","pad":"{big}"}}"#);
        assert!(reduce_oversized_line(duplicate.as_bytes(), OversizedLinePolicy::ClaudeStreamJson).is_err());
        assert!(reduce_oversized_line(b"{not json", OversizedLinePolicy::ClaudeStreamJson).is_err());
        let huge = line(&image_result(PROVIDER_SESSION_LINE_RAW_MAX_BYTES));
        assert!(reduce_oversized_line(&huge, OversizedLinePolicy::ClaudeStreamJson).is_err());
        // Many small strings cannot be shortened under the bound: still refused.
        let many = json!({"type":"user","items":vec!["x".repeat(100); 2000]});
        assert!(reduce_oversized_line(&line(&many), OversizedLinePolicy::ClaudeStreamJson).is_err());
    }

    #[test]
    fn codex_notifications_shrink_but_requests_and_public_text_do_not() {
        let big = "B".repeat(300_000);
        let completed = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"i","type":"commandExecution","command":"cat big.log","aggregatedOutput":big,"status":"completed"}}});
        let reduced = reduce_oversized_line(&line(&completed), OversizedLinePolicy::CodexAppServer).unwrap();
        let value = reduced.json().unwrap();
        assert_eq!(value["params"]["item"]["command"], "cat big.log");
        assert!(value["params"]["item"]["aggregatedOutput"].as_str().unwrap().ends_with("bytes omitted]"));
        // A large file change keeps its added/removed line counts.
        let patch = format!("@@ -1,2 +1,3 @@\n{}{}", "+added line\n".repeat(9000), "-removed line\n".repeat(4000));
        let change = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"c","type":"fileChange","status":"completed","changes":[
                {"path":"src/big.ts","kind":{"type":"update"},"diff":patch},
                {"path":"src/new.ts","kind":{"type":"add"},"diff":"new line\n".repeat(12_000)},
                {"path":"src/small.ts","kind":{"type":"update"},"diff":"@@\n+a\n"}]}}});
        let value = reduce_oversized_line(&line(&change), OversizedLinePolicy::CodexAppServer).unwrap().json().unwrap();
        let counts = |index: usize| {
            let change = &value["params"]["item"]["changes"][index];
            crate::provider::vendor_observation::codex_events::stream::diff_counts(
                change["kind"]["type"].as_str().unwrap(), change["diff"].as_str().unwrap())
        };
        assert_eq!(counts(0), (9000, 4000));
        assert_eq!(counts(1), (12_000, 0));
        assert_eq!(value["params"]["item"]["changes"][2]["diff"], "@@\n+a\n");
        assert_eq!(value["params"]["item"]["changes"][0]["path"], "src/big.ts");
        // Too many lines to keep a count: the diff is dropped, so the count reads
        // as unknown instead of being counted from a prefix (review M1).
        let huge = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"c","type":"fileChange","status":"completed","changes":[
                {"path":"src/huge.ts","kind":{"type":"add"},"diff":"x\n".repeat(40_000)}]}}});
        let value = reduce_oversized_line(&line(&huge), OversizedLinePolicy::CodexAppServer).unwrap().json().unwrap();
        assert!(value["params"]["item"]["changes"][0].get("diff").is_none());
        assert_eq!(value["params"]["item"]["changes"][0]["path"], "src/huge.ts");
        let diff = json!({"method":"turn/diff/updated","params":{"threadId":"t","turnId":"r","diff":big}});
        assert!(reduce_oversized_line(&line(&diff), OversizedLinePolicy::CodexAppServer).is_ok());
        let approval = json!({"id":7,"method":"item/fileChange/requestApproval","params":{"pad":big}});
        assert!(reduce_oversized_line(&line(&approval), OversizedLinePolicy::CodexAppServer).is_err());
        let response = json!({"id":7,"result":{"pad":big}});
        assert!(reduce_oversized_line(&line(&response), OversizedLinePolicy::CodexAppServer).is_err());
        // An agent message whose own text is over the bound is not shortened.
        let message = json!({"method":"item/completed","params":{"item":{"id":"i","type":"agentMessage","text":big}}});
        assert_eq!(reduce_oversized_line(&line(&message), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
        // Nothing outside the named discardable fields is shortened: an item kind
        // or method without one, or a large non-discardable field, still fails.
        let unknown = json!({"method":"item/completed","params":{"item":{"id":"i","type":"webSearch","query":big}}});
        assert_eq!(reduce_oversized_line(&line(&unknown), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
        let method = json!({"method":"future/notification","params":{"pad":big}});
        assert_eq!(reduce_oversized_line(&line(&method), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
        let actions = json!({"method":"item/started","params":{"threadId":"t","turnId":"r","item":{"id":"i","type":"commandExecution",
            "command":"ls","cwd":"/w","status":"inProgress","commandActions":[{"type":"unknown","command":big}]}}});
        assert_eq!(reduce_oversized_line(&line(&actions), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::StillOverBound);
    }

    fn codex_frame(value: &Value) -> BoundedObservation {
        let raw = line(value);
        if raw.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
            reduce_oversized_line(&raw, OversizedLinePolicy::CodexAppServer).unwrap()
        } else {
            BoundedObservation::new(raw).unwrap()
        }
    }

    /// Review B1: a Codex approval is completed from the command and working
    /// folder of the item's start notification, and the completion is compared
    /// with them. Both notifications are oversized here (their output is), and
    /// the command and folder are longer than the shortening threshold: the
    /// approval and the completion must still see the original text.
    #[test]
    fn codex_approval_and_completion_use_the_original_command_after_shortening() {
        use crate::provider::vendor_observation::codex_approval::Action;
        use crate::provider::vendor_observation::codex_events::stream::{Observed, TurnOutcome, TurnStream};
        let command = format!("printf '%s' {}", "c".repeat(10_000));
        let cwd = format!("/snapshot/{}", "d".repeat(6_000));
        assert!(command.len() > KEEP_STRING_BYTES && cwd.len() > KEEP_STRING_BYTES);
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&codex_frame(&json!({"method":"turn/started","params":{"threadId":"t","turn":{"id":"r","status":"inProgress","items":[],"error":null}}}))).unwrap();
        let item = |status: &str, output: usize| json!({"method":if status == "inProgress" {"item/started"} else {"item/completed"},
            "params":{"threadId":"t","turnId":"r","item":{"id":"i","type":"commandExecution","command":command,"cwd":cwd,
                "status":status,"aggregatedOutput":"o".repeat(output)}}});
        let started = item("inProgress", 200_000);
        assert!(line(&started).len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        match stream.observe(&codex_frame(&started)).unwrap() {
            Observed::Tool(tool) => assert_eq!(tool.command.as_deref(), Some(command.as_str())),
            _ => panic!("expected the command start"),
        }
        // The request names the command itself: compared with the original.
        let request = json!({"id":10,"method":"item/commandExecution/requestApproval","params":{"threadId":"t","turnId":"r",
            "itemId":"i","startedAtMs":1,"kind":"command","environmentId":null,"command":command,"cwd":cwd}});
        let approval = match stream.observe(&codex_frame(&request)).unwrap() {
            Observed::Approval(approval) => approval,
            _ => panic!("expected the approval"),
        };
        // What the user is shown and what the fingerprint binds is the original.
        match &approval.action {
            Action::Command { command: Some(shown), cwd: Some(folder), .. } => {
                assert_eq!(shown, &command);
                assert_eq!(folder, &cwd);
            }
            _ => panic!("expected a command approval"),
        }
        stream.observe(&codex_frame(&json!({"method":"serverRequest/resolved","params":{"threadId":"t","requestId":10}}))).unwrap();
        match stream.observe(&codex_frame(&item("completed", 300_000))).unwrap() {
            Observed::Tool(tool) => assert_eq!(tool.command.as_deref(), Some(command.as_str())),
            _ => panic!("expected the command end"),
        }
        stream.observe(&codex_frame(&json!({"method":"turn/completed","params":{"threadId":"t","turn":{"id":"r","status":"completed","items":[],"error":null}}}))).unwrap();
        assert_eq!(stream.finish().unwrap(), TurnOutcome::Completed);
    }

    /// The same start with a request that omits the command: the approval is
    /// filled from the original, never from a shortened copy.
    #[test]
    fn codex_approval_filled_from_the_start_keeps_the_original_command() {
        use crate::provider::vendor_observation::codex_approval::Action;
        use crate::provider::vendor_observation::codex_events::stream::{Observed, TurnStream};
        let command = format!("echo {}", "e".repeat(20_000));
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&codex_frame(&json!({"method":"turn/started","params":{"threadId":"t","turn":{"id":"r","status":"inProgress","items":[],"error":null}}}))).unwrap();
        stream.observe(&codex_frame(&json!({"method":"item/started","params":{"threadId":"t","turnId":"r","item":{"id":"i",
            "type":"commandExecution","command":command,"cwd":"/w","status":"inProgress","aggregatedOutput":"o".repeat(200_000)}}}))).unwrap();
        let request = json!({"id":11,"method":"item/commandExecution/requestApproval","params":{"threadId":"t","turnId":"r",
            "itemId":"i","startedAtMs":1,"kind":"command","environmentId":null}});
        let Observed::Approval(approval) = stream.observe(&codex_frame(&request)).unwrap() else { panic!("expected the approval") };
        assert!(matches!(&approval.action, Action::Command { command: Some(shown), .. } if shown == &command));
    }

    /// Review I4: a large tool output delta is shortened; a message delta, which
    /// is public text, never is.
    #[test]
    fn codex_tool_output_deltas_shrink_and_message_deltas_do_not() {
        use crate::provider::vendor_observation::codex_events::stream::{Observed, TurnStream};
        let mut stream = TurnStream::with_budget("t", "r", crate::provider::ProviderEventBudget::DurableV1).unwrap();
        stream.observe(&codex_frame(&json!({"method":"turn/started","params":{"threadId":"t","turn":{"id":"r","status":"inProgress","items":[],"error":null}}}))).unwrap();
        stream.observe(&codex_frame(&json!({"method":"item/started","params":{"threadId":"t","turnId":"r","item":{"id":"i",
            "type":"commandExecution","command":"cat big.log","cwd":"/w","status":"inProgress"}}}))).unwrap();
        for method in ["item/commandExecution/outputDelta"] {
            let delta = json!({"method":method,"params":{"threadId":"t","turnId":"r","itemId":"i","delta":"z".repeat(400_000)}});
            assert!(matches!(stream.observe(&codex_frame(&delta)).unwrap(), Observed::Tool(_)));
        }
        let file = json!({"method":"item/fileChange/outputDelta","params":{"threadId":"t","turnId":"r","itemId":"f","delta":"z".repeat(400_000)}});
        assert!(reduce_oversized_line(&line(&file), OversizedLinePolicy::CodexAppServer).is_ok());
        let message = json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"r","itemId":"m","delta":"z".repeat(200_000)}});
        assert_eq!(reduce_oversized_line(&line(&message), OversizedLinePolicy::CodexAppServer).unwrap_err(), OversizedLineRefusal::UnknownShape);
    }

    /// Review B1, Claude side: an approval comes only from a `control_request`,
    /// which is never shortened, even after shortened frames about the same
    /// tool use. The allow reply echoes the original input exactly.
    #[test]
    fn claude_allow_echoes_the_original_input_after_shortened_frames() {
        use crate::provider::vendor_observation::claude_approval::Observation;
        let expected = crate::provider::vendor_observation::claude_init::ExpectedInit {
            cwd: test_path!("/snapshot"), model: "model", permission_mode: "default", tools: &["Write"],
        };
        let mut session = claude_session::Session::new(&expected);
        let init = json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":test_path!("/snapshot"),"model":"model","permissionMode":"default","tools":["Write"],"mcp_servers":[],"skills":[],"plugins":[]});
        session.observe(&BoundedObservation::new(line(&init)).unwrap()).unwrap();
        let content = "a line of the file being written\n".repeat(3_000);
        // The assistant frame naming the tool use is oversized and shortened.
        let assistant = json!({"type":"assistant","uuid":"a1","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m1","role":"assistant","content":[
                {"type":"tool_use","id":"toolu_w","name":"Write","input":{"file_path":"notes.md","content":content.repeat(2)}}]}});
        let reduced = reduce_oversized_line(&line(&assistant), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        session.observe(&reduced).unwrap();
        // The permission request itself fits and is read exactly.
        let request = json!({"type":"control_request","request_id":"perm_w","request":{"subtype":"can_use_tool",
            "tool_name":"Write","input":{"file_path":"notes.md","content":content},"tool_use_id":"toolu_w"}});
        let raw = line(&request);
        assert!(raw.len() < PROVIDER_OBSERVATION_LINE_MAX_BYTES && content.len() > KEEP_STRING_BYTES);
        let claude_session::Event::Permission(Observation::Requested(asked)) = session.observe(&BoundedObservation::new(raw).unwrap()).unwrap()
            else { panic!("expected the permission request") };
        let reply = session.allow_once(&asked).unwrap();
        let wire: Value = serde_json::from_str(reply.as_str()).unwrap();
        assert_eq!(wire["response"]["response"]["updatedInput"]["content"].as_str(), Some(content.as_str()));
        // The same request over the bound is refused, never shortened.
        let mut over = request.clone();
        over["request"]["input"]["content"] = Value::String(content.repeat(2));
        assert_eq!(reduce_oversized_line(&line(&over), OversizedLinePolicy::ClaudeStreamJson).unwrap_err(), OversizedLineRefusal::ProtectedFrame);
    }

    /// Review M1: a new file with too many lines to keep a count drops the
    /// content, so the work history shows no count rather than a wrong one.
    #[test]
    fn a_new_file_over_the_count_cap_has_no_line_count() {
        let frame = json!({"type":"user","uuid":"u9","session_id":"s","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_9","content":"done"}]},
            "tool_use_result":{"type":"create","filePath":"huge.txt","content":"x\n".repeat(40_000)}});
        let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let mut projection = claude_events::SessionProjection::new("s").unwrap();
        match projection.project(&reduced).unwrap() {
            claude_events::Projection::ToolResults(results) => assert_eq!(results[0].lines, None),
            _ => panic!("expected tool results"),
        }
    }

    /// Review I1: shortening runs on the session's polling thread. A line near
    /// the raw bound is handled well within a poll's budget even in a debug build.
    #[test]
    fn a_line_near_the_raw_bound_is_shortened_quickly() {
        let frame = image_result(3 * 1024 * 1024 + 900 * 1024);
        let raw = line(&frame);
        assert!(raw.len() > 7 * 1024 * 1024 && raw.len() < PROVIDER_SESSION_LINE_RAW_MAX_BYTES);
        let started = std::time::Instant::now();
        let reduced = reduce_oversized_line(&raw, OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let took = started.elapsed();
        assert!(reduced.byte_len() < 4096);
        eprintln!("near-bound line ({} bytes) shortened in {took:?}", raw.len());
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }

    /// Review I1: a line of many small values costs far more to parse than a
    /// long string; it is refused before any JSON tree is built.
    #[test]
    fn many_small_values_are_refused_before_parsing() {
        let numbers = format!("{{\"type\":\"user\",\"items\":[{}]}}", vec!["1"; 200_000].join(","));
        assert!(numbers.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        let started = std::time::Instant::now();
        assert_eq!(reduce_oversized_line(numbers.as_bytes(), OversizedLinePolicy::ClaudeStreamJson).unwrap_err(), OversizedLineRefusal::TooManyValues);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        // Separators inside strings are not counted.
        assert_eq!(count_values(br#"{"a":"x,y:z[{","b":[1,2]}"#), 7);
        assert_eq!(count_values(br#"{"a":"q\"[,"}"#), 3);
    }

    #[test]
    fn a_session_keeps_going_after_several_large_images() {
        let expected = crate::provider::vendor_observation::claude_init::ExpectedInit {
            cwd: test_path!("/snapshot"), model: "model", permission_mode: "default", tools: &["Read"],
        };
        let mut session = claude_session::Session::new(&expected);
        let init = json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":test_path!("/snapshot"),"model":"model","permissionMode":"default","tools":["Read"],"mcp_servers":[],"skills":[],"plugins":[]});
        session.observe(&BoundedObservation::new(line(&init)).unwrap()).unwrap();
        for index in 0..10 {
            let mut frame = image_result(148 * 1024);
            frame["uuid"] = json!(format!("u{index}"));
            let reduced = reduce_oversized_line(&line(&frame), OversizedLinePolicy::ClaudeStreamJson).unwrap();
            match session.observe(&reduced).unwrap() {
                // The work history still learns how the tool use ended.
                claude_session::Event::Output(claude_events::Projection::ToolResults(results)) => {
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0].tool_use_id, "toolu_1");
                    assert_eq!(results[0].is_error, Some(false));
                }
                _ => panic!("expected the tool result"),
            }
        }
        assert!(!session.is_closed());
    }
}
