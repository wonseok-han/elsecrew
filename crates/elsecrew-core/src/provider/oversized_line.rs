//! A session's provider stdout line over the observation line bound
//! (2026-11-26 D2). An agent that reads a large image or file gets it back as
//! one stream-json/JSON-RPC line (a 55 KB PNG became a ~148 KB line), which
//! used to end the session. In session mode the transport reads such a line up
//! to `PROVIDER_SESSION_LINE_RAW_MAX_BYTES` and this module shortens its long
//! strings so the parsers see an ordinary bounded observation.
//!
//! Only frames whose long strings nobody acts on are shortened: Claude tool
//! results and other ignored frames, the private blocks of an assistant frame,
//! and Codex notifications. A permission request is never shortened: the allow
//! reply echoes its input back to the provider (`updatedInput`), so a shortened
//! input would be written as the user's file. Handshake, result and every
//! request/response frame keep the old bound and still fail when oversized.
use super::{unique_json, BoundedObservation, ProviderError, PROVIDER_OBSERVATION_LINE_MAX_BYTES};
use serde_json::{Map, Value};

/// The largest raw line a session connection buffers before shortening it.
/// One line is held at a time per connection; anything larger still fails.
pub const PROVIDER_SESSION_LINE_RAW_MAX_BYTES: usize = 8 * 1024 * 1024;
/// What one session connection may stream in each direction in total. It
/// matches the durable session output budget (`ProviderEventBudget::DurableV1`),
/// which the session parsers still enforce on what they observe. The 1 MiB
/// connection total fits a login or preflight exchange, not an agent session
/// that reads several images. It counts bytes; it does not hold them.
pub const PROVIDER_SESSION_STREAM_MAX_BYTES: usize = 256 * 1024 * 1024;
/// Strings up to this size are kept whole (public text is at most 64 KiB and
/// is never shortened at all).
const KEEP_STRING_BYTES: usize = 4 * 1024;
/// How much of a shortened string stays, so activity can still name a target.
const KEPT_PREFIX_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OversizedLinePolicy {
    ClaudeStreamJson,
    CodexAppServer,
}

fn refused(reason: &str) -> ProviderError {
    ProviderError(format!("oversized provider line {reason}"))
}

/// `line` is one complete frame, with or without its trailing newline.
pub fn reduce_oversized_line(
    line: &[u8],
    policy: OversizedLinePolicy,
) -> Result<BoundedObservation, ProviderError> {
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    if body.len() > PROVIDER_SESSION_LINE_RAW_MAX_BYTES || body.contains(&b'\n') {
        return Err(refused("exceeds the session line bound"));
    }
    // Duplicate keys are refused before anything is rewritten, as for any frame.
    let mut root = unique_json::parse(body).map_err(|_| refused("is not unique-key JSON"))?;
    let object = root.as_object_mut().ok_or_else(|| refused("is not an object"))?;
    match policy {
        OversizedLinePolicy::ClaudeStreamJson => reduce_claude(object)?,
        OversizedLinePolicy::CodexAppServer => reduce_codex(object)?,
    }
    let mut bytes = serde_json::to_vec(&root).map_err(|_| refused("could not be re-encoded"))?;
    if bytes.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
        return Err(refused("is still over the line bound after shortening"));
    }
    bytes.push(b'\n');
    BoundedObservation::new(bytes)
}

