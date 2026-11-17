//! Applying a task's workspace to the user's own folder, latest wins
//! (user decision 2026-11-17): every file the workspace changed since its
//! base commit is written over the original as it is now, and every file it
//! removed is removed. No conflict check, no confirmation token and no backup:
//! Git in the user's folder is the way back. What stays is path safety:
//! - the folder is reopened as the same Git repository at the same real path;
//! - every write goes through that directory handle, never through a link in
//!   the path (a link inside the folder could lead into `.git`);
//! - Git metadata and agent bookkeeping are never touched;
//! - everything is checked before the first write, and a write that still
//!   fails reports exactly what was already applied.

use super::*;

/// Paths an apply never writes: Git's own data and agent CLI bookkeeping.
fn skipped_path(path: &str) -> bool {
    path.split('/').any(|part| part.eq_ignore_ascii_case(".git"))
        || path == ".claude/.cc-writes"
        || path.starts_with(".claude/.cc-writes/")
}

fn parent(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

enum Step {
    Remove(String),
    Write { path: String, executable: bool },
}

impl Step {
    fn path(&self) -> &str {
        match self {
            Self::Remove(path) | Self::Write { path, .. } => path,
        }
    }
}

impl ProjectManager {
    pub(crate) fn apply_chat_latest(
        &self,
        storage: &Storage,
        project_id: &str,
        chat_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        if !self.source_writes_enabled || !cfg!(target_os = "macos") {
            return Err(ProjectFailure::recovery("source application is not enabled"));
        }
        if !storage.blocker_ids()?.is_empty() {
            return Err(ProjectFailure::recovery(
                "wait for running work or an unresolved recovery to finish before applying",
            ));
        }
        let chat = storage
            .projection_record("chat", chat_id)?
            .ok_or_else(|| ProjectFailure::dirty("apply chat missing"))?;
        let record = chat
            .get("record")
            .and_then(Value::as_object)
            .ok_or_else(|| ProjectFailure::dirty("apply chat record missing"))?;
        if record.get("projectId").and_then(Value::as_str) != Some(project_id) {
            return Err(ProjectFailure::dirty("apply chat belongs to another project"));
        }
        let address = WorkspaceAddress {
            project_id: project_id.to_owned(),
            chat_id: chat_id.to_owned(),
            worktree_id: value_string(record, "worktreeId")?.to_owned(),
            workspace_identity: value_string(record, "workspaceIdentity")?.to_owned(),
        };
        let active = self.validate_workspace_address(storage, &address)?;
        let worktree = self.worktree_capability(&active)?;
        // The same Git repository at the same real path: a moved or replaced
        // folder (or a link put in its place) is never written.
        let source = self.open_source_repository(&active.canonical_git_root)?;
        if source.canonical_root != active.canonical_git_root {
            return Err(ProjectFailure::dirty("the project folder moved; open it again before applying"));
        }
        let root = &source._root;
        let entries = self.diff_entries(&active)?;

        // Check everything first; anything unsafe or unusual is skipped.
        // Files this apply removes free their names, so a file replaced by a
        // folder of the same name is written after the removal.
        let removing: std::collections::BTreeSet<&str> = entries
            .iter()
            .filter(|entry| !entry.current_exists)
            .map(|entry| entry.path.as_str())
            .chain(entries.iter().filter_map(|entry| entry.old_path.as_deref()))
            .collect();
        let mut steps = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for entry in &entries {
            if skipped_path(&entry.path) || entry.old_path.as_deref().is_some_and(skipped_path) {
                continue;
            }
            if let Some(old) = entry.old_path.as_deref() {
                if safe_parents(root, old, &removing)? {
                    steps.push(Step::Remove(old.to_owned()));
                } else {
                    skipped.push(old.to_owned());
                }
            }
            if !safe_parents(root, &entry.path, &removing)? {
                skipped.push(entry.path.clone());
                continue;
            }
            if !entry.current_exists {
                steps.push(Step::Remove(entry.path.clone()));
                continue;
            }
            let source = worktree.symlink_metadata(&entry.path).ok().filter(|meta| meta.is_file());
            let usable = source.is_some()
                && root.symlink_metadata(&entry.path).map_or(true, |meta| !meta.is_dir());
            if let (true, Some(meta)) = (usable, source) {
                let executable = meta.permissions().mode() & 0o111 != 0;
                steps.push(Step::Write { path: entry.path.clone(), executable });
            } else {
                // Links, special files and names taken by a folder stay as they are.
                skipped.push(entry.path.clone());
            }
        }

        let (mut written, mut removed) = (Vec::<String>::new(), Vec::<String>::new());
        let mut failed: Option<String> = None;
        // Removals first, so freed names are available to the writes.
        steps.sort_by_key(|step| matches!(step, Step::Write { .. }));
        for step in &steps {
            let outcome = match step {
                Step::Remove(path) => remove_original_file(root, path).map(|done| {
                    if done {
                        removed.push(path.clone());
                    }
                }),
                Step::Write { path, executable } => {
                    copy_into_original(&worktree, root, path, *executable).map(|()| written.push(path.clone()))
                }
            };
            if outcome.is_err() {
                failed = Some(step.path().to_owned());
                break;
            }
        }
        Ok(ProjectAction {
            result: json!({
                "schemaVersion":1,"action":"applyChatLatest","projectId":project_id,"chatId":chat_id,
                "written":written.len(),"removed":removed.len(),"skipped":skipped,"failed":failed,
            }),
            ..ProjectAction::default()
        })
    }
}

/// True when no folder on the way to `path` is a link or a non-folder in the
/// original. Missing folders are fine (they will be created as real folders),
/// and so is a file this apply removes first (its name becomes a folder).
fn safe_parents(
    root: &Dir,
    path: &str,
    removing: &std::collections::BTreeSet<&str>,
) -> Result<bool, ProjectFailure> {
    let Some(parent) = parent(path) else { return Ok(true) };
    let mut prefix = String::new();
    for part in parent.split('/') {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        match root.symlink_metadata(&prefix) {
            Ok(meta) if meta.is_dir() => {}
            Ok(meta) if !meta.file_type().is_symlink() && removing.contains(prefix.as_str()) => return Ok(true),
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(_) => return Err(ProjectFailure::dirty("original folder cannot be inspected")),
        }
    }
    Ok(true)
}

/// Removes a file or link at `path` in the original; a folder is left alone.
fn remove_original_file(root: &Dir, path: &str) -> Result<bool, ProjectFailure> {
    match root.symlink_metadata(path) {
        Ok(meta) if !meta.is_dir() => root
            .remove_file(path)
            .map(|()| true)
            .map_err(|_| ProjectFailure::dirty("original file cannot be removed")),
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ProjectFailure::dirty("original file cannot be inspected")),
    }
}

