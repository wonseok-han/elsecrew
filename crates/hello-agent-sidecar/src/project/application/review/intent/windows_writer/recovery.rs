//! Explicit acknowledgement of an interrupted rename; never reruns a syscall.
use super::*;

fn interrupted_step(j: &Value) -> Result<Move, ProjectFailure> {
    match j["phase"].as_str() {
        Some("moveBeforeIntent") => Ok(Move::PreserveBefore),
        Some("installIntent") => Ok(Move::InstallAfter),
        Some("detachAfterIntent") => Ok(Move::DetachAfter),
        Some("restoreBeforeIntent") => Ok(Move::RestoreBefore),
        Some("cancelStagedIntent") => Ok(Move::CancelStaged),
        _ => Err(invalid()),
    }
}

fn observe_leaf(
    dir: &Dir,
    name: &str,
    writable: bool,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Option<(File, Value)>, ProjectFailure> {
    match dir.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(invalid()),
        Ok(_) => {}
    }
    let options = if writable {
        let mut options = windows_replace::source_options();
        options.create_new(false);
        options
    } else {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .share_mode(1)
            .custom_flags(windows_fs::OPEN_REPARSE_POINT);
        options
    };
    // Every present file remains pinned against writes/deletes through the
    // final comparison or acknowledgement publication.
    let mut file = dir.open_with(name, &options).map_err(|_| invalid())?;
    let observed = entry(&mut file, check)?;
    if entry(&mut open_real_file_at(dir, name)?, check)? != observed {
        return Err(invalid());
    }
    Ok(Some((file, observed)))
}

struct Evidence {
    journal: Value,
    comparison: &'static str,
    // Keep all protections until the caller has published its CAS result.
    _root: Dir,
    _parent: Dir,
    _lease: DirectoryLease,
    _source: Option<(File, Value)>,
    _destination: Option<(File, Value)>,
    _other: Option<File>,
}

