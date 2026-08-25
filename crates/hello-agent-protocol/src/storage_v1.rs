use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display};

pub const SQLITE_SCHEMA_VERSION: u32 = 1;
pub const SQLITE_TABLES_V1: &[&str] = &[
    "schema_meta",
    "app_instances",
    "events",
    "projects",
    "chats",
    "runs",
    "locks",
];
pub const LEDGER_FIELDS_V1: &[&str] = &[
    "event_id",
    "operation_id",
    "request_id",
    "aggregate_type",
    "aggregate_id",
    "aggregate_sequence",
    "event_type",
    "payload_schema_version",
    "canonical_json_payload",
    "content_hash",
    "timestamp",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageContractError {
    detail: String,
}

impl StorageContractError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for StorageContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for StorageContractError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationDecisionV1 {
    Append,
    DuplicateNoOp,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRecordV1 {
    pub operation_id: String,
    pub request_id: Option<String>,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerEventV1 {
    pub event_id: String,
    pub operation_id: String,
    pub request_id: Option<String>,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_sequence: u64,
    pub event_type: String,
    pub payload_schema_version: u32,
    pub canonical_json_payload: String,
    pub content_hash: String,
    pub timestamp: String,
}

impl From<&LedgerEventV1> for OperationRecordV1 {
    fn from(value: &LedgerEventV1) -> Self {
        Self {
            operation_id: value.operation_id.clone(),
            request_id: value.request_id.clone(),
            content_hash: value.content_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionStateV1 {
    pub app_instances: BTreeMap<String, Map<String, Value>>,
    pub projects: BTreeMap<String, Map<String, Value>>,
    pub chats: BTreeMap<String, Map<String, Value>>,
    pub runs: BTreeMap<String, Map<String, Value>>,
    pub locks: BTreeMap<String, Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryInputV1 {
    pub previous_app_instance: Map<String, Value>,
    pub runs: Vec<Map<String, Value>>,
    pub locks: Vec<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryResultV1 {
    pub recovery_required: bool,
    pub blocker_ids: Vec<String>,
    pub runs: Vec<Map<String, Value>>,
    pub locks: Vec<Map<String, Value>>,
}

pub fn classify_operation_v1(
    existing: &[OperationRecordV1],
    incoming: &OperationRecordV1,
) -> Result<OperationDecisionV1, StorageContractError> {
    require_id(&incoming.operation_id, "operation_id")?;
    require_id(&incoming.content_hash, "content_hash")?;
    let Some(previous) = existing
        .iter()
        .find(|entry| entry.operation_id == incoming.operation_id)
    else {
        return Ok(OperationDecisionV1::Append);
    };
    Ok(if previous.content_hash == incoming.content_hash {
        OperationDecisionV1::DuplicateNoOp
    } else {
        OperationDecisionV1::Conflict
    })
}

pub fn replay_ledger_v1(
    events: &[LedgerEventV1],
) -> Result<ProjectionStateV1, StorageContractError> {
    let mut projection = ProjectionStateV1::default();
    let mut operations = Vec::<OperationRecordV1>::new();
    let mut event_ids = HashSet::new();
    let mut aggregate_sequences = HashMap::<String, u64>::new();

    for event in events {
        validate_ledger_event(event)?;
        match classify_operation_v1(&operations, &OperationRecordV1::from(event))? {
            OperationDecisionV1::DuplicateNoOp => continue,
            OperationDecisionV1::Conflict => {
                return Err(StorageContractError::new(format!(
                    "operation_id reused with different content: {}",
                    event.operation_id
                )))
            }
            OperationDecisionV1::Append => operations.push(OperationRecordV1::from(event)),
        }
        if !event_ids.insert(event.event_id.clone()) {
            return Err(StorageContractError::new(format!(
                "duplicate event_id: {}",
                event.event_id
            )));
        }

        let aggregate_key = format!("{}:{}", event.aggregate_type, event.aggregate_id);
        let expected = aggregate_sequences
            .get(&aggregate_key)
            .copied()
            .unwrap_or(0)
            + 1;
        if event.aggregate_sequence != expected {
            return Err(StorageContractError::new(format!(
                "{aggregate_key} expected aggregate_sequence {expected}"
            )));
        }
        aggregate_sequences.insert(aggregate_key, event.aggregate_sequence);

        let mut payload = parse_canonical_object(&event.canonical_json_payload)?;
        let id_field = aggregate_id_field(&event.aggregate_type)?;
        if payload.get(id_field).and_then(Value::as_str) != Some(event.aggregate_id.as_str()) {
            return Err(StorageContractError::new(format!(
                "{id_field} must match aggregate_id"
            )));
        }
        payload.insert(
            "last_event_sequence".to_string(),
            Value::from(event.aggregate_sequence),
        );
        projection_table_mut(&mut projection, &event.aggregate_type)?
            .insert(event.aggregate_id.clone(), payload);
    }
    Ok(projection)
}

pub fn recover_unclean_shutdown_v1(
    mut input: RecoveryInputV1,
) -> Result<RecoveryResultV1, StorageContractError> {
    if input
        .previous_app_instance
        .get("clean_shutdown_at")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(RecoveryResultV1 {
            recovery_required: false,
            blocker_ids: Vec::new(),
            runs: input.runs,
            locks: input.locks,
        });
    }

    let mut affected_runs = HashSet::new();
    for run in &mut input.runs {
        let run_id = record_id(run, "run_id")?.to_string();
        if matches!(
            run.get("state").and_then(Value::as_str),
            Some("preparing" | "running" | "cancelling")
        ) {
            run.insert(
                "state".to_string(),
                Value::String("recoveryRequired".to_string()),
            );
            affected_runs.insert(run_id);
        }
    }

    let mut blocker_ids: Vec<String> = affected_runs
        .iter()
        .map(|run_id| format!("run:{run_id}"))
        .collect();
    for lock in &mut input.locks {
        let lock_id = record_id(lock, "lock_id")?.to_string();
        if lock
            .get("writer_run_id")
            .and_then(Value::as_str)
            .is_some_and(|run_id| affected_runs.contains(run_id))
        {
            lock.insert("recovery_required".to_string(), Value::Bool(true));
            blocker_ids.push(format!("lock:{lock_id}"));
        }
    }
    blocker_ids.sort();
    Ok(RecoveryResultV1 {
        recovery_required: !blocker_ids.is_empty(),
        blocker_ids,
        runs: input.runs,
        locks: input.locks,
    })
}

pub fn canonical_json_v1(value: Value) -> Result<String, StorageContractError> {
    serde_json::to_string(&canonicalize(value))
        .map_err(|error| StorageContractError::new(format!("could not encode JSON: {error}")))
}

fn validate_ledger_event(event: &LedgerEventV1) -> Result<(), StorageContractError> {
    for (name, value) in [
        ("event_id", event.event_id.as_str()),
        ("operation_id", event.operation_id.as_str()),
        ("aggregate_id", event.aggregate_id.as_str()),
        ("event_type", event.event_type.as_str()),
        ("content_hash", event.content_hash.as_str()),
        ("timestamp", event.timestamp.as_str()),
    ] {
        require_id(value, name)?;
    }
    if let Some(request_id) = &event.request_id {
        require_id(request_id, "request_id")?;
    }
    aggregate_table(&event.aggregate_type)?;
    if event.aggregate_sequence == 0 || event.aggregate_sequence > super::MAX_SEQUENCE {
        return Err(StorageContractError::new(
            "aggregate_sequence must be between 1 and 2^53-1",
        ));
    }
    if event.payload_schema_version != SQLITE_SCHEMA_VERSION {
        return Err(StorageContractError::new(
            "unsupported payload_schema_version",
        ));
    }
    if event.event_type != format!("{}.upserted", event.aggregate_type) {
        return Err(StorageContractError::new(
            "G3 reducer accepts only aggregate upsert events",
        ));
    }
    Ok(())
}

fn parse_canonical_object(source: &str) -> Result<Map<String, Value>, StorageContractError> {
    let value: Value = serde_json::from_str(source).map_err(|error| {
        StorageContractError::new(format!(
            "canonical_json_payload must be valid JSON: {error}"
        ))
    })?;
    let canonical = canonical_json_v1(value.clone())?;
    if canonical != source {
        return Err(StorageContractError::new(
            "canonical_json_payload is not canonical JSON",
        ));
    }
    value
        .as_object()
        .cloned()
        .ok_or_else(|| StorageContractError::new("canonical_json_payload must be an object"))
}

fn projection_table_mut<'a>(
    projection: &'a mut ProjectionStateV1,
    aggregate_type: &str,
) -> Result<&'a mut BTreeMap<String, Map<String, Value>>, StorageContractError> {
    match aggregate_type {
        "app_instance" => Ok(&mut projection.app_instances),
        "project" => Ok(&mut projection.projects),
        "chat" => Ok(&mut projection.chats),
        "run" => Ok(&mut projection.runs),
        "lock" => Ok(&mut projection.locks),
        other => Err(StorageContractError::new(format!(
            "unknown aggregate_type: {other}"
        ))),
    }
}

fn aggregate_table(aggregate_type: &str) -> Result<&'static str, StorageContractError> {
    match aggregate_type {
        "app_instance" => Ok("app_instances"),
        "project" => Ok("projects"),
        "chat" => Ok("chats"),
        "run" => Ok("runs"),
        "lock" => Ok("locks"),
        other => Err(StorageContractError::new(format!(
            "unknown aggregate_type: {other}"
        ))),
    }
}

fn aggregate_id_field(aggregate_type: &str) -> Result<&'static str, StorageContractError> {
    match aggregate_type {
        "app_instance" => Ok("sidecar_instance_id"),
        "project" => Ok("project_id"),
        "chat" => Ok("chat_id"),
        "run" => Ok("run_id"),
        "lock" => Ok("lock_id"),
        other => Err(StorageContractError::new(format!(
            "unknown aggregate_type: {other}"
        ))),
    }
}

fn record_id<'a>(
    record: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, StorageContractError> {
    let value = record
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default();
    require_id(value, field)?;
    Ok(value)
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let sorted: BTreeMap<_, _> = values
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        other => other,
    }
}

fn require_id(value: &str, name: &str) -> Result<(), StorageContractError> {
    if value.is_empty() {
        Err(StorageContractError::new(format!("{name} is required")))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
