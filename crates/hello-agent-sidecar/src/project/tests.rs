use super::*;
use hello_agent_protocol::project_v1::validate_project_payload_v1;
use tempfile::TempDir;

#[test]
fn active_manifest_binding_rejects_other_managed_child() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let original = required_project(&fixture.storage, &fixture.project_id).unwrap();
    let mut changed = original.clone();
    changed.insert("manifest_path".into(), json!(canonical_protocol_path_without_existing(
        &fixture.manager.manifests_root.join("worktree_other.json")
    ).unwrap()));
    append_object(&mut fixture.storage, "project", &fixture.project_id,
        "op_wrong_manifest_child", "req_wrong_manifest_child", Value::Object(changed)).unwrap();
    let error = fixture.manager.validate_active_project(&fixture.storage, &fixture.project_id).err().unwrap();
    assert!(error.detail.contains("exact managed snapshot manifest child"));
    append_object(&mut fixture.storage, "project", &fixture.project_id,
        "op_restore_manifest_child", "req_restore_manifest_child", Value::Object(original)).unwrap();
    let restored = fixture.manager.validate_active_project(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(restored.manifest_path, active.manifest_path);
}

#[cfg(unix)]
#[test]
fn snapshot_rejects_managed_root_inside_source_before_preparation() {
    let fixture = Fixture::open();
    let data = fixture.source.join("nested-app-data");
    fs::create_dir(&data).unwrap();
    let db = data.join("nested.sqlite3");
    let mut manager = ProjectManager::new(&db).unwrap();
    let mut storage = Storage::open(&db).unwrap();
    manager.open_project(&mut storage, fixture.source.to_str().unwrap(), "req_nested_open").unwrap();
    let before = required_project(&storage, &fixture.project_id).unwrap();
    let sequence = storage.connection_for_test().query_row("SELECT COUNT(*) FROM events", [], |row| row.get::<_, i64>(0)).unwrap();
    let error = manager.create_worktree(&mut storage, &fixture.project_id, "req_nested_create").unwrap_err();
    assert!(error.detail.contains("inside the original project"));
    assert_eq!(required_project(&storage, &fixture.project_id).unwrap(), before);
    assert_eq!(storage.connection_for_test().query_row("SELECT COUNT(*) FROM events", [], |row| row.get::<_, i64>(0)).unwrap(), sequence);
    assert!(storage.blocker_ids().unwrap().is_empty());
    assert_eq!(fs::read_dir(manager.worktrees_root).unwrap().count(), 0);
    assert_eq!(fs::read_dir(manager.manifests_root).unwrap().count(), 0);
}

fn make_legacy_binding(fixture: &mut Fixture) -> serde_json::Map<String, Value> {
    let mut record = fixture.storage.projection_record("project", &fixture.project_id).unwrap().unwrap();
    record.remove(path_binding::FIELD);
    append_object(&mut fixture.storage, "project", &fixture.project_id,
        "op_legacy_path_fixture", "req_legacy_path_fixture", Value::Object(record.clone())).unwrap();
    fixture.storage.projection_record("project", &fixture.project_id).unwrap().unwrap()
}

#[test]
fn legacy_path_binding_requires_picker_before_catalog_or_snapshot_access() {
    let mut fixture = Fixture::open();
    let original = make_legacy_binding(&mut fixture);
    let moved = fixture.source.with_extension("moved");
    fs::rename(&fixture.source, &moved).unwrap();
    assert_eq!(fixture.manager.application_history(&fixture.storage, &fixture.project_id).unwrap().result["entries"], json!([]));
    let error = fixture.manager.reopen_project(&mut fixture.storage, &fixture.project_id, "req_legacy_reopen").unwrap_err();
    assert!(error.detail.contains("native folder selection"), "{:?}", error);
    let error = fixture.manager.create_worktree(&mut fixture.storage, &fixture.project_id, "req_legacy_create").unwrap_err();
    assert!(error.detail.contains("native folder selection"), "{:?}", error);
    assert_eq!(fixture.storage.projection_record("project", &fixture.project_id).unwrap().unwrap(), original);
    fs::rename(moved, &fixture.source).unwrap();
    fixture.manager.open_project(&mut fixture.storage, fixture.source.to_str().unwrap(), "req_legacy_picker").unwrap();
    fixture.manager.reopen_project(&mut fixture.storage, &fixture.project_id, "req_bound_reopen").unwrap();
}

