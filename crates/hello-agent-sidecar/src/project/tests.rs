use super::*;
use hello_agent_protocol::project_v1::validate_project_payload_v1;
use tempfile::TempDir;

#[cfg(unix)]
fn source_index_fixture() -> Fixture {
    let fixture = Fixture::open();
    git_raw(
        &fixture.source,
        ["update-index", "--force-remove", "vendor/sub"],
    );
    git_raw(
        &fixture.source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "remove gitlink for index baseline",
        ],
    );
    fixture
}

#[cfg(unix)]
#[test]
fn source_index_baseline_rejects_staged_and_head_changes() {
    use application::git_baseline::capture_index_baseline;
    let fixture = source_index_fixture();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    capture_index_baseline(&fixture.manager, &source).unwrap();
    let original_index = fs::read(fixture.source.join(".git/index")).unwrap();
    // Index-only modification: working-tree bytes stay unchanged.
    let oid = git_text(&fixture.source, ["rev-parse", "HEAD:src/delete.txt"]);
    let entry = format!("100644,{oid},src/edit.txt");
    git_raw(&fixture.source, ["update-index", "--cacheinfo", &entry]);
    assert!(capture_index_baseline(&fixture.manager, &source).is_err());
    assert_eq!(
        fs::read(fixture.source.join("src/edit.txt")).unwrap(),
        b"hello"
    );
    fs::write(fixture.source.join(".git/index"), original_index).unwrap();
    capture_index_baseline(&fixture.manager, &source).unwrap();
    git_raw(
        &fixture.source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "new head same tree",
        ],
    );
    assert!(capture_index_baseline(&fixture.manager, &source).is_err());
}

#[cfg(unix)]
#[test]
fn source_index_baseline_rejects_split_index_and_gitlinks() {
    use application::git_baseline::capture_index_baseline;
    let with_gitlink = Fixture::open();
    let source = with_gitlink
        .manager
        .open_source_repository(&with_gitlink.source)
        .unwrap();
    assert!(capture_index_baseline(&with_gitlink.manager, &source).is_err());
    let fixture = source_index_fixture();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    git_raw(&fixture.source, ["update-index", "--split-index"]);
    assert!(capture_index_baseline(&fixture.manager, &source).is_err());
    git_raw(&fixture.source, ["update-index", "--no-split-index"]);
    git_raw(
        &fixture.source,
        ["sparse-checkout", "set", "--cone", "--sparse-index", "src"],
    );
    assert!(capture_index_baseline(&fixture.manager, &source).is_err());
    git_raw(
        &fixture.source,
        ["sparse-checkout", "reapply", "--no-sparse-index"],
    );
    assert!(capture_index_baseline(&fixture.manager, &source).is_err());
    git_raw(&fixture.source, ["sparse-checkout", "disable"]);
    git_raw(
        &fixture.source,
        ["update-index", "--assume-unchanged", "src/edit.txt"],
    );
    assert!(capture_index_baseline(&fixture.manager, &source).is_err());
}

#[cfg(unix)]
#[test]
fn source_index_baseline_rejects_intent_to_add_for_committed_empty_file() {
    use application::git_baseline::capture_index_baseline;
    let fixture = source_index_fixture();
    fs::write(fixture.source.join("empty.txt"), b"").unwrap();
    git_raw(&fixture.source, ["add", "empty.txt"]);
    git_raw(
        &fixture.source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "empty file",
        ],
    );
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    git_raw(&fixture.source, ["rm", "--cached", "empty.txt"]);
    git_raw(&fixture.source, ["add", "--intent-to-add", "empty.txt"]);
    assert_eq!(
        capture_index_baseline(&fixture.manager, &source)
            .unwrap_err()
            .kind,
        "dirtyConflict"
    );
}

#[cfg(unix)]
#[test]
fn source_index_baseline_does_not_run_user_hooks_filters_or_rewrite_index() {
    use application::git_baseline::capture_index_baseline;
    use std::os::unix::fs::PermissionsExt;
    let fixture = source_index_fixture();
    let hook = fixture.source.join("probe.sh");
    fs::write(&hook, b"#!/bin/sh\n/usr/bin/touch \"$0.called\"\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    for key in [
        "core.fsmonitor",
        "filter.probe.clean",
        "filter.probe.smudge",
        "diff.external",
    ] {
        git_raw(&fixture.source, ["config", key, hook.to_str().unwrap()]);
    }
    fs::write(
        fixture.source.join(".gitattributes"),
        b"* filter=probe diff=probe\n",
    )
    .unwrap();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    let before = fs::read(fixture.source.join(".git/index")).unwrap();
    let raw_before = application::capture(&source._root).unwrap();
    capture_index_baseline(&fixture.manager, &source).unwrap();
    assert_eq!(fs::read(fixture.source.join(".git/index")).unwrap(), before);
    assert_eq!(application::capture(&source._root).unwrap(), raw_before);
    assert!(!fixture.source.join("probe.sh.called").exists());
}

