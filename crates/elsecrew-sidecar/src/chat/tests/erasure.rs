//! Chat content erasure (docs/proposals/m5-chat-erasure.md) on disposable DBs only.
use super::*;
use crate::project::chat_erasure::interrupt_at;
use crate::storage::chat_erasure::ERASED_TEXT;

const GOAL: &str = "SENTINEL-goal 7f3a9c erase this private goal";
const FOLLOWUP: &str = "SENTINEL-followup 51d2e0 erase this follow-up";
const OUTPUT: &str = "Continue with the reviewed change?";
const CONTROL: &str = "CONTROL-goal a81c44 must remain";

fn database_bytes(storage: &Storage) -> Vec<u8> {
    let mut bytes = fs::read(storage.path()).unwrap();
    let mut wal = storage.path().as_os_str().to_owned();
    wal.push("-wal");
    if let Ok(more) = fs::read(&wal) {
        bytes.extend(more);
    }
    bytes
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle.as_bytes())
}

fn materialize(storage: &mut Storage) {
    for _ in 0..20_000 {
        if !storage.materialization_requires_work().unwrap() {
            return;
        }
        storage.run_materialization_scheduler_step().unwrap();
    }
    panic!("materialization did not finish");
}

/// Moves a chat to `discarded` through the ordinary discard flow.
fn discard(f: &mut Fixture, chat_id: &str) {
    let stored = chat_projection_record(&f.storage, chat_id).unwrap();
    let chat = update_chat_state(&stored["record"], "reviewReady").unwrap();
    let mut payload = stored.clone();
    payload.insert("record".into(), chat.clone());
    payload.insert("state".into(), json!("reviewReady"));
    f.storage
        .upsert_projection_record("chat", chat_id, &format!("op_erasure_ready_{chat_id}"), None, payload)
        .unwrap();
    let address = address_from_chat(&chat).unwrap();
    let task = chat["taskId"].as_str().unwrap();
    let preview = f.chats.preview_workspace_discard(&f.storage, &f.projects, &address, task).unwrap();
    let revision = preview["chatRevision"].as_u64().unwrap();
    f.chats
        .discard_workspace(&mut f.storage, &mut f.projects, &address, task, revision, &format!("req_discard_{chat_id}"))
        .unwrap();
}

fn discarded_chat(f: &mut Fixture, goal: &str, request: &str) -> String {
    let snapshot = f.snapshot(&format!("{request}_snapshot"));
    let chat = f.create_chat(&snapshot, goal, request);
    let chat_id = chat["chatId"].as_str().unwrap().to_string();
    f.chats
        .append_user_message(&mut f.storage, &f.projects, &json!({
            "projectId":chat["projectId"],"chatId":chat["chatId"],"taskId":chat["taskId"],
            "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
            "expectedEventSequence":1,"clientMessageId":format!("user_{}", "a".repeat(32)),
            "text":FOLLOWUP,
        }))
        .unwrap();
    discard(f, &chat_id);
    chat_id
}

fn erasure_state(f: &Fixture, chat_id: &str) -> Value {
    let project = Value::Object(f.storage.projection_record("project", &f.project_id).unwrap().unwrap());
    project["chat_erasure_v1"][chat_id]["state"].clone()
}

fn assert_erased_history(storage: &Storage, chats: &G5Manager, chat_id: &str) {
    let history = storage.chat_message_history(chat_id).unwrap();
    assert!(!history.is_empty());
    assert!(history.iter().all(|message| message["text"] == ERASED_TEXT), "{history:?}");
    let detail = chats.get_chat(storage, chat_id, 0, None, 10, false).unwrap();
    assert_eq!(detail["chat"]["goal"], ERASED_TEXT);
}

/// Full product admission on restart: reopen/replay/materialization/output verification.
macro_rules! reopen {
    ($f:ident) => {{
        let path = $f.storage.path().to_path_buf();
        drop($f.storage);
        $f.storage = Storage::open_durable_output_for_test(&path).unwrap();
    }};
}