fn reduce_claude(object: &mut Map<String, Value>) -> Result<(), ProviderError> {
    let kind = object.get("type").and_then(Value::as_str).ok_or_else(|| refused("has no type"))?;
    let subtype = object.get("subtype").and_then(Value::as_str);
    match (kind, subtype) {
        // Authority and handshake frames are read exactly or not at all.
        ("control_request" | "control_cancel_request" | "control_response" | "result", _)
        | ("system", Some("init")) => Err(refused("is a frame that is never shortened")),
        ("assistant", _) => {
            for (key, value) in object.iter_mut() {
                if key != "message" {
                    shorten(value);
                    continue;
                }
                let Some(message) = value.as_object_mut() else { return Err(refused("has no message")) };
                for (key, value) in message.iter_mut() {
                    if key != "content" {
                        shorten(value);
                        continue;
                    }
                    let Some(blocks) = value.as_array_mut() else { return Err(refused("has no content")) };
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
            let created_lines = created_line_count(object);
            for value in object.values_mut() { shorten(value); }
            if let Some(lines) = created_lines {
                // Only the count survives: one empty line each, no content.
                object["tool_use_result"]["content"] = Value::String("\n".repeat(lines));
            }
            Ok(())
        }
    }
}

/// The work history counts the lines of a file written whole from the tool
/// result's `content`; keep that count when the content itself is dropped.
/// A count too large to fit is left to `shorten` (the history is then off).
fn created_line_count(object: &Map<String, Value>) -> Option<usize> {
    let extra = object.get("tool_use_result")?.as_object()?;
    if extra.get("type")?.as_str()? != "create" { return None; }
    let content = extra.get("content")?.as_str()?;
    if content.len() <= KEEP_STRING_BYTES { return None; }
    Some(content.lines().count()).filter(|lines| *lines <= 32 * 1024)
}

fn reduce_codex(object: &mut Map<String, Value>) -> Result<(), ProviderError> {
    // Requests (approvals) and responses carry an id: never shortened.
    if object.contains_key("id") {
        return Err(refused("is a request or response"));
    }
    if !object.get("method").is_some_and(Value::is_string) {
        return Err(refused("is not a notification"));
    }
    for (key, value) in object.iter_mut() {
        if key != "params" {
            shorten(value);
            continue;
        }
        let Some(params) = value.as_object_mut() else {
            shorten(value);
            continue;
        };
        for (key, value) in params.iter_mut() {
            match key.as_str() {
                // Message and plan deltas are public text.
                "delta" => {}
                "item" => {
                    let public = matches!(
                        value.get("type").and_then(Value::as_str),
                        Some("agentMessage" | "plan")
                    );
                    match value.as_object_mut() {
                        Some(item) if public => {
                            for (key, value) in item.iter_mut() {
                                if key != "text" { shorten(value); }
                            }
                        }
                        Some(item) => {
                            let counts = change_counts(item);
                            for value in item.values_mut() { shorten(value); }
                            restore_change_counts(item, counts);
                        }
                        _ => shorten(value),
                    }
                }
                _ => shorten(value),
            }
        }
    }
    Ok(())
}

/// The work history counts each file change's added and removed lines from
/// its `diff`. Keep those counts when a long diff is dropped.
fn change_counts(item: &Map<String, Value>) -> Vec<Option<(u64, u64)>> {
    let Some(changes) = item.get("changes").and_then(Value::as_array) else { return Vec::new() };
    changes.iter().map(|change| {
        let diff = change.get("diff")?.as_str()?;
        if diff.len() <= KEEP_STRING_BYTES { return None; }
        let kind = change.get("kind")?.get("type")?.as_str()?;
        Some(crate::provider::vendor_observation::codex_events::stream::diff_counts(kind, diff))
            .filter(|(added, removed)| added + removed <= 16 * 1024)
    }).collect()
}

fn restore_change_counts(item: &mut Map<String, Value>, counts: Vec<Option<(u64, u64)>>) {
    let Some(changes) = item.get_mut("changes").and_then(Value::as_array_mut) else { return };
    for (change, counts) in changes.iter_mut().zip(counts) {
        let Some((added, removed)) = counts else { continue };
        // One hunk of bare `+`/`-` lines counts the same for every change kind.
        change["diff"] = Value::String(format!("@@\n{}{}", "+\n".repeat(added as usize), "-\n".repeat(removed as usize)));
    }
}

fn shorten(value: &mut Value) {
    match value {
        Value::String(text) if text.len() > KEEP_STRING_BYTES => {
            let mut end = KEPT_PREFIX_BYTES;
            while !text.is_char_boundary(end) { end -= 1; }
            let omitted = text.len() - end;
            text.truncate(end);
            text.push_str(&format!("…[{omitted} bytes omitted]"));
        }
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
        let diff = json!({"method":"turn/diff/updated","params":{"threadId":"t","turnId":"r","diff":big}});
        assert!(reduce_oversized_line(&line(&diff), OversizedLinePolicy::CodexAppServer).is_ok());
        let approval = json!({"id":7,"method":"item/fileChange/requestApproval","params":{"pad":big}});
        assert!(reduce_oversized_line(&line(&approval), OversizedLinePolicy::CodexAppServer).is_err());
        let response = json!({"id":7,"result":{"pad":big}});
        assert!(reduce_oversized_line(&line(&response), OversizedLinePolicy::CodexAppServer).is_err());
        // An agent message whose own text is over the bound is not shortened.
        let message = json!({"method":"item/completed","params":{"item":{"id":"i","type":"agentMessage","text":big}}});
        assert!(reduce_oversized_line(&line(&message), OversizedLinePolicy::CodexAppServer).is_err());
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