#[cfg(unix)]
#[test]
fn source_raw_baseline_compares_bytes_modes_links_and_untracked_files() {
    use application::git_baseline::raw_baseline::capture_clean_baseline;
    use std::os::unix::fs::{symlink, PermissionsExt};
    let fixture = source_index_fixture();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    fs::create_dir(fixture.source.join("extras")).unwrap();
    fs::write(fixture.source.join("extras/notes"), b"first").unwrap();
    fs::write(fixture.source.join(".gitignore"), b"extras/\n").unwrap();
    let first = capture_clean_baseline(&fixture.manager, &source).unwrap();
    fs::write(fixture.source.join("extras/notes"), b"other").unwrap();
    assert_ne!(
        first,
        capture_clean_baseline(&fixture.manager, &source).unwrap()
    );
    fs::write(fixture.source.join("src/edit.txt"), b"HELLO").unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
    fs::write(fixture.source.join("src/edit.txt"), b"hello").unwrap();
    fs::set_permissions(
        fixture.source.join("src/edit.txt"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
    fs::set_permissions(
        fixture.source.join("src/edit.txt"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    fs::remove_file(fixture.source.join("src/edit-link")).unwrap();
    symlink("delete.txt", fixture.source.join("src/edit-link")).unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
    fs::remove_file(fixture.source.join("src/edit-link")).unwrap();
    symlink("edit.txt", fixture.source.join("src/edit-link")).unwrap();
    fs::write(fixture.source.join("assets/data.bin"), [0, 158, 146, 150]).unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
    fs::write(fixture.source.join("assets/data.bin"), [0, 159, 146, 150]).unwrap();
    fs::remove_file(fixture.source.join("src/delete.txt")).unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
}

#[cfg(unix)]
#[test]
fn source_raw_baseline_ignores_filters_and_handles_packed_objects() {
    use application::git_baseline::raw_baseline::capture_clean_baseline;
    use std::os::unix::fs::PermissionsExt;
    let fixture = source_index_fixture();
    git_raw(&fixture.source, ["gc", "--prune=now"]);
    let hook = fixture.source.join("probe.sh");
    fs::write(
        &hook,
        b"#!/bin/sh\n/usr/bin/touch \"$0.called\"\nprintf hello\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    for key in [
        "core.fsmonitor",
        "filter.probe.clean",
        "filter.probe.smudge",
        "diff.probe.textconv",
        "diff.external",
    ] {
        git_raw(&fixture.source, ["config", key, hook.to_str().unwrap()]);
    }
    fs::write(
        fixture.source.join(".gitattributes"),
        b"* filter=probe diff=probe\n",
    )
    .unwrap();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    let index = fs::read(fixture.source.join(".git/index")).unwrap();
    capture_clean_baseline(&fixture.manager, &source).unwrap();
    fs::write(fixture.source.join("src/edit.txt"), b"dirty").unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
    assert!(!fixture.source.join("probe.sh.called").exists());
    assert_eq!(fs::read(fixture.source.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read(fixture.source.join("src/edit.txt")).unwrap(),
        b"dirty"
    );
}

#[cfg(unix)]
#[test]
fn source_raw_baseline_rejects_missing_objects_and_replaced_source_root() {
    use application::git_baseline::raw_baseline::capture_clean_baseline;
    let fixture = source_index_fixture();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    let oid = git_text(&fixture.source, ["rev-parse", "HEAD:src/edit.txt"]);
    let object = fixture
        .source
        .join(".git/objects")
        .join(&oid[..2])
        .join(&oid[2..]);
    let saved = fixture._root.path().join("saved-object");
    fs::rename(&object, &saved).unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
    fs::rename(&saved, &object).unwrap();
    capture_clean_baseline(&fixture.manager, &source).unwrap();
    fs::rename(&fixture.source, fixture._root.path().join("old-source")).unwrap();
    fs::create_dir(&fixture.source).unwrap();
    fs::write(fixture.source.join("keep"), b"replacement").unwrap();
    assert!(capture_clean_baseline(&fixture.manager, &source).is_err());
    assert_eq!(
        fs::read(fixture.source.join("keep")).unwrap(),
        b"replacement"
    );
}

#[cfg(unix)]
#[test]
fn source_receipt_ledger_binding_requires_fence_and_survives_activation() {
    source_receipt_binding_case(false);
}

#[cfg(unix)]
#[test]
fn source_receipt_failed_publication_preserves_fence_and_unbound_file() {
    source_receipt_binding_case(true);
}

#[cfg(unix)]
fn source_receipt_binding_case(fail_publication: bool) {
    use application::git_baseline::raw_baseline::{capture_clean_baseline, receipt};
    let mut fixture = source_index_fixture();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_receipt_bind_open",
        )
        .unwrap();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    let baseline = capture_clean_baseline(&fixture.manager, &source).unwrap();
    let active = fixture.create();
    assert!(receipt::verify_bound(&fixture.manager, &fixture.storage, &active, &baseline).is_err());
    assert!(receipt::persist_and_bind(
        &fixture.manager,
        &mut fixture.storage,
        &active,
        &baseline,
        "req_unfenced"
    )
    .is_err());
    let mut previous = required_project(&fixture.storage, &active.project_id).unwrap();
    previous.insert("lifecycle".into(), json!("recoveryRequired"));
    fixture
        .storage
        .upsert_projection_record(
            "project",
            &active.project_id,
            "op_receipt_test_prepare",
            None,
            previous,
        )
        .unwrap();
    assert!(receipt::persist_and_bind(
        &fixture.manager,
        &mut fixture.storage,
        &active,
        &baseline,
        "req_lock_missing"
    )
    .is_err());
    let mut lock = fixture
        .storage
        .projection_record("lock", &active.lock_id)
        .unwrap()
        .unwrap();
    lock.insert("recovery_required".into(), json!(true));
    fixture
        .storage
        .upsert_projection_record("lock", &active.lock_id, "op_receipt_test_fence", None, lock)
        .unwrap();
    let database = fixture.manager.data_root.join("hello-agent.sqlite3");
    if fail_publication {
        fixture.storage.connection_for_test().execute_batch("CREATE TRIGGER test_receipt_failure BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT, 'receipt publication test failure'); END;").unwrap();
    }
    let result = receipt::persist_and_bind(
        &fixture.manager,
        &mut fixture.storage,
        &active,
        &baseline,
        "req_receipt_bind",
    );
    if fail_publication {
        assert!(result.is_err());
        fixture
            .storage
            .connection_for_test()
            .execute_batch("DROP TRIGGER test_receipt_failure;")
            .unwrap();
        assert!(required_project(&fixture.storage, &active.project_id)
            .unwrap()
            .get("source_baselines")
            .is_none());
        assert_eq!(
            fixture
                .storage
                .projection_record("lock", &active.lock_id)
                .unwrap()
                .unwrap()["recovery_required"],
            true
        );
        assert!(
            receipt::verify_bound(&fixture.manager, &fixture.storage, &active, &baseline).is_err()
        );
        assert!(fixture
            .manager
            .data_root
            .join("source-baselines-v1")
            .join(format!("{}.source-baseline-v1.json", active.worktree_id))
            .is_file());
        return;
    }
    result.unwrap();
    receipt::verify_bound(&fixture.manager, &fixture.storage, &active, &baseline).unwrap();
    assert!(receipt::persist_and_bind(
        &fixture.manager,
        &mut fixture.storage,
        &active,
        &baseline,
        "req_receipt_duplicate"
    )
    .is_err());
    let bound = required_project(&fixture.storage, &active.project_id).unwrap();
    let mut activated = bound.clone();
    activated.remove("source_baselines");
    activated.insert("lifecycle".into(), json!("active"));
    let activated = preserve_project_extensions(Value::Object(activated), &bound).unwrap();
    fixture
        .storage
        .upsert_projection_record(
            "project",
            &active.project_id,
            "op_receipt_test_active",
            None,
            activated.as_object().unwrap().clone(),
        )
        .unwrap();
    drop(fixture.storage);
    let mut reopened = Storage::open(&database).unwrap();
    receipt::verify_bound(&fixture.manager, &reopened, &active, &baseline).unwrap();
    let mut lock = reopened
        .projection_record("lock", &active.lock_id)
        .unwrap()
        .unwrap();
    lock.insert("recovery_required".into(), json!(false));
    reopened
        .upsert_projection_record(
            "lock",
            &active.lock_id,
            "op_receipt_test_unlock",
            None,
            lock,
        )
        .unwrap();
    fixture
        .manager
        .close_project(&mut reopened, &active.project_id, "req_receipt_close")
        .unwrap();
    fixture
        .manager
        .open_project(
            &mut reopened,
            fixture.source.to_str().unwrap(),
            "req_receipt_reopen",
        )
        .unwrap();
    receipt::verify_bound(&fixture.manager, &reopened, &active, &baseline).unwrap();
    let bytes = fs::read(
        fixture
            .manager
            .data_root
            .join("source-baselines-v1")
            .join(format!("{}.source-baseline-v1.json", active.worktree_id)),
    )
    .unwrap();
    assert!(!bytes.is_empty());
}

#[cfg(unix)]
#[test]
fn source_receipt_survives_reopen_and_rejects_legacy_drift_and_tampering() {
    use application::git_baseline::raw_baseline::{
        capture_clean_baseline,
        receipt::{self, BaselineReceipt},
    };
    let mut fixture = source_index_fixture();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_refresh_receipt",
        )
        .unwrap();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    let baseline = capture_clean_baseline(&fixture.manager, &source).unwrap();
    let mut active = fixture.create();
    let receipt_path = fixture.manager.data_root.join("source-baselines-v1");
    let receipt_dir = receipt::ReceiptStore::open(&fixture.manager).unwrap();
    let recorded = receipt::persist(&receipt_dir, &active, &baseline).unwrap();
    let restored = BaselineReceipt::from_value(
        &serde_json::from_slice(&serde_json::to_vec(&recorded.to_value()).unwrap()).unwrap(),
    )
    .unwrap();
    let reopened = receipt::ReceiptStore::open(&fixture.manager).unwrap();
    assert!(!fixture
        .source
        .join(format!("{}.source-baseline-v1.json", active.worktree_id))
        .exists());
    let current = capture_clean_baseline(&fixture.manager, &source).unwrap();
    receipt::verify(&reopened, &active, &current, Some(&restored)).unwrap();
    assert!(receipt::verify(&reopened, &active, &current, None).is_err());
    assert!(receipt::persist(&reopened, &active, &current).is_err());
    let original_identity = active.workspace_identity.clone();
    active.workspace_identity = "workspace_other".to_owned();
    assert!(receipt::verify(&reopened, &active, &current, Some(&restored)).is_err());
    active.workspace_identity = original_identity;
    fs::write(fixture.source.join("untracked.txt"), b"new local work").unwrap();
    let changed = capture_clean_baseline(&fixture.manager, &source).unwrap();
    assert!(receipt::verify(&reopened, &active, &changed, Some(&restored)).is_err());
    let path = receipt_path.join(format!("{}.source-baseline-v1.json", active.worktree_id));
    let before = fs::read(&path).unwrap();
    let mut corrupt = before.clone();
    corrupt[0] = b'!';
    fs::write(&path, &corrupt).unwrap();
    assert!(receipt::verify(&reopened, &active, &current, Some(&restored)).is_err());
    assert!(receipt::persist(&reopened, &active, &current).is_err());
    assert_eq!(fs::read(&path).unwrap(), corrupt);
    fs::write(&path, &before[..before.len() / 2]).unwrap();
    assert!(receipt::verify(&reopened, &active, &current, Some(&restored)).is_err());
}

#[cfg(unix)]
#[test]
fn source_receipt_rejects_links_and_invalid_descriptors() {
    use application::git_baseline::raw_baseline::{
        capture_clean_baseline,
        receipt::{self, BaselineReceipt},
    };
    use std::os::unix::fs::symlink;
    let mut fixture = source_index_fixture();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_refresh_receipt_links",
        )
        .unwrap();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    let baseline = capture_clean_baseline(&fixture.manager, &source).unwrap();
    let active = fixture.create();
    let receipt_path = fixture.manager.data_root.join("source-baselines-v1");
    let receipt_dir = receipt::ReceiptStore::open(&fixture.manager).unwrap();
    let recorded = receipt::persist(&receipt_dir, &active, &baseline).unwrap();
    for (key, value) in [
        ("length", json!(0)),
        ("length", json!(64 * 1024 * 1024 + 1)),
        ("digest", json!("sha256:bad")),
        ("schemaVersion", json!(2)),
        ("extra", json!(true)),
    ] {
        let mut invalid = recorded.to_value();
        invalid[key] = value;
        assert!(BaselineReceipt::from_value(&invalid).is_err());
    }
    let path = receipt_path.join(format!("{}.source-baseline-v1.json", active.worktree_id));
    let alias = receipt_path.join("receipt-alias");
    fs::hard_link(&path, &alias).unwrap();
    assert!(receipt::verify(&receipt_dir, &active, &baseline, Some(&recorded)).is_err());
    fs::remove_file(&alias).unwrap();
    fs::remove_file(&path).unwrap();
    symlink(fixture.source.join("src/edit.txt"), &path).unwrap();
    assert!(receipt::verify(&receipt_dir, &active, &baseline, Some(&recorded)).is_err());
    assert!(receipt::persist(&receipt_dir, &active, &baseline).is_err());
    assert_eq!(
        fs::read(fixture.source.join("src/edit.txt")).unwrap(),
        b"hello"
    );
}

#[test]
fn releasing_project_view_removes_watchers_without_abandoning_snapshot() {
    let mut fixture = Fixture::open();
    fixture.create();
    fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert_eq!(fixture.manager.watchers.len(), 1);
    for (chat_id, project_id) in [
        ("chat_a", fixture.project_id.clone()),
        ("chat_other", "other_project".to_owned()),
    ] {
        fixture.manager.watchers.insert(
            format!("workspace:{chat_id}"),
            WatchState {
                stream_id: format!("watch_{chat_id}"),
                snapshot: BTreeMap::new(),
                next_seq: 1,
                workspace: Some(WorkspaceAddress {
                    chat_id: chat_id.to_owned(),
                    project_id,
                    worktree_id: "worktree_test".to_owned(),
                    workspace_identity: "identity_test".to_owned(),
                }),
            },
        );
    }
    let before = fixture
        .storage
        .projection_record("project", &fixture.project_id)
        .unwrap();
    let result = fixture
        .manager
        .release_project_view(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert_eq!(result.result["projectId"], fixture.project_id);
    assert_eq!(fixture.manager.watchers.len(), 1);
    assert!(fixture
        .manager
        .watchers
        .contains_key("workspace:chat_other"));
    assert_eq!(
        fixture
            .storage
            .projection_record("project", &fixture.project_id)
            .unwrap(),
        before
    );
    fixture
        .manager
        .validate_active_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert_eq!(fixture.manager.watchers.len(), 2);
    assert!(fixture
        .manager
        .release_project_view(&fixture.storage, "missing_project")
        .is_err());
    assert_eq!(fixture.manager.watchers.len(), 2);
}

struct Fixture {
    _root: TempDir,
    source: PathBuf,
    storage: Storage,
    manager: ProjectManager,
    project_id: String,
}

impl Fixture {
    fn open() -> Self {
        let root = tempfile::tempdir().expect("temporary fixture");
        let source = root.path().join("source");
        let data = root.path().join("app-data");
        fs::create_dir_all(source.join("packages/app")).unwrap();
        fs::create_dir_all(source.join("src")).unwrap();
        fs::create_dir_all(source.join("assets")).unwrap();
        fs::write(source.join("src/edit.txt"), b"hello").unwrap();
        fs::write(source.join("src/run.sh"), b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(source.join("src/run.sh"), fs::Permissions::from_mode(0o755))
                .unwrap();
            std::os::unix::fs::symlink("edit.txt", source.join("src/edit-link")).unwrap();
        }
        fs::write(source.join("src/old-name.txt"), b"rename me\n").unwrap();
        fs::write(source.join("src/delete.txt"), b"delete me\n").unwrap();
        fs::write(source.join("packages/app/keep.txt"), b"nested\n").unwrap();
        fs::write(source.join("assets/data.bin"), [0, 159, 146, 150]).unwrap();
        git_raw(&source, ["init", "-q"]);
        git_raw(&source, ["add", "."]);
        git_raw(
            &source,
            [
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
        let head = git_text(&source, ["rev-parse", "HEAD"]);
        let cache_info = format!("160000,{head},vendor/sub");
        let output = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&source)
            .args(["update-index", "--add", "--cacheinfo", &cache_info])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        git_raw(
            &source,
            [
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "-m",
                "gitlink fixture",
            ],
        );
        fs::create_dir_all(&data).unwrap();
        let database = data.join("hello-agent.sqlite3");
        let mut storage = Storage::open(&database).unwrap();
        let mut manager = ProjectManager::new(&database).unwrap();
        let selected = source.join("packages/app");
        let action = manager
            .open_project(&mut storage, selected.to_str().unwrap(), "req_open_fixture")
            .unwrap();
        validate_project_payload_v1("projectIdentity", &action.result).unwrap();
        let project_id = action.result["projectId"].as_str().unwrap().to_string();
        Self {
            _root: root,
            source,
            storage,
            manager,
            project_id,
        }
    }

    fn create(&mut self) -> ActiveProject {
        let action = self
            .manager
            .create_worktree(&mut self.storage, &self.project_id, "req_create_fixture")
            .unwrap();
        validate_project_payload_v1("worktreeManifest", &action.result).unwrap();
        self.manager
            .validate_active_project(&self.storage, &self.project_id)
            .unwrap()
    }
}

#[test]
fn saved_project_catalog_uses_typed_wire_keys_without_activating_project() {
    let fixture = Fixture::open();
    let before = fixture
        .storage
        .projection_record("project", &fixture.project_id)
        .unwrap();
    let page = fixture
        .manager
        .list_saved_projects(&fixture.storage, None)
        .unwrap();
    assert_eq!(page.result["schemaVersion"], 1);
    assert_eq!(page.result["action"], "list");
    assert_eq!(page.result["projects"].as_array().unwrap().len(), 1);
    assert_eq!(page.result["projects"][0]["projectId"], fixture.project_id);
    assert_eq!(
        page.result["projects"][0]["canonicalPath"],
        before.as_ref().unwrap()["canonical_path"]
    );
    assert!(page.result["nextCursor"].is_null());
    assert_eq!(
        fixture
            .storage
            .projection_record("project", &fixture.project_id)
            .unwrap(),
        before
    );
}

#[test]
fn saved_project_reopen_accepts_new_head_and_reuses_active_snapshot() {
    let mut fixture = Fixture::open();
    git_raw(
        &fixture.source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "new head",
        ],
    );
    let reopened = fixture
        .manager
        .reopen_project(
            &mut fixture.storage,
            &fixture.project_id,
            "req_reopen_new_head",
        )
        .unwrap();
    assert_eq!(reopened.result["projectId"], fixture.project_id);
    assert_eq!(
        reopened.result["canonicalGitRoot"],
        canonical_protocol_path(&fs::canonicalize(&fixture.source).unwrap()).unwrap()
    );
    let active = fixture.create();
    let before = fixture
        .storage
        .projection_record("project", &fixture.project_id)
        .unwrap();
    fixture
        .manager
        .reopen_project(
            &mut fixture.storage,
            &fixture.project_id,
            "req_reopen_active",
        )
        .unwrap();
    assert_eq!(
        fixture
            .storage
            .projection_record("project", &fixture.project_id)
            .unwrap(),
        before
    );
    assert_eq!(
        fixture
            .manager
            .validate_active_project(&fixture.storage, &fixture.project_id)
            .unwrap()
            .worktree_id,
        active.worktree_id
    );
}

#[test]
fn saved_project_reopen_rejects_unknown_and_recovery_records() {
    let mut fixture = Fixture::open();
    assert!(fixture
        .manager
        .reopen_project(&mut fixture.storage, "project_unknown", "req_unknown")
        .is_err());
    let mut record = fixture
        .storage
        .projection_record("project", &fixture.project_id)
        .unwrap()
        .unwrap();
    record.insert("lifecycle".into(), json!("recoveryRequired"));
    append_object(
        &mut fixture.storage,
        "project",
        &fixture.project_id,
        "op_recovery_fixture",
        "req_recovery_fixture",
        Value::Object(record.clone()),
    )
    .unwrap();
    let before_reopen = fixture
        .storage
        .projection_record("project", &fixture.project_id)
        .unwrap();
    assert!(fixture
        .manager
        .reopen_project(
            &mut fixture.storage,
            &fixture.project_id,
            "req_recovery_reopen"
        )
        .is_err());
    assert_eq!(
        fixture
            .storage
            .projection_record("project", &fixture.project_id)
            .unwrap(),
        before_reopen
    );
}

#[cfg(unix)]
#[test]
fn saved_project_reopen_rejects_retargeted_symlink() {
    let mut fixture = Fixture::open();
    let other = Fixture::open();
    fs::rename(&fixture.source, fixture.source.with_extension("preserved")).unwrap();
    std::os::unix::fs::symlink(&other.source, &fixture.source).unwrap();
    let before = fixture
        .storage
        .projection_record("project", &fixture.project_id)
        .unwrap();
    assert!(fixture
        .manager
        .reopen_project(&mut fixture.storage, &fixture.project_id, "req_retarget")
        .is_err());
    assert_eq!(
        fixture
            .storage
            .projection_record("project", &fixture.project_id)
            .unwrap(),
        before
    );
}

#[test]
fn detached_worktree_preserves_original_and_surfaces_tree_read_and_diff() {
    let mut fixture = Fixture::open();
    let before_head = git_text(&fixture.source, ["rev-parse", "HEAD"]);
    let before_status = git_bytes_raw(&fixture.source, ["status", "--porcelain=v1", "-z"]);
    let before_refs = git_bytes_raw(
        &fixture.source,
        ["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    let before_bytes = fs::read(fixture.source.join("src/edit.txt")).unwrap();
    let active = fixture.create();

    assert_eq!(
        git_text(&active.worktree_path, ["rev-parse", "HEAD"]),
        before_head
    );
    assert!(git_raw_status(&active.worktree_path, ["symbolic-ref", "-q", "HEAD"]) != 0);
    assert_eq!(
        git_text(&fixture.source, ["rev-parse", "HEAD"]),
        before_head
    );
    assert_eq!(
        git_bytes_raw(&fixture.source, ["status", "--porcelain=v1", "-z"]),
        before_status
    );
    assert_eq!(
        git_bytes_raw(
            &fixture.source,
            ["for-each-ref", "--format=%(refname) %(objectname)"]
        ),
        before_refs
    );
    assert_eq!(
        fs::read(fixture.source.join("src/edit.txt")).unwrap(),
        before_bytes
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_ne!(
            fs::metadata(active.worktree_path.join("src/run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        assert_eq!(
            fs::read_link(active.worktree_path.join("src/edit-link")).unwrap(),
            PathBuf::from("edit.txt")
        );
        let submodule = active.worktree_path.join("vendor/sub");
        assert!(submodule.is_dir());
        assert!(!submodule.join(".git").exists());
    }

    let read = fixture
        .manager
        .read_file(&fixture.storage, &fixture.project_id, "src/edit.txt")
        .unwrap();
    assert_eq!(read.result["classification"], "utf8");
    validate_project_payload_v1("fileRead", &read.result).unwrap();
    assert_eq!(read.streams[0].chunks, vec!["hello"]);
    let tree = fixture
        .manager
        .get_tree(&fixture.storage, &fixture.project_id, "src", None)
        .unwrap();
    validate_project_payload_v1("treePage", &tree.result).unwrap();
    assert!(tree.result["entries"].as_array().unwrap().len() >= 3);

    fs::rename(
        active.worktree_path.join("src/old-name.txt"),
        active.worktree_path.join("src/new-name.txt"),
    )
    .unwrap();
    fs::remove_file(active.worktree_path.join("src/delete.txt")).unwrap();
    fs::write(active.worktree_path.join("assets/data.bin"), [0, 255, 1]).unwrap();
    fs::write(active.worktree_path.join("src/new.txt"), b"new\n").unwrap();
    let summary = fixture
        .manager
        .diff_summary(&fixture.storage, &fixture.project_id)
        .unwrap();
    validate_project_payload_v1("diffSummary", &summary.result).unwrap();
    let entries = summary.result["entries"].as_array().unwrap();
    for expected in ["renamed", "deleted", "binary", "added"] {
        assert!(
            entries.iter().any(|entry| entry["kind"] == expected),
            "{expected}"
        );
    }
    let renamed = fixture
        .manager
        .diff_file(&fixture.storage, &fixture.project_id, "src/new-name.txt")
        .unwrap();
    assert_eq!(renamed.result["kind"], "renamed");
    validate_project_payload_v1("diffFile", &renamed.result).unwrap();
    assert!(!renamed.streams[0].chunks.is_empty());
}

#[test]
fn restart_and_diff_do_not_depend_on_the_source_repository() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    fs::write(
        active.worktree_path.join("src/edit.txt"),
        b"changed after snapshot",
    )
    .unwrap();
    let moved_source = fixture.source.with_file_name("source-moved-away");
    fs::rename(&fixture.source, &moved_source).unwrap();

    let database = fixture.storage.path().to_path_buf();
    drop(fixture.manager);
    let mut manager = ProjectManager::new(&database).unwrap();
    let reopened = manager
        .validate_active_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert_eq!(reopened.base_commit, active.base_commit);
    let summary = manager
        .diff_summary(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert!(summary.result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["path"] == "src/edit.txt" && entry["kind"] == "modified"));
    assert!(!reopened
        .worktree_path
        .join(".git/objects/info/alternates")
        .exists());
    assert!(git_bytes_raw(&reopened.worktree_path, ["remote"]).is_empty());
}

#[cfg(unix)]
#[test]
fn snapshot_export_succeeds_with_read_only_source_git_directory() {
    let mut fixture = Fixture::open();
    let before_status = git_bytes_raw(&fixture.source, ["status", "--porcelain=v1"]);
    set_tree_writable(&fixture.source.join(".git"), false);
    let result = fixture.create();
    set_tree_writable(&fixture.source.join(".git"), true);
    assert_eq!(
        fs::read(result.worktree_path.join("src/edit.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(
        git_bytes_raw(&fixture.source, ["status", "--porcelain=v1"]),
        before_status
    );
}

#[test]
fn sha256_repository_is_snapshotted_when_system_git_supports_it() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&data).unwrap();
    let init = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&source)
        .args(["init", "-q", "--object-format=sha256"])
        .output()
        .unwrap();
    if !init.status.success() {
        let stderr = String::from_utf8_lossy(&init.stderr);
        assert!(stderr.contains("unknown") || stderr.contains("unsupported"));
        return;
    }
    fs::write(source.join("raw.txt"), b"sha256 raw bytes\n").unwrap();
    git_raw(&source, ["add", "."]);
    git_raw(
        &source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );
    let database = data.join("hello-agent.sqlite3");
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    let opened = manager
        .open_project(&mut storage, source.to_str().unwrap(), "req_sha256_open")
        .unwrap();
    let project_id = opened.result["projectId"].as_str().unwrap();
    let created = manager
        .create_worktree(&mut storage, project_id, "req_sha256_create")
        .unwrap();
    assert_eq!(created.result["objectFormat"], "sha256");
    assert_eq!(created.result["baseCommit"].as_str().unwrap().len(), 64);
    assert_eq!(created.result["baseTree"].as_str().unwrap().len(), 64);
    let snapshot = PathBuf::from(created.result["worktreePath"].as_str().unwrap());
    assert_eq!(
        fs::read(snapshot.join("raw.txt")).unwrap(),
        b"sha256 raw bytes\n"
    );
}

#[test]
fn case_colliding_committed_paths_fail_closed_before_activation() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&data).unwrap();
    git_raw(&source, ["init", "-q"]);
    fs::write(source.join("blob"), b"same").unwrap();
    let oid = git_text(&source, ["hash-object", "-w", "blob"]);
    let tree_input = format!("100644 blob {oid}\tCase.txt\n100644 blob {oid}\tcase.txt\n");
    let tree = git_with_stdin(&source, &["mktree"], tree_input.as_bytes());
    let commit = git_with_stdin(
        &source,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit-tree",
            tree.trim(),
        ],
        b"case collision\n",
    );
    git_raw(&source, ["update-ref", "HEAD", commit.trim()]);

    let database = data.join("hello-agent.sqlite3");
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    let opened = manager
        .open_project(&mut storage, source.to_str().unwrap(), "req_case_open")
        .unwrap();
    let project_id = opened.result["projectId"].as_str().unwrap();
    let failure = manager
        .create_worktree(&mut storage, project_id, "req_case_create")
        .unwrap_err();
    assert_eq!(failure.kind, "manifestMismatch");
    assert!(failure.detail.contains("case-colliding"));
    assert!(!storage.blocker_ids().unwrap().is_empty());
    assert!(fs::read_dir(data.join(WORKTREE_DIRECTORY))
        .unwrap()
        .all(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with('.')));
}

#[test]
fn corrupt_truncated_and_oversized_bundles_fail_closed() {
    assert_eq!(
        checked_bundle_size(SNAPSHOT_BUNDLE_MAX_BYTES_V1 - 1, 1).unwrap(),
        SNAPSHOT_BUNDLE_MAX_BYTES_V1
    );
    assert!(checked_bundle_size(SNAPSHOT_BUNDLE_MAX_BYTES_V1, 1).is_err());

    let fixture = Fixture::open();
    let source = fixture
        .manager
        .open_source_repository(&fixture.source)
        .unwrap();
    for (name, replacement) in [
        (".corrupt-bundle", b"not a git bundle".as_slice()),
        (".truncated-bundle", b"# v2 git bundle\n".as_slice()),
    ] {
        fixture.manager.worktrees_dir.create_dir(name).unwrap();
        let staging = open_real_child_dir(&fixture.manager.worktrees_dir, name).unwrap();
        fixture
            .manager
            .export_bundle(&source.git_dir, &staging)
            .unwrap();
        fs::write(
            fixture
                .manager
                .worktrees_root
                .join(name)
                .join(SNAPSHOT_BUNDLE),
            replacement,
        )
        .unwrap();
        let failure = fixture
            .manager
            .initialize_snapshot_repository(&staging, &source)
            .unwrap_err();
        assert_eq!(failure.kind, "manifestMismatch");
    }
}

#[test]
fn materialization_parser_rejects_non_utf8_paths_and_abnormal_modes() {
    let oid = "0".repeat(40);
    let mut non_utf8 = format!("100644 blob {oid}\t").into_bytes();
    non_utf8.push(0xff);
    assert!(parse_tree_record(&non_utf8, "sha1").is_err());
    assert!(parse_tree_record(format!("100664 blob {oid}\tfile.txt").as_bytes(), "sha1").is_err());
}

#[test]
fn optimistic_save_conflict_and_symlink_or_manifest_mismatch_write_zero_target_bytes() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let target = active.worktree_path.join("src/edit.txt");
    let original = fs::read(&target).unwrap();
    let expected = sha256_file_version_v1(&original);
    let begin = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    fixture
        .manager
        .append_save_chunk(save_id, 1, "world")
        .unwrap();
    fs::write(&target, b"external").unwrap();
    let conflict = fixture
        .manager
        .commit_save(
            &mut fixture.storage,
            save_id,
            &expected,
            1,
            5,
            "req_save_conflict",
        )
        .unwrap();
    assert_eq!(conflict.result["outcome"], "dirtyConflict");
    assert_eq!(fs::read(&target).unwrap(), b"external");

    let expected = sha256_file_version_v1(b"external");
    let begin = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    fixture
        .manager
        .append_save_chunk(save_id, 1, "saved")
        .unwrap();
    let committed = fixture
        .manager
        .commit_save(
            &mut fixture.storage,
            save_id,
            &expected,
            1,
            5,
            "req_save_committed",
        )
        .unwrap();
    assert_eq!(committed.result["outcome"], "committed");
    assert_eq!(fs::read(&target).unwrap(), b"saved");
    assert!(fixture.storage.blocker_ids().unwrap().is_empty());

    let outside = fixture.source.parent().unwrap().join("outside.txt");
    fs::write(&outside, b"outside").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, active.worktree_path.join("src/escape.txt")).unwrap();
        let failure = fixture
            .manager
            .read_file(&fixture.storage, &fixture.project_id, "src/escape.txt")
            .unwrap_err();
        assert_eq!(failure.kind, "symlinkEscape");
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
    }

    fs::write(&active.manifest_path, b"{}").unwrap();
    let before = fs::read(&target).unwrap();
    let failure = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &sha256_file_version_v1(&before),
            1,
            "utf8",
        )
        .unwrap_err();
    assert_eq!(failure.kind, "manifestMismatch");
    assert_eq!(fs::read(&target).unwrap(), before);
}

