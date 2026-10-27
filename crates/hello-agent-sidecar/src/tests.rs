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
fn runtime_error_survives_a_concurrent_completed_response() {
    let (_root, path) = database();
    let instance = "deferred_runtime_error";
    let mut session = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::blocked());
    session.start_admission(&path);
    wait_until_admitted(&mut session);
    let AdmissionState::Ready { sidecar, .. } = &mut session.admission else { panic!("not ready") };
    let request = request_value(instance, "concurrent_status", "system.status", json!({}));
    let response = success_response(&request, json!({}));
    let failure = SidecarError::new("agentRuntimeFailure", "injected concurrent cleanup failure");
    let (responses, events) = sidecar.finish_runtime_poll(vec![response], Vec::new(), Some(failure.clone())).unwrap();
    assert_eq!(responses.len(), 1);
    assert!(events.is_empty());
    assert_eq!(sidecar.poll_runtime_events().unwrap_err(), failure);
    assert!(sidecar.deferred_runtime_error.is_none());
}

#[test]
fn discovery_admission_rejection_keeps_session_available() {
    let (_root, path) = database();
    let instance = "discovery_rejection";
    let mut session = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::native_candidate());
    session.start_admission(&path);
    wait_until_admitted(&mut session);
    for (method, runtime) in [
        ("agent.getModelChoices", "codex"),
        ("agent.getCapabilities", "codex"),
        ("agent.getCapabilities", "claude-code"),
    ] {
        let handled = session.handle_request(request_value(instance, "rejected", method, json!({
            "runtime":runtime,"refresh":true,"chatId":"chat_missing","roleId":"coding"
        }))).expect("query rejection must not terminate the service");
        let response = handled.response.expect("immediate rejection");
        assert!(!response.ok);
        let error = response.error.unwrap();
        assert_eq!(error.code, "agentDiscoveryRejected");
        assert_eq!(error.kind, "requestBlocked");
        let status = session.handle_request(request_value(instance, "alive", "system.status", json!({}))).unwrap().response.unwrap();
        assert!(status.ok);
        let AdmissionState::Ready { sidecar, .. } = &session.admission else { unreachable!() };
        assert!(sidecar.pending_agent_capabilities.is_empty());
        assert!(sidecar.storage.projection_records("run").unwrap().is_empty());
        assert!(sidecar.storage.projection_records("lock").unwrap().is_empty());
    }
}

#[test]
fn discovery_rejection_stdio_returns_error_then_status_and_clean_quit() {
    use std::sync::mpsc;
    struct Input(mpsc::Receiver<Vec<u8>>, Cursor<Vec<u8>>);
    impl Read for Input {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.1.position() as usize == self.1.get_ref().len() {
                match self.0.recv() {
                    Ok(bytes) => self.1 = Cursor::new(bytes),
                    Err(_) => return Ok(0),
                }
            }
            self.1.read(buf)
        }
    }
    struct Output(mpsc::Sender<Vec<u8>>);
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.send(bytes.to_vec()).map_err(|_| io::ErrorKind::BrokenPipe)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }
    let (_root, path) = database();
    let instance = "discovery_stdio";
    let (tx, input) = mpsc::channel();
    let (output, rx) = mpsc::channel();
    let worker = thread::spawn(move || serve_stdio_with_identity_and_runtime(
        &path, Input(input, Cursor::new(Vec::new())), Output(output), instance.into(),
        agent_runtime::AgentRuntime::native_candidate(),
    ));
    let mut bytes = Vec::new();
    let mut next = || loop {
        if let Some(end) = bytes.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = bytes.drain(..=end).collect();
            break decode_jsonl(&line).unwrap();
        }
        bytes.extend(rx.recv_timeout(Duration::from_secs(10)).expect("stdio response"));
    };
    assert!(matches!(next(), EnvelopeV1::Event(_)));
    let send = |id: &str, method: &str, params: Value| {
        let request = EnvelopeV1::Request(request_value(instance, id, method, params));
        tx.send(encode_jsonl(&request).unwrap().into_bytes()).unwrap();
    };
    let admission_deadline = Instant::now() + Duration::from_secs(10);
    for index in 0.. {
        send(&format!("admission_{index}"), "system.status", json!({}));
        let EnvelopeV1::Response(status) = next() else { panic!("status response") };
        if status.result.as_ref().is_some_and(|v| v["storageAdmission"]["state"] == "ready") { break; }
        assert!(Instant::now() < admission_deadline, "storage admission did not finish: {:?}", status.result);
        thread::sleep(Duration::from_millis(10));
    }
    send("reject", "agent.getModelChoices", json!({"runtime":"codex","refresh":true,"chatId":"chat_missing","roleId":"coding"}));
    let EnvelopeV1::Response(rejected) = next() else { panic!("rejection response") };
    assert_eq!(rejected.request_id, "reject");
    let error = rejected.error.unwrap();
    assert_eq!(error.code, "agentDiscoveryRejected");
    assert_eq!(error.kind, "requestBlocked");
    send("alive", "system.status", json!({}));
    let EnvelopeV1::Response(status) = next() else { panic!("status response") };
    assert!(status.ok);
    assert_eq!(status.request_id, "alive");
    send("quit", "app.prepareQuit", json!({}));
    let EnvelopeV1::Response(quit) = next() else { panic!("quit response") };
    assert!(quit.ok);
    assert_eq!(quit.request_id, "quit");
    drop(tx);
    worker.join().unwrap().expect("clean stdio exit after rejected discovery");
}