#[test]
fn legacy_path_binding_picker_preserves_active_workspace_and_draft() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    fs::write(active.worktree_path.join("src/edit.txt"), b"unsaved work kept").unwrap();
    let mut original = make_legacy_binding(&mut fixture);
    assert!(fixture.manager.validate_active_project(&fixture.storage, &fixture.project_id).is_err());
    fixture.manager.open_project(&mut fixture.storage, fixture.source.to_str().unwrap(), "req_legacy_active_picker").unwrap();
    let record = fixture.storage.projection_record("project", &fixture.project_id).unwrap().unwrap();
    path_binding::require(&record).unwrap();
    original.insert("selected_path".into(), json!(canonical_protocol_path(&fixture.source).unwrap()));
    original.insert(path_binding::FIELD.into(), record[path_binding::FIELD].clone());
    original.insert("last_event_sequence".into(), record["last_event_sequence"].clone());
    assert_eq!(record, original);
    let restored = fixture.manager.validate_active_project(&fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(restored.worktree_id, active.worktree_id);
    assert_eq!(fs::read(active.worktree_path.join("src/edit.txt")).unwrap(), b"unsaved work kept");
    assert_eq!(fs::read(fixture.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[test]
fn legacy_path_binding_picker_does_not_clear_recovery() {
    let mut fixture = Fixture::open();
    let mut record = make_legacy_binding(&mut fixture);
    record.insert("lifecycle".into(), json!("recoveryRequired"));
    append_object(&mut fixture.storage, "project", &fixture.project_id,
        "op_legacy_recovery", "req_legacy_recovery", Value::Object(record)).unwrap();
    let error = fixture.manager.open_project(&mut fixture.storage, fixture.source.to_str().unwrap(), "req_legacy_recovery_picker").unwrap_err();
    assert_eq!(error.kind, "recoveryRequired");
    let record = fixture.storage.projection_record("project", &fixture.project_id).unwrap().unwrap();
    path_binding::require(&record).unwrap();
    assert_eq!(record["lifecycle"], "recoveryRequired");
}

#[test]
fn legacy_path_binding_mismatched_marker_is_not_repaired_implicitly() {
    let mut fixture = Fixture::open();
    let mut record = fixture.storage.projection_record("project", &fixture.project_id).unwrap().unwrap();
    record.insert(path_binding::FIELD.into(), path_binding::value("/different/source"));
    append_object(&mut fixture.storage, "project", &fixture.project_id,
        "op_invalid_binding", "req_invalid_binding", Value::Object(record)).unwrap();
    let before = fixture.storage.projection_record("project", &fixture.project_id).unwrap();
    let moved = fixture.source.with_extension("moved");
    fs::rename(&fixture.source, &moved).unwrap();
    assert_eq!(fixture.manager.reopen_project(&mut fixture.storage, &fixture.project_id, "req_invalid_reopen").unwrap_err().kind, "manifestMismatch");
    assert_eq!(fixture.manager.create_worktree(&mut fixture.storage, &fixture.project_id, "req_invalid_create").unwrap_err().kind, "manifestMismatch");
    fs::rename(moved, &fixture.source).unwrap();
    assert_eq!(fixture.manager.open_project(&mut fixture.storage, fixture.source.to_str().unwrap(), "req_invalid_picker").unwrap_err().kind, "manifestMismatch");
    assert_eq!(fixture.storage.projection_record("project", &fixture.project_id).unwrap(), before);
}

#[cfg(unix)]
#[test]
#[ignore = "opt-in generated 10000-file snapshot phase diagnostic"]
fn large_snapshot_phase_diagnostic() {
    let fixture = Fixture::open();
    // Application baselines deliberately reject submodules; remove only the
    // synthetic fixture gitlink before constructing this independent workload.
    git_raw(&fixture.source, ["rm", "--cached", "vendor/sub"]);
    for directory in 0..1000 {
        let folder = fixture.source.join(format!("tree/folder-{directory:04}"));
        fs::create_dir_all(&folder).unwrap();
        for file in 0..10 {
            fs::write(folder.join(format!("file-{file}.txt")), b"fixture\n").unwrap();
        }
    }
    fs::write(fixture.source.join("large.txt"), vec![b'x'; 5 * 1024 * 1024]).unwrap();
    git_raw(&fixture.source, ["add", "."]);
    git_raw(&fixture.source, ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "large diagnostic"]);
    let begin = std::time::Instant::now();
    let source = fixture.manager.open_source_repository(&fixture.source).unwrap();
    let opened = begin.elapsed();
    let first = application::git_baseline::raw_baseline::capture_clean_baseline(&fixture.manager, &source).unwrap();
    let captured = begin.elapsed();
    let staging_root = TempDir::new().unwrap();
    let staging = Dir::open_ambient_dir(staging_root.path(), ambient_authority()).unwrap();
    fixture.manager.export_bundle(&source.git_dir, &staging).unwrap();
    let exported = begin.elapsed();
    let second = application::git_baseline::raw_baseline::capture_clean_baseline(&fixture.manager, &source).unwrap();
    assert_eq!(first, second);
    let recaptured = begin.elapsed();
    fixture.manager.initialize_snapshot_repository(&staging, &source).unwrap();
    let initialized = begin.elapsed();
    let stats = fixture.manager.materialize_snapshot(&staging, &source).unwrap();
    let materialized = begin.elapsed();
    assert!(stats.entry_count >= 10001);
    assert_eq!(fs::read(staging_root.path().join("tree/folder-0999/file-9.txt")).unwrap(), b"fixture\n");
    assert!(git_text(&fixture.source, ["status", "--porcelain"]).is_empty());
    println!("{}", json!({"diagnostic":true,"gateEligible":false,
        "kind":"snapshot-phase-cost","addedFiles":10001,"addedDirectories":1001,
        "openMs":opened.as_secs_f64()*1000.0,
        "baselineMs":(captured-opened).as_secs_f64()*1000.0,
        "exportMs":(exported-captured).as_secs_f64()*1000.0,
        "baselineRecheckMs":(recaptured-exported).as_secs_f64()*1000.0,
        "initializeMs":(initialized-recaptured).as_secs_f64()*1000.0,
        "materializeAndSyncMs":(materialized-initialized).as_secs_f64()*1000.0,
        "scope":"Private phase diagnostic; excludes receipt publication, UI and full create safety sequence"}));
}

#[test]
fn materialized_snapshot_refreshes_index_stat_without_hiding_edits() {
    let mut fixture = Fixture::open();
    let source_before = fs::read(fixture.source.join(".git/index")).unwrap();
    let active = fixture.create();
    assert_eq!(fs::read(fixture.source.join(".git/index")).unwrap(), source_before);
    let debug = git_text(&active.worktree_path, ["ls-files", "--debug", "--", "src/edit.txt"]);
    assert!(!debug.contains("ctime: 0:0"), "materialized index lacks stat data: {debug}");
    assert_eq!(git_text(&active.worktree_path, ["write-tree"]), active.base_tree);
    assert!(fixture.manager.diff_summary(&fixture.storage, &fixture.project_id).unwrap().result["entries"].as_array().unwrap().is_empty());
    fs::write(active.worktree_path.join("src/edit.txt"), b"world").unwrap();
    let changed = fixture.manager.diff_summary(&fixture.storage, &fixture.project_id).unwrap();
    assert!(changed.result["entries"].as_array().unwrap().iter().any(|entry| entry["path"] == "src/edit.txt"));
    assert_eq!(git_text(&active.worktree_path, ["write-tree"]), active.base_tree);
    assert_eq!(fs::read(fixture.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[test]
fn materialized_sync_batch_drains_full_and_trailing_files() {
    let temporary = TempDir::new().unwrap();
    let root = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
    for count in [0, 1, 4] {
        let mut files = Vec::new();
        for index in 0..count {
            let mut file = root.create(format!("{count}-{index}")).unwrap();
            file.write_all(b"committed bytes").unwrap();
            files.push(file);
        }
        sync_materialized_files(&mut files).unwrap();
        assert!(files.is_empty());
        for index in 0..count {
            assert_eq!(root.read(format!("{count}-{index}")).unwrap(), b"committed bytes");
        }
    }
}

#[cfg(unix)]
#[test]
fn materialized_sync_batch_propagates_sync_failure_and_drains_handles() {
    let temporary = TempDir::new().unwrap();
    let root = Dir::open_ambient_dir(temporary.path(), ambient_authority()).unwrap();
    let mut files = vec![root.create("before").unwrap()];
    // A device cannot provide regular-file durability. Exercise the real I/O
    // failure alongside successful file workers without modifying user files.
    files.push(CapFile::from_std(fs::File::open("/dev/null").unwrap()));
    files.push(root.create("after").unwrap());
    assert!(sync_materialized_files(&mut files).is_err());
    assert!(files.is_empty());
}

#[cfg(unix)]
#[test]
fn retirement_preserves_completed_record_and_lock_but_rejects_pending() {
    use application::review::{confirmation::Confirmations, intent, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    assert_eq!(f.manager.application_capabilities().result["enabled"], cfg!(target_os = "macos"));
    assert_eq!(f.manager.application_cleanup_enabled, cfg!(target_os = "macos"));
    f.manager.application_cleanup_enabled = false;
    let mut owner = Confirmations::default();
    let (token, _) = owner.open(&f.manager, &f.storage, ReviewTarget::Project(&f.project_id), &review_id).unwrap();
    let grant = owner.prepare_backup(&f.manager, &mut f.storage, &token, "req_retire_backup").unwrap();
    let prepared = intent::begin(&f.manager, &mut f.storage, &mut owner, grant, "req_retire_prepare").unwrap();
    let id = prepared["applicationId"].as_str().unwrap();
    assert!(intent::retirement::preview(&f.storage, &f.project_id, id).is_err());
    let terminal = intent::abandon_unwritten(&mut f.storage, &f.project_id, &prepared, "req_retire_abandon").unwrap();
    let lock_id = terminal["lockId"].as_str().unwrap();
    let lock = f.storage.projection_record("lock", lock_id).unwrap();
    let before = required_project(&f.storage, &f.project_id).unwrap();
    assert!(intent::retirement::retire(&mut f.storage, &f.project_id, id, "stale", "req_retire_stale").is_err());
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before);
    let preview = intent::retirement::preview(&f.storage, &f.project_id, id).unwrap();
    assert_eq!(f.manager.inspect_application_retirement(&f.storage, &f.project_id, id).unwrap().result["enabled"], false);
    assert!(f.manager.retire_application(&mut f.storage, &f.project_id, id, preview["fingerprint"].as_str().unwrap(), "req_product_retire_disabled").is_err());
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before);
    intent::retirement::retire(&mut f.storage, &f.project_id, id, preview["fingerprint"].as_str().unwrap(), "req_retire").unwrap();
    let after = required_project(&f.storage, &f.project_id).unwrap();
    assert!(!after.contains_key("application_intent"));
    assert_eq!(after["application_history"][id], terminal);
    assert_eq!(f.storage.projection_record("lock", lock_id).unwrap(), lock);
    assert_eq!(after["application_backups"], before["application_backups"]);
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(unix)]
#[test]
fn abandoned_backup_cleanup_rechecks_file_and_keeps_durable_result() {
    use application::review::{backup::{staging, cleanup}, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    f.manager.application_cleanup_enabled = false;
    assert!(staging::stop_after_reservation_for_test(&f.manager, &mut f.storage, ReviewTarget::Project(&f.project_id), &review_id).is_err());
    let pending = staging::inspect(&f.manager, &f.storage, &f.project_id, &review_id).unwrap();
    let name = pending["name"].as_str().unwrap();
    let root = f.manager.data_root.join("application-backups-v1");
    fs::create_dir_all(&root).unwrap();
    let file = root.join(name);
    fs::write(&file, b"partial backup").unwrap();
    assert!(cleanup::preview(&f.manager, &f.storage, &f.project_id, &review_id).is_err());
    staging::abandon(&f.manager, &mut f.storage, &f.project_id, &review_id, &pending, "req_cleanup_abandon").unwrap();
    let first = cleanup::preview(&f.manager, &f.storage, &f.project_id, &review_id).unwrap();
    fs::write(&file, b"changed partial backup").unwrap();
    assert!(cleanup::remove(&f.manager, &mut f.storage, &f.project_id, &review_id, first["fingerprint"].as_str().unwrap(), "req_cleanup_stale").is_err());
    assert!(file.exists());
    let fresh = cleanup::preview(&f.manager, &f.storage, &f.project_id, &review_id).unwrap();
    let public = f.manager.inspect_backup_cleanup(&f.storage, &f.project_id, &review_id).unwrap();
    assert_eq!(public.result["enabled"], false);
    assert!(f.manager.cleanup_backup(&mut f.storage, &f.project_id, &review_id, fresh["fingerprint"].as_str().unwrap(), "req_product_cleanup_disabled").is_err());
    assert!(file.exists());
    cleanup::remove(&f.manager, &mut f.storage, &f.project_id, &review_id, fresh["fingerprint"].as_str().unwrap(), "req_cleanup").unwrap();
    assert!(!file.exists());
    let done = cleanup::preview(&f.manager, &f.storage, &f.project_id, &review_id).unwrap();
    assert_eq!(done["state"], "deleted");
    assert_eq!(done["filePresent"], false);
    assert_eq!(staging::inspect(&f.manager, &f.storage, &f.project_id, &review_id).unwrap()["state"], "abandoned");
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(target_os = "macos")]
#[test]
fn completed_current_backup_stays_protected_until_explicit_retirement() {
    use application::review::{confirmation::Confirmations, intent, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    // Exercise the approved macOS default through public manager methods.
    assert!(f.manager.application_cleanup_enabled);
    let mut owner = Confirmations::default();
    let (token, _) = owner.open(&f.manager, &f.storage, ReviewTarget::Project(&f.project_id), &review_id).unwrap();
    let grant = owner.prepare_backup(&f.manager, &mut f.storage, &token, "req_cleanup_ready").unwrap();
    let prepared = intent::begin(&f.manager, &mut f.storage, &mut owner, grant, "req_cleanup_prepare").unwrap();
    let id = prepared["applicationId"].as_str().unwrap();
    let terminal = intent::abandon_unwritten(&mut f.storage, &f.project_id, &prepared, "req_cleanup_terminal").unwrap();
    let before = required_project(&f.storage, &f.project_id).unwrap();
    let archive = f.manager.data_root.join("application-backups-v1")
        .join(before["application_backups"][&review_id]["name"].as_str().unwrap());
    let bytes = fs::read(&archive).unwrap();
    let lock_id = terminal["lockId"].as_str().unwrap();
    let lock = f.storage.projection_record("lock", lock_id).unwrap();
    // A terminal current application is still a restoration target.
    assert!(f.manager.inspect_backup_cleanup(&f.storage, &f.project_id, &review_id).is_err());
    assert!(f.manager.cleanup_backup(&mut f.storage, &f.project_id, &review_id, "unapproved", "req_cleanup_current").is_err());
    assert_eq!(fs::read(&archive).unwrap(), bytes);
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before);

    let retirement = f.manager.inspect_application_retirement(&f.storage, &f.project_id, id).unwrap().result;
    assert_eq!(retirement["enabled"], true);
    let retirement_fingerprint = retirement["inspection"]["fingerprint"].as_str().unwrap();
    assert!(f.manager.retire_application(&mut f.storage, &f.project_id, id, "stale", "req_retire_stale").is_err());
    f.manager.preparation_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
    assert!(f.manager.retire_application(&mut f.storage, &f.project_id, id, retirement_fingerprint, "req_retire_busy").is_err());
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before);
    f.manager.preparation_cancel = None;
    f.manager.retire_application(&mut f.storage, &f.project_id, id, retirement_fingerprint, "req_cleanup_retire").unwrap();
    assert_eq!(fs::read(&archive).unwrap(), bytes);
    let preview = f.manager.inspect_backup_cleanup(&f.storage, &f.project_id, &review_id).unwrap().result;
    assert_eq!(preview["enabled"], true);
    let fingerprint = preview["inspection"]["fingerprint"].as_str().unwrap();
    assert!(f.manager.cleanup_backup(&mut f.storage, &f.project_id, &review_id, "stale", "req_cleanup_stale").is_err());
    f.manager.preparation_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
    assert!(f.manager.cleanup_backup(&mut f.storage, &f.project_id, &review_id, fingerprint, "req_cleanup_busy").is_err());
    assert_eq!(fs::read(&archive).unwrap(), bytes);
    f.manager.preparation_cancel = None;
    f.manager.cleanup_backup(&mut f.storage, &f.project_id, &review_id, fingerprint, "req_cleanup_history").unwrap();
    assert!(!archive.exists());
    let after = required_project(&f.storage, &f.project_id).unwrap();
    assert_eq!(after["application_history"][id], terminal);
    assert_eq!(f.storage.projection_record("lock", lock_id).unwrap(), lock);
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    let deleted = f.manager.inspect_backup_cleanup(&f.storage, &f.project_id, &review_id).unwrap().result["inspection"].clone();
    assert_eq!(deleted["state"], "deleted");
    // A new file at the old archive name must not be consumed by a retry.
    fs::write(&archive, b"replacement must survive").unwrap();
    assert!(f.manager.cleanup_backup(&mut f.storage, &f.project_id, &review_id, deleted["fingerprint"].as_str().unwrap(), "req_cleanup_retry").is_err());
    assert_eq!(fs::read(&archive).unwrap(), b"replacement must survive");
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), after);
}

#[cfg(unix)]
#[test]
fn preparation_cancel_reaches_index_and_bound_receipt_without_mutation() {
    use application::git_baseline::{capture_index_baseline, raw_baseline::{capture_clean_baseline, receipt}};
    let (mut f, _) = prepared_backup_fixture();
    let active = f.manager.validate_active_project(&f.storage, &f.project_id).unwrap();
    let source = f.manager.open_source_repository(&active.canonical_git_root).unwrap();
    let baseline = capture_clean_baseline(&f.manager, &source).unwrap();
    let before = required_project(&f.storage, &f.project_id).unwrap();
    f.manager.preparation_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
    assert!(capture_index_baseline(&f.manager, &source).is_err());
    assert!(receipt::verify_bound(&f.manager, &f.storage, &active, &baseline).is_err());
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before);
    f.manager.preparation_cancel = None;
    assert!(capture_index_baseline(&f.manager, &source).is_ok());
    assert!(receipt::verify_bound(&f.manager, &f.storage, &active, &baseline).is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn backup_cleanup_interruption_requires_explicit_fresh_resume_after_reopen() {
    use application::review::{backup::{cleanup, staging}, ReviewTarget};
    for point in [cleanup::FailurePoint::Pending, cleanup::FailurePoint::Unlinked] {
        for replacement in [false, true] {
            let (mut f, review_id) = prepared_backup_fixture();
            f.manager.application_cleanup_enabled = true;
            assert!(staging::stop_after_reservation_for_test(&f.manager, &mut f.storage, ReviewTarget::Project(&f.project_id), &review_id).is_err());
            let pending = staging::inspect(&f.manager, &f.storage, &f.project_id, &review_id).unwrap();
            let archive_root = f.manager.data_root.join("application-backups-v1");
            fs::create_dir_all(&archive_root).unwrap();
            let archive = archive_root.join(pending["name"].as_str().unwrap());
            fs::write(&archive, b"partial archive").unwrap();
            staging::abandon(&f.manager, &mut f.storage, &f.project_id, &review_id, &pending, "req_interruption_abandon").unwrap();
            let preview = f.manager.inspect_backup_cleanup(&f.storage, &f.project_id, &review_id).unwrap().result;
            let old = preview["inspection"]["fingerprint"].as_str().unwrap();
            cleanup::fail_once_at(point);
            let interrupted = f.manager.cleanup_backup(&mut f.storage, &f.project_id, &review_id, old, "req_interrupted_cleanup").err().expect("injected interruption must fail");
            assert_eq!(interrupted.detail, "injected cleanup interruption", "cleanup failed before the intended interruption point");
            assert_eq!(archive.exists(), point == cleanup::FailurePoint::Pending);
            let before = required_project(&f.storage, &f.project_id).unwrap();
            assert_eq!(before["application_backups"][&review_id]["cleanup"]["state"], "pending");
            let database = f._root.path().join("app-data/hello-agent.sqlite3");
            // Simulated process interruption: destroy both owners and reopen the
            // durable database. No test hook is present in production builds.
            let Fixture { _root, source, storage, manager, project_id } = f;
            drop(manager); drop(storage);
            if replacement { fs::write(&archive, b"replacement survives recovery").unwrap(); }
            let mut storage = Storage::open(&database).unwrap();
            let mut manager = ProjectManager::new(&database).unwrap();
            manager.application_cleanup_enabled = true;
            assert_eq!(required_project(&storage, &project_id).unwrap(), before);
            assert_eq!(archive.exists(), replacement || point == cleanup::FailurePoint::Pending);
            if replacement {
                assert!(manager.inspect_backup_cleanup(&storage, &project_id, &review_id).is_err());
                assert!(manager.cleanup_backup(&mut storage, &project_id, &review_id, old, "req_replacement_cleanup").is_err());
                assert_eq!(fs::read(&archive).unwrap(), b"replacement survives recovery");
                assert_eq!(required_project(&storage, &project_id).unwrap(), before);
            } else {
                let fresh = manager.inspect_backup_cleanup(&storage, &project_id, &review_id).unwrap().result;
                assert_eq!(fresh["inspection"]["state"], "pending");
                assert_eq!(required_project(&storage, &project_id).unwrap(), before);
                assert!(manager.cleanup_backup(&mut storage, &project_id, &review_id, old, "req_old_cleanup").is_err());
                manager.cleanup_backup(&mut storage, &project_id, &review_id, fresh["inspection"]["fingerprint"].as_str().unwrap(), "req_explicit_resume").unwrap();
                assert!(!archive.exists());
                assert_eq!(manager.inspect_backup_cleanup(&storage, &project_id, &review_id).unwrap().result["inspection"]["state"], "deleted");
            }
            assert_eq!(fs::read(source.join("src/edit.txt")).unwrap(), b"hello");
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn application_poll_conflict_exposes_bounded_failure_without_unlocking() {
    use application::review::{confirmation::Confirmations, ReviewTarget};
    let (mut f, review_id) = prepared_edit_fixture();
    let mut confirmations = Confirmations::default();
    let (token, _) = confirmations.open(&f.manager, &f.storage, ReviewTarget::Project(&f.project_id), &review_id).unwrap();
    f.manager.application_confirmations = confirmations;
    f.manager.application_cancel.begin(&f.project_id, &token);
    f.manager.start_confirmed_application(&mut f.storage, &token, "req_failure_start").unwrap();
    f.manager.poll_application(&mut f.storage);
    fs::write(f.source.join("src/edit.txt"), b"external edit").unwrap();
    f.manager.poll_application(&mut f.storage);
    assert!(!f.manager.application_running());
    let status = f.manager.application_status(&f.storage, &f.project_id).unwrap();
    assert_eq!(status.result["status"]["running"], false);
    assert!(status.result["status"]["failure"]["code"].as_str().unwrap().starts_with("E_"));
    assert!(status.result["status"]["failure"]["detail"].as_str().unwrap().chars().count() <= 2048);
    assert!(!f.storage.blocker_ids().unwrap().is_empty());
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"external edit");
}

#[cfg(unix)]
#[test]
fn backup_preparation_catalog_abandons_only_the_reviewed_pending_record() {
    use application::review::{backup::staging, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    assert!(staging::stop_after_reservation_for_test(&f.manager, &mut f.storage, ReviewTarget::Project(&f.project_id), &review_id).is_err());
    let before = required_project(&f.storage, &f.project_id).unwrap();
    let listed = f.manager.backup_preparations(&f.storage, &f.project_id).unwrap();
    assert_eq!(listed.result["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listed.result["entries"][0]["reviewId"], review_id);
    let fingerprint = listed.result["entries"][0]["fingerprint"].as_str().unwrap();
    assert!(f.manager.abandon_backup_preparation(&mut f.storage, &f.project_id, &review_id, "stale", "req_bad_backup_end").is_err());
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before);
    f.manager.abandon_backup_preparation(&mut f.storage, &f.project_id, &review_id, fingerprint, "req_backup_end").unwrap();
    assert_eq!(staging::inspect(&f.manager, &f.storage, &f.project_id, &review_id).unwrap()["state"], "abandoned");
    assert!(f.manager.backup_preparations(&f.storage, &f.project_id).unwrap().result["entries"].as_array().unwrap().is_empty());
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(target_os = "macos")]
#[test]
fn application_canceled_before_start_never_reserves_backup_or_resets_token() {
    use application::review::{confirmation::Confirmations, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    let mut confirmations = Confirmations::default();
    let (token, _) = confirmations.open(&f.manager, &f.storage, ReviewTarget::Project(&f.project_id), &review_id).unwrap();
    f.manager.application_confirmations = confirmations;
    f.manager.application_cancel.begin(&f.project_id, &token);
    let before = required_project(&f.storage, &f.project_id).unwrap();
    assert!(f.manager.application_cancel.cancel_confirmation(&token));
    assert!(f.manager.start_confirmed_application(&mut f.storage, &token, "req_early_cancel").is_err());
    assert!(!f.manager.application_running());
    assert!(f.manager.preparation_cancel.is_none());
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before);
    assert!(f.manager.start_confirmed_application(&mut f.storage, &token, "req_early_retry").is_err());
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(target_os = "macos")]
#[test]
fn application_scheduler_cancel_keeps_recovery_and_rejects_wrong_identity() {
    use application::review::{confirmation::Confirmations, ReviewTarget};
    let (mut f, review_id) = prepared_edit_fixture();
    let mut confirmations = Confirmations::default();
    let (token, _) = confirmations.open(&f.manager, &f.storage, ReviewTarget::Project(&f.project_id), &review_id).unwrap();
    f.manager.application_confirmations = confirmations;
    f.manager.application_cancel.begin(&f.project_id, &token);
    let record = f.manager.start_confirmed_application(&mut f.storage, &token, "req_scheduler_start").unwrap();
    assert!(f.manager.application_running());
    assert!(f.manager.cancel_application(&f.project_id, "application_wrong").is_err());
    assert!(f.manager.application_running());
    f.manager.poll_application(&mut f.storage);
    f.manager.cancel_application(&f.project_id, record["applicationId"].as_str().unwrap()).unwrap();
    assert!(!f.manager.application_running());
    f.manager.poll_application(&mut f.storage);
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    assert!(!f.storage.blocker_ids().unwrap().is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn restoration_owner_cancellation_preserves_record_and_requires_new_admission() {
    use application::review::{confirmation::Confirmations, intent, ReviewTarget};
    let (mut f, review_id) = prepared_edit_fixture();
    assert_eq!(f.manager.application_capabilities().result["enabled"], true);
    // Exercise the disabled guard explicitly; macOS production is enabled.
    f.manager.source_writes_enabled = false;
    assert_eq!(f.manager.application_capabilities().result["enabled"], false);
    assert!(f.manager.start_application_request(&mut f.storage, &f.project_id, "chat_unknown", "confirmation_unknown", "req_blocked_start").is_err());
    assert!(f.manager.restore_application_request(&f.storage, &f.project_id, "application_unknown", "unknown", "req_blocked_restore").is_err());
    f.manager.source_writes_enabled = true;
    let mut confirmations = Confirmations::default();
    let (token, _) = confirmations.open(&f.manager, &f.storage, ReviewTarget::Project(&f.project_id), &review_id).unwrap();
    f.manager.application_confirmations = confirmations;
    f.manager.application_cancel.begin(&f.project_id, &token);
    let record = f.manager.start_confirmed_application(&mut f.storage, &token, "req_restore_fixture").unwrap();
    f.manager.stop_application();
    let preview = f.manager.restoration_preview(&f.storage, &f.project_id, record["applicationId"].as_str().unwrap(), 0, None).unwrap();
    assert_eq!(preview.result["state"], "prepared");
    assert!(preview.result["page"]["total"].as_u64().unwrap() > 0);
    assert!(f.manager.restoration_preview(&f.storage, &f.project_id, "application_other", 0, None).is_err());
    assert!(f.manager.restoration_preview(&f.storage, &f.project_id, record["applicationId"].as_str().unwrap(), 0, Some("changed")).is_err());
    assert_eq!(intent::inspect(&f.storage, &f.project_id).unwrap(), record);
    let mut owner = intent::execution::RestoringBatch::start(&f.storage, &f.project_id, &record, "req_restore_cancel").unwrap();
    let canceled = std::sync::atomic::AtomicBool::new(true);
    assert!(owner.advance(&f.manager, &mut f.storage, &canceled).unwrap());
    canceled.store(false, std::sync::atomic::Ordering::Release);
    assert!(owner.advance(&f.manager, &mut f.storage, &canceled).is_err());
    assert_eq!(intent::inspect(&f.storage, &f.project_id).unwrap(), record);
    f.manager.start_selected_restoration(&f.storage, &f.project_id, &record, "req_restore_new").unwrap();
    assert!(f.manager.application_running());
    f.manager.poll_application(&mut f.storage);
    assert!(!f.manager.application_running());
    assert_eq!(intent::inspect(&f.storage, &f.project_id).unwrap()["state"], "abandoned");
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(target_os = "macos")]
#[test]
fn application_owner_cancellation_is_terminal_without_source_writes() {
    use application::review::{confirmation::Confirmations, intent, ReviewTarget};
    let (mut f, review_id) = prepared_edit_fixture();
    let mut confirmations = Confirmations::default();
    let (token, _) = confirmations
        .open(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            &review_id,
        )
        .unwrap();
    let prepared = confirmations
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_owner_backup")
        .unwrap();
    let mut owner = intent::execution::ApplyingBatch::start(
        &f.manager,
        &mut f.storage,
        &mut confirmations,
        prepared,
        "req_owner_start",
    )
    .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    assert!(!owner.advance(&f.manager, &mut f.storage, &cancel).unwrap());
    assert_eq!(owner.record()["state"], "applying");
    assert!(owner.record()["pendingItem"].get("writeJournal").is_none());
    cancel.store(true, std::sync::atomic::Ordering::Release);
    assert!(owner.advance(&f.manager, &mut f.storage, &cancel).unwrap());
    cancel.store(false, std::sync::atomic::Ordering::Release);
    assert!(owner.advance(&f.manager, &mut f.storage, &cancel).is_err());
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    assert!(!f.storage.blocker_ids().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn explicit_application_workspace_records_baseline_and_rejects_dirty_before_reservation() {
    let (mut f, _) = prepared_backup_fixture();
    let previous = required_project(&f.storage, &f.project_id).unwrap();
    let result = f
        .manager
        .prepare_application_workspace(&mut f.storage, &f.project_id, "req_public_baseline")
        .unwrap();
    let worktree = result.result["worktreeId"].as_str().unwrap();
    assert_ne!(previous["worktree_id"], worktree);
    let current = required_project(&f.storage, &f.project_id).unwrap();
    assert!(current["source_baselines"].get(worktree).is_some());
    assert!(current["source_baselines"]
        .get(previous["worktree_id"].as_str().unwrap())
        .is_some());
    assert_eq!(
        f.manager
            .application_preview(&f.storage, &f.project_id)
            .unwrap()
            .result["preview"]["total"],
        0
    );
    fs::write(f.source.join("src/edit.txt"), b"user changes").unwrap();
    assert!(f
        .manager
        .prepare_application_workspace(&mut f.storage, &f.project_id, "req_dirty_baseline")
        .is_err());
    assert_eq!(
        required_project(&f.storage, &f.project_id).unwrap(),
        current
    );
    assert!(f.storage.blocker_ids().unwrap().is_empty());
    assert_eq!(
        fs::read(f.source.join("src/edit.txt")).unwrap(),
        b"user changes"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn application_cancels_before_first_write_with_or_without_pending_item() {
    use application::review::{confirmation::Confirmations, intent, persistence, ReviewTarget};
    for with_pending in [false, true] {
        let (mut f, _) = prepared_backup_fixture();
        let active = f
            .manager
            .validate_active_project(&f.storage, &f.project_id)
            .unwrap();
        let before = fs::read(f.source.join("src/edit.txt")).unwrap();
        fs::write(active.worktree_path.join("src/edit.txt"), b"never-applied").unwrap();
        let review = persistence::publish(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            "req_unwritten_review",
        )
        .unwrap();
        let mut owner = Confirmations::default();
        let (token, _) = owner
            .open(
                &f.manager,
                &f.storage,
                ReviewTarget::Project(&f.project_id),
                review["reviewId"].as_str().unwrap(),
            )
            .unwrap();
        let prepared = owner
            .prepare_backup(&f.manager, &mut f.storage, &token, "req_unwritten_backup")
            .unwrap();
        let record = intent::begin(
            &f.manager,
            &mut f.storage,
            &mut owner,
            prepared,
            "req_unwritten_begin",
        )
        .unwrap();
        let mut record = intent::mark_write_started(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &record,
            "req_unwritten_fence",
        )
        .unwrap();
        if with_pending {
            record = intent::record_first_item(
                &f.manager,
                &mut f.storage,
                &f.project_id,
                &record,
                "req_unwritten_pending",
            )
            .unwrap();
        }
        let result = intent::execution::restore_selected(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &record,
            &std::sync::atomic::AtomicBool::new(false),
            "req_unwritten_cancel",
        )
        .unwrap();
        assert_eq!(result["state"], "abandoned");
        assert_eq!(intent::inspect(&f.storage, &f.project_id).unwrap(), result);
        assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), before);
        assert_eq!(
            f.storage
                .projection_record("lock", result["lockId"].as_str().unwrap())
                .unwrap()
                .unwrap()["recovery_required"],
            false
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn application_confirmed_execution_and_selected_restore_roundtrip() {
    use application::review::{confirmation::Confirmations, intent, persistence, ReviewTarget};
    let (mut f, _) = prepared_backup_fixture();
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    let before = fs::read(f.source.join("src/edit.txt")).unwrap();
    fs::write(
        active.worktree_path.join("src/edit.txt"),
        b"orchestrated-change",
    )
    .unwrap();
    fs::write(
        active.worktree_path.join("src/new.txt"),
        b"orchestrated-new",
    )
    .unwrap();
    fs::create_dir_all(active.worktree_path.join("new-folder/nested")).unwrap();
    fs::write(
        active.worktree_path.join("new-folder/nested/file.txt"),
        b"nested-new",
    )
    .unwrap();
    let review = persistence::publish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        "req_execution_review",
    )
    .unwrap();
    let mut owner = Confirmations::default();
    let (token, _) = owner
        .open(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            review["reviewId"].as_str().unwrap(),
        )
        .unwrap();
    let prepared = owner
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_execution_backup")
        .unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let applied = intent::execution::apply_confirmed(
        &f.manager,
        &mut f.storage,
        &mut owner,
        prepared,
        &cancel,
        "req_execution_apply",
    )
    .unwrap();
    assert_eq!(applied["state"], "applied");
    assert_eq!(
        fs::read(f.source.join("new-folder/nested/file.txt")).unwrap(),
        b"nested-new"
    );
    fs::write(f.source.join("new-folder/foreign.txt"), b"external").unwrap();
    assert!(intent::execution::restore_selected(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &applied,
        &cancel,
        "req_foreign_directory"
    )
    .is_err());
    assert_eq!(intent::inspect(&f.storage, &f.project_id).unwrap(), applied);
    fs::rename(
        f.source.join("new-folder/foreign.txt"),
        active.worktree_path.join("foreign-preserved.txt"),
    )
    .unwrap();
    assert_eq!(
        fs::read(f.source.join("src/new.txt")).unwrap(),
        b"orchestrated-new"
    );
    let stopped = std::sync::atomic::AtomicBool::new(true);
    assert_eq!(
        intent::execution::restore_selected(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &applied,
            &stopped,
            "req_execution_cancel"
        )
        .unwrap(),
        applied
    );
    let restored = intent::execution::restore_selected(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &applied,
        &cancel,
        "req_execution_restore",
    )
    .unwrap();
    assert_eq!(restored["state"], "restored");
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), before);
    assert!(!f.source.join("src/new.txt").exists());
    assert!(!f.source.join("new-folder").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn application_link_roundtrip_never_changes_target_file() {
    use application::review::{confirmation::Confirmations, intent, persistence, ReviewTarget};
    let (mut f, _) = prepared_backup_fixture();
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    let target = f.source.join("src/edit.txt");
    let bytes = fs::read(&target).unwrap();
    std::os::unix::fs::symlink(&target, active.worktree_path.join("src/link.txt")).unwrap();
    let review =
        persistence::publish(&f.manager, &mut f.storage, &f.project_id, "req_link_review").unwrap();
    let mut owner = Confirmations::default();
    let (token, _) = owner
        .open(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            review["reviewId"].as_str().unwrap(),
        )
        .unwrap();
    let prepared = owner
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_link_backup")
        .unwrap();
    let record = intent::begin(
        &f.manager,
        &mut f.storage,
        &mut owner,
        prepared,
        "req_link_begin",
    )
    .unwrap();
    let record = intent::mark_write_started(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &record,
        "req_link_start",
    )
    .unwrap();
    let record = intent::record_first_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &record,
        "req_link_pending",
    )
    .unwrap();
    let applied = intent::writer::write_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &record,
        "req_link_apply",
    )
    .unwrap();
    assert_eq!(
        fs::read_link(f.source.join("src/link.txt")).unwrap(),
        target
    );
    assert_eq!(fs::read(&target).unwrap(), bytes);
    let restored = intent::writer::restore_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &applied,
        "req_link_restore",
    )
    .unwrap();
    assert!(fs::symlink_metadata(f.source.join("src/link.txt")).is_err());
    let retained = restored["pendingItem"]["writeJournal"]["retainedName"]
        .as_str()
        .unwrap();
    assert_eq!(
        fs::read_link(f.source.join("src").join(retained)).unwrap(),
        target
    );
    assert_eq!(fs::read(&target).unwrap(), bytes);
}

#[cfg(target_os = "macos")]
#[test]
fn application_advances_two_files_without_losing_prior_journal() {
    application_two_file_roundtrip(false, None);
}

#[cfg(target_os = "macos")]
#[test]
fn application_cancels_unstarted_tail_and_restores_only_written_prefix() {
    application_two_file_roundtrip(true, None);
}

#[cfg(target_os = "macos")]
#[test]
fn application_recovers_lost_applied_record_without_writing_remaining_files() {
    application_two_file_roundtrip(false, Some("applied"));
}

#[cfg(target_os = "macos")]
#[test]
fn application_preserves_partial_staging_when_explicitly_canceled() {
    application_two_file_roundtrip(false, Some("staged"));
}

#[cfg(target_os = "macos")]
fn application_two_file_roundtrip(cancel_tail: bool, lost_phase: Option<&str>) {
    use application::review::{confirmation::Confirmations, intent, persistence, ReviewTarget};
    let (mut f, _) = prepared_backup_fixture();
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    fs::write(active.worktree_path.join("src/edit.txt"), b"first-update").unwrap();
    fs::write(active.worktree_path.join("src/new.txt"), b"second-addition").unwrap();
    let review = persistence::publish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        "req_multi_review",
    )
    .unwrap();
    let mut owner = Confirmations::default();
    let (token, _) = owner
        .open(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            review["reviewId"].as_str().unwrap(),
        )
        .unwrap();
    let prepared = owner
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_multi_backup")
        .unwrap();
    let start = intent::begin(
        &f.manager,
        &mut f.storage,
        &mut owner,
        prepared,
        "req_multi_begin",
    )
    .unwrap();
    let start = intent::mark_write_started(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &start,
        "req_multi_start",
    )
    .unwrap();
    let first = intent::record_first_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &start,
        "req_multi_first",
    )
    .unwrap();
    assert!(intent::advance_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &first,
        "req_multi_early"
    )
    .is_err());
    if let Some(phase) = lost_phase {
        assert!(matches!(phase, "applied" | "staged"));
        f.storage.connection_for_test().execute_batch(&format!("CREATE TRIGGER fail_applied BEFORE INSERT ON events WHEN NEW.operation_id LIKE '%_item_0_{phase}' BEGIN SELECT RAISE(ABORT, 'lost writer record'); END;")).unwrap();
        assert!(intent::writer::write_first_regular_item(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &first,
            "req_lost_applied",
        )
        .is_err());
        f.storage
            .connection_for_test()
            .execute_batch("DROP TRIGGER fail_applied;")
            .unwrap();
        let interrupted = intent::inspect(&f.storage, &f.project_id).unwrap();
        assert_eq!(
            interrupted["pendingItem"]["writeJournal"]["phase"],
            if phase == "staged" {
                "reserved"
            } else {
                "staged"
            }
        );
        assert_eq!(
            fs::read(f.source.join("src/edit.txt")).unwrap(),
            if phase == "staged" {
                b"hello".as_slice()
            } else {
                b"first-update".as_slice()
            }
        );
        let retained = f.source.join("src").join(
            interrupted["pendingItem"]["writeJournal"]["retainedName"]
                .as_str()
                .unwrap(),
        );
        if phase == "staged" {
            fs::write(&retained, b"partial").unwrap();
            fs::write(f.source.join("src/edit.txt"), b"external-edit").unwrap();
            assert!(intent::execution::restore_selected(
                &f.manager,
                &mut f.storage,
                &f.project_id,
                &interrupted,
                &std::sync::atomic::AtomicBool::new(false),
                "req_partial_conflict"
            )
            .is_err());
            assert_eq!(
                intent::inspect(&f.storage, &f.project_id).unwrap(),
                interrupted
            );
            fs::write(f.source.join("src/edit.txt"), b"hello").unwrap();
        }
        let restored = intent::execution::restore_selected(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &interrupted,
            &std::sync::atomic::AtomicBool::new(false),
            "req_recover_lost_applied",
        )
        .unwrap();
        assert_eq!(restored["state"], "restored");
        assert_eq!(restored["unstartedFrom"], 1);
        assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
        assert!(!f.source.join("src/new.txt").exists());
        if phase == "staged" {
            assert_eq!(
                restored["pendingItem"]["writeJournal"]["canceledStaging"],
                true
            );
            assert_eq!(fs::read(retained).unwrap(), b"partial");
        }
        return;
    }
    let first = intent::writer::write_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &first,
        "req_multi_write_first",
    )
    .unwrap();
    let second = intent::advance_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &first,
        "req_multi_next",
    )
    .unwrap();
    assert_eq!(second["completedItems"][0], first["pendingItem"]);
    assert_eq!(second["pendingItem"]["index"], 1);
    assert!(intent::advance_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &first,
        "req_multi_stale"
    )
    .is_err());
    if cancel_tail {
        let cancel = intent::cancel_unstarted_tail(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &second,
            "req_cancel_tail",
        )
        .unwrap();
        assert_eq!(cancel["unstartedFrom"], 1);
        assert!(!f.source.join("src/new.txt").exists());
        assert!(intent::advance_regular_item(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &cancel,
            "req_cancel_no_forward"
        )
        .is_err());
        let restored = intent::writer::restore_first_regular_item(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &cancel,
            "req_cancel_restore",
        )
        .unwrap();
        let finished = intent::completion::finish(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            &restored,
            true,
            "req_cancel_finish",
        )
        .unwrap();
        assert_eq!(finished["state"], "restored");
        assert_eq!(finished["unstartedFrom"], 1);
        assert!(!f.source.join("src/new.txt").exists());
        return;
    }
    let second = intent::writer::write_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &second,
        "req_multi_write_second",
    )
    .unwrap();
    assert_eq!(
        fs::read(f.source.join("src/edit.txt")).unwrap(),
        b"first-update"
    );
    assert_eq!(
        fs::read(f.source.join("src/new.txt")).unwrap(),
        b"second-addition"
    );
    assert_eq!(second["completedItems"][0], first["pendingItem"]);
    assert!(intent::advance_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &second,
        "req_multi_past_end"
    )
    .is_err());
    assert_eq!(
        intent::inspect_pending_item(&f.manager, &f.storage, &f.project_id).unwrap(),
        second
    );
    let second = intent::completion::finish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &second,
        false,
        "req_multi_finish",
    )
    .unwrap();
    assert_eq!(second["state"], "applied");
    assert_eq!(
        f.storage
            .projection_record("lock", second["lockId"].as_str().unwrap())
            .unwrap()
            .unwrap()["recovery_required"],
        false
    );
    assert!(intent::writer::restore_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &second,
        "req_multi_restore_without_lock"
    )
    .is_err());
    let before_restore = required_project(&f.storage, &f.project_id).unwrap();
    application::review::backup::space::with_free_space(SNAPSHOT_DISK_RESERVE_BYTES_V1 + 64 * 1024 - 1, || {
        assert!(intent::completion::begin_restore(
            &f.manager, &mut f.storage, &f.project_id, &second, "req_restore_no_space"
        ).is_err());
    });
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before_restore);
    assert_eq!(fs::read(f.source.join("src/new.txt")).unwrap(), b"second-addition");
    let second = application::review::backup::space::with_free_space(
        SNAPSHOT_DISK_RESERVE_BYTES_V1 + 64 * 1024,
        || intent::completion::begin_restore(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &second,
        "req_multi_relock",
    ))
    .unwrap();
    assert_eq!(fs::read(f.source.join("src/new.txt")).unwrap(), b"second-addition");
    assert_eq!(second["state"], "applying");
    assert!(intent::previous_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &second,
        "req_multi_early_back"
    )
    .is_err());
    let restored_second = intent::writer::restore_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &second,
        "req_multi_restore_second",
    )
    .unwrap();
    assert!(!f.source.join("src/new.txt").exists());
    let previous = intent::previous_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &restored_second,
        "req_multi_previous",
    )
    .unwrap();
    assert_eq!(previous["pendingItem"], first["pendingItem"]);
    assert_eq!(previous["restoredItems"][0], restored_second["pendingItem"]);
    assert_eq!(previous["completedItems"], json!([]));
    assert!(intent::advance_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &previous,
        "req_multi_no_forward"
    )
    .is_err());
    let restored_first = intent::writer::restore_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &previous,
        "req_multi_restore_first",
    )
    .unwrap();
    assert_eq!(restored_first["restoredItems"], previous["restoredItems"]);
    assert_eq!(
        intent::writer::inspect_regular_outcome(&f.manager, &f.storage, &f.project_id).unwrap()
            ["comparison"],
        "matchesUnappliedPair"
    );
    assert!(intent::previous_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &restored_first,
        "req_multi_before_start"
    )
    .is_err());
    let finished = intent::completion::finish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &restored_first,
        true,
        "req_multi_restored_finish",
    )
    .unwrap();
    assert_eq!(finished["state"], "restored");
    assert_eq!(
        f.storage
            .projection_record("lock", finished["lockId"].as_str().unwrap())
            .unwrap()
            .unwrap()["recovery_required"],
        false
    );
    assert!(intent::completion::begin_restore(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &finished,
        "req_multi_restore_twice"
    )
    .is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn application_regular_writer_retains_previous_file_and_refuses_replay() {
    use application::review::{confirmation::Confirmations, intent, persistence, ReviewTarget};
    let (mut f, _) = prepared_backup_fixture();
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    let before = fs::read(f.source.join("src/edit.txt")).unwrap();
    fs::write(
        active.worktree_path.join("src/edit.txt"),
        b"applied-content",
    )
    .unwrap();
    let review = persistence::publish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        "req_writer_review",
    )
    .unwrap();
    let mut confirmations = Confirmations::default();
    let (token, _) = confirmations
        .open(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            review["reviewId"].as_str().unwrap(),
        )
        .unwrap();
    let prepared = confirmations
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_writer_backup")
        .unwrap();
    let intent = intent::begin(
        &f.manager,
        &mut f.storage,
        &mut confirmations,
        prepared,
        "req_writer_begin",
    )
    .unwrap();
    let intent = intent::mark_write_started(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &intent,
        "req_writer_start",
    )
    .unwrap();
    let intent = intent::record_first_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &intent,
        "req_writer_pending",
    )
    .unwrap();
    let result = intent::writer::write_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &intent,
        "req_writer_apply",
    )
    .unwrap();
    let journal = &result["pendingItem"]["writeJournal"];
    assert_eq!(journal["phase"], "applied");
    assert_eq!(
        fs::read(f.source.join("src/edit.txt")).unwrap(),
        b"applied-content"
    );
    assert_eq!(
        fs::read(
            f.source
                .join("src")
                .join(journal["retainedName"].as_str().unwrap())
        )
        .unwrap(),
        before
    );
    assert!(intent::writer::write_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &result,
        "req_writer_replay"
    )
    .is_err());
    assert_eq!(intent::inspect(&f.storage, &f.project_id).unwrap(), result);
    assert!(intent::abandon_unwritten(
        &mut f.storage,
        &f.project_id,
        &result,
        "req_writer_abandon"
    )
    .is_err());
    assert_eq!(
        intent::writer::inspect_regular_outcome(&f.manager, &f.storage, &f.project_id).unwrap()
            ["comparison"],
        "matchesAppliedPair"
    );
    fs::write(f.source.join("src/edit.txt"), b"later-user-edit").unwrap();
    assert!(intent::writer::restore_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &result,
        "req_restore_conflict"
    )
    .is_err());
    assert_eq!(
        fs::read(f.source.join("src/edit.txt")).unwrap(),
        b"later-user-edit"
    );
    assert_eq!(intent::inspect(&f.storage, &f.project_id).unwrap(), result);
    fs::write(f.source.join("src/edit.txt"), b"applied-content").unwrap();
    // Simulate a lost completion publication after the filesystem restore.
    f.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_restore_completion BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_application_%_item_0_restored' BEGIN SELECT RAISE(ABORT, 'restore completion failure'); END;").unwrap();
    assert!(intent::writer::restore_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &result,
        "req_restore"
    )
    .is_err());
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), before);
    let interrupted = intent::inspect(&f.storage, &f.project_id).unwrap();
    assert_eq!(
        interrupted["pendingItem"]["writeJournal"]["phase"],
        "restoring"
    );
    assert_eq!(
        intent::writer::inspect_regular_outcome(&f.manager, &f.storage, &f.project_id).unwrap()
            ["comparison"],
        "matchesUnappliedPair"
    );
    f.storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_restore_completion;")
        .unwrap();
    let restored = intent::writer::acknowledge_regular_outcome(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &interrupted,
        "req_restore_ack",
    )
    .unwrap();
    assert_eq!(restored["pendingItem"]["writeJournal"]["phase"], "restored");
    assert_eq!(
        fs::read(
            f.source
                .join("src")
                .join(journal["retainedName"].as_str().unwrap())
        )
        .unwrap(),
        b"applied-content"
    );
    assert!(intent::writer::restore_first_regular_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &restored,
        "req_restore_replay"
    )
    .is_err());
}

