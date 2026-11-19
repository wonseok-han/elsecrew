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

/// A plain (not Git) folder's first history is bounded: more than this many
/// files or bytes is refused with a plain message, never half-recorded.
const FOLDER_RECORD_MAX_FILES: usize = 20_000;
const FOLDER_RECORD_MAX_BYTES: u64 = 1024 * 1024 * 1024;
/// Never recorded in a plain folder's history.
const FOLDER_RECORD_EXCLUDES: &str = ".DS_Store\nnode_modules/\n.elsecrew-prepare-*/\n";
/// The folder a shadow history belongs to, so a copied folder pointing at
/// the same history is told apart (review I2): its path and file number when
/// recorded. Either still matching is the same folder (moved on one disk, or
/// re-created in place by a sync or restore: review I4); a copy has neither.
pub(crate) const SHADOW_OWNER_FILE: &str = "elsecrew-folder";

pub(crate) fn owner_text(folder: &Path) -> Result<String, ProjectFailure> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::metadata(folder).map_err(|error| ProjectFailure::manifest(format!("cannot inspect folder: {error}")))?;
    let path = folder.to_str().ok_or_else(|| ProjectFailure::manifest("folder path is not valid text"))?;
    Ok(format!("ino:{}\npath:{path}\n", meta.ino()))
}

pub(crate) fn owns(owner: &str, folder: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let mut ino = None;
    let mut path = None;
    for line in owner.lines() {
        if let Some(value) = line.strip_prefix("ino:") { ino = value.parse::<u64>().ok(); }
        if let Some(value) = line.strip_prefix("path:") { path = Some(value); }
    }
    let same_path = path.is_some_and(|path| Path::new(path) == folder);
    let same_file = ino.is_some() && std::fs::metadata(folder).is_ok_and(|meta| Some(meta.ino()) == ino);
    same_path || same_file
}

#[cfg(test)]
thread_local! {
    pub(crate) static FOLDER_RECORD_LIMITS: std::cell::Cell<(usize, u64)> =
        const { std::cell::Cell::new((FOLDER_RECORD_MAX_FILES, FOLDER_RECORD_MAX_BYTES)) };
}
fn folder_record_limits() -> (usize, u64) {
    #[cfg(test)]
    { FOLDER_RECORD_LIMITS.with(|limits| limits.get()) }
    #[cfg(not(test))]
    { (FOLDER_RECORD_MAX_FILES, FOLDER_RECORD_MAX_BYTES) }
}

