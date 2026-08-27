#![cfg(not(windows))]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use hello_agent_protocol::agent_adapter_v1::{
    validate_agent_adapter_payload_v1, G5A_CAPABILITIES_V1,
};
use hello_agent_protocol::run_recovery_v1::{validate_g5_payload_v1, G5_CAPABILITIES_V1};
use hello_agent_protocol::{decode_jsonl, encode_jsonl, ready_payload, EnvelopeV1, RequestV1};
use rusqlite::Connection;

#[test]
fn binary_startup_failure_uses_stderr_nonzero_and_no_stdout_ready() {
    let root = tempfile::tempdir().expect("temporary directory");
    fs::create_dir(root.path().join("hello-agent.sqlite3")).expect("invalid database directory");
    let output = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(root.path())
        .stdin(Stdio::null())
        .output()
        .expect("run sidecar");

    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty(), "pre-ready stdout must stay empty");
    assert!(!output.stderr.is_empty(), "startup detail must use stderr");
}

#[test]
fn binary_ready_status_quit_flow_marks_clean_and_exits_zero() {
    let root = tempfile::tempdir().expect("temporary directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sidecar");
    let mut stdin = child.stdin.take().expect("stdin pipe");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout pipe"));

    let mut ready_line = String::new();
    stdout.read_line(&mut ready_line).expect("read ready");
    let EnvelopeV1::Event(ready_event) = decode_jsonl(ready_line.as_bytes()).expect("decode ready")
    else {
        panic!("first envelope must be ready")
    };
    let ready = ready_payload(&ready_event).expect("ready payload");

    for (request_id, method) in [
        ("req_process_status", "system.status"),
        ("req_process_quit", "app.prepareQuit"),
    ] {
        let request = EnvelopeV1::Request(RequestV1 {
            protocol_version: 1,
            kind: "request".to_string(),
            sidecar_instance_id: ready.sidecar_instance_id.clone(),
            request_id: request_id.to_string(),
            method: method.to_string(),
            params: serde_json::json!({}),
        });
        stdin
            .write_all(encode_jsonl(&request).unwrap().as_bytes())
            .expect("write request");
        stdin.flush().expect("flush request");
        let mut response_line = String::new();
        stdout.read_line(&mut response_line).expect("read response");
        let EnvelopeV1::Response(response) =
            decode_jsonl(response_line.as_bytes()).expect("decode response")
        else {
            panic!("request must receive response")
        };
        assert!(response.ok, "{method}");
    }
    drop(stdin);
    let mut trailing_stdout = Vec::new();
    stdout
        .read_to_end(&mut trailing_stdout)
        .expect("read remaining stdout");
    assert!(trailing_stdout.is_empty());
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_end(&mut stderr)
        .expect("read stderr");
    let status = child.wait().expect("wait sidecar");
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty());

    let database = Connection::open(root.path().join("hello-agent.sqlite3"))
        .expect("inspect sidecar database");
    let clean: Option<String> = database
        .query_row(
            "SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id=?1;",
            [ready.sidecar_instance_id],
            |row| row.get(0),
        )
        .expect("clean marker");
    assert!(clean.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn binary_g5a_public_events_follow_accepted_and_keep_raw_carriers_private() {
    let root = tempfile::tempdir().expect("temporary directory");
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("src/file.txt"), "fixture").unwrap();
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
            "-q",
            "-m",
            "fixture",
        ],
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .args(["--stdio-agent-fixture", "clean"])
        .current_dir(&data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn G5A fixture sidecar");
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(ready_event) = receive_envelope(&envelopes) else {
        panic!("ready event")
    };
    let ready = ready_payload(&ready_event).unwrap();
    assert_eq!(
        ready.capabilities,
        G5A_CAPABILITIES_V1
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>()
    );

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_open",
        "project.open",
        serde_json::json!({"selectedPath":source}),
    );
    let EnvelopeV1::Response(open) = receive_envelope(&envelopes) else {
        panic!("open response")
    };
    assert!(open.ok, "{:?}", open.error);
    let project_id = open.result.unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_string();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_settings",
        "project.getRoleSettings",
        serde_json::json!({"projectId":project_id}),
    );
    assert!(matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if response.ok));
    let prepared = prepare_workspace(
        &mut stdin,
        &envelopes,
        &ready.sidecar_instance_id,
        &project_id,
        "req_agent_prepare",
    );
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_chat",
        "chat.create",
        serde_json::json!({
            "preparationId":prepared["preparationId"],"projectId":project_id,
            "worktreeId":prepared["manifest"]["worktreeId"],
            "workspaceIdentity":prepared["manifest"]["workspaceIdentity"],
            "goal":"normalized public event fixture","expectedSettingsRevision":1,
        }),
    );
    let EnvelopeV1::Response(chat_response) = receive_envelope(&envelopes) else {
        panic!("chat response")
    };
    assert!(chat_response.ok, "{:?}", chat_response.error);
    let chat = chat_response.result.unwrap();
    let chat_id = chat["chatId"].as_str().unwrap().to_string();
    let EnvelopeV1::Event(created) = receive_envelope(&envelopes) else {
        panic!("direct chat event")
    };
    assert_eq!(created.event, "chat.eventAppended");
    validate_g5_payload_v1("teamMessage", &created.payload).unwrap();
    let message_id = created.payload["messageId"].as_str().unwrap().to_string();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_empty_detail",
        "chat.get",
        serde_json::json!({"chatId":chat_id,"afterEventSequence":0,"cursor":null,"limit":50}),
    );
    let EnvelopeV1::Response(empty_detail) = receive_envelope(&envelopes) else {
        panic!("empty detail response")
    };
    assert!(empty_detail.ok, "{:?}", empty_detail.error);
    let empty_detail = empty_detail.result.unwrap();
    assert_eq!(empty_detail["agentRuns"], serde_json::json!([]));
    validate_agent_adapter_payload_v1("agentChatDetailPage", &empty_detail).unwrap();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_capability",
        "agent.getCapabilities",
        serde_json::json!({"runtime":"codex","refresh":true}),
    );
    let EnvelopeV1::Response(capability) = receive_envelope(&envelopes) else {
        panic!("capability response")
    };
    let capability = capability.result.unwrap();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_start",
        "run.startAgent",
        serde_json::json!({
            "chatId":chat_id,"roleId":"planning","inputMessageIds":[message_id],
            "expectedCapabilityFingerprint":capability["fingerprint"],
        }),
    );
    let EnvelopeV1::Response(accepted) = receive_envelope(&envelopes) else {
        panic!("accepted must be emitted before events")
    };
    assert!(accepted.ok, "{:?}", accepted.error);
    let accepted = accepted.result.unwrap();
    validate_agent_adapter_payload_v1("runAccepted", &accepted).unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_string();

    let EnvelopeV1::Event(running) = receive_envelope(&envelopes) else {
        panic!("running state event")
    };
    assert_eq!(running.event, "agent.runStateChanged");
    assert_eq!(running.seq, 1);
    assert_eq!(running.payload["run"]["state"], "running");
    validate_agent_adapter_payload_v1("runStateChanged", &running.payload).unwrap();

    let mut public_events = Vec::new();
    for expected_seq in 2..=4 {
        let EnvelopeV1::Event(event) = receive_envelope(&envelopes) else {
            panic!("public event")
        };
        assert_eq!(event.event, "agent.publicEvent");
        assert_eq!(event.seq, expected_seq);
        assert_eq!(event.chat_id.as_deref(), Some(chat_id.as_str()));
        assert_eq!(event.task_id.as_deref(), chat["taskId"].as_str());
        assert_eq!(event.run_id.as_deref(), Some(run_id.as_str()));
        validate_agent_adapter_payload_v1("publicEvent", &event.payload).unwrap();
        for field in [
            "chatId",
            "taskId",
            "projectId",
            "worktreeId",
            "workspaceIdentity",
        ] {
            assert_eq!(event.payload[field], chat[field]);
        }
        assert_eq!(event.payload["runId"], run_id);
        assert_eq!(event.payload["roleId"], "planning");
        assert_eq!(
            event.payload["capabilityFingerprint"],
            capability["fingerprint"]
        );
        public_events.push(event.payload);
    }
    assert_eq!(
        public_events
            .iter()
            .map(|event| event["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["status", "question", "final"]
    );
    let EnvelopeV1::Event(terminal) = receive_envelope(&envelopes) else {
        panic!("terminal state event")
    };
    assert_eq!(terminal.event, "agent.runStateChanged");
    assert_eq!(terminal.seq, 5);
    assert_eq!(terminal.payload["run"]["state"], "succeeded");
    validate_agent_adapter_payload_v1("runStateChanged", &terminal.payload).unwrap();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_terminal_detail",
        "chat.get",
        serde_json::json!({"chatId":chat_id,"afterEventSequence":0,"cursor":null,"limit":50}),
    );
    let EnvelopeV1::Response(detail) = receive_envelope(&envelopes) else {
        panic!("terminal detail response; an unexpected raw event was emitted")
    };
    assert!(detail.ok, "{:?}", detail.error);
    let detail = detail.result.unwrap();
    validate_agent_adapter_payload_v1("agentChatDetailPage", &detail).unwrap();
    assert_eq!(detail["agentRuns"][0]["runId"], run_id);
    assert_eq!(detail["agentRuns"][0]["state"], "succeeded");
    assert_eq!(detail["messages"].as_array().unwrap().len(), 4);
    let mut public_text = public_events
        .iter()
        .filter_map(|event| event["text"].as_str())
        .collect::<Vec<_>>();
    public_text.extend(
        detail["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["text"].as_str()),
    );
    let public_text = public_text.join("\n").to_lowercase();
    for forbidden in [
        "token-never-persist",
        "reasoning",
        "stderr",
        "tool",
        "env",
        "auth",
        "prompt",
        "usage",
    ] {
        assert!(
            !public_text.contains(forbidden),
            "public surface contains forbidden carrier: {forbidden}"
        );
    }

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    assert!(
        matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if response.result.as_ref().unwrap()["safeToQuit"] == true)
    );
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn binary_project_worktree_read_stream_and_quit_follow_g4_order() {
    let root = tempfile::tempdir().expect("temporary directory");
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("src/file.txt"), "가나다 hello").unwrap();
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
            "-q",
            "-m",
            "fixture",
        ],
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(&data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sidecar");
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let ready = receive_envelope(&envelopes);
    let EnvelopeV1::Event(ready_event) = ready else {
        panic!("ready event")
    };
    let ready = ready_payload(&ready_event).unwrap();
    assert_eq!(
        ready.capabilities,
        G5_CAPABILITIES_V1
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>()
    );

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_open",
        "project.open",
        serde_json::json!({"selectedPath":source}),
    );
    let EnvelopeV1::Response(open) = receive_envelope(&envelopes) else {
        panic!("open response")
    };
    assert!(open.ok);
    let project_id = open.result.unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_string();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_create",
        "project.createWorktree",
        serde_json::json!({"projectId":project_id}),
    );
    let EnvelopeV1::Response(create) = receive_envelope(&envelopes) else {
        panic!("create response")
    };
    assert!(create.ok);
    let worktree_path = create.result.unwrap()["worktreePath"]
        .as_str()
        .unwrap()
        .to_string();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_watch",
        "file.watch",
        serde_json::json!({"projectId":project_id}),
    );
    let EnvelopeV1::Response(watch) = receive_envelope(&envelopes) else {
        panic!("watch response")
    };
    assert!(watch.ok);
    fs::write(
        std::path::Path::new(&worktree_path).join("src/file.txt"),
        "비동기 변경",
    )
    .unwrap();
    let EnvelopeV1::Event(changed) = receive_envelope(&envelopes) else {
        panic!("spontaneous watch event")
    };
    assert_eq!(changed.event, "file.changed");
    assert!(changed.payload["paths"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path == "src/file.txt"));

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_read",
        "file.read",
        serde_json::json!({"projectId":project_id,"path":"src/file.txt"}),
    );
    let EnvelopeV1::Response(read) = receive_envelope(&envelopes) else {
        panic!("read descriptor")
    };
    let descriptor = read.result.unwrap();
    let stream_id = descriptor["stream"]["streamId"]
        .as_str()
        .unwrap()
        .to_string();
    let EnvelopeV1::Event(chunk) = receive_envelope(&envelopes) else {
        panic!("blob chunk")
    };
    assert_eq!(chunk.stream_id, stream_id);
    assert_eq!(chunk.seq, 1);
    assert_eq!(chunk.event, "blob.chunk");
    assert_eq!(chunk.payload["text"], "비동기 변경");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    let EnvelopeV1::Response(quit) = receive_envelope(&envelopes) else {
        panic!("quit response")
    };
    assert!(quit.ok);
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[cfg(target_os = "macos")]
#[test]
fn binary_cancel_clean_proves_fixture_parent_and_descendant_gone() {
    binary_cancel_scenario("cancel-clean", true);
}

