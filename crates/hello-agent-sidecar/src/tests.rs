use super::*;
use hello_agent_protocol::lifecycle_g5d_v1::{
    parse_current_agent_fixture_system_status_v1 as parse_g5d_agent_fixture_system_status_v1,
    parse_current_product_system_status_v1 as parse_g5d_product_system_status_v1,
    validate_g5d_blocked_request_error_v1, StorageAdmissionStateV1,
};
use hello_agent_protocol::run_recovery_v1::validate_g5d_product_ready_payload_v1;
use hello_agent_protocol::{decode_jsonl, EnvelopeV1, RequestV1};
use rusqlite::{Connection, OptionalExtension};
use serde_json::json;
use std::ffi::OsString;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn database() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("temporary directory");
    let path = root.path().join("hello-agent.sqlite3");
    (root, path)
}

#[test]
fn approval_errors_are_request_scoped_and_do_not_stop_the_session() {
    let (_root, path) = database();
    let instance = "approval_session";
    let mut sidecar = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::blocked());
    sidecar.start_admission(&path);
    wait_until_admitted(&mut sidecar);
    let identity = json!({"chatId":"chat_01","taskId":"task_01","projectId":"project_01","worktreeId":"worktree_01","workspaceIdentity":"workspace_01","runId":"agent_run_01"});
    for params in [
        json!({"action":"list","identity":identity}),
        json!({"action":"accept","identity":identity}),
        json!({"action":"decline","identity":identity,"approvalId":"../x"}),
        json!({"action":"list","identity":identity,"force":true}),
    ] {
        let response = sidecar
            .handle_request(request_value(
                instance,
                "approval_request",
                "run.approval",
                params,
            ))
            .unwrap()
            .response
            .unwrap();
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "approvalUnavailable");
    }
    let status = sidecar
        .handle_request(request_value(
            instance,
            "status_after_approval",
            "system.status",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(status.ok);
}

fn request_value(instance: &str, id: &str, method: &str, params: Value) -> RequestV1 {
    RequestV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "request".to_string(),
        sidecar_instance_id: instance.to_string(),
        request_id: id.to_string(),
        method: method.to_string(),
        params,
    }
}

