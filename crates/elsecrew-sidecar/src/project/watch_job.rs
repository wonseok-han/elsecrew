//! One owned, cancellable validation at a time. Results are hints for a fresh
//! binding check, never authority for file writes or agent execution.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

pub(super) struct WatchJob {
    pub key: String,
    pub stream: String,
    active: ActiveProject,
    held_worktree: Option<Dir>,
    held_git: Option<Dir>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), ProjectFailure>>>,
}

pub(super) struct Completed {
    pub key: String,
    pub stream: String,
    pub active: ActiveProject,
    pub succeeded: bool,
    held_worktree: Dir,
    held_git: Dir,
}

impl Completed {
    pub fn matches_capability(&self, current: &Dir) -> Result<bool, ProjectFailure> {
        same_directory(&self.held_worktree, current).and_then(|same| {
            Ok(same && same_directory(&self.held_git, &open_real_child_dir(current, ".git")?)?)
        })
    }
}

fn same_directory(left: &Dir, right: &Dir) -> Result<bool, ProjectFailure> {
    #[cfg(unix)]
    {
        Ok(capability_directory_identity(left)? == capability_directory_identity(right)?)
    }
    #[cfg(windows)]
    {
        Ok(windows_fs::identity(left, true)? == windows_fs::identity(right, true)?)
    }
}

impl WatchJob {
    pub fn finished(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
    }

    pub fn complete(mut self) -> Completed {
        let succeeded = matches!(self.worker.take().expect("owned worker").join(), Ok(Ok(())));
        Completed {
            key: self.key.clone(),
            stream: self.stream.clone(),
            active: self.active.clone(),
            succeeded,
            held_worktree: self.held_worktree.take().expect("held worktree"),
            held_git: self.held_git.take().expect("held Git directory"),
        }
    }
}

impl Drop for WatchJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        // Joining includes cancellation, pipe readers and child reaping. Never
        // detach background work during prepareQuit or manager destruction.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl ProjectManager {
    pub(super) fn start_watch_job(
        &self,
        key: String,
        stream: String,
        active: ActiveProject,
        worktree: Dir,
    ) -> Result<WatchJob, ProjectFailure> {
        let clone_dir = |dir: &Dir| {
            dir.try_clone().map_err(|error| {
                ProjectFailure::recovery(format!("could not clone watcher capability: {error}"))
            })
        };
        let held_worktree = clone_dir(&worktree)?;
        let held_git = open_real_child_dir(&worktree, ".git")?;
        let cancel = Arc::new(AtomicBool::new(false));
        // Only copy directory capabilities needed by the existing verifier.
        // No Storage connection, application owner, pending save or watcher is
        // shared with the worker, and mutation features remain disabled.
        let verifier = Self {
            #[cfg(all(test, windows))]
            windows_git_diagnostic: self.windows_git_diagnostic,
            preparation_cancel: None,
            source_writes_enabled: false,
            application_cleanup_enabled: false,
            application_failure: None,
            application_cancel: Default::default(),
            #[cfg(any(target_os = "macos", windows))]
            application_job: None,
            #[cfg(any(unix, windows))]
            application_confirmations: Default::default(),
            data_root: self.data_root.clone(),
            data_dir: clone_dir(&self.data_dir)?,
            worktrees_root: self.worktrees_root.clone(),
            manifests_root: self.manifests_root.clone(),
            worktrees_dir: clone_dir(&self.worktrees_dir)?,
            manifests_dir: clone_dir(&self.manifests_dir)?,
            save_staging_root: self.save_staging_root.clone(),
            save_staging_dir: clone_dir(&self.save_staging_dir)?,
            pending_saves: HashMap::new(),
            watchers: HashMap::new(),
            last_polled_watcher: None,
            watch_job: None,
            watch_cancel: Some(cancel.clone()),
        };
        let captured = active.clone();
        let worker = std::thread::Builder::new()
            .name("snapshot-watch".into())
            .spawn(move || verifier.verify_snapshot_repository(&worktree, &captured))
            .map_err(|error| {
                ProjectFailure::recovery(format!("could not start watcher validation: {error}"))
            })?;
        Ok(WatchJob {
            key,
            stream,
            active,
            cancel,
            worker: Some(worker),
            held_worktree: Some(held_worktree),
            held_git: Some(held_git),
        })
    }
}
