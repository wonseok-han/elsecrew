//! Read-only project review candidate. Not an approval ticket or write capability.
use super::{
    capture,
    git_baseline::raw_baseline::{capture_clean_baseline, receipt},
    Entry, SourceInventory,
};
use crate::project::{snapshot_identity, ProjectFailure, ProjectManager};
use crate::storage::Storage;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use unicode_normalization::UnicodeNormalization;

const MAX_REVIEW_BYTES: usize = 32 * 1024 * 1024;
pub(in crate::project) mod persistence;

fn validate_paths<'a>(paths: impl Iterator<Item = &'a str>) -> Result<(), ProjectFailure> {
    let mut names = BTreeMap::new();
    let mut budget = 2 * super::MAX_PATH_BYTES;
    for path in paths {
        let mut key = String::new();
        // Conservative compatibility normalization may reject more than the host FS.
        // It is collision prevention, not a claim to emulate every filesystem collation.
        for ch in path
            .nfkc()
            .flat_map(char::to_uppercase)
            .flat_map(char::to_lowercase)
            .nfd()
        {
            if key.len() + ch.len_utf8() > 4096 || ch.len_utf8() > budget {
                return Err(ProjectFailure::dirty(
                    "normalized review paths exceed budget",
                ));
            }
            budget -= ch.len_utf8();
            key.push(ch);
        }
        if names
            .insert(key, path)
            .is_some_and(|previous| previous != path)
        {
            return Err(ProjectFailure::dirty(
                "source and snapshot paths have a case or normalization collision",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Image {
    kind: u8,
    executable: bool,
    size: u64,
    digest: [u8; 32],
}

impl From<&Entry> for Image {
    fn from(entry: &Entry) -> Self {
        Self {
            kind: entry.kind,
            executable: entry.kind == 1 && entry.mode & 0o100 != 0,
            size: entry.size,
            digest: entry.digest,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Change {
    before: Option<Image>,
    after: Option<Image>,
}

pub(in crate::project) struct ReviewPlan {
    binding: Value,
    changes: BTreeMap<String, Change>,
    digest: [u8; 32],
    snapshot: SourceInventory,
}

// Serialize to a bounded hash sink instead of allocating the full review JSON.
struct ReviewHash {
    hash: Sha256,
    remaining: usize,
}
impl Write for ReviewHash {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("review exceeds byte budget"))?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[allow(dead_code)]
pub(in crate::project) fn prepare(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
) -> Result<ReviewPlan, ProjectFailure> {
    if !storage.blocker_ids()?.is_empty() {
        return Err(ProjectFailure::recovery(
            "review requires all writers and recovery blockers to finish",
        ));
    }
    let active = manager.validate_active_project(storage, project_id)?;
    let source = manager.open_source_repository(&active.canonical_git_root)?;
    let baseline = capture_clean_baseline(manager, &source)?;
    receipt::verify_bound(manager, storage, &active, &baseline)?;
    let workspace = manager.worktree_capability(&active)?;
    let snapshot = capture(&workspace)?;
    // Snapshot files are materialized with Git's 0644/0755 modes. Other chmod
    // changes have no supported application semantics yet; never omit them.
    for entry in snapshot.entries.values() {
        if (entry.kind == 1 && !matches!(entry.mode & 0o7777, 0o644 | 0o755))
            || (entry.kind == 2 && entry.mode & 0o7777 != 0o755)
        {
            return Err(ProjectFailure::dirty(
                "snapshot permission changes outside Git executable mode are unsupported",
            ));
        }
    }
    let tracked: BTreeSet<&str> = baseline.tracked_paths().collect();
    let original = &baseline.inventory().entries;
    validate_paths(
        original
            .keys()
            .chain(snapshot.entries.keys())
            .map(String::as_str),
    )?;
    let mut changes = BTreeMap::new();
    for path in &tracked {
        let before = original
            .get(*path)
            .ok_or_else(|| ProjectFailure::dirty("tracked baseline file missing"))?;
        let after = snapshot.entries.get(*path);
        if after.is_some_and(|entry| entry.kind != before.kind) {
            return Err(ProjectFailure::dirty(
                "file kind transitions require a separately supported apply policy",
            ));
        }
        let before = Some(Image::from(before));
        let after = after.map(Image::from);
        if before != after {
            changes.insert((*path).to_owned(), Change { before, after });
        }
    }
    for (path, after) in &snapshot.entries {
        if path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
        {
            return Err(ProjectFailure::dirty(
                "nested Git metadata cannot be part of an application review",
            ));
        }
        if tracked.contains(path.as_str()) {
            continue;
        }
        if let Some(before) = original.get(path) {
            if before.kind == 2 && after.kind == 2 {
                continue;
            }
            return Err(ProjectFailure::dirty(
                "snapshot path collides with an untracked or ignored source entry",
            ));
        }
        changes.insert(
            path.clone(),
            Change {
                before: None,
                after: Some(Image::from(after)),
            },
        );
    }
    // Source entries not tracked by Git are never inferred to be deletions.
    // Existing directories remain; only new directories appear as additions.
    if baseline != capture_clean_baseline(manager, &source)? || snapshot != capture(&workspace)? {
        return Err(ProjectFailure::dirty(
            "source or snapshot changed during review",
        ));
    }
    manager.validate_active_project(storage, project_id)?;
    let binding = snapshot_identity(&active)?;
    let mut sink = ReviewHash {
        hash: Sha256::new(),
        remaining: MAX_REVIEW_BYTES,
    };
    serde_json::to_writer(&mut sink, &(1u32, &binding))
        .map_err(|_| ProjectFailure::dirty("review exceeds its serialization budget"))?;
    for (path, change) in &changes {
        let image = |entry: &Option<Image>| {
            entry.as_ref().map(|entry| serde_json::json!({
            "kind":entry.kind, "executable":entry.executable, "size":entry.size, "digest":entry.digest,
        }))
        };
        serde_json::to_writer(
            &mut sink,
            &(path, image(&change.before), image(&change.after)),
        )
        .map_err(|_| ProjectFailure::dirty("review exceeds its serialization budget"))?;
    }
    Ok(ReviewPlan {
        binding,
        changes,
        digest: sink.hash.finalize().into(),
        snapshot,
    })
}

#[allow(dead_code)]
pub(in crate::project) fn revalidate(
    manager: &ProjectManager,
    storage: &Storage,
    project_id: &str,
    expected: &ReviewPlan,
) -> Result<(), ProjectFailure> {
    let current = prepare(manager, storage, project_id)?;
    if expected.binding != current.binding
        || expected.digest != current.digest
        || expected.changes != current.changes
        || expected.snapshot != current.snapshot
    {
        return Err(ProjectFailure::dirty(
            "review is stale; confirm a new review before applying",
        ));
    }
    Ok(())
}

#[cfg(test)]
impl ReviewPlan {
    pub(in crate::project) fn changed_paths(&self) -> Vec<&str> {
        self.changes.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_collisions_cover_case_unicode_and_cross_image_names() {
        for pair in [
            ["notes", "Notes"],
            ["src/edit.txt", "src/Edit.txt"],
            ["café", "cafe\u{301}"],
            ["Σ", "ς"],
        ] {
            assert!(validate_paths(pair.into_iter()).is_err());
        }
        assert!(validate_paths(["same", "same", "other"].into_iter()).is_ok());
    }
    #[test]
    fn hash_sink_refuses_over_budget_without_hashing_partial_bytes() {
        let mut sink = ReviewHash {
            hash: Sha256::new(),
            remaining: 3,
        };
        sink.write_all(b"abc").unwrap();
        assert!(sink.write_all(b"d").is_err());
        assert_eq!(sink.hash.finalize()[..], Sha256::digest(b"abc")[..]);
    }
}
