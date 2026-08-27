use super::*;
use crate::agent_runtime::{AgentRuntime, FakeScenario, RunIdentity};
use hello_agent_protocol::agent_adapter_v1::AgentRunStartRequestV1;
use hello_agent_protocol::project_v1::sha256_file_version_v1;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

struct Fixture {
    _root: TempDir,
    source: std::path::PathBuf,
    storage: Storage,
    projects: ProjectManager,
    chats: G5Manager,
    project_id: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let data = root.path().join("data");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::write(source.join("src/value.txt"), b"base").unwrap();
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
        let database = data.join("hello-agent.sqlite3");
        let mut storage = Storage::open(&database).unwrap();
        let mut projects = ProjectManager::new(&database).unwrap();
        let opened = projects
            .open_project(&mut storage, source.to_str().unwrap(), "req_open")
            .unwrap();
        let project_id = opened.result["projectId"].as_str().unwrap().to_string();
        Self {
            _root: root,
            source,
            storage,
            projects,
            chats: G5Manager::default(),
            project_id,
        }
    }

    fn snapshot(&mut self, request: &str) -> Value {
        let prepared = self
            .chats
            .prepare_chat_workspace(
                &mut self.storage,
                &mut self.projects,
                &self.project_id,
                request,
            )
            .unwrap()
            .result;
        let mut snapshot = prepared["manifest"].clone();
        snapshot["preparationId"] = prepared["preparationId"].clone();
        snapshot
    }

    fn create_chat(&mut self, snapshot: &Value, goal: &str, request: &str) -> Value {
        self.chats
            .default_role_settings(&mut self.storage, &self.project_id, request)
            .unwrap();
        self.chats
            .create_chat(
                &mut self.storage,
                &self.projects,
                snapshot["preparationId"].as_str().unwrap(),
                &self.project_id,
                snapshot["worktreeId"].as_str().unwrap(),
                snapshot["workspaceIdentity"].as_str().unwrap(),
                goal,
                1,
                request,
            )
            .unwrap()
            .result
    }
}

#[test]
fn multiple_snapshots_and_chats_remain_explicitly_addressed() {
    let mut fixture = Fixture::new();
    let first = fixture.snapshot("req_snapshot_a");
    let second = fixture.snapshot("req_snapshot_b");
    assert_ne!(first["worktreeId"], second["worktreeId"]);
    assert_ne!(first["workspaceIdentity"], second["workspaceIdentity"]);

    let chat_a = fixture.create_chat(&first, "first goal", "req_chat_a");
    let chat_b = fixture.create_chat(&second, "second goal", "req_chat_b");
    let address_a = address_from_chat(&chat_a).unwrap();
    let address_b = address_from_chat(&chat_b).unwrap();

    let read_a = fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
        .unwrap();
    let read_b = fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_b, "src/value.txt")
        .unwrap();
    assert_eq!(read_a.streams[0].chunks, vec!["base"]);
    assert_eq!(read_b.streams[0].chunks, vec!["base"]);

    let aliased = WorkspaceAddress {
        chat_id: address_a.chat_id.clone(),
        project_id: address_b.project_id.clone(),
        worktree_id: address_b.worktree_id.clone(),
        workspace_identity: address_b.workspace_identity.clone(),
    };
    assert_eq!(
        fixture
            .projects
            .workspace_read_file(&fixture.storage, &aliased, "src/value.txt")
            .unwrap_err()
            .kind,
        "manifestMismatch"
    );

    fixture
        .projects
        .workspace_close(&fixture.storage, &address_a)
        .unwrap();
    assert_eq!(
        fixture
            .projects
            .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
            .unwrap_err()
            .kind,
        "recoveryRequired"
    );
    fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_b, "src/value.txt")
        .unwrap();
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
}