impl ProjectManager {
    /// "링크 파일 방식" (user decision 2026-11-19): a plain folder with files
    /// gets a history in app data (`shadow-git-v1/<id>.git`) and only a small
    /// `.git` file naming it, published exclusively. The first commit records
    /// the folder as it is now. Native-picker consent and the folder's
    /// identity are required, as for an empty folder. A link of ours whose
    /// history is gone or belongs to another folder (a copy) is replaced.
    pub(crate) fn initialize_folder_project(&self, path: &str, identity: &str) -> Result<(), ProjectFailure> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (path, identity);
            Err(ProjectFailure::manifest("plain folder projects are unavailable on this platform"))
        }
        #[cfg(target_os = "macos")]
        {
            let selected = canonical_existing_directory(Path::new(path))?;
            if selected != Path::new(path) || selected.starts_with(&self.data_root) || self.data_root.starts_with(&selected) {
                return Err(ProjectFailure::manifest("new project must be separate from app data"));
            }
            // A stale link of ours in the folder itself may be replaced; any
            // other `.git` here or above means a Git repository.
            let stale_link = self.stale_shadow_link(&selected);
            if selected.ancestors().any(|folder| (folder != selected || !stale_link) && std::fs::symlink_metadata(folder.join(".git")).is_ok()) {
                return Err(ProjectFailure::manifest("folder is already inside a Git repository"));
            }
            let root = open_absolute_directory_capability(&selected)?;
            let check = || -> Result<(), ProjectFailure> {
                let current = open_absolute_directory_capability(&selected)?;
                let (dev, ino) = capability_directory_identity(&root)?;
                if format!("{dev}:{ino}") != identity || capability_directory_identity(&current)? != (dev, ino) {
                    return Err(ProjectFailure::dirty("selected folder changed; choose it again"));
                }
                Ok(())
            };
            check()?;
            clean_stale_preparations(&root);
            if !stale_link && root.entries().map_err(|_| ProjectFailure::manifest("cannot inspect folder"))?
                .flatten().all(|entry| entry.file_name() == OsStr::new(".git")) {
                return Err(ProjectFailure::manifest("an empty folder starts as a new project"));
            }
            let shadows = create_managed_directory(&self.data_root, &self.data_dir, super::git_worktree::SHADOW_GIT_DIR)
                .map_err(|error| ProjectFailure::recovery(error.detail))?.0;
            let shadow = shadows.join(format!("{}.git", Uuid::new_v4().simple()));
            let stage_name = format!(".elsecrew-prepare-{}", Uuid::new_v4().simple());
            root.create_dir(&stage_name).map_err(|error| ProjectFailure::recovery(format!("folder preparation failed: {error}")))?;
            let stage = open_real_child_dir(&root, &stage_name)?;
            let git_dir_arg = format!("--git-dir={}", shadow.display());
            let work_tree_arg = format!("--work-tree={}", selected.display());
            let mut published = false;
            let prepared = (|| -> Result<(), ProjectFailure> {
                let separate = format!("--separate-git-dir={}", shadow.display());
                self.git_at(&stage, ["init", "--quiet", "--template=", "--initial-branch=main", separate.as_str(), "."])?;
                let shadow_dir = open_absolute_directory_capability(&shadow)?;
                shadow_dir.create_dir("info").map_err(|error| ProjectFailure::recovery(format!("folder preparation failed: {error}")))?;
                write_new_file(&open_real_child_dir(&shadow_dir, "info")?, "exclude", FOLDER_RECORD_EXCLUDES)?;
                write_new_file(&shadow_dir, SHADOW_OWNER_FILE, &owner_text(&selected)?)?;
                // Run from the folder itself (review I1: from the preparation
                // folder Git listed only that subfolder): count, size, and
                // refuse other repositories inside, before recording anything.
                let listed = self.git_at(&root, [git_dir_arg.as_str(), work_tree_arg.as_str(), "ls-files", "--others", "--exclude-standard", "-z", ":/"])?;
                let files: Vec<&[u8]> = listed.stdout.split(|byte| *byte == 0).filter(|name| !name.is_empty()).collect();
                let (max_files, max_bytes) = folder_record_limits();
                if files.len() > max_files {
                    return Err(ProjectFailure::manifest("folder has too many files to record"));
                }
                let mut bytes = 0u64;
                for name in &files {
                    let relative = std::str::from_utf8(name).map_err(|_| ProjectFailure::manifest("folder has a file name that is not valid text"))?;
                    if relative.ends_with('/') {
                        return Err(ProjectFailure::manifest("folder contains another Git repository"));
                    }
                    bytes += std::fs::symlink_metadata(selected.join(relative)).map(|meta| meta.len()).unwrap_or(0);
                    if bytes > max_bytes {
                        return Err(ProjectFailure::manifest("folder is too large to record"));
                    }
                }
                self.git_at(&root, [git_dir_arg.as_str(), work_tree_arg.as_str(), "add", "--all", "--", ":/"])?;
                self.git_at(&root, [git_dir_arg.as_str(), work_tree_arg.as_str(),
                    "-c", "user.name=Elsecrew", "-c", "user.email=local@elsecrew.invalid",
                    "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgSign=false",
                    "commit", "--quiet", "--allow-empty", "--no-verify", "-m", "작업 시작 전 폴더 상태"])?;
                self.git_at(&root, [git_dir_arg.as_str(), "rev-parse", "--verify", "HEAD^{commit}"])?;
                check()?;
                if stale_link {
                    // Only our own small link file, checked again just now.
                    if !self.stale_shadow_link(&selected) {
                        return Err(ProjectFailure::dirty("the folder's record link changed; choose it again"));
                    }
                    root.remove_file(".git").map_err(|error| ProjectFailure::recovery(format!("folder publication failed: {error}")))?;
                }
                let from = std::ffi::CString::new(".git").unwrap();
                // SAFETY: one-component relative paths and retained directory
                // handles; exclusive publication never replaces a `.git`.
                let result = unsafe {
                    libc::renameatx_np(stage.as_raw_fd(), from.as_ptr(), root.as_raw_fd(), from.as_ptr(), libc::RENAME_EXCL | RENAME_NOFOLLOW_ANY)
                };
                if result != 0 {
                    return Err(ProjectFailure::recovery(format!("folder publication failed: {}", std::io::Error::last_os_error())));
                }
                published = true;
                sync_capability_directory(&root)?;
                Ok(())
            })();
            // Our own preparation folder (now empty, or holding only our
            // link file) goes; nothing else in the folder is touched.
            let _ = stage.remove_file(".git");
            let _ = root.remove_dir(&stage_name);
            if prepared.is_err() && !published {
                // The half-made history is ours, in app data only; once the
                // link is in the folder the history stays (review M1).
                let _ = std::fs::remove_dir_all(&shadow);
            }
            prepared
        }
    }

    /// The folder's own `.git` is a link of ours (into `shadow-git-v1`) whose
    /// history is gone or belongs to another folder: it may be recorded anew.
    #[cfg(target_os = "macos")]
    fn stale_shadow_link(&self, folder: &Path) -> bool {
        let link = folder.join(".git");
        let Ok(metadata) = std::fs::symlink_metadata(&link) else { return false };
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
            return false;
        }
        let Ok(text) = std::fs::read_to_string(&link) else { return false };
        let Some(target) = text.strip_prefix("gitdir: ").and_then(|rest| rest.strip_suffix('\n')).map(PathBuf::from) else { return false };
        // Ours by shape: a `<name>.git` inside any `shadow-git-v1` (this app
        // data, or another Mac's or an older one's: review M6).
        let ours = target.is_absolute()
            && target.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".git") && !name.starts_with('.'))
            && target.parent().is_some_and(|parent| parent.file_name() == Some(OsStr::new(super::git_worktree::SHADOW_GIT_DIR)));
        ours && self.repository_git_path(folder).is_err()
    }
}