#[test]
fn read_hashes_exact_large_bytes_without_exposing_text_and_chunks_utf8_safely() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let exact = vec![b'x'; FILE_MAX_BYTES_V1 as usize];
    fs::write(active.worktree_path.join("src/exact.txt"), &exact).unwrap();
    let read = fixture
        .manager
        .read_file(&fixture.storage, &fixture.project_id, "src/exact.txt")
        .unwrap();
    assert_eq!(read.result["classification"], "utf8");
    assert_eq!(read.result["byteLength"], FILE_MAX_BYTES_V1);
    assert_eq!(read.streams[0].chunks.concat().as_bytes(), exact);

    let large = vec![b'x'; (FILE_MAX_BYTES_V1 + 1) as usize];
    fs::write(active.worktree_path.join("src/large.txt"), &large).unwrap();
    let read = fixture
        .manager
        .read_file(&fixture.storage, &fixture.project_id, "src/large.txt")
        .unwrap();
    validate_project_payload_v1("fileRead", &read.result).unwrap();
    assert_eq!(read.result["classification"], "tooLarge");
    assert_eq!(read.result["version"], sha256_file_version_v1(&large));
    assert!(read.streams.is_empty());

    let unicode = "가".repeat(MAX_TEXT_CHUNK_BYTES / 3 + 2);
    fs::write(active.worktree_path.join("src/unicode.txt"), &unicode).unwrap();
    let read = fixture
        .manager
        .read_file(&fixture.storage, &fixture.project_id, "src/unicode.txt")
        .unwrap();
    assert!(read.streams[0]
        .chunks
        .iter()
        .all(|chunk| chunk.len() <= MAX_TEXT_CHUNK_BYTES));
    assert_eq!(read.streams[0].chunks.concat(), unicode);
}

