//! Cooperative backup budget ownership. No archive or user file is deleted.
use super::{
    create_managed_directory, open_real_file_at, windows_fs,
    windows_source_inventory::file_observation, ProjectFailure, ProjectManager,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use serde_json::{json, Value};
use std::io::Read;

const GIB: u64 = 1024 * 1024 * 1024;
fn invalid() -> ProjectFailure {
    ProjectFailure::recovery("backup storage is busy, changed, or has invalid settings")
}
fn valid(limit: u64) -> bool {
    (GIB..=1024 * GIB).contains(&limit) && limit % GIB == 0
}

pub(super) struct BudgetGuard {
    _lock: File,
    pub(super) directory: Dir,
    pub(super) used: u64,
    pub(super) limit: u64,
}
impl BudgetGuard {
    pub(super) fn acquire(manager: &ProjectManager) -> Result<Self, ProjectFailure> {
        manager.verify_managed_paths()?;
        // This dedicated lock stays outside the archive directory. Sharing denied
        // for its lifetime serializes inspection, budget changes and future writes.
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .follow(FollowSymlinks::No)
            .custom_flags(windows_fs::OPEN_REPARSE_POINT);
        let lock = manager
            .data_dir
            .open_with("application-backup-budget.lock", &options)
            .map_err(|_| invalid())?;
        file_observation(&lock)?;
        let (_, directory) = create_managed_directory(
            &manager.data_root,
            &manager.data_dir,
            "application-backups-v1",
        )?;
        let before = windows_fs::identity(&directory, true)?;
        let mut used = 0u64;
        for (index, entry) in directory.entries().map_err(|_| invalid())?.enumerate() {
            manager.check_application_preparation()?;
            if index >= 100_000 {
                return Err(invalid());
            }
            let name = entry
                .map_err(|_| invalid())?
                .file_name()
                .into_string()
                .map_err(|_| invalid())?;
            super::validate_relative_path(&name, false)?;
            if name.contains(['/', '\\']) {
                return Err(invalid());
            }
            super::protocol_path::from_native(&format!("C:/{name}"), true)
                .map_err(ProjectFailure::manifest)?;
            let file = open_real_file_at(&directory, &name)?;
            let observation = file_observation(&file)?;
            used = used.checked_add(observation.length).ok_or_else(invalid)?;
            if observation != file_observation(&open_real_file_at(&directory, &name)?)? {
                return Err(invalid());
            }
        }
        let (_, rebound) = create_managed_directory(
            &manager.data_root,
            &manager.data_dir,
            "application-backups-v1",
        )?;
        if windows_fs::identity(&rebound, true)? != before {
            return Err(invalid());
        }
        manager.verify_managed_paths()?;
        let limit = read_limit(manager)?;
        Ok(Self {
            _lock: lock,
            directory,
            used,
            limit,
        })
    }
    pub(super) fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }
}
fn settings(manager: &ProjectManager) -> Result<Dir, ProjectFailure> {
    Ok(create_managed_directory(
        &manager.data_root,
        &manager.data_dir,
        "application-settings-v1",
    )?
    .1)
}
fn read_limit(manager: &ProjectManager) -> Result<u64, ProjectFailure> {
    let directory = settings(manager)?;
    match directory.symlink_metadata("restore-budget.json") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(5 * GIB),
        Err(_) => return Err(invalid()),
        Ok(_) => {}
    }
    let mut file = open_real_file_at(&directory, "restore-budget.json")?;
    let before = file_observation(&file)?;
    if before.length > 256 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(257)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 != before.length
        || file_observation(&file)? != before
        || file_observation(&open_real_file_at(&directory, "restore-budget.json")?)? != before
    {
        return Err(invalid());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let limit = value["limitBytes"].as_u64().ok_or_else(invalid)?;
    if value.as_object().map(|v| v.len()) != Some(2) || value["version"] != 1 || !valid(limit) {
        return Err(invalid());
    }
    Ok(limit)
}
pub(super) fn usage(manager: &ProjectManager) -> Result<Value, ProjectFailure> {
    let guard = BudgetGuard::acquire(manager)?;
    if guard.used > 9_007_199_254_740_991 {
        return Err(invalid());
    }
    Ok(json!({"usedBytes":guard.used,"limitBytes":guard.limit,"remainingBytes":guard.remaining()}))
}
pub(super) fn set_limit(
    manager: &ProjectManager,
    expected: u64,
    limit: u64,
) -> Result<(), ProjectFailure> {
    let guard = BudgetGuard::acquire(manager)?;
    if !valid(limit) || guard.limit != expected {
        return Err(invalid());
    }
    super::write_json_atomic(
        &settings(manager)?,
        "restore-budget.json",
        &json!({"version":1,"limitBytes":limit}),
    )?;
    if read_limit(manager)? != limit {
        return Err(invalid());
    }
    Ok(())
}
