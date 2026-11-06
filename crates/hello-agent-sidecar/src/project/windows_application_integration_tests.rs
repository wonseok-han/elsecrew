//! Opt-in real Git/files/SQLite transaction, using only a generated project.
//! Test-only Git admission does not activate public source-write requests.
use super::*;
use application_review::{confirmation::Confirmations, intent, persistence, ReviewTarget};

fn bootstrap_git(manager: &ProjectManager, directory: &Dir, args: &[&str]) {
    let image = manager.admitted_git_runtime().unwrap();
    let cwd =
        windows_directory_lease::DirectoryLease::acquire(directory, OsStr::new("guard")).unwrap();
    let args: Vec<String> = args.iter().map(|value| (*value).to_owned()).collect();
    let child =
        windows_git_process::StreamingGitChild::spawn_internal_git(image, &args, cwd, false, None)
            .unwrap();
    ProjectManager::finish_git_control(child, &[0]).unwrap();
}

fn drain(manager: &mut ProjectManager, storage: &mut Storage) {
    for _ in 0..128 {
        if !manager.application_running() {
            break;
        }
        manager.poll_application(storage);
    }
    assert!(
        !manager.application_running(),
        "application exceeded bounded fixture steps"
    );
    assert!(
        manager.application_failure.is_none(),
        "application failure: {:?}",
        manager.application_failure
    );
}