#[cfg(target_os = "macos")]
#[test]
fn fixture_child_never_spawns_descendants_before_release_barrier() {
    let started = std::time::Instant::now();
    let status = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .args(["--fixture-child", "cancel-clean"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn unreleased fixture child");
    assert!(status.success());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "an unreleased fixture must exit on EOF instead of spawning the 30-second leaf"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn binary_cancel_uncertain_blocks_then_abandon_resolution_clears_quit() {
    binary_cancel_scenario("cancel-uncertain", false);
}

#[cfg(target_os = "macos")]
#[test]
fn binary_fresh_b_failure_and_exact_abandon_preserve_bound_a() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("src/file.txt"), "fixture").unwrap();
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
            "-q",
            "-m",
            "fixture",
        ],
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(&data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(ready_event) = receive_envelope(&envelopes) else {
        panic!("ready")
    };
    let ready = ready_payload(&ready_event).unwrap();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_open",
        "project.open",
        serde_json::json!({"selectedPath":source}),
    );
    let EnvelopeV1::Response(open) = receive_envelope(&envelopes) else {
        panic!("open")
    };
    let project_id = open.result.unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_string();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_settings",
        "project.getRoleSettings",
        serde_json::json!({"projectId":project_id}),
    );
    assert!(matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if response.ok));
    let prepared_a = prepare_workspace(
        &mut stdin,
        &envelopes,
        &ready.sidecar_instance_id,
        &project_id,
        "req_ab_prepare_a",
    );
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_chat_a",
        "chat.create",
        serde_json::json!({
            "preparationId":prepared_a["preparationId"],"projectId":project_id,
            "worktreeId":prepared_a["manifest"]["worktreeId"],
            "workspaceIdentity":prepared_a["manifest"]["workspaceIdentity"],
            "goal":"bound a","expectedSettingsRevision":1,
        }),
    );
    let EnvelopeV1::Response(chat_a) = receive_envelope(&envelopes) else {
        panic!("chat a")
    };
    assert!(chat_a.ok, "{:?}", chat_a.error);
    let chat_a = chat_a.result.unwrap();
    assert!(
        matches!(receive_envelope(&envelopes), EnvelopeV1::Event(event) if event.event == "chat.eventAppended")
    );
    let prepared_b = prepare_workspace(
        &mut stdin,
        &envelopes,
        &ready.sidecar_instance_id,
        &project_id,
        "req_ab_prepare_b",
    );
    assert_ne!(prepared_a["preparationId"], prepared_b["preparationId"]);
    assert_ne!(
        prepared_a["manifest"]["worktreeId"],
        prepared_b["manifest"]["worktreeId"]
    );
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_chat_b_fail",
        "chat.create",
        serde_json::json!({
            "preparationId":prepared_b["preparationId"],"projectId":project_id,
            "worktreeId":prepared_b["manifest"]["worktreeId"],
            "workspaceIdentity":prepared_b["manifest"]["workspaceIdentity"],
            "goal":"must fail","expectedSettingsRevision":99,
        }),
    );
    assert!(matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if !response.ok));
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_abandon_b",
        "workspace.abandonPrepared",
        serde_json::json!({
            "preparationId":prepared_b["preparationId"],"projectId":project_id,
            "worktreeId":prepared_b["manifest"]["worktreeId"],
            "workspaceIdentity":prepared_b["manifest"]["workspaceIdentity"],
        }),
    );
    let abandon_response = receive_envelope(&envelopes);
    assert!(
        matches!(&abandon_response, EnvelopeV1::Response(response) if response.ok),
        "fresh workspace B abandon failed: {abandon_response:?}"
    );
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_tree_a",
        "workspace.getTree",
        serde_json::json!({
            "chatId":chat_a["chatId"],"projectId":project_id,
            "worktreeId":prepared_a["manifest"]["worktreeId"],
            "workspaceIdentity":prepared_a["manifest"]["workspaceIdentity"],
            "parentPath":"","cursor":null,
        }),
    );
    let tree_response = receive_envelope(&envelopes);
    assert!(
        matches!(&tree_response, EnvelopeV1::Response(response) if response.ok),
        "bound workspace A tree response failed: {tree_response:?}"
    );
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_late_abandon_a",
        "workspace.abandonPrepared",
        serde_json::json!({
            "preparationId":prepared_a["preparationId"],"projectId":project_id,
            "worktreeId":prepared_a["manifest"]["worktreeId"],
            "workspaceIdentity":prepared_a["manifest"]["workspaceIdentity"],
        }),
    );
    assert!(matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if !response.ok));
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_ab_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    assert!(
        matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if response.result.as_ref().unwrap()["safeToQuit"] == true)
    );
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[cfg(target_os = "macos")]
fn binary_cancel_scenario(scenario: &str, clean: bool) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("src/file.txt"), "fixture").unwrap();
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
            "-q",
            "-m",
            "fixture",
        ],
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-agent-sidecar"))
        .arg("--stdio")
        .current_dir(&data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(ready_event) = receive_envelope(&envelopes) else {
        panic!("ready")
    };
    let ready = ready_payload(&ready_event).unwrap();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_g5_open",
        "project.open",
        serde_json::json!({"selectedPath":source}),
    );
    let EnvelopeV1::Response(open) = receive_envelope(&envelopes) else {
        panic!("open")
    };
    let project_id = open.result.unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_string();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_g5_snapshot",
        "workspace.prepareChat",
        serde_json::json!({"projectId":project_id}),
    );
    let EnvelopeV1::Response(snapshot) = receive_envelope(&envelopes) else {
        panic!("snapshot")
    };
    let snapshot = snapshot.result.unwrap();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_g5_settings",
        "project.getRoleSettings",
        serde_json::json!({"projectId":project_id}),
    );
    let EnvelopeV1::Response(settings) = receive_envelope(&envelopes) else {
        panic!("settings")
    };
    assert_eq!(settings.result.unwrap()["revision"], 1);
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_g5_chat",
        "chat.create",
        serde_json::json!({
            "preparationId":snapshot["preparationId"],"projectId":project_id,
            "worktreeId":snapshot["manifest"]["worktreeId"],
            "workspaceIdentity":snapshot["manifest"]["workspaceIdentity"],"goal":"cancel fixture",
            "expectedSettingsRevision":1,
        }),
    );
    let EnvelopeV1::Response(chat) = receive_envelope(&envelopes) else {
        panic!("chat")
    };
    let chat = chat.result.unwrap();
    let chat_id = chat["chatId"].as_str().unwrap().to_string();
    let EnvelopeV1::Event(created) = receive_envelope(&envelopes) else {
        panic!("chat event")
    };
    assert_eq!(created.event, "chat.eventAppended");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_g5_run",
        "run.startFixture",
        serde_json::json!({"chatId":chat_id,"roleId":"coding","scenario":scenario}),
    );
    let EnvelopeV1::Response(start) = receive_envelope(&envelopes) else {
        panic!("run start")
    };
    assert!(start.ok, "{:?}", start.error);
    let run_id = start.result.unwrap()["run"]["runId"]
        .as_str()
        .unwrap()
        .to_string();
    let EnvelopeV1::Event(started) = receive_envelope(&envelopes) else {
        panic!("run event")
    };
    assert_eq!(started.event, "run.event");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_g5_cancel",
        "run.cancel",
        serde_json::json!({"chatId":chat_id,"runId":run_id}),
    );
    let EnvelopeV1::Response(cancel) = receive_envelope(&envelopes) else {
        panic!("cancel")
    };
    assert_eq!(
        cancel.result.unwrap()["outcome"],
        if clean {
            "canceled"
        } else {
            "recoveryRequired"
        }
    );
    let EnvelopeV1::Event(finished) = receive_envelope(&envelopes) else {
        panic!("finished")
    };
    assert_eq!(finished.event, "run.finished");
    assert_eq!(
        finished.payload["run"]["terminationEvidence"]["processesProvenGone"],
        clean
    );
    if !clean {
        assert!(
            finished.payload["recovery"]["processIdentities"]
                .as_array()
                .is_some_and(|identities| identities.len() >= 2),
            "recovery must retain both the fixture parent and its observed descendant"
        );
        let recovery_id = finished.payload["recovery"]["recoveryId"]
            .as_str()
            .unwrap()
            .to_string();
        write_request(
            &mut stdin,
            &ready.sidecar_instance_id,
            "req_g5_recovery_list",
            "recovery.list",
            serde_json::json!({"cursor":null,"limit":100}),
        );
        let EnvelopeV1::Response(recoveries) = receive_envelope(&envelopes) else {
            panic!("recoveries")
        };
        let recovery_page = recoveries.result.unwrap();
        validate_g5_payload_v1("recoveryPage", &recovery_page).unwrap();
        assert_eq!(recovery_page["items"][0]["state"], "unresolved");
        write_request(
            &mut stdin,
            &ready.sidecar_instance_id,
            "req_g5_quit_blocked",
            "app.prepareQuit",
            serde_json::json!({}),
        );
        let EnvelopeV1::Response(blocked_quit) = receive_envelope(&envelopes) else {
            panic!("blocked quit")
        };
        assert_eq!(blocked_quit.result.unwrap()["safeToQuit"], false);
        write_request(
            &mut stdin,
            &ready.sidecar_instance_id,
            "req_g5_recovery_resolve",
            "recovery.markResolved",
            serde_json::json!({
                "recoveryId":recovery_id,"expectedRevision":1,"resolution":"abandon"
            }),
        );
        let EnvelopeV1::Response(resolved) = receive_envelope(&envelopes) else {
            panic!("resolved recovery")
        };
        assert!(resolved.ok, "{:?}", resolved.error);
        assert_eq!(resolved.result.unwrap()["state"], "resolvedAbandoned");
    }

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_g5_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    let EnvelopeV1::Response(quit) = receive_envelope(&envelopes) else {
        panic!("quit")
    };
    assert_eq!(quit.result.unwrap()["safeToQuit"], true);
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

