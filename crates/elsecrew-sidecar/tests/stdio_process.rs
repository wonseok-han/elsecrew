#![cfg(not(windows))]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use elsecrew_protocol::agent_adapter_v1::G5D_NATIVE_CAPABILITIES_V1 as G5D_PRODUCT_CAPABILITIES_V1;
use elsecrew_protocol::agent_adapter_v1::{
    validate_agent_adapter_payload_v1, G5D_AGENT_FIXTURE_CAPABILITIES_V1,
};
#[cfg(target_os = "macos")]
use elsecrew_protocol::lifecycle_g5d_v1::parse_current_native_system_status_v1 as parse_g5d_product_system_status_v1;
#[cfg(not(target_os = "macos"))]
use elsecrew_protocol::lifecycle_g5d_v1::parse_current_product_system_status_v1 as parse_g5d_product_system_status_v1;
use elsecrew_protocol::lifecycle_g5d_v1::{
    parse_current_agent_fixture_system_status_v1 as parse_g5d_agent_fixture_system_status_v1,
    validate_g5d_blocked_request_error_v1, StorageAdmissionStateV1,
};
use elsecrew_protocol::run_recovery_v1::validate_g5_payload_v1;
#[cfg(not(target_os = "macos"))]
use elsecrew_protocol::run_recovery_v1::G5D_PRODUCT_CAPABILITIES_V1;
use elsecrew_protocol::{decode_jsonl, encode_jsonl, ready_payload, EnvelopeV1, RequestV1};
use rusqlite::Connection;

static ADMISSION_POLL_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(target_os = "macos")]
#[path = "support/parallel_agent_streams.rs"]
mod parallel_agent_streams;

#[test]
fn binary_storage_failure_is_reported_after_shell_ready_and_quits_cleanly() {
    let root = tempfile::tempdir().expect("temporary directory");
    fs::create_dir(root.path().join("elsecrew.sqlite3")).expect("invalid database directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .arg("--stdio")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run sidecar");
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(ready_event) = receive_envelope(&envelopes) else {
        panic!("shell ready")
    };
    let ready = ready_payload(&ready_event).unwrap();
    wait_for_storage_state(
        &mut stdin,
        &envelopes,
        &ready.sidecar_instance_id,
        false,
        StorageAdmissionStateV1::Failed,
    );
    write_raw_request_with_params(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_failed_raw_params",
        "project.open",
        "[\"not\",\"an\",\"object\"]",
    );
    let EnvelopeV1::Response(blocked) = receive_envelope(&envelopes) else {
        panic!("failed admission block")
    };
    validate_g5d_blocked_request_error_v1(
        StorageAdmissionStateV1::Failed,
        blocked.error.as_ref().unwrap(),
    )
    .expect("exact failed admission error");
    wait_for_storage_state(
        &mut stdin,
        &envelopes,
        &ready.sidecar_instance_id,
        false,
        StorageAdmissionStateV1::Failed,
    );
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_failed_quit",
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
fn binary_ready_status_quit_flow_marks_clean_and_exits_zero() {
    let root = tempfile::tempdir().expect("temporary directory");
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
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
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "shell-ready envelope must not wait for storage admission"
    );
    assert_eq!(
        ready.capabilities,
        G5D_PRODUCT_CAPABILITIES_V1
            .iter()
            .chain(std::iter::once(&"chat.appendMessage"))
            .chain(["agent.outputHead", "agent.outputPage"].iter())
            .chain(std::iter::once(&"provider.login"))
            .chain(["run.approval", "project.catalog", "provider.install"].iter())
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>()
    );
    wait_for_storage_ready_reader(&mut stdin, &mut stdout, &ready.sidecar_instance_id, false);

    #[cfg(target_os = "macos")]
    for (request_id, method, params, success) in [
        (
            "native_codex",
            "agent.getCapabilities",
            serde_json::json!({"runtime":"codex","refresh":true}),
            true,
        ),
        (
            "native_claude",
            "agent.getCapabilities",
            serde_json::json!({"runtime":"claude-code","refresh":true}),
            true,
        ),
        (
            "native_fixture_denied",
            "run.startFixture",
            serde_json::json!({}),
            false,
        ),
        (
            "native_start_denied",
            "run.startAgent",
            serde_json::json!({}),
            false,
        ),
    ] {
        write_request(
            &mut stdin,
            &ready.sidecar_instance_id,
            request_id,
            method,
            params,
        );
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let EnvelopeV1::Response(response) = decode_jsonl(line.as_bytes()).unwrap() else {
            panic!("native response")
        };
        assert_eq!(response.ok, success, "{method}");
        if success {
            assert_eq!(response.result.unwrap()["canStart"], false);
        }
    }

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

    let database = Connection::open(root.path().join("elsecrew.sqlite3"))
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

#[test]
fn binary_delayed_storage_still_emits_shell_ready_within_three_seconds() {
    let root = tempfile::tempdir().expect("temporary directory");
    initialize_database_with_sidecar(root.path());
    let lock = Connection::open(root.path().join("elsecrew.sqlite3")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();

    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .arg("--stdio")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(ready_event) = receive_envelope(&envelopes) else {
        panic!("shell ready")
    };
    assert!(started.elapsed() < Duration::from_secs(3));
    let ready = ready_payload(&ready_event).unwrap();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_delayed_status",
        "system.status",
        serde_json::json!({}),
    );
    let EnvelopeV1::Response(status) = receive_envelope(&envelopes) else {
        panic!("verifying status")
    };
    assert_eq!(
        parse_g5d_product_system_status_v1(&status.result.unwrap())
            .unwrap()
            .storage_admission
            .state,
        StorageAdmissionStateV1::Verifying
    );
    write_raw_request_with_params(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_delayed_raw_params",
        "recovery.markResolved",
        "null",
    );
    let EnvelopeV1::Response(blocked) = receive_envelope(&envelopes) else {
        panic!("verifying admission block")
    };
    validate_g5d_blocked_request_error_v1(
        StorageAdmissionStateV1::Verifying,
        blocked.error.as_ref().unwrap(),
    )
    .expect("exact verifying admission error");
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_delayed_status_after_raw",
        "system.status",
        serde_json::json!({}),
    );
    let EnvelopeV1::Response(status_after) = receive_envelope(&envelopes) else {
        panic!("status after malformed params")
    };
    assert_eq!(
        parse_g5d_product_system_status_v1(&status_after.result.unwrap())
            .unwrap()
            .storage_admission
            .state,
        StorageAdmissionStateV1::Verifying
    );

    lock.execute_batch("ROLLBACK;").unwrap();
    drop(lock);
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, false);
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_delayed_quit",
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
#[test]
fn binary_g5c_capability_cache_refreshes_only_when_requested_and_codex_stays_blocked() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .args(["--stdio-agent-fixture", "discovery-stale-closure"])
        .current_dir(root.path())
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
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, true);
    for (request_id, refresh) in [("req_cache_1", false), ("req_cache_2", false)] {
        write_request(
            &mut stdin,
            &ready.sidecar_instance_id,
            request_id,
            "agent.getCapabilities",
            serde_json::json!({"runtime":"claude-code","refresh":refresh}),
        );
        let EnvelopeV1::Response(response) = receive_envelope(&envelopes) else {
            panic!("capability")
        };
        assert_eq!(response.result.unwrap()["canStart"], true);
    }
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_cache_refresh",
        "agent.getCapabilities",
        serde_json::json!({"runtime":"claude-code","refresh":true}),
    );
    let EnvelopeV1::Response(refreshed) = receive_envelope(&envelopes) else {
        panic!("refreshed")
    };
    let refreshed = refreshed.result.unwrap();
    assert_eq!(refreshed["canStart"], false);
    assert_eq!(refreshed["blockedReason"], "installationIncomplete");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_codex_blocked",
        "agent.getCapabilities",
        serde_json::json!({"runtime":"codex","refresh":true}),
    );
    let EnvelopeV1::Response(codex) = receive_envelope(&envelopes) else {
        panic!("codex")
    };
    let codex = codex.result.unwrap();
    assert_eq!(codex["canStart"], false);
    assert_eq!(codex["blockedReason"], "exactModelUnavailable");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_cache_quit",
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
#[test]
fn binary_g5a_public_events_follow_accepted_and_keep_raw_carriers_private() {
    g5a_public_event_flow(false, None);
}