#[cfg(unix)]
fn prepared_backup_fixture() -> (Fixture, String) {
    use application::review::persistence;
    let mut f = source_index_fixture();
    f.manager
        .open_project(
            &mut f.storage,
            f.source.to_str().unwrap(),
            "req_backup_open",
        )
        .unwrap();
    f.manager
        .create_worktree_inner(
            &mut f.storage,
            &f.project_id,
            "req_backup_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    let record = persistence::publish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        "req_backup_review",
    )
    .unwrap();
    (f, record["reviewId"].as_str().unwrap().to_owned())
}

#[cfg(target_os = "macos")]
fn prepared_edit_fixture() -> (Fixture, String) {
    let (mut f, _) = prepared_backup_fixture();
    let active = f.manager.validate_active_project(&f.storage, &f.project_id).unwrap();
    // Execution requires a nonempty reviewed diff; the backup-only fixture
    // deliberately contains no edits and cannot exercise cancellation mid-run.
    fs::write(active.worktree_path.join("src/edit.txt"), b"updated").unwrap();
    let review = application::review::persistence::publish(
        &f.manager, &mut f.storage, &f.project_id, "req_execution_review",
    ).unwrap();
    (f, review["reviewId"].as_str().unwrap().to_owned())
}

#[cfg(unix)]
#[test]
fn applying_item_reader_uses_bound_archive_and_rejects_stale_or_corrupt_records() {
    use application::review::{confirmation::Confirmations, intent, persistence, ReviewTarget};
    let (mut f, _) = prepared_backup_fixture();
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    fs::write(active.worktree_path.join("src/edit.txt"), b"updated").unwrap();
    let review =
        persistence::publish(&f.manager, &mut f.storage, &f.project_id, "req_item_review").unwrap();
    let mut owner = Confirmations::default();
    let (token, _) = owner
        .open(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            review["reviewId"].as_str().unwrap(),
        )
        .unwrap();
    let grant = owner
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_item_backup")
        .unwrap();
    let prepared = intent::begin(
        &f.manager,
        &mut f.storage,
        &mut owner,
        grant,
        "req_item_begin",
    )
    .unwrap();
    assert!(
        intent::inspect_write_item(&f.manager, &f.storage, &f.project_id, &prepared, 0).is_err()
    );
    let applying = intent::mark_write_started(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &prepared,
        "req_item_start",
    )
    .unwrap();
    f.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_pending_item BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_application_%_item_0_pending' BEGIN SELECT RAISE(ABORT, 'test pending item failure'); END;").unwrap();
    assert!(intent::record_first_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &applying,
        "req_pending_fail"
    )
    .is_err());
    assert_eq!(
        intent::inspect(&f.storage, &f.project_id).unwrap(),
        applying
    );
    f.storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_pending_item;")
        .unwrap();
    let without_pending = applying;
    let applying = intent::record_first_item(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &without_pending,
        "req_pending_save",
    )
    .unwrap();
    assert_eq!(applying["pendingItem"]["index"], 0);
    assert_eq!(applying["pendingItem"]["item"]["path"], "src/edit.txt");
    for stale in [&without_pending, &applying] {
        assert!(intent::record_first_item(
            &f.manager,
            &mut f.storage,
            &f.project_id,
            stale,
            "req_pending_replay"
        )
        .is_err());
    }
    let db = f.manager.data_root.join("hello-agent.sqlite3");
    drop(f.storage);
    let storage = Storage::open(&db).unwrap();
    assert_eq!(
        intent::inspect_pending_item(&f.manager, &storage, &f.project_id).unwrap(),
        applying
    );
    let item =
        intent::inspect_write_item(&f.manager, &storage, &f.project_id, &applying, 0).unwrap();
    assert_eq!(item["path"], "src/edit.txt");
    assert_eq!(item["before"]["size"], 5);
    assert_eq!(item["after"]["size"], 7);
    let observe = || intent::observe_pending_source(&f.manager, &storage, &f.project_id).unwrap();
    assert_eq!(observe()["comparison"], "matchesBeforeEntry");
    fs::write(f.source.join("src/edit.txt"), b"updated").unwrap();
    assert_eq!(observe()["comparison"], "matchesAfterImage");
    fs::write(f.source.join("src/edit.txt"), b"external edit").unwrap();
    assert_eq!(observe()["comparison"], "conflict");
    fs::write(f.source.join("src/edit.txt"), b"hello").unwrap();
    let original = f.source.join("src/edit.txt");
    let held = f.source.join("src/original-held.txt");
    fs::rename(&original, &held).unwrap();
    fs::write(&original, b"hello").unwrap();
    assert_eq!(observe()["comparison"], "conflict");
    fs::remove_file(&original).unwrap();
    fs::rename(&held, &original).unwrap();
    fs::rename(f.source.join("src"), f.source.join("held-src")).unwrap();
    std::os::unix::fs::symlink("held-src", f.source.join("src")).unwrap();
    assert_eq!(observe()["comparison"], "conflict");
    fs::remove_file(f.source.join("src")).unwrap();
    fs::rename(f.source.join("held-src"), f.source.join("src")).unwrap();
    assert_eq!(observe()["comparison"], "matchesBeforeEntry");
    assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), applying);
    assert!(intent::inspect_write_item(&f.manager, &storage, &f.project_id, &applying, 1).is_err());
    let mut wrong = applying.clone();
    wrong["backup"]["header"]["count"] = json!(2);
    assert!(intent::inspect_write_item(&f.manager, &storage, &f.project_id, &wrong, 0).is_err());
    let path = f
        .manager
        .data_root
        .join("application-backups-v1")
        .join(applying["backup"]["name"].as_str().unwrap());
    fs::write(path, b"corrupt archive").unwrap();
    assert!(intent::inspect_pending_item(&f.manager, &storage, &f.project_id).is_err());
    assert!(intent::inspect_write_item(&f.manager, &storage, &f.project_id, &applying, 0).is_err());
    assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), applying);
    assert!(!storage.blocker_ids().unwrap().is_empty());
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(unix)]
#[test]
fn application_write_boundary_survives_restart_and_cannot_be_abandoned() {
    use application::review::{confirmation::Confirmations, intent, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    let mut owner = Confirmations::default();
    let (token, _) = owner
        .open(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            &review_id,
        )
        .unwrap();
    let before_backup = required_project(&f.storage, &f.project_id).unwrap();
    application::review::backup::space::with_free_space(0, || {
        assert!(owner.prepare_backup(
            &f.manager, &mut f.storage, &token, "req_backup_no_space"
        ).is_err());
    });
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before_backup);
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    // A failed attempt consumes the confirmation; retry requires fresh consent.
    let (token, _) = owner.open(
        &f.manager, &f.storage, ReviewTarget::Project(&f.project_id), &review_id
    ).unwrap();
    let grant = owner
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_write_backup")
        .unwrap();
    let prepared = intent::begin(
        &f.manager,
        &mut f.storage,
        &mut owner,
        grant,
        "req_write_prepare",
    )
    .unwrap();
    let before_project = required_project(&f.storage, &f.project_id).unwrap();
    let lock_id = prepared["lockId"].as_str().unwrap();
    let before_lock = f.storage.projection_record("lock", lock_id).unwrap();
    let source_before = fs::read(f.source.join("src/edit.txt")).unwrap();
    f.manager.preparation_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
    assert!(intent::mark_write_started(
        &f.manager, &mut f.storage, &f.project_id, &prepared, "req_write_canceled"
    ).is_err());
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before_project);
    assert_eq!(f.storage.projection_record("lock", lock_id).unwrap(), before_lock);
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), source_before);
    f.manager.preparation_cancel = None;
    application::review::backup::space::with_free_space(0, || {
        assert!(intent::mark_write_started(
            &f.manager, &mut f.storage, &f.project_id, &prepared, "req_write_no_space"
        ).is_err());
    });
    assert_eq!(required_project(&f.storage, &f.project_id).unwrap(), before_project);
    assert_eq!(f.storage.projection_record("lock", lock_id).unwrap(), before_lock);
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), source_before);
    let backup_path = f
        .manager
        .data_root
        .join("application-backups-v1")
        .join(prepared["backup"]["name"].as_str().unwrap());
    let held_backup = backup_path.with_extension("held");
    fs::rename(&backup_path, &held_backup).unwrap();
    assert!(intent::mark_write_started(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &prepared,
        "req_write_missing_backup"
    )
    .is_err());
    assert_eq!(
        required_project(&f.storage, &f.project_id).unwrap(),
        before_project
    );
    assert_eq!(
        f.storage.projection_record("lock", lock_id).unwrap(),
        before_lock
    );
    fs::rename(&held_backup, &backup_path).unwrap();
    f.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_writing BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_application_%_writing_lock' BEGIN SELECT RAISE(ABORT, 'test writing failure'); END;").unwrap();
    assert!(intent::mark_write_started(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &prepared,
        "req_write_fail"
    )
    .is_err());
    assert_eq!(
        required_project(&f.storage, &f.project_id).unwrap(),
        before_project
    );
    assert_eq!(
        f.storage.projection_record("lock", lock_id).unwrap(),
        before_lock
    );
    f.storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_writing;")
        .unwrap();
    let applying = application::review::backup::space::with_free_space(
        SNAPSHOT_DISK_RESERVE_BYTES_V1,
        || intent::mark_write_started(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        &prepared,
        "req_write_start",
    ))
    .unwrap();
    assert_eq!(applying["state"], "applying");
    let db = f.manager.data_root.join("hello-agent.sqlite3");
    drop(f.storage);
    let mut storage = Storage::open(&db).unwrap();
    assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), applying);
    assert!(!storage.blocker_ids().unwrap().is_empty());
    assert!(intent::mark_write_started(
        &f.manager,
        &mut storage,
        &f.project_id,
        &prepared,
        "req_write_replay"
    )
    .is_err());
    for expected in [&prepared, &applying] {
        assert!(intent::abandon_unwritten(
            &mut storage,
            &f.project_id,
            expected,
            "req_write_no_abandon"
        )
        .is_err());
    }
    assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), applying);
    assert!(!storage.blocker_ids().unwrap().is_empty());
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(unix)]
#[test]
fn application_intent_fence_is_atomic_restart_safe_and_cancelable_before_writes() {
    use application::review::{confirmation::Confirmations, intent, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    let mut owner = Confirmations::default();
    let target = ReviewTarget::Project(&f.project_id);
    let (token, _) = owner
        .open(&f.manager, &f.storage, target, &review_id)
        .unwrap();
    let grant = owner
        .prepare_backup(&f.manager, &mut f.storage, &token, "req_intent_backup")
        .unwrap();
    let before_project = required_project(&f.storage, &f.project_id).unwrap();
    let lock_id = before_project["lock_id"].as_str().unwrap().to_owned();
    let before_lock = f.storage.projection_record("lock", &lock_id).unwrap();
    f.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_intent_lock BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_application_%_prepare_lock' BEGIN SELECT RAISE(ABORT, 'test intent lock failure'); END;").unwrap();
    assert!(intent::begin(
        &f.manager,
        &mut f.storage,
        &mut owner,
        grant,
        "req_intent_fail"
    )
    .is_err());
    assert_eq!(
        required_project(&f.storage, &f.project_id).unwrap(),
        before_project
    );
    assert_eq!(
        f.storage.projection_record("lock", &lock_id).unwrap(),
        before_lock
    );
    assert!(f.storage.blocker_ids().unwrap().is_empty());
    f.storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_intent_lock;")
        .unwrap();
    let (token, _) = owner
        .open(&f.manager, &f.storage, target, &review_id)
        .unwrap();
    let grant = owner
        .prepare_backup(
            &f.manager,
            &mut f.storage,
            &token,
            "req_intent_retry_backup",
        )
        .unwrap();
    let prepared = intent::begin(
        &f.manager,
        &mut f.storage,
        &mut owner,
        grant,
        "req_intent_begin",
    )
    .unwrap();
    assert_eq!(prepared["state"], "prepared");
    assert!(!f.storage.blocker_ids().unwrap().is_empty());
    assert!(f
        .manager
        .create_worktree(&mut f.storage, &f.project_id, "req_intent_block_create")
        .is_err());
    let db = f.manager.data_root.join("hello-agent.sqlite3");
    drop(f.storage);
    let mut storage = Storage::open(&db).unwrap();
    assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), prepared);
    assert!(!storage.blocker_ids().unwrap().is_empty());
    let mut wrong = prepared.clone();
    wrong["applicationId"] = json!("different");
    assert!(
        intent::abandon_unwritten(&mut storage, &f.project_id, &wrong, "req_intent_wrong").is_err()
    );
    storage.connection_for_test().execute_batch("CREATE TRIGGER fail_intent_abandon BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_application_%_abandon_lock' BEGIN SELECT RAISE(ABORT, 'test intent abandon failure'); END;").unwrap();
    assert!(intent::abandon_unwritten(
        &mut storage,
        &f.project_id,
        &prepared,
        "req_intent_abandon_fail"
    )
    .is_err());
    assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), prepared);
    assert!(!storage.blocker_ids().unwrap().is_empty());
    storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_intent_abandon;")
        .unwrap();
    let abandoned =
        intent::abandon_unwritten(&mut storage, &f.project_id, &prepared, "req_intent_abandon")
            .unwrap();
    assert_eq!(abandoned["state"], "abandoned");
    assert!(storage.blocker_ids().unwrap().is_empty());
    assert_eq!(
        intent::abandon_unwritten(
            &mut storage,
            &f.project_id,
            &prepared,
            "req_intent_abandon_replay"
        )
        .unwrap(),
        abandoned
    );
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    // A fresh review/confirmation is required; archiving never reuses approval.
    for fail_commit in [true, false] {
        let review = application::review::persistence::publish(
            &f.manager,
            &mut storage,
            &f.project_id,
            "req_history_review",
        )
        .unwrap();
        let (token, _) = owner
            .open(
                &f.manager,
                &storage,
                target,
                review["reviewId"].as_str().unwrap(),
            )
            .unwrap();
        let grant = owner
            .prepare_backup(&f.manager, &mut storage, &token, "req_history_backup")
            .unwrap();
        let before = required_project(&storage, &f.project_id).unwrap();
        if fail_commit {
            storage.connection_for_test().execute_batch("CREATE TRIGGER fail_history BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_application_%_prepare_lock' BEGIN SELECT RAISE(ABORT, 'history atomicity'); END;").unwrap();
        }
        let next = intent::begin(
            &f.manager,
            &mut storage,
            &mut owner,
            grant,
            "req_history_begin",
        );
        if fail_commit {
            assert!(next.is_err());
            assert_eq!(required_project(&storage, &f.project_id).unwrap(), before);
            assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), abandoned);
            storage
                .connection_for_test()
                .execute_batch("DROP TRIGGER fail_history;")
                .unwrap();
        } else {
            let next = next.unwrap();
            assert_ne!(next["applicationId"], abandoned["applicationId"]);
            let project = required_project(&storage, &f.project_id).unwrap();
            assert_eq!(
                project["application_history"][abandoned["applicationId"].as_str().unwrap()],
                abandoned
            );
            assert_eq!(
                f.manager
                    .application_history(&storage, &f.project_id)
                    .unwrap()
                    .result,
                json!({"schemaVersion":1,"action":"applicationHistory","projectId":f.project_id,
                    "entries":[{"applicationId":abandoned["applicationId"],"state":"abandoned",
                    "total":abandoned["backup"]["header"]["count"]}]})
            );
            assert_eq!(intent::inspect(&storage, &f.project_id).unwrap(), next);
            assert!(intent::abandon_unwritten(
                &mut storage,
                &f.project_id,
                &abandoned,
                "req_history_stale"
            )
            .is_err());
        }
    }
}