#[test]
fn installation_activation_never_starts_without_a_consent_plan() {
    let (root, path) = database();
    let instance = "install_activation";
    let mut session = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::blocked());
    session.start_admission(&path);
    wait_until_admitted(&mut session);
    let response = session.handle_request(request_value(instance, "status", "provider.install", json!({"action":"status"}))).unwrap().response.unwrap();
    assert!(response.ok);
    assert_eq!(response.result.unwrap()["state"], if cfg!(target_os="macos") { "idle" } else { "unsupported" });
    let response = session.handle_request(request_value(instance, "unconsented", "provider.install", json!({
        "action":"start", "provider":"codex", "planId":"11111111-2222-3333-4444-555555555555"
    }))).unwrap().response.unwrap();
    if cfg!(target_os="macos") { assert!(!response.ok); }
    else { assert_eq!(response.result.unwrap()["state"], "unsupported"); }
    for name in ["provider-install.pending", "provider-install-execute.pending", "provider-install-journal-v1.jsonl", "provider-install-staging-v1"] {
        assert!(!root.path().join(name).exists(), "unconsented action created {name}");
    }
}

#[test]
fn native_session_negotiates_adapter_without_granting_unscoped_execution() {
    let (_root, path) = database();
    let instance = "native_admission_contract";
    let mut session = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::native_candidate());
    let ready = session.ready_event().unwrap();
    let capabilities = ready.payload["capabilities"].as_array().unwrap();
    assert!(capabilities.iter().any(|value| value == "run.startAgent"));
    assert!(!capabilities.iter().any(|value| value == "run.startFixture"));
    let mut baseline = ready.payload.clone();
    baseline["capabilities"] = json!(hello_agent_protocol::agent_adapter_v1::g5d_native_capabilities_v1());
    assert!(hello_agent_protocol::agent_adapter_v1::validate_g5d_native_ready_payload_v1(&baseline).is_ok());
    baseline["capabilities"] = json!(G5D_AGENT_FIXTURE_CAPABILITIES_V1);
    assert!(hello_agent_protocol::agent_adapter_v1::validate_g5d_native_ready_payload_v1(&baseline).is_err());
    baseline["capabilities"] = json!(hello_agent_protocol::agent_adapter_v1::g5d_native_capabilities_v1());
    baseline["capabilities"].as_array_mut().unwrap().push(json!("run.futureFixture"));
    assert!(hello_agent_protocol::agent_adapter_v1::validate_g5d_native_ready_payload_v1(&baseline).is_err());
    session.start_admission(&path);
    wait_until_admitted(&mut session);
    assert!(matches!(session.admission, AdmissionState::Ready { .. }));
    for runtime in ["codex", "claude-code"] {
        let response = session.handle_request(request_value(instance, runtime, "agent.getCapabilities", json!({"runtime":runtime,"refresh":true}))).unwrap().response.unwrap();
        assert!(response.ok, "{response:?}");
        assert_eq!(response.result.unwrap()["canStart"], false);
    }
    let response = session.handle_request(request_value(instance, "invalid_start", "run.startAgent", json!({}))).unwrap().response.unwrap();
    assert!(!response.ok);
    let response = session.handle_request(request_value(instance, "fixture_denied", "run.startFixture", json!({}))).unwrap().response.unwrap();
    assert!(!response.ok);
    let AdmissionState::Ready { sidecar, .. } = &session.admission else { unreachable!() };
    assert!(sidecar.storage.projection_records("run").unwrap().is_empty());
    assert!(sidecar.storage.projection_records("lock").unwrap().is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn installation_admission_preserves_fences_and_allows_status_and_cancellation() {
    let (root, _) = database();
    let path = root.path().canonicalize().unwrap().join("hello-agent.sqlite3");
    let mut session = SidecarSession::new("installation_exclusion".into(), agent_runtime::AgentRuntime::blocked());
    session.start_admission(&path);
    wait_until_admitted(&mut session);
    let AdmissionState::Ready { sidecar, .. } = &mut session.admission else { panic!("admission failed") };
    let request = |method: &str, params: Value| request_value("installation_exclusion", "check", method, params);
    for name in ["codex-login.pending", "claude-login.pending"] {
        let fence = path.parent().unwrap().join(name);
        std::fs::write(&fence, b"unconfirmed").unwrap();
        for action in ["prepare", "start"] {
            assert!(sidecar.installation_conflicts(&request("provider.install", json!({"action":action}))));
        }
        assert!(!sidecar.installation_conflicts(&request("provider.install", json!({"action":"status"}))));
        assert!(!sidecar.installation_conflicts(&request("provider.install", json!({"action":"cancel"}))));
        assert_eq!(std::fs::read(&fence).unwrap(), b"unconfirmed");
        std::fs::remove_file(&fence).unwrap();
    }
    assert!(!sidecar.installation_conflicts(&request("provider.install", json!({"action":"prepare"}))));
    let fence = path.parent().unwrap().join("provider-install.pending");
    // Even a broken link is evidence, not an absent installer.
    std::os::unix::fs::symlink("missing-installation-record", &fence).unwrap();
    for method in ["agent.getCapabilities", "agent.getModelChoices", "run.startAgent"] {
        assert!(sidecar.installation_conflicts(&request(method, json!({}))));
    }
    for action in ["start", "check", "diagnose", "recover"] {
        assert!(sidecar.installation_conflicts(&request("provider.login", json!({"action":action}))));
    }
    for action in ["status", "cancel", "submitCode"] {
        assert!(!sidecar.installation_conflicts(&request("provider.login", json!({"action":action}))));
    }
    for method in ["run.cancelAgent", "recovery.inspectAgent", "recovery.resolveAgent", "app.prepareQuit"] {
        assert!(!sidecar.installation_conflicts(&request(method, json!({}))));
    }
    assert_eq!(std::fs::read_link(&fence).unwrap(), std::path::Path::new("missing-installation-record"));
    assert!(sidecar.storage.projection_records("run").unwrap().is_empty());
}

#[test]
fn catalog_preparation_dispatch_binds_inspection_and_retries_completion() {
    let (_root, path) = database();
    let instance = "preparation_session";
    let mut sidecar = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::blocked());
    sidecar.start_admission(&path);
    wait_until_admitted(&mut sidecar);
    let AdmissionState::Ready { sidecar, .. } = &mut sidecar.admission else { panic!("storage admission missing"); };
    let root = path.parent().unwrap().canonicalize().unwrap();
    let workspace = root.join("worktrees-v1/worktree_failed").to_string_lossy().into_owned();
    let manifest = root.join("worktree-manifests-v1/worktree_failed.json").to_string_lossy().into_owned();
    for (kind, id, payload) in [
        ("project", "project_failed", json!({"project_id":"project_failed","canonical_path":root.to_string_lossy(),"lifecycle":"recoveryRequired","worktree_id":"worktree_failed","workspace_identity":"workspace_failed","lock_id":"lock_worktree_failed","worktree_path":workspace,"manifest_path":manifest})),
        ("lock", "lock_worktree_failed", json!({"lock_id":"lock_worktree_failed","project_id":"project_failed","writer_run_id":null,"worktree_path":workspace,"poisoned":false,"recovery_required":true,"evidence":{"phase":"beforeSnapshotExport","launchFenceVersion":1,"workspace_identity":"workspace_failed"}})),
    ] {
        sidecar.storage.upsert_projection_record(kind, id, &format!("op_seed_{kind}"), Some("req_seed"), payload.as_object().unwrap().clone()).unwrap();
    }
    let inspected = sidecar.handle_request(request_value(instance, "inspect", "project.catalog", json!({"action":"inspectPreparation","projectId":"project_failed"}))).unwrap().response.unwrap();
    assert!(inspected.ok, "{inspected:?}");
    let inspection = inspected.result.unwrap()["inspection"].clone();
    for request_id in ["abandon", "lost_response_retry"] {
        let reply = sidecar.handle_request(request_value(instance, request_id, "project.catalog", json!({"action":"abandonPreparation","inspection":inspection}))).unwrap().response.unwrap();
        assert!(reply.ok, "{reply:?}");
        assert_eq!(reply.result.unwrap()["inspection"], inspection);
    }
    assert!(sidecar.storage.blocker_ids().unwrap().is_empty());
    let mut changed = inspection.clone();
    changed["projectRevision"] = json!(999);
    let rejected = sidecar.handle_request(request_value(instance, "stale", "project.catalog", json!({"action":"abandonPreparation","inspection":changed}))).unwrap().response.unwrap();
    assert!(!rejected.ok);
    sidecar.prepared_to_quit = true;
    let quitting = sidecar.handle_request(request_value(instance, "quitting", "project.catalog", json!({"action":"abandonPreparation","inspection":inspection}))).unwrap().response.unwrap();
    assert!(!quitting.ok);
}

