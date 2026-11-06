//! Point-in-time comparison only. Matching an after image is not writer ownership.
use super::*;
#[cfg(unix)]
use crate::project::application::capture_checked;
#[cfg(unix)]
use cap_std::fs::MetadataExt;
use std::path::Path;

#[allow(dead_code)]
pub(in crate::project) fn observe_pending_source(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
) -> Result<Value, ProjectFailure> {
    observe_pending_source_checked(manager, storage, project_id, &|| Ok(()))
}

/// Cancellation is advisory for this read-only observation; it never abandons
/// an intent or clears the lock. A writer must still recheck held objects.
pub(in crate::project) fn observe_pending_source_checked(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<Value, ProjectFailure> {
    check()?;
    let current = inspect_pending_item(manager, storage, project_id)?;
    let binding = &current["binding"];
    let root = binding["canonicalGitRoot"].as_str().ok_or_else(invalid)?;
    let project = required_project(storage, project_id)?;
    if project.get("canonical_path") != Some(&json!(root)) {
        return Err(invalid());
    }
    let source = manager.open_source_repository(Path::new(root))?;
    if crate::project::canonical_protocol_path(&source.canonical_root)? != root
        || binding["sourceIdentity"] != source.source_identity
        || binding["baseCommit"] != source.base_commit
        || binding["baseTree"] != source.base_tree
        || binding["objectFormat"] != source.object_format
    {
        return Err(invalid());
    }
    #[cfg(unix)]
    let raw = capture_checked(&source._root, check)?;
    #[cfg(windows)]
    let raw = crate::project::windows_source_inventory::capture(
        &source._root,
        &source.canonical_root,
        check,
    )?;
    let reopened = crate::project::open_absolute_directory_capability(&source.canonical_root)?;
    #[cfg(unix)]
    let meta = reopened.dir_metadata().map_err(|_| invalid())?;
    #[cfg(unix)]
    if crate::project::application::stamp(&meta) != raw.root_stamp {
        return Err(invalid());
    }
    #[cfg(unix)]
    let root_identity = json!([meta.dev(), meta.ino()]);
    #[cfg(windows)]
    let root_identity = {
        let identity = crate::project::windows_fs::identity(&reopened, true)?;
        if identity != raw.root_identity() {
            return Err(invalid());
        }
        json!(identity)
    };
    let item = &current["pendingItem"]["item"];
    let path = item["path"].as_str().ok_or_else(invalid)?;
    let mut parent = Path::new(path).parent();
    let mut parents_present = true;
    while let Some(p) = parent.filter(|p| !p.as_os_str().is_empty()) {
        if raw
            .entries
            .get(p.to_str().ok_or_else(invalid)?)
            .is_none_or(|e| e.kind() != 2)
        {
            parents_present = false;
            break;
        }
        parent = p.parent();
    }
    let entry = raw.entries.get(path);
    #[cfg(unix)]
    let before = entry.map(|e| json!({"kind":e.kind,"mode":e.mode,"size":e.size,"digest":e.digest,"identity":e.identity})).unwrap_or(Value::Null);
    #[cfg(unix)]
    let after = entry.map(|e| json!({"kind":e.kind,"executable":e.kind == 1 && e.mode & 0o100 != 0,"size":e.size,"digest":e.digest})).unwrap_or(Value::Null);
    #[cfg(windows)]
    let before = entry
        .map(|e| serde_json::to_value(e).map_err(|_| invalid()))
        .transpose()?
        .unwrap_or(Value::Null);
    #[cfg(windows)]
    let after = entry
        .map(|e| serde_json::to_value(Image::from(e)).map_err(|_| invalid()))
        .transpose()?
        .unwrap_or(Value::Null);
    let comparison = if !parents_present {
        "conflict"
    } else if before == item["before"] {
        "matchesBeforeEntry"
    } else if after == item["after"] {
        "matchesAfterImage"
    } else {
        "conflict"
    };
    check()?;
    if inspect_pending_item(manager, storage, project_id)? != current {
        return Err(invalid());
    }
    // No durable state changes: current bytes cannot prove which process wrote them.
    Ok(
        json!({"applicationId":current["applicationId"],"index":current["pendingItem"]["index"],"path":path,
        "comparison":comparison,"rootIdentity":root_identity,"observed":before}),
    )
}