#[cfg(target_os = "macos")]
#[test]
fn binary_g5a_progress_survives_continuous_requests() {
    g5a_public_event_flow(true, None);
}

#[cfg(target_os = "macos")]
#[test]
fn binary_agent_cancel_defers_completion_without_blocking_status_or_quit() {
    for scenario in ["cancel-clean", "cancel-uncertain"] {
        g5a_public_event_flow(false, Some(scenario));
    }
}

#[cfg(all(
    target_os = "macos",
    any(debug_assertions, feature = "qa-output-fixtures")
))]
#[test]
fn binary_long_stream_commits_and_pages_three_thousand_outputs() {
    g5a_public_event_flow(false, Some("long-stream"));
}

#[test]
#[cfg(all(not(debug_assertions), not(feature = "qa-output-fixtures")))]
fn release_cli_rejects_durable_fixture_before_opening_database() {
    for scenario in ["long-stream", "paced-long-stream"] {
        let root = tempfile::tempdir().unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
            .args(["--stdio-agent-fixture", scenario])
            .current_dir(root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr)
            .contains("unknown stdio agent fixture scenario"));
        assert!(fs::read_dir(root.path()).unwrap().next().is_none());
    }
}

#[cfg(target_os = "macos")]
fn g5a_public_event_flow(continuous_requests: bool, cancellation: Option<&str>) {
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .args(["--stdio-agent-fixture", cancellation.unwrap_or("clean")])
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
        G5D_AGENT_FIXTURE_CAPABILITIES_V1
            .iter()
            .chain(std::iter::once(&"chat.appendMessage"))
            .chain(["agent.outputHead", "agent.outputPage"].iter())
            .chain(std::iter::once(&"provider.login"))
            .chain(["run.approval", "project.catalog", "provider.install"].iter())
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>()
    );
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, true);

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
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_settings_exact",
        "project.updateRoleSettings",
        serde_json::json!({
            "projectId":project_id,
            "expectedRevision":1,
            "roles":[
                {"roleId":"planning","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"},
                {"roleId":"coding","runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":"workspaceWrite"},
                {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"}
            ]
        }),
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
            "goal":"normalized public event fixture","expectedSettingsRevision":2,
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
    assert_eq!(empty_detail["agentRecovery"], serde_json::Value::Null);
    validate_agent_adapter_payload_v1("agentChatDetailPage", &empty_detail).unwrap();

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_agent_capability",
        "agent.getCapabilities",
        serde_json::json!({"runtime":"claude-code","refresh":true}),
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
            "chatId":chat_id,"roleId":"coding","inputMessageIds":[message_id],
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

    if cancellation == Some("long-stream") {
        durable_stdio_output_flow(
            &mut stdin,
            &envelopes,
            &ready.sidecar_instance_id,
            &chat,
            &run_id,
            &capability["fingerprint"],
        );
        write_request(
            &mut stdin,
            &ready.sidecar_instance_id,
            "req_long_quit",
            "app.prepareQuit",
            serde_json::json!({}),
        );
        assert!(
            matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response)
            if response.ok && response.result.as_ref().unwrap()["safeToQuit"] == true)
        );
        drop(stdin);
        assert!(child.wait().unwrap().success());
        return;
    }

    if let Some(scenario) = cancellation {
        agent_cancel_flow(
            &mut stdin,
            &envelopes,
            &ready.sidecar_instance_id,
            &chat,
            &run_id,
            scenario,
        );
        drop(stdin);
        assert!(child.wait().unwrap().success());
        return;
    }

    let mut buffered = std::collections::VecDeque::new();
    if continuous_requests {
        use std::sync::{atomic::AtomicBool, Arc};
        let sending = Arc::new(AtomicBool::new(true));
        let sent = Arc::new(AtomicU64::new(0));
        let writer_active = sending.clone();
        let writer_count = sent.clone();
        let instance = ready.sidecar_instance_id.clone();
        let writer = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(1);
            let mut count = 0;
            while Instant::now() < until {
                count += 1;
                write_request(
                    &mut stdin,
                    &instance,
                    &format!("req_busy_{count}"),
                    "system.status",
                    serde_json::json!({}),
                );
                writer_count.store(count, Ordering::Release);
                thread::sleep(Duration::from_millis(5));
            }
            writer_active.store(false, Ordering::Release);
            stdin
        });
        let mut responses = 0;
        let mut progressed_while_busy = false;
        let drain_started = Instant::now();
        while sending.load(Ordering::Acquire) || responses < sent.load(Ordering::Acquire) {
            let envelope = match envelopes.recv_timeout(Duration::from_millis(50)) {
                Ok(envelope) => envelope,
                Err(mpsc::RecvTimeoutError::Timeout)
                    if drain_started.elapsed() < Duration::from_secs(5) =>
                {
                    continue
                }
                error => panic!("busy request drain failed: {error:?}"),
            };
            match envelope {
                EnvelopeV1::Response(response) => {
                    assert!(response.ok && response.request_id.starts_with("req_busy_"));
                    responses += 1;
                }
                event @ EnvelopeV1::Event(_) => {
                    progressed_while_busy |= sending.load(Ordering::Acquire);
                    buffered.push_back(event);
                }
                _ => panic!("unexpected request on stdout"),
            }
        }
        stdin = writer.join().unwrap();
        assert!(responses > 10);
        assert!(
            progressed_while_busy,
            "runtime must progress without an idle input interval"
        );
    }
    let mut receive_run = || {
        buffered
            .pop_front()
            .unwrap_or_else(|| receive_envelope(&envelopes))
    };
    let EnvelopeV1::Event(running) = receive_run() else {
        panic!("running state event")
    };
    assert_eq!(running.event, "agent.runStateChanged");
    assert_eq!(running.seq, 1);
    assert_eq!(running.payload["run"]["state"], "running");
    validate_agent_adapter_payload_v1("runStateChanged", &running.payload).unwrap();

    let mut public_events = Vec::new();
    for expected_seq in 2..=4 {
        let EnvelopeV1::Event(event) = receive_run() else {
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
        assert_eq!(event.payload["roleId"], "coding");
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
    let EnvelopeV1::Event(terminal) = receive_run() else {
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
    assert_eq!(detail["agentRecovery"], serde_json::Value::Null);
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
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    for forbidden in [
        "private@example.invalid",
        "private-org",
        "private-token",
        "token-never-persist",
    ] {
        assert!(!stderr.to_lowercase().contains(forbidden));
    }

    let connection = Connection::open(data.join("elsecrew.sqlite3")).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT canonical_json_payload FROM events WHERE aggregate_type='run' \
             UNION ALL SELECT projection_json FROM runs",
        )
        .unwrap();
    let persisted = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
        .to_lowercase();
    assert!(persisted.contains("runtime_attestation"));
    assert!(persisted.contains("provider_discovery"));
    for forbidden in [
        "private@example.invalid",
        "private-org",
        "private-token",
        "token-never-persist",
    ] {
        assert!(!persisted.contains(forbidden));
    }
}

#[cfg(target_os = "macos")]
fn durable_stdio_output_flow(
    stdin: &mut std::process::ChildStdin,
    envelopes: &Receiver<EnvelopeV1>,
    instance: &str,
    chat: &serde_json::Value,
    run_id: &str,
    fingerprint: &serde_json::Value,
) {
    use elsecrew_protocol::agent_adapter_v1::output_pages::{
        decode_output_page, OutputHead, OutputPageRequest,
    };
    let binding = serde_json::json!({
        "chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],
        "worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],
        "runId":run_id,"roleId":"coding","capabilityFingerprint":fingerprint,
    });
    let typed_binding = serde_json::from_value(binding.clone()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut sequence = 0;
    let mut last_head = 0;
    let mut sealed = false;
    loop {
        assert!(Instant::now() < deadline, "long stream did not terminate");
        let EnvelopeV1::Event(event) = receive_envelope(envelopes) else {
            panic!("expected output/state event")
        };
        assert_eq!(event.seq, sequence + 1);
        sequence = event.seq;
        assert_eq!(event.run_id.as_deref(), Some(run_id));
        match event.event.as_str() {
            "agent.outputAvailable" => {
                let head: OutputHead = serde_json::from_value(event.payload).unwrap();
                head.validate(&typed_binding).unwrap();
                assert!(head.committed_sequence >= last_head);
                last_head = head.committed_sequence;
                sealed = head.sealed;
            }
            "agent.runStateChanged" => {
                let state = event.payload["run"]["state"].as_str().unwrap();
                if state == "running" {
                    continue;
                }
                assert_eq!(state, "succeeded", "{:?}", event.payload);
                assert_eq!(event.payload["run"]["terminationOutcome"], "cleanExit");
                break;
            }
            other => panic!("unexpected long output event: {other}"),
        }
    }
    assert!(sealed);
    assert_eq!(last_head, 3000);
    write_request(
        stdin,
        instance,
        "req_long_head",
        "agent.outputHead",
        binding.clone(),
    );
    let EnvelopeV1::Response(response) = receive_envelope(envelopes) else {
        panic!("head response")
    };
    assert!(response.ok, "{:?}", response.error);
    let head: OutputHead = serde_json::from_value(response.result.unwrap()).unwrap();
    head.validate(&typed_binding).unwrap();
    assert!(head.sealed);
    assert_eq!(head.committed_sequence, 3000);
    let mut cursor = 0;
    let mut ids = std::collections::HashSet::new();
    while cursor < head.committed_sequence {
        let request = OutputPageRequest {
            format_version: 1,
            binding: typed_binding.clone(),
            after_sequence: cursor,
            through_sequence: head.committed_sequence,
            limit: 100,
        };
        write_request(
            stdin,
            instance,
            &format!("req_long_page_{cursor}"),
            "agent.outputPage",
            serde_json::to_value(&request).unwrap(),
        );
        let EnvelopeV1::Response(response) = receive_envelope(envelopes) else {
            panic!("page response")
        };
        assert!(response.ok, "{:?}", response.error);
        let page = decode_output_page(
            &serde_json::to_vec(&response.result.unwrap()).unwrap(),
            &request,
        )
        .unwrap();
        for row in &page.rows {
            cursor += 1;
            assert_eq!(row.output_sequence, cursor);
            assert_eq!(row.event.text, format!("output {cursor}"));
            assert!(ids.insert(row.event.event_id.clone()));
        }
        assert_eq!(page.next_after_sequence, cursor);
        assert!(!page.rows.is_empty());
    }
    assert_eq!(ids.len(), 3000);
}

#[cfg(target_os = "macos")]
fn agent_cancel_flow(
    stdin: &mut std::process::ChildStdin,
    envelopes: &Receiver<EnvelopeV1>,
    instance: &str,
    chat: &serde_json::Value,
    run_id: &str,
    scenario: &str,
) {
    let fields = [
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
        "runId",
    ];
    let mut identity = serde_json::json!({"runId":run_id});
    for field in &fields[..5] {
        identity[*field] = chat[*field].clone();
    }
    for (index, field) in fields.iter().enumerate() {
        let mut wrong = identity.clone();
        wrong[*field] = serde_json::json!(format!("wrong_{index}"));
        let request_id = format!("req_wrong_cancel_{index}");
        write_request(stdin, instance, &request_id, "run.cancelAgent", wrong);
        loop {
            match receive_envelope(envelopes) {
                EnvelopeV1::Response(response) => {
                    assert_eq!(response.request_id, request_id);
                    assert!(!response.ok);
                    break;
                }
                EnvelopeV1::Event(event) => assert_ne!(event.payload["run"]["state"], "canceled"),
                _ => panic!("unexpected request"),
            }
        }
    }
    write_request(
        stdin,
        instance,
        "req_deferred_cancel",
        "run.cancelAgent",
        identity.clone(),
    );
    write_request(
        stdin,
        instance,
        "req_cancel_status",
        "system.status",
        serde_json::json!({}),
    );
    write_request(
        stdin,
        instance,
        "req_duplicate_cancel",
        "run.cancelAgent",
        identity.clone(),
    );
    write_request(
        stdin,
        instance,
        "req_pending_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    let mut status_seen = false;
    let mut duplicate_seen = false;
    let mut quit_seen = false;
    let mut terminal_seen = false;
    let mut result = None;
    while !status_seen || !duplicate_seen || !quit_seen || !terminal_seen || result.is_none() {
        match receive_envelope(envelopes) {
            EnvelopeV1::Response(response) => match response.request_id.as_str() {
                "req_cancel_status" => {
                    assert!(response.ok);
                    assert!(
                        result.is_none(),
                        "status must remain responsive before cancellation completes"
                    );
                    status_seen = true;
                }
                "req_duplicate_cancel" => {
                    assert!(!response.ok);
                    duplicate_seen = true;
                }
                "req_pending_quit" => {
                    assert_eq!(response.result.unwrap()["safeToQuit"], false);
                    quit_seen = true;
                }
                "req_deferred_cancel" => {
                    assert!(response.ok, "{:?}", response.error);
                    assert!(result.is_none(), "only one final response");
                    let value = response.result.unwrap();
                    validate_agent_adapter_payload_v1("runCancelResult", &value).unwrap();
                    assert_eq!(value["request"], identity);
                    assert_eq!(
                        value["termination"]["processesProvenGone"],
                        scenario == "cancel-clean"
                    );
                    assert_eq!(
                        value["outcome"],
                        if scenario == "cancel-clean" {
                            "canceled"
                        } else {
                            "recoveryRequired"
                        }
                    );
                    result = Some(value);
                }
                _ => panic!("unexpected response"),
            },
            EnvelopeV1::Event(event) => {
                if ["canceled", "recoveryRequired"]
                    .contains(&event.payload["run"]["state"].as_str().unwrap_or_default())
                {
                    assert!(
                        result.is_some(),
                        "completed cancellation reply precedes its terminal event"
                    );
                    terminal_seen = true;
                }
            }
            _ => panic!("unexpected request"),
        }
    }
    if scenario == "cancel-uncertain" {
        let recovery = &result.as_ref().unwrap()["recovery"];
        let mut resolution = identity;
        resolution["recoveryId"] = recovery["recoveryId"].clone();
        resolution["expectedRevision"] = recovery["revision"].clone();
        resolution["resolution"] = serde_json::json!("abandon");
        write_request(
            stdin,
            instance,
            "req_cancel_recovery",
            "recovery.resolveAgent",
            resolution,
        );
        let EnvelopeV1::Response(response) = receive_envelope(envelopes) else {
            panic!("recovery response");
        };
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result.unwrap()["outcome"], "resolvedAbandoned");
    }
    write_request(
        stdin,
        instance,
        "req_cancel_final_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    let EnvelopeV1::Response(response) = receive_envelope(envelopes) else {
        panic!("final quit response");
    };
    assert!(response.ok);
    assert_eq!(response.result.unwrap()["safeToQuit"], true);
}

#[cfg(target_os = "macos")]
#[test]
fn binary_provider_recovery_terminal_blocks_quit_until_exact_agent_abandon() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("file.txt"), "fixture").unwrap();
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .args([
            "--stdio-agent-fixture",
            "provider-terminal-recovery-required",
        ])
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
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, true);

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_open",
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
        "req_recovery_defaults",
        "project.getRoleSettings",
        serde_json::json!({"projectId":project_id}),
    );
    assert!(matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if response.ok));
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_settings",
        "project.updateRoleSettings",
        serde_json::json!({
            "projectId":project_id,"expectedRevision":1,
            "roles":[
                {"roleId":"planning","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"},
                {"roleId":"coding","runtime":"claude-code","modelSelector":"claude-opus-5","reasoningEffort":null,"permission":"workspaceWrite"},
                {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"medium","permission":"readOnly"}
            ]
        }),
    );
    assert!(matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if response.ok));
    let prepared = prepare_workspace(
        &mut stdin,
        &envelopes,
        &ready.sidecar_instance_id,
        &project_id,
        "req_recovery_prepare",
    );
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_chat",
        "chat.create",
        serde_json::json!({
            "preparationId":prepared["preparationId"],"projectId":project_id,
            "worktreeId":prepared["manifest"]["worktreeId"],
            "workspaceIdentity":prepared["manifest"]["workspaceIdentity"],
            "goal":"provider recovery terminal","expectedSettingsRevision":2
        }),
    );
    let EnvelopeV1::Response(chat_response) = receive_envelope(&envelopes) else {
        panic!("chat")
    };
    let chat = chat_response.result.unwrap();
    let EnvelopeV1::Event(initial) = receive_envelope(&envelopes) else {
        panic!("initial message")
    };
    let message_id = initial.payload["messageId"].as_str().unwrap().to_string();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_capability",
        "agent.getCapabilities",
        serde_json::json!({"runtime":"claude-code","refresh":true}),
    );
    let EnvelopeV1::Response(capability) = receive_envelope(&envelopes) else {
        panic!("capability")
    };
    let capability = capability.result.unwrap();
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_start",
        "run.startAgent",
        serde_json::json!({
            "chatId":chat["chatId"],"roleId":"coding","inputMessageIds":[message_id],
            "expectedCapabilityFingerprint":capability["fingerprint"]
        }),
    );
    let EnvelopeV1::Response(accepted) = receive_envelope(&envelopes) else {
        panic!("accepted")
    };
    let run_id = accepted.result.unwrap()["runId"]
        .as_str()
        .unwrap()
        .to_string();
    let EnvelopeV1::Event(running) = receive_envelope(&envelopes) else {
        panic!("running")
    };
    assert_eq!(running.payload["run"]["state"], "running");
    let EnvelopeV1::Event(terminal) = receive_envelope(&envelopes) else {
        panic!("terminal")
    };
    assert_eq!(terminal.payload["run"]["state"], "recoveryRequired");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_detail",
        "chat.get",
        serde_json::json!({"chatId":chat["chatId"],"afterEventSequence":0,"cursor":null,"limit":50}),
    );
    let EnvelopeV1::Response(detail) = receive_envelope(&envelopes) else {
        panic!("detail")
    };
    let detail = detail.result.unwrap();
    assert_eq!(detail["agentRuns"][0]["state"], "recoveryRequired");

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_quit_blocked",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    let EnvelopeV1::Response(blocked) = receive_envelope(&envelopes) else {
        panic!("blocked quit")
    };
    let blocked = blocked.result.unwrap();
    assert_eq!(blocked["safeToQuit"], false);
    assert!(blocked["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker.as_str() == Some(&format!("run:{run_id}"))));
    assert!(blocked["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker
            .as_str()
            .is_some_and(|value| value.starts_with("lock:"))));

    validate_agent_adapter_payload_v1("agentChatDetailPage", &detail).unwrap();
    let recovery = detail["agentRecovery"].clone();
    let recovery_id = recovery["recoveryId"].as_str().unwrap().to_string();
    assert_eq!(recovery["chatId"], chat["chatId"]);
    assert_eq!(recovery["taskId"], chat["taskId"]);
    assert_eq!(recovery["projectId"], chat["projectId"]);
    assert_eq!(recovery["worktreeId"], chat["worktreeId"]);
    assert_eq!(recovery["workspaceIdentity"], chat["workspaceIdentity"]);
    assert_eq!(recovery["runId"], run_id);
    assert_eq!(recovery["revision"], 1);
    assert_eq!(recovery["state"], "unresolved");
    assert_eq!(recovery["poisoned"], true);
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_inspect",
        "recovery.inspectAgent",
        serde_json::json!({
            "recoveryId":recovery_id,"chatId":chat["chatId"],"taskId":chat["taskId"],
            "projectId":chat["projectId"],"worktreeId":chat["worktreeId"],
            "workspaceIdentity":chat["workspaceIdentity"],"runId":run_id
        }),
    );
    let EnvelopeV1::Response(inspected) = receive_envelope(&envelopes) else {
        panic!("actual recovery inspect")
    };
    let inspected = inspected.result.unwrap();
    assert_eq!(inspected, recovery);
    assert_eq!(inspected["state"], "unresolved");
    assert_eq!(inspected["poisoned"], true);

    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_legacy_list",
        "recovery.list",
        serde_json::json!({"cursor":null,"limit":10}),
    );
    let EnvelopeV1::Response(legacy) = receive_envelope(&envelopes) else {
        panic!("legacy list")
    };
    assert!(legacy.result.unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_resolve",
        "recovery.resolveAgent",
        serde_json::json!({
            "recoveryId":recovery_id,"chatId":chat["chatId"],"taskId":chat["taskId"],
            "projectId":chat["projectId"],"worktreeId":chat["worktreeId"],
            "workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,
            "expectedRevision":1,"resolution":"abandon"
        }),
    );
    let EnvelopeV1::Response(resolved) = receive_envelope(&envelopes) else {
        panic!("resolved")
    };
    assert_eq!(resolved.result.unwrap()["outcome"], "resolvedAbandoned");
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_recovery_quit",
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
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
        G5D_PRODUCT_CAPABILITIES_V1
            .iter()
            .chain(std::iter::once(&"chat.appendMessage"))
            .chain(["agent.outputHead", "agent.outputPage"].iter())
            .chain(std::iter::once(&"provider.login"))
            .chain(["run.approval", "project.catalog", "provider.install"].iter())
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>()
    );
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, false);

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

