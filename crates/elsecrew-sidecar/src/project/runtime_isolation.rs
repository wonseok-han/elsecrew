//! Admission of existing runtime files; never follows links or repairs data.
use super::*;

#[cfg(unix)]
pub(crate) fn verify(root: &Dir) -> Result<(), ProjectFailure> {
    let mut remaining = SNAPSHOT_ENTRIES_MAX_V1;
    scan(root, 0, &mut remaining)
}

#[cfg(unix)]
fn scan(root: &Dir, depth: usize, remaining: &mut u64) -> Result<(), ProjectFailure> {
    use cap_std::fs::MetadataExt;
    if depth > 128 {
        return Err(ProjectFailure::recovery(
            "runtime file scan exceeds depth budget",
        ));
    }
    for entry in root
        .entries()
        .map_err(|_| ProjectFailure::recovery("runtime directory listing failed"))?
    {
        let entry =
            entry.map_err(|_| ProjectFailure::recovery("runtime directory entry unavailable"))?;
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| ProjectFailure::recovery("runtime file scan exceeds entry budget"))?;
        let name = entry.file_name();
        let meta = root
            .symlink_metadata(&name)
            .map_err(|_| ProjectFailure::recovery("runtime entry metadata unavailable"))?;
        if meta.is_file() && meta.nlink() == 1 {
            continue;
        }
        if meta.is_dir() && !meta.file_type().is_symlink() {
            let child = open_real_os_child_dir(root, &name).map_err(|mut error| {
                error.detail = "runtime directory capability unavailable".into();
                error
            })?;
            scan(&child, depth + 1, remaining)?;
        } else {
            return Err(ProjectFailure::recovery(if meta.file_type().is_symlink() {
                "runtime symbolic link rejected"
            } else if meta.is_file() {
                "runtime shared regular file rejected"
            } else {
                "runtime special file rejected"
            }));
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn runtime_scan_includes_hidden_git_and_nested_files_without_following_links() {
        for location in ["work", ".hidden", ".git/objects/item", "nested/item"] {
            let temp = tempfile::tempdir().unwrap();
            let outside = temp.path().join("original");
            fs::write(&outside, b"original").unwrap();
            let path = temp.path().join("workspace");
            fs::create_dir(&path).unwrap();
            let root = Dir::open_ambient_dir(&path, ambient_authority()).unwrap();
            let target = path.join(location);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(&outside, &target).unwrap();
            verify(&root).unwrap();
            fs::remove_file(&target).unwrap();
            fs::hard_link(&outside, &target).unwrap();
            assert_eq!(
                verify(&root).unwrap_err().detail,
                "runtime shared regular file rejected"
            );
            fs::remove_file(&target).unwrap();
            std::os::unix::fs::symlink(&outside, &target).unwrap();
            assert_eq!(
                verify(&root).unwrap_err().detail,
                "runtime symbolic link rejected"
            );
            assert_eq!(fs::read(&outside).unwrap(), b"original");
        }
    }

    #[test]
    fn runtime_scan_rejects_exhausted_budgets_and_directory_links() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::write(temp.path().join("file"), b"data").unwrap();
        assert!(scan(&root, 0, &mut 0).is_err());
        assert!(scan(&root, 129, &mut 10).is_err());
        std::os::unix::fs::symlink(temp.path(), temp.path().join("loop")).unwrap();
        assert!(verify(&root).is_err());
    }

    #[test]
    fn runtime_scan_reports_special_files_without_exposing_names() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let _socket =
            std::os::unix::net::UnixListener::bind(temp.path().join("private-name")).unwrap();
        assert_eq!(
            verify(&root).unwrap_err().detail,
            "runtime special file rejected"
        );
    }
}