#[test]
fn diff_text_accepts_exactly_ten_mib_and_rejects_one_byte_more() {
    let exact = bounded_diff_text(vec![b'x'; DIFF_MAX_BYTES]).unwrap();
    let exact_text = std::str::from_utf8(&exact).unwrap();
    let chunks = chunk_utf8(exact_text, MAX_TEXT_CHUNK_BYTES);
    assert!(chunks
        .iter()
        .all(|chunk| chunk.len() <= MAX_TEXT_CHUNK_BYTES));
    assert_eq!(
        chunks.iter().map(String::len).sum::<usize>(),
        DIFF_MAX_BYTES
    );

    let error = bounded_diff_text(vec![b'x'; DIFF_MAX_BYTES + 1]).unwrap_err();
    assert_eq!(error.kind, "manifestMismatch");
}

#[test]
fn nul_and_sparse_over_limit_diff_content_are_binary_without_large_allocation() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    fs::write(active.worktree_path.join("src/edit.txt"), b"hello\0world").unwrap();
    let nul_read = fixture
        .manager
        .read_file(&fixture.storage, &fixture.project_id, "src/edit.txt")
        .unwrap();
    assert_eq!(nul_read.result["classification"], "binary");
    let nul_summary = fixture
        .manager
        .diff_summary(&fixture.storage, &fixture.project_id)
        .unwrap();
    let nul = nul_summary.result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "src/edit.txt")
        .unwrap();
    assert_eq!(nul["kind"], "binary");

    let tracked = active.worktree_path.join("src/delete.txt");
    std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&tracked)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let untracked = active.worktree_path.join("sparse-untracked.bin");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&untracked)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let summary = fixture
        .manager
        .diff_summary(&fixture.storage, &fixture.project_id)
        .unwrap();
    for path in ["src/delete.txt", "sparse-untracked.bin"] {
        let entry = summary.result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == path)
            .unwrap();
        assert_eq!(entry["kind"], "binary");
        let file = fixture
            .manager
            .diff_file(&fixture.storage, &fixture.project_id, path)
            .unwrap();
        assert_eq!(file.result["kind"], "binary");
        assert_eq!(file.result["byteLength"], 1024_u64 * 1024 * 1024);
        assert!(file.streams.is_empty());
    }
}

