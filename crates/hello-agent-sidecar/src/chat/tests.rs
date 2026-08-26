use super::*;
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