#[test]
fn erasure_removes_actual_chat_content_everywhere_and_survives_restart() {
    let mut f = Fixture::with_durable_output(true);
    let control_snapshot = f.snapshot("req_control_snapshot");
    let control = f.create_chat(&control_snapshot, CONTROL, "req_control_chat");
    let control_id = control["chatId"].as_str().unwrap().to_string();

    let snapshot = f.snapshot("req_target_snapshot");
    let chat = f.create_exact_claude_chat(&snapshot, GOAL, "req_target_chat");
    let chat_id = chat["chatId"].as_str().unwrap().to_string();
    let initial = initial_message_id(&f, &chat_id);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(&mut f.storage, &mut f.projects, AgentRunStartRequestV1 {
            chat_id: chat_id.clone(), role_id: "coding".into(), input_message_ids: vec![initial],
            expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
        }, "req_target_run")
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    drain_actual_scenario(&mut f, &mut runtime, &run_id);
    let sequence = chat_projection_record(&f.storage, &chat_id).unwrap()["durable_event_sequence"].as_u64().unwrap();
    let current = chat_projection_record(&f.storage, &chat_id).unwrap()["record"].clone();
    f.chats
        .append_user_message(&mut f.storage, &f.projects, &json!({
            "projectId":current["projectId"],"chatId":chat_id,"taskId":current["taskId"],
            "worktreeId":current["worktreeId"],"workspaceIdentity":current["workspaceIdentity"],
            "expectedEventSequence":sequence,"clientMessageId":format!("user_{}", "b".repeat(32)),"text":FOLLOWUP,
        }))
        .unwrap();
    discard(&mut f, &chat_id);
    materialize(&mut f.storage);
    f.storage.checkpoint().unwrap();
    let before = database_bytes(&f.storage);
    for sentinel in [GOAL, FOLLOWUP, OUTPUT, CONTROL] {
        assert!(contains(&before, sentinel), "fixture must store {sentinel}");
    }

    let list = f.projects.chat_erasure_list(&f.storage).unwrap().result;
    let entries = list["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "only discarded chats are candidates: {list}");
    assert_eq!(entries[0]["chatId"], chat_id.as_str());
    assert_eq!(entries[0]["eligible"], true);
    assert_eq!(entries[0]["state"], "none");

    let result = f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_erase").unwrap().result;
    assert_eq!(result, json!({"schemaVersion":1,"action":"eraseChatContent","chatId":chat_id,"erased":true}));
    assert_eq!(erasure_state(&f, &chat_id), "erased");
    assert_erased_history(&f.storage, &f.chats, &chat_id);
    let run = f.storage.projection_record("run", &run_id).unwrap().unwrap();
    let outputs = durable_output::read_all(&mut f.storage, &run["record"]);
    assert!(!outputs.is_empty() && outputs.iter().all(|event| event["text"] == ERASED_TEXT));
    let listed = f.chats.list_chats(&f.storage, &f.project_id, None, 50).unwrap();
    assert!(listed["items"].as_array().unwrap().iter().all(|item| item["chatId"] != chat_id.as_str()));
    assert!(f.projects.chat_erasure_list(&f.storage).unwrap().result["entries"].as_array().unwrap().is_empty());
    // Idempotent: a repeated request reports the same durable completion.
    assert_eq!(f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_erase_again").unwrap().result, result);

    let after = database_bytes(&f.storage);
    for sentinel in [GOAL, FOLLOWUP, OUTPUT] {
        assert!(!contains(&after, sentinel), "{sentinel} survived erasure in the DB or WAL bytes");
    }
    assert!(contains(&after, CONTROL));

    reopen!(f);
    materialize(&mut f.storage);
    assert_erased_history(&f.storage, &f.chats, &chat_id);
    assert_eq!(f.storage.chat_message_history(&control_id).unwrap()[0]["text"], CONTROL);
    f.storage.checkpoint().unwrap();
    reopen!(f);
    let restarted = database_bytes(&f.storage);
    for sentinel in [GOAL, FOLLOWUP, OUTPUT] {
        assert!(!contains(&restarted, sentinel), "{sentinel} reappeared after restart");
    }
    assert!(contains(&restarted, CONTROL));
}

#[test]
fn interrupted_erasure_resumes_from_each_durable_state() {
    for (stage, expected) in [(1, "pending"), (2, "redacted"), (3, "redacted")] {
        let mut f = Fixture::with_durable_output(true);
        let chat_id = discarded_chat(&mut f, GOAL, &format!("req_resume_{stage}"));
        materialize(&mut f.storage);
        interrupt_at(stage);
        assert!(f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_first").is_err());
        assert_eq!(erasure_state(&f, &chat_id), expected, "stage {stage}");
        let entry = f.projects.chat_erasure_list(&f.storage).unwrap().result["entries"][0].clone();
        assert_eq!(entry["state"], expected);
        // Restart between attempts; admission must accept every intermediate state.
        f.storage.checkpoint().unwrap();
        reopen!(f);
        assert_eq!(erasure_state(&f, &chat_id), expected);
        if expected == "pending" {
            assert_eq!(f.storage.chat_message_history(&chat_id).unwrap()[0]["text"], GOAL);
        } else {
            assert_erased_history(&f.storage, &f.chats, &chat_id);
        }
        f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_retry").unwrap();
        assert_eq!(erasure_state(&f, &chat_id), "erased");
        assert_erased_history(&f.storage, &f.chats, &chat_id);
        let bytes = database_bytes(&f.storage);
        assert!(!contains(&bytes, GOAL) && !contains(&bytes, FOLLOWUP), "stage {stage}");
        reopen!(f);
        materialize(&mut f.storage);
        assert_erased_history(&f.storage, &f.chats, &chat_id);
    }
}

fn ledger_snapshot(storage: &Storage) -> (Vec<String>, Vec<(String, String)>) {
    let connection = storage.connection_for_test();
    let payloads = connection
        .prepare("SELECT canonical_json_payload FROM events ORDER BY rowid").unwrap()
        .query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    let schema = connection
        .prepare("SELECT name, sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name").unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    (payloads, schema)
}

#[test]
fn ineligible_chats_are_rejected_without_mutation() {
    let mut f = Fixture::with_durable_output(true);
    // Not discarded: may still accept follow-ups or runs.
    let snapshot = f.snapshot("req_live_snapshot");
    let live = f.create_chat(&snapshot, GOAL, "req_live_chat");
    let live_id = live["chatId"].as_str().unwrap().to_string();
    let before = ledger_snapshot(&f.storage);
    assert!(f.projects.erase_chat_content(&mut f.storage, &live_id, "req_live").is_err());
    assert_eq!(ledger_snapshot(&f.storage), before);
    assert_eq!(erasure_state(&f, &live_id), Value::Null);
    assert!(f.projects.erase_chat_content(&mut f.storage, "chat_missing", "req_missing").is_err());

    // Discarded, but an unclassified field could hold content: the full dry run
    // refuses before any durable request exists (nothing recorded, nothing hidden).
    let chat_id = discarded_chat(&mut f, GOAL, "req_unknown");
    let mut payload = chat_projection_record(&f.storage, &chat_id).unwrap();
    payload.insert("unclassified_note".into(), json!(GOAL));
    f.storage.upsert_projection_record("chat", &chat_id, "op_unknown_field", None, payload).unwrap();
    materialize(&mut f.storage);
    let entry = f.projects.chat_erasure_list(&f.storage).unwrap().result["entries"]
        .as_array().unwrap().iter().find(|entry| entry["chatId"] == chat_id.as_str()).cloned().unwrap();
    assert_eq!(entry["eligible"], false);
    let before = ledger_snapshot(&f.storage);
    let error = f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_unknown_erase").unwrap_err();
    assert!(format!("{error:?}").contains("not classified"), "{error:?}");
    assert_eq!(ledger_snapshot(&f.storage), before, "a refused request must not record anything");
    assert_eq!(erasure_state(&f, &chat_id), Value::Null);
    assert_eq!(f.storage.chat_message_history(&chat_id).unwrap()[0]["text"], GOAL);
    reopen!(f);
    assert_eq!(f.storage.chat_message_history(&chat_id).unwrap()[0]["text"], GOAL);
}

#[test]
fn content_copied_into_a_retained_field_is_refused_before_any_record() {
    let mut f = Fixture::with_durable_output(true);
    // The goal equals a retained identifier, so the leftover scan must refuse.
    let snapshot = f.snapshot("req_leftover_snapshot");
    let worktree = snapshot["worktreeId"].as_str().unwrap().to_string();
    assert!(worktree.len() >= 12);
    let chat = f.create_chat(&snapshot, &worktree, "req_leftover_chat");
    let chat_id = chat["chatId"].as_str().unwrap().to_string();
    discard(&mut f, &chat_id);
    materialize(&mut f.storage);
    let before = ledger_snapshot(&f.storage);
    let error = f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_leftover").unwrap_err();
    assert!(format!("{error:?}").contains("unclassified field"), "{error:?}");
    assert_eq!(ledger_snapshot(&f.storage), before);
    assert_eq!(erasure_state(&f, &chat_id), Value::Null);
}

/// Actual run that went through recovery and `resolvedAbandoned` (call on V10 storage).
fn resolved_recovery_chat(f: &mut Fixture) -> (String, String) {
    let (mut runtime, chat, run_id) =
        start_exact_actual_scenario(f, FakeScenario::ProviderTerminalRecoveryRequired, "req_erase_recovery");
    drain_actual_scenario(f, &mut runtime, &run_id);
    let recovery = f.storage.projection_record("run", &run_id).unwrap().unwrap()["recovery"].clone();
    let request = json!({
        "recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],"taskId":chat["taskId"],
        "projectId":chat["projectId"],"worktreeId":chat["worktreeId"],
        "workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,
        "expectedRevision":recovery["revision"],"resolution":"abandon"
    });
    runtime.resolve_recovery(&mut f.storage, &mut f.projects, &request, "req_erase_recovery_resolve").unwrap();
    let run = f.storage.projection_record("run", &run_id).unwrap().unwrap();
    assert_eq!(run["recovery"]["state"], "resolvedAbandoned");
    assert_eq!(run["termination"]["processesProvenGone"], true, "resolution records termination evidence");
    (chat["chatId"].as_str().unwrap().to_string(), run_id)
}

/// A run that went through recovery resolution keeps `recovery_resolution_intent`
/// in its historical payloads; it must be classified, not block erasure.
#[test]
fn chat_with_resolved_run_recovery_history_is_erased() {
    let mut f = Fixture::new();
    let path = f.storage.path().to_path_buf();
    drop(f.storage);
    f.storage = Storage::open_for_admission(&path, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    let (chat_id, run_id) = resolved_recovery_chat(&mut f);
    let intent_seen: bool = f.storage.connection_for_test().query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE aggregate_type='run' AND aggregate_id=?1 \
         AND json_type(canonical_json_payload,'$.recovery_resolution_intent') IS NOT NULL)",
        [&run_id], |row| row.get(0)).unwrap();
    assert!(intent_seen, "fixture must carry recovery intent history");
    materialize(&mut f.storage);
    f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_erase_recovered").unwrap();
    assert_erased_history(&f.storage, &f.chats, &chat_id);
    assert!(!contains(&database_bytes(&f.storage), "provider boundary fixture"));
    drop(f.storage);
    f.storage = Storage::open_for_admission(&path, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    assert_erased_history(&f.storage, &f.chats, &chat_id);
}

/// A resolved recovery never substitutes for termination evidence, and only the
/// protocol state `resolvedAbandoned` counts as resolved.
#[test]
fn resolved_recovery_without_termination_evidence_is_refused() {
    for case in ["noEvidence", "nonProtocolState"] {
        let mut f = Fixture::new();
        let path = f.storage.path().to_path_buf();
        drop(f.storage);
        f.storage = Storage::open_for_admission(&path, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        let (chat_id, run_id) = resolved_recovery_chat(&mut f);
        let mut run = f.storage.projection_record("run", &run_id).unwrap().unwrap();
        if case == "noEvidence" {
            run.get_mut("termination").unwrap()["processesProvenGone"] = json!(false);
        } else {
            run.get_mut("recovery").unwrap()["state"] = json!("resolved");
        }
        f.storage.upsert_projection_record("run", &run_id, &format!("op_tamper_{case}"), None, run).unwrap();
        materialize(&mut f.storage);
        let project_id = f.project_id.clone();
        assert!(f.storage.chat_erasure_precheck(&chat_id, &project_id).is_err(), "{case}: storage eligibility");
        let before = ledger_snapshot(&f.storage);
        assert!(f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_tampered").is_err(), "{case}");
        assert_eq!(ledger_snapshot(&f.storage), before, "{case}: nothing recorded");
        assert_eq!(erasure_state(&f, &chat_id), Value::Null);
    }
}

/// Transient sidecar state (applying, saving, preparation) keeps a `pending`
/// request; the retry then completes.
#[test]
fn transient_busy_retry_keeps_pending_request() {
    let mut f = Fixture::with_durable_output(true);
    let chat_id = discarded_chat(&mut f, GOAL, "req_busy");
    materialize(&mut f.storage);
    interrupt_at(1);
    assert!(f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_busy_first").is_err());
    assert_eq!(erasure_state(&f, &chat_id), "pending");
    crate::project::chat_erasure::simulate_busy(true);
    let error = f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_busy_retry").unwrap_err();
    crate::project::chat_erasure::simulate_busy(false);
    assert!(format!("{error:?}").contains("waits for"), "{error:?}");
    assert_eq!(erasure_state(&f, &chat_id), "pending", "transient refusal must not withdraw");
    assert_eq!(f.storage.chat_message_history(&chat_id).unwrap()[0]["text"], GOAL);
    f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_busy_done").unwrap();
    assert_eq!(erasure_state(&f, &chat_id), "erased");
    assert_erased_history(&f.storage, &f.chats, &chat_id);
}

/// A refusal discovered inside the rewrite transaction (after the guard triggers
/// were dropped) rolls back and withdraws the request.
#[test]
fn in_transaction_refusal_rolls_back_and_withdraws_pending() {
    let mut f = Fixture::with_durable_output(true);
    let chat_id = discarded_chat(&mut f, GOAL, "req_in_tx");
    materialize(&mut f.storage);
    interrupt_at(1);
    assert!(f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_in_tx_first").is_err());
    assert_eq!(erasure_state(&f, &chat_id), "pending");
    let before = ledger_snapshot(&f.storage);
    crate::storage::chat_erasure::inject_refusal_in_transaction();
    let error = f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_in_tx_retry").unwrap_err();
    assert!(format!("{error:?}").contains("injected in-transaction refusal"), "{error:?}");
    let after = ledger_snapshot(&f.storage);
    assert_eq!(after.1, before.1, "guard triggers and schema restored");
    // Only the withdrawal record was appended; no content row changed.
    assert_eq!(after.0[..before.0.len()], before.0[..]);
    assert_eq!(after.0.len(), before.0.len() + 1);
    assert_eq!(erasure_state(&f, &chat_id), Value::Null);
    assert_eq!(f.storage.chat_message_history(&chat_id).unwrap()[0]["text"], GOAL);
    assert!(f.chats.list_chats(&f.storage, &f.project_id, None, 50).unwrap()["items"]
        .as_array().unwrap().iter().any(|item| item["chatId"] == chat_id.as_str()));
    reopen!(f);
    assert_eq!(f.storage.chat_message_history(&chat_id).unwrap()[0]["text"], GOAL);
}

/// A `pending` request whose chat is compacted before the retry is withdrawn:
/// the chat becomes an ordinary visible discarded chat with its content intact.
#[test]
fn pending_request_is_withdrawn_when_the_chat_was_compacted() {
    use crate::storage_lifetime::compaction_writer::{CompactionStepResult, CompactionTarget};
    let mut f = Fixture::with_durable_output(true);
    let chat_id = discarded_chat(&mut f, GOAL, "req_pending_compact");
    materialize(&mut f.storage);
    interrupt_at(1);
    assert!(f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_pending_first").is_err());
    assert_eq!(erasure_state(&f, &chat_id), "pending");
    let visible = |f: &Fixture| f.chats.list_chats(&f.storage, &f.project_id, None, 50).unwrap()["items"]
        .as_array().unwrap().iter().any(|item| item["chatId"] == chat_id.as_str());
    assert!(visible(&f), "pending keeps content and must stay visible");
    let record = chat_projection_record(&f.storage, &chat_id).unwrap()["record"].clone();
    for index in 0..1000 {
        persist_chat_state(&mut f.storage, &record, &format!("op_pending_compact_{index}")).unwrap();
    }
    materialize(&mut f.storage);
    f.storage.checkpoint().unwrap();
    reopen!(f);
    let target = CompactionTarget { aggregate_type: "chat".into(), aggregate_id: chat_id.clone() };
    let mut finalized = false;
    for _ in 0..2000 {
        if matches!(f.storage.run_compaction_scheduler_step(&target).unwrap(), CompactionStepResult::Finalized { .. }) {
            finalized = true;
            break;
        }
    }
    assert!(finalized, "pending chat compaction did not finalize");
    let error = f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_pending_retry").unwrap_err();
    assert!(format!("{error:?}").contains("compact"), "{error:?}");
    assert_eq!(erasure_state(&f, &chat_id), Value::Null, "refused pending request must be withdrawn");
    assert!(visible(&f));
    assert_eq!(f.storage.chat_message_history(&chat_id).unwrap()[0]["text"], GOAL);
    reopen!(f);
    assert_eq!(erasure_state(&f, &chat_id), Value::Null);
}

#[test]
fn compaction_runs_after_erasure_and_compacted_chats_are_not_eligible() {
    use crate::storage_lifetime::compaction_writer::{CompactionStepResult, CompactionTarget};
    let mut f = Fixture::with_durable_output(true);
    let erased = discarded_chat(&mut f, GOAL, "req_compact_erased");
    let snapshot = f.snapshot("req_compact_long_snapshot");
    let long = f.create_chat(&snapshot, CONTROL, "req_compact_long_chat");
    let long_id = long["chatId"].as_str().unwrap().to_string();
    materialize(&mut f.storage);
    f.projects.erase_chat_content(&mut f.storage, &erased, "req_compact_erase").unwrap();
    for index in 0..1000 {
        persist_chat_state(&mut f.storage, &long, &format!("op_erasure_compact_{index}")).unwrap();
    }
    materialize(&mut f.storage);
    f.storage.checkpoint().unwrap();
    reopen!(f);
    let target = CompactionTarget { aggregate_type: "chat".into(), aggregate_id: long_id.clone() };
    let mut finalized = false;
    for _ in 0..2000 {
        if matches!(f.storage.run_compaction_scheduler_step(&target).unwrap(), CompactionStepResult::Finalized { .. }) {
            finalized = true;
            break;
        }
    }
    assert!(finalized, "compaction after erasure did not finalize");
    f.storage.checkpoint().unwrap();
    reopen!(f);
    materialize(&mut f.storage);
    assert_erased_history(&f.storage, &f.chats, &erased);
    assert_eq!(f.storage.chat_message_history(&long_id).unwrap()[0]["text"], CONTROL);

    // The compacted chat now has frozen history; v1 refuses before any write.
    discard(&mut f, &long_id);
    let entry = f.projects.chat_erasure_list(&f.storage).unwrap().result["entries"]
        .as_array().unwrap().iter().find(|entry| entry["chatId"] == long_id.as_str()).cloned().unwrap();
    assert_eq!(entry["eligible"], false);
    let before = ledger_snapshot(&f.storage);
    assert!(f.projects.erase_chat_content(&mut f.storage, &long_id, "req_compacted").is_err());
    assert_eq!(ledger_snapshot(&f.storage), before);
    assert_eq!(erasure_state(&f, &long_id), Value::Null);
}

/// Product admission without the durable output ledger (storage V10) keeps run
/// output in the legacy `public_events` projection array with byte counters.
#[test]
fn erasure_covers_legacy_public_event_runs_on_v10() {
    let mut f = Fixture::new();
    let path = f.storage.path().to_path_buf();
    drop(f.storage);
    f.storage = Storage::open_for_admission(&path, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    let snapshot = f.snapshot("req_legacy_snapshot");
    let chat = f.create_exact_claude_chat(&snapshot, GOAL, "req_legacy_chat");
    let chat_id = chat["chatId"].as_str().unwrap().to_string();
    let initial = initial_message_id(&f, &chat_id);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(&mut f.storage, &mut f.projects, AgentRunStartRequestV1 {
            chat_id: chat_id.clone(), role_id: "coding".into(), input_message_ids: vec![initial],
            expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
        }, "req_legacy_run")
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    drain_actual_scenario(&mut f, &mut runtime, &run_id);
    let run = f.storage.projection_record("run", &run_id).unwrap().unwrap();
    assert!(run.get("output_format_version").is_none(), "fixture must use the legacy output array");
    assert!(run["public_events"].as_array().unwrap().iter().any(|event| event["text"] == OUTPUT));
    discard(&mut f, &chat_id);
    materialize(&mut f.storage);
    f.storage.checkpoint().unwrap();
    assert!(contains(&database_bytes(&f.storage), OUTPUT));

    f.projects.erase_chat_content(&mut f.storage, &chat_id, "req_legacy_erase").unwrap();
    let run = f.storage.projection_record("run", &run_id).unwrap().unwrap();
    let events = run["public_events"].as_array().unwrap();
    assert!(events.iter().all(|event| event["text"] == ERASED_TEXT));
    assert_eq!(run["public_text_bytes"], (events.len() * ERASED_TEXT.len()) as u64);
    let bytes = database_bytes(&f.storage);
    for sentinel in [GOAL, OUTPUT] {
        assert!(!contains(&bytes, sentinel), "{sentinel} survived legacy erasure");
    }
    drop(f.storage);
    f.storage = Storage::open_for_admission(&path, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    materialize(&mut f.storage);
    assert_erased_history(&f.storage, &f.chats, &chat_id);
}
