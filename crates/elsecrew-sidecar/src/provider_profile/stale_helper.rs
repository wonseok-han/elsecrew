//! Explicit, local maintenance only; not an automatic runtime admission repair.
//! Preserve a verified obsolete helper directory outside the executable profile.
use super::*;
use std::ffi::CString;
use std::os::fd::AsRawFd;

const OLD_CLI: &str = "/Applications/ChatGPT.app/Contents/Resources/codex";
const HELPER: &str = "codex-execve-wrapper";
const RENAME_NOFOLLOW_ANY: libc::c_uint = 0x10;

/// Caller selects a previously inspected directory identity, never a renderer
/// path. Existing pending reservations prohibit maintenance. No credential or
/// vendor executable is opened. Like the profile binding, this does not defend
/// against a hostile same-UID process changing names after validation.
pub(super) fn preserve(
    binding: &CodexProfileBinding,
    leaf: &str,
    expected_device: u64,
    expected_inode: u64,
) -> Result<PathBuf, SidecarError> {
    preserve_checked(
        binding,
        leaf,
        expected_device,
        expected_inode,
        Path::new(OLD_CLI),
        || {},
    )
}

fn require_absent(target: &Path) -> Result<(), SidecarError> {
    match std::fs::symlink_metadata(target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(failure()),
    }
}

fn preserve_checked(
    binding: &CodexProfileBinding,
    leaf: &str,
    expected_device: u64,
    expected_inode: u64,
    obsolete_target: &Path,
    before_move: impl FnOnce(),
) -> Result<PathBuf, SidecarError> {
    if !leaf.starts_with("codex-arg0")
        || leaf.len() > 80
        || !leaf.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(failure());
    }
    // Never classify an existing file, symlink, or permission error as absent.
    require_absent(obsolete_target)?;
    let mut owner = crate::provider_login_process::LoginProcess::reserve(binding)?;
    let result = preserve_reserved(
        binding,
        leaf,
        expected_device,
        expected_inode,
        obsolete_target,
        before_move,
    );
    let cleanup = owner.finish();
    // Failure after rename preserves the directory at its new location; never
    // undo it with an overwriting rename or report successful runtime admission.
    cleanup?;
    result
}