fn observe(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    expected: &Value,
    flush_moved: bool,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Evidence, ProjectFailure> {
    check()?;
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut j = expected["pendingItem"]["writeJournal"].clone();
    validate_receipts(&j, &expected["pendingItem"]["item"])?;
    let step = interrupted_step(&j)?;
    let item = &expected["pendingItem"]["item"];
    let (root, dir, leaf) = source_parent(manager, expected)?;
    if json!(windows_fs::identity(&root, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&dir, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    let guard = name(&j, "guard")?;
    let lease = DirectoryLease::acquire(&dir, OsStr::new(&guard))?;
    if entry(&mut open_real_file_at(&dir, &guard)?, check)? != j["guardEntry"] {
        return Err(invalid());
    }
    let (_, intent, complete, from, to, expected_entry, receipt) =
        transition(&j, item, &leaf, step)?;
    if j["phase"] != intent || !expected_entry.is_object() {
        return Err(invalid());
    }
    let (other_name, other_entry, required) = match step {
        Move::PreserveBefore => (
            name(&j, "staged")?,
            j["stagedEntry"].clone(),
            !item["after"].is_null(),
        ),
        Move::InstallAfter | Move::DetachAfter | Move::CancelStaged => (
            name(&j, "retained")?,
            j["retainedEntry"].clone(),
            !item["before"].is_null(),
        ),
        Move::RestoreBefore => (
            name(&j, "detached")?,
            j["detachedEntry"].clone(),
            !item["after"].is_null(),
        ),
    };
    if other_entry.is_null() == required {
        return Err(invalid());
    }
    let mut other = pin_entry(&dir, &other_name, &other_entry, check)?;
    if matches!(step, Move::CancelStaged) {
        absent(&dir, &leaf)?;
    }
    let mut source = observe_leaf(&dir, &from, false, check)?;
    let mut destination = observe_leaf(&dir, &to, flush_moved, check)?;
    let comparison = match (&source, &destination) {
        (Some((_, before)), None) if *before == expected_entry => "matchesUnmoved",
        (None, Some((_, after))) if unchanged_by_move(&expected_entry, after) => "matchesMoved",
        _ => "conflict",
    };
    if flush_moved {
        if comparison != "matchesMoved" {
            return Err(invalid());
        }
        let (file, observed) = destination.as_mut().ok_or_else(invalid)?;
        windows_durability::require_ntfs(file)?;
        file.sync_all().map_err(|_| invalid())?;
        // A matching namespace alone is insufficient. Flush and reobserve the
        // same newly opened write-through handle before storing completion.
        if entry(file, check)? != *observed {
            return Err(invalid());
        }
        j[receipt] = observed.clone();
        j["phase"] = json!(complete);
    }
    for (name, held) in [(&from, &mut source), (&to, &mut destination)] {
        if let Some((file, observed)) = held {
            if entry(file, check)? != *observed
                || entry(&mut open_real_file_at(&dir, name)?, check)? != *observed
            {
                return Err(invalid());
            }
        } else {
            absent(&dir, name)?;
        }
    }
    verify_pin(&dir, &other_name, &other_entry, &mut other, check)?;
    if matches!(step, Move::CancelStaged) {
        absent(&dir, &leaf)?;
    }
    lease.validate()?;
    let rebound = open_absolute_directory_capability(Path::new(
        expected["binding"]["canonicalGitRoot"]
            .as_str()
            .ok_or_else(invalid)?,
    ))?;
    let (rebound_parent, _) = parent(&rebound, item["path"].as_str().ok_or_else(invalid)?)?;
    if json!(windows_fs::identity(&rebound, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&rebound_parent, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    check()?;
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    Ok(Evidence {
        journal: j,
        comparison,
        _root: root,
        _parent: dir,
        _lease: lease,
        _source: source,
        _destination: destination,
        _other: other,
    })
}

/// Read-only classification. It does not prove who performed a move or grant
/// permission to retry, complete the whole batch, or remove any file/lock.
pub(in crate::project) fn inspect_interrupted_move(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    expected: &Value,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    let evidence = observe(manager, storage, project_id, expected, false, check)?;
    Ok(
        json!({"applicationId":expected["applicationId"],"index":expected["pendingItem"]["index"],"phase":expected["pendingItem"]["writeJournal"]["phase"],"comparison":evidence.comparison}),
    )
}

/// Explicit request bound to the exact old record. A confirmed moved pair may
/// advance one journal phase; unmoved/conflicting pairs remain locked. No rename
/// or automatic cleanup occurs, and batch completion is a separate operation.
pub(in crate::project) fn acknowledge_interrupted_move(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    let evidence = observe(manager, storage, project_id, expected, true, check)?;
    check()?;
    publish(
        storage,
        project_id,
        expected,
        evidence.journal.clone(),
        request,
    )
}

/// Verify the Windows-specific settled layout. A restored item retains the
/// after-image under `detached`, not the macOS exchange name. This is a bounded
/// observation of one item, not batch completion or permission to delete files.
pub(in crate::project) fn inspect_settled_item(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    expected: &Value,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    check()?;
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let j = &expected["pendingItem"]["writeJournal"];
    validate_receipts(j, &expected["pendingItem"]["item"])?;
    let restored = match j["phase"].as_str() {
        Some("applied") => false,
        Some("restored") => true,
        _ => return Err(invalid()),
    };
    let (root, dir, leaf) = source_parent(manager, expected)?;
    if json!(windows_fs::identity(&root, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&dir, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    let guard = name(j, "guard")?;
    let lease = DirectoryLease::acquire(&dir, OsStr::new(&guard))?;
    let expected_names = [
        (guard, j["guardEntry"].clone()),
        (
            leaf,
            j[if restored {
                "restoredEntry"
            } else {
                "installedEntry"
            }]
            .clone(),
        ),
        (
            name(j, "retained")?,
            if restored {
                Value::Null
            } else {
                j["retainedEntry"].clone()
            },
        ),
        (
            name(j, "detached")?,
            if restored {
                j["detachedEntry"].clone()
            } else {
                Value::Null
            },
        ),
        (name(j, "staged")?, Value::Null),
    ];
    let mut pins = Vec::with_capacity(expected_names.len());
    for (name, expected_entry) in &expected_names {
        pins.push(pin_entry(&dir, name, expected_entry, check)?);
    }
    for ((name, expected_entry), held) in expected_names.iter().zip(pins.iter_mut()) {
        verify_pin(&dir, name, expected_entry, held, check)?;
    }
    lease.validate()?;
    let rebound = open_absolute_directory_capability(Path::new(
        expected["binding"]["canonicalGitRoot"]
            .as_str()
            .ok_or_else(invalid)?,
    ))?;
    let (rebound_parent, _) = parent(
        &rebound,
        expected["pendingItem"]["item"]["path"]
            .as_str()
            .ok_or_else(invalid)?,
    )?;
    if json!(windows_fs::identity(&rebound, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&rebound_parent, true)?) != j["parentIdentity"]
        || inspect_pending_item(manager, storage, project_id)? != *expected
    {
        return Err(invalid());
    }
    check()?;
    Ok(
        json!({"applicationId":expected["applicationId"],"index":expected["pendingItem"]["index"],
        "comparison": if restored { "matchesRestoredPair" } else { "matchesAppliedPair" }}),
    )
}

/// An install intent may have been committed before cancellation prevented its
/// syscall. Preserve that evidence and select a distinct cancellation phase;
/// never republish the earlier beforeMoved operation or retry installation.
pub(in crate::project) fn cancel_unmoved_install(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    if expected["pendingItem"]["writeJournal"]["phase"] != "installIntent" {
        return Err(invalid());
    }
    let mut evidence = observe(manager, storage, project_id, expected, false, check)?;
    if evidence.comparison != "matchesUnmoved" {
        return Err(invalid());
    }
    evidence.journal["cancelingBeforeInstall"] = json!(true);
    evidence.journal["phase"] = json!("cancelReady");
    check()?;
    publish(
        storage,
        project_id,
        expected,
        evidence.journal.clone(),
        request,
    )
}
