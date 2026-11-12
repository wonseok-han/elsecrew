//! Persisted evidence of a native picker selection made with lossless encoding.
//! This is not snapshot, process-termination, or application authority.
use super::{value_string, ProjectFailure};
use serde_json::{json, Map, Value};

pub(super) const FIELD: &str = "native_path_binding";

pub(super) fn value(root: &str) -> Value {
    json!({"schemaVersion":1,"canonicalPath":root})
}

pub(super) fn require(record: &Map<String, Value>) -> Result<(), ProjectFailure> {
    let root = value_string(record, "canonical_path")?;
    if record.get(FIELD) != Some(&value(root)) {
        return Err(ProjectFailure::manifest(
            "saved project needs a current native folder selection before filesystem access",
        ));
    }
    Ok(())
}