#[cfg(unix)]
#[test]
fn confirmed_backup_handoff_is_single_session_and_rechecks_files() {
    use application::review::{confirmation::Confirmations, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    let target = ReviewTarget::Project(&f.project_id);
    let mut owner = Confirmations::default();
    let issue = |owner: &mut Confirmations, storage: &mut Storage| {
        let (token, _) = owner.open(&f.manager, storage, target, &review_id).unwrap();
        let grant = owner
            .prepare_backup(&f.manager, storage, &token, "req_confirm_backup")
            .unwrap();
        assert!(owner
            .prepare_backup(&f.manager, storage, &token, "req_confirm_backup_replay")
            .is_err());
        grant
    };
    let grant = issue(&mut owner, &mut f.storage);
    let (_, first) = owner
        .consume_prepared(&f.manager, &f.storage, grant)
        .unwrap();
    assert_eq!(first["state"], "ready");
    let grant = issue(&mut owner, &mut f.storage);
    owner.cancel();
    assert!(owner
        .consume_prepared(&f.manager, &f.storage, grant)
        .is_err());
    let grant = issue(&mut owner, &mut f.storage);
    assert!(Confirmations::default()
        .consume_prepared(&f.manager, &f.storage, grant)
        .is_err());
    let mut grant = issue(&mut owner, &mut f.storage);
    grant.expire_for_test();
    assert!(owner
        .consume_prepared(&f.manager, &f.storage, grant)
        .is_err());
    let grant = issue(&mut owner, &mut f.storage);
    assert!(owner
        .open(&f.manager, &f.storage, target, "invalid")
        .is_err());
    assert!(owner
        .consume_prepared(&f.manager, &f.storage, grant)
        .is_err());
    let grant = issue(&mut owner, &mut f.storage);
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    fs::write(active.worktree_path.join("src/edit.txt"), b"late edit").unwrap();
    assert!(owner
        .consume_prepared(&f.manager, &f.storage, grant)
        .is_err());
    assert_eq!(
        fs::read_dir(f.manager.data_root.join("application-backups-v1"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
}

#[cfg(unix)]
#[test]
fn staged_backup_explicit_recovery_revalidates_or_preserves_abandonment() {
    use application::review::{backup::staging, persistence, ReviewTarget};
    for complete in [false, true] {
        let (mut f, review_id) = prepared_backup_fixture();
        if complete {
            f.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_recovery_ready BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_backup_%_ready' BEGIN SELECT RAISE(ABORT, 'test recovery'); END;").unwrap();
            assert!(staging::prepare(
                &f.manager,
                &mut f.storage,
                ReviewTarget::Project(&f.project_id),
                &review_id,
                "req_recovery_prepare"
            )
            .is_err());
            f.storage
                .connection_for_test()
                .execute_batch("DROP TRIGGER fail_recovery_ready;")
                .unwrap();
        } else {
            assert!(staging::stop_after_reservation_for_test(
                &f.manager,
                &mut f.storage,
                ReviewTarget::Project(&f.project_id),
                &review_id
            )
            .is_err());
        }
        let db = f.manager.data_root.join("hello-agent.sqlite3");
        drop(f.storage);
        let mut storage = Storage::open(&db).unwrap();
        let manager = ProjectManager::new(&db).unwrap();
        let pending = staging::inspect(&manager, &storage, &f.project_id, &review_id).unwrap();
        let target = ReviewTarget::Project(&f.project_id);
        if complete {
            let active = manager
                .validate_active_project(&storage, &f.project_id)
                .unwrap();
            fs::write(active.worktree_path.join("src/edit.txt"), b"changed").unwrap();
            assert!(staging::resume(
                &manager,
                &mut storage,
                target,
                &review_id,
                "req_recovery_stale"
            )
            .is_err());
            fs::write(active.worktree_path.join("src/edit.txt"), b"hello").unwrap();
            let ready = staging::resume(
                &manager,
                &mut storage,
                target,
                &review_id,
                "req_recovery_resume",
            )
            .unwrap();
            assert_eq!(ready["state"], "ready");
            assert_eq!(
                staging::resume(
                    &manager,
                    &mut storage,
                    target,
                    &review_id,
                    "req_recovery_retry"
                )
                .unwrap(),
                ready
            );
            assert!(staging::abandon(
                &manager,
                &mut storage,
                &f.project_id,
                &review_id,
                &pending,
                "req_recovery_wrong_abandon"
            )
            .is_err());
        } else {
            assert!(staging::resume(
                &manager,
                &mut storage,
                target,
                &review_id,
                "req_recovery_missing"
            )
            .is_err());
            let path = manager
                .data_root
                .join("application-backups-v1")
                .join(pending["name"].as_str().unwrap());
            fs::write(&path, b"partial preserved").unwrap();
            assert!(staging::resume(
                &manager,
                &mut storage,
                target,
                &review_id,
                "req_recovery_partial"
            )
            .is_err());
            let mut stale = pending.clone();
            stale["name"] = json!("different");
            assert!(staging::abandon(
                &manager,
                &mut storage,
                &f.project_id,
                &review_id,
                &stale,
                "req_recovery_stale_abandon"
            )
            .is_err());
            let abandoned = staging::abandon(
                &manager,
                &mut storage,
                &f.project_id,
                &review_id,
                &pending,
                "req_recovery_abandon",
            )
            .unwrap();
            assert_eq!(abandoned["state"], "abandoned");
            assert_eq!(
                staging::abandon(
                    &manager,
                    &mut storage,
                    &f.project_id,
                    &review_id,
                    &pending,
                    "req_recovery_abandon_retry"
                )
                .unwrap(),
                abandoned
            );
            assert_eq!(fs::read(&path).unwrap(), b"partial preserved");
            let next = persistence::publish(
                &manager,
                &mut storage,
                &f.project_id,
                "req_recovery_new_review",
            )
            .unwrap();
            staging::prepare(
                &manager,
                &mut storage,
                target,
                next["reviewId"].as_str().unwrap(),
                "req_recovery_new_backup",
            )
            .unwrap();
            assert_eq!(
                staging::inspect(&manager, &storage, &f.project_id, &review_id).unwrap(),
                abandoned
            );
        }
        assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    }
}

#[cfg(unix)]
#[test]
fn staged_backup_survives_restart_and_keeps_header_binding() {
    use application::review::{backup::staging, ReviewTarget};
    let (mut f, review_id) = prepared_backup_fixture();
    f.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_backup_reserve BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_backup_%_preparing' BEGIN SELECT RAISE(ABORT, 'test backup reserve failure'); END;").unwrap();
    assert!(staging::prepare(
        &f.manager,
        &mut f.storage,
        ReviewTarget::Project(&f.project_id),
        &review_id,
        "req_backup_fail_reserve"
    )
    .is_err());
    // Free-space admission may create the managed directory, but no archive
    // bytes may be allocated before the durable reservation succeeds.
    let backup_root = f.manager.data_root.join("application-backups-v1");
    assert_eq!(fs::read_dir(&backup_root).unwrap().count(), 0);
    assert!(required_project(&f.storage, &f.project_id)
        .unwrap()
        .get("application_backups")
        .is_none());
    f.storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_backup_reserve;")
        .unwrap();
    let ready = staging::prepare(
        &f.manager,
        &mut f.storage,
        ReviewTarget::Project(&f.project_id),
        &review_id,
        "req_backup_prepare",
    )
    .unwrap();
    assert_eq!(ready["state"], "ready");
    assert!(staging::prepare(
        &f.manager,
        &mut f.storage,
        ReviewTarget::Project(&f.project_id),
        &review_id,
        "req_backup_duplicate"
    )
    .is_err());
    let database = f.manager.data_root.join("hello-agent.sqlite3");
    drop(f.storage);
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    assert_eq!(
        staging::inspect(&manager, &storage, &f.project_id, &review_id).unwrap(),
        ready
    );
    manager
        .open_project(
            &mut storage,
            f.source.to_str().unwrap(),
            "req_backup_reopen_project",
        )
        .unwrap();
    assert_eq!(
        staging::inspect(&manager, &storage, &f.project_id, &review_id).unwrap(),
        ready
    );
    let mut altered = required_project(&storage, &f.project_id).unwrap();
    altered["application_backups"][&review_id]["header"]["binding"]["workspaceIdentity"] =
        json!("different");
    storage
        .upsert_projection_record(
            "project",
            &f.project_id,
            "op_backup_bad_header",
            None,
            altered,
        )
        .unwrap();
    assert!(staging::inspect(&manager, &storage, &f.project_id, &review_id).is_err());
}

#[cfg(unix)]
#[test]
fn staged_backup_failures_preserve_reservation_without_promoting_files() {
    use application::review::{backup::staging, persistence, ReviewTarget};
    for completed_file in [false, true] {
        let (mut f, review_id) = prepared_backup_fixture();
        if completed_file {
            f.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_backup_ready BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_backup_%_ready' BEGIN SELECT RAISE(ABORT, 'test backup ready failure'); END;").unwrap();
            assert!(staging::prepare(
                &f.manager,
                &mut f.storage,
                ReviewTarget::Project(&f.project_id),
                &review_id,
                "req_backup_fail_publish"
            )
            .is_err());
            f.storage
                .connection_for_test()
                .execute_batch("DROP TRIGGER fail_backup_ready;")
                .unwrap();
        } else {
            assert!(staging::stop_after_reservation_for_test(
                &f.manager,
                &mut f.storage,
                ReviewTarget::Project(&f.project_id),
                &review_id
            )
            .is_err());
        }
        let database = f.manager.data_root.join("hello-agent.sqlite3");
        drop(f.storage);
        let mut storage = Storage::open(&database).unwrap();
        let manager = ProjectManager::new(&database).unwrap();
        let pending = staging::inspect(&manager, &storage, &f.project_id, &review_id).unwrap();
        assert_eq!(pending["state"], "preparing");
        assert!(pending.get("backup").is_none());
        let path = manager
            .data_root
            .join("application-backups-v1")
            .join(pending["name"].as_str().unwrap());
        assert_eq!(path.exists(), completed_file);
        let second =
            persistence::publish(&manager, &mut storage, &f.project_id, "req_next_review").unwrap();
        assert!(staging::prepare(
            &manager,
            &mut storage,
            ReviewTarget::Project(&f.project_id),
            second["reviewId"].as_str().unwrap(),
            "req_next_backup"
        )
        .is_err());
        assert_eq!(
            staging::inspect(&manager, &storage, &f.project_id, &review_id).unwrap(),
            pending
        );
        assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    }
}

#[cfg(unix)]
#[test]
fn application_backup_preserves_before_images_and_rejects_tampering() {
    use application::review::{backup, persistence, ReviewTarget};
    use std::io::{BufRead, Read};
    use std::os::unix::fs::{symlink, PermissionsExt};
    let mut f = source_index_fixture();
    fs::write(f.source.join("raw.bin"), [0, 255, 10, 13]).unwrap();
    symlink("outside-not-followed", f.source.join("raw-link")).unwrap();
    fs::set_permissions(
        f.source.join("src/edit.txt"),
        fs::Permissions::from_mode(0o750),
    )
    .unwrap();
    git_raw(&f.source, ["add", "."]);
    git_raw(
        &f.source,
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "backup originals",
        ],
    );
    f.manager
        .open_project(
            &mut f.storage,
            f.source.to_str().unwrap(),
            "req_backup_open",
        )
        .unwrap();
    f.manager
        .create_worktree_inner(
            &mut f.storage,
            &f.project_id,
            "req_backup_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    fs::write(active.worktree_path.join("src/edit.txt"), b"changed").unwrap();
    fs::remove_file(active.worktree_path.join("raw.bin")).unwrap();
    fs::remove_file(active.worktree_path.join("raw-link")).unwrap();
    symlink("new-target", active.worktree_path.join("raw-link")).unwrap();
    fs::create_dir(active.worktree_path.join("new-dir")).unwrap();
    fs::write(active.worktree_path.join("new-dir/new-file"), b"new").unwrap();
    let record = persistence::publish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        "req_backup_review",
    )
    .unwrap();
    for stage in [1, 2] {
        assert!(backup::fail_for_test(
            &f.manager,
            &f.storage,
            ReviewTarget::Project(&f.project_id),
            record["reviewId"].as_str().unwrap(),
            stage
        )
        .is_err());
        assert_eq!(
            fs::read_dir(f.manager.data_root.join("application-backups-v1"))
                .unwrap()
                .count(),
            0
        );
    }
    let point = backup::create(
        &f.manager,
        &f.storage,
        ReviewTarget::Project(&f.project_id),
        record["reviewId"].as_str().unwrap(),
    )
    .unwrap();
    backup::verify(&f.manager, &point).unwrap();
    let descriptor = point.to_value();
    let reopened = backup::Backup::from_value(&descriptor).unwrap();
    let verified_header = backup::verify(&f.manager, &reopened).unwrap();
    assert_eq!(verified_header["reviewId"], record["reviewId"]);
    let mut bad_descriptor = descriptor.clone();
    bad_descriptor["name"] = serde_json::json!("../outside.bin");
    assert!(backup::Backup::from_value(&bad_descriptor).is_err());
    bad_descriptor = descriptor.clone();
    bad_descriptor["extra"] = serde_json::json!(true);
    assert!(backup::Backup::from_value(&bad_descriptor).is_err());
    let path = point.path_for_test(&f.manager);
    let bytes = fs::read(&path).unwrap();
    let mut reader = std::io::Cursor::new(&bytes);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let header: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(header["reviewId"], record["reviewId"]);
    let mut items = std::collections::BTreeMap::new();
    for _ in 0..header["count"].as_u64().unwrap() {
        line.clear();
        reader.read_line(&mut line).unwrap();
        let item: serde_json::Value = serde_json::from_str(&line).unwrap();
        let mut data = vec![0; item["before"]["size"].as_u64().unwrap_or(0) as usize];
        reader.read_exact(&mut data).unwrap();
        items.insert(item["path"].as_str().unwrap().to_owned(), (item, data));
    }
    assert_eq!(reader.position() as usize, bytes.len());
    assert_eq!(items["raw.bin"].1, [0, 255, 10, 13]);
    assert_eq!(items["raw-link"].1, b"outside-not-followed");
    assert_eq!(items["src/edit.txt"].1, b"hello");
    assert_eq!(
        items["src/edit.txt"].0["before"]["mode"].as_u64().unwrap() & 0o7777,
        0o750
    );
    assert!(items["new-dir"].0["before"].is_null());
    assert!(items["new-dir/new-file"].0["before"].is_null());
    assert_eq!(items["new-dir"].0["after"]["kind"], 2);
    assert_eq!(items["new-dir/new-file"].0["after"]["kind"], 1);
    assert!(items["raw.bin"].0["after"].is_null());
    assert_eq!(
        fs::read(f.source.join("raw.bin")).unwrap(),
        [0, 255, 10, 13]
    );
    assert_eq!(fs::read(f.source.join("src/edit.txt")).unwrap(), b"hello");
    fs::write(&path, b"truncated").unwrap();
    assert!(backup::verify(&f.manager, &point).is_err());
    fs::write(&path, &bytes).unwrap();
    fs::hard_link(&path, path.with_extension("alias")).unwrap();
    assert!(backup::verify(&f.manager, &point).is_err());
}

#[cfg(unix)]
#[test]
fn application_confirmation_is_ephemeral_single_use_and_revalidates() {
    use application::review::{confirmation::Confirmations, persistence, ReviewTarget};
    let mut f = source_index_fixture();
    f.manager
        .open_project(
            &mut f.storage,
            f.source.to_str().unwrap(),
            "req_confirm_open",
        )
        .unwrap();
    f.manager
        .create_worktree_inner(
            &mut f.storage,
            &f.project_id,
            "req_confirm_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    let saved = persistence::publish(
        &f.manager,
        &mut f.storage,
        &f.project_id,
        "req_confirm_publish",
    )
    .unwrap();
    let id = saved["reviewId"].as_str().unwrap();
    let target = ReviewTarget::Project(&f.project_id);
    let mut owner = Confirmations::default();
    let (first, _) = owner.open(&f.manager, &f.storage, target, id).unwrap();
    assert!(Confirmations::default()
        .confirm(&f.manager, &f.storage, &first)
        .is_err());
    owner.confirm(&f.manager, &f.storage, &first).unwrap();
    assert!(owner.confirm(&f.manager, &f.storage, &first).is_err());
    let (canceled, _) = owner.open(&f.manager, &f.storage, target, id).unwrap();
    owner.cancel();
    assert!(owner.confirm(&f.manager, &f.storage, &canceled).is_err());
    let (expired, _) = owner.open(&f.manager, &f.storage, target, id).unwrap();
    owner.expire_for_test();
    assert!(owner.confirm(&f.manager, &f.storage, &expired).is_err());
    let (old, _) = owner.open(&f.manager, &f.storage, target, id).unwrap();
    let (new, _) = owner.open(&f.manager, &f.storage, target, id).unwrap();
    assert_ne!(old, new);
    assert!(owner.confirm(&f.manager, &f.storage, &old).is_err());
    assert!(owner.confirm(&f.manager, &f.storage, &new).is_err());
    let (failed_open, _) = owner.open(&f.manager, &f.storage, target, id).unwrap();
    assert!(owner
        .open(&f.manager, &f.storage, target, "invalid")
        .is_err());
    assert!(owner.confirm(&f.manager, &f.storage, &failed_open).is_err());
    let (stale, _) = owner.open(&f.manager, &f.storage, target, id).unwrap();
    let active = f
        .manager
        .validate_active_project(&f.storage, &f.project_id)
        .unwrap();
    std::fs::write(
        active.worktree_path.join("src/edit.txt"),
        b"changed after preview",
    )
    .unwrap();
    assert!(owner.confirm(&f.manager, &f.storage, &stale).is_err());
    assert_eq!(
        std::fs::read(f.source.join("src/edit.txt")).unwrap(),
        b"hello"
    );
}

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
fn chat_application_reviews_keep_separate_targets_and_invalidate_on_chat_change() {
    use application::review::{persistence, ReviewTarget};
    let mut fixture = source_index_fixture();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_chat_review_open",
        )
        .unwrap();
    let mut saved = Vec::new();
    for name in ["one", "two"] {
        fixture
            .manager
            .create_worktree_inner(
                &mut fixture.storage,
                &fixture.project_id,
                &format!("req_chat_review_{name}"),
                None,
                None,
                SnapshotCreation::ApplicationCandidate,
            )
            .unwrap();
        let active = fixture
            .manager
            .validate_active_project(&fixture.storage, &fixture.project_id)
            .unwrap();
        let address = WorkspaceAddress {
            chat_id: format!("chat_{name}"),
            project_id: fixture.project_id.clone(),
            worktree_id: active.worktree_id,
            workspace_identity: active.workspace_identity,
        };
        let task = format!("task_{name}");
        let payload = json!({"chat_id":address.chat_id,"project_id":address.project_id,"worktree_id":address.worktree_id,"workspace_identity":address.workspace_identity,"state":"draft","durable_event_sequence":0,"last_message":null,"record":{"chatId":address.chat_id,"projectId":address.project_id,"worktreeId":address.worktree_id,"workspaceIdentity":address.workspace_identity,"taskId":task,"state":"draft"}});
        fixture
            .storage
            .upsert_projection_record(
                "chat",
                &address.chat_id,
                &format!("op_chat_review_{name}"),
                None,
                payload.as_object().unwrap().clone(),
            )
            .unwrap();
        let result = persistence::publish_target(
            &fixture.manager,
            &mut fixture.storage,
            ReviewTarget::Chat {
                address: &address,
                task_id: &task,
            },
            &format!("req_publish_{name}"),
        )
        .unwrap();
        saved.push((
            address,
            task,
            result["reviewId"].as_str().unwrap().to_owned(),
        ));
    }
    for (address, task, id) in &saved {
        let preview = fixture
            .manager
            .chat_application_preview(&fixture.storage, &address.project_id, &address.chat_id)
            .unwrap();
        assert_eq!(preview.result["chatId"], address.chat_id);
        assert_eq!(preview.result["preview"]["worktreeId"], address.worktree_id);
        assert!(fixture
            .manager
            .chat_application_preview(&fixture.storage, "project_other", &address.chat_id)
            .is_err());
        persistence::reopen_target(
            &fixture.manager,
            &fixture.storage,
            ReviewTarget::Chat {
                address,
                task_id: task,
            },
            id,
        )
        .unwrap();
        assert!(persistence::reopen_target(
            &fixture.manager,
            &fixture.storage,
            ReviewTarget::Chat {
                address,
                task_id: "wrong_task"
            },
            id
        )
        .is_err());
    }
    let (first, task, id) = &saved[0];
    let mixed = WorkspaceAddress {
        chat_id: first.chat_id.clone(),
        ..saved[1].0.clone()
    };
    assert!(persistence::reopen_target(
        &fixture.manager,
        &fixture.storage,
        ReviewTarget::Chat {
            address: &mixed,
            task_id: task
        },
        id
    )
    .is_err());
    let mut chat = fixture
        .storage
        .projection_record("chat", &first.chat_id)
        .unwrap()
        .unwrap();
    let old_revision = chat["last_event_sequence"].as_u64().unwrap();
    chat.insert("test_followup".into(), json!(true));
    fixture
        .storage
        .upsert_projection_record("chat", &first.chat_id, "op_followup_review", None, chat)
        .unwrap();
    assert!(persistence::reopen_target(
        &fixture.manager,
        &fixture.storage,
        ReviewTarget::Chat {
            address: first,
            task_id: task
        },
        id
    )
    .is_err());
    let project = required_project(&fixture.storage, &fixture.project_id).unwrap();
    assert!(fixture
        .storage
        .upsert_projection_records_atomic_checked(
            vec![crate::storage::ProjectionUpsert {
                aggregate_type: "project",
                aggregate_id: fixture.project_id.clone(),
                operation_id: "op_stale_chat_review".into(),
                request_id: None,
                payload: project.clone()
            }],
            &[("chat", &first.chat_id, old_revision)]
        )
        .is_err());
    assert_eq!(
        required_project(&fixture.storage, &fixture.project_id).unwrap(),
        project
    );
    let (second, task, id) = &saved[1];
    persistence::reopen_target(
        &fixture.manager,
        &fixture.storage,
        ReviewTarget::Chat {
            address: second,
            task_id: task,
        },
        id,
    )
    .unwrap();
    let lock_id = format!("lock_{}", second.worktree_id);
    fixture.storage.upsert_projection_record("run", "run_test_writer", "op_review_completed_run", None, json!({"run_id":"run_test_writer","chat_id":second.chat_id,"state":"succeeded","worktree_path":null}).as_object().unwrap().clone()).unwrap();
    let mut lock = fixture
        .storage
        .projection_record("lock", &lock_id)
        .unwrap()
        .unwrap();
    lock.insert("writer_run_id".into(), json!("run_test_writer"));
    fixture
        .storage
        .upsert_projection_record("lock", &lock_id, "op_review_writer_acquire", None, lock)
        .unwrap();
    assert!(persistence::publish_target(
        &fixture.manager,
        &mut fixture.storage,
        ReviewTarget::Chat {
            address: second,
            task_id: task
        },
        "req_review_while_owned"
    )
    .is_err());
    let mut lock = fixture
        .storage
        .projection_record("lock", &lock_id)
        .unwrap()
        .unwrap();
    lock.insert("writer_run_id".into(), Value::Null);
    fixture
        .storage
        .upsert_projection_record("lock", &lock_id, "op_review_writer_release", None, lock)
        .unwrap();
    assert!(persistence::reopen_target(
        &fixture.manager,
        &fixture.storage,
        ReviewTarget::Chat {
            address: second,
            task_id: task
        },
        id
    )
    .is_err());
    let mut tampered = fixture
        .storage
        .projection_record("lock", &lock_id)
        .unwrap()
        .unwrap();
    tampered.insert("worktree_path".into(), json!("/different-workspace"));
    fixture
        .storage
        .upsert_projection_record("lock", &lock_id, "op_review_wrong_path", None, tampered)
        .unwrap();
    assert!(persistence::publish_target(
        &fixture.manager,
        &mut fixture.storage,
        ReviewTarget::Chat {
            address: second,
            task_id: task
        },
        "req_review_wrong_path"
    )
    .is_err());
    assert!(persistence::reopen_target(
        &fixture.manager,
        &fixture.storage,
        ReviewTarget::Chat {
            address: second,
            task_id: task
        },
        id
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn saved_application_review_reopens_without_approval_and_rejects_replacement() {
    use application::review::persistence;
    let mut fixture = source_index_fixture();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_saved_review_open",
        )
        .unwrap();
    fixture
        .manager
        .create_worktree_inner(
            &mut fixture.storage,
            &fixture.project_id,
            "req_saved_review_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    let first = persistence::publish(
        &fixture.manager,
        &mut fixture.storage,
        &fixture.project_id,
        "req_review_publish",
    )
    .unwrap();
    assert_eq!(first["state"], "prepared");
    let database = fixture.manager.data_root.join("hello-agent.sqlite3");
    drop(fixture.storage);
    let mut storage = Storage::open(&database).unwrap();
    let mut manager = ProjectManager::new(&database).unwrap();
    let review_id = first["reviewId"].as_str().unwrap();
    assert!(
        persistence::reopen(&manager, &storage, &fixture.project_id, review_id)
            .unwrap()
            .changed_paths()
            .is_empty()
    );
    assert_eq!(
        required_project(&storage, &fixture.project_id).unwrap()["application_review"],
        first
    );
    let mut altered = required_project(&storage, &fixture.project_id).unwrap();
    altered["application_review"]["state"] = json!("approved");
    storage
        .upsert_projection_record(
            "project",
            &fixture.project_id,
            "op_review_fake_approval",
            None,
            altered,
        )
        .unwrap();
    assert!(persistence::reopen(&manager, &storage, &fixture.project_id, review_id).is_err());
    let mut restored = required_project(&storage, &fixture.project_id).unwrap();
    restored.insert("application_review".into(), first.clone());
    storage
        .upsert_projection_record(
            "project",
            &fixture.project_id,
            "op_review_restore_prepared",
            None,
            restored,
        )
        .unwrap();
    storage.connection_for_test().execute_batch("CREATE TRIGGER fail_review BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_review_%' BEGIN SELECT RAISE(ABORT, 'review publication failure'); END;").unwrap();
    assert!(persistence::publish(
        &manager,
        &mut storage,
        &fixture.project_id,
        "req_publish_failure"
    )
    .is_err());
    storage
        .connection_for_test()
        .execute_batch("DROP TRIGGER fail_review;")
        .unwrap();
    assert_eq!(
        required_project(&storage, &fixture.project_id).unwrap()["application_review"],
        first
    );
    let next = persistence::publish(
        &manager,
        &mut storage,
        &fixture.project_id,
        "req_review_replace",
    )
    .unwrap();
    assert!(persistence::reopen(&manager, &storage, &fixture.project_id, review_id).is_err());
    let next_id = next["reviewId"].as_str().unwrap();
    persistence::reopen(&manager, &storage, &fixture.project_id, next_id).unwrap();
    let active = manager
        .validate_active_project(&storage, &fixture.project_id)
        .unwrap();
    fs::write(active.worktree_path.join("replacement"), b"hello").unwrap();
    fs::rename(
        active.worktree_path.join("replacement"),
        active.worktree_path.join("src/edit.txt"),
    )
    .unwrap();
    assert!(persistence::reopen(&manager, &storage, &fixture.project_id, next_id).is_err());
    manager
        .create_worktree_inner(
            &mut storage,
            &fixture.project_id,
            "req_saved_review_new_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    assert_eq!(
        required_project(&storage, &fixture.project_id).unwrap()["application_review"],
        next
    );
    assert!(persistence::reopen(&manager, &storage, &fixture.project_id, next_id).is_err());
}

#[cfg(unix)]
#[test]
fn application_review_binds_all_changes_and_rejects_stale_snapshot() {
    use application::review;
    use std::os::unix::fs::{symlink, PermissionsExt};
    let mut fixture = source_index_fixture();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_review_open",
        )
        .unwrap();
    fixture
        .manager
        .create_worktree_inner(
            &mut fixture.storage,
            &fixture.project_id,
            "req_review_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    let active = fixture
        .manager
        .validate_active_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    let clean = review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).unwrap();
    assert!(
        clean.changed_paths().is_empty(),
        "{:?}",
        clean.changed_paths()
    );
    fs::write(active.worktree_path.join("src/edit.txt"), [0, 255, 7]).unwrap();
    fs::set_permissions(
        active.worktree_path.join("src/edit.txt"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    fs::set_permissions(
        active.worktree_path.join("src/edit.txt"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::create_dir(active.worktree_path.join("new-dir")).unwrap();
    fs::write(active.worktree_path.join("new-dir/file"), b"added").unwrap();
    symlink("/not-followed", active.worktree_path.join("new-link")).unwrap();
    let plan = review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).unwrap();
    assert_eq!(
        plan.changed_paths(),
        vec!["new-dir", "new-dir/file", "new-link", "src/edit.txt"]
    );
    review::revalidate(
        &fixture.manager,
        &fixture.storage,
        &fixture.project_id,
        &plan,
    )
    .unwrap();
    fs::write(active.worktree_path.join("new-dir/replacement"), b"added").unwrap();
    fs::rename(
        active.worktree_path.join("new-dir/replacement"),
        active.worktree_path.join("new-dir/file"),
    )
    .unwrap();
    assert!(review::revalidate(
        &fixture.manager,
        &fixture.storage,
        &fixture.project_id,
        &plan
    )
    .is_err());
    let plan = review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).unwrap();
    assert!(review::revalidate(
        &fixture.manager,
        &fixture.storage,
        &fixture.project_id,
        &clean
    )
    .is_err());
    fs::remove_file(active.worktree_path.join("src/edit.txt")).unwrap();
    let deleted = review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).unwrap();
    assert!(deleted.changed_paths().contains(&"src/edit.txt"));
    assert!(review::revalidate(
        &fixture.manager,
        &fixture.storage,
        &fixture.project_id,
        &plan
    )
    .is_err());
    fs::create_dir(active.worktree_path.join("src/edit.txt")).unwrap();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    assert_eq!(
        fs::read(fixture.source.join("src/edit.txt")).unwrap(),
        b"hello"
    );
}

#[cfg(unix)]
#[test]
fn application_review_preserves_untracked_source_and_rejects_legacy() {
    use application::review;
    let mut fixture = source_index_fixture();
    fs::write(fixture.source.join("personal"), b"keep").unwrap();
    fs::write(fixture.source.join("café"), b"unicode").unwrap();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_review_open",
        )
        .unwrap();
    let legacy = fixture.create();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    fixture
        .manager
        .create_worktree_inner(
            &mut fixture.storage,
            &fixture.project_id,
            "req_review_snapshot",
            None,
            None,
            SnapshotCreation::ApplicationCandidate,
        )
        .unwrap();
    let active = fixture
        .manager
        .validate_active_project(&fixture.storage, &fixture.project_id)
        .unwrap();
    assert_ne!(legacy.worktree_id, active.worktree_id);
    assert!(
        review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id)
            .unwrap()
            .changed_paths()
            .is_empty()
    );
    fs::write(active.worktree_path.join("personal"), b"keep").unwrap();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    fs::remove_file(active.worktree_path.join("personal")).unwrap();
    fs::write(active.worktree_path.join("Personal"), b"case collision").unwrap();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    fs::remove_file(active.worktree_path.join("Personal")).unwrap();
    fs::write(
        active.worktree_path.join("cafe\u{301}"),
        b"unicode collision",
    )
    .unwrap();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    fs::remove_file(active.worktree_path.join("cafe\u{301}")).unwrap();
    fs::rename(
        active.worktree_path.join("src/edit.txt"),
        active.worktree_path.join("src/Edit.txt"),
    )
    .unwrap();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    fs::rename(
        active.worktree_path.join("src/Edit.txt"),
        active.worktree_path.join("src/edit.txt"),
    )
    .unwrap();
    fs::write(fixture.source.join("personal"), b"changed externally").unwrap();
    assert!(review::prepare(&fixture.manager, &fixture.storage, &fixture.project_id).is_err());
    assert_eq!(
        fs::read(fixture.source.join("personal")).unwrap(),
        b"changed externally"
    );
}

#[cfg(unix)]
#[test]
fn application_snapshot_creation_preserves_all_receipts_after_restart() {
    use application::git_baseline::raw_baseline::{capture_clean_baseline, receipt};
    let mut fixture = source_index_fixture();
    fixture
        .manager
        .open_project(
            &mut fixture.storage,
            fixture.source.to_str().unwrap(),
            "req_app_open",
        )
        .unwrap();
    let before = git_bytes_raw(&fixture.source, ["worktree", "list", "--porcelain"]);
    let mut snapshots = Vec::new();
    for request in ["req_app_one", "req_app_two", "req_app_three"] {
        fixture
            .manager
            .create_worktree_inner(
                &mut fixture.storage,
                &fixture.project_id,
                request,
                None,
                None,
                SnapshotCreation::ApplicationCandidate,
            )
            .unwrap();
        snapshots.push(
            fixture
                .manager
                .validate_active_project(&fixture.storage, &fixture.project_id)
                .unwrap(),
        );
    }
    let database = fixture.manager.data_root.join("hello-agent.sqlite3");
    drop(fixture.storage);
    let storage = Storage::open(&database).unwrap();
    let manager = ProjectManager::new(&database).unwrap();
    let source = manager.open_source_repository(&fixture.source).unwrap();
    let baseline = capture_clean_baseline(&manager, &source).unwrap();
    let project = required_project(&storage, &fixture.project_id).unwrap();
    assert_eq!(project["source_baselines"].as_object().unwrap().len(), 3);
    for active in &snapshots {
        receipt::verify_bound(&manager, &storage, active, &baseline).unwrap();
    }
    assert_eq!(
        before,
        git_bytes_raw(&fixture.source, ["worktree", "list", "--porcelain"])
    );
    assert!(storage.blocker_ids().unwrap().is_empty());
    fs::write(
        fixture.source.join("new-untracked-file"),
        b"changed after creation",
    )
    .unwrap();
    let changed = capture_clean_baseline(&manager, &source).unwrap();
    assert!(receipt::verify_bound(&manager, &storage, &snapshots[0], &changed).is_err());
}

#[cfg(unix)]
#[test]
fn application_snapshot_creation_keeps_failures_fenced_without_downgrade() {
    for scenario in ["dirty", "publication", "activation"] {
        let mut fixture = source_index_fixture();
        fixture
            .manager
            .open_project(
                &mut fixture.storage,
                fixture.source.to_str().unwrap(),
                "req_app_open",
            )
            .unwrap();
        if scenario == "dirty" {
            fs::write(fixture.source.join("src/edit.txt"), b"user change").unwrap();
        }
        if scenario == "publication" {
            fixture.storage.connection_for_test().execute_batch("CREATE TRIGGER fail_baseline BEFORE INSERT ON events WHEN NEW.operation_id LIKE 'op_source_baseline_%' BEGIN SELECT RAISE(ABORT, 'publication failure'); END;").unwrap();
        }
        let point = (scenario == "activation")
            .then_some(WorktreeFailurePoint::AfterWorkspaceRenameBeforeActivation);
        assert!(fixture
            .manager
            .create_worktree_inner(
                &mut fixture.storage,
                &fixture.project_id,
                "req_app_failure",
                point,
                None,
                SnapshotCreation::ApplicationCandidate
            )
            .is_err());
        let project = required_project(&fixture.storage, &fixture.project_id).unwrap();
        assert_eq!(project["lifecycle"], "recoveryRequired");
        assert!(!fixture.storage.blocker_ids().unwrap().is_empty());
        assert!(fixture
            .manager
            .inspect_unstarted_snapshot(&fixture.storage, &fixture.project_id)
            .is_err());
        assert_eq!(
            project.contains_key("source_baselines"),
            scenario == "activation"
        );
        if scenario == "publication" {
            assert!(fixture
                .manager
                .data_root
                .join("source-baselines-v1")
                .join(format!(
                    "{}.source-baseline-v1.json",
                    project["worktree_id"].as_str().unwrap()
                ))
                .is_file());
        }
        if scenario == "dirty" {
            assert_eq!(
                fs::read(fixture.source.join("src/edit.txt")).unwrap(),
                b"user change"
            );
        }
    }
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
                scan: None,
                initial_scan_pending: false,
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
    fixture.manager.finish_workspace_discard(&WorkspaceAddress {
        chat_id: "chat_a".into(),
        project_id: fixture.project_id.clone(),
        worktree_id: "worktree_test".into(),
        workspace_identity: "identity_test".into(),
    });
    assert_eq!(fixture.manager.watchers.len(), 2);
    assert!(fixture.manager.watchers.contains_key(&fixture.project_id));
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

#[test]
fn clean_diff_does_not_hide_later_tracked_or_untracked_changes() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let summary = fixture.manager.diff_summary(&fixture.storage, &fixture.project_id).unwrap();
    assert!(summary.result["entries"].as_array().unwrap().is_empty());

    fs::write(active.worktree_path.join("src/edit.txt"), b"changed").unwrap();
    fs::write(active.worktree_path.join("src/fresh.txt"), b"new").unwrap();
    let summary = fixture.manager.diff_summary(&fixture.storage, &fixture.project_id).unwrap();
    let entries = summary.result["entries"].as_array().unwrap();
    assert!(entries.iter().any(|entry| entry["path"] == "src/edit.txt" && entry["kind"] == "modified"));
    assert!(entries.iter().any(|entry| entry["path"] == "src/fresh.txt" && entry["kind"] == "added"));

    fs::write(active.worktree_path.join("src/edit.txt"), b"hello").unwrap();
    fs::remove_file(active.worktree_path.join("src/fresh.txt")).unwrap();
    let summary = fixture.manager.diff_summary(&fixture.storage, &fixture.project_id).unwrap();
    assert!(summary.result["entries"].as_array().unwrap().is_empty());
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

#[cfg(unix)]
#[test]
fn concurrent_snapshot_checks_reject_each_repository_invariant_drift() {
    use std::os::unix::fs::PermissionsExt as _;
    for case in ["head", "symbolic", "object", "index", "remote", "oversized-remote"] {
        let mut fixture = Fixture::open();
        let active = fixture.create();
        let workspace = fixture.manager.worktree_capability(&active).unwrap();
        fixture.manager.verify_snapshot_repository(&workspace, &active).unwrap();
        match case {
            "head" => fs::write(active.worktree_path.join(".git/HEAD"), b"invalid\n").unwrap(),
            "symbolic" => {
                git_raw(&active.worktree_path, ["update-ref", "refs/heads/drift", &active.base_commit]);
                git_raw(&active.worktree_path, ["symbolic-ref", "HEAD", "refs/heads/drift"]);
            }
            "object" => {
                let objects = active.worktree_path.join(".git/objects");
                let packs: Vec<_> = fs::read_dir(objects.join("pack")).unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "pack"))
                    .collect();
                if packs.is_empty() {
                    let object = objects.join(&active.base_commit[..2]).join(&active.base_commit[2..]);
                    assert!(object.is_file());
                    fs::set_permissions(&object, fs::Permissions::from_mode(0o600)).unwrap();
                    fs::write(object, b"invalid object").unwrap();
                } else {
                    // Corrupt the actual test-owned object storage, not a
                    // duplicate loose copy that Git may ignore in favor of a pack.
                    for pack in packs {
                        fs::set_permissions(&pack, fs::Permissions::from_mode(0o600)).unwrap();
                        fs::write(pack, b"invalid pack").unwrap();
                    }
                }
            }
            "index" => {
                fs::write(active.worktree_path.join("drift.txt"), b"drift").unwrap();
                git_raw(&active.worktree_path, ["add", "drift.txt"]);
            }
            "remote" => git_raw(&active.worktree_path, ["remote", "add", "forbidden", "/nonexistent-fixture-remote"]),
            "oversized-remote" => {
                let name = "r".repeat(1025);
                git_raw(&active.worktree_path, ["remote", "add", name.as_str(), "/nonexistent-fixture-remote"]);
            }
            _ => unreachable!(),
        }
        assert!(fixture.manager.verify_snapshot_repository(&workspace, &active).is_err(), "accepted {case} drift");
    }
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
    let completed_project = storage
        .projection_record("project", &fixture.project_id)
        .unwrap();
    manager
        .abandon_unstarted_snapshot(&mut storage, &inspection, "req_response_lost")
        .unwrap();
    assert_eq!(
        storage
            .projection_record("project", &fixture.project_id)
            .unwrap(),
        completed_project
    );
    assert!(manager
        .abandon_unstarted_snapshot(&mut storage, &stale, "req_stale_completed")
        .is_err());
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
    assert!(manager
        .abandon_unstarted_snapshot(&mut storage, &inspection, "req_old_snapshot")
        .is_err());
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
fn watcher_scan_slices_match_complete_scan_and_allow_restart() {
    let root = TempDir::new().unwrap();
    for index in 0..20 {
        let child = root.path().join(format!("dir-{index}"));
        fs::create_dir(&child).unwrap();
        fs::write(child.join("file.txt"), b"content").unwrap();
    }
    let capability = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
    let expected = snapshot_tree(&capability).unwrap();
    let mut scan = WatchScan::new(&capability).unwrap();
    assert!(!scan.advance(0).unwrap());
    assert!(scan.snapshot.is_empty());
    let mut turns = 0;
    loop {
        let before = scan.snapshot.len();
        let done = scan.advance(3).unwrap();
        assert!(scan.snapshot.len() - before <= 3);
        assert!(scan.pending.len() <= 2, "retain ancestors, not siblings");
        turns += 1;
        if done { break; }
        assert!(turns < 100);
    }
    assert!(turns > 1);
    assert!(scan.snapshot == expected);
    assert!(scan.pending.is_empty());
    let mut canceled = WatchScan::new(&capability).unwrap();
    assert!(!canceled.advance(1).unwrap());
    drop(canceled);
    assert!(snapshot_tree(&capability).unwrap() == expected);
}

#[test]
fn watcher_scan_skipped_names_consume_slice_budget() {
    let root = TempDir::new().unwrap();
    for index in 0..20 {
        fs::write(root.path().join(format!(".hello-agent-save-{index}")), b"").unwrap();
    }
    let capability = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
    let mut scan = WatchScan::new(&capability).unwrap();
    assert!(!scan.advance(2).unwrap());
    assert!(scan.snapshot.is_empty());
    assert!(!scan.pending.is_empty());
    while !scan.advance(2).unwrap() {}
    assert!(scan.snapshot.is_empty());
}

#[test]
#[cfg(unix)]
fn watcher_entry_metadata_does_not_follow_external_or_dangling_links() {
    let root = TempDir::new().unwrap();
    let watched = root.path().join("watched");
    let outside = root.path().join("outside");
    fs::create_dir(&watched).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("private.txt"), b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, watched.join("external")).unwrap();
    std::os::unix::fs::symlink(root.path().join("missing"), watched.join("dangling")).unwrap();
    fs::write(watched.join("local.txt"), b"local").unwrap();
    let capability = Dir::open_ambient_dir(&watched, ambient_authority()).unwrap();
    let before = snapshot_tree(&capability).unwrap();
    assert_eq!(before.len(), 3);
    assert_eq!(before["external"].kind, 3);
    assert_eq!(before["dangling"].kind, 3);
    assert_eq!(before["local.txt"].length, 5);
    fs::write(outside.join("private.txt"), b"changed outside").unwrap();
    assert!(snapshot_tree(&capability).unwrap() == before);
    fs::write(watched.join("local.txt"), b"local change").unwrap();
    assert!(snapshot_tree(&capability).unwrap() != before);
}

fn poll_until_watch_event(fixture: &mut Fixture) -> Vec<WatchEmission> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        let events = fixture.manager.poll_watchers(&fixture.storage);
        if !events.is_empty() { return events; }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("watch event did not arrive within bounded scan turns");
}

