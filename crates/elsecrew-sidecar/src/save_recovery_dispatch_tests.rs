use super::*;
use serde_json::json;
use std::{
    thread,
    time::{Duration, Instant},
};

fn database() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("elsecrew.sqlite3");
    (root, path)
}
fn request_value(instance: &str, id: &str, method: &str, params: Value) -> RequestV1 {
    RequestV1 {
        protocol_version: PROTOCOL_VERSION,
        kind: "request".into(),
        sidecar_instance_id: instance.into(),
        request_id: id.into(),
        method: method.into(),
        params,
    }
}
fn wait_until_admitted(session: &mut SidecarSession) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        session.poll_admission();
        if !matches!(session.admission, AdmissionState::Verifying(_)) {
            return;
        }
        assert!(Instant::now() < deadline, "storage admission timed out");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn windows_save_recovery_gate_rejects_direct_confirmed_requests() {
    let (_root, path) = database();
    let instance = "save_recovery_gate";
    let mut session = SidecarSession::new(instance.into(), agent_runtime::AgentRuntime::blocked());
    session.start_admission(&path);
    wait_until_admitted(&mut session);
    let valid = json!({"chatId":"chat_one","projectId":"project_one","worktreeId":"worktree_one",
        "workspaceIdentity":"workspace_one","saveId":"save_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "expectedLockRevision":1,"decision":"complete","confirmed":true});
    for decision in ["complete", "abort"] {
        let mut params = valid.clone();
        params["decision"] = json!(decision);
        let response = session
            .handle_request(request_value(
                instance,
                decision,
                "workspace.resolveSaveRecovery",
                params,
            ))
            .unwrap()
            .response
            .unwrap();
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "methodUnavailable");
    }
    for (field, value) in [
        ("confirmed", json!(false)),
        ("expectedLockRevision", json!(0)),
        (
            "expectedLockRevision",
            json!(elsecrew_protocol::MAX_SEQUENCE + 1),
        ),
        ("decision", json!("retry")),
        ("force", json!(true)),
    ] {
        let mut params = valid.clone();
        params[field] = value;
        let response = session
            .handle_request(request_value(
                instance,
                "invalid_save_recovery",
                "workspace.resolveSaveRecovery",
                params,
            ))
            .unwrap()
            .response
            .unwrap();
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "invalidParams");
    }
    let status = session
        .handle_request(request_value(
            instance,
            "after_save_recovery",
            "system.status",
            json!({}),
        ))
        .unwrap()
        .response
        .unwrap();
    assert!(status.ok);
}
