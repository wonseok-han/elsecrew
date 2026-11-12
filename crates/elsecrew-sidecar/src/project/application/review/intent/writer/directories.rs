use super::*;
use std::collections::BTreeSet;

/// Check all immediate children of a newly-created directory. A foreign entry
/// is a conflict, including one hidden among the retained restoration files.
pub(super) fn verify(
    parent: &Dir,
    leaf: &str,
    record: &Value,
    scope: &Value,
    applied: bool,
    require_restored_children: bool,
) -> Result<(), ProjectFailure> {
    let j = journal(record)?;
    if j["stagedImage"]["kind"] != 2 {
        return Ok(());
    }
    if !record["pendingItem"]["item"]["before"].is_null() {
        return Err(invalid());
    }
    let name = if applied {
        leaf
    } else {
        j["retainedName"].as_str().ok_or_else(invalid)?
    };
    let directory = Dir::from_std_file(open(parent, name, true)?.into_std());
    let path = record["pendingItem"]["item"]["path"]
        .as_str()
        .ok_or_else(invalid)?;
    let mut expected = BTreeSet::new();
    for field in ["completedItems", "restoredItems"] {
        let Some(items) = scope.get(field) else {
            continue;
        };
        let items = items.as_array().ok_or_else(invalid)?;
        if items.len() > 100_000 {
            return Err(invalid());
        }
        for item in items {
            child(item, path, require_restored_children, &mut expected)?;
        }
    }
    if let Some(item) = scope.get("pendingItem") {
        child(item, path, require_restored_children, &mut expected)?;
    }
    for _ in 0..2 {
        let mut actual = BTreeSet::new();
        for entry in directory.entries().map_err(|_| invalid())? {
            let name = entry
                .map_err(|_| invalid())?
                .file_name()
                .into_string()
                .map_err(|_| invalid())?;
            if actual.len() >= 100_000 || !actual.insert(name) {
                return Err(invalid());
            }
        }
        if actual != expected {
            return Err(invalid());
        }
    }
    Ok(())
}

fn child(
    item: &Value,
    parent: &str,
    require_restored: bool,
    expected: &mut BTreeSet<String>,
) -> Result<(), ProjectFailure> {
    let path = item["item"]["path"].as_str().ok_or_else(invalid)?;
    let Some((directory, leaf)) = path.rsplit_once('/') else {
        return Ok(());
    };
    if directory != parent {
        return Ok(());
    }
    if !item["item"]["before"].is_null() {
        return Err(invalid());
    }
    let j = validate_journal(&item["writeJournal"])?;
    let name = match j["phase"].as_str() {
        Some("applied") if !require_restored => Some(leaf),
        Some("restored") if !j["stagedImage"].is_null() => {
            Some(j["retainedName"].as_str().ok_or_else(invalid)?)
        }
        Some("restored") => None,
        _ => return Err(invalid()),
    };
    if let Some(name) = name {
        if !expected.insert(name.to_owned()) {
            return Err(invalid());
        }
    }
    Ok(())
}
