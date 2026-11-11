//! Native-picker consent creates only an empty local Git baseline, never stages user files.
use super::*;

impl ProjectManager {
    pub(crate) fn initialize_empty_project(
        &self,
        path: &str,
        identity: &str,
    ) -> Result<(), ProjectFailure> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (path, identity);
            Err(ProjectFailure::manifest(
                "new empty projects are unavailable on this platform",
            ))
        }
        #[cfg(target_os = "macos")]
        {
            let selected = canonical_existing_directory(Path::new(path))?;
            if selected != Path::new(path)
                || selected.starts_with(&self.data_root)
                || self.data_root.starts_with(&selected)
            {
                return Err(ProjectFailure::manifest(
                    "new project must be separate from app data",
                ));
            }
            let root = open_absolute_directory_capability(&selected)?;
            let check = || -> Result<(), ProjectFailure> {
                let current = open_absolute_directory_capability(&selected)?;
                let (dev, ino) = capability_directory_identity(&root)?;
                if format!("{dev}:{ino}") != identity
                    || capability_directory_identity(&current)? != (dev, ino)
                {
                    return Err(ProjectFailure::dirty(
                        "selected empty folder changed; choose it again",
                    ));
                }
                Ok(())
            };
            check()?;
            require_only(&root, None)?;
            let stage_name = format!(".elsecrew-prepare-{}", Uuid::new_v4().simple());
            root.create_dir(&stage_name).map_err(|error| {
                ProjectFailure::recovery(format!("new project preparation failed: {error}"))
            })?;
            let stage = open_real_child_dir(&root, &stage_name)?;
            // On failure retain only our preparation folder; never recursively remove anything
            // in the selected folder. No original file or existing .git may be replaced.
            let prepared = (|| {
                self.git_at(
                    &stage,
                    [
                        "init",
                        "--quiet",
                        "--template=",
                        "--initial-branch=main",
                        ".",
                    ],
                )?;
                self.git_at(
                    &stage,
                    [
                        "-c",
                        "user.name=Elsecrew",
                        "-c",
                        "user.email=local@elsecrew.invalid",
                        "-c",
                        "core.hooksPath=/dev/null",
                        "-c",
                        "commit.gpgSign=false",
                        "commit",
                        "--quiet",
                        "--allow-empty",
                        "-m",
                        "Initial empty project",
                    ],
                )?;
                let git = open_real_child_dir(&stage, ".git")?;
                self.git_at(&git, ["rev-parse", "--verify", "HEAD^{commit}"])?;
                check()?;
                require_only(&root, Some(&stage_name))?;
                let linked = open_real_child_dir(&root, &stage_name)?;
                if capability_directory_identity(&linked)? != capability_directory_identity(&stage)?
                {
                    return Err(ProjectFailure::dirty(
                        "new project preparation folder changed",
                    ));
                }
                let from = std::ffi::CString::new(".git").unwrap();
                // SAFETY: one-component relative paths and retained directory handles;
                // exclusive publication cannot replace a concurrent .git or follow symlinks.
                let result = unsafe {
                    libc::renameatx_np(
                        stage.as_raw_fd(),
                        from.as_ptr(),
                        root.as_raw_fd(),
                        from.as_ptr(),
                        libc::RENAME_EXCL | RENAME_NOFOLLOW_ANY,
                    )
                };
                if result != 0 {
                    return Err(ProjectFailure::recovery(format!(
                        "new project publication failed: {}",
                        std::io::Error::last_os_error()
                    )));
                }
                sync_capability_directory(&root)?;
                root.remove_dir(&stage_name).map_err(|error| {
                    ProjectFailure::recovery(format!(
                        "new project prepared; empty staging folder cleanup failed: {error}"
                    ))
                })?;
                sync_capability_directory(&root)?;
                Ok(())
            })();
            prepared.map_err(|mut error: ProjectFailure| {
                error
                    .detail
                    .push_str(&format!("; preparation folder: {stage_name}"));
                error
            })
        }
    }
}

#[cfg(target_os = "macos")]
fn require_only(root: &Dir, allowed: Option<&str>) -> Result<(), ProjectFailure> {
    for entry in root
        .entries()
        .map_err(|_| ProjectFailure::manifest("cannot inspect empty folder"))?
    {
        let entry =
            entry.map_err(|_| ProjectFailure::manifest("cannot inspect empty folder entry"))?;
        if allowed.is_none_or(|name| entry.file_name() != OsStr::new(name)) {
            return Err(ProjectFailure::dirty(
                "folder is no longer empty; no existing files were initialized",
            ));
        }
    }
    Ok(())
}
