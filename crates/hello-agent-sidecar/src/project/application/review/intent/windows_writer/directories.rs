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
