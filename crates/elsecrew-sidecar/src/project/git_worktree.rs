//! Task workspaces as real `git worktree`s of the user's repository
//! (docs/proposals/m5-real-worktrees.md §3.2). The task gets its own branch
//! `elsecrew/<task>`; the user's folder and its checked-out branch are never
//! touched. In the user's `.git` only the worktree record and the task branch
//! appear. Every Git command runs without hooks, fsmonitor or remotes
//! (`fixed_git_command`).
use super::*;

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

/// `elsecrew/task-<8 hex>`: unique per workspace, valid for Git and for the
/// manifest contract.
fn task_branch(worktree_id: &str) -> String {
    let short: String = worktree_id.trim_start_matches("worktree_").chars().take(8).collect();
    format!("{TASK_BRANCH_PREFIX_V2}task-{short}")
}

impl ProjectManager {
    /// The worktree's `.git` file names `<root>/.git/worktrees/<name>`, and
    /// that record points back at this very worktree.
    pub(super) fn verify_worktree_link(&self, worktree: &Dir, active: &ActiveProject) -> Result<(), ProjectFailure> {
        let root = canonical_protocol_path_without_existing(&active.canonical_git_root)?;
        let link = read_small_file(worktree, ".git", "worktree Git link")?;
        let prefix = format!("gitdir: {}/.git/worktrees/", root.trim_end_matches('/'));
        let name = link
            .strip_suffix('\n')
            .and_then(|line| line.strip_prefix(&prefix))
            .filter(|name| !name.is_empty() && !name.contains('/') && !name.starts_with('.'))
            .ok_or_else(|| ProjectFailure::manifest("worktree Git link does not name the project's repository"))?;
        let git_dir = open_absolute_directory_capability(&active.canonical_git_root.join(".git"))?;
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
        // A filter added to the repository since the workspace was made would
        // run its program on `add`; refuse instead of recording.
        if self.content_filters_configured(&worktree)? {
            return Err(ProjectFailure::manifest("the repository now uses Git content filters; step not recorded"));
        }
        self.git_at(&worktree, ["add", "-A"])?;
        let staged = self.git_allow_status_at(&worktree, ["diff", "--cached", "--quiet"], &[0, 1])?;
        if staged.status.code() == Some(0) {
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
    fn content_filters_requested(&self, root: &Dir, git_dir: &Dir, commit: &str) -> Result<bool, ProjectFailure> {
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

    /// Branches and tags of the user's repository with their commits.
    fn source_refs(&self, root: &Dir) -> Result<Vec<String>, ProjectFailure> {
        let output = self.git_at(root, ["for-each-ref", "--format=%(refname) %(objectname)"])?;
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| ProjectFailure::manifest("repository refs are not UTF-8"))?;
        let mut refs: Vec<String> = text.lines().map(str::to_owned).collect();
        refs.sort();
        Ok(refs)
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
        let worktree_path = self.worktrees_root.join(&worktree_id);
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
        // Only the task branch may have appeared among the user's refs.
        let mut expected_refs = refs_before;
        expected_refs.push(format!("refs/heads/{branch} {}", active.base_commit));
        expected_refs.sort();
        if undo!(self.source_refs(&source._root)) != expected_refs {
            undo!(Err::<(), _>(ProjectFailure::recovery(
                "the repository's branches changed while the task workspace was made",
            )));
        }
        let worktree = undo!(open_real_child_dir(&self.worktrees_dir, &worktree_id));
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
