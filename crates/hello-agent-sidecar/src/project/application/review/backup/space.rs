//! Conservative free-space admission, not a reservation against other writers.
use super::*;
use crate::project::{available_space, SNAPSHOT_DISK_RESERVE_BYTES_V1};

const ENTRY_ALLOWANCE: u64 = 64 * 1024;

#[cfg(test)]
thread_local! { static TEST_FREE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) }; }

#[cfg(test)]
pub(in crate::project) fn with_free_space<T>(bytes: u64, run: impl FnOnce() -> T) -> T {
    struct Reset(Option<u64>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_FREE.with(|value| value.set(self.0));
        }
    }
    let _reset = Reset(TEST_FREE.with(|value| value.replace(Some(bytes))));
    run()
}

fn low_space() -> ProjectFailure {
    ProjectFailure::recovery("insufficient disk space for backup/staging and the 512 MiB reserve")
}

#[derive(Default)]
struct Volumes(BTreeMap<u64, (u64, u64)>);
impl Volumes {
    fn charge(&mut self, dir: &Dir, bytes: u64) -> Result<(), ProjectFailure> {
        let device = dir.dir_metadata().map_err(|_| invalid())?.dev();
        let free = available_space(dir)?;
        #[cfg(test)]
        let free = TEST_FREE.with(|value| value.get().unwrap_or(free));
        let entry = self.0.entry(device).or_insert((free, 0));
        entry.0 = entry.0.min(free);
        entry.1 = entry.1.checked_add(bytes).ok_or_else(low_space)?;
        Ok(())
    }
    fn verify(&self) -> Result<(), ProjectFailure> {
        for (free, bytes) in self.0.values() {
            let required = bytes
                .checked_add(SNAPSHOT_DISK_RESERVE_BYTES_V1)
                .ok_or_else(low_space)?;
            if *free < required {
                return Err(low_space());
            }
        }
        Ok(())
    }
}

// New directories inherit their nearest existing ancestor's filesystem. Existing
// parents are opened without following links, including nested mount points.
fn allocation_parent(root: &Dir, path: &str) -> Result<Dir, ProjectFailure> {
    let mut parts = path.split('/').collect::<Vec<_>>();
    if parts
        .iter()
        .any(|p| p.is_empty() || matches!(*p, "." | "..") || p.eq_ignore_ascii_case(".git"))
    {
        return Err(invalid());
    }
    parts.pop().ok_or_else(invalid)?;
    let mut dir = root.try_clone().map_err(|_| invalid())?;
    for part in parts {
        match dir.symlink_metadata(part) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(_) => return Err(invalid()),
            Ok(meta) if meta.is_dir() => {
                dir = Dir::from_std_file(open(&dir, part, true)?.into_std())
            }
            Ok(_) => return Err(invalid()),
        }
    }
    Ok(dir)
}

pub(super) fn plan(manager: &ProjectManager, plan: &ReviewPlan) -> Result<(), ProjectFailure> {
    let source = manager.open_source_repository(std::path::Path::new(
        plan.binding["canonicalGitRoot"]
            .as_str()
            .ok_or_else(invalid)?,
    ))?;
    let mut volumes = Volumes::default();
    let mut archive = MAX_REVIEW_BYTES as u64;
    for (path, change) in &plan.changes {
        manager.check_application_preparation()?;
        let before = change.before.as_ref().map_or(0, |image| image.size);
        let after = change.after.as_ref().map_or(0, |image| image.size);
        archive = archive.checked_add(before).ok_or_else(low_space)?;
        // The before image is renamed in place, not copied on the source volume.
        let staging = after.checked_add(ENTRY_ALLOWANCE).ok_or_else(low_space)?;
        volumes.charge(&allocation_parent(&source._root, path)?, staging)?;
    }
    volumes.charge(&directory(manager)?, archive)?;
    volumes.verify()
}

pub(in crate::project) fn writer_parent(dir: &Dir, bytes: u64) -> Result<(), ProjectFailure> {
    let mut volumes = Volumes::default();
    volumes.charge(
        dir,
        bytes.checked_add(ENTRY_ALLOWANCE).ok_or_else(low_space)?,
    )?;
    volumes.verify()
}

/// Recheck a durable, verified archive before entering a writing phase. The
/// archive already exists, so only remaining staging and metadata are charged.
pub(in crate::project) fn intent(
    manager: &ProjectManager,
    expected: &Value,
    restoring: bool,
) -> Result<(), ProjectFailure> {
    let archive = Backup::from_value(&expected["backup"]["backup"])?;
    let (header, items) = verify_items(manager, &archive)?;
    if header != expected["backup"]["header"] || header["binding"] != expected["binding"] {
        return Err(invalid());
    }
    let source = manager.open_source_repository(std::path::Path::new(
        header["binding"]["canonicalGitRoot"]
            .as_str()
            .ok_or_else(invalid)?,
    ))?;
    let mut volumes = Volumes::default();
    for item in items {
        manager.check_application_preparation()?;
        // Restore swaps retained names and allocates no new payload. Apply
        // stages only after; before and the archive already occupy disk space.
        let after = if restoring || item["after"].is_null() {
            0
        } else {
            item["after"]["size"].as_u64().ok_or_else(invalid)?
        };
        let bytes = after.checked_add(ENTRY_ALLOWANCE).ok_or_else(low_space)?;
        volumes.charge(
            &allocation_parent(&source._root, item["path"].as_str().ok_or_else(invalid)?)?,
            bytes,
        )?;
    }
    volumes.charge(&directory(manager)?, 0)?;
    volumes.verify()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn volume_totals_keep_reserve_and_reject_overflow() {
        let reserve = SNAPSHOT_DISK_RESERVE_BYTES_V1;
        assert!(Volumes(BTreeMap::from([(1, (reserve + 30, 30))]))
            .verify()
            .is_ok());
        assert!(Volumes(BTreeMap::from([(1, (reserve + 29, 30))]))
            .verify()
            .is_err());
        assert!(Volumes(BTreeMap::from([
            (1, (reserve + 10, 10)),
            (2, (reserve + 19, 20))
        ]))
        .verify()
        .is_err());
        assert!(Volumes(BTreeMap::from([(1, (u64::MAX, u64::MAX))]))
            .verify()
            .is_err());
    }
}