#[test]
fn save_continuations_reject_cross_chat_and_preserve_other_workspace() {
    let mut fixture = Fixture::new();
    let first = fixture.snapshot("req_save_snapshot_a");
    let second = fixture.snapshot("req_save_snapshot_b");
    let chat_a = fixture.create_chat(&first, "save a", "req_save_chat_a");
    let chat_b = fixture.create_chat(&second, "save b", "req_save_chat_b");
    let address_a = address_from_chat(&chat_a).unwrap();
    let address_b = address_from_chat(&chat_b).unwrap();
    let expected = sha256_file_version_v1(b"base");
    let begin = fixture
        .projects
        .workspace_begin_save(
            &mut fixture.storage,
            &address_a,
            "src/value.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    assert_eq!(
        fixture
            .projects
            .workspace_append_save_chunk(&fixture.storage, &address_b, save_id, 1, "other")
            .unwrap_err()
            .kind,
        "manifestMismatch"
    );
    fixture
        .projects
        .workspace_append_save_chunk(&fixture.storage, &address_a, save_id, 1, "after")
        .unwrap();
    fixture
        .projects
        .workspace_commit_save(
            &mut fixture.storage,
            &address_a,
            save_id,
            &expected,
            1,
            5,
            "req_save_commit",
        )
        .unwrap();
    let b = fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_b, "src/value.txt")
        .unwrap();
    assert_eq!(b.streams[0].chunks, vec!["base"]);
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
}