#[test]
fn catalog_dispatch_lists_empty_storage_and_rejects_unknown_reopen() {
    let (_root, path) = database();
    let instance = "catalog_session";
    let mut sidecar = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::blocked());
    sidecar.start_admission(&path);
    wait_until_admitted(&mut sidecar);
    let listed = sidecar
        .handle_request(request_value(
            instance,
            "catalog_list",
            "project.catalog",
            json!({"action":"list","cursor":null}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(listed.ok);
    assert_eq!(
        listed.result,
        Some(json!({"schemaVersion":1,"action":"list","projects":[],"nextCursor":null}))
    );
    let missing = sidecar
        .handle_request(request_value(
            instance,
            "catalog_missing",
            "project.catalog",
            json!({"action":"reopen","projectId":"project_missing"}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(!missing.ok);
    let listed_again = sidecar
        .handle_request(request_value(
            instance,
            "catalog_again",
            "project.catalog",
            json!({"action":"list","cursor":null}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(listed_again.ok);
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
        Some(json!("provider.install"))
    );
    assert_eq!(
        baseline["capabilities"].as_array_mut().unwrap().pop(),
        Some(json!("project.catalog"))
    );
    assert_eq!(
        baseline["capabilities"].as_array_mut().unwrap().pop(),
        Some(json!("run.approval"))
    );
    assert_eq!(
        baseline["capabilities"].as_array_mut().unwrap().pop(),
        Some(json!("provider.login"))
    );
    for capability in ["agent.outputPage", "agent.outputHead", "chat.appendMessage"] {
        assert_eq!(
            baseline["capabilities"].as_array_mut().unwrap().pop(),
            Some(json!(capability))
        );
    }
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
fn long_stream_fixture_admits_durable_storage_without_enabling_normal_startup() {
    assert!(!agent_runtime::AgentRuntime::native_candidate().uses_durable_output_fixture());
    for (scenario, expected) in [(agent_runtime::FakeScenario::Clean, false),
        (agent_runtime::FakeScenario::LongStream, true),
        (agent_runtime::FakeScenario::PacedLongStream, true)] {
        let (_root, path) = database();
        for iteration in 0..2 {
            let runtime = agent_runtime::AgentRuntime::fake(std::env::current_exe().unwrap(), scenario);
            let mut session = SidecarSession::new(format!("durable_fixture_{iteration}"), runtime);
            session.start_admission(&path);
            wait_until_admitted(&mut session);
            let AdmissionState::Ready { sidecar, .. } = &session.admission else {
                panic!("fixture admission did not become ready");
            };
            assert_eq!(sidecar.storage.uses_durable_output().unwrap(), expected);
        }
        if expected {
            let mut normal = SidecarSession::new("normal_after_durable".into(), agent_runtime::AgentRuntime::blocked());
            normal.start_admission(&path);
            wait_until_admitted(&mut normal);
            assert!(!matches!(normal.admission, AdmissionState::Ready { .. }));
        }
    }
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
