//! Internal macOS writer candidate. Not reachable from product IPC until the
//! complete apply/restore flow and independent safety review are finished.
//! External editors must be stopped. On uncertainty preserve both names and lock.
use super::*;
use crate::project::application::{open, stamp};
use crate::project::{open_absolute_directory_capability, sync_capability_directory};
use cap_std::fs::{Dir, File, MetadataExt};
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;

fn parent(root: &Dir, path: &str) -> Result<(Dir, String), ProjectFailure> {
    let mut parts: Vec<_> = path.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || matches!(*p, "." | ".." | ".git"))
    {
        return Err(invalid());
    }
    let leaf = parts.pop().ok_or_else(invalid)?.to_owned();
    let mut dir = root.try_clone().map_err(|_| invalid())?;
    for part in parts {
        dir = Dir::from_std_file(open(&dir, part, true)?.into_std());
    }
    Ok((dir, leaf))
}

fn image(dir: &Dir, name: &str) -> Result<Value, ProjectFailure> {
    let meta = match dir.symlink_metadata(name) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(_) => return Err(invalid()),
    };
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > super::super::super::MAX_BYTES {
        return Err(invalid());
    }
    let mut file = open(dir, name, false)?;
    if stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?) {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .filter(|v| *v <= meta.len())
            .ok_or_else(invalid)?;
        hash.update(&buffer[..n]);
    }
    if count != meta.len()
        || stamp(&meta) != stamp(&file.metadata().map_err(|_| invalid())?)
        || stamp(&meta) != stamp(&dir.symlink_metadata(name).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    let digest: [u8; 32] = hash.finalize().into();
    Ok(
        json!({"kind":1,"mode":meta.mode(),"size":count,"digest":digest,"identity":[meta.dev(),meta.ino(),meta.nlink()]}),
    )
}

fn matches_after(actual: &Value, expected: &Value) -> bool {
    if actual.is_null() || expected.is_null() {
        return actual == expected;
    }
    actual["kind"] == expected["kind"]
        && actual["size"] == expected["size"]
        && actual["digest"] == expected["digest"]
        && actual["mode"].as_u64().map(|m| m & 0o100 != 0) == expected["executable"].as_bool()
}

fn publish(
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    journal: Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect(storage, project_id)? != *expected {
        return Err(invalid());
    }
    let mut project = required_project(storage, project_id)?;
    let revision = project["last_event_sequence"]
        .as_u64()
        .ok_or_else(invalid)?;
    let lock_id = expected["lockId"].as_str().ok_or_else(invalid)?;
    let lock = storage
        .projection_record("lock", lock_id)?
        .ok_or_else(invalid)?;
    let lock_revision = lock["last_event_sequence"].as_u64().ok_or_else(invalid)?;
    let phase = journal["phase"].as_str().ok_or_else(invalid)?.to_owned();
    let mut next = expected.clone();
    next["pendingItem"]["writeJournal"] = journal;
    project.insert("application_intent".into(), next.clone());
    storage.upsert_projection_records_atomic_checked(
        vec![crate::storage::ProjectionUpsert {
            aggregate_type: "project",
            aggregate_id: project_id.to_owned(),
            operation_id: format!(
                "op_{}_item_0_{phase}",
                expected["applicationId"].as_str().ok_or_else(invalid)?
            ),
            request_id: Some(request.to_owned()),
            payload: project,
        }],
        &[
            ("project", project_id, revision),
            ("lock", lock_id, lock_revision),
        ],
    )?;
    Ok(next)
}