/// Streams the workspace file into a fixed-length temporary name beside the
/// target, then renames it into place, so a reader never sees half a file
/// and a long file name never makes the temporary name too long.
fn copy_into_original(
    worktree: &Dir,
    root: &Dir,
    path: &str,
    executable: bool,
) -> Result<(), ProjectFailure> {
    if let Some(parent) = parent(path) {
        root.create_dir_all(parent)
            .map_err(|_| ProjectFailure::dirty("original folder cannot be created"))?;
    }
    let temp = match parent(path) {
        Some(parent) => format!("{parent}/.elsecrew-{}.tmp", Uuid::new_v4().simple()),
        None => format!(".elsecrew-{}.tmp", Uuid::new_v4().simple()),
    };
    let copied = (|| -> std::io::Result<()> {
        let mut from = worktree.open(path)?;
        let mut to = root.create(&temp)?;
        std::io::copy(&mut from, &mut to)?;
        to.sync_all()?;
        root.set_permissions(
            &temp,
            cap_std::fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
        )?;
        root.rename(&temp, root, path)
    })();
    if copied.is_err() {
        let _ = root.remove_file(&temp);
        return Err(ProjectFailure::dirty("original file cannot be written"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn writes_replace_files_create_folders_and_never_follow_links_or_escape() {
        let temp = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("old.txt"), b"old").unwrap();
        std::fs::create_dir(temp.path().join("folder")).unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
        // A link inside the project that leads into Git's own data.
        std::os::unix::fs::symlink(".git", temp.path().join("cfg")).unwrap();
        std::fs::write(work.path().join("old.txt"), b"new").unwrap();
        std::fs::create_dir_all(work.path().join("a/b")).unwrap();
        std::fs::write(work.path().join("a/b/run.sh"), b"#!/bin/sh\n").unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let worktree = Dir::open_ambient_dir(work.path(), ambient_authority()).unwrap();
        copy_into_original(&worktree, &root, "old.txt", false).unwrap();
        assert_eq!(std::fs::read(temp.path().join("old.txt")).unwrap(), b"new");
        copy_into_original(&worktree, &root, "a/b/run.sh", true).unwrap();
        assert_eq!(std::fs::metadata(temp.path().join("a/b/run.sh")).unwrap().permissions().mode() & 0o777, 0o755);
        // Links on the way are refused before any write.
        let none = std::collections::BTreeSet::new();
        assert!(!safe_parents(&root, "cfg/hooks/post-checkout", &none).unwrap());
        assert!(!safe_parents(&root, "link/escaped.txt", &none).unwrap());
        assert!(!safe_parents(&root, "old.txt/inner", &none).unwrap());
        assert!(safe_parents(&root, "a/b/run.sh", &none).unwrap());
        assert!(safe_parents(&root, "new/deep/file", &none).unwrap());
        // A file this apply removes may become a folder; a link never does.
        assert!(safe_parents(&root, "old.txt/inner", &std::collections::BTreeSet::from(["old.txt"])).unwrap());
        assert!(!safe_parents(&root, "cfg/hooks/x", &std::collections::BTreeSet::from(["cfg"])).unwrap());
        assert!(!temp.path().join(".git/hooks").exists());
        assert!(!outside.path().join("escaped.txt").exists());
        assert!(remove_original_file(&root, "old.txt").unwrap());
        assert!(!remove_original_file(&root, "old.txt").unwrap());
        assert!(!remove_original_file(&root, "folder").unwrap());
        // A very long name still fits: the temporary name does not include it.
        let long = "n".repeat(250);
        std::fs::write(work.path().join(&long), b"x").unwrap();
        copy_into_original(&worktree, &root, &long, false).unwrap();
        // No temporary file is left behind.
        let names: Vec<_> = std::fs::read_dir(temp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(names.iter().all(|name| !name.to_string_lossy().starts_with(".elsecrew-")), "{names:?}");
        assert!(skipped_path(".git/config") && skipped_path("sub/.GIT/x") && skipped_path(".claude/.cc-writes/a"));
        assert!(!skipped_path(".claude/settings.json") && !skipped_path("src/app.js"));
    }
}
