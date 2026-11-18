//! "main에 합치기" for a task worktree (docs/proposals/m5-real-worktrees.md
//! §3.3, task = PR). The task branch first takes the base branch's newest
//! commits inside the task's own worktree; the base branch then only moves
//! forward. Nothing is ever resolved in the user's folder: a conflict is
//! undone in the worktree and reported with its files.
use super::*;

/// Files listed for a conflict; the count is always exact.
const CONFLICT_PATHS_MAX: usize = 100;

/// The app's identity and a Git without side effects for the commits a
/// merge makes (hooks, fsmonitor and remotes are off in every command).
const COMMIT_SETTINGS: [&str; 10] = [
    "-c", "user.name=Elsecrew", "-c", "user.email=elsecrew@local",
    "-c", "commit.gpgsign=false", "-c", "gc.auto=0", "-c", "maintenance.auto=false",
];

struct MergeState {
    branch: String,
    base_ref: Option<String>,
    head: String,
    base_tip: Option<String>,
    /// Task commits the base branch does not have.
    ahead: u64,
    /// Base commits the task branch does not have.
    behind: u64,
    /// The user's folder is on the base branch (moves with the merge).
    base_checked_out: bool,
}

impl MergeState {
    fn value(&self, uncommitted: bool) -> Value {
        let blocked = if self.base_ref.is_none() {
            Some("noBaseBranch")
        } else if self.base_tip.is_none() {
            Some("baseBranchMissing")
        } else if self.ahead == 0 && !uncommitted {
            Some("nothingToMerge")
        } else {
            None
        };
        json!({
            "kind":"gitWorktree",
            "branch":self.branch,
            "baseRef":self.base_ref,
            "head":self.head,
            "baseTip":self.base_tip,
            "ahead":self.ahead,
            "behind":self.behind,
            "uncommitted":uncommitted,
            "baseCheckedOut":self.base_checked_out,
            "blocked":blocked,
        })
    }
}

fn count(output: &Output) -> Result<u64, ProjectFailure> {
    trim_output(output)?.parse().map_err(|_| ProjectFailure::manifest("Git count is not a number"))
}

impl ProjectManager {
    fn merge_state(&self, source_root: &Dir, worktree: &Dir, active: &ActiveProject) -> Result<MergeState, ProjectFailure> {
        let WorkspaceKind::GitWorktree { branch, base_ref } = &active.kind else {
            return Err(ProjectFailure::manifest("not a task worktree"));
        };
        let head = trim_output(&self.git_at(worktree, ["rev-parse", "--verify", "HEAD^{commit}"])?)?.to_owned();
        let base_tip = match base_ref {
            Some(base) => {
                let found = self.git_allow_status_at(
                    source_root,
                    ["rev-parse", "--verify", "--quiet", &format!("refs/heads/{base}^{{commit}}")],
                    &[0, 1],
                )?;
                found.status.success().then(|| trim_output(&found).map(str::to_owned)).transpose()?
            }
            None => None,
        };
        let (ahead, behind) = match &base_tip {
            Some(tip) => (
                count(&self.git_at(worktree, ["rev-list", "--count", &format!("{tip}..HEAD")])?)?,
                count(&self.git_at(worktree, ["rev-list", "--count", &format!("HEAD..{tip}")])?)?,
            ),
            None => (0, 0),
        };
        let checked_out = self.git_allow_status_at(source_root, ["symbolic-ref", "-q", "HEAD"], &[0, 1])?;
        let base_checked_out = base_ref.as_ref().is_some_and(|base| {
            checked_out.status.success()
                && trim_output(&checked_out).is_ok_and(|current| current == format!("refs/heads/{base}"))
        });
        Ok(MergeState { branch: branch.clone(), base_ref: base_ref.clone(), head, base_tip, ahead, behind, base_checked_out })
    }

    fn uncommitted(&self, worktree: &Dir) -> Result<bool, ProjectFailure> {
        let status = self.git_at(worktree, ["status", "--porcelain", "-z", "--untracked-files=normal"])?;
        Ok(!status.stdout.is_empty())
    }

