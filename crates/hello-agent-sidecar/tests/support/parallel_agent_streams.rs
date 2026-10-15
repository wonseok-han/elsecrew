use super::*;
use serde_json::{json, Value};
use std::collections::HashSet;

// Real JSONL transport and persistence, fake bounded providers; not a load or
// real-account benchmark. Queue all starts before consuming any run events.
#[test]
fn four_chat_agent_streams_keep_acceptance_events_and_history_scoped() {
    let root = tempfile::Builder::new()
        .prefix("hello-agent-four-streams-")
        .tempdir()
        .unwrap()
        .keep();
    eprintln!("four-chat evidence root: {}", root.display());
    let source = root.join("source");
    let data = root.join("data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("hello.txt"), "original\n").unwrap();
    git(&source, &["init", "-q"]);
    git(&source, &["add", "."]);
    git(
        &source,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .args(["--stdio-agent-fixture", "final-delayed"])
        .current_dir(&data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(event) = receive_envelope(&envelopes) else {
        panic!("ready")
    };
    let instance = ready_payload(&event).unwrap().sidecar_instance_id;
    wait_for_storage_ready(&mut stdin, &envelopes, &instance, true);
    let request = |stdin: &mut std::process::ChildStdin, id: &str, method: &str, params: Value| {
        write_request(stdin, &instance, id, method, params);
        let EnvelopeV1::Response(response) = receive_envelope(&envelopes) else {
            panic!("response for {id}")
        };
        assert_eq!(response.request_id, id);
        assert!(response.ok, "{id}: {:?}", response.error);
        response.result.unwrap()
    };
    let project = request(
        &mut stdin,
        "open",
        "project.open",
        json!({"selectedPath":source}),
    );
    let project_id = &project["projectId"];
    request(
        &mut stdin,
        "settings",
        "project.getRoleSettings",
        json!({"projectId":project_id}),
    );
    request(
        &mut stdin,
        "settings_update",
        "project.updateRoleSettings",
        json!({
        "projectId":project_id,"expectedRevision":1,"roles":[
            {"roleId":"planning","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"},
            {"roleId":"coding","runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":"workspaceWrite"},
            {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"}
        ]}),
    );
    let mut chats = Vec::new();
    let mut inputs = Vec::new();
    for index in 0..4 {
        let prepared = prepare_workspace(
            &mut stdin,
            &envelopes,
            &instance,
            project_id.as_str().unwrap(),
            &format!("prepare_{index}"),
        );
        let chat = request(
            &mut stdin,
            &format!("create_{index}"),
            "chat.create",
            json!({
                "preparationId":prepared["preparationId"],"projectId":project_id,
                "worktreeId":prepared["manifest"]["worktreeId"],"workspaceIdentity":prepared["manifest"]["workspaceIdentity"],
                "goal":format!("isolated chat {index}"),"expectedSettingsRevision":2
            }),
        );
        let EnvelopeV1::Event(created) = receive_envelope(&envelopes) else {
            panic!("created")
        };
        assert_eq!(created.event, "chat.eventAppended");
        assert_eq!(created.payload["chatId"], chat["chatId"]);
        inputs.push(created.payload["messageId"].clone());
        chats.push(chat);
    }
    assert_eq!(
        chats
            .iter()
            .map(|c| c["workspaceIdentity"].as_str().unwrap())
            .collect::<HashSet<_>>()
            .len(),
        4
    );
    let capability = request(
        &mut stdin,
        "capability",
        "agent.getCapabilities",
        json!({"runtime":"claude-code","refresh":true}),
    );
    for (index, chat) in chats.iter().enumerate() {
        write_request(
            &mut stdin,
            &instance,
            &format!("start_{index}"),
            "run.startAgent",
            json!({
                "chatId":chat["chatId"],"roleId":"coding","inputMessageIds":[inputs[index]],
                "expectedCapabilityFingerprint":capability["fingerprint"]
            }),
        );
    }
    let mut runs: Vec<Option<String>> = vec![None; 4];
    let mut sequences = [0; 4];
    let mut finished = [false; 4];
    let mut active = 0;
    let mut peak_active = 0;
    let mut public: Vec<Vec<Value>> = vec![Vec::new(); 4];
    let mut identities = HashSet::new();
    while !finished.iter().all(|done| *done) {
        match receive_envelope(&envelopes) {
            EnvelopeV1::Response(response) => {
                let index: usize = response
                    .request_id
                    .strip_prefix("start_")
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!(index < 4 && runs[index].is_none());
                assert!(response.ok, "{:?}", response.error);
                let accepted = response.result.unwrap();
                validate_agent_adapter_payload_v1("runAccepted", &accepted).unwrap();
                assert_eq!(accepted["chatId"], chats[index]["chatId"]);
                runs[index] = Some(accepted["runId"].as_str().unwrap().to_owned());
            }
            EnvelopeV1::Event(event) => {
                let index = chats
                    .iter()
                    .position(|c| c["chatId"].as_str() == event.chat_id.as_deref())
                    .expect("known chat");
                assert!(!finished[index], "event after terminal");
                assert!(runs[index].is_some(), "event before acceptance");
                assert_eq!(event.run_id, runs[index]);
                assert_eq!(event.task_id.as_deref(), chats[index]["taskId"].as_str());
                sequences[index] += 1;
                assert_eq!(
                    event.seq, sequences[index],
                    "run stream sequence gap/duplicate"
                );
                let payload = &event.payload;
                assert!(identities.insert(payload["eventId"].as_str().unwrap().to_owned()));
                match event.event.as_str() {
                    "agent.publicEvent" => {
                        validate_agent_adapter_payload_v1("publicEvent", payload).unwrap();
                        for key in [
                            "chatId",
                            "taskId",
                            "projectId",
                            "worktreeId",
                            "workspaceIdentity",
                        ] {
                            assert_eq!(payload[key], chats[index][key]);
                        }
                        assert_eq!(payload["runId"].as_str(), runs[index].as_deref());
                        assert_eq!(payload["capabilityFingerprint"], capability["fingerprint"]);
                        public[index].push(payload.clone());
                    }
                    "agent.runStateChanged" => {
                        validate_agent_adapter_payload_v1("runStateChanged", payload).unwrap();
                        assert_eq!(payload["run"]["runId"].as_str(), runs[index].as_deref());
                        if sequences[index] == 1 {
                            assert_eq!(payload["run"]["state"], "running");
                            active += 1;
                            peak_active = peak_active.max(active);
                        } else {
                            assert_eq!(sequences[index], 5);
                            assert_eq!(payload["run"]["state"], "succeeded");
                            finished[index] = true;
                            active -= 1;
                        }
                    }
                    other => panic!("unexpected event {other}"),
                }
            }
            _ => panic!("unexpected request"),
        }
    }
    assert_eq!(runs.iter().collect::<HashSet<_>>().len(), 4);
    assert_eq!(active, 0);
    let mut messages = HashSet::new();
    for (index, chat) in chats.iter().enumerate() {
        assert_eq!(
            public[index]
                .iter()
                .map(|e| e["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["status", "question", "final"]
        );
        let detail = request(
            &mut stdin,
            &format!("detail_{index}"),
            "chat.get",
            json!({"chatId":chat["chatId"],"afterEventSequence":0,"cursor":null,"limit":50}),
        );
        validate_agent_adapter_payload_v1("agentChatDetailPage", &detail).unwrap();
        assert_eq!(detail["agentRuns"].as_array().unwrap().len(), 1);
        assert_eq!(
            detail["agentRuns"][0]["runId"].as_str(),
            runs[index].as_deref()
        );
        assert_eq!(detail["agentRuns"][0]["state"], "succeeded");
        assert!(detail["agentRecovery"].is_null());
        let stored = detail["messages"].as_array().unwrap();
        assert_eq!(stored.len(), 4);
        for (ordinal, message) in stored.iter().enumerate() {
            assert_eq!(message["chatId"], chat["chatId"]);
            assert_eq!(message["taskId"], chat["taskId"]);
            assert_eq!(message["worktreeId"], chat["worktreeId"]);
            assert_eq!(message["eventSequence"], ordinal + 1);
            assert!(messages.insert(message["messageId"].as_str().unwrap().to_owned()));
            if ordinal == 0 {
                assert_eq!(message["text"], format!("isolated chat {index}"));
            } else {
                assert_eq!(message["runId"].as_str(), runs[index].as_deref());
                assert_eq!(message["text"], public[index][ordinal - 1]["text"]);
            }
        }
    }
    assert_eq!(
        fs::read_to_string(source.join("hello.txt")).unwrap(),
        "original\n"
    );
    assert_eq!(
        request(&mut stdin, "quit", "app.prepareQuit", json!({}))["safeToQuit"],
        true
    );
    drop(stdin);
    assert!(child.wait().unwrap().success());
    assert!(
        peak_active > 1,
        "fixture must exercise overlapping run streams"
    );
    eprintln!("four-chat transport: accepted=4 public=12 terminal=4 stored=16 peak_active={peak_active}; clean exit; original unchanged");
    fs::remove_dir_all(root).unwrap();
}
