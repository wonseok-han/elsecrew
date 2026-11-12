//! Read-only retention graph. A candidate is not permission to unlink an archive.
use super::*;
use serde_json::{json, Map};
use std::collections::BTreeSet;

/// Current application records retain their backup even after completion: the
/// current applied record is still the public restoration target. Historical
/// references remain evidence and must be handled by a future deletion journal.
pub(in crate::project) fn summarize(
    project: &Map<String, Value>,
) -> Result<Vec<Value>, ProjectFailure> {
    let mut current = BTreeSet::new();
    let mut history = BTreeSet::new();
    fn reference(record: &Value) -> Result<(String, String), ProjectFailure> {
        let review = record["backup"]["header"]["reviewId"]
            .as_str()
            .ok_or_else(invalid)?;
        let name = record["backup"]["name"]
            .as_str()
            .filter(|name| valid_name(name))
            .ok_or_else(invalid)?;
        if !matches!(
            record["state"].as_str(),
            Some("prepared" | "applying" | "applied" | "restored" | "abandoned")
        ) {
            return Err(invalid());
        }
        Ok((review.to_owned(), name.to_owned()))
    }
    if let Some(record) = project.get("application_intent") {
        current.insert(reference(record)?);
    }
    if let Some(records) = project.get("application_history") {
        for record in records.as_object().ok_or_else(invalid)?.values() {
            history.insert(reference(record)?);
        }
    }
    let Some(records) = project.get("application_backups") else {
        return if current.is_empty() && history.is_empty() {
            Ok(Vec::new())
        } else {
            Err(invalid())
        };
    };
    let records = records.as_object().ok_or_else(invalid)?;
    if records.len() > 1000 {
        return Err(invalid());
    }
    let mut names = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (review, record) in records {
        let name = record["name"]
            .as_str()
            .filter(|name| valid_name(name))
            .ok_or_else(invalid)?;
        if record["version"] != 1
            || record["header"]["reviewId"] != *review
            || !names.insert(name.to_owned())
        {
            return Err(invalid());
        }
        let key = (review.to_owned(), name.to_owned());
        let state = record["state"].as_str().ok_or_else(invalid)?;
        let reason = if current.contains(&key) {
            "currentApplication"
        } else if history.contains(&key) {
            "applicationHistory"
        } else {
            match state {
                "preparing" => "unfinishedPreparation",
                "ready" => "preparedBackup",
                "abandoned" => "abandonedPreparation",
                _ => return Err(invalid()),
            }
        };
        if !matches!(state, "preparing" | "ready" | "abandoned") {
            return Err(invalid());
        }
        let cleanup_state = super::cleanup::recorded_state(record)?;
        if cleanup_state != "none" && (current.contains(&key) || state == "preparing") {
            return Err(invalid());
        }
        seen.insert(key);
        result.push(json!({"reviewId":review,"state":state,"retainedFor":reason,"cleanupState":cleanup_state}));
    }
    if !current.is_subset(&seen) || !history.is_subset(&seen) {
        return Err(invalid());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_status_is_separate_from_historical_retention() {
        let backup = json!({"version":1,"name":format!("backup_{}.bin", "a".repeat(32)),"state":"ready","header":{"reviewId":"review_a"}});
        let mut project = json!({"application_backups":{"review_a":backup},"application_history":{"application_a":{"state":"restored","backup":backup}}}).as_object().unwrap().clone();
        assert_eq!(summarize(&project).unwrap()[0]["cleanupState"], "none");
        for state in ["pending", "deleted"] {
            project.get_mut("application_backups").unwrap()["review_a"]["cleanup"] =
                json!({"version":1,"state":state,"identity":null});
            let summary = summarize(&project).unwrap();
            assert_eq!(summary[0]["cleanupState"], state);
            assert_eq!(summary[0]["retainedFor"], "applicationHistory");
        }
        project.insert(
            "application_intent".into(),
            json!({"state":"restored","backup":backup}),
        );
        assert!(summarize(&project).is_err());
        project.remove("application_intent");
        project.get_mut("application_backups").unwrap()["review_a"]["cleanup"] =
            json!({"version":1,"state":"deleted","unexpected":null});
        assert!(summarize(&project).is_err());
    }
    #[test]
    fn current_and_historical_references_prevent_unreferenced_classification() {
        let name = format!("backup_{}.bin", "a".repeat(32));
        let backup =
            json!({"version":1,"name":name,"state":"abandoned","header":{"reviewId":"review_a"}});
        let mut project = json!({"application_backups":{"review_a":backup}})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            summarize(&project).unwrap()[0]["retainedFor"],
            "abandonedPreparation"
        );
        let record = json!({"state":"restored","backup":backup});
        project.insert(
            "application_history".into(),
            json!({"application_a":record}),
        );
        assert_eq!(
            summarize(&project).unwrap()[0]["retainedFor"],
            "applicationHistory"
        );
        project.insert("application_intent".into(), record);
        assert_eq!(
            summarize(&project).unwrap()[0]["retainedFor"],
            "currentApplication"
        );
        project.get_mut("application_backups").unwrap()["review_a"]["name"] =
            json!(format!("backup_{}.bin", "b".repeat(32)));
        assert!(summarize(&project).is_err());
    }
}
