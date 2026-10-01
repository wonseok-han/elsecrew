//! Resolve descendants of a restored newly-created directory. Only durable
//! journal names are substitutions; the caller still opens every parent no-follow.
use super::*;
use std::collections::BTreeMap;

pub(super) fn path(scope: &Value, original: &str) -> Result<String, ProjectFailure> {
    let parts: Vec<_> = original.split('/').collect();
    if original.len() > 4096
        || parts
            .iter()
            .any(|p| p.is_empty() || matches!(*p, "." | "..") || p.eq_ignore_ascii_case(".git"))
    {
        return Err(invalid());
    }
    let mut moved = BTreeMap::new();
    let restored = match scope.get("restoredItems") {
        Some(value) => value.as_array().ok_or_else(invalid)?.as_slice(),
        None => &[],
    };
    if restored.len() > 100_000 {
        return Err(invalid());
    }
    for pending in scope.get("pendingItem").into_iter().chain(restored.iter()) {
        if pending["item"]["after"]["kind"] != 2 || pending["writeJournal"]["phase"] != "restored" {
            continue;
        }
        if !pending["item"]["before"].is_null() {
            return Err(invalid());
        }
        let j = validate_journal(&pending["writeJournal"])?;
        // Canceled staging never moved the destination directory.
        if j["canceledStaging"] == true {
            continue;
        }
        let source = pending["item"]["path"].as_str().ok_or_else(invalid)?;
        let saved = j["retainedName"].as_str().ok_or_else(invalid)?;
        if moved.insert(source.to_owned(), saved.to_owned()).is_some() {
            return Err(invalid());
        }
    }
    let mut prefix = String::new();
    let mut result = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index != 0 {
            prefix.push('/');
            result.push('/');
        }
        prefix.push_str(part);
        // A directory's own target must remain its original leaf so the pair
        // checker can verify absence at the target and presence at retainedName.
        result.push_str(if index + 1 < parts.len() {
            moved.get(&prefix).map(String::as_str).unwrap_or(part)
        } else {
            part
        });
        if result.len() > 8192 {
            return Err(invalid());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn moved(path: &str, suffix: &str) -> Value {
        json!({"item":{"path":path,"before":null,"after":{"kind":2}},"writeJournal":{"version":1,"phase":"restored","retainedName":format!(".hello-agent-apply-{suffix}"),"rootIdentity":[1,2],"parentIdentity":[1,3],"stagedImage":null}})
    }
    #[test]
    fn nested_moves_resolve_by_original_ancestors_not_string_prefixes() {
        let a = "a".repeat(32);
        let b = "b".repeat(32);
        let scope = json!({"pendingItem":moved("new", &a),"restoredItems":[moved("new/sub", &b)]});
        assert_eq!(
            path(&scope, "new/sub/file").unwrap(),
            format!(".hello-agent-apply-{a}/.hello-agent-apply-{b}/file")
        );
        assert_eq!(
            path(&scope, "new/sub").unwrap(),
            format!(".hello-agent-apply-{a}/sub")
        );
        assert_eq!(path(&scope, "new").unwrap(), "new");
        assert_eq!(path(&scope, "newer/file").unwrap(), "newer/file");
        assert!(path(&scope, "new/../file").is_err());
        assert!(path(&scope, "new/.GIT/config").is_err());
    }
}