#[test]
fn settings_are_snapshotted_and_global_workspace_aliases_are_rejected() {
    let mut fixture = Fixture::new();
    let first = fixture.snapshot("req_settings_snapshot_a");
    let chat = fixture.create_chat(&first, "stable settings", "req_settings_chat_a");
    let roles = json!([
        {"roleId":"planning","runtime":"codex","modelSelector":"different","reasoningEffort":"medium","permission":"readOnly"},
        {"roleId":"coding","runtime":"claude-code","modelSelector":"opus","reasoningEffort":null,"permission":"workspaceWrite"},
        {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"high","permission":"readOnly"}
    ]);
    fixture
        .chats
        .update_role_settings(
            &mut fixture.storage,
            &fixture.project_id,
            1,
            roles,
            "req_settings_update",
        )
        .unwrap();
    let stored = fixture
        .storage
        .projection_record("chat", chat["chatId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(stored["record"]["settingsRevision"], 1);
    assert_eq!(stored["record"]["roleSnapshots"][0]["modelSelector"], "sol");
    let error = fixture
        .chats
        .create_chat(
            &mut fixture.storage,
            &fixture.projects,
            first["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            first["worktreeId"].as_str().unwrap(),
            first["workspaceIdentity"].as_str().unwrap(),
            "alias",
            2,
            "req_alias",
        )
        .unwrap_err();
    assert_eq!(error.kind, "manifestMismatch");
}

#[test]
fn chat_ledger_pages_keep_durable_sequence_separate_from_wire_streams() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_ledger_snapshot");
    let chat = fixture.create_chat(&snapshot, "persist me", "req_ledger_chat");
    let detail = fixture
        .chats
        .get_chat(
            &fixture.storage,
            chat["chatId"].as_str().unwrap(),
            0,
            None,
            50,
        )
        .unwrap();
    assert_eq!(detail["messages"][0]["eventSequence"], 1);
    assert_eq!(detail["messages"][0]["text"], "persist me");
    validate_g5_payload_v1("chatDetailPage", &detail).unwrap();
}

#[test]
fn unavailable_exact_profile_never_persists_or_spawns_a_run() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_unavailable_snapshot");
    fixture
        .chats
        .default_role_settings(&mut fixture.storage, &fixture.project_id, "req_defaults")
        .unwrap();
    let roles = json!([
        {"roleId":"planning","runtime":"codex","modelSelector":"unavailable","reasoningEffort":"medium","permission":"readOnly"},
        {"roleId":"coding","runtime":"claude-code","modelSelector":"opus","reasoningEffort":null,"permission":"workspaceWrite"},
        {"roleId":"review","runtime":"codex","modelSelector":"sol","reasoningEffort":"high","permission":"readOnly"}
    ]);
    fixture
        .chats
        .update_role_settings(
            &mut fixture.storage,
            &fixture.project_id,
            1,
            roles,
            "req_unavailable_settings",
        )
        .unwrap();
    let chat = fixture
        .chats
        .create_chat(
            &mut fixture.storage,
            &fixture.projects,
            snapshot["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot["worktreeId"].as_str().unwrap(),
            snapshot["workspaceIdentity"].as_str().unwrap(),
            "no fallback",
            2,
            "req_unavailable_chat",
        )
        .unwrap()
        .result;
    let before = fixture.storage.projection_records("run").unwrap().len();
    let error = fixture
        .chats
        .start_fixture(
            &mut fixture.storage,
            &mut fixture.projects,
            chat["chatId"].as_str().unwrap(),
            "planning",
            "planning-success",
            "req_unavailable_run",
        )
        .unwrap_err();
    assert_eq!(error.kind, "manifestMismatch");
    assert_eq!(
        fixture.storage.projection_records("run").unwrap().len(),
        before
    );
}

#[test]
fn failed_b_creation_and_exact_abandon_leave_bound_a_fully_active() {
    let mut fixture = Fixture::new();
    let snapshot_a = fixture.snapshot("req_cleanup_snapshot_a");
    let chat_a = fixture.create_chat(&snapshot_a, "bound a", "req_cleanup_chat_a");
    let address_a = address_from_chat(&chat_a).unwrap();
    let snapshot_b = fixture.snapshot("req_cleanup_snapshot_b");
    let failed = fixture
        .chats
        .create_chat(
            &mut fixture.storage,
            &fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "must not bind",
            99,
            "req_cleanup_chat_b_failed",
        )
        .unwrap_err();
    assert_eq!(failed.kind, "dirtyConflict");
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot_b["preparationId"].as_str().unwrap()]
            ["bindingState"],
        "unbound"
    );
    assert!(fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            "workspace_wrong_alias",
            "req_cleanup_wrong_abandon_b",
        )
        .is_err());
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot_b["preparationId"].as_str().unwrap()]
            ["bindingState"],
        "unbound"
    );
    let abandoned = fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "req_cleanup_abandon_b",
        )
        .unwrap();
    assert_eq!(abandoned.result["bindingState"], "abandoned");
    assert!(fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "req_cleanup_duplicate_abandon_b",
        )
        .is_err());
    fixture
        .projects
        .workspace_get_tree(&fixture.storage, &address_a, "", None)
        .unwrap();
    let expected = sha256_file_version_v1(b"base");
    let begin = fixture
        .projects
        .workspace_begin_save(
            &mut fixture.storage,
            &address_a,
            "src/value.txt",
            &expected,
            4,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    fixture
        .projects
        .workspace_append_save_chunk(&fixture.storage, &address_a, save_id, 1, "kept")
        .unwrap();
    fixture
        .projects
        .workspace_commit_save(
            &mut fixture.storage,
            &address_a,
            save_id,
            &expected,
            1,
            4,
            "req_cleanup_save_a",
        )
        .unwrap();
    assert_eq!(
        fixture
            .projects
            .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
            .unwrap()
            .streams[0]
            .chunks,
        vec!["kept"]
    );
    assert_eq!(
        fs::read(fixture.source.join("src/value.txt")).unwrap(),
        b"base"
    );
    assert!(fixture
        .chats
        .abandon_prepared_workspace(
            &mut fixture.storage,
            &mut fixture.projects,
            snapshot_a["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_a["worktreeId"].as_str().unwrap(),
            snapshot_a["workspaceIdentity"].as_str().unwrap(),
            "req_cleanup_late_abandon_a",
        )
        .is_err());
    fixture
        .projects
        .workspace_read_file(&fixture.storage, &address_a, "src/value.txt")
        .unwrap();
}

#[test]
fn duplicate_chat_identity_rolls_back_and_keeps_new_preparation_unbound() {
    let mut fixture = Fixture::new();
    let snapshot_a = fixture.snapshot("req_duplicate_snapshot_a");
    let chat_a = fixture.create_chat(&snapshot_a, "chat a", "req_duplicate_chat_a");
    let snapshot_b = fixture.snapshot("req_duplicate_snapshot_b");
    let before = fixture.storage.projection_records("chat").unwrap().len();
    let error = fixture
        .chats
        .create_chat_inner(
            &mut fixture.storage,
            &fixture.projects,
            snapshot_b["preparationId"].as_str().unwrap(),
            &fixture.project_id,
            snapshot_b["worktreeId"].as_str().unwrap(),
            snapshot_b["workspaceIdentity"].as_str().unwrap(),
            "duplicate id",
            1,
            "req_duplicate_chat_b",
            chat_a["chatId"].as_str(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "manifestMismatch");
    assert_eq!(
        fixture.storage.projection_records("chat").unwrap().len(),
        before
    );
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot_b["preparationId"].as_str().unwrap()]
            ["bindingState"],
        "unbound"
    );
}