fn wait_until_admitted(sidecar: &mut SidecarSession) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        sidecar.poll_admission();
        if !matches!(sidecar.admission, AdmissionState::Verifying(_)) {
            return;
        }
        assert!(Instant::now() < deadline, "storage admission timed out");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn g5d_shell_ready_precedes_storage_and_verifying_blocks_before_params_or_side_effects() {
    let (_root, path) = database();
    let instance = "sidecar_g5d_verifying";
    let gate = Arc::new(AtomicBool::new(false));
    let mut sidecar =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());

    let ready = sidecar.ready_event().expect("shell ready");
    let mut baseline = ready.payload.clone();
    assert_eq!(
        baseline["capabilities"].as_array_mut().unwrap().pop(),
        Some(json!("run.approval"))
    );
    assert_eq!(
        baseline["capabilities"].as_array_mut().unwrap().pop(),
        Some(json!("provider.login"))
    );
    assert_eq!(
        baseline["capabilities"].as_array_mut().unwrap().pop(),
        Some(json!("chat.appendMessage"))
    );
    validate_g5d_product_ready_payload_v1(&baseline).expect("exact product D1 baseline");
    assert!(
        !path.exists(),
        "building shell readiness must not touch SQLite"
    );
    sidecar.start_admission_with_gate(&path, Some(Arc::clone(&gate)));

    let status_request = request_value(instance, "req_status", "system.status", json!({}));
    let status = sidecar
        .handle_request(status_request)
        .unwrap()
        .response
        .unwrap();
    let status = status.result.expect("verifying status");
    let parsed = parse_g5d_product_system_status_v1(&status).expect("valid verifying status");
    assert_eq!(
        parsed.storage_admission.state,
        StorageAdmissionStateV1::Verifying
    );

    for (id, method, params) in [
        ("req_project", "project.open", json!({"selectedPath": 7})),
        (
            "req_storage",
            "storage.status",
            json!({"schemaVersion": "bad"}),
        ),
        ("req_recovery", "recovery.markResolved", json!(null)),
        ("req_agent", "run.startAgent", json!([])),
        ("req_followup", "chat.appendMessage", json!(null)),
    ] {
        let response = sidecar
            .handle_request(request_value(instance, id, method, params))
            .unwrap()
            .response
            .unwrap();
        assert!(!response.ok, "{method} must be blocked while verifying");
        validate_g5d_blocked_request_error_v1(
            StorageAdmissionStateV1::Verifying,
            response.error.as_ref().unwrap(),
        )
        .expect("fixed verifying error");
    }
    assert!(!path.exists(), "blocked requests must not create storage");

    let quit = sidecar
        .handle_request(request_value(
            instance,
            "req_quit",
            "app.prepareQuit",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert_eq!(quit.result.unwrap()["safeToQuit"], true);
    assert!(sidecar.safe_to_quit());
    assert!(
        !path.exists(),
        "cancelled pre-open admission must leave no database"
    );
    gate.store(true, Ordering::Release);
}

#[test]
fn g5d_success_and_failure_states_are_exact_and_failure_is_sticky() {
    let (_root, path) = database();
    let instance = "sidecar_g5d_ready";
    let mut ready_sidecar =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());
    ready_sidecar.start_admission(&path);
    wait_until_admitted(&mut ready_sidecar);
    assert!(matches!(
        ready_sidecar.admission,
        AdmissionState::Ready { .. }
    ));

    let status = ready_sidecar
        .handle_request(request_value(
            instance,
            "req_ready",
            "system.status",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap()
        .result
        .unwrap();
    let parsed = parse_g5d_product_system_status_v1(&status).expect("valid ready status");
    assert_eq!(
        parsed.storage_admission.state,
        StorageAdmissionStateV1::Ready
    );
    let storage = ready_sidecar
        .handle_request(request_value(
            instance,
            "req_storage",
            "storage.status",
            json!({"schemaVersion":1}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(
        storage.ok,
        "storage.status is available only after admission"
    );
    let _ = ready_sidecar
        .handle_request(request_value(
            instance,
            "req_ready_quit",
            "app.prepareQuit",
            json!({}),
        ))
        .unwrap();
    let blocked = ready_sidecar
        .handle_request(request_value(
            instance,
            "req_followup_after_quit",
            "chat.appendMessage",
            json!(null),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(!blocked.ok);
    assert_eq!(blocked.error.unwrap().kind, "requestBlocked");
    drop(ready_sidecar);

    let restart_gate = Arc::new(AtomicBool::new(false));
    let mut restarted = SidecarSession::new(
        "sidecar_g5d_restart".to_string(),
        agent_runtime::AgentRuntime::blocked(),
    );
    restarted.start_admission_with_gate(&path, Some(Arc::clone(&restart_gate)));
    let restart_status = restarted
        .handle_request(request_value(
            "sidecar_g5d_restart",
            "req_restart_status",
            "system.status",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        parse_g5d_product_system_status_v1(&restart_status)
            .unwrap()
            .storage_admission
            .state,
        StorageAdmissionStateV1::Verifying,
        "every process restart must re-enter verification"
    );
    let _ = restarted
        .handle_request(request_value(
            "sidecar_g5d_restart",
            "req_restart_quit",
            "app.prepareQuit",
            json!({}),
        ))
        .unwrap();
    restart_gate.store(true, Ordering::Release);

    let bad_root = tempfile::tempdir().unwrap();
    let bad_path = bad_root.path().join("hello-agent.sqlite3");
    fs::create_dir(&bad_path).unwrap();
    let bad_instance = "sidecar_g5d_failed";
    let mut failed = SidecarSession::new(
        bad_instance.to_string(),
        agent_runtime::AgentRuntime::blocked(),
    );
    failed.start_admission(&bad_path);
    wait_until_admitted(&mut failed);
    assert!(matches!(failed.admission, AdmissionState::Failed { .. }));
    for id in ["req_failed_1", "req_failed_2"] {
        let response = failed
            .handle_request(request_value(
                bad_instance,
                id,
                "project.open",
                json!({"selectedPath": 7}),
            ))
            .unwrap()
            .response
            .unwrap();
        validate_g5d_blocked_request_error_v1(
            StorageAdmissionStateV1::Failed,
            response.error.as_ref().unwrap(),
        )
        .expect("fixed sticky failure error");
    }
    let failed_status = failed
        .handle_request(request_value(
            bad_instance,
            "req_failed_status",
            "system.status",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap()
        .result
        .unwrap();
    let failed_status =
        parse_g5d_product_system_status_v1(&failed_status).expect("valid failed status");
    assert_eq!(
        failed_status.storage_admission.state,
        StorageAdmissionStateV1::Failed
    );
}

#[test]
fn g5d_agent_fixture_advertises_exact_d1_capabilities_before_storage() {
    let runtime = agent_runtime::AgentRuntime::fake(
        std::env::current_exe().unwrap(),
        agent_runtime::FakeScenario::Clean,
    );
    let sidecar = SidecarSession::new("sidecar_g5d_fixture".to_string(), runtime);
    let ready = sidecar.ready_event().unwrap();
    let status = serde_json::to_value(sidecar.system_status().unwrap()).unwrap();
    parse_g5d_agent_fixture_system_status_v1(&status).expect("exact fixture D1 status");
    assert_eq!(ready.event, "system.ready");
}

#[test]
fn admission_preflight_preserves_strict_allowed_and_ready_request_validation() {
    let (_root, path) = database();
    let instance = "sidecar_preflight_scope";
    let gate = Arc::new(AtomicBool::new(false));
    let mut sidecar =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());
    sidecar.start_admission_with_gate(&path, Some(Arc::clone(&gate)));
    let raw = |method: &str| {
        format!(
            "{{\"protocolVersion\":1,\"kind\":\"request\",\"sidecarInstanceId\":\"{instance}\",\"requestId\":\"req_preflight\",\"method\":\"{method}\",\"params\":null}}\n"
        )
    };
    assert!(
        preflight_blocked_admission_request(raw("project.open").as_bytes(), &sidecar).is_some()
    );
    assert!(
        preflight_blocked_admission_request(raw("system.status").as_bytes(), &sidecar).is_none()
    );
    assert!(
        preflight_blocked_admission_request(raw("app.prepareQuit").as_bytes(), &sidecar).is_none()
    );

    gate.store(true, Ordering::Release);
    wait_until_admitted(&mut sidecar);
    assert!(
        preflight_blocked_admission_request(raw("project.open").as_bytes(), &sidecar).is_none()
    );
}

#[test]
fn project_error_response_preserves_ambiguous_save_outcome() {
    let request = RequestV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "request".to_string(),
        sidecar_instance_id: "sidecar_unknown_save".to_string(),
        request_id: "req_unknown_save".to_string(),
        method: "file.save".to_string(),
        params: serde_json::json!({}),
    };
    let response = project_error_response(
        &request,
        &ProjectFailure::ambiguous("rename completed before durable confirmation"),
    );
    assert!(!response.ok);
    assert_eq!(
        response.error.unwrap().outcome_unknown,
        Some(true),
        "library response must not downgrade an ambiguous replacement"
    );
}

#[test]
fn invalid_g4_params_return_structured_error_without_killing_the_session() {
    let (_root, path) = database();
    let instance = "sidecar_invalid_g4_params";
    let mut sidecar =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());
    sidecar.start_admission(&path);
    wait_until_admitted(&mut sidecar);
    for request in [
        request_value(instance, "req_invalid", "project.open", json!({})),
        request_value(
            instance,
            "req_invalid_save",
            "file.save",
            json!({"saveId":"save_01","seq":"one","text":"hello"}),
        ),
    ] {
        let invalid = sidecar.handle_request(request).unwrap().response.unwrap();
        assert!(!invalid.ok);
        assert_eq!(invalid.error.as_ref().unwrap().code, "invalidParams");
    }
    let quit = sidecar
        .handle_request(request_value(
            instance,
            "req_quit_after_invalid",
            "app.prepareQuit",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(quit.ok);
}

fn output_envelopes(output: &[u8]) -> Vec<EnvelopeV1> {
    output
        .split_inclusive(|byte| *byte == b'\n')
        .map(|line| decode_jsonl(line).expect("stdout must be protocol-only JSONL"))
        .collect()
}

#[test]
fn ready_status_and_idempotent_prepare_quit_mark_clean_before_safe_response() {
    let (_root, path) = database();
    let instance = "sidecar_transport";
    let mut sidecar =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());
    sidecar.start_admission(&path);
    wait_until_admitted(&mut sidecar);
    let status_response = sidecar
        .handle_request(request_value(
            instance,
            "req_status",
            "system.status",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap();
    parse_g5d_product_system_status_v1(&status_response.result.unwrap()).expect("ready status");
    for request_id in ["req_quit_1", "req_quit_2"] {
        let response = sidecar
            .handle_request(request_value(
                instance,
                request_id,
                "app.prepareQuit",
                json!({}),
            ))
            .unwrap()
            .response
            .unwrap();
        assert!(response.ok);
        assert_eq!(response.result.as_ref().unwrap()["safeToQuit"], true);
    }
    drop(sidecar);

    let connection = Connection::open(&path).expect("inspect database");
    let clean: Option<String> = connection
        .query_row(
            "SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id=?1;",
            [instance],
            |row| row.get(0),
        )
        .expect("clean marker");
    let clean_operations: u32 = connection
        .query_row(
            "SELECT count(*) FROM events WHERE operation_id=?1;",
            [format!("op_clean_{instance}")],
            |row| row.get(0),
        )
        .expect("clean event count");
    assert!(clean.is_some());
    assert_eq!(clean_operations, 1);
}

#[test]
fn stdin_eof_without_prepare_quit_never_marks_clean() {
    let (_root, path) = database();
    let instance = "sidecar_unclean_eof";
    let mut output = Vec::new();
    let error = serve_stdio_with_identity(
        &path,
        Cursor::new(Vec::<u8>::new()),
        &mut output,
        instance.to_string(),
    )
    .expect_err("unexpected EOF");
    assert_eq!(error.kind(), "unexpectedEof");
    assert_eq!(output_envelopes(&output).len(), 1);

    if path.exists() {
        let connection = Connection::open(&path).expect("inspect database");
        let app_instances_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='app_instances');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        if app_instances_exists {
            let clean: Option<String> = connection
                .query_row(
                    "SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id=?1;",
                    [instance],
                    |row| row.get(0),
                )
                .optional()
                .expect("optional clean marker")
                .flatten();
            assert_eq!(clean, None);
        }
    }
}

#[test]
fn malformed_oversize_and_partial_jsonl_fail_closed_after_ready() {
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (b"not-json\n".to_vec(), "protocolViolation"),
        (
            {
                let mut bytes = vec![b'a'; MAX_CONTROL_LINE_BYTES + 1];
                bytes.push(b'\n');
                bytes
            },
            "controlLineTooLarge",
        ),
        (b"{".to_vec(), "partialJsonl"),
    ];
    for (index, (input, expected_kind)) in cases.into_iter().enumerate() {
        let (_root, path) = database();
        let mut output = Vec::new();
        let error = serve_stdio_with_identity(
            &path,
            Cursor::new(input),
            &mut output,
            format!("sidecar_invalid_{index}"),
        )
        .expect_err("invalid transport must fail");
        assert_eq!(error.kind(), expected_kind);
        assert_eq!(output_envelopes(&output).len(), 1);
    }
}

#[test]
fn startup_failure_becomes_sticky_admission_failure_after_shell_ready() {
    let root = tempfile::tempdir().unwrap();
    let database_path = root.path().join("hello-agent.sqlite3");
    std::fs::create_dir(&database_path).unwrap();
    let mut sidecar = SidecarSession::new(
        "sidecar_startup_failure".to_string(),
        agent_runtime::AgentRuntime::blocked(),
    );
    assert_eq!(sidecar.ready_event().unwrap().event, "system.ready");
    sidecar.start_admission(&database_path);
    wait_until_admitted(&mut sidecar);
    assert!(matches!(sidecar.admission, AdmissionState::Failed { .. }));
}

#[test]
fn cooperative_scheduler_has_zero_ticks_before_ready_and_after_prepare_quit() {
    let (_root, path) = database();
    let gate = Arc::new(AtomicBool::new(false));
    let mut verifying = SidecarSession::new(
        "sidecar_scheduler_verifying".to_string(),
        agent_runtime::AgentRuntime::blocked(),
    );
    verifying.start_admission_with_gate(&path, Some(Arc::clone(&gate)));
    verifying.maintenance_tick().unwrap();
    assert!(!path.exists());
    let _ = verifying
        .handle_request(request_value(
            "sidecar_scheduler_verifying",
            "req_scheduler_quit_verifying",
            "app.prepareQuit",
            json!({}),
        ))
        .unwrap();
    gate.store(true, Ordering::Release);

    let bad_root = tempfile::tempdir().unwrap();
    let bad_path = bad_root.path().join("hello-agent.sqlite3");
    fs::create_dir(&bad_path).unwrap();
    let mut failed = SidecarSession::new(
        "sidecar_scheduler_failed".to_string(),
        agent_runtime::AgentRuntime::blocked(),
    );
    failed.start_admission(&bad_path);
    wait_until_admitted(&mut failed);
    failed.maintenance_tick().unwrap();

    let (_root, path) = database();
    let instance = "sidecar_scheduler_quitting";
    let mut ready =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());
    ready.start_admission(&path);
    wait_until_admitted(&mut ready);
    let _ = ready
        .handle_request(request_value(
            instance,
            "req_scheduler_quit_ready",
            "app.prepareQuit",
            json!({}),
        ))
        .unwrap();
    let AdmissionState::Ready { sidecar, .. } = &mut ready.admission else {
        panic!("ready sidecar")
    };
    sidecar
        .storage
        .connection_for_test()
        .execute_batch("DROP TABLE lifetime_materialization_job;")
        .unwrap();
    assert_eq!(
        sidecar.maintenance_tick().unwrap(),
        storage_lifetime::scheduler::MaintenanceTickResult::NoWork,
        "prepareQuit must block scheduler access before cleanup and clean marking"
    );
}

#[test]
fn fatal_scheduler_error_poisons_later_mutation_and_agent_run() {
    let (_root, path) = database();
    let instance = "sidecar_scheduler_poison";
    let mut session =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());
    session.start_admission(&path);
    wait_until_admitted(&mut session);
    let AdmissionState::Ready { sidecar, .. } = &mut session.admission else {
        panic!("ready sidecar")
    };
    sidecar
        .storage
        .connection_for_test()
        .execute_batch("DROP TABLE lifetime_materialization_job;")
        .unwrap();
    assert!(sidecar.maintenance_tick().is_err());
    assert!(sidecar.maintenance_poisoned);

    for (id, method, params) in [
        (
            "req_poison_project",
            "project.open",
            json!({"selectedPath":"/fixture"}),
        ),
        (
            "req_poison_agent",
            "run.startAgent",
            json!({"schemaVersion":1}),
        ),
    ] {
        let error = match sidecar.handle_request(request_value(instance, id, method, params)) {
            Ok(_) => panic!("poison must fail closed before mutation or run validation"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), "storageMaintenancePoisoned");
    }
}

#[test]
fn later_wave_method_is_structured_error_and_read_only_status_remains_available_after_quit() {
    let (_root, path) = database();
    let instance = "sidecar_capabilities";
    let mut sidecar =
        SidecarSession::new(instance.to_string(), agent_runtime::AgentRuntime::blocked());
    sidecar.start_admission(&path);
    wait_until_admitted(&mut sidecar);
    let unavailable = sidecar
        .handle_request(request_value(
            instance,
            "req_later",
            "run.startAgentSmoke",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(!unavailable.ok);
    assert_eq!(
        unavailable.error.as_ref().unwrap().code,
        "methodUnavailable"
    );
    let _quit = sidecar
        .handle_request(request_value(
            instance,
            "req_quit",
            "app.prepareQuit",
            json!({}),
        ))
        .unwrap();
    let status = sidecar
        .handle_request(request_value(
            instance,
            "req_status_after",
            "system.status",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(status.ok);
}

#[test]
fn cli_rejects_missing_or_extra_arguments_before_touching_stdio() {
    for args in [
        vec![OsString::from("sidecar")],
        vec![
            OsString::from("sidecar"),
            OsString::from("--stdio"),
            OsString::from("database.sqlite3"),
        ],
        vec![OsString::from("sidecar"), OsString::from("--other")],
    ] {
        let error = run_cli(args).expect_err("invalid argv");
        assert_eq!(error.kind(), "invalidArguments");
    }
}
