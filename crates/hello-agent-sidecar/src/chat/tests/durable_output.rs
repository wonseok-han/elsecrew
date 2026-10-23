use super::*;
use crate::agent_runtime::preparation::RunPreparation;
use crate::storage::{agent_output::FinalOutput, ProjectionUpsert};
use hello_agent_protocol::agent_adapter_v1::AgentRunBindingV1;

#[test]
fn four_durable_runs_drain_three_thousand_events_each_without_batch_failure() {
    let mut f = Fixture::with_durable_output(true);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::LongStream);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let mut runs = Vec::new();
    let mut chats = Vec::new();
    for index in 0..4 {
        let snapshot = f.snapshot(&format!("req_long_snapshot_{index}"));
        let chat = if index == 0 {
            f.create_exact_claude_chat(&snapshot, "long stream", "req_long_chat_0")
        } else {
            f.chats
                .create_chat(
                    &mut f.storage,
                    &f.projects,
                    snapshot["preparationId"].as_str().unwrap(),
                    &f.project_id,
                    snapshot["worktreeId"].as_str().unwrap(),
                    snapshot["workspaceIdentity"].as_str().unwrap(),
                    "long stream",
                    2,
                    &format!("req_long_chat_{index}"),
                )
                .unwrap()
                .result
        };
        chats.push(chat);
    }
    for (index, chat) in chats.iter().enumerate() {
        let chat_id = chat["chatId"].as_str().unwrap();
        let request = AgentRunStartRequestV1 {
            chat_id: chat_id.into(),
            role_id: "coding".into(),
            input_message_ids: vec![initial_message_id(&f, chat_id)],
            expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
        };
        let start = runtime
            .start_agent(
                &mut f.storage,
                &mut f.projects,
                request,
                &format!("req_long_run_{index}"),
            )
            .unwrap();
        runs.push(start.result["runId"].as_str().unwrap().to_owned());
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    while runs.iter().any(|id| runtime.has_active_run(id)) {
        assert!(
            std::time::Instant::now() < deadline,
            "long streams did not drain"
        );
        let events = runtime.poll(&mut f.storage, &mut f.projects).unwrap();
        assert!(
            events
                .iter()
                .filter(|event| event.event == "agent.publicEvent")
                .count()
                <= 400
        );
        for event in events
            .into_iter()
            .filter(|event| event.event == "agent.publicEvent")
        {
            *counts
                .entry(event.payload["runId"].as_str().unwrap().into())
                .or_default() += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    for id in &runs {
        assert_eq!(counts.get(id), Some(&3000));
        let run = f.storage.projection_record("run", id).unwrap().unwrap();
        assert_eq!(run["record"]["state"], "succeeded");
        assert!(run.get("public_events").is_none());
    }
    let path = f.storage.path().to_path_buf();
    drop(f.storage);
    f.storage = Storage::open_durable_output_for_test(&path).unwrap();
    for id in runs {
        let projection = f.storage.projection_record("run", &id).unwrap().unwrap();
        let run = &projection["record"];
        let binding: AgentRunBindingV1 = serde_json::from_value(json!({
            "chatId":run["chatId"],"taskId":run["taskId"],"projectId":run["projectId"],
            "worktreeId":run["worktreeId"],"workspaceIdentity":run["workspaceIdentity"],
            "runId":id,"roleId":"coding","capabilityFingerprint":run["capabilityFingerprint"]
        }))
        .unwrap();
        let head = f.storage.agent_output_head(&binding).unwrap();
        assert_eq!(head.committed_sequence, 3000);
        assert!(head.sealed);
        for after in (0..3000).step_by(100) {
            let page = f
                .storage
                .agent_output_page(
                    &hello_agent_protocol::agent_adapter_v1::output_pages::OutputPageRequest {
                        format_version: 1,
                        binding: binding.clone(),
                        after_sequence: after,
                        through_sequence: 3000,
                        limit: 100,
                    },
                )
                .unwrap();
            assert_eq!(page.rows.len(), 100);
            for row in page.rows {
                assert_eq!(row.event.text, format!("output {}", row.output_sequence));
            }
        }
    }
}

// Exercise the real reservation, chat materializer and output transaction.
// No provider process, credentials or model request is used.
#[test]
fn final_output_and_followup_context_commit_or_rollback_together_after_restart() {
    let mut f = Fixture::with_durable_output(true);
    let snapshot = f.snapshot("req_durable_snapshot");
    let chat = f.create_exact_claude_chat(&snapshot, "goal", "req_durable_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let request = AgentRunStartRequestV1 {
        chat_id: chat_id.into(),
        role_id: "coding".into(),
        input_message_ids: vec![initial_message_id(&f, chat_id)],
        expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
    };
    RunPreparation::resolve(&f.storage, &f.projects, &request)
        .unwrap()
        .reserve(
            &mut f.storage,
            &f.projects,
            &serde_json::from_value(capability).unwrap(),
            "req_durable_reserve",
        )
        .unwrap();
    let projection = f.storage.projection_records("run").unwrap().remove(0);
    assert_eq!(projection["output_format_version"], 1);
    let run = &projection["record"];
    let event = json!({
        "schemaVersion":1,"eventId":"agent_event_atomic_final",
        "chatId":run["chatId"],"taskId":run["taskId"],"projectId":run["projectId"],
        "worktreeId":run["worktreeId"],"workspaceIdentity":run["workspaceIdentity"],
        "runId":run["runId"],"roleId":"coding","capabilityFingerprint":run["capabilityFingerprint"],
        "kind":"final","text":"durable answer","createdAt":"2026-10-23T01:02:00Z"
    });
    let binding: AgentRunBindingV1 = serde_json::from_value(json!({
        "chatId":event["chatId"],"taskId":event["taskId"],"projectId":event["projectId"],
        "worktreeId":event["worktreeId"],"workspaceIdentity":event["workspaceIdentity"],
        "runId":event["runId"],"roleId":event["roleId"],"capabilityFingerprint":event["capabilityFingerprint"]
    })).unwrap();
    let before = f.storage.chat_message_history(chat_id).unwrap();
    let saved = f
        .storage
        .projection_record("chat", chat_id)
        .unwrap()
        .unwrap();
    let expected = saved["last_event_sequence"].as_u64().unwrap();
    let mut payload = saved.clone();
    payload.remove("last_event_sequence");
    let sequence = saved["durable_event_sequence"].as_u64().unwrap() + 1;
    payload.insert("durable_event_sequence".into(), json!(sequence));
    payload.insert("last_message".into(), json!({
        "schemaVersion":1,"messageId":"message_durable_final","chatId":run["chatId"],
        "taskId":run["taskId"],"worktreeId":run["worktreeId"],"eventSequence":sequence,
        "kind":"role","roleId":"coding","runId":run["runId"],"text":event["text"],"createdAt":event["createdAt"]
    }));
    let context = || ProjectionUpsert {
        aggregate_type: "chat",
        aggregate_id: chat_id.into(),
        operation_id: "op_durable_atomic_final".into(),
        request_id: None,
        payload: payload.clone(),
    };
    let output = || FinalOutput {
        binding: binding.clone(),
        sequence: 1,
        event: event.clone(),
        item_update: None,
    };

    // Reject at the output insert, after the chat event and projection writes.
    f.storage.connection_for_test().execute_batch("CREATE TRIGGER reject_test_output BEFORE INSERT ON run_output_records BEGIN SELECT RAISE(ABORT, 'injected output failure'); END;").unwrap();
    assert!(f
        .storage
        .append_final_output_with_context(context(), expected, output())
        .is_err());
    assert_eq!(f.storage.chat_message_history(chat_id).unwrap(), before);
    assert_eq!(
        f.storage
            .projection_record("chat", chat_id)
            .unwrap()
            .unwrap(),
        saved
    );
    assert_eq!(
        f.storage
            .agent_output_head(&binding)
            .unwrap()
            .committed_sequence,
        0
    );
    f.storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER reject_test_output;")
        .unwrap();

    // Reopen verifies that the rejected transaction left neither half behind.
    let path = f.storage.path().to_path_buf();
    drop(f.storage);
    f.storage = Storage::open_durable_output_for_test(&path).unwrap();
    assert_eq!(f.storage.chat_message_history(chat_id).unwrap(), before);
    assert_eq!(
        f.storage
            .agent_output_head(&binding)
            .unwrap()
            .committed_sequence,
        0
    );
    f.storage
        .append_final_output_with_context(context(), expected, output())
        .unwrap();
    assert!(f
        .storage
        .append_final_output_with_context(context(), expected, output())
        .is_err());
    drop(f.storage);
    f.storage = Storage::open_durable_output_for_test(&path).unwrap();
    let messages = f.storage.chat_message_history(chat_id).unwrap();
    assert_eq!(messages.len(), before.len() + 1);
    assert_eq!(messages.last().unwrap()["text"], "durable answer");
    assert_eq!(messages.last().unwrap()["runId"], run["runId"]);
    assert_eq!(
        f.storage
            .agent_output_head(&binding)
            .unwrap()
            .committed_sequence,
        1
    );
    let page = f
        .storage
        .agent_output_page(
            &hello_agent_protocol::agent_adapter_v1::output_pages::OutputPageRequest {
                format_version: 1,
                binding,
                after_sequence: 0,
                through_sequence: 1,
                limit: 100,
            },
        )
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(serde_json::to_value(&page.rows[0].event).unwrap(), event);
    assert_eq!(fs::read(f.source.join("src/value.txt")).unwrap(), b"base");
}
