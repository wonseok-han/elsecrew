//! Codex-only vendor helper exception. Workspace and Claude scanners stay strict.
use super::*;
use crate::provider_executable::PinnedExecutable;

pub(crate) fn verify_codex_runtime_files(
    home: &Path,
    executable: &PinnedExecutable,
) -> Result<(), SidecarError> {
    let target = executable.revalidate()?;
    let root = open_absolute_directory(home)?;
    let mut remaining = hello_agent_protocol::project_v1::SNAPSHOT_ENTRIES_MAX_V1;
    scan(&root, Path::new(""), target, &mut remaining, 0)?;
    executable.revalidate()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn held_executable_replacement_is_rejected() {
        use sha2::{Digest, Sha256};
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let target = home.join("vendor");
        std::fs::write(&target, b"synthetic executable").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable =
            PinnedExecutable::open(&target, Sha256::digest(b"synthetic executable").into())
                .unwrap();
        let helpers = home.join("tmp/arg0/codex-arg0test123");
        std::fs::create_dir_all(&helpers).unwrap();
        symlink(&target, helpers.join("apply_patch")).unwrap();
        std::fs::write(helpers.join(".lock"), b"").unwrap();
        verify_codex_runtime_files(&home, &executable).unwrap();
        symlink(home.join("missing"), helpers.join("codex-execve-wrapper")).unwrap();
        assert!(verify_codex_runtime_files(&home, &executable).is_err());
        std::fs::remove_file(helpers.join("codex-execve-wrapper")).unwrap();
        std::fs::rename(&target, home.join("old")).unwrap();
        std::fs::write(&target, b"synthetic executable").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(verify_codex_runtime_files(&home, &executable).is_err());
    }

    #[test]
    fn helper_links_are_bound_to_location_target_and_structure() {
        for variant in [
            "valid",
            "wrong-target",
            "extra",
            "directory-link",
            "hardlink",
            "outside",
            "writable",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().canonicalize().unwrap();
            let target = home.join("vendor");
            std::fs::write(&target, b"synthetic executable").unwrap();
            let helpers = home.join("tmp/arg0/codex-arg0test123");
            std::fs::create_dir_all(&helpers).unwrap();
            for name in ["applypatch", "apply_patch", "codex-execve-wrapper"] {
                symlink(&target, helpers.join(name)).unwrap();
            }
            match variant {
                "wrong-target" => {
                    std::fs::remove_file(helpers.join("apply_patch")).unwrap();
                    symlink(home.join("missing"), helpers.join("apply_patch")).unwrap();
                }
                "extra" => std::fs::write(helpers.join("unexpected"), b"x").unwrap(),
                "directory-link" => symlink(&home, home.join("linked-dir")).unwrap(),
                "hardlink" => std::fs::hard_link(&target, home.join("shared")).unwrap(),
                "outside" => symlink(&target, home.join("apply_patch")).unwrap(),
                "writable" => std::fs::set_permissions(
                    home.join("tmp"),
                    std::fs::Permissions::from_mode(0o777),
                )
                .unwrap(),
                _ => (),
            }
            let result = scan(
                &open_absolute_directory(&home).unwrap(),
                Path::new(""),
                &target,
                &mut 1000,
                0,
            );
            assert_eq!(result.is_ok(), variant == "valid", "{variant}");
            assert!(
                verify_runtime_files(&home).is_err(),
                "generic scanner must remain strict"
            );
        }
    }
}

fn helper_directory(path: &Path) -> bool {
    let parts: Vec<_> = path.iter().collect();
    parts.len() == 3
        && parts[0] == "tmp"
        && parts[1] == "arg0"
        && parts[2].to_str().is_some_and(|name| {
            name.starts_with("codex-arg0")
                && name.len() > 10
                && name.len() <= 80
                && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
}

fn scan(
    root: &Dir,
    relative: &Path,
    target: &Path,
    remaining: &mut u64,
    depth: usize,
) -> Result<(), SidecarError> {
    if depth > 128 {
        return Err(failure());
    }
    for entry in root.entries().map_err(|_| failure())? {
        let name = entry.map_err(|_| failure())?.file_name();
        *remaining = remaining.checked_sub(1).ok_or_else(failure)?;
        let metadata = root.symlink_metadata(&name).map_err(|_| failure())?;
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(failure());
        }
        let helper_leaf = matches!(
            name.to_str(),
            Some("applypatch" | "apply_patch" | "codex-execve-wrapper")
        );
        if metadata.file_type().is_symlink() {
            // macOS symlinks normally have mode 0777: ownership and the exact
            // signed target are meaningful, permission bits on a link are not.
            if !helper_directory(relative)
                || !helper_leaf
                || metadata.nlink() != 1
                || root.read_link_contents(&name).map_err(|_| failure())? != target
            {
                return Err(failure());
            }
            continue;
        }
        // Vendor arg0 directories may also contain a regular .lock, never
        // arbitrary executables or nested directories.
        if helper_directory(relative) && name != ".lock" {
            return Err(failure());
        }
        if metadata.mode() & 0o022 != 0 {
            return Err(failure());
        }
        if metadata.is_file() && metadata.nlink() == 1 {
            continue;
        }
        if !metadata.is_dir() || helper_directory(relative) {
            return Err(failure());
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No)
            .custom_flags(libc::O_NONBLOCK);
        let file = root.open_with(&name, &options).map_err(|_| failure())?;
        let opened = file.metadata().map_err(|_| failure())?;
        if !opened.is_dir() || opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(failure());
        }
        scan(
            &Dir::from_std_file(file.into_std()),
            &relative.join(name),
            target,
            remaining,
            depth + 1,
        )?;
    }
    Ok(())
}