/// One-shot execution only. Existing journals require explicit recovery, never
/// a blind retry. A failed CAS after filesystem mutation keeps the staged journal.
#[allow(dead_code)]
pub(in crate::project) fn write_first_regular_item(
    manager: &ProjectManager,
    storage: &mut Storage,
    project_id: &str,
    expected: &Value,
    request: &str,
) -> Result<Value, ProjectFailure> {
    if inspect_pending_item(manager, storage, project_id)? != *expected
        || expected["pendingItem"].get("writeJournal").is_some()
    {
        return Err(invalid());
    }
    let item = &expected["pendingItem"]["item"];
    for side in ["before", "after"] {
        if !item[side].is_null() && item[side]["kind"] != 1 {
            return Err(invalid());
        }
    }
    if item["before"].is_null() && item["after"].is_null() {
        return Err(invalid());
    }
    let observation = observe_pending_source(manager, storage, project_id)?;
    if observation["comparison"] != "matchesBeforeEntry" {
        return Err(invalid());
    }
    let root_path = Path::new(
        expected["binding"]["canonicalGitRoot"]
            .as_str()
            .ok_or_else(invalid)?,
    );
    let root = open_absolute_directory_capability(root_path)?;
    let root_identity = root.dir_metadata().map_err(|_| invalid())?;
    if observation["rootIdentity"] != json!([root_identity.dev(), root_identity.ino()]) {
        return Err(invalid());
    }
    let path = item["path"].as_str().ok_or_else(invalid)?;
    let (dir, leaf) = parent(&root, path)?;
    let parent_identity = dir.dir_metadata().map_err(|_| invalid())?;
    let retained = format!(".hello-agent-apply-{}", Uuid::new_v4().simple());
    let mut journal = json!({"version":1,"phase":"reserved","retainedName":retained,
        "rootIdentity":[root_identity.dev(),root_identity.ino()],
        "parentIdentity":[parent_identity.dev(),parent_identity.ino()],"stagedImage":null});
    let mut current = publish(storage, project_id, expected, journal.clone(), request)?;
    if !item["after"].is_null() {
        let snapshot = open_absolute_directory_capability(Path::new(
            expected["worktreePath"].as_str().ok_or_else(invalid)?,
        ))?;
        let (input_dir, input_leaf) = parent(&snapshot, path)?;
        let input_image = image(&input_dir, &input_leaf)?;
        if !matches_after(&input_image, &item["after"]) {
            return Err(invalid());
        }
        let mut input = open(&input_dir, &input_leaf, false)?;
        let name = CString::new(retained.as_str()).map_err(|_| invalid())?;
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(invalid());
        }
        let mut output = File::from_std(unsafe { std::fs::File::from_raw_fd(fd) });
        let size = item["after"]["size"].as_u64().ok_or_else(invalid)?;
        let copied =
            std::io::copy(&mut (&mut input).take(size + 1), &mut output).map_err(|_| invalid())?;
        let mode = if item["before"].is_null() {
            0o644
        } else {
            item["before"]["mode"].as_u64().ok_or_else(invalid)? & 0o777
        };
        let mode = if item["after"]["executable"] == true {
            mode | 0o100
        } else {
            mode & !0o100
        };
        if copied != size || unsafe { libc::fchmod(output.as_raw_fd(), mode as libc::mode_t) } != 0
        {
            return Err(invalid());
        }
        output.flush().map_err(|_| invalid())?;
        output.sync_all().map_err(|_| invalid())?;
        let staged = image(&dir, &retained)?;
        if !matches_after(&staged, &item["after"]) || image(&input_dir, &input_leaf)? != input_image
        {
            return Err(invalid());
        }
        journal["stagedImage"] = staged;
    }
    sync_capability_directory(&dir)?;
    journal["phase"] = json!("staged");
    current = publish(storage, project_id, &current, journal.clone(), request)?;
    let reopened = open_absolute_directory_capability(root_path)?;
    let (linked_parent, _) = parent(&reopened, path)?;
    let linked_root = reopened.dir_metadata().map_err(|_| invalid())?;
    let linked = linked_parent.dir_metadata().map_err(|_| invalid())?;
    if (linked_root.dev(), linked_root.ino()) != (root_identity.dev(), root_identity.ino())
        || (linked.dev(), linked.ino()) != (parent_identity.dev(), parent_identity.ino())
        || image(&dir, &leaf)? != item["before"]
        || image(&dir, &retained)? != journal["stagedImage"]
    {
        return Err(invalid());
    }
    let target = CString::new(leaf.as_str()).map_err(|_| invalid())?;
    let saved = CString::new(retained.as_str()).map_err(|_| invalid())?;
    let status = unsafe {
        if item["after"].is_null() {
            libc::renameatx_np(
                dir.as_raw_fd(),
                target.as_ptr(),
                dir.as_raw_fd(),
                saved.as_ptr(),
                libc::RENAME_EXCL,
            )
        } else {
            libc::renameatx_np(
                dir.as_raw_fd(),
                saved.as_ptr(),
                dir.as_raw_fd(),
                target.as_ptr(),
                if item["before"].is_null() {
                    libc::RENAME_EXCL
                } else {
                    libc::RENAME_SWAP
                },
            )
        }
    };
    if status != 0 {
        return Err(invalid());
    }
    sync_capability_directory(&dir)?;
    // A post-rename conflict never triggers destructive cleanup or automatic undo.
    let after = image(&dir, &leaf)?;
    let before = image(&dir, &retained)?;
    if after != journal["stagedImage"] || before != item["before"] {
        return Err(invalid());
    }
    let check = parent(&open_absolute_directory_capability(root_path)?, path)?
        .0
        .dir_metadata()
        .map_err(|_| invalid())?;
    if (check.dev(), check.ino()) != (parent_identity.dev(), parent_identity.ino()) {
        return Err(invalid());
    }
    journal["phase"] = json!("applied");
    publish(storage, project_id, &current, journal, request)
}