fn write_request(
    stdin: &mut impl Write,
    instance: &str,
    request_id: &str,
    method: &str,
    params: serde_json::Value,
) {
    let request = EnvelopeV1::Request(RequestV1 {
        protocol_version: 1,
        kind: "request".to_string(),
        sidecar_instance_id: instance.to_string(),
        request_id: request_id.to_string(),
        method: method.to_string(),
        params,
    });
    stdin
        .write_all(encode_jsonl(&request).unwrap().as_bytes())
        .unwrap();
    stdin.flush().unwrap();
}

fn prepare_workspace(
    stdin: &mut impl Write,
    envelopes: &Receiver<EnvelopeV1>,
    instance: &str,
    project_id: &str,
    request_id: &str,
) -> serde_json::Value {
    write_request(
        stdin,
        instance,
        request_id,
        "workspace.prepareChat",
        serde_json::json!({"projectId":project_id}),
    );
    let EnvelopeV1::Response(response) = receive_envelope(envelopes) else {
        panic!("prepare")
    };
    assert!(response.ok, "{:?}", response.error);
    response.result.unwrap()
}

fn spawn_envelope_reader(stdout: impl Read + Send + 'static) -> Receiver<EnvelopeV1> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if sender.send(decode_jsonl(line.as_bytes()).unwrap()).is_err() {
                break;
            }
        }
    });
    receiver
}

fn receive_envelope(receiver: &Receiver<EnvelopeV1>) -> EnvelopeV1 {
    receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("bounded sidecar envelope timeout")
}

fn git(directory: &std::path::Path, args: &[&str]) {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