#[test]
fn injected_bind_crash_rolls_back_preparation_and_initial_message_together() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_crash_snapshot");
    fixture
        .chats
        .default_role_settings(
            &mut fixture.storage,
            &fixture.project_id,
            "req_crash_settings",
        )
        .unwrap();
    let settings =
        project_record(&fixture.storage, &fixture.project_id).unwrap()["role_settings"].clone();
    let created = now();
    let chat = json!({
        "schemaVersion":1,"chatId":"chat_injected_crash","taskId":"task_injected_crash",
        "projectId":fixture.project_id,"worktreeId":snapshot["worktreeId"],
        "workspaceIdentity":snapshot["workspaceIdentity"],"goal":"rollback together",
        "settingsRevision":1,"roleSnapshots":settings["roles"],"state":"draft",
        "createdAt":created,"updatedAt":created,
    });
    let message = team_message(&chat, 1, "user", None, None, "rollback together").unwrap();
    let payload = chat_projection(&chat, Some(message), 1).unwrap();
    let error = fixture
        .storage
        .bind_prepared_chat_atomic_with_failure(
            ChatBindInput {
                preparation_id: snapshot["preparationId"].as_str().unwrap().to_string(),
                project_id: fixture.project_id.clone(),
                worktree_id: snapshot["worktreeId"].as_str().unwrap().to_string(),
                workspace_identity: snapshot["workspaceIdentity"].as_str().unwrap().to_string(),
                expected_settings_revision: 1,
                chat_id: "chat_injected_crash".to_string(),
                request_id: "req_crash_bind".to_string(),
                chat_payload: payload,
            },
            crate::storage::ChatBindFailurePoint::AfterPreparationBindBeforeChat,
        )
        .unwrap_err();
    assert_eq!(error.kind(), "injectedFailure");
    assert!(fixture
        .storage
        .projection_record("chat", "chat_injected_crash")
        .unwrap()
        .is_none());
    let project = project_record(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        project["prepared_workspaces"][snapshot["preparationId"].as_str().unwrap()]["bindingState"],
        "unbound"
    );
}

fn git(path: &std::path::Path, args: &[&str]) {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sidecar_binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/debug/hello-agent-sidecar")
}

fn initial_message_id(fixture: &Fixture, chat_id: &str) -> String {
    fixture
        .chats
        .get_chat(&fixture.storage, chat_id, 0, None, 10)
        .unwrap()["messages"][0]["messageId"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn actual_agent_preflight_rejects_unknown_or_drifted_capability_with_zero_run_and_lock() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_agent_blocked_snapshot");
    let chat = fixture.create_chat(&snapshot, "blocked agent", "req_agent_blocked_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let before_runs = fixture.storage.projection_records("run").unwrap().len();
    let before_lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    let mut runtime = AgentRuntime::blocked();
    let capability = runtime.get_capabilities("codex", false).unwrap();
    let error = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.to_string(),
                role_id: "planning".into(),
                input_message_ids: vec![message_id.clone()],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_blocked",
        )
        .unwrap_err();
    assert!(error.detail.contains("before run, spawn, or lock"));
    assert_eq!(
        fixture.storage.projection_records("run").unwrap().len(),
        before_runs
    );
    assert_eq!(
        fixture
            .storage
            .projection_record(
                "lock",
                &format!("lock_{}", chat["worktreeId"].as_str().unwrap())
            )
            .unwrap()
            .unwrap(),
        before_lock
    );

    let mut fake = AgentRuntime::fake(sidecar_binary(), FakeScenario::Clean);
    let exact = fake.get_capabilities("codex", true).unwrap();
    assert_eq!(exact["canStart"], cfg!(target_os = "macos"));
    let drifted = format!("sha256:{}", "0".repeat(64));
    assert_ne!(
        exact["fingerprint"].as_str().expect("fake fingerprint"),
        drifted
    );
    let error = fake
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.to_string(),
                role_id: "planning".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: drifted,
            },
            "req_agent_drifted",
        )
        .unwrap_err();
    assert!(error.detail.contains("profileChanged"));
    assert_eq!(
        fixture.storage.projection_records("run").unwrap().len(),
        before_runs
    );
    assert_eq!(
        fixture
            .storage
            .projection_record(
                "lock",
                &format!("lock_{}", chat["worktreeId"].as_str().unwrap())
            )
            .unwrap()
            .unwrap(),
        before_lock
    );
}