#[test]
#[ignore = "explicit 100-race native protocol diagnostic; no provider or account"]
fn binary_workspace_save_races_preserve_external_bytes_across_two_chats() {
    use elsecrew_protocol::project_v1::sha256_file_version_v1;
    use serde_json::{json, Value};

    // Preserve data on failure; only confirmed quit permits synthetic cleanup.
    let root = tempfile::Builder::new()
        .prefix("elsecrew-save-races-")
        .tempdir()
        .unwrap()
        .keep()
        .canonicalize()
        .unwrap();
    eprintln!("save race diagnostic: {}", root.display());
    let source = root.join("source");
    let data = root.join("data");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("src/file.txt"), "original").unwrap();
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .arg("--stdio")
        .current_dir(&data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(event) = receive_envelope(&envelopes) else {
        panic!("ready");
    };
    let ready = ready_payload(&event).unwrap();
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, false);
    let mut sequence = 0;
    let mut request = |method: &str, params: Value| -> Value {
        sequence += 1;
        let id = format!("req_save_race_{sequence}");
        write_request(&mut stdin, &ready.sidecar_instance_id, &id, method, params);
        for _ in 0..100 {
            match envelopes
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|error| panic!("{method} {id}: {error}"))
            {
                EnvelopeV1::Response(response) => {
                    assert_eq!(response.request_id, id);
                    assert_eq!(response.sidecar_instance_id, ready.sidecar_instance_id);
                    assert!(response.ok, "{method}: {:?}", response.error);
                    return response.result.unwrap();
                }
                EnvelopeV1::Event(event) => {
                    assert_eq!(event.sidecar_instance_id, ready.sidecar_instance_id)
                }
                _ => panic!("unexpected request from sidecar"),
            }
        }
        panic!("response event budget exceeded");
    };
    let project = request("project.open", json!({"selectedPath": source}));
    let settings = request(
        "project.getRoleSettings",
        json!({"projectId": project["projectId"]}),
    );
    let mut workspaces = Vec::new();
    for index in 0..2 {
        let prepared = request(
            "workspace.prepareChat",
            json!({"projectId": project["projectId"]}),
        );
        let chat = request(
            "chat.create",
            json!({
                "projectId": project["projectId"], "preparationId": prepared["preparationId"],
                "worktreeId": prepared["manifest"]["worktreeId"],
                "workspaceIdentity": prepared["manifest"]["workspaceIdentity"],
                "goal": format!("save race {index}"), "expectedSettingsRevision": settings["revision"]
            }),
        );
        let address = json!({"chatId": chat["chatId"], "projectId": chat["projectId"],
            "worktreeId": chat["worktreeId"], "workspaceIdentity": chat["workspaceIdentity"]});
        let path = std::path::PathBuf::from(prepared["manifest"]["worktreePath"].as_str().unwrap())
            .join("src/file.txt");
        assert!(path.starts_with(&data));
        workspaces.push((address, path));
    }
    let mut observations = Vec::new();
    for index in 0..100 {
        let (address, path) = &workspaces[index % 2];
        let other = &workspaces[1 - index % 2].1;
        let other_before = fs::read(other).unwrap();
        let external = format!("external-{index}");
        let replacement = format!("saved-{index}");
        for conflict in [true, false] {
            let expected = sha256_file_version_v1(&fs::read(path).unwrap());
            let addressed = |fields: Value| {
                let mut params = address.clone();
                params
                    .as_object_mut()
                    .unwrap()
                    .extend(fields.as_object().unwrap().clone());
                params
            };
            let begin = request(
                "workspace.file.save",
                addressed(json!({"path":"src/file.txt",
                "expectedVersion":expected,"totalBytes":replacement.len(),"encoding":"utf8"})),
            );
            if conflict && index % 2 == 0 {
                fs::write(path, &external).unwrap();
            }
            request(
                "workspace.file.save",
                addressed(json!({"saveId":begin["saveId"],"seq":1,"text":replacement})),
            );
            if conflict && index % 2 == 1 {
                fs::write(path, &external).unwrap();
            }
            let committed = request(
                "workspace.file.save",
                addressed(json!({"saveId":begin["saveId"],
                "expectedVersion":expected,"lastSeq":1,"totalBytes":replacement.len()})),
            );
            for key in ["chatId", "projectId", "worktreeId", "workspaceIdentity"] {
                assert_eq!(committed[key], address[key]);
            }
            assert_eq!(
                committed["outcome"],
                if conflict {
                    "dirtyConflict"
                } else {
                    "committed"
                }
            );
            assert_eq!(
                fs::read(path).unwrap(),
                if conflict {
                    external.as_bytes()
                } else {
                    replacement.as_bytes()
                }
            );
            assert_eq!(fs::read(other).unwrap(), other_before);
            assert_eq!(fs::read(source.join("src/file.txt")).unwrap(), b"original");
        }
        observations.push(json!({"trial":index,"chatId":address["chatId"],
            "externalWriteStage":if index % 2 == 0 {"afterBegin"} else {"afterChunk"},
            "reportedConflict":true,"externalBytesPreserved":true,"controlSaveCommitted":true,
            "otherChatUnchanged":true,"sourceUnchanged":true}));
    }
    let quit = request("app.prepareQuit", json!({}));
    assert_eq!(quit["safeToQuit"], true);
    drop(request);
    drop(stdin);
    assert!(child.wait().unwrap().success());
    println!(
        "{}",
        json!({"evidence":"native-workspace-save-races", "diagnostic":true,
        "gateEligible":false,"saveRaces":100,"controlSaves":100,"observations":observations,
        "counters":{"silentOverwrites":0,"unreportedConflicts":0,"crossChatWrites":0},
        "shutdown":"accepted","limitations":["not packaged UI; no agent writes; controlled interleavings before commit"]})
    );
    fs::remove_dir_all(&root).unwrap();
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
    let status = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
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
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, false);
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
    let followup = serde_json::json!({
        "projectId":project_id,"chatId":chat_a["chatId"],"taskId":chat_a["taskId"],
        "worktreeId":chat_a["worktreeId"],"workspaceIdentity":chat_a["workspaceIdentity"],
        "clientMessageId":"user_0123456789abcdef0123456789abcdef",
        "expectedEventSequence":1,"text":"Follow-up through the real wire",
    });
    let mut first_message = None;
    for request_id in ["req_followup_first", "req_followup_retry"] {
        write_request(
            &mut stdin,
            &ready.sidecar_instance_id,
            request_id,
            "chat.appendMessage",
            followup.clone(),
        );
        let EnvelopeV1::Response(response) = receive_envelope(&envelopes) else {
            panic!("follow-up response");
        };
        assert!(response.ok, "{:?}", response.error);
        let message = response.result.unwrap();
        validate_g5_payload_v1(
            "chatAppendMessageTranscript",
            &serde_json::json!({"request":followup,"message":message}),
        )
        .unwrap();
        if let Some(first) = &first_message {
            assert_eq!(&message, first);
        } else {
            assert!(
                matches!(receive_envelope(&envelopes), EnvelopeV1::Event(event) if event.event == "chat.eventAppended" && event.payload == message)
            );
            first_message = Some(message);
        }
    }
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
    use elsecrew_core::proctree::{
        identity_of, ProbeResult, ProcTable, SystemProcTable, TreeTracker,
    };

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
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .args(["--stdio-agent-fixture", "clean"])
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
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, true);

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

    // Observe from outside the sidecar before cancellation. Keeping identities,
    // rather than walking only the post-cancel tree, also detects reparenting.
    let sidecar_identity = identity_of(child.id()).unwrap();
    let mut observer = TreeTracker::new(sidecar_identity);
    let observation_deadline = Instant::now() + Duration::from_secs(3);
    let observed = loop {
        observer.observe(&SystemProcTable).unwrap();
        let descendants: Vec<_> = observer
            .tracked_identities()
            .into_iter()
            .filter(|identity| *identity != sidecar_identity)
            .collect();
        if descendants.len() >= 2 {
            break descendants;
        }
        assert!(
            Instant::now() < observation_deadline,
            "fixture parent and leaf were not observed"
        );
        thread::sleep(Duration::from_millis(10));
    };
    let detached: Vec<_> = observed
        .iter()
        .filter(|identity| {
            // getsid only observes these already-owned fixture identities.
            unsafe { libc::getsid(identity.pid as libc::pid_t) == identity.pid as libc::pid_t }
        })
        .collect();
    assert_eq!(
        detached.len(),
        1,
        "fixture leaf must lead its own OS session"
    );
    assert_ne!(
        unsafe { libc::getsid(child.id() as libc::pid_t) },
        detached[0].pid as libc::pid_t
    );

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
    let canceled = cancel.result.unwrap();
    assert_eq!(canceled["run"]["runId"], run_id);
    assert_eq!(canceled["run"]["chatId"], chat_id);
    assert_eq!(canceled["run"]["state"], canceled["outcome"]);
    assert_eq!(canceled["run"]["terminationEvidence"]["processesProvenGone"], clean);
    assert_eq!(
        canceled["outcome"],
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
    for identity in &observed {
        assert_eq!(
            SystemProcTable.probe(*identity),
            ProbeResult::Gone,
            "observed fixture identity survived cancellation: {identity:?}"
        );
    }
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
    assert_eq!(SystemProcTable.probe(sidecar_identity), ProbeResult::Gone);
    println!(
        "{}",
        serde_json::json!({
            "evidence":"fixture-cancel-external-process-observation",
            "scenario":scenario,"diagnostic":true,"gateEligible":false,
            "observedDescendants":observed.len(),"survivingObservedProcesses":0,
            "detachedSessionDescendants":detached.len(),
            "uncertainReported":!clean,"shutdown":"accepted",
            "limitations":["session-detached fixture, not an independently reparented orphan or real provider", "observer shares the core OS probe implementation"]
        })
    );
}

