//! App-owned budget only. Callers hold the archive directory flock through reads/writes.
use super::*;

const FILE: &str = "restore-budget.json";
const GIB: u64 = 1024 * 1024 * 1024;

fn directory(manager: &ProjectManager) -> Result<Dir, ProjectFailure> {
    manager.verify_managed_paths()?;
    Ok(create_managed_directory(
        &manager.data_root,
        &manager.data_dir,
        "application-settings-v1",
    )?
    .1)
}

fn valid(limit: u64) -> bool {
    (GIB..=1024 * GIB).contains(&limit) && limit % GIB == 0
}

pub(super) fn read(manager: &ProjectManager) -> Result<u64, ProjectFailure> {
    read_from(&directory(manager)?)
}

fn read_from(dir: &Dir) -> Result<u64, ProjectFailure> {
    let name = CString::new(FILE).map_err(|_| invalid())?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return if io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
            Ok(RESTORE_BUDGET)
        } else {
            Err(invalid())
        };
    }
    let file = File::from_std(unsafe { std::fs::File::from_raw_fd(fd) });
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 256 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(257)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > 256 {
        return Err(invalid());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let limit = value["limitBytes"].as_u64().ok_or_else(invalid)?;
    if value.as_object().map(|v| v.len()) != Some(2) || value["version"] != 1 || !valid(limit) {
        return Err(invalid());
    }
    Ok(limit)
}

pub(super) fn set(
    manager: &ProjectManager,
    expected: u64,
    limit: u64,
) -> Result<(), ProjectFailure> {
    let dir = directory(manager)?;
    if !valid(limit) || read_from(&dir)? != expected {
        return Err(ProjectFailure::recovery(
            "backup budget changed or requested limit is invalid; inspect again",
        ));
    }
    crate::project::write_json_atomic(
        &dir,
        FILE,
        &serde_json::json!({"version":1,"limitBytes":limit}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_saved_limit_and_corrupt_settings() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        assert_eq!(read_from(&dir).unwrap(), RESTORE_BUDGET);
        crate::project::write_json_atomic(
            &dir,
            FILE,
            &serde_json::json!({"version":1,"limitBytes":10*GIB}),
        )
        .unwrap();
        assert_eq!(read_from(&dir).unwrap(), 10 * GIB);
        crate::project::write_json_atomic(
            &dir,
            FILE,
            &serde_json::json!({"version":1,"limitBytes":0}),
        )
        .unwrap();
        assert!(read_from(&dir).is_err());
        for limit in [0, GIB - 1, GIB + 1, 1025 * GIB] {
            assert!(!valid(limit));
        }
    }
}