#[test]
fn sparse_base_over_diff_limit_is_classified_without_loading_blob() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&data).unwrap();
    let base = source.join("large-base.bin");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&base)
        .unwrap()
        .set_len(DIFF_MAX_BYTES as u64 + 1)
        .unwrap();
    git_raw(&source, ["init", "-q"]);
    git_raw(&source, ["add", "."]);
    git_raw(
        &source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );
    let database = data.join("hello-agent.sqlite3");
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    let opened = manager
        .open_project(&mut storage, source.to_str().unwrap(), "req_large_base")
        .unwrap();
    let project_id = opened.result["projectId"].as_str().unwrap();
    let created = manager
        .create_worktree(&mut storage, project_id, "req_large_base_worktree")
        .unwrap();
    let worktree = PathBuf::from(created.result["worktreePath"].as_str().unwrap());
    fs::remove_file(worktree.join("large-base.bin")).unwrap();
    let summary = manager.diff_summary(&storage, project_id).unwrap();
    let entry = &summary.result["entries"].as_array().unwrap()[0];
    assert_eq!(entry["kind"], "binary");
    let file = manager
        .diff_file(&storage, project_id, "large-base.bin")
        .unwrap();
    assert_eq!(file.result["kind"], "binary");
    assert_eq!(file.result["byteLength"], DIFF_MAX_BYTES as u64 + 1);
}

#[test]
fn git_registration_drift_blocks_file_access_before_any_write() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let target = active.worktree_path.join("src/edit.txt");
    let before = fs::read(&target).unwrap();
    fs::write(active.worktree_path.join("drift.txt"), b"drift").unwrap();
    git_raw(&active.worktree_path, ["add", "drift.txt"]);
    git_raw(
        &active.worktree_path,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "-m",
            "drift",
        ],
    );
    let failure = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &sha256_file_version_v1(&before),
            1,
            "utf8",
        )
        .unwrap_err();
    assert_eq!(failure.kind, "manifestMismatch");
    assert_eq!(fs::read(&target).unwrap(), before);
}

#[test]
fn crash_boundaries_persist_recovery_and_never_auto_retry() {
    for point in [
        WorktreeFailurePoint::AfterRecoveryLedgerBeforeExport,
        WorktreeFailurePoint::AfterBundleBeforeMaterialization,
        WorktreeFailurePoint::AfterWorkspaceRenameBeforeActivation,
    ] {
        let mut fixture = Fixture::open();
        let failure = fixture
            .manager
            .create_worktree_with_failure(
                &mut fixture.storage,
                &fixture.project_id,
                "req_crash",
                Some(point),
            )
            .unwrap_err();
        assert_eq!(failure.kind, "recoveryRequired");
        assert!(fixture
            .storage
            .blocker_ids()
            .unwrap()
            .iter()
            .any(|id| id.starts_with("lock:")));
        let retry = fixture
            .manager
            .create_worktree(&mut fixture.storage, &fixture.project_id, "req_retry")
            .unwrap_err();
        assert_eq!(retry.kind, "recoveryRequired");
    }
}

