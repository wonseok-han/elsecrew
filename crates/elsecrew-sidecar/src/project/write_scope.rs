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
        self.git == other.git
            || self.folders.iter().any(|mine| other.folders.iter().any(|theirs| mine.starts_with(theirs) || theirs.starts_with(mine)))
    }
}

fn real(path: &Path) -> Result<PathBuf, ProjectFailure> {
    std::fs::canonicalize(path).map_err(|error| ProjectFailure::manifest(format!("write scope path unavailable: {error}")))
}

impl ProjectManager {
    /// The folders a task's agents, saves, merges and cleanups write.
    pub(crate) fn task_write_scope(&self, storage: &Storage, address: &WorkspaceAddress) -> Result<WriteScope, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        let source = real(&active.canonical_git_root)?;
        Ok(WriteScope {
            folders: vec![real(&active.worktree_path)?, source.clone()],
            git: real(&self.repository_git_path(&source)?)?,
        })
    }

    /// A project's own folder (a save to its view, a merge into it).
    pub(crate) fn project_write_scope(&self, storage: &Storage, project_id: &str) -> Result<WriteScope, ProjectFailure> {
        let path = storage
            .project_catalog_path(project_id)?
            .ok_or_else(|| ProjectFailure::manifest("saved project was not found"))?;
        let source = real(Path::new(&path))?;
        Ok(WriteScope { folders: vec![source.clone()], git: real(&self.repository_git_path(&source)?)? })
    }

    /// Every task workspace on record (active or not): a new project, a new
    /// workspace folder or a new task folder may not overlap any of them.
    pub(crate) fn recorded_workspace_folders(&self, storage: &Storage) -> Result<Vec<PathBuf>, ProjectFailure> {
        let mut folders = Vec::new();
        for lock in storage.projection_records("lock")? {
            if let Some(path) = lock.get("worktree_path").and_then(Value::as_str) {
                folders.push(std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path)));
            }
        }
        folders.sort();
        folders.dedup();
        Ok(folders)
    }

    /// Refuses a folder that lies inside, or holds, a recorded task workspace.
    pub(crate) fn refuse_workspace_overlap(&self, storage: &Storage, folder: &Path, what: &str) -> Result<(), ProjectFailure> {
        let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        for workspace in self.recorded_workspace_folders(storage)? {
            if folder.starts_with(&workspace) || workspace.starts_with(&folder) {
                return Err(ProjectFailure::manifest(format!("{what} overlaps a task's workspace")));
            }
        }
        Ok(())
    }
}