#[test]
#[cfg(any(unix, windows))]
fn watcher_scan_rejects_descendant_replaced_between_slices() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("child")).unwrap();
    fs::write(root.path().join("child/old.txt"), b"old").unwrap();
    let capability = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
    let mut scan = WatchScan::new(&capability).unwrap();
    assert!(!scan.advance(1).unwrap());
    assert_eq!(scan.pending.last().unwrap().2, "child");
    fs::rename(root.path().join("child"), root.path().join("moved")).unwrap();
    fs::create_dir(root.path().join("child")).unwrap();
    fs::write(root.path().join("child/new.txt"), b"new").unwrap();
    assert!(scan.validate_held_paths(&capability).is_err());
}

#[test]
#[cfg(unix)]
fn watcher_pending_scan_allows_tree_requests_and_stop_drops_scan() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    for index in 0..300 {
        fs::write(active.worktree_path.join(format!("scan-{index}")), b"x").unwrap();
    }
    fixture.manager.watch_project(&fixture.storage, &fixture.project_id).unwrap();
    let state = &fixture.manager.watchers[&fixture.project_id];
    assert!(state.initial_scan_pending);
    assert!(state.scan.as_ref().unwrap().snapshot.len() <= 128);
    assert!(fixture.manager.get_tree(&fixture.storage, &fixture.project_id, "", None).is_ok());
    fixture.manager.stop_watchers();
    assert!(fixture.manager.watchers.is_empty());
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());
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
    let events = poll_until_watch_event(&mut fixture);
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
    let restarted = poll_until_watch_event(&mut fixture);
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
    let events = poll_until_watch_event(&mut fixture);
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
    if cfg!(unix) {
        let initial = poll_until_watch_event(&mut fixture);
        assert!(initial[0].payload["rescanRequired"].as_bool().unwrap());
    }
    fs::write(active.worktree_path.join("resumed.txt"), b"x").unwrap();
    let resumed_events = poll_until_watch_event(&mut fixture);
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
    let events = if cfg!(unix) {
        assert!(action.result["watching"].as_bool().unwrap());
        assert!(action.watch_events.is_empty());
        assert!(fixture.manager.watchers[&fixture.project_id].initial_scan_pending);
        poll_until_watch_event(&mut fixture)
    } else {
        assert!(!action.result["watching"].as_bool().unwrap());
        action.watch_events
    };
    assert_eq!(events.len(), 1);
    assert!(events[0].payload["rescanRequired"]
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

#[test]
#[cfg(unix)]
fn watcher_completed_validation_rechecks_manifest_and_rejects_replaced_stream() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    fixture.manager.watch_project(&fixture.storage, &fixture.project_id).unwrap();
    fs::write(active.worktree_path.join("src/edit.txt"), b"watch change").unwrap();
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !fixture.manager.watch_job.as_ref().unwrap().finished() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let original = fs::read(&active.manifest_path).unwrap();
    fs::write(&active.manifest_path, b"{}").unwrap();
    let events = fixture.manager.poll_watchers(&fixture.storage);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["rescanRequired"], true);
    assert!(fixture.manager.watchers.is_empty());
    fs::write(&active.manifest_path, original).unwrap();
    fixture.manager.watch_project(&fixture.storage, &fixture.project_id).unwrap();
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());
    fixture.manager.watch_project(&fixture.storage, &fixture.project_id).unwrap();
    let stream = fixture.manager.watchers[&fixture.project_id].stream_id.clone();
    // Old result is canceled/discarded; a new validation is owned by the new stream.
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());
    assert_eq!(fixture.manager.watch_job.as_ref().unwrap().stream, stream);
    fixture.manager.stop_watchers();
    assert!(fixture.manager.watch_job.is_none());
    assert!(fixture.manager.watchers.is_empty());
}