#[test]
fn unstarted_snapshot_recovery_survives_restart_and_allows_new_identity() {
    let mut fixture = Fixture::open();
    let before = git_bytes_raw(&fixture.source, ["worktree", "list", "--porcelain"]);
    fixture
        .manager
        .create_worktree_with_failure(
            &mut fixture.storage,
            &fixture.project_id,
            "req_unstarted",
            Some(WorktreeFailurePoint::AfterRecoveryLedgerBeforeExport),
        )
        .unwrap_err();
    let database = fixture.manager.data_root.join("hello-agent.sqlite3");
    drop(fixture.storage);
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    let inspection = manager
        .inspect_unstarted_snapshot(&storage, &fixture.project_id)
        .unwrap();
    let mut stale = inspection.clone();
    stale["lockRevision"] = json!(0);
    assert!(manager
        .abandon_unstarted_snapshot(&mut storage, &stale, "req_stale")
        .is_err());
    assert!(!storage.blocker_ids().unwrap().is_empty());
    storage.connection_for_test().execute_batch("CREATE TRIGGER fail_snapshot_release BEFORE INSERT ON events WHEN NEW.aggregate_type = 'lock' BEGIN SELECT RAISE(ABORT, 'release failure'); END;").unwrap();
    assert!(manager
        .abandon_unstarted_snapshot(&mut storage, &inspection, "req_failed_release")
        .is_err());
    storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_snapshot_release;")
        .unwrap();
    assert_eq!(
        manager
            .inspect_unstarted_snapshot(&storage, &fixture.project_id)
            .unwrap(),
        inspection
    );
    manager
        .abandon_unstarted_snapshot(&mut storage, &inspection, "req_abandon_unstarted")
        .unwrap();
    assert!(storage.blocker_ids().unwrap().is_empty());
    let completed_project = storage.projection_record("project", &fixture.project_id).unwrap();
    manager.abandon_unstarted_snapshot(&mut storage, &inspection, "req_response_lost").unwrap();
    assert_eq!(storage.projection_record("project", &fixture.project_id).unwrap(), completed_project);
    assert!(manager.abandon_unstarted_snapshot(&mut storage, &stale, "req_stale_completed").is_err());
    manager
        .open_project(
            &mut storage,
            fixture.source.to_str().unwrap(),
            "req_reopen_unstarted",
        )
        .unwrap();
    let created = manager
        .create_worktree(&mut storage, &fixture.project_id, "req_new_snapshot")
        .unwrap();
    assert_ne!(created.result["worktreeId"], inspection["worktreeId"]);
    assert!(manager.abandon_unstarted_snapshot(&mut storage, &inspection, "req_old_snapshot").is_err());
    assert_eq!(
        before,
        git_bytes_raw(&fixture.source, ["worktree", "list", "--porcelain"])
    );
}

#[test]
fn unstarted_snapshot_recovery_rejects_legacy_and_launched_records() {
    for point in [
        WorktreeFailurePoint::AfterRecoveryLedgerBeforeExport,
        WorktreeFailurePoint::AfterBundleBeforeMaterialization,
        WorktreeFailurePoint::AfterWorkspaceRenameBeforeActivation,
    ] {
        let mut fixture = Fixture::open();
        fixture
            .manager
            .create_worktree_with_failure(
                &mut fixture.storage,
                &fixture.project_id,
                "req_launch_proof",
                Some(point),
            )
            .unwrap_err();
        if point == WorktreeFailurePoint::AfterRecoveryLedgerBeforeExport {
            let inspected = fixture
                .manager
                .inspect_unstarted_snapshot(&fixture.storage, &fixture.project_id)
                .unwrap();
            let id = inspected["lockId"].as_str().unwrap();
            let mut lock = fixture
                .storage
                .projection_record("lock", id)
                .unwrap()
                .unwrap();
            lock.get_mut("evidence")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("launchFenceVersion");
            fixture
                .storage
                .upsert_projection_record("lock", id, "op_make_legacy", None, lock)
                .unwrap();
        }
        assert!(fixture
            .manager
            .inspect_unstarted_snapshot(&fixture.storage, &fixture.project_id)
            .is_err());
        assert!(!fixture.storage.blocker_ids().unwrap().is_empty());
    }
}

#[test]
fn unstarted_snapshot_recovery_transaction_checks_both_revisions() {
    for kind in ["project", "lock"] {
        let mut fixture = Fixture::open();
        fixture
            .manager
            .create_worktree_with_failure(
                &mut fixture.storage,
                &fixture.project_id,
                "req_cas_prepare",
                Some(WorktreeFailurePoint::AfterRecoveryLedgerBeforeExport),
            )
            .unwrap_err();
        let expected = fixture
            .manager
            .inspect_unstarted_snapshot(&fixture.storage, &fixture.project_id)
            .unwrap();
        let id = expected[if kind == "project" {
            "projectId"
        } else {
            "lockId"
        }]
        .as_str()
        .unwrap()
        .to_owned();
        let error = fixture
            .manager
            .abandon_unstarted_snapshot_with_hook(
                &mut fixture.storage,
                &expected,
                "req_cas_release",
                &mut |storage| {
                    let mut record = storage.projection_record(kind, &id).unwrap().unwrap();
                    if kind == "lock" {
                        record.get_mut("evidence").unwrap()["phase"] =
                            json!("snapshotExportStarting");
                    } else {
                        record.insert("test_revision_change".into(), json!(true));
                    }
                    storage
                        .upsert_projection_record(
                            kind,
                            &id,
                            "op_change_before_release",
                            None,
                            record,
                        )
                        .unwrap();
                },
            )
            .unwrap_err();
        assert!(error.detail.contains("changed before atomic update"));
        assert_eq!(
            required_project(&fixture.storage, &fixture.project_id).unwrap()["lifecycle"],
            "recoveryRequired"
        );
        assert!(!fixture.storage.blocker_ids().unwrap().is_empty());
    }
}

