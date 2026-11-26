//! Task workspaces as real `git worktree`s of the user's repository
//! (docs/proposals/m5-real-worktrees.md §3.2). The task gets its own branch
//! `elsecrew/<task>`; the user's folder and its checked-out branch are never
//! touched. In the user's `.git` only the worktree record and the task branch
//! appear. Every Git command runs without hooks, fsmonitor or remotes
//! (`fixed_git_command`).
use super::*;

#[cfg(test)]
thread_local! {
    /// Tests: stop right after `git worktree add`, as if the app quit there.
    pub(super) static STOP_AFTER_WORKTREE_ADD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Bytes a `.git` link file or `gitdir` back link may take.
const LINK_MAX_BYTES: u64 = 4096;

fn read_small_file(dir: &Dir, name: &str, what: &str) -> Result<String, ProjectFailure> {
    let metadata = dir.symlink_metadata(name).map_err(|error| {
        ProjectFailure::manifest(format!("{what} is missing: {error}"))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > LINK_MAX_BYTES {
        return Err(ProjectFailure::manifest(format!("{what} is not a small regular file")));
    }
    let mut text = String::new();
    open_file_relative(dir, name)?
        .read_to_string(&mut text)
        .map_err(|error| ProjectFailure::manifest(format!("could not read {what}: {error}")))?;
    Ok(text)
}

/// The setting file for "작업 공간 폴더" (where new task worktrees go).
const WORKSPACE_FOLDER_FILE: &str = "workspace-folder.json";

/// A folder name for a project: its folder's own name, without characters
/// a path or a shell would trip over.
fn project_folder_name(root: &Path) -> String {
    let name: String = root.file_name().and_then(|name| name.to_str()).unwrap_or("project")
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' })
        .take(60)
        .collect();
    let name = name.trim_matches(|c| c == '.' || c == '-').to_owned();
    if name.is_empty() { "project".into() } else { name }
}

/// `elsecrew/task-<8 hex>`: unique per workspace, valid for Git and for the
/// manifest contract.
fn task_branch(worktree_id: &str) -> String {
    let short: String = worktree_id.trim_start_matches("worktree_").chars().take(8).collect();
    format!("{TASK_BRANCH_PREFIX_V2}task-{short}")
}

/// A plain folder's history lives in app data (`shadow-git-v1/<name>.git`,
/// user decision 2026-11-19 "링크 파일 방식"); the folder holds only a small
/// `.git` file naming it, made by `initialize_folder_project`.
pub(crate) const SHADOW_GIT_DIR: &str = "shadow-git-v1";

impl ProjectManager {
    /// The repository's Git directory for a project folder: its own real
    /// `.git` folder, or, through a small `.git` link file, a shadow history
    /// of this app (exactly `<data>/shadow-git-v1/<name>.git`, a real folder).
    /// Nothing else is followed.
    pub(crate) fn repository_git_path(&self, root: &Path) -> Result<PathBuf, ProjectFailure> {
        let link = root.join(".git");
        let metadata = std::fs::symlink_metadata(&link)
            .map_err(|error| ProjectFailure::manifest(format!("could not inspect source .git entry: {error}")))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            // A .git folder whose `commondir` names another repository shares
            // that repository's objects and refs: not a repository of its own
            // (PR #18 re-review B1).
            if std::fs::symlink_metadata(link.join("commondir")).is_ok() {
                return Err(ProjectFailure::manifest("source repository must own a real .git directory; one that shares another repository's (commondir) is unsupported"));
            }
            return Ok(link);
        }
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > LINK_MAX_BYTES {
            return Err(ProjectFailure::manifest("source repository must own a real .git directory; linked worktrees are unsupported"));
        }
        let text = std::fs::read_to_string(&link)
            .map_err(|error| ProjectFailure::manifest(format!("could not read source .git link: {error}")))?;
        let target = text.strip_prefix("gitdir: ").and_then(|rest| rest.strip_suffix('\n')).map(PathBuf::from)
            .ok_or_else(|| ProjectFailure::manifest("source .git link is malformed"))?;
        // Ours by shape (checked first: this app data may have no shadow
        // folder yet, e.g. on another Mac; review N1).
        let shadow_shaped = target.parent().is_some_and(|parent| parent.file_name() == Some(std::ffi::OsStr::new(SHADOW_GIT_DIR)));
        let stale = || ProjectFailure::manifest("selected directory is not inside a supported Git repository: its record is missing or belongs to another folder");
        let shadows = match std::fs::canonicalize(self.data_root.join(SHADOW_GIT_DIR)) {
            Ok(shadows) => shadows,
            Err(_) if shadow_shaped => return Err(stale()),
            Err(_) => return Err(ProjectFailure::manifest("source repository must own a real .git directory; linked worktrees are unsupported")),
        };
        let real = std::fs::symlink_metadata(&target).is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink());
        let named = target.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".git") && !name.starts_with('.'));
        // Ours, but its history is gone, elsewhere (another Mac's app data),
        // or another folder's (a copied folder keeps the link): the folder
        // can be recorded anew (review I2, I3, M6).
        if !named || target.parent() != Some(shadows.as_path()) {
            if named && shadow_shaped { return Err(stale()); }
            return Err(ProjectFailure::manifest("source repository must own a real .git directory; linked worktrees are unsupported"));
        }
        if !real || std::fs::canonicalize(&target).ok().as_deref() != Some(target.as_path()) {
            return Err(stale());
        }
        let owner = std::fs::read_to_string(target.join(super::empty_project::SHADOW_OWNER_FILE)).map_err(|_| stale())?;
        if !super::empty_project::owns(&owner, root) {
            return Err(stale());
        }
        // Still this folder: keep the mark current (a folder re-created in
        // place and later moved must still be recognised). Best effort.
        if let Ok(current) = super::empty_project::owner_text(root) {
            if current != owner {
                let temporary = target.join(format!(".{}.{}.tmp", super::empty_project::SHADOW_OWNER_FILE, Uuid::new_v4().simple()));
                if std::fs::write(&temporary, &current).is_ok() {
                    let _ = std::fs::rename(&temporary, target.join(super::empty_project::SHADOW_OWNER_FILE));
                }
            }
        }
        Ok(target)
    }

    /// The "작업 공간 폴더": the saved setting, or `~/elsecrew/workspaces`
    /// (Orca-like, visible to the user). Tests use a folder next to their
    /// own app data so they never touch the real home.
    pub(crate) fn workspaces_home(&self) -> Result<PathBuf, ProjectFailure> {
        let settings = create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1;
        if let Ok(value) = read_bounded_json(&settings, WORKSPACE_FOLDER_FILE) {
            if let Some(path) = value["path"].as_str().map(PathBuf::from).filter(|path| path.is_absolute()) {
                return Ok(path);
            }
        }
        if cfg!(test) {
            // Two missing levels, like `~/elsecrew/workspaces` on a new Mac.
            return Ok(self.data_root.parent().unwrap_or(&self.data_root).join("elsecrew").join("workspaces"));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).filter(|path| path.is_absolute())
            .ok_or_else(|| ProjectFailure::recovery("the home folder is unknown"))?;
        Ok(home.join("elsecrew").join("workspaces"))
    }

    /// The setting as the settings page shows it.
    pub(crate) fn workspace_folder(&self) -> Result<Value, ProjectFailure> {
        let custom = self.workspace_folder_is_custom()?;
        let path = self.workspaces_home()?;
        // The default folder (and `~/elsecrew`) may not exist yet: resolve
        // the nearest existing folder and keep the rest of the path as is.
        let mut existing = path.as_path();
        let mut rest = Vec::new();
        let base = loop {
            match std::fs::canonicalize(existing) {
                Ok(base) => break base,
                Err(_) => {
                    rest.push(existing.file_name().ok_or_else(|| ProjectFailure::manifest("the workspace folder has no existing parent"))?);
                    existing = existing.parent().ok_or_else(|| ProjectFailure::manifest("the workspace folder has no existing parent"))?;
                }
            }
        };
        let shown = rest.iter().rev().fold(base, |path, name| path.join(name));
        Ok(json!({"path":canonical_protocol_path_from_absolute(&shown)?,"custom":custom}))
    }

    fn workspace_folder_is_custom(&self) -> Result<bool, ProjectFailure> {
        let settings = create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1;
        Ok(settings.symlink_metadata(WORKSPACE_FOLDER_FILE).is_ok())
    }

    /// Saves a new "작업 공간 폴더" (or `None`: back to the default). Only a
    /// real, existing, writable folder outside the app's data and outside
    /// every saved project is accepted. Existing tasks stay where they are.
    pub(crate) fn set_workspace_folder(&self, storage: &Storage, path: Option<&str>) -> Result<Value, ProjectFailure> {
        let settings = create_managed_directory(&self.data_root, &self.data_dir, "application-settings-v1")?.1;
        let Some(path) = path else {
            match settings.remove_file(WORKSPACE_FOLDER_FILE) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(ProjectFailure::recovery(format!("could not reset the workspace folder: {error}"))),
            }
            return self.workspace_folder();
        };
        let wanted = Path::new(path);
        let canonical = std::fs::canonicalize(wanted).map_err(|_| ProjectFailure::manifest("the chosen folder is unavailable"))?;
        let metadata = std::fs::symlink_metadata(&canonical).map_err(|_| ProjectFailure::manifest("the chosen folder is unavailable"))?;
        if !wanted.is_absolute() || canonical != wanted || !metadata.is_dir() || metadata.permissions().readonly() {
            return Err(ProjectFailure::manifest("choose a real, writable folder (not a link)"));
        }
        if canonical.starts_with(&self.data_root) || self.data_root.starts_with(&canonical) {
            return Err(ProjectFailure::manifest("the workspace folder cannot overlap the app's own data"));
        }
        for project in storage.projection_records("project")? {
            if let Some(root) = project.get("canonical_path").and_then(Value::as_str) {
                if canonical.starts_with(root) || Path::new(root).starts_with(&canonical) {
                    return Err(ProjectFailure::manifest("the workspace folder cannot overlap a saved project"));
                }
            }
        }
        // Nor inside an existing task's workspace: a new task there would sit
        // in a folder another task's agent writes (PR #18 review B1). Holding
        // existing workspaces is the normal case (the default folder).
        for workspace in self.recorded_workspace_folders(storage)? {
            if super::write_scope::path_within(&canonical, &workspace) {
                return Err(ProjectFailure::manifest("the workspace folder is inside a task's workspace"));
            }
        }
        write_json_atomic(&settings, WORKSPACE_FOLDER_FILE, &json!({"path":canonical_protocol_path(&canonical)?}))?;
        self.workspace_folder()
    }

    /// `<작업 공간 폴더>/<project>/task-<8 hex>` for a new task, made of
    /// real folders only and never inside the user's project or the app's
    /// own data (nor the other way round).
    fn new_task_worktree_path(&self, storage: &Storage, source_root: &Path, worktree_id: &str) -> Result<PathBuf, ProjectFailure> {
        let home = self.workspaces_home()?;
        // A chosen folder that is gone (an unplugged disk) is not silently
        // made again; only the default folder is created when missing.
        if self.workspace_folder_is_custom()? && !home.is_dir() {
            return Err(ProjectFailure::recovery("the chosen workspace folder is missing; choose it again in settings"));
        }
        let wanted = home.join(project_folder_name(source_root));
        let short: String = worktree_id.trim_start_matches("worktree_").chars().take(8).collect();
        // Nothing is made before the new folder is known not to meet another
        // task's workspace (PR #18 re-review B1).
        self.refuse_new_task_folder(storage, &wanted, &wanted.join(format!("task-{short}")))?;
        std::fs::create_dir_all(&wanted).map_err(|error| {
            ProjectFailure::recovery(format!("could not make the workspace folder: {error}"))
        })?;
        let canonical_home = std::fs::canonicalize(&home).map_err(|error| ProjectFailure::recovery(format!("workspace folder unavailable: {error}")))?;
        let project_dir = std::fs::canonicalize(&wanted).map_err(|error| ProjectFailure::recovery(format!("workspace folder unavailable: {error}")))?;
        if project_dir != canonical_home.join(project_folder_name(source_root)) {
            return Err(ProjectFailure::symlink("the project's workspace folder is a link"));
        }
        for (inner, outer) in [(&project_dir, source_root), (&source_root.to_path_buf(), &project_dir), (&project_dir, &self.data_root), (&self.data_root, &project_dir)] {
            if inner.starts_with(outer) {
                return Err(ProjectFailure::manifest("the workspace folder overlaps the project or the app's data"));
            }
        }
        // Nor any other saved project (a home folder that is itself a
        // repository, say): the same rule as choosing the folder.
        for project in storage.projection_records("project")? {
            if let Some(root) = project.get("canonical_path").and_then(Value::as_str) {
                if project_dir.starts_with(root) || Path::new(root).starts_with(&project_dir) {
                    return Err(ProjectFailure::manifest("the workspace folder overlaps a saved project; choose another in settings"));
                }
            }
        }
        let task_dir = project_dir.join(format!("task-{short}"));
        // Again on the real folders now made (PR #18 review B1).
        self.refuse_new_task_folder(storage, &project_dir, &task_dir)?;
        Ok(task_dir)
    }

    /// The worktree's `.git` file names `<root>/.git/worktrees/<name>`, and
    /// that record points back at this very worktree.
    pub(super) fn verify_worktree_link(&self, worktree: &Dir, active: &ActiveProject) -> Result<(), ProjectFailure> {
        let repository = self.repository_git_path(&active.canonical_git_root)?;
        let root = canonical_protocol_path_without_existing(&repository)?;
        let link = read_small_file(worktree, ".git", "worktree Git link")?;
        let prefix = format!("gitdir: {}/worktrees/", root.trim_end_matches('/'));
        let name = link
            .strip_suffix('\n')
            .and_then(|line| line.strip_prefix(&prefix))
            .filter(|name| !name.is_empty() && !name.contains('/') && !name.starts_with('.'))
            .ok_or_else(|| ProjectFailure::manifest("worktree Git link does not name the project's repository"))?;
        let git_dir = open_absolute_directory_capability(&repository)?;
        let record = open_real_child_dir(&open_real_child_dir(&git_dir, "worktrees")?, name)?;
        let back = read_small_file(&record, "gitdir", "worktree back link")?;
        let expected = format!(
            "{}/.git\n",
            canonical_protocol_path_without_existing(&active.worktree_path)?.trim_end_matches('/')
        );
        if back != expected {
            return Err(ProjectFailure::manifest("worktree record points at another folder"));
        }
        Ok(())
    }

    /// A task of a plain folder's earlier record (the folder was recorded
    /// anew after its record went missing or stale): its `.git` link names a
    /// worktree record in another shadow history of this app (never
    /// `current`), and that record points back at exactly this task folder.
    /// Only such a folder can be ours without being in the project's own
    /// repository. It can no longer open or merge, but may be discarded.
    /// Returns the old history and the record's name.
    pub(super) fn earlier_record_worktree(&self, task: &Path, current: Option<&Path>) -> Option<(PathBuf, String)> {
        let link_path = task.join(".git");
        let link = std::fs::symlink_metadata(&link_path).ok()
            .filter(|meta| meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= LINK_MAX_BYTES)
            .and_then(|_| std::fs::read_to_string(&link_path).ok())?;
        let record = PathBuf::from(link.strip_prefix("gitdir: ")?.strip_suffix('\n')?);
        let name = record.file_name()?.to_str()?.to_owned();
        let records = record.parent()?;
        let history = records.parent()?;
        let history_name = history.file_name()?.to_str()?;
        let shadows = std::fs::canonicalize(self.data_root.join(SHADOW_GIT_DIR)).ok()?;
        if name.starts_with('.')
            || records.file_name() != Some(OsStr::new("worktrees"))
            || history.parent() != Some(shadows.as_path())
            || !history_name.ends_with(".git")
            || history_name.starts_with('.')
            || current.is_none_or(|current| current == history)
            || std::fs::canonicalize(history).ok().as_deref() != Some(history)
        {
            return None;
        }
        let history_dir = open_absolute_directory_capability(history).ok()?;
        let record_dir = open_real_child_dir(&open_real_child_dir(&history_dir, "worktrees").ok()?, &name).ok()?;
        let back = read_small_file(&record_dir, "gitdir", "worktree back link").ok()?;
        let expected = format!("{}/.git\n", canonical_protocol_path_without_existing(task).ok()?.trim_end_matches('/'));
        (back == expected).then(|| (history.to_path_buf(), name))
    }

    /// Discarding (and cleaning up) a task: its worktree checked as always,
    /// or else a task of the folder's earlier record, which can do nothing
    /// but be discarded.
    pub(super) fn verify_repository_for_discard(&self, worktree: &Dir, active: &ActiveProject) -> Result<(), ProjectFailure> {
        let checked = self.verify_snapshot_repository(worktree, active);
        if checked.is_err() && self.is_earlier_record_task(active) {
            return Ok(());
        }
        checked
    }

    /// A plain folder of this app: its `.git` is our small link file into
    /// its shadow history (a Git project has a real `.git` folder).
    pub(super) fn is_plain_folder(&self, root: &Path) -> bool {
        std::fs::symlink_metadata(root.join(".git")).is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
            && self.repository_git_path(root).is_ok()
    }

    /// Whether this task worktree belongs to its folder's earlier record
    /// (never when the project's own record cannot be found: review M14).
    pub(super) fn is_earlier_record_task(&self, active: &ActiveProject) -> bool {
        if !matches!(active.kind, WorkspaceKind::GitWorktree { .. }) {
            return false;
        }
        let current = self.repository_git_path(&active.canonical_git_root).ok()
            .and_then(|path| std::fs::canonicalize(path).ok());
        self.earlier_record_worktree(&active.worktree_path, current.as_deref()).is_some()
    }

    /// Review M8: a shadow history that no folder uses any more (its folder
    /// was recorded anew) and with no task left is removed. It is kept
    /// whenever unsure: not exactly one of ours, a task record remains, its
    /// folder (owner mark) still links to it or cannot be looked at (a disk
    /// not attached, say). Best effort; never fails the caller.
    pub(super) fn retire_orphaned_history(&self, history: &Path) {
        let Ok(shadows) = std::fs::canonicalize(self.data_root.join(SHADOW_GIT_DIR)) else { return };
        let Some(name) = history.file_name().and_then(|name| name.to_str()) else { return };
        if history.parent() != Some(shadows.as_path()) || !name.ends_with(".git") || name.starts_with('.') {
            return;
        }
        let Ok(shadows_dir) = open_absolute_directory_capability(&shadows) else { return };
        let Ok(dir) = open_real_child_dir(&shadows_dir, name) else { return };
        if let Ok(records) = open_real_child_dir(&dir, "worktrees") {
            if records.entries().map_or(true, |mut entries| entries.next().is_some()) {
                return;
            }
        }
        // Without a mark at all (an earlier build's record) any folder
        // pointing here is recorded anew when opened, so nothing uses it.
        // With one, it goes only when that very folder (same file number on
        // the same disk, review I5) now links to another history; a mark that
        // cannot be read or understood keeps it (review M13).
        match dir.symlink_metadata(super::empty_project::SHADOW_OWNER_FILE) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return,
            Ok(_) => {
                let Ok(owner) = read_small_file(&dir, super::empty_project::SHADOW_OWNER_FILE, "folder mark") else { return };
                let Some(folder) = owner.lines().find_map(|line| line.strip_prefix("path:")) else { return };
                if !super::empty_project::same_folder_by_number(&owner, Path::new(folder)) {
                    return;
                }
                let expected = format!("gitdir: {}\n", history.display());
                match std::fs::read_to_string(Path::new(folder).join(".git")) {
                    Ok(link) if link != expected => {}
                    _ => return,
                }
            }
        }
        let mut count = 0;
        if workspace_cleanup::walk(&dir, false, 0, &mut count).is_err() {
            return;
        }
        let mut count = 0;
        if workspace_cleanup::walk(&dir, true, 0, &mut count).is_ok() {
            let _ = shadows_dir.remove_dir(name);
        }
    }

    /// Still the task's worktree on the task's branch, grown from its base.
    /// Commits are expected (the app records each step); a moved branch, a
    /// detached HEAD or history that lost the base are not.
    pub(super) fn verify_worktree_repository(&self, worktree: &Dir, active: &ActiveProject) -> Result<(), ProjectFailure> {
        let WorkspaceKind::GitWorktree { branch, .. } = &active.kind else {
            return Err(ProjectFailure::manifest("not a task worktree"));
        };
        self.verify_worktree_link(worktree, active)?;
        let symbolic = self.git_allow_status_at(worktree, ["symbolic-ref", "-q", "HEAD"], &[0, 1])?;
        if !symbolic.status.success() || trim_output(&symbolic)? != format!("refs/heads/{branch}") {
            return Err(ProjectFailure::manifest("task worktree is not on its task branch"));
        }
        let grown = self.git_allow_status_at(
            worktree,
            ["merge-base", "--is-ancestor", active.base_commit.as_str(), "HEAD"],
            &[0, 1],
        )?;
        if grown.status.code() != Some(0) {
            return Err(ProjectFailure::manifest("task branch no longer contains its base commit"));
        }
        Ok(())
    }

    /// After each team step the app records the task's work on its branch
    /// (agents never commit). Nothing to record, or a v1 copy, is `false`.
    /// The author is fixed; the user's own Git identity and signing setup are
    /// never used (decision 2026-11-18).
    pub(crate) fn workspace_commit_step(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        message: &str,
    ) -> Result<bool, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        if active.kind == WorkspaceKind::Snapshot {
            return Ok(false);
        }
        let worktree = self.worktree_capability(&active)?;
        // A base merge the team is resolving is recorded (as the merge)
        // only once no conflict marks remain; until then nothing is.
        let merging = self.merge_in_progress(&worktree)?;
        if merging && !self.unresolved_conflicts(&worktree)?.is_empty() {
            return Ok(false);
        }
        // A filter added to the repository since the workspace was made would
        // run its program on `add`; refuse instead of recording.
        if self.content_filters_configured(&worktree)? {
            return Err(ProjectFailure::manifest("the repository now uses Git content filters; step not recorded"));
        }
        self.git_at(&worktree, ["add", "-A"])?;
        let staged = self.git_allow_status_at(&worktree, ["diff", "--cached", "--quiet"], &[0, 1])?;
        // A resolved merge is recorded even when the team kept exactly the
        // task's side: the merge commit itself is what joins the base.
        if staged.status.code() == Some(0) && !merging {
            return Ok(false);
        }
        let line: String = message.lines().map(str::trim).find(|line| !line.is_empty())
            .unwrap_or("Elsecrew")
            .chars().filter(|c| !c.is_control()).take(200).collect();
        self.git_at(&worktree, [
            "-c", "user.name=Elsecrew", "-c", "user.email=elsecrew@local",
            "-c", "commit.gpgsign=false", "-c", "gc.auto=0", "-c", "maintenance.auto=false",
            "commit", "-q", "--no-verify", "-m", line.as_str(),
        ])?;
        self.verify_worktree_repository(&worktree, &active)?;
        Ok(true)
    }

    /// A content filter program set in the repository's own config (the
    /// app never reads global or system config).
    pub(super) fn content_filters_configured(&self, dir: &Dir) -> Result<bool, ProjectFailure> {
        let found = self.git_allow_status_at(dir, ["config", "--get-regexp", r"^filter\."], &[0, 1])?;
        Ok(found.status.code() == Some(0))
    }

    /// Files at `commit` asking for a content filter (`filter=lfs` and the
    /// like in a tracked `.gitattributes`, or the repository's own
    /// `info/attributes`). Checked out without the filter they would be
    /// pointer files, and an agent's edit would commit the real bytes.
    pub(super) fn content_filters_requested(&self, root: &Dir, git_dir: &Dir, commit: &str) -> Result<bool, ProjectFailure> {
        let pattern = r"(^|[[:space:]])filter=";
        let tracked = self.git_allow_status_at(
            root,
            ["grep", "-q", "-I", "-E", pattern, commit, "--", ":(glob).gitattributes", ":(glob)**/.gitattributes"],
            &[0, 1],
        )?;
        if tracked.status.code() == Some(0) {
            return Ok(true);
        }
        // An attributes file named in the repository's own config could ask
        // for a filter too; not read, refused (rare in a local config).
        let extra = self.git_allow_status_at(root, ["config", "--local", "--get", "core.attributesFile"], &[0, 1])?;
        if extra.status.code() == Some(0) {
            return Ok(true);
        }
        Ok(match git_dir.symlink_metadata("info/attributes") {
            Ok(metadata) if metadata.is_file() && metadata.len() <= 1024 * 1024 => {
                let mut text = String::new();
                open_file_relative(git_dir, "info/attributes")?
                    .read_to_string(&mut text)
                    .map_err(|error| ProjectFailure::manifest(format!("could not read info/attributes: {error}")))?;
                text.split_whitespace().any(|word| word.starts_with("filter="))
            }
            Ok(_) => true,
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        })
    }

    /// After a task worktree's folder is deleted (discard cleanup, project
    /// deletion): drop exactly its record in the user's `.git/worktrees`
    /// (the one whose back link names this folder; never a repository-wide
    /// prune) and its branch only when Git agrees it is merged
    /// (`branch -d`). An unmerged task branch stays, like Orca. Best effort:
    /// the folder is already gone, leftovers are harmless.
    pub(super) fn forget_task_worktree(&self, manifest: &Value) {
        if manifest["schemaVersion"] != PROJECT_SCHEMA_VERSION_V2 || manifest["workspaceKind"] != WORKSPACE_KIND_GIT_WORKTREE_V2 {
            return;
        }
        let (Some(root), Some(path), Some(branch)) = (
            manifest_source_root(manifest), manifest["worktreePath"].as_str(), manifest["branchName"].as_str(),
        ) else { return };
        if std::fs::symlink_metadata(path).is_ok() || !branch.starts_with(TASK_BRANCH_PREFIX_V2) {
            return;
        }
        let Ok(source_root) = open_absolute_directory_capability(Path::new(root)) else { return };
        let Ok(repository) = self.repository_git_path(Path::new(root)) else { return };
        let Ok(git_dir) = open_absolute_directory_capability(&repository) else { return };
        if let Ok(records) = open_real_child_dir(&git_dir, "worktrees") {
            let back_link = format!("{}/.git\n", path.trim_end_matches('/'));
            let names: Vec<String> = records.entries().into_iter().flatten().flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
            for name in names {
                let Ok(record) = open_real_child_dir(&records, &name) else { continue };
                if read_small_file(&record, "gitdir", "worktree back link").ok().as_deref() != Some(back_link.as_str()) {
                    continue;
                }
                if workspace_cleanup::walk(&record, true, 0, &mut 0).is_ok() {
                    let _ = records.remove_dir(&name);
                }
            }
        }
        let _ = self.git_allow_status_at(&source_root, ["branch", "-d", branch], &[0, 1]);
    }

    /// What a new task will not see: changes in the user's folder that are
    /// not committed (a task starts from the last commit, like a PR from a
    /// pushed branch). `applies` is false while tasks are still v1 copies.
    pub(crate) fn source_uncommitted(&self, storage: &Storage, project_id: &str) -> Result<Value, ProjectFailure> {
        if !self.git_worktrees_enabled {
            return Ok(json!({"applies":false,"uncommitted":0}));
        }
        let project = required_project(storage, project_id)?;
        let source = self.open_project_source(&project)?;
        // Comparing contents may run a repository's own clean filter; such a
        // repository cannot make tasks anyway, so it is not asked.
        if self.content_filters_configured(&source._root)? {
            return Ok(json!({"applies":true,"uncommitted":0}));
        }
        let status = self.git_at(&source._root, ["status", "--porcelain", "-z", "--untracked-files=normal"])?;
        // `XY path\0`, and renames/copies carry their old path as one more field.
        let mut fields = status.stdout.split(|byte| *byte == 0).filter(|entry| !entry.is_empty());
        let mut count = 0;
        while let Some(entry) = fields.next() {
            count += 1;
            if matches!(entry.first(), Some(b'R' | b'C')) || matches!(entry.get(1), Some(b'R' | b'C')) {
                fields.next();
            }
        }
        Ok(json!({"applies":true,"uncommitted":count}))
    }

    /// A task worktree whose creation stopped before it was recorded (the
    /// app quit after `git worktree add`): its folder has a `.git` link file
    /// and its manifest, if written, is this task's v2 manifest. A v1 copy
    /// never has a `.git` file, so its "unexpected artifact" rule stays.
    pub(super) fn unstarted_task_worktree(&self, worktree_id: &str, folder: &Path) -> bool {
        let folder = match std::fs::symlink_metadata(folder) {
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            Ok(metadata) => metadata.is_dir() && !metadata.file_type().is_symlink()
                && std::fs::symlink_metadata(folder.join(".git"))
                    .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink()),
        };
        let manifest = match self.manifests_dir.symlink_metadata(format!("{worktree_id}.json")) {
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            Ok(_) => read_bounded_json(&self.manifests_dir, &format!("{worktree_id}.json")).is_ok_and(|manifest| {
                manifest["schemaVersion"] == PROJECT_SCHEMA_VERSION_V2 && manifest["worktreeId"] == worktree_id
                    && manifest["lifecycle"] == "active"
            }),
        };
        folder && manifest
    }

    /// Undoes an unstarted task worktree before its preparation is
    /// abandoned: the worktree (folder and its record) through Git, the task
    /// branch only while it is still at the base this preparation recorded,
    /// and the manifest. Any failure keeps the preparation in recovery.
    pub(super) fn remove_unstarted_task_worktree(&self, project: &Map<String, Value>, worktree_id: &str) -> Result<(), ProjectFailure> {
        let path = PathBuf::from(value_string(project, "worktree_path")?);
        if !path.is_absolute() || !self.unstarted_task_worktree(worktree_id, &path) {
            return Err(ProjectFailure::recovery("unexpected activated snapshot artifact"));
        }
        let has_folder = std::fs::symlink_metadata(&path).is_ok();
        let has_manifest = self.manifests_dir.symlink_metadata(format!("{worktree_id}.json")).is_ok();
        if !has_folder && !has_manifest {
            return Ok(());
        }
        let source = self.open_project_source(project)?;
        if has_folder {
            self.git_at(&source._root, [OsStr::new("worktree"), OsStr::new("remove"), OsStr::new("--force"), path.as_os_str()])?;
        }
        let branch = task_branch(worktree_id);
        let base = value_string(project, "base_commit")?;
        let at = self.git_allow_status_at(&source._root, ["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")], &[0, 1])?;
        if at.status.success() && trim_output(&at)? == base {
            self.git_allow_status_at(&source._root, ["branch", "-D", branch.as_str()], &[0, 1])?;
        }
        if has_manifest {
            self.manifests_dir.remove_file(format!("{worktree_id}.json")).map_err(|error| {
                ProjectFailure::recovery(format!("could not remove the unstarted manifest: {error}"))
            })?;
        }
        Ok(())
    }

    /// Deletes a task worktree's folder (discard cleanup, project deletion):
    /// through Git while the repository is there, which also drops its
    /// record; otherwise by a no-follow erase of the recorded folder. Then
    /// its leftovers in the repository are forgotten.
    pub(super) fn remove_task_worktree_folder(&self, worktree_id: &str, manifest: &Value, expected_inode: Option<u64>) -> Result<(), ProjectFailure> {
        let path = self.workspace_path(worktree_id, manifest)?;
        // Only ever a task worktree folder: named after this task, with a
        // `.git` link file into the project's own repository, and (cleanup)
        // the very folder whose identity the cleanup intent recorded. A
        // damaged record can never point this at a user's folder.
        let short: String = worktree_id.trim_start_matches("worktree_").chars().take(8).collect();
        if path.file_name().and_then(|name| name.to_str()) != Some(format!("task-{short}").as_str()) {
            return Err(ProjectFailure::manifest("not a task worktree folder"));
        }
        // A task of the folder's earlier record: its record is in that old
        // history, not the project's own repository.
        let mut earlier = None;
        if std::fs::symlink_metadata(&path).is_ok() {
            let root = manifest_source_root(manifest).ok_or_else(|| ProjectFailure::manifest("task worktree has no project"))?;
            let link = std::fs::symlink_metadata(path.join(".git")).ok()
                .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= LINK_MAX_BYTES)
                .and_then(|_| std::fs::read_to_string(path.join(".git")).ok());
            let current = self.repository_git_path(Path::new(root)).ok();
            let repository = current.as_deref()
                .ok_or_else(|| ProjectFailure::manifest("no repository"))
                .and_then(canonical_protocol_path_without_existing)
                .unwrap_or_else(|_| format!("{}/.git", root.trim_end_matches('/')));
            let prefix = format!("gitdir: {}/worktrees/", repository.trim_end_matches('/'));
            if !link.as_deref().is_some_and(|link| link.starts_with(&prefix)) {
                let current = current.and_then(|path| std::fs::canonicalize(path).ok());
                earlier = self.earlier_record_worktree(&path, current.as_deref());
            }
            if earlier.is_none() && !link.is_some_and(|link| link.starts_with(&prefix)) {
                // A visible folder the user emptied (or whose `.git` they
                // removed) is only taken away when nothing is left in it.
                let empty = std::fs::read_dir(&path).is_ok_and(|mut entries| entries.next().is_none());
                if empty {
                    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                        return Err(ProjectFailure::manifest("task worktree path has no parent"));
                    };
                    let _ = open_absolute_directory_capability(parent)?.remove_dir(name);
                    self.forget_task_worktree(manifest);
                    return Ok(());
                }
                return Err(ProjectFailure::manifest(
                    "the task folder lost its link to the project; remove the folder yourself or restore its .git file",
                ));
            }
            if let Some(inode) = expected_inode {
                use std::os::unix::fs::MetadataExt as _;
                let current = std::fs::symlink_metadata(&path).map_err(|_| ProjectFailure::manifest("task worktree folder unavailable"))?;
                if current.ino() != inode || !current.is_dir() || current.file_type().is_symlink() {
                    return Err(ProjectFailure::manifest("the task worktree folder changed since cleanup began"));
                }
            }
        }
        if earlier.is_none() && std::fs::symlink_metadata(&path).is_ok() {
            if let Some(root) = manifest_source_root(manifest).and_then(|root| open_absolute_directory_capability(Path::new(root)).ok()) {
                let _ = self.git_allow_status_at(&root, [OsStr::new("worktree"), OsStr::new("remove"), OsStr::new("--force"), path.as_os_str()], &[0, 128]);
            }
        }
        if std::fs::symlink_metadata(&path).is_ok() {
            let folder = open_absolute_directory_capability(&path)?;
            workspace_cleanup::walk(&folder, true, 0, &mut 0)?;
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(ProjectFailure::manifest("task worktree path has no parent"));
            };
            open_absolute_directory_capability(parent)?.remove_dir(name).map_err(|error| {
                ProjectFailure::recovery(format!("could not remove the task worktree folder: {error}"))
            })?;
        }
        if let Some((history, record)) = earlier {
            // Its record in the old history pointed at the folder just
            // removed (checked above); the history goes once nothing uses it.
            if let Ok(records) = open_absolute_directory_capability(&history).and_then(|dir| open_real_child_dir(&dir, "worktrees")) {
                if let Ok(dir) = open_real_child_dir(&records, &record) {
                    if workspace_cleanup::walk(&dir, true, 0, &mut 0).is_ok() {
                        let _ = records.remove_dir(&record);
                    }
                }
                if records.entries().is_ok_and(|mut entries| entries.next().is_none()) {
                    if let Ok(dir) = open_absolute_directory_capability(&history) {
                        let _ = dir.remove_dir("worktrees");
                    }
                }
            }
            self.retire_orphaned_history(&history);
            return Ok(());
        }
        self.forget_task_worktree(manifest);
        Ok(())
    }

    /// Branches and tags of the user's repository with their commits.
    fn source_refs(&self, root: &Dir) -> Result<Vec<String>, ProjectFailure> {
        let output = self.git_at(root, ["for-each-ref", "--format=%(refname) %(objectname)"])?;
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| ProjectFailure::manifest("repository refs are not UTF-8"))?;
        let mut refs: Vec<String> = text.lines().map(str::to_owned).collect();
        refs.sort();
        Ok(refs)
    }

    /// Whether this project's tasks can be real worktrees: the folder is on a
    /// branch (there is something to merge into) and the repository uses no
    /// content filters (LFS) or merge drivers. Otherwise tasks keep the v1
    /// copy, which needs neither.
    /// A repository with content filters (Git LFS) cannot be served by a v1
    /// copy either: its clean check compares raw bytes against the stored
    /// pointers. Such a project gets a clear "not supported yet" instead.
    pub(super) fn task_worktrees_supported(&self, storage: &Storage, project_id: &str) -> Result<bool, ProjectFailure> {
        let project = required_project(storage, project_id)?;
        let source = self.open_project_source(&project)?;
        if self.content_filters_configured(&source._root)?
            || self.content_filters_requested(&source._root, &source.git_dir, &source.base_commit)?
        {
            return Err(ProjectFailure::manifest(
                "repositories that use Git LFS or other content filters are not supported yet",
            ));
        }
        let on_branch = self.git_allow_status_at(&source._root, ["symbolic-ref", "-q", "HEAD"], &[0, 1])?.status.success();
        let drivers = self.git_allow_status_at(&source._root, ["config", "--get-regexp", r"^merge\..*\.driver$"], &[0, 1])?.status.code() == Some(0);
        Ok(on_branch && !drivers)
    }

    /// A task's workspace as a real worktree on a new task branch, made from
    /// the commit the user's folder is on now. Uncommitted edits in the user's
    /// folder stay there; the task starts from the last commit.
    pub(super) fn create_git_worktree(
        &mut self,
        storage: &mut Storage,
        project_id: &str,
        request_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let project = required_project(storage, project_id)?;
        if !snapshot_export_blockers(storage)?.is_empty() {
            return Err(ProjectFailure::recovery(
                "durable recovery blockers must be resolved before another workspace is made",
            ));
        }
        let lifecycle = value_string(&project, "lifecycle")?;
        if !matches!(lifecycle, "opened" | "active" | "abandoned") {
            return Err(ProjectFailure::recovery("project cannot make a task workspace now"));
        }
        let task_copy = is_task_copy(storage, &project)?;
        let project_copy = (lifecycle == "active"
            && !task_copy
            && self.validate_active_project(storage, project_id).is_ok())
        .then(|| project.clone());

        let source = self.open_project_source(&project)?;
        if self.worktrees_root.starts_with(&source.canonical_root) {
            return Err(ProjectFailure::manifest("managed worktree root is inside the original project"));
        }
        // Content filters (Git LFS and the like) would run helper programs
        // while checking out; not supported yet (decision 2026-11-18).
        if self.content_filters_configured(&source._root)?
            || self.content_filters_requested(&source._root, &source.git_dir, &source.base_commit)?
        {
            return Err(ProjectFailure::manifest(
                "repositories with Git content filters (such as Git LFS) are not supported yet",
            ));
        }
        for lock in ["index.lock", "HEAD.lock", "config.lock", "packed-refs.lock"] {
            if source.git_dir.symlink_metadata(lock).is_ok() {
                return Err(ProjectFailure::dirty(
                    "another Git command is working in the original; nothing was prepared",
                ));
            }
        }
        let base_ref = {
            let output = self.git_allow_status_at(&source._root, ["symbolic-ref", "-q", "--short", "HEAD"], &[0, 1])?;
            output.status.success().then(|| trim_output(&output).map(str::to_owned)).transpose()?
        };

        let worktree_id = format!("worktree_{}", Uuid::new_v4().simple());
        let workspace_identity = format!("workspace_{}", Uuid::new_v4().simple());
        let branch = task_branch(&worktree_id);
        let worktree_path = self.new_task_worktree_path(storage, &source.canonical_root, &worktree_id)?;
        let manifest_path = self.manifests_root.join(format!("{worktree_id}.json"));
        if worktree_path.exists() || manifest_path.exists() {
            return Err(ProjectFailure::recovery("new workspace identity unexpectedly already exists"));
        }
        let taken = self.git_allow_status_at(
            &source._root,
            ["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")],
            &[0, 1],
        )?;
        if taken.status.success() {
            return Err(ProjectFailure::recovery("task branch name is already taken"));
        }
        let active = ActiveProject {
            kind: WorkspaceKind::GitWorktree { branch: branch.clone(), base_ref },
            project_id: project_id.to_owned(),
            canonical_git_root: source.canonical_root.clone(),
            worktree_id: worktree_id.clone(),
            worktree_path: worktree_path.clone(),
            base_commit: source.base_commit.clone(),
            base_tree: source.base_tree.clone(),
            object_format: source.object_format.clone(),
            source_identity: source.source_identity.clone(),
            workspace_identity: workspace_identity.clone(),
            manifest_path: manifest_path.clone(),
            lock_id: format!("lock_{worktree_id}"),
        };
        // Refuse a base branch name the contract cannot carry before anything
        // is recorded or made.
        let manifest = manifest_value(&active, "active")?;
        validate_project_payload_v1("worktreeManifest", &manifest).map_err(|error| {
            ProjectFailure::manifest(format!("task worktree manifest is invalid: {error}"))
        })?;
        let refs_before = self.source_refs(&source._root)?;

        let worktree_text = canonical_protocol_path_without_existing(&worktree_path)?;
        let root_text = canonical_protocol_path(&source.canonical_root)?;
        let lock_id = active.lock_id.clone();
        let preparation_lock = json!({
            "lock_id": lock_id,
            "project_id": project_id,
            "writer_run_id": Value::Null,
            "worktree_path": worktree_text,
            "poisoned": false,
            "recovery_required": true,
            "evidence": {"phase":"beforeSnapshotExport","launchFenceVersion":1,"request_id":request_id,
                "workspace_identity":workspace_identity},
        });
        let preparation_project = preserve_project_extensions(
            json!({
                "project_id": project_id,
                "canonical_path": root_text,
                "selected_path": value_string(&project, "selected_path")?,
                "base_commit": source.base_commit,
                "base_tree": source.base_tree,
                "object_format": source.object_format,
                "source_identity": source.source_identity,
                "workspace_identity": workspace_identity,
                "lifecycle": "recoveryRequired",
                "worktree_id": worktree_id,
                "worktree_path": worktree_text,
                "manifest_path": canonical_protocol_path_without_existing(&manifest_path)?,
                "lock_id": lock_id,
            }),
            &project,
        )?;
        snapshot_recovery::write_pair(
            storage, &format!("snapshot_prepare_{worktree_id}"), request_id,
            preparation_project, preparation_lock, &[],
        )?;

        // Until the workspace is handed over, a failure removes the worktree
        // and its branch again and puts the project back as it was.
        let path_arg = worktree_path.as_os_str().to_owned();
        macro_rules! undo {
            ($step:expr) => {
                match $step {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = self.git_allow_status_at(
                            &source._root,
                            [OsStr::new("worktree"), OsStr::new("remove"), OsStr::new("--force"), path_arg.as_os_str()],
                            &[0, 128],
                        );
                        // Only the branch this call made: absent before, and
                        // still at the base. Never a repository-wide prune
                        // (it would drop the user's own worktrees on an
                        // unplugged disk).
                        let ours = self.git_allow_status_at(
                            &source._root,
                            ["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")],
                            &[0, 1],
                        ).ok().filter(|output| output.status.success())
                            .and_then(|output| trim_output(&output).ok().map(|head| head == active.base_commit))
                            .unwrap_or(false);
                        if ours {
                            let _ = self.git_allow_status_at(&source._root, ["branch", "-D", branch.as_str()], &[0, 1]);
                        }
                        return Err(self.fail_unfinished_copy(
                            storage, &project, &lock_id, &workspace_identity, &worktree_id,
                            None, request_id, error,
                        ));
                    }
                }
            };
        }
        undo!(self.git_at(
            &source._root,
            [
                OsStr::new("worktree"), OsStr::new("add"), OsStr::new("--quiet"),
                OsStr::new("-b"), OsStr::new(branch.as_str()),
                path_arg.as_os_str(), OsStr::new(active.base_commit.as_str()),
            ],
        ));
        #[cfg(test)]
        if STOP_AFTER_WORKTREE_ADD.with(std::cell::Cell::get) {
            return Err(ProjectFailure::recovery("injected stop after worktree add"));
        }
        // Only the task branch may have appeared among the user's refs.
        let mut expected_refs = refs_before;
        expected_refs.push(format!("refs/heads/{branch} {}", active.base_commit));
        expected_refs.sort();
        if undo!(self.source_refs(&source._root)) != expected_refs {
            undo!(Err::<(), _>(ProjectFailure::recovery(
                "the repository's branches changed while the task workspace was made",
            )));
        }
        let worktree = undo!(self.worktree_capability(&active));
        undo!(self.verify_worktree_repository(&worktree, &active));
        if undo!(available_space(&worktree)) < SNAPSHOT_DISK_RESERVE_BYTES_V1 {
            undo!(Err::<(), _>(ProjectFailure::recovery(
                "task workspace would leave less than 512 MiB of free disk space",
            )));
        }
        drop(worktree);

        // From here on the workspace has durable pieces outside Git (manifest,
        // ledger): failures stay fenced for recovery like a v1 copy.
        write_json_atomic(&self.manifests_dir, &format!("{worktree_id}.json"), &manifest)?;
        let project_now = required_project(storage, project_id)?;
        self.persist_pre_activation(storage, &active, &project_now, request_id)?;
        if read_bounded_json(&self.manifests_dir, &format!("{worktree_id}.json"))? != manifest {
            return Err(ProjectFailure::manifest("external manifest changed before activation"));
        }
        let worktree = self.worktree_capability(&active)?;
        self.verify_worktree_repository(&worktree, &active)?;
        match &project_copy {
            Some(view) => self.persist_task_copy_active(storage, &active, view, request_id)?,
            None => self.persist_active(storage, &active, &project_now, request_id)?,
        }
        Ok(ProjectAction { result: manifest, ..ProjectAction::default() })
    }
}
