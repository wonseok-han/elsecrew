//! "main에 합치기" for a task worktree (docs/proposals/m5-real-worktrees.md
//! §3.3, task = PR). The task branch first takes the base branch's newest
//! commits inside the task's own worktree; the base branch then only moves
//! forward. Nothing is ever resolved in the user's folder: a conflict is
//! undone in the worktree and reported with its files, or, when the user
//! hands it to the team, left in the worktree with its conflict marks. The
//! app finishes that merge only once no marks remain.
use super::*;

/// Files listed for a conflict; the count is always exact.
const CONFLICT_PATHS_MAX: usize = 100;

/// The app's identity and a Git without side effects for the commits a
/// merge makes (hooks, fsmonitor and remotes are off in every command).
const COMMIT_SETTINGS: [&str; 10] = [
    "-c", "user.name=Elsecrew", "-c", "user.email=elsecrew@local",
    "-c", "commit.gpgsign=false", "-c", "gc.auto=0", "-c", "maintenance.auto=false",
];

/// What recording a plain folder's own edits before a merge found.
enum FolderEdits {
    Unchanged,
    Recorded(String),
    /// More files or bytes than a folder's first record may take.
    TooLarge,
    /// Another Git repository appeared inside (refused as in the first
    /// record: review M17).
    HasRepository,
}

impl ProjectManager {
    fn on_branch(&self, source_root: &Dir, base: &str) -> Result<bool, ProjectFailure> {
        let checked_out = self.git_allow_status_at(source_root, ["symbolic-ref", "-q", "HEAD"], &[0, 1])?;
        Ok(checked_out.status.success() && trim_output(&checked_out)? == format!("refs/heads/{base}"))
    }

    /// Commits everything changed in a plain folder (its excludes apply),
    /// within the same bounds as the folder's first record.
    fn record_folder_edits(&self, source_root: &Dir) -> Result<FolderEdits, ProjectFailure> {
        let status = self.git_at(source_root, ["status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames"])?;
        let entries: Vec<&[u8]> = status.stdout.split(|byte| *byte == 0).filter(|entry| entry.len() > 3).collect();
        if entries.is_empty() {
            return Ok(FolderEdits::Unchanged);
        }
        let (max_files, max_bytes) = super::empty_project::folder_record_limits();
        if entries.len() > max_files {
            return Ok(FolderEdits::TooLarge);
        }
        let mut bytes = 0u64;
        for entry in &entries {
            let relative = std::str::from_utf8(&entry[3..]).map_err(|_| ProjectFailure::manifest("folder has a file name that is not valid text"))?;
            if relative.ends_with('/') {
                return Ok(FolderEdits::HasRepository);
            }
            if let Ok(meta) = source_root.symlink_metadata(relative) {
                bytes = bytes.saturating_add(meta.len());
            }
            if bytes > max_bytes {
                return Ok(FolderEdits::TooLarge);
            }
        }
        self.git_at(source_root, ["add", "--all", "--", ":/"])?;
        // Listed but nothing to record (review M18).
        if self.git_allow_status_at(source_root, ["diff", "--cached", "--quiet"], &[0, 1])?.status.code() == Some(0) {
            return Ok(FolderEdits::Unchanged);
        }
        let mut args: Vec<&str> = COMMIT_SETTINGS.to_vec();
        args.extend(["commit", "--quiet", "--no-verify", "-m", "내 폴더에서 바꾼 내용"]);
        self.git_at(source_root, args)?;
        let head = self.git_at(source_root, ["rev-parse", "--verify", "HEAD^{commit}"])?;
        Ok(FolderEdits::Recorded(trim_output(&head)?.to_owned()))
    }
}

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
    /// Another worktree of the repository has the base branch checked out;
    /// moving it underneath would leave that checkout inconsistent.
    base_elsewhere: bool,
    /// A base merge left for the team to resolve (MERGE_HEAD present).
    resolving: bool,
    /// Conflicted files that still carry conflict marks.
    unresolved: u64,
    /// The task's first commit is already in the base branch: it was merged
    /// before (so later commits are "new since the merge").
    merged_before: bool,
}

/// A line Git writes around conflicting hunks, whatever marker size a
/// `conflict-marker-size` attribute asks for: 7 or more `<` or `>` then a
/// space or the end. The `=======` separator alone is not counted: it is
/// also a Markdown heading underline, and Git never writes it without the
/// `<`/`>` lines around it.
fn conflict_mark(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let run = |mark: u8| line.iter().take_while(|byte| **byte == mark).count();
    [b'<', b'>'].into_iter().any(|mark| {
        let count = run(mark);
        count >= 7 && matches!(line.get(count), None | Some(b' '))
    })
}