#[test]
fn post_rename_failures_keep_recovery_blocker_and_report_unknown_outcome() {
    for point in [
        SaveFailurePoint::AfterRenameBeforeDirectorySync,
        SaveFailurePoint::AfterReopenBeforeLedgerClear,
    ] {
        let mut fixture = Fixture::open();
        let active = fixture.create();
        let target = active.worktree_path.join("src/edit.txt");
        let expected = sha256_file_version_v1(&fs::read(&target).unwrap());
        let begin = fixture
            .manager
            .begin_save(
                &fixture.storage,
                &fixture.project_id,
                "src/edit.txt",
                &expected,
                5,
                "utf8",
            )
            .unwrap();
        let save_id = begin.result["saveId"].as_str().unwrap();
        fixture
            .manager
            .append_save_chunk(save_id, 1, "world")
            .unwrap();
        let error = fixture
            .manager
            .commit_save_with_failure(
                &mut fixture.storage,
                save_id,
                &expected,
                1,
                5,
                "req_save_fault",
                Some(point),
            )
            .unwrap_err();
        assert!(error.outcome_unknown, "{error:?}");
        assert_eq!(fs::read(&target).unwrap(), b"world");
        assert!(fixture
            .storage
            .blocker_ids()
            .unwrap()
            .iter()
            .any(|id| id.starts_with("lock:")));
        let blocked = fixture
            .manager
            .begin_save(
                &fixture.storage,
                &fixture.project_id,
                "src/edit.txt",
                &sha256_file_version_v1(b"world"),
                1,
                "utf8",
            )
            .unwrap_err();
        assert_eq!(blocked.kind, "recoveryRequired");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn same_content_parent_replacement_before_revalidation_is_recovery_required() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let parent = active.worktree_path.join("src");
    let external_parent = fixture._root.path().join("external-parent");
    let target = parent.join("edit.txt");
    let expected = sha256_file_version_v1(&fs::read(&target).unwrap());
    let begin = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap().to_string();
    fixture
        .manager
        .append_save_chunk(&save_id, 1, "world")
        .unwrap();
    let error = fixture
        .manager
        .commit_save_with_pre_revalidation_hook(
            &mut fixture.storage,
            &save_id,
            &expected,
            1,
            5,
            "req_parent_move",
            || {
                fs::rename(&parent, &external_parent).unwrap();
                fs::create_dir(&parent).unwrap();
                fs::write(parent.join("edit.txt"), b"hello").unwrap();
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, "recoveryRequired");
    assert!(!error.outcome_unknown);
    assert_eq!(
        fs::read(external_parent.join("edit.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(fs::read(parent.join("edit.txt")).unwrap(), b"hello");
    assert!(fixture
        .storage
        .blocker_ids()
        .unwrap()
        .iter()
        .any(|id| id.starts_with("lock:")));
}

#[cfg(target_os = "macos")]
#[test]
fn parent_move_at_stable_root_rename_syscall_never_writes_external_target() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let parent = active.worktree_path.join("src");
    let external_parent = fixture._root.path().join("syscall-moved-parent");
    let target = parent.join("edit.txt");
    let expected = sha256_file_version_v1(&fs::read(&target).unwrap());
    let begin = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap().to_string();
    fixture
        .manager
        .append_save_chunk(&save_id, 1, "world")
        .unwrap();
    let error = fixture
        .manager
        .commit_save_with_pre_syscall_hook(
            &mut fixture.storage,
            &save_id,
            &expected,
            1,
            5,
            "req_syscall_parent_move",
            || fs::rename(&parent, &external_parent).unwrap(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "recoveryRequired");
    assert!(!error.outcome_unknown);
    assert_eq!(
        fs::read(external_parent.join("edit.txt")).unwrap(),
        b"hello"
    );
    assert!(fixture
        .storage
        .blocker_ids()
        .unwrap()
        .iter()
        .any(|id| id.starts_with("lock:")));
}

#[cfg(target_os = "macos")]
#[test]
fn managed_worktree_child_swap_at_save_syscall_never_writes_symlink_target() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let managed = active.worktree_path.clone();
    let held = fixture._root.path().join("held-workspace");
    let external = fixture._root.path().join("external-workspace");
    fs::create_dir_all(external.join("src")).unwrap();
    fs::write(external.join("src/edit.txt"), b"external").unwrap();
    let expected = sha256_file_version_v1(&fs::read(managed.join("src/edit.txt")).unwrap());
    let begin = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    fixture
        .manager
        .append_save_chunk(save_id, 1, "world")
        .unwrap();
    let error = fixture
        .manager
        .commit_save_with_pre_syscall_hook(
            &mut fixture.storage,
            save_id,
            &expected,
            1,
            5,
            "req_workspace_swap",
            || {
                fs::rename(&managed, &held).unwrap();
                std::os::unix::fs::symlink(&external, &managed).unwrap();
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, "recoveryRequired");
    assert!(!error.outcome_unknown);
    assert_eq!(
        fs::read(external.join("src/edit.txt")).unwrap(),
        b"external"
    );
    assert_eq!(fs::read(held.join("src/edit.txt")).unwrap(), b"hello");
    assert!(fixture
        .storage
        .blocker_ids()
        .unwrap()
        .iter()
        .any(|id| id.starts_with("lock:")));
    fs::remove_file(&managed).unwrap();
    fs::rename(&held, &managed).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn parent_symlink_swap_at_stable_root_rename_syscall_never_writes_external_target() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let managed_parent = active.worktree_path.join("src");
    let held_parent = fixture._root.path().join("held-managed-parent");
    let external_parent = fixture._root.path().join("external-managed-parent");
    fs::create_dir_all(&external_parent).unwrap();
    fs::write(external_parent.join("edit.txt"), b"external").unwrap();
    let target = managed_parent.join("edit.txt");
    let expected = sha256_file_version_v1(&fs::read(&target).unwrap());
    let begin = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &expected,
            5,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap().to_string();
    fixture
        .manager
        .append_save_chunk(&save_id, 1, "world")
        .unwrap();
    let error = fixture
        .manager
        .commit_save_with_pre_syscall_hook(
            &mut fixture.storage,
            &save_id,
            &expected,
            1,
            5,
            "req_child_swap",
            || {
                fs::rename(&managed_parent, &held_parent).unwrap();
                std::os::unix::fs::symlink(&external_parent, &managed_parent).unwrap();
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, "recoveryRequired");
    assert!(!error.outcome_unknown);
    assert_eq!(
        fs::read(external_parent.join("edit.txt")).unwrap(),
        b"external"
    );
    assert!(fixture
        .storage
        .blocker_ids()
        .unwrap()
        .iter()
        .any(|id| id.starts_with("lock:")));
    fs::remove_file(&managed_parent).unwrap();
    fs::rename(&held_parent, &managed_parent).unwrap();
}

#[cfg(unix)]
#[test]
fn managed_root_swap_before_bound_git_never_touches_external_or_registers_worktree() {
    let mut fixture = Fixture::open();
    let before_registration = git_bytes_raw(&fixture.source, ["worktree", "list", "--porcelain"]);
    let managed = fixture.manager.worktrees_root.clone();
    let moved = managed.with_extension("held-root");
    let external = fixture
        .source
        .parent()
        .unwrap()
        .join("external-managed-root");
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("sentinel"), b"external").unwrap();
    let result = fixture.manager.create_worktree_with_pre_export_hook(
        &mut fixture.storage,
        &fixture.project_id,
        "req_root_swap",
        || {
            fs::rename(&managed, &moved).unwrap();
            std::os::unix::fs::symlink(&external, &managed).unwrap();
        },
    );
    let error = result.unwrap_err();
    assert_eq!(error.kind, "recoveryRequired");
    assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"external");
    assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
    assert_eq!(
        git_bytes_raw(&fixture.source, ["worktree", "list", "--porcelain"]),
        before_registration
    );
    fs::remove_file(&managed).unwrap();
    fs::rename(&moved, &managed).unwrap();
}

#[cfg(unix)]
#[test]
fn source_root_swap_after_capability_open_cannot_redirect_bundle_export() {
    let mut fixture = Fixture::open();
    let held = fixture.source.with_file_name("source-held");
    let external = fixture.source.with_file_name("source-external");
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("sentinel.txt"), b"external").unwrap();
    git_raw(&external, ["init", "-q"]);
    git_raw(&external, ["add", "."]);
    git_raw(
        &external,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "-m",
            "external",
        ],
    );
    let external_head = git_text(&external, ["rev-parse", "HEAD"]);
    let source_path = fixture.source.clone();
    let action = fixture
        .manager
        .create_worktree_with_pre_export_hook(
            &mut fixture.storage,
            &fixture.project_id,
            "req_source_swap",
            || {
                fs::rename(&source_path, &held).unwrap();
                std::os::unix::fs::symlink(&external, &source_path).unwrap();
            },
        )
        .unwrap();
    let snapshot = PathBuf::from(action.result["worktreePath"].as_str().unwrap());
    assert_eq!(fs::read(snapshot.join("src/edit.txt")).unwrap(), b"hello");
    assert_eq!(
        fs::read(external.join("sentinel.txt")).unwrap(),
        b"external"
    );
    assert_eq!(git_text(&external, ["rev-parse", "HEAD"]), external_head);
    fs::remove_file(&fixture.source).unwrap();
    fs::rename(&held, &fixture.source).unwrap();
}

#[test]
fn executable_git_configuration_is_ignored_without_running_sentinels() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    let sentinel = root.path().join("sentinel");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("file.txt"), b"safe").unwrap();
    fs::write(
        source.join(".gitattributes"),
        b"file.txt filter=evil diff=evil\n",
    )
    .unwrap();
    git_raw(&source, ["init", "-q"]);
    git_raw(&source, ["add", "."]);
    git_raw(
        &source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );
    git_raw(
        &source,
        [
            "config",
            "filter.evil.process",
            &format!("sh -c 'printf process > {}'", sentinel.display()),
        ],
    );
    git_raw(
        &source,
        [
            "config",
            "core.pager",
            &format!("sh -c 'printf pager > {}'", sentinel.display()),
        ],
    );
    git_raw(
        &source,
        [
            "config",
            "diff.external",
            &format!("sh -c 'printf external > {}'", sentinel.display()),
        ],
    );
    git_raw(
        &source,
        [
            "config",
            "diff.evil.textconv",
            &format!("sh -c 'printf textconv > {}'", sentinel.display()),
        ],
    );
    let hooks = root.path().join("hooks");
    fs::create_dir_all(&hooks).unwrap();
    fs::write(
        hooks.join("post-checkout"),
        format!("#!/bin/sh\nprintf hook > '{}'\n", sentinel.display()),
    )
    .unwrap();
    git_raw(
        &source,
        ["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    git_raw(
        &source,
        [
            "config",
            "filter.evil.smudge",
            &format!("sh -c 'printf filter > {}'", sentinel.display()),
        ],
    );
    let database = data.join("hello-agent.sqlite3");
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    let opened = manager
        .open_project(&mut storage, source.to_str().unwrap(), "req_danger")
        .unwrap();
    let project_id = opened.result["projectId"].as_str().unwrap();
    let created = manager
        .create_worktree(&mut storage, project_id, "req_snapshot")
        .unwrap();
    let snapshot = PathBuf::from(created.result["worktreePath"].as_str().unwrap());
    assert_eq!(fs::read(snapshot.join("file.txt")).unwrap(), b"safe");
    assert!(!sentinel.exists());
}

#[test]
fn inherited_required_filter_configuration_cannot_execute() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let data = root.path().join("data");
    let sentinel = root.path().join("inherited-filter-sentinel");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(source.join("file.txt"), b"safe").unwrap();
    fs::write(source.join(".gitattributes"), b"file.txt filter=evil\n").unwrap();
    git_raw(&source, ["init", "-q"]);
    git_raw(&source, ["add", "."]);
    git_raw(
        &source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );
    let command = format!("sh -c 'printf inherited > {}'", sentinel.display());
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("project::tests::inherited_required_filter_configuration_child")
        .arg("--ignored")
        .arg("--nocapture")
        .env("HELLO_AGENT_INHERITED_FILTER_CHILD", "1")
        .env("HELLO_AGENT_INHERITED_FILTER_SOURCE", &source)
        .env("HELLO_AGENT_INHERITED_FILTER_DATA", &data)
        .env("HELLO_AGENT_INHERITED_FILTER_SENTINEL", &sentinel)
        .env("GIT_CONFIG_COUNT", "3")
        .env("GIT_CONFIG_KEY_0", "filter.evil.required")
        .env("GIT_CONFIG_VALUE_0", "true")
        .env("GIT_CONFIG_KEY_1", "filter.evil.smudge")
        .env("GIT_CONFIG_VALUE_1", &command)
        .env("GIT_CONFIG_KEY_2", "filter.evil.process")
        .env("GIT_CONFIG_VALUE_2", &command)
        .env("GIT_CONFIG_PARAMETERS", "'filter.evil.required=true'")
        .env("GIT_EXEC_PATH", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!sentinel.exists());
}

