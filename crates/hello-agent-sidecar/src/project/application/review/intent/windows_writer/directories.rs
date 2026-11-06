//! Ownership map for a new directory's immediate children. Child content and
//! handle verification are separate; a name match alone grants no move authority.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// A new parent can contain only new children from this exact application scope,
/// including the private recovery files retained by each settled child journal.
/// Reject duplicate/overlapping names even if their receipts happen to match.
#[allow(dead_code)]
pub(super) fn children(
    scope: &Value,
    parent_path: &str,
    require_restored: bool,
) -> Result<BTreeMap<String, Value>, ProjectFailure> {
    validate_relative_path(parent_path, false)?;
    let mut result = BTreeMap::new();
    let mut count = 0usize;
    let mut paths = BTreeSet::new();
    for field in ["completedItems", "restoredItems"] {
        if let Some(items) = scope.get(field) {
            let items = items.as_array().ok_or_else(invalid)?;
            count = count.checked_add(items.len()).ok_or_else(invalid)?;
            if count > 100_000 {
                return Err(invalid());
            }
            for item in items {
                child(item, parent_path, require_restored, &mut result, &mut paths)?;
            }
        }
    }
    if let Some(item) = scope.get("pendingItem") {
        if count >= 100_000 {
            return Err(invalid());
        }
        child(item, parent_path, require_restored, &mut result, &mut paths)?;
    }
    Ok(result)
}
fn child(
    record: &Value,
    parent: &str,
    require_restored: bool,
    result: &mut BTreeMap<String, Value>,
    paths: &mut BTreeSet<String>,
) -> Result<(), ProjectFailure> {
    let item = &record["item"];
    let path = item["path"].as_str().ok_or_else(invalid)?;
    validate_relative_path(path, false)?;
    let Some((directory, leaf)) = path.rsplit_once('/') else {
        return Ok(());
    };
    if directory != parent {
        return Ok(());
    }
    if !paths.insert(path.to_owned()) || !item["before"].is_null() {
        return Err(invalid());
    }
    let journal = &record["writeJournal"];
    validate_receipts(journal, item)?;
    if require_restored && journal["phase"] != "restored" {
        return Err(invalid());
    }
    for (name, receipt) in recovery::settled_names(journal, leaf.to_owned())? {
        if !receipt.is_null() && result.insert(name, receipt).is_some() {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Directory timestamps/length can change as journaled children are installed.
/// Only use this comparison together with the complete child ownership scan.
fn same_directory(expected: &Value, actual: &Value) -> bool {
    if expected["observation"]["attributes"]
        .as_u64()
        .is_none_or(|v| v & 0x10 == 0)
    {
        return false;
    }
    let mut comparable = actual.clone();
    for field in ["changed", "written", "length"] {
        comparable["observation"][field] = expected["observation"][field].clone();
    }
    comparable == *expected
}
fn observed_directory(dir: &Dir) -> Result<Value, ProjectFailure> {
    Ok(
        json!({"observation":crate::project::windows_source_inventory::directory_observation(dir)?,"digest":vec![0u8;32]}),
    )
}

/// Build the parent index once, avoiding a full ledger scan for every nested
/// directory. Existing scope/archive validation remains the caller's obligation.
type Ownership = BTreeMap<String, BTreeMap<String, Value>>;
type LogicalNames = BTreeMap<(String, String), String>;
fn index(
    scope: &Value,
    restored: bool,
    subtree: &str,
) -> Result<(Ownership, LogicalNames), ProjectFailure> {
    let mut result = BTreeMap::new();
    let mut logical = BTreeMap::new();
    let mut paths = BTreeSet::new();
    let mut count = 0usize;
    let mut add = |record: &Value| -> Result<(), ProjectFailure> {
        count += 1;
        if count > 100_000 {
            return Err(invalid());
        }
        let path = record["item"]["path"].as_str().ok_or_else(invalid)?;
        validate_relative_path(path, false)?;
        if let Some((parent, leaf)) = path.rsplit_once('/') {
            if parent != subtree
                && !parent
                    .strip_prefix(subtree)
                    .is_some_and(|tail| tail.starts_with('/'))
            {
                return Ok(());
            }
            child(
                record,
                parent,
                restored,
                result.entry(parent.to_owned()).or_default(),
                &mut paths,
            )?;
            for (name, receipt) in
                recovery::settled_names(&record["writeJournal"], leaf.to_owned())?
            {
                if !receipt.is_null() {
                    logical.insert((parent.to_owned(), name), path.to_owned());
                }
            }
        }
        Ok(())
    };
    for field in ["completedItems", "restoredItems"] {
        if let Some(records) = scope.get(field) {
            for record in records.as_array().ok_or_else(invalid)? {
                add(record)?;
            }
        }
    }
    if let Some(record) = scope.get("pendingItem") {
        add(record)?;
    }
    Ok((result, logical))
}

/// Read-only tree comparison. No deletion, rename, completion publication, or
/// restoration grant follows merely from this observation. Caller rebinds the
/// original root/parent and ledger and repeats checks at its mutation boundary.
#[allow(dead_code)]
pub(super) fn verify_tree(
    root: &Dir,
    expected: &Value,
    scope: &Value,
    logical_path: &str,
    restored: bool,
    check: &dyn Fn() -> Result<(), ProjectFailure>,
) -> Result<(), ProjectFailure> {
    validate_relative_path(logical_path, false)?;
    let (ownership, logical_names) = index(scope, restored, logical_path)?;
    let mut stack = vec![(
        root.try_clone().map_err(|_| invalid())?,
        expected.clone(),
        logical_path.to_owned(),
        0usize,
    )];
    let mut visited = 0usize;
    while let Some((dir, expected, path, depth)) = stack.pop() {
        check()?;
        if depth > 128 {
            return Err(invalid());
        }
        let before = observed_directory(&dir)?;
        if !same_directory(&expected, &before) {
            return Err(invalid());
        }
        let empty = BTreeMap::new();
        let children = ownership.get(&path).unwrap_or(&empty);
        for pass in 0..2 {
            let mut names = BTreeSet::new();
            for entry in dir.entries().map_err(|_| invalid())? {
                check()?;
                let name = entry
                    .map_err(|_| invalid())?
                    .file_name()
                    .into_string()
                    .map_err(|_| invalid())?;
                if names.len() >= 500_000 || !names.insert(name) {
                    return Err(invalid());
                }
            }
            if names.len() != children.len() || names.iter().any(|n| !children.contains_key(n)) {
                return Err(invalid());
            }
            for (name, receipt) in children {
                check()?;
                if receipt["observation"]["attributes"]
                    .as_u64()
                    .ok_or_else(invalid)?
                    & 0x10
                    != 0
                {
                    // Recover the original logical child name even when its
                    // physical name is the journal's detached recovery name.
                    let logical = logical_names
                        .get(&(path.clone(), name.clone()))
                        .ok_or_else(invalid)?
                        .clone();
                    let child_dir = open_real_child_dir(&dir, name)?;
                    if !same_directory(receipt, &observed_directory(&child_dir)?) {
                        return Err(invalid());
                    }
                    if pass == 0 {
                        visited += 1;
                        if visited > 100_000 {
                            return Err(invalid());
                        }
                        stack.push((child_dir, receipt.clone(), logical, depth + 1));
                    }
                } else {
                    let mut held = pin_entry(&dir, name, receipt, check)?;
                    verify_pin(&dir, name, receipt, &mut held, check)?;
                }
            }
        }
        if observed_directory(&dir)? != before {
            return Err(invalid());
        }
    }
    check()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reserved_child(path: &str, nonce: &str) -> Value {
        // A canceled empty reservation intentionally owns no filesystem names.
        json!({"item":{"path":path,"before":null,"after":{"kind":1,"size":0,"digest":vec![0;32]}},
            "writeJournal":{"version":1,"platform":"windows","phase":"restored","nonce":nonce,
            "rootIdentity":{"volume":1,"file":vec![1;16]},"parentIdentity":{"volume":1,"file":vec![2;16]},"guardEntry":null,"stagedEntry":null,
            "retainedEntry":null,"installedEntry":null,"detachedEntry":null,"restoredEntry":null,
            "canceledReservation":true}})
    }
    #[test]
    fn tree_observation_checks_owned_content_and_rejects_foreign_names() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        let root_entry = observed_directory(&dir).unwrap();
        verify_tree(&dir, &root_entry, &json!({}), "new", true, &|| Ok(())).unwrap();
        let mut record = reserved_child("new/file.txt", "11111111111111111111111111111111");
        let staged_name = name(&record["writeJournal"], "staged").unwrap();
        let mut staged = dir
            .open_with(&staged_name, &windows_replace::source_options())
            .unwrap();
        staged.write_all(b"partial").unwrap();
        staged.sync_all().unwrap();
        record["item"]["after"]["size"] = json!(7);
        record["writeJournal"]["stagedEntry"] = entry(&mut staged, &|| Ok(())).unwrap();
        drop(staged);
        let scope = json!({"pendingItem":record});
        verify_tree(&dir, &root_entry, &scope, "new", true, &|| Ok(())).unwrap();
        dir.write("foreign.txt", b"keep").unwrap();
        assert!(verify_tree(&dir, &root_entry, &scope, "new", true, &|| Ok(())).is_err());
        assert_eq!(dir.read("foreign.txt").unwrap(), b"keep");
        dir.remove_file("foreign.txt").unwrap();
        dir.write(&staged_name, b"changed").unwrap();
        assert!(verify_tree(&dir, &root_entry, &scope, "new", true, &|| Ok(())).is_err());
        assert_eq!(dir.read(&staged_name).unwrap(), b"changed");
    }
    #[test]
    fn child_map_rejects_unsettled_or_preexisting_children() {
        let child = reserved_child("new/file.txt", "11111111111111111111111111111111");
        let mut scope = json!({"pendingItem":child});
        assert!(children(&scope, "new", true).unwrap().is_empty());
        scope["pendingItem"]["item"]["before"] = json!({"foreign":true});
        assert!(children(&scope, "new", false).is_err());
        let duplicate = json!({"completedItems":[child.clone()],"pendingItem":child.clone()});
        assert!(children(&duplicate, "new", true).is_err());
        scope["pendingItem"] = child;
        scope["pendingItem"]["writeJournal"]["phase"] = json!("reserved");
        assert!(children(&scope, "new", false).is_err());
    }
}