impl MergeState {
    fn value(&self, uncommitted: bool) -> Value {
        let blocked = if self.base_ref.is_none() {
            Some("noBaseBranch")
        } else if self.base_tip.is_none() {
            Some("baseBranchMissing")
        } else if self.base_elsewhere {
            Some("baseInOtherWorktree")
        } else if self.resolving && self.unresolved > 0 {
            Some("conflictsUnresolved")
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
            "resolving":self.resolving,
            "unresolved":self.unresolved,
            "mergedBefore":self.merged_before,
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
        let base_elsewhere = match base_ref {
            Some(base) => self.base_in_other_worktree(source_root, &active.canonical_git_root, base)?,
            None => false,
        };
        let resolving = self.merge_in_progress(worktree)?;
        let unresolved = if resolving { self.unresolved_conflicts(worktree)?.len() as u64 } else { 0 };
        // Only labels the changes panel: anything Git cannot answer is "not
        // merged before", never a failed merge check (review M48).
        let merged_before = base_tip.as_ref().is_some_and(|tip| {
            let range = format!("{}..HEAD", active.base_commit);
            let Ok(commits) = self.git_allow_status_at(worktree, ["rev-list", "--first-parent", range.as_str()], &[0, 128]) else {
                return false;
            };
            // No task commit yet is an empty list.
            let first = commits.status.success().then(|| std::str::from_utf8(&commits.stdout).ok()).flatten()
                .and_then(|text| text.lines().map(str::trim).filter(|line| !line.is_empty()).last());
            first.is_some_and(|first| {
                self.git_allow_status_at(worktree, ["merge-base", "--is-ancestor", first, tip.as_str()], &[0, 1, 128])
                    .is_ok_and(|output| output.status.success())
            })
        });
        Ok(MergeState { branch: branch.clone(), base_ref: base_ref.clone(), head, base_tip, ahead, behind, base_checked_out, base_elsewhere, resolving, unresolved, merged_before })
    }

    /// Whether every conflict is one the team can resolve by editing text:
    /// both sides changed a regular text file (stages 1, 2 and 3), within the
    /// file limit, with Git's marks in it. Binary, deleted, linked or too
    /// large conflicts carry no marks, so "no marks left" would not mean
    /// resolved: those are undone and reported instead.
    fn team_resolvable(&self, worktree: &Dir) -> Result<bool, ProjectFailure> {
        let listed = self.git_at(worktree, ["ls-files", "-u", "-z"])?;
        let mut stages: std::collections::BTreeMap<Vec<u8>, Vec<(String, u8)>> = Default::default();
        for entry in listed.stdout.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
            let tab = entry.iter().position(|byte| *byte == b'\t').ok_or_else(|| ProjectFailure::manifest("unmerged entry has no path"))?;
            let fields: Vec<&str> = std::str::from_utf8(&entry[..tab]).map_err(|_| ProjectFailure::manifest("unmerged entry is not UTF-8"))?.split(' ').collect();
            let (Some(mode), Some(stage)) = (fields.first(), fields.get(2).and_then(|stage| stage.parse::<u8>().ok())) else {
                return Err(ProjectFailure::manifest("unmerged entry is malformed"));
            };
            stages.entry(entry[tab + 1..].to_vec()).or_default().push(((*mode).to_owned(), stage));
        }
        for (path, entries) in stages {
            let mut seen: Vec<u8> = entries.iter().map(|(_, stage)| *stage).collect();
            seen.sort_unstable();
            if seen != [1, 2, 3] || entries.iter().any(|(mode, _)| mode != "100644" && mode != "100755") {
                return Ok(false);
            }
            let path = protocol_git_path(&path)?;
            let Ok(file) = open_file_relative(worktree, &path) else { return Ok(false) };
            let mut bytes = Vec::new();
            file.take(FILE_MAX_BYTES_V1 + 1).read_to_end(&mut bytes)
                .map_err(|error| ProjectFailure::manifest(format!("could not read a conflicted file: {error}")))?;
            if bytes.len() as u64 > FILE_MAX_BYTES_V1 || bytes.contains(&0) || !bytes.split(|byte| *byte == b'\n').any(conflict_mark) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Conflicted files of a merge in progress that still carry conflict
    /// marks. A file the team deleted counts as resolved.
    pub(super) fn unresolved_conflicts(&self, worktree: &Dir) -> Result<Vec<String>, ProjectFailure> {
        let conflicted = self.git_at(worktree, ["diff", "--name-only", "-z", "--diff-filter=U"])?;
        let mut left = Vec::new();
        for path in conflicted.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
            let path = protocol_git_path(path)?;
            let marked = match open_file_relative(worktree, &path) {
                Ok(file) => {
                    let mut bytes = Vec::new();
                    file.take(FILE_MAX_BYTES_V1 + 1).read_to_end(&mut bytes)
                        .map_err(|error| ProjectFailure::manifest(format!("could not read a conflicted file: {error}")))?;
                    bytes.len() as u64 > FILE_MAX_BYTES_V1 || bytes.split(|byte| *byte == b'\n').any(conflict_mark)
                }
                Err(_) if worktree.symlink_metadata(&path).is_err() => false,
                Err(_) => true,
            };
            if marked { left.push(path); }
        }
        Ok(left)
    }

    /// Whether a worktree other than the user's folder has `base` checked out.
    fn base_in_other_worktree(&self, source_root: &Dir, root: &Path, base: &str) -> Result<bool, ProjectFailure> {
        let listed = self.git_at(source_root, ["worktree", "list", "--porcelain", "-z"])?;
        // A plain folder's shadow history lists its main worktree under the
        // history's own path (no core.worktree): that is the folder itself.
        let shadow = self.repository_git_path(root).ok()
            .filter(|path| path != &root.join(".git"))
            .map(|path| canonical_protocol_path_without_existing(&path)).transpose()?;
        let root = canonical_protocol_path_without_existing(root)?;
        let wanted = format!("branch refs/heads/{base}");
        let mut current: Option<String> = None;
        for field in listed.stdout.split(|byte| *byte == 0) {
            let field = String::from_utf8_lossy(field);
            if let Some(path) = field.strip_prefix("worktree ") {
                current = Some(path.to_owned());
            } else if field == wanted && current.as_deref() != Some(root.as_str()) && current.as_deref() != shadow.as_deref() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A merge left half-way (an abort that failed): nothing may be recorded
    /// or merged until it is cleared, or conflict marks would be committed.
    pub(super) fn merge_in_progress(&self, worktree: &Dir) -> Result<bool, ProjectFailure> {
        let found = self.git_allow_status_at(worktree, ["rev-parse", "-q", "--verify", "MERGE_HEAD"], &[0, 1])?;
        Ok(found.status.success())
    }

    /// Merge drivers named in the repository's own config would run their
    /// programs outside any sandbox during a merge.
    fn merge_drivers_configured(&self, source_root: &Dir) -> Result<bool, ProjectFailure> {
        let found = self.git_allow_status_at(source_root, ["config", "--get-regexp", r"^merge\..*\.driver$"], &[0, 1])?;
        Ok(found.status.code() == Some(0))
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
        keep_conflicts: bool,
    ) -> Result<Value, ProjectFailure> {
        // It changes the user's folder: the same gate as applying a copy.
        if !self.source_writes_enabled {
            return Err(ProjectFailure::recovery("changing the original is not available on this platform"));
        }
        let active = self.validate_workspace_address(storage, address)?;
        if active.kind == WorkspaceKind::Snapshot {
            return Err(ProjectFailure::manifest("a copy workspace is applied, not merged"));
        }
        let worktree = self.worktree_capability(&active)?;
        // The saved folder must still be the root of its own repository.
        let project = required_project(storage, &address.project_id)?;
        let source = self.open_project_source(&project)?;
        let source_root = source._root.try_clone().map_err(|error| {
            ProjectFailure::recovery(format!("could not retain the original folder: {error}"))
        })?;
        for lock in ["index.lock", "HEAD.lock", "packed-refs.lock"] {
            if source.git_dir.symlink_metadata(lock).is_ok() {
                return Err(ProjectFailure::dirty("another Git command is working in the original; nothing was merged"));
            }
        }
        let before = self.merge_state(&source_root, &worktree, &active)?;
        let unsupported = || -> Result<Value, ProjectFailure> {
            Ok(json!({"outcome":"blocked","reason":"unsupportedRepository","merge":before.value(self.uncommitted(&worktree)?)}))
        };
        // Both contexts: a worktree may carry its own `config.worktree`.
        if self.content_filters_configured(&source_root)? || self.merge_drivers_configured(&source_root)?
            || self.content_filters_configured(&worktree)? || self.merge_drivers_configured(&worktree)?
        {
            return unsupported();
        }
        if before.base_elsewhere {
            return Ok(json!({"outcome":"blocked","reason":"baseInOtherWorktree","merge":before.value(self.uncommitted(&worktree)?)}));
        }
        let (Some(base), Some(mut base_tip)) = (before.base_ref.clone(), before.base_tip.clone()) else {
            return Ok(json!({"outcome":"blocked","reason":"noBaseBranch","merge":before.value(self.uncommitted(&worktree)?)}));
        };
        if before.head != expected_head || base_tip != expected_base {
            return Ok(json!({"outcome":"blocked","reason":"changedSinceReview","merge":before.value(self.uncommitted(&worktree)?)}));
        }
        // A plain folder: its user cannot commit their own edits, so the app
        // records them first, in the folder's own history (app data). The
        // task then takes them in like any newer base commit; an overlap is
        // a conflict the team can resolve, not a refusal. A Git project's
        // edits stay the user's own (never committed for them).
        if self.is_plain_folder(&active.canonical_git_root) && self.on_branch(&source_root, &base)? {
            match self.record_folder_edits(&source_root)? {
                FolderEdits::Unchanged => {}
                FolderEdits::Recorded(head) => base_tip = head,
                FolderEdits::TooLarge => {
                    return Ok(json!({"outcome":"blocked","reason":"folderChangesTooLarge","merge":before.value(self.uncommitted(&worktree)?)}));
                }
                FolderEdits::HasRepository => {
                    return Ok(json!({"outcome":"blocked","reason":"folderHasRepository","merge":before.value(self.uncommitted(&worktree)?)}));
                }
            }
        }
        // Files asking for LFS since the task began would land as pointers.
        if self.content_filters_requested(&source_root, &source.git_dir, &base_tip)?
            || self.content_filters_requested(&source_root, &source.git_dir, &before.head)?
        {
            return unsupported();
        }
        // A merge the team was resolving: finish it only when no conflict
        // marks are left; the step commit then records it as a merge.
        if before.resolving {
            if before.unresolved > 0 {
                return Ok(json!({"outcome":"blocked","reason":"conflictsUnresolved","merge":before.value(self.uncommitted(&worktree)?)}));
            }
            if !self.workspace_commit_step(storage, address, &format!("{base}과 겹친 부분 정리"))? || self.merge_in_progress(&worktree)? {
                return Err(ProjectFailure::ambiguous("the resolved merge in the task workspace could not be recorded"));
            }
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
                // Handed to the team: the marks stay in the worktree and the
                // step commits finish the merge once they are gone. Only
                // text conflicts with marks can be handed over.
                let resolvable = self.team_resolvable(&worktree)?;
                if keep_conflicts && resolvable {
                    let after = self.merge_state(&source_root, &worktree, &active)?;
                    return Ok(json!({
                        "outcome":"resolving",
                        "conflictCount":paths.len(),
                        "conflicts":paths.into_iter().take(CONFLICT_PATHS_MAX).collect::<Vec<_>>(),
                        "merge":after.value(self.uncommitted(&worktree)?),
                    }));
                }
                // A failed abort leaves MERGE_HEAD; `merge_in_progress`
                // then refuses every later step commit and merge.
                if self.git_allow_status_at(&worktree, ["merge", "--abort"], &[0]).is_err() {
                    return Err(ProjectFailure::ambiguous("the conflicting merge in the task workspace could not be undone"));
                }
                self.verify_worktree_repository(&worktree, &active)?;
                let after = self.merge_state(&source_root, &worktree, &active)?;
                return Ok(json!({
                    "outcome":"conflict",
                    "teamResolvable":resolvable,
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
        let base_now = || -> Result<Option<String>, ProjectFailure> {
            let found = self.git_allow_status_at(&source_root, ["rev-parse", "--verify", "--quiet", &format!("{base_ref}^{{commit}}")], &[0, 1])?;
            found.status.success().then(|| trim_output(&found).map(str::to_owned)).transpose()
        };
        if on_base {
            // The base must still be what was reviewed (not reset or moved).
            if base_now()?.as_deref() != Some(base_tip.as_str()) {
                let after = self.merge_state(&source_root, &worktree, &active)?;
                return Ok(json!({"outcome":"blocked","reason":"changedSinceReview","merge":after.value(self.uncommitted(&worktree)?)}));
            }
            // The user's folder follows the branch. Git refuses (and changes
            // nothing) when this would overwrite the user's own edits.
            // Nor a file Git ignores: by default Git would replace it with
            // the task's tracked file of that name (review I6).
            let moved = self.git_allow_status_at(&source_root, ["merge", "--ff-only", "--no-overwrite-ignore", "--no-verify", "-q", &new_head], &[0, 1, 128])?;
            if moved.status.code() != Some(0) {
                let reason = if base_now()?.as_deref() == Some(base_tip.as_str()) { "userChangesOverlap" } else { "changedSinceReview" };
                let after = self.merge_state(&source_root, &worktree, &active)?;
                return Ok(json!({"outcome":"blocked","reason":reason,"merge":after.value(self.uncommitted(&worktree)?)}));
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

#[cfg(test)]
mod tests {
    use super::conflict_mark;

    #[test]
    fn conflict_marks_are_found_at_any_marker_size() {
        for line in [&b"<<<<<<< HEAD"[..], b">>>>>>> da9c487", b"<<<<<<<<<< HEAD\r", b"<<<<<<<", b">>>>>>>>>"] {
            assert!(conflict_mark(line), "{}", String::from_utf8_lossy(line));
        }
        for line in [&b"<<<<<< six"[..], b"== title ==", b"=======", b"==========", b"<<<<<<<x", b"text <<<<<<< HEAD"] {
            assert!(!conflict_mark(line), "{}", String::from_utf8_lossy(line));
        }
    }
}