#[test]
#[cfg(unix)]
fn watcher_result_keeps_directory_identity_until_consumed() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    fixture.manager.watch_project(&fixture.storage, &fixture.project_id).unwrap();
    assert!(fixture.manager.poll_watchers(&fixture.storage).is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !fixture.manager.watch_job.as_ref().unwrap().finished() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let done = fixture.manager.watch_job.take().unwrap().complete();
    let current = fixture.manager.worktree_capability(&active).unwrap();
    assert!(done.succeeded);
    assert!(done.matches_capability(&current).unwrap());
    fs::rename(active.worktree_path.join(".git"), active.worktree_path.join(".git-held")).unwrap();
    fs::create_dir(active.worktree_path.join(".git")).unwrap();
    assert!(!done.matches_capability(&current).unwrap());
    fs::remove_dir(active.worktree_path.join(".git")).unwrap();
    fs::rename(active.worktree_path.join(".git-held"), active.worktree_path.join(".git")).unwrap();
    assert!(done.matches_capability(&current).unwrap());
    let old_path = active.worktree_path.with_extension("old");
    fs::rename(&active.worktree_path, &old_path).unwrap();
    fs::create_dir(&active.worktree_path).unwrap();
    let replacement = fixture.manager.worktree_capability(&active).unwrap();
    assert!(!done.matches_capability(&replacement).unwrap());
}