#[test]
#[ignore = "requires explicitly selected installed Windows Git; generated project only"]
fn confirmed_multi_item_apply_and_restore_through_scheduler_and_ledger() {
    let selected = std::env::var("HELLO_AGENT_WINDOWS_TEST_GIT")
        .expect("set HELLO_AGENT_WINDOWS_TEST_GIT to the selected git.exe");
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let source = root.path().join("source");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&source).unwrap();
    let db = data.join("diagnostic.sqlite3");
    let mut manager = ProjectManager::new(&db).unwrap();
    assert!(!manager.windows_git_diagnostic);
    write_json_atomic(
        &manager.data_dir,
        git_selection::FILE,
        &json!({"schemaVersion":1,"path":selected}),
    )
    .unwrap();
    let preview = manager.preview_git_trust().unwrap();
    manager
        .confirm_git_trust(
            serde_json::from_value(preview["identity"].clone()).unwrap(),
            true,
        )
        .unwrap();
    fs::write(source.join("guard"), b"unchanged").unwrap();
    fs::write(source.join("edit.txt"), b"before-edit").unwrap();
    fs::write(source.join("delete.txt"), b"before-delete").unwrap();
    let directory = Dir::open_ambient_dir(&source, ambient_authority()).unwrap();
    // This generated repository needs no installed template files or hooks.
    // Keep fixture initialization independent of Git's relocated template path.
    bootstrap_git(&manager, &directory, &["init", "--quiet", "--template="]);
    bootstrap_git(&manager, &directory, &["add", "."]);
    bootstrap_git(
        &manager,
        &directory,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    manager.windows_git_diagnostic = true;
    // The field exists only in the Rust test binary, defaults false, and does
    // not bypass installation consent, dependency inspection or source gates.
    assert!(!WINDOWS_GIT_EXECUTION_ENABLED);
    assert_eq!(manager.application_capabilities().result["enabled"], false);
    let mut storage = Storage::open(&db).unwrap();
    let opened = manager
        .open_project(&mut storage, source.to_str().unwrap(), "req_native_open")
        .unwrap();
    let project = opened.result["projectId"].as_str().unwrap().to_owned();
    manager
        .create_worktree_inner(
            &mut storage,
            &project,
            "req_native_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    let active = manager.validate_active_project(&storage, &project).unwrap();
    let saved = required_project(&storage, &project).unwrap();
    let mut wrong = saved.clone();
    wrong.insert(
        "manifest_path".into(),
        json!(canonical_protocol_path_without_existing(
            &manager.manifests_root.join("worktree_other.json")
        )
        .unwrap()),
    );
    append_object(
        &mut storage,
        "project",
        &project,
        "op_native_wrong_manifest",
        "req_native_wrong_manifest",
        Value::Object(wrong),
    )
    .unwrap();
    assert!(manager
        .validate_active_project(&storage, &project)
        .err()
        .unwrap()
        .detail
        .contains("exact managed snapshot manifest child"));
    append_object(
        &mut storage,
        "project",
        &project,
        "op_native_restore_manifest",
        "req_native_restore_manifest",
        Value::Object(saved),
    )
    .unwrap();
    manager.validate_active_project(&storage, &project).unwrap();
    fs::write(active.worktree_path.join("edit.txt"), b"after-edit").unwrap();
    fs::remove_file(active.worktree_path.join("delete.txt")).unwrap();
    fs::write(active.worktree_path.join("added.txt"), b"after-add").unwrap();
    fs::create_dir_all(active.worktree_path.join("new/nested")).unwrap();
    fs::write(
        active.worktree_path.join("new/nested/file.txt"),
        b"nested-new",
    )
    .unwrap();
    let review =
        persistence::publish(&manager, &mut storage, &project, "req_native_review").unwrap();
    let mut confirmations = Confirmations::default();
    let (token, _) = confirmations
        .open(
            &manager,
            &storage,
            ReviewTarget::Project(&project),
            review["reviewId"].as_str().unwrap(),
        )
        .unwrap();
    manager.application_confirmations = confirmations;
    manager.application_cancel.begin(&project, &token);
    manager
        .start_confirmed_application(&mut storage, &token, "req_native_apply")
        .unwrap();
    drain(&mut manager, &mut storage);
    let applied = intent::inspect(&storage, &project).unwrap();
    assert_eq!(applied["state"], "applied");
    assert_eq!(fs::read(source.join("edit.txt")).unwrap(), b"after-edit");
    assert_eq!(fs::read(source.join("added.txt")).unwrap(), b"after-add");
    assert!(!source.join("delete.txt").exists());
    assert_eq!(
        fs::read(source.join("new/nested/file.txt")).unwrap(),
        b"nested-new"
    );
    assert!(storage.blocker_ids().unwrap().is_empty());
    // A changed displayed record must not create a restoration owner.
    let mut stale = applied.clone();
    stale["applicationId"] = json!("application_stale");
    assert!(manager
        .start_selected_restoration(&storage, &project, &stale, "req_native_stale")
        .is_err());
    assert_eq!(intent::inspect(&storage, &project).unwrap(), applied);
    manager
        .start_selected_restoration(&storage, &project, &applied, "req_native_restore")
        .unwrap();
    drain(&mut manager, &mut storage);
    let restored = intent::inspect(&storage, &project).unwrap();
    assert_eq!(restored["state"], "restored");
    assert_eq!(fs::read(source.join("edit.txt")).unwrap(), b"before-edit");
    assert_eq!(
        fs::read(source.join("delete.txt")).unwrap(),
        b"before-delete"
    );
    assert_eq!(fs::read(source.join("guard")).unwrap(), b"unchanged");
    assert!(!source.join("added.txt").exists());
    assert!(!source.join("new").exists());
    assert!(storage.blocker_ids().unwrap().is_empty());
    // Completion must survive reopening the real SQLite ledger.
    drop(storage);
    let mut reopened = Storage::open(&db).unwrap();
    assert_eq!(intent::inspect(&reopened, &project).unwrap(), restored);
    assert_eq!(manager.application_capabilities().result["enabled"], false);
    // After a successful real rename/apply/restore, interrupt a fresh snapshot
    // at the rename boundary. It must remain fenced instead of becoming active.
    let failure = manager
        .create_worktree_inner(
            &mut reopened,
            &project,
            "req_native_rename_interrupted",
            Some(WorktreeFailurePoint::AfterWorkspaceRenameBeforeActivation),
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap_err();
    assert_eq!(failure.code, "E_RECOVERY_REQUIRED");
    assert!(failure
        .detail
        .contains("failure injected after snapshot rename"));
    assert_eq!(
        required_project(&reopened, &project).unwrap()["lifecycle"],
        "recoveryRequired"
    );
    assert!(!reopened.blocker_ids().unwrap().is_empty());
    assert!(manager
        .validate_active_project(&reopened, &project)
        .is_err());
}
