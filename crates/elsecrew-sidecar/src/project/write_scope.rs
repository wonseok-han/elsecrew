//! Where a task writes, as real paths (ADE multi-project PR-A, review B1).
//! Two tasks share a write lock when these meet: the same repository's Git
//! folder, or folders inside one another (a task's workspace, its project's
//! own folder), links resolved. Anything that cannot be resolved is no scope
//! at all, and the caller then treats it as meeting everything.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WriteScope {
    /// Real folders written: the workspace and the project's own folder.
    folders: Vec<PathBuf>,
    /// The repository's real Git folder (a plain folder's history in app data).
    git: PathBuf,
}

impl WriteScope {
    /// Whether two scopes can write the same files or the same repository.
    pub(crate) fn meets(&self, other: &WriteScope) -> bool {
        same_path(&self.git, &other.git)
            || self.folders.iter().any(|mine| other.folders.iter().any(|theirs| path_within(mine, theirs) || path_within(theirs, mine)))
    }
}

/// Paths compare by parts (`elsecrew_core::path_key`, tested on every CI platform).
use elsecrew_core::path_key::{components_within, path_key_components};

fn components(path: &Path) -> Vec<String> {
    path_key_components(&path.to_string_lossy(), cfg!(windows), cfg!(any(windows, target_os = "macos")))
}

fn same_path(left: &Path, right: &Path) -> bool {
    components(left) == components(right)
}

/// `inner` is `outer` or inside it, by whole path components.
pub(crate) fn path_within(inner: &Path, outer: &Path) -> bool {
    components_within(&components(inner), &components(outer))
}

fn real(path: &Path) -> Result<PathBuf, ProjectFailure> {
    std::fs::canonicalize(path).map_err(|error| ProjectFailure::manifest(format!("write scope path unavailable: {error}")))
}

/// The folder Git keeps a repository's objects and refs in: the Git folder,
/// or the one its `commondir` names (PR #18 re-review B1).
fn common_git(git: &Path) -> Result<PathBuf, ProjectFailure> {
    match std::fs::read_to_string(git.join("commondir")) {
        Ok(text) => {
            let named = PathBuf::from(text.trim_end_matches('\n'));
            real(&if named.is_absolute() { named } else { git.join(named) })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => real(git),
        Err(_) => Err(ProjectFailure::manifest("the repository's common Git folder cannot be read")),
    }
}

impl ProjectManager {
    /// The folders a task's agents, saves, merges and cleanups write.
    pub(crate) fn task_write_scope(&self, storage: &Storage, address: &WorkspaceAddress) -> Result<WriteScope, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        let source = real(&active.canonical_git_root)?;
        Ok(WriteScope {
            folders: vec![real(&active.worktree_path)?, source.clone()],
            git: common_git(&real(&self.repository_git_path(&source)?)?)?,
        })
    }

    /// A project's own folder (a save to its view, a merge into it).
    pub(crate) fn project_write_scope(&self, storage: &Storage, project_id: &str) -> Result<WriteScope, ProjectFailure> {
        let path = storage
            .project_catalog_path(project_id)?
            .ok_or_else(|| ProjectFailure::manifest("saved project was not found"))?;
        let source = real(Path::new(&path))?;
        Ok(WriteScope { folders: vec![source.clone()], git: common_git(&real(&self.repository_git_path(&source)?)?)? })
    }

    /// Every task workspace on record (active or not): a new project, a new
    /// workspace folder or a new task folder may not overlap any of them.
    pub(crate) fn recorded_workspace_folders(&self, storage: &Storage) -> Result<Vec<PathBuf>, ProjectFailure> {
        let mut folders = Vec::new();
        for lock in storage.projection_records("lock")? {
            if let Some(path) = lock.get("worktree_path").and_then(Value::as_str) {
                // A workspace whose folder is gone is compared as recorded; one
                // that cannot be read otherwise refuses (fail closed).
                folders.push(match std::fs::canonicalize(path) {
                    Ok(real) => real,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => PathBuf::from(path),
                    Err(_) => return Err(ProjectFailure::manifest("a task's workspace folder cannot be resolved")),
                });
            }
        }
        folders.sort();
        folders.dedup();
        Ok(folders)
    }

    /// Refuses a folder that lies inside, or holds, a recorded task workspace.
    /// The folder must resolve: one that cannot is refused (fail closed).
    pub(crate) fn refuse_workspace_overlap(&self, storage: &Storage, folder: &Path, what: &str) -> Result<(), ProjectFailure> {
        let folder = std::fs::canonicalize(folder).map_err(|_| ProjectFailure::manifest(format!("{what} cannot be resolved")))?;
        for workspace in self.recorded_workspace_folders(storage)? {
            if path_within(&folder, &workspace) || path_within(&workspace, &folder) {
                return Err(ProjectFailure::manifest(format!("{what} overlaps a task's workspace")));
            }
        }
        Ok(())
    }

    /// A new task's folder (and its project folder under the workspace
    /// folder) may not be inside, at or around another task's workspace.
    /// Asked before the folders are made, on the deepest part that exists.
    /// Also outside the project itself, the app's data and every saved
    /// project — all asked before anything is made (PR #18 review B1, 3rd round).
    pub(crate) fn refuse_new_task_folder(&self, storage: &Storage, source_root: &Path, project_dir: &Path, task_dir: &Path) -> Result<(), ProjectFailure> {
        let resolve = |path: &Path| -> PathBuf {
            // The existing part resolved, the rest appended as named.
            let mut existing = path.to_path_buf();
            let mut rest = Vec::new();
            while std::fs::symlink_metadata(&existing).is_err() {
                match (existing.file_name().map(|name| name.to_os_string()), existing.parent().map(Path::to_path_buf)) {
                    (Some(name), Some(parent)) => { rest.push(name); existing = parent; }
                    _ => break,
                }
            }
            let mut resolved = std::fs::canonicalize(&existing).unwrap_or(existing);
            for name in rest.into_iter().rev() { resolved.push(name); }
            resolved
        };
        let (project_dir, task_dir) = (resolve(project_dir), resolve(task_dir));
        let meets = |other: &Path| path_within(&project_dir, other) || path_within(other, &project_dir);
        if meets(&resolve(source_root)) || meets(&resolve(&self.data_root)) {
            return Err(ProjectFailure::manifest("the workspace folder overlaps the project or the app's data"));
        }
        for project in storage.projection_records("project")? {
            if let Some(root) = project.get("canonical_path").and_then(Value::as_str) {
                if meets(&resolve(Path::new(root))) {
                    return Err(ProjectFailure::manifest("the workspace folder overlaps a saved project; choose another in settings"));
                }
            }
        }
        for workspace in self.recorded_workspace_folders(storage)? {
            if path_within(&project_dir, &workspace) || path_within(&task_dir, &workspace) || path_within(&workspace, &task_dir) {
                return Err(ProjectFailure::manifest("the new task's folder overlaps another task's workspace"));
            }
        }
        Ok(())
    }
}