/// A preparation folder left by an interrupted start holds at most our link
/// file; it is taken away before trying again (review M2).
#[cfg(target_os = "macos")]
fn clean_stale_preparations(root: &Dir) {
    let names: Vec<String> = root.entries().into_iter().flatten().flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(".elsecrew-prepare-"))
        .collect();
    for name in names {
        let Ok(stage) = open_real_child_dir(root, &name) else { continue };
        let only_link = stage.entries().into_iter().flatten().flatten()
            .all(|entry| entry.file_name() == OsStr::new(".git") && entry.metadata().is_ok_and(|meta| meta.is_file() && meta.len() <= 4096));
        if only_link {
            let _ = stage.remove_file(".git");
            let _ = root.remove_dir(&name);
        }
    }
}

#[cfg(target_os = "macos")]
fn write_new_file(dir: &Dir, name: &str, text: &str) -> Result<(), ProjectFailure> {
    use std::io::Write as _;
    let mut file = dir.open_with(name, cap_std::fs::OpenOptions::new().write(true).create_new(true))
        .map_err(|error| ProjectFailure::recovery(format!("folder preparation failed: {error}")))?;
    file.write_all(text.as_bytes()).map_err(|error| ProjectFailure::recovery(format!("folder preparation failed: {error}")))
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