    /// Where the task branch stands against its base, for the result bar.
    /// A v1 copy answers `{"kind":"snapshot"}` and keeps "내 폴더에 반영".
    pub(crate) fn workspace_merge_inspect(&self, storage: &Storage, address: &WorkspaceAddress) -> Result<Value, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        if active.kind == WorkspaceKind::Snapshot {
            return Ok(json!({"kind":"snapshot"}));
        }
        let worktree = self.worktree_capability(&active)?;
        let source_root = open_absolute_directory_capability(&active.canonical_git_root)?;
        let state = self.merge_state(&source_root, &worktree, &active)?;
        Ok(state.value(self.uncommitted(&worktree)?))
    }

    /// Merges the task into its base branch. `expected_head` and
    /// `expected_base` are what the user saw; anything newer is refused so
    /// the merge is exactly the one confirmed.
    pub(crate) fn workspace_merge(
        &self,
        storage: &Storage,
        address: &WorkspaceAddress,
        expected_head: &str,
        expected_base: &str,
    ) -> Result<Value, ProjectFailure> {
        let active = self.validate_workspace_address(storage, address)?;
        if active.kind == WorkspaceKind::Snapshot {
            return Err(ProjectFailure::manifest("a copy workspace is applied, not merged"));
        }
        let worktree = self.worktree_capability(&active)?;
        let source_root = open_absolute_directory_capability(&active.canonical_git_root)?;
        let source_git = open_real_child_dir(&source_root, ".git")?;
        for lock in ["index.lock", "HEAD.lock", "packed-refs.lock"] {
            if source_git.symlink_metadata(lock).is_ok() {
                return Err(ProjectFailure::dirty("another Git command is working in the original; nothing was merged"));
            }
        }
        if self.content_filters_configured(&source_root)? {
            return Err(ProjectFailure::manifest("the repository now uses Git content filters; nothing was merged"));
        }
        let before = self.merge_state(&source_root, &worktree, &active)?;
        let (Some(base), Some(base_tip)) = (before.base_ref.clone(), before.base_tip.clone()) else {
            return Ok(json!({"outcome":"blocked","reason":"noBaseBranch","merge":before.value(self.uncommitted(&worktree)?)}));
        };
        if before.head != expected_head || base_tip != expected_base {
            return Ok(json!({"outcome":"blocked","reason":"changedSinceReview","merge":before.value(self.uncommitted(&worktree)?)}));
        }
        // Leftover edits since the last step are part of what is merged.
        if self.uncommitted(&worktree)? {
            self.workspace_commit_step(storage, address, "합치기 전 남은 변경")?;
        }
        // 1. The base's newest commits come into the task branch, in the
        //    task's own worktree. A conflict is undone right there.
        let merged_base = self.git_allow_status_at(&worktree, ["merge-base", "--is-ancestor", &base_tip, "HEAD"], &[0, 1])?;
        if merged_base.status.code() != Some(0) {
            let message = format!("{base}의 최신 내용 가져오기");
            let mut args: Vec<&str> = COMMIT_SETTINGS.to_vec();
            args.extend(["merge", "--no-ff", "--no-edit", "--no-verify", "-m", message.as_str(), base_tip.as_str()]);
            let merged = self.git_allow_status_at(&worktree, args, &[0, 1])?;
            if merged.status.code() != Some(0) {
                let conflicted = self.git_at(&worktree, ["diff", "--name-only", "-z", "--diff-filter=U"])?;
                let paths: Vec<String> = conflicted.stdout.split(|byte| *byte == 0)
                    .filter(|path| !path.is_empty())
                    .map(protocol_git_path)
                    .collect::<Result<_, _>>()?;
                self.git_at(&worktree, ["merge", "--abort"])?;
                self.verify_worktree_repository(&worktree, &active)?;
                let after = self.merge_state(&source_root, &worktree, &active)?;
                return Ok(json!({
                    "outcome":"conflict",
                    "conflictCount":paths.len(),
                    "conflicts":paths.into_iter().take(CONFLICT_PATHS_MAX).collect::<Vec<_>>(),
                    "merge":after.value(self.uncommitted(&worktree)?),
                }));
            }
        }
        self.verify_worktree_repository(&worktree, &active)?;
        let new_head = trim_output(&self.git_at(&worktree, ["rev-parse", "--verify", "HEAD^{commit}"])?)?.to_owned();
        // 2. The base branch only moves forward to the task's head.
        let base_ref = format!("refs/heads/{base}");
        let checked_out = self.git_allow_status_at(&source_root, ["symbolic-ref", "-q", "HEAD"], &[0, 1])?;
        let on_base = checked_out.status.success() && trim_output(&checked_out)? == base_ref;
        if on_base {
            // The user's folder follows the branch. Git refuses (and changes
            // nothing) when this would overwrite the user's own edits.
            let moved = self.git_allow_status_at(&source_root, ["merge", "--ff-only", "--no-verify", "-q", &new_head], &[0, 1, 128])?;
            if moved.status.code() != Some(0) {
                let after = self.merge_state(&source_root, &worktree, &active)?;
                return Ok(json!({"outcome":"blocked","reason":"userChangesOverlap","merge":after.value(self.uncommitted(&worktree)?)}));
            }
        } else {
            // Not checked out: one atomic compare-and-swap of the branch.
            let moved = self.git_allow_status_at(&source_root, ["update-ref", &base_ref, &new_head, &base_tip], &[0, 1, 128])?;
            if moved.status.code() != Some(0) {
                let after = self.merge_state(&source_root, &worktree, &active)?;
                return Ok(json!({"outcome":"blocked","reason":"changedSinceReview","merge":after.value(self.uncommitted(&worktree)?)}));
            }
        }
        let after = self.merge_state(&source_root, &worktree, &active)?;
        if after.base_tip.as_deref() != Some(new_head.as_str()) {
            return Err(ProjectFailure::ambiguous("the base branch did not end at the merged commit"));
        }
        Ok(json!({"outcome":"merged","merge":after.value(self.uncommitted(&worktree)?)}))
    }
}