#[test]
fn actual_agent_pre_release_failures_are_terminal_and_release_writer_after_reopen() {
    for scenario in [
        FakeScenario::SpawnFailure,
        FakeScenario::IdentityFailure,
        FakeScenario::ReleaseFailure,
    ] {
        let mut fixture = Fixture::new();
        let database = fixture._root.path().join("data/hello-agent.sqlite3");
        let snapshot = fixture.snapshot("req_pre_release_snapshot");
        let chat = fixture.create_chat(&snapshot, "pre-release failure", "req_pre_release_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let mut runtime = AgentRuntime::fake(sidecar_binary(), scenario);
        let capability = runtime.get_capabilities("claude-code", true).unwrap();
        let error = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: "coding".into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_pre_release_failure",
            )
            .unwrap_err();
        assert!(error.detail.contains("failure"));
        let actual = fixture
            .storage
            .projection_records("run")
            .unwrap()
            .into_iter()
            .find(|run| run["source_kind"] == "actualAgent")
            .unwrap();
        assert_eq!(actual["record"]["state"], "failed");
        assert_eq!(actual["pre_release_failure"]["processesProvenGone"], true);
        let lock_id = format!("lock_{}", chat["worktreeId"].as_str().unwrap());
        let lock = fixture
            .storage
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], Value::Null);
        assert_eq!(lock["poisoned"], false);
        drop(runtime);
        let Fixture {
            _root: root_guard,
            storage,
            projects,
            chats,
            ..
        } = fixture;
        drop(storage);
        drop(projects);
        drop(chats);
        let reopened = Storage::open(&database).unwrap();
        let lock = reopened
            .projection_record("lock", &lock_id)
            .unwrap()
            .unwrap();
        assert_eq!(lock["writer_run_id"], Value::Null);
        assert_eq!(lock["recovery_required"], false);
        drop(reopened);
        drop(root_guard);
    }
}