fn preserve_reserved(
    binding: &CodexProfileBinding,
    leaf: &str,
    expected_device: u64,
    expected_inode: u64,
    obsolete_target: &Path,
    before_move: impl FnOnce(),
) -> Result<PathBuf, SidecarError> {
    let home = binding.revalidate()?;
    let parent_path = home.join("tmp/arg0");
    let parent = open_absolute_directory(&parent_path)?;
    let source = open_absolute_directory(&parent_path.join(leaf))?;
    let validate = || -> Result<(), SidecarError> {
        require_absent(obsolete_target)?;
        binding.revalidate()?;
        let reopened = open_absolute_directory(&parent_path)?;
        if !same_directory(&parent, &reopened)? {
            return Err(failure());
        }
        let current = open_absolute_directory(&parent_path.join(leaf))?;
        let meta = current.dir_metadata().map_err(|_| failure())?;
        if !same_directory(&source, &current)?
            || meta.dev() != expected_device
            || meta.ino() != expected_inode
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o022 != 0
        {
            return Err(failure());
        }
        let entries = current.entries().map_err(|_| failure())?;
        let mut count = 0;
        for entry in entries {
            if entry.map_err(|_| failure())?.file_name() != HELPER {
                return Err(failure());
            }
            count += 1;
        }
        let link = current.symlink_metadata(HELPER).map_err(|_| failure())?;
        if count != 1
            || !link.file_type().is_symlink()
            || link.nlink() != 1
            || link.uid() != unsafe { libc::geteuid() }
            || current.read_link_contents(HELPER).map_err(|_| failure())? != obsolete_target
        {
            return Err(failure());
        }
        Ok(())
    };
    validate()?;
    let destination = format!("codex-stale-helper-preserved-{}", uuid::Uuid::new_v4());
    let from = CString::new(leaf).map_err(|_| failure())?;
    let to = CString::new(destination.as_str()).map_err(|_| failure())?;
    let target = binding.login_lease_directory()?;
    before_move();
    validate()?;
    // Directory-to-directory rename: no symlink traversal and no overwrite.
    if unsafe {
        libc::renameatx_np(
            parent.as_raw_fd(),
            from.as_ptr(),
            target.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL | RENAME_NOFOLLOW_ANY,
        )
    } != 0
    {
        return Err(failure());
    }
    parent
        .try_clone()
        .map_err(|_| failure())?
        .into_std_file()
        .sync_all()
        .map_err(|_| failure())?;
    target
        .try_clone()
        .map_err(|_| failure())?
        .into_std_file()
        .sync_all()
        .map_err(|_| failure())?;
    let preserved = open_absolute_directory(&binding.app_data_path.join(&destination))?;
    if !same_directory(&source, &preserved)? {
        return Err(failure());
    }
    binding.revalidate()?;
    Ok(binding.app_data_path.join(destination))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires explicit approval for app-owned stale helper preservation"]
    fn installed_stale_helper_preservation() {
        assert_eq!(
            std::env::var("ELSECREW_APPROVED_STALE_HELPER_RECOVERY").as_deref(),
            Ok("1")
        );
        let path = PathBuf::from(
            std::env::var_os("ELSECREW_INSPECTION_APP_DATA")
                .expect("explicit app data directory"),
        );
        assert!(path.ends_with("com.wonseokhan.elsecrew/m5"));
        let app_data = open_absolute_directory(&path).unwrap();
        let profile = profile_directory(&app_data, false).unwrap();
        let binding = CodexProfileBinding {
            app_data_path: path,
            app_data,
            profile,
        };
        // Exact locally inspected legacy artifact, not a recursive cleanup.
        let saved = preserve(&binding, "codex-arg0ovNqx1", 16777232, 166226396).unwrap();
        eprintln!("stale_helper_preserved={}", saved.display());
        verify_runtime_files(&binding.revalidate().unwrap()).unwrap();
    }

    #[test]
    fn preserves_only_exact_stale_helper_directory_and_respects_pending() {
        for case in [
            "valid",
            "wrong_inode",
            "extra",
            "wrong_target",
            "pending",
            "wrong_leaf",
            "regular_file",
            "target_appeared",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let obsolete_target = root.join("removed-cli");
            let storage = crate::Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
            let binding = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = binding.revalidate().unwrap();
            let leaf = "codex-arg0test";
            let path = home.join("tmp/arg0").join(leaf);
            std::fs::create_dir_all(&path).unwrap();
            std::os::unix::fs::symlink(
                if case == "wrong_target" {
                    Path::new("/unapproved")
                } else {
                    &obsolete_target
                },
                path.join(HELPER),
            )
            .unwrap();
            if case == "regular_file" {
                std::fs::remove_file(path.join(HELPER)).unwrap();
                std::fs::write(path.join(HELPER), b"preserve").unwrap();
            }
            if case == "extra" {
                std::fs::write(path.join("keep"), b"keep").unwrap();
            }
            if case == "pending" {
                std::fs::write(root.join("codex-login.pending"), b"keep").unwrap();
            }
            let meta = std::fs::metadata(&path).unwrap();
            use std::os::unix::fs::MetadataExt;
            let result = preserve_checked(
                &binding,
                if case == "wrong_leaf" {
                    "../escape"
                } else {
                    leaf
                },
                meta.dev(),
                meta.ino() + u64::from(case == "wrong_inode"),
                &obsolete_target,
                || {
                    if case == "target_appeared" {
                        std::fs::write(&obsolete_target, b"restored").unwrap();
                    }
                },
            );
            assert_eq!(result.is_ok(), case == "valid", "{case}");
            if let Ok(saved) = result {
                assert!(!path.exists());
                assert_eq!(std::fs::metadata(&saved).unwrap().ino(), meta.ino());
                assert_eq!(
                    std::fs::read_link(saved.join(HELPER)).unwrap(),
                    obsolete_target
                );
                verify_runtime_files(&home).unwrap();
            } else {
                assert!(std::fs::symlink_metadata(path.join(HELPER)).is_ok());
            }
            assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), CONFIG);
            assert_eq!(root.join("codex-login.pending").exists(), case == "pending");
        }
    }
}