#[test]
#[cfg(any(unix, windows))]
fn watcher_scan_rejects_a_different_root_between_slices() {
    let root = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    fs::write(root.path().join("old.txt"), b"old").unwrap();
    let capability = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
    let replacement = Dir::open_ambient_dir(other.path(), ambient_authority()).unwrap();
    let scan = WatchScan::new(&capability).unwrap();
    scan.validate_held_paths(&capability).unwrap();
    assert!(scan.validate_held_paths(&replacement).is_err());
}

#[test]
fn tree_lists_directories_before_files_across_pages() {
    let mut fixture = Fixture::open();
    let active = fixture.create();
    let folder = active.worktree_path.join("ordering");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("aaa.txt"), b"file").unwrap();
    for index in 0..=TREE_PAGE_MAX_ENTRIES_V1 {
        fs::create_dir(folder.join(format!("z-{index:05}"))).unwrap();
    }
    let first = fixture
        .manager
        .get_tree(&fixture.storage, &fixture.project_id, "ordering", None)
        .unwrap();
    let entries = first.result["entries"].as_array().unwrap();
    assert_eq!(entries.len(), TREE_PAGE_MAX_ENTRIES_V1);
    assert!(entries.iter().all(|entry| entry["kind"] == "directory"));
    assert_eq!(entries[0]["name"], "z-00000");
    let second = fixture
        .manager
        .get_tree(
            &fixture.storage,
            &fixture.project_id,
            "ordering",
            first.result["nextCursor"].as_str(),
        )
        .unwrap();
    for page in [&first.result, &second.result] {
        hello_agent_protocol::project_v1::validate_project_payload_v1("treePage", page).unwrap();
    }
    assert_eq!(second.result["entries"][0]["kind"], "directory");
    assert_eq!(second.result["entries"][1]["name"], "aaa.txt");
    assert!(second.result["nextCursor"].is_null());
}