#[test]
fn exact_fake_agent_barrier_filters_private_events_and_keeps_fixture_ledger_distinct() {
    let executable = sidecar_binary();
    assert!(
        executable.is_file(),
        "build the sidecar binary for process gate"
    );
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_agent_clean_snapshot");
    let chat = fixture.create_chat(&snapshot, "clean agent", "req_agent_clean_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(executable, FakeScenario::Clean);
    let capability = runtime.get_capabilities("codex", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "planning".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_clean",
        )
        .unwrap();
    for field in [
        "chatId",
        "taskId",
        "projectId",
        "worktreeId",
        "workspaceIdentity",
    ] {
        assert_eq!(started.result[field], chat[field]);
    }
    assert_eq!(started.result["roleId"], "planning");
    assert_eq!(
        started.result["capabilityFingerprint"],
        capability["fingerprint"]
    );
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    let preparing = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(preparing["source_kind"], "actualAgent");
    assert_eq!(
        preparing["barrier"]["runAndLockPersistedBeforeRelease"],
        true
    );
    assert_eq!(preparing["barrier"]["identityPersistedBeforeRelease"], true);
    assert_eq!(
        preparing["barrier"]["trackedProcesses"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for _ in 0..100 {
        let _ = runtime.poll(&mut fixture.storage, &mut fixture.projects);
        if fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap()["state"]
            != "running"
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let terminal = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    assert_eq!(terminal["record"]["state"], "succeeded");
    assert_eq!(terminal["termination"]["processesProvenGone"], true);
    let serialized = serde_json::to_string(&terminal).unwrap();
    assert!(!serialized.contains("token-never-persist"));
    assert!(!serialized.contains("\"type\":\"reasoning\""));
    assert!(terminal["public_events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| {
            ["status", "final"].contains(&event["kind"].as_str().unwrap_or_default())
        }));
    assert!(terminal["public_events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| {
            event["chatId"] == chat["chatId"]
                && event["taskId"] == chat["taskId"]
                && event["projectId"] == chat["projectId"]
                && event["worktreeId"] == chat["worktreeId"]
                && event["workspaceIdentity"] == chat["workspaceIdentity"]
                && event["runId"] == run_id
                && event["roleId"] == "planning"
                && event["capabilityFingerprint"] == capability["fingerprint"]
        }));
    assert!(fixture
        .chats
        .get_chat(&fixture.storage, chat_id, 0, None, 10)
        .unwrap()["runs"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn fake_agent_cancel_uncertain_poison_survives_and_is_abandon_only() {
    let executable = sidecar_binary();
    assert!(
        executable.is_file(),
        "build the sidecar binary for process gate"
    );
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_agent_uncertain_snapshot");
    let chat = fixture.create_chat(&snapshot, "uncertain agent", "req_agent_uncertain_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(executable, FakeScenario::CancelUncertain);
    let capability = runtime.get_capabilities("claude-code", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "coding".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_agent_uncertain",
        )
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    let identity = RunIdentity {
        chat_id: chat_id.into(),
        task_id: chat["taskId"].as_str().unwrap().into(),
        project_id: chat["projectId"].as_str().unwrap().into(),
        worktree_id: chat["worktreeId"].as_str().unwrap().into(),
        workspace_identity: chat["workspaceIdentity"].as_str().unwrap().into(),
        run_id: run_id.clone(),
    };
    let canceled = runtime
        .cancel(&mut fixture.storage, &mut fixture.projects, &identity)
        .unwrap();
    assert_eq!(canceled.result["outcome"], "recoveryRequired");
    assert_eq!(
        canceled.result["termination"]["writerLockState"],
        "recoveryRequired"
    );
    let recovery = canceled.result["recovery"].clone();
    assert!(fixture
        .chats
        .list_recoveries(&fixture.storage, None, 10)
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(fixture
        .chats
        .inspect_recovery(&fixture.storage, recovery["recoveryId"].as_str().unwrap())
        .unwrap_err()
        .detail
        .contains("missing"));
    assert!(fixture
        .chats
        .mark_recovery_resolved(
            &mut fixture.storage,
            &mut fixture.projects,
            recovery["recoveryId"].as_str().unwrap(),
            1,
            "abandon",
            "req_legacy_cross_ledger",
        )
        .unwrap_err()
        .detail
        .contains("missing"));
    assert_eq!(
        fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap()["recovery"],
        recovery
    );
    let resolved = runtime.resolve_recovery(&mut fixture.storage,&mut fixture.projects,&json!({"recoveryId":recovery["recoveryId"],"chatId":identity.chat_id,"taskId":identity.task_id,"projectId":identity.project_id,"worktreeId":identity.worktree_id,"workspaceIdentity":identity.workspace_identity,"runId":identity.run_id,"expectedRevision":1,"resolution":"abandon"}),"req_agent_resolve").unwrap();
    assert_eq!(resolved["outcome"], "resolvedAbandoned");
    let lock = fixture
        .storage
        .projection_record(
            "lock",
            &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(lock["poisoned"], false);
    assert_eq!(lock["writer_run_id"], Value::Null);
}

#[test]
fn actual_recovery_api_rejects_legacy_fixture_ledger() {
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_legacy_recovery_snapshot");
    let chat = fixture.create_chat(&snapshot, "legacy recovery", "req_legacy_recovery_chat");
    let run_id = "agent_run_legacy_cross_ledger".to_string();
    let recovery = json!({"recoveryId":"agent_recovery_legacy_cross_ledger","chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"revision":1,"state":"unresolved"});
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: "op_legacy_cross_ledger".into(),
            request_id: None,
            payload: json!({"run_id":run_id,"chat_id":chat["chatId"],"state":"recoveryRequired","worktree_path":snapshot["worktreePath"],"record":{"runId":run_id},"recovery":recovery})
                .as_object()
                .unwrap()
                .clone(),
        }])
        .unwrap();
    let listed = fixture
        .chats
        .list_recoveries(&fixture.storage, None, 10)
        .unwrap();
    let recovery = listed["items"][0].clone();
    let request = json!({"recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id});
    let mut actual = AgentRuntime::blocked();
    assert!(actual
        .inspect_recovery(&fixture.storage, &request)
        .unwrap_err()
        .detail
        .contains("missing"));
    let mut resolve = request;
    resolve["expectedRevision"] = Value::from(1);
    resolve["resolution"] = Value::String("abandon".into());
    assert!(actual
        .resolve_recovery(
            &mut fixture.storage,
            &mut fixture.projects,
            &resolve,
            "req_actual_cross_ledger"
        )
        .unwrap_err()
        .detail
        .contains("missing"));
    assert_eq!(
        fixture
            .chats
            .inspect_recovery(&fixture.storage, recovery["recoveryId"].as_str().unwrap())
            .unwrap()["runId"],
        run_id
    );
}

#[test]
fn fake_agent_clean_cancel_releases_writer_and_malformed_or_oversize_fail_closed() {
    let executable = sidecar_binary();
    for scenario in [
        FakeScenario::CancelClean,
        FakeScenario::Malformed,
        FakeScenario::Oversized,
    ] {
        let mut fixture = Fixture::new();
        let snapshot = fixture.snapshot("req_agent_failure_snapshot");
        let chat = fixture.create_chat(&snapshot, "agent failure gate", "req_agent_failure_chat");
        let chat_id = chat["chatId"].as_str().unwrap();
        let message_id = initial_message_id(&fixture, chat_id);
        let (runtime_name, role_id) = if scenario == FakeScenario::CancelClean {
            ("claude-code", "coding")
        } else {
            ("codex", "planning")
        };
        let mut runtime = AgentRuntime::fake(executable.clone(), scenario);
        let capability = runtime.get_capabilities(runtime_name, true).unwrap();
        let started = runtime
            .start_agent(
                &mut fixture.storage,
                &mut fixture.projects,
                AgentRunStartRequestV1 {
                    chat_id: chat_id.into(),
                    role_id: role_id.into(),
                    input_message_ids: vec![message_id],
                    expected_capability_fingerprint: capability["fingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                },
                "req_agent_failure",
            )
            .unwrap();
        let run_id = started.result["runId"].as_str().unwrap().to_string();
        if scenario == FakeScenario::CancelClean {
            let identity = RunIdentity {
                chat_id: chat_id.into(),
                task_id: chat["taskId"].as_str().unwrap().into(),
                project_id: chat["projectId"].as_str().unwrap().into(),
                worktree_id: chat["worktreeId"].as_str().unwrap().into(),
                workspace_identity: chat["workspaceIdentity"].as_str().unwrap().into(),
                run_id: run_id.clone(),
            };
            let result = runtime
                .cancel(&mut fixture.storage, &mut fixture.projects, &identity)
                .unwrap();
            assert_eq!(result.result["outcome"], "canceled");
            let lock = fixture
                .storage
                .projection_record(
                    "lock",
                    &format!("lock_{}", chat["worktreeId"].as_str().unwrap()),
                )
                .unwrap()
                .unwrap();
            assert_eq!(lock["writer_run_id"], Value::Null);
            assert_eq!(lock["poisoned"], false);
        } else {
            for _ in 0..100 {
                let _ = runtime.poll(&mut fixture.storage, &mut fixture.projects);
                if fixture
                    .storage
                    .projection_record("run", &run_id)
                    .unwrap()
                    .unwrap()["state"]
                    != "running"
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let terminal = fixture
                .storage
                .projection_record("run", &run_id)
                .unwrap()
                .unwrap();
            assert_eq!(terminal["record"]["state"], "failed");
            assert_eq!(terminal["termination"]["processesProvenGone"], true);
        }
    }
}

#[test]
fn control_launcher_child_has_no_descendant_before_release_and_tree_is_gone_afterward() {
    use hello_agent_core::proctree::{identity_of, ProbeResult, ProcTable, SystemProcTable};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    let mut launcher = Command::new(sidecar_binary())
        .args(["--agent-fixture-launcher", "clean"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let launcher_identity = identity_of(launcher.id()).unwrap();
    let mut reader = BufReader::new(launcher.stdout.take().unwrap());
    let mut ready = String::new();
    reader.read_line(&mut ready).unwrap();
    let child_pid: u32 = ready
        .trim()
        .strip_prefix("ready:")
        .unwrap()
        .parse()
        .unwrap();
    let child_identity = identity_of(child_pid).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        !SystemProcTable
            .list()
            .unwrap()
            .iter()
            .any(|entry| entry.ppid == child_pid),
        "waiting child must not create descendants before release"
    );
    launcher.stdin.take().unwrap().write_all(b"run\n").unwrap();
    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();
    assert!(launcher.wait().unwrap().success());
    assert!(output.contains("\"type\":\"final\""));
    assert!(matches!(
        SystemProcTable.probe(child_identity),
        ProbeResult::Gone
    ));
    assert!(matches!(
        SystemProcTable.probe(launcher_identity),
        ProbeResult::Gone
    ));
}

#[test]
fn dynamic_grandchild_is_durable_and_blocks_recovery_until_every_identity_is_gone() {
    use hello_agent_core::proctree::{
        ProbeResult, ProcIdentity, ProcTable, SystemProcTable, TreeSignal,
    };
    let mut fixture = Fixture::new();
    let snapshot = fixture.snapshot("req_descendant_snapshot");
    let chat = fixture.create_chat(&snapshot, "durable descendant", "req_descendant_chat");
    let chat_id = chat["chatId"].as_str().unwrap();
    let message_id = initial_message_id(&fixture, chat_id);
    let mut runtime = AgentRuntime::fake(sidecar_binary(), FakeScenario::Descendant);
    let capability = runtime.get_capabilities("codex", true).unwrap();
    let started = runtime
        .start_agent(
            &mut fixture.storage,
            &mut fixture.projects,
            AgentRunStartRequestV1 {
                chat_id: chat_id.into(),
                role_id: "planning".into(),
                input_message_ids: vec![message_id],
                expected_capability_fingerprint: capability["fingerprint"].as_str().unwrap().into(),
            },
            "req_descendant_start",
        )
        .unwrap();
    let run_id = started.result["runId"].as_str().unwrap().to_string();
    let mut projection = Map::new();
    for _ in 0..100 {
        let _ = runtime.poll(&mut fixture.storage, &mut fixture.projects);
        projection = fixture
            .storage
            .projection_record("run", &run_id)
            .unwrap()
            .unwrap();
        if projection["process_identity_internal"]
            .as_array()
            .is_some_and(|items| items.len() >= 3)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let internal = projection["process_identity_internal"]
        .as_array()
        .cloned()
        .unwrap();
    assert!(internal.len() >= 3, "grandchild identity must be durable");
    projection["state"] = Value::String("recoveryRequired".into());
    fixture
        .storage
        .upsert_projection_records_atomic(vec![ProjectionUpsert {
            aggregate_type: "run",
            aggregate_id: run_id.clone(),
            operation_id: "op_test_descendant_crash".into(),
            request_id: None,
            payload: projection,
        }])
        .unwrap();
    drop(runtime);
    let recovered = AgentRuntime::blocked();
    recovered
        .recover_unclean_runs(&mut fixture.storage, &mut fixture.projects)
        .unwrap();
    let run = fixture
        .storage
        .projection_record("run", &run_id)
        .unwrap()
        .unwrap();
    let recovery = run["recovery"].clone();
    assert!(recovery["trackedProcesses"].as_array().unwrap().len() >= 3);
    let request = json!({"recoveryId":recovery["recoveryId"],"chatId":chat["chatId"],"taskId":chat["taskId"],"projectId":chat["projectId"],"worktreeId":chat["worktreeId"],"workspaceIdentity":chat["workspaceIdentity"],"runId":run_id,"expectedRevision":1,"resolution":"abandon"});
    let mut recovered = recovered;
    assert_eq!(
        recovered
            .resolve_recovery(
                &mut fixture.storage,
                &mut fixture.projects,
                &request,
                "req_descendant_alive"
            )
            .unwrap()["outcome"],
        "processesRemain"
    );
    for value in &internal {
        let _ = SystemProcTable.signal(value["pid"].as_u64().unwrap() as u32, TreeSignal::Kill);
    }
    for _ in 0..100 {
        if internal.iter().all(|value| {
            matches!(
                SystemProcTable.probe(ProcIdentity {
                    pid: value["pid"].as_u64().unwrap() as u32,
                    start_us: value["start_us"].as_i64().unwrap(),
                }),
                ProbeResult::Gone
            )
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        recovered
            .resolve_recovery(
                &mut fixture.storage,
                &mut fixture.projects,
                &request,
                "req_descendant_gone"
            )
            .unwrap()["outcome"],
        "resolvedAbandoned"
    );
}