#[test]
#[ignore = "spawned by inherited_required_filter_configuration_cannot_execute"]
fn inherited_required_filter_configuration_child() {
    if std::env::var_os("HELLO_AGENT_INHERITED_FILTER_CHILD").is_none() {
        return;
    }
    let source = PathBuf::from(std::env::var_os("HELLO_AGENT_INHERITED_FILTER_SOURCE").unwrap());
    let data = PathBuf::from(std::env::var_os("HELLO_AGENT_INHERITED_FILTER_DATA").unwrap());
    let sentinel =
        PathBuf::from(std::env::var_os("HELLO_AGENT_INHERITED_FILTER_SENTINEL").unwrap());
    let database = data.join("hello-agent.sqlite3");
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    let opened = manager
        .open_project(
            &mut storage,
            source.to_str().unwrap(),
            "req_inherited_filter",
        )
        .unwrap();
    manager
        .create_worktree(
            &mut storage,
            opened.result["projectId"].as_str().unwrap(),
            "req_inherited_filter_worktree",
        )
        .unwrap();
    assert!(!sentinel.exists());
}

#[test]
fn non_git_bare_headless_and_external_linked_worktree_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let database = data.join("hello-agent.sqlite3");
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();

    let plain = root.path().join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert!(manager
        .open_project(&mut storage, plain.to_str().unwrap(), "req_plain")
        .is_err());

    let bare = root.path().join("bare.git");
    fs::create_dir_all(&bare).unwrap();
    git_raw(&bare, ["init", "--bare", "-q"]);
    assert!(manager
        .open_project(&mut storage, bare.to_str().unwrap(), "req_bare")
        .is_err());

    let headless = root.path().join("headless");
    fs::create_dir_all(&headless).unwrap();
    git_raw(&headless, ["init", "-q"]);
    assert!(manager
        .open_project(&mut storage, headless.to_str().unwrap(), "req_headless")
        .is_err());

    let source = root.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("file.txt"), b"fixture").unwrap();
    git_raw(&source, ["init", "-q"]);
    git_raw(&source, ["add", "."]);
    git_raw(
        &source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    );
    let linked = root.path().join("external-linked");
    git_raw(
        &source,
        [
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    let failure = manager
        .open_project(&mut storage, linked.to_str().unwrap(), "req_linked")
        .unwrap_err();
    assert_eq!(failure.kind, "manifestMismatch");
}

#[test]
fn close_abandons_without_deletion_and_new_open_never_reuses_path() {
    let mut fixture = Fixture::open();
    let first = fixture.create();
    let closed = fixture
        .manager
        .close_project(&mut fixture.storage, &fixture.project_id, "req_close")
        .unwrap();
    validate_project_payload_v1("projectClose", &closed.result).unwrap();
    assert!(first.worktree_path.exists());
    let blocked = fixture
        .manager
        .read_file(&fixture.storage, &fixture.project_id, "src/edit.txt")
        .unwrap_err();
    assert_eq!(blocked.kind, "recoveryRequired");

    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_reopen",
        )
        .unwrap();
    let second = fixture
        .manager
        .create_worktree(&mut fixture.storage, &fixture.project_id, "req_second")
        .unwrap();
    assert_ne!(
        second.result["worktreePath"].as_str().unwrap(),
        first.worktree_path.to_str().unwrap()
    );
    assert!(first.worktree_path.exists());
}

#[test]
fn watcher_coalesces_paths_and_overflow_requires_rescan() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    fs::write(active.worktree_path.join("src/edit.txt"), b"changed").unwrap();
    fs::write(active.worktree_path.join("src/another.txt"), b"changed").unwrap();
    let events = fixture.manager.poll_watchers(&fixture.storage);
    assert_eq!(events.len(), 1);
    assert!(!events[0].payload["rescanRequired"].as_bool().unwrap());
    let paths = events[0].payload["paths"].as_array().unwrap();
    assert!(paths.iter().any(|path| path == "src/edit.txt"));
    assert!(paths.iter().any(|path| path == "src/another.txt"));
    validate_project_payload_v1("watchEvent", &events[0].payload).unwrap();
    let first_stream = events[0].stream_id.clone();

    fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    fs::write(active.worktree_path.join("src/edit.txt"), b"changed again").unwrap();
    let restarted = fixture.manager.poll_watchers(&fixture.storage);
    assert_eq!(restarted.len(), 1);
    assert_ne!(restarted[0].stream_id, first_stream);
    assert_eq!(restarted[0].seq, 1);

    for index in 0..=WATCH_MAX_CHANGED_PATHS {
        fs::write(
            active
                .worktree_path
                .join(format!("overflow-{index:04}.txt")),
            b"x",
        )
        .unwrap();
    }
    let events = fixture.manager.poll_watchers(&fixture.storage);
    assert_eq!(events.len(), 1);
    assert!(events[0].payload["rescanRequired"].as_bool().unwrap());
    assert!(events[0].payload["paths"].as_array().unwrap().is_empty());
    validate_project_payload_v1("watchEvent", &events[0].payload).unwrap();
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());

    let resumed = fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert!(resumed.result["watching"].as_bool().unwrap());
    fs::write(active.worktree_path.join("resumed.txt"), b"x").unwrap();
    let resumed_events = fixture.manager.poll_watchers(&fixture.storage);
    assert_eq!(resumed_events.len(), 1);
    assert!(!resumed_events[0].payload["rescanRequired"]
        .as_bool()
        .unwrap());
}

#[test]
fn watcher_initial_scan_over_capacity_emits_once_and_stays_suspended() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let overflow = active.worktree_path.join("watch-overflow");
    fs::create_dir(&overflow).unwrap();
    for index in 0..=WATCH_MAX_ENTRIES {
        fs::write(overflow.join(format!("{index:05}")), b"").unwrap();
    }
    let action = fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert!(!action.result["watching"].as_bool().unwrap());
    assert!(action.result["rescanRequired"].as_bool().unwrap());
    assert_eq!(action.watch_events.len(), 1);
    assert!(action.watch_events[0].payload["rescanRequired"]
        .as_bool()
        .unwrap());
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());

    fs::remove_dir_all(&overflow).unwrap();
    let resumed = fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert!(resumed.result["watching"].as_bool().unwrap());
}

#[cfg(unix)]
#[test]
fn watcher_scan_unavailable_emits_rescan_once_then_suspends() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    let unavailable = active
        .worktree_path
        .with_extension("temporarily-unavailable");
    fs::rename(&active.worktree_path, &unavailable).unwrap();
    let events = fixture.manager.poll_watchers(&fixture.storage);
    assert_eq!(events.len(), 1);
    assert!(events[0].payload["rescanRequired"].as_bool().unwrap());
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());
    fs::rename(&unavailable, &active.worktree_path).unwrap();
    let resumed = fixture
        .manager
        .watch_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert!(resumed.result["watching"].as_bool().unwrap());
}

#[cfg(unix)]
#[test]
fn rename_swap_after_validation_cannot_read_or_write_external_directory() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let src = active.worktree_path.join("src");
    let held = active.worktree_path.join("src-held");
    let outside = fixture.source.parent().unwrap().join("external-swap");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("edit.txt"), b"external-secret").unwrap();
    let error = fixture
        .manager
        .read_file_with_post_validation_hook(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            || {
                fs::rename(&src, &held).unwrap();
                std::os::unix::fs::symlink(&outside, &src).unwrap();
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, "symlinkEscape");
    assert!(!error.detail.contains("external-secret"));
    assert_eq!(
        fs::read(outside.join("edit.txt")).unwrap(),
        b"external-secret"
    );

    fs::remove_file(&src).unwrap();
    fs::rename(&held, &src).unwrap();
    let target = src.join("edit.txt");
    let expected = sha256_file_version_v1(&fs::read(&target).unwrap());
    let begin = fixture
        .manager
        .begin_save(
            &fixture.storage,
            &fixture.project_id,
            "src/edit.txt",
            &expected,
            4,
            "utf8",
        )
        .unwrap();
    let save_id = begin.result["saveId"].as_str().unwrap();
    fixture
        .manager
        .append_save_chunk(save_id, 1, "safe")
        .unwrap();
    fs::rename(&src, outside.join("moved-src")).unwrap();
    let error = fixture
        .manager
        .commit_save(
            &mut fixture.storage,
            save_id,
            &expected,
            1,
            4,
            "req_external_move",
        )
        .unwrap_err();
    assert!(!error.outcome_unknown);
    assert_eq!(
        fs::read(outside.join("moved-src/edit.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(
        fs::read(outside.join("edit.txt")).unwrap(),
        b"external-secret"
    );
}

#[cfg(unix)]
fn set_tree_writable(path: &Path, writable: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            set_tree_writable(&entry.unwrap().path(), writable);
        }
    }
    let mode = metadata.permissions().mode();
    let next = if writable {
        mode | 0o200
    } else {
        mode & !0o222
    };
    fs::set_permissions(path, fs::Permissions::from_mode(next)).unwrap();
}

fn git_raw<const N: usize>(directory: &Path, args: [&str; N]) {
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

fn git_raw_status<const N: usize>(directory: &Path, args: [&str; N]) -> i32 {
    Command::new("/usr/bin/git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .status()
        .unwrap()
        .code()
        .unwrap_or(-1)
}

fn git_bytes_raw<const N: usize>(directory: &Path, args: [&str; N]) -> Vec<u8> {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

fn git_with_stdin(directory: &Path, args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn git_text<const N: usize>(directory: &Path, args: [&str; N]) -> String {
    String::from_utf8(git_bytes_raw(directory, args))
        .unwrap()
        .trim()
        .to_string()
}