fn wait_for_storage_ready(
    stdin: &mut impl Write,
    envelopes: &Receiver<EnvelopeV1>,
    instance: &str,
    agent_fixture: bool,
) {
    wait_for_storage_state(
        stdin,
        envelopes,
        instance,
        agent_fixture,
        StorageAdmissionStateV1::Ready,
    );
}

fn initialize_database_with_sidecar(data_root: &std::path::Path) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elsecrew-sidecar"))
        .arg("--stdio")
        .current_dir(data_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let envelopes = spawn_envelope_reader(child.stdout.take().unwrap());
    let EnvelopeV1::Event(ready_event) = receive_envelope(&envelopes) else {
        panic!("initial shell ready")
    };
    let ready = ready_payload(&ready_event).unwrap();
    wait_for_storage_ready(&mut stdin, &envelopes, &ready.sidecar_instance_id, false);
    write_request(
        &mut stdin,
        &ready.sidecar_instance_id,
        "req_initialize_quit",
        "app.prepareQuit",
        serde_json::json!({}),
    );
    assert!(
        matches!(receive_envelope(&envelopes), EnvelopeV1::Response(response) if response.result.as_ref().unwrap()["safeToQuit"] == true)
    );
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

fn wait_for_storage_state(
    stdin: &mut impl Write,
    envelopes: &Receiver<EnvelopeV1>,
    instance: &str,
    agent_fixture: bool,
    expected: StorageAdmissionStateV1,
) {
    let poll_sequence = ADMISSION_POLL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    for attempt in 0..1_000_u32 {
        write_request(
            stdin,
            instance,
            &format!("req_admission_{poll_sequence}_{attempt}"),
            "system.status",
            serde_json::json!({}),
        );
        let EnvelopeV1::Response(response) = receive_envelope(envelopes) else {
            panic!("storage admission status response")
        };
        let value = response.result.expect("storage admission status result");
        let state = if agent_fixture {
            parse_g5d_agent_fixture_system_status_v1(&value)
                .expect("valid agent fixture G5D status")
                .storage_admission
                .state
        } else {
            parse_g5d_product_system_status_v1(&value)
                .expect("valid product G5D status")
                .storage_admission
                .state
        };
        if state == expected {
            return;
        }
        assert_eq!(
            state,
            StorageAdmissionStateV1::Verifying,
            "admission reached an unexpected terminal state"
        );
        thread::sleep(Duration::from_millis(2));
    }
    panic!("storage admission did not reach {expected:?}");
}

fn wait_for_storage_ready_reader(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    instance: &str,
    agent_fixture: bool,
) {
    let poll_sequence = ADMISSION_POLL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    for attempt in 0..1_000_u32 {
        write_request(
            stdin,
            instance,
            &format!("req_admission_reader_{poll_sequence}_{attempt}"),
            "system.status",
            serde_json::json!({}),
        );
        let mut response_line = String::new();
        stdout
            .read_line(&mut response_line)
            .expect("read admission response");
        let EnvelopeV1::Response(response) =
            decode_jsonl(response_line.as_bytes()).expect("decode admission response")
        else {
            panic!("admission request must receive a response")
        };
        let value = response.result.expect("admission status result");
        let state = if agent_fixture {
            parse_g5d_agent_fixture_system_status_v1(&value)
                .unwrap()
                .storage_admission
                .state
        } else {
            parse_g5d_product_system_status_v1(&value)
                .unwrap()
                .storage_admission
                .state
        };
        if state == StorageAdmissionStateV1::Ready {
            return;
        }
        assert_eq!(state, StorageAdmissionStateV1::Verifying);
        thread::sleep(Duration::from_millis(2));
    }
    panic!("storage admission did not become ready");
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

fn write_raw_request_with_params(
    stdin: &mut impl Write,
    instance: &str,
    request_id: &str,
    method: &str,
    raw_params: &str,
) {
    let line = format!(
        "{{\"protocolVersion\":1,\"kind\":\"request\",\"sidecarInstanceId\":{},\"requestId\":{},\"method\":{},\"params\":{raw_params}}}\n",
        serde_json::to_string(instance).unwrap(),
        serde_json::to_string(request_id).unwrap(),
        serde_json::to_string(method).unwrap(),
    );
    stdin.write_all(line.as_bytes()).unwrap();
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
