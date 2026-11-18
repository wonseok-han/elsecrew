//! Admission of existing runtime files; never follows links or repairs data.
use super::*;

#[cfg(unix)]
pub(crate) fn verify(root: &Dir) -> Result<(), ProjectFailure> {
    let mut remaining = SNAPSHOT_ENTRIES_MAX_V1;
    scan(root, 0, &mut remaining, false)
}

/// A real repository (task worktree) may hold symbolic links: tracked ones,
/// or tool-made ones such as `node_modules/.bin/*`. A relative link that
/// stays inside the workspace is allowed; an absolute or escaping one, a
/// shared (hard-linked) file or a special file is still refused.
#[cfg(unix)]
pub(crate) fn verify_allowing_inner_links(root: &Dir) -> Result<(), ProjectFailure> {
    let mut remaining = SNAPSHOT_ENTRIES_MAX_V1;
    scan(root, 0, &mut remaining, true)
}

/// Whether a relative link target, read from a directory `depth` levels
/// below the workspace root, stays inside the workspace.
fn stays_inside(target: &std::path::Path, depth: usize) -> bool {
    let mut level = depth as isize;
    for component in target.components() {
        match component {
            std::path::Component::Normal(_) => level += 1,
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                level -= 1;
                if level < 0 { return false; }
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return false,
        }
    }
    !target.as_os_str().is_empty()
}

#[cfg(unix)]
fn scan(root: &Dir, depth: usize, remaining: &mut u64, inner_links: bool) -> Result<(), ProjectFailure> {
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
            scan(&child, depth + 1, remaining, inner_links)?;
        } else if inner_links && meta.file_type().is_symlink()
            && root.read_link_contents(&name).is_ok_and(|target| stays_inside(&target, depth))
        {
            continue;
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
        assert!(scan(&root, 0, &mut 0, false).is_err());
        assert!(scan(&root, 129, &mut 10, false).is_err());
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

#[cfg(all(test, unix))]
mod inner_link_tests {
    use super::*;

    #[test]
    fn only_relative_links_that_stay_inside_are_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        std::fs::create_dir_all(base.join("node_modules/.bin")).unwrap();
        std::fs::write(base.join("node_modules/tool.js"), b"x").unwrap();
        std::os::unix::fs::symlink("../tool.js", base.join("node_modules/.bin/tool")).unwrap();
        std::os::unix::fs::symlink("node_modules/tool.js", base.join("top-link")).unwrap();
        let root = Dir::open_ambient_dir(base, cap_std::ambient_authority()).unwrap();
        verify_allowing_inner_links(&root).unwrap();
        assert!(verify(&root).is_err());
        for (name, target) in [("escape", "../outside"), ("deep-escape", "node_modules/../../outside"), ("absolute", "/etc/hosts")] {
            std::os::unix::fs::symlink(target, base.join(name)).unwrap();
            assert!(verify_allowing_inner_links(&root).is_err(), "{name}");
            std::fs::remove_file(base.join(name)).unwrap();
        }
        std::fs::hard_link(base.join("node_modules/tool.js"), base.join("shared")).unwrap();
        assert!(verify_allowing_inner_links(&root).is_err());
    }
}
