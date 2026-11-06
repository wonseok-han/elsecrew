//! Read-only inspection and explicit recovery of journaled moves.
//! New apply execution cannot be resumed from a persisted journal.
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObservationMode {
    ReadOnly,
    FlushMoved,
    ResumeUnmovedRestore,
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
    other_name: String,
    other_entry: Value,
}

fn observe(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    expected: &Value,
    mode: ObservationMode,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Evidence, ProjectFailure> {
    check()?;
    if mode != ObservationMode::ReadOnly && expected["state"] != "applying" {
        return Err(invalid());
    }
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut j = expected["pendingItem"]["writeJournal"].clone();
    validate_receipts(&j, &expected["pendingItem"]["item"])?;
    let step = interrupted_step(&j)?;
    if mode == ObservationMode::ResumeUnmovedRestore
        && matches!(step, Move::PreserveBefore | Move::InstallAfter)
    {
        return Err(invalid());
    }
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
    let mut source = observe_leaf(
        &dir,
        &from,
        mode == ObservationMode::ResumeUnmovedRestore,
        check,
    )?;
    let mut destination = observe_leaf(&dir, &to, mode == ObservationMode::FlushMoved, check)?;
    let comparison = match (&source, &destination) {
        (Some((_, before)), None) if *before == expected_entry => "matchesUnmoved",
        (None, Some((_, after))) if unchanged_by_move(&expected_entry, after) => "matchesMoved",
        _ => "conflict",
    };
    if mode == ObservationMode::ResumeUnmovedRestore && comparison != "matchesUnmoved" {
        return Err(invalid());
    }
    if mode == ObservationMode::FlushMoved {
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
        other_name,
        other_entry,
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
    let evidence = observe(
        manager,
        storage,
        project_id,
        expected,
        ObservationMode::ReadOnly,
        check,
    )?;
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
    let evidence = observe(
        manager,
        storage,
        project_id,
        expected,
        ObservationMode::FlushMoved,
        check,
    )?;
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
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let restored = verify_bound_item_layout(manager, expected, check)?;
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    Ok(
        json!({"applicationId":expected["applicationId"],"index":expected["pendingItem"]["index"],
        "comparison": if restored { "matchesRestoredPair" } else { "matchesAppliedPair" }}),
    )
}

/// Exact settled leaf layout, shared with newly-created directory child checks.
pub(super) fn settled_names(
    j: &Value,
    leaf: String,
) -> Result<[(String, Value); 5], ProjectFailure> {
    validate(j)?;
    let restored = match j["phase"].as_str() {
        Some("applied") => false,
        Some("restored") => true,
        _ => return Err(invalid()),
    };
    let guard = name(j, "guard")?;
    Ok([
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
        (
            name(j, "staged")?,
            if j["canceledUnwritten"] == true || j["canceledReservation"] == true {
                j["stagedEntry"].clone()
            } else {
                Value::Null
            },
        ),
    ])
}

/// Verify one archive-bound record selected by the batch verifier. This helper
/// performs filesystem checks only: its caller must validate the full ledger and
/// archive before selecting a completed/restored item and before publication.
pub(in crate::project) fn verify_bound_item_layout(
    manager: &ProjectManager,
    expected: &Value,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<bool, ProjectFailure> {
    check()?;
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
    let expected_names = settled_names(j, leaf)?;
    let mut pins = Vec::with_capacity(expected_names.len());
    for (name, expected_entry) in &expected_names {
        pins.push(pin_entry(&dir, name, expected_entry, check)?);
    }
    for ((name, expected_entry), held) in expected_names.iter().zip(pins.iter_mut()) {
        verify_pin(&dir, name, expected_entry, held, check)?;
    }
    // Normal journals always have a guard. A canceled empty reservation may
    // have no existing child at all; this path only observes and never mutates.
    let lease = expected_names
        .iter()
        .zip(pins.iter())
        .find_map(|((name, _), held)| held.as_ref().map(|_| name))
        .map(|name| DirectoryLease::acquire(&dir, OsStr::new(name)))
        .transpose()?;
    if let Some(lease) = &lease {
        lease.validate()?;
    }
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
    {
        return Err(invalid());
    }
    check()?;
    Ok(restored)
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
    let mut evidence = observe(
        manager,
        storage,
        project_id,
        expected,
        ObservationMode::ReadOnly,
        check,
    )?;
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

/// Cancel a prepared item only while the exact original is still at its leaf.
/// This also handles a preserve intent whose syscall never happened. Prepared
/// files remain in place under their receipts; cleanup and batch unlock remain
/// separate. A partial reservation without staged receipts is not accepted.
pub(in crate::project) fn cancel_unwritten_item(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    check()?;
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut j = expected["pendingItem"]["writeJournal"].clone();
    let item = &expected["pendingItem"]["item"];
    validate_receipts(&j, item)?;
    if !matches!(j["phase"].as_str(), Some("staged" | "moveBeforeIntent")) {
        return Err(invalid());
    }
    let (root, dir, leaf) = source_parent(manager, expected)?;
    if json!(windows_fs::identity(&root, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&dir, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    let guard = name(&j, "guard")?;
    let lease = DirectoryLease::acquire(&dir, OsStr::new(&guard))?;
    let expected_names = [
        (guard, j["guardEntry"].clone()),
        (leaf, item["before"].clone()),
        (name(&j, "staged")?, j["stagedEntry"].clone()),
        (name(&j, "retained")?, Value::Null),
        (name(&j, "detached")?, Value::Null),
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
    j["canceledUnwritten"] = json!(true);
    j["restoredEntry"] = item["before"].clone();
    j["phase"] = json!("restored");
    publish(storage, project_id, expected, j, request)
}

/// Only an explicit restore owner may request this. Reobserve and hold the exact
/// unmoved source with rename rights; do not convert an apply intent into write
/// permission. The existing durable restore intent precedes this syscall too.
pub(in crate::project) fn resume_interrupted_restore(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    let mut evidence = observe(
        manager,
        storage,
        project_id,
        expected,
        ObservationMode::ResumeUnmovedRestore,
        check,
    )?;
    let mut j = evidence.journal.clone();
    let step = interrupted_step(&j)?;
    if !matches!(
        step,
        Move::DetachAfter | Move::CancelStaged | Move::RestoreBefore
    ) {
        return Err(invalid());
    }
    let item = &expected["pendingItem"]["item"];
    let (_, leaf) = parent(&evidence._root, item["path"].as_str().ok_or_else(invalid)?)?;
    let (_, intent, complete, from, to, expected_entry, receipt) =
        transition(&j, item, &leaf, step)?;
    if j["phase"] != intent
        || evidence.comparison != "matchesUnmoved"
        || evidence._destination.is_some()
    {
        return Err(invalid());
    }
    let (file, observed) = evidence._source.as_mut().ok_or_else(invalid)?;
    if *observed != expected_entry || entry(file, check)? != expected_entry {
        return Err(invalid());
    }
    windows_durability::require_ntfs(file)?;
    absent(&evidence._parent, &to)?;
    evidence._lease.validate()?;
    check()?;
    if inspect_pending_item(manager, storage, project_id)? != *expected {
        return Err(invalid());
    }
    windows_replace::move_new(file, &evidence._lease, &to)?;
    file.sync_all().map_err(|_| invalid())?;
    let moved = entry(file, check)?;
    if !unchanged_by_move(&expected_entry, &moved)
        || entry(&mut open_real_file_at(&evidence._parent, &to)?, check)? != moved
    {
        return Err(invalid());
    }
    absent(&evidence._parent, &from)?;
    if matches!(step, Move::CancelStaged) {
        absent(&evidence._parent, &leaf)?;
    }
    verify_pin(
        &evidence._parent,
        &evidence.other_name,
        &evidence.other_entry,
        &mut evidence._other,
        check,
    )?;
    evidence._lease.validate()?;
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
    j[receipt] = moved;
    j["phase"] = json!(complete);
    publish(storage, project_id, expected, j, request)
}

/// Reconcile a partial reservation without deleting or completing any staging
/// file. Captured partial entries are observations, not successful after-images.
pub(in crate::project) fn cancel_reserved_staging(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    check()?;
    if expected["state"] != "applying"
        || inspect_pending_item(manager, storage, project_id)? != *expected
    {
        return Err(invalid());
    }
    let mut j = expected["pendingItem"]["writeJournal"].clone();
    let item = &expected["pendingItem"]["item"];
    validate_receipts(&j, item)?;
    if j["phase"] != "reserved" {
        return Err(invalid());
    }
    let (root, dir, leaf) = source_parent(manager, expected)?;
    if json!(windows_fs::identity(&root, true)?) != j["rootIdentity"]
        || json!(windows_fs::identity(&dir, true)?) != j["parentIdentity"]
    {
        return Err(invalid());
    }
    let mut original = pin_entry(&dir, &leaf, &item["before"], check)?;
    let guard_name = name(&j, "guard")?;
    let staged_name = name(&j, "staged")?;
    let mut guard = observe_leaf(&dir, &guard_name, false, check)?;
    let mut staged = observe_leaf(&dir, &staged_name, false, check)?;
    let lease_name = if guard.is_some() {
        Some(&guard_name)
    } else if staged.is_some() {
        Some(&staged_name)
    } else if original.is_some() {
        Some(&leaf)
    } else {
        None
    };
    let lease = lease_name
        .map(|name| DirectoryLease::acquire(&dir, OsStr::new(name)))
        .transpose()?;
    absent(&dir, &name(&j, "retained")?)?;
    absent(&dir, &name(&j, "detached")?)?;
    for (name, held) in [(&guard_name, &mut guard), (&staged_name, &mut staged)] {
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
    verify_pin(&dir, &leaf, &item["before"], &mut original, check)?;
    if let Some(lease) = &lease {
        lease.validate()?;
    }
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
    j["guardEntry"] = guard
        .as_ref()
        .map(|(_, value)| value.clone())
        .unwrap_or(Value::Null);
    j["stagedEntry"] = staged
        .as_ref()
        .map(|(_, value)| value.clone())
        .unwrap_or(Value::Null);
    j["restoredEntry"] = item["before"].clone();
    j["canceledReservation"] = json!(true);
    j["phase"] = json!("restored");
    publish(storage, project_id, expected, j, request)
}
