use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display};

pub const SQLITE_SCHEMA_VERSION: u32 = 1;
pub const RECOVERY_VALIDATION_FAILURE_STATUS_V1: &str = "unavailable";
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
    pub timestamp: String,
}

pub type OperationRecordV1 = LedgerEventV1;

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
    pub previous_app_instance: Option<Map<String, Value>>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageStateV1 {
    pub events: Vec<LedgerEventV1>,
    pub projection: ProjectionStateV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionFailurePointV1 {
    AfterEventInsertBeforeProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionOutcomeV1 {
    Committed,
    DuplicateNoOp,
    Conflict,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageTransactionResultV1 {
    pub outcome: TransactionOutcomeV1,
    pub steps: Vec<String>,
    pub state: StorageStateV1,
    pub risky_operation_started: bool,
}

pub fn classify_operation_v1(
    existing: &[OperationRecordV1],
    incoming: &OperationRecordV1,
) -> Result<OperationDecisionV1, StorageContractError> {
    validate_ledger_event(incoming)?;
    let Some(previous) = existing
        .iter()
        .find(|entry| entry.operation_id == incoming.operation_id)
    else {
        return Ok(OperationDecisionV1::Append);
    };
    validate_ledger_event(previous)?;
    Ok(if immutable_operation_content_equal(previous, incoming) {
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
        match classify_operation_v1(&operations, event)? {
            OperationDecisionV1::DuplicateNoOp => continue,
            OperationDecisionV1::Conflict => {
                return Err(StorageContractError::new(format!(
                    "operation_id reused with different content: {}",
                    event.operation_id
                )))
            }
            OperationDecisionV1::Append => operations.push(event.clone()),
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
    let previous = validate_previous_app_instance(input.previous_app_instance.as_ref())?;

    let mut affected_runs = HashSet::new();
    if previous.is_some_and(|row| row.get("clean_shutdown_at") == Some(&Value::Null)) {
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
    }

    let mut blocker_ids: HashSet<String> = affected_runs
        .iter()
        .map(|run_id| format!("run:{run_id}"))
        .collect();
    for lock in &mut input.locks {
        let lock_id = record_id(lock, "lock_id")?.to_string();
        let poisoned = lock
            .get("poisoned")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                StorageContractError::new("persisted lock poisoned must be an explicit boolean")
            })?;
        let mut recovery_required = lock
            .get("recovery_required")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                StorageContractError::new(
                    "persisted lock recovery_required must be an explicit boolean",
                )
            })?;
        if lock
            .get("writer_run_id")
            .and_then(Value::as_str)
            .is_some_and(|run_id| affected_runs.contains(run_id))
        {
            lock.insert("recovery_required".to_string(), Value::Bool(true));
            recovery_required = true;
        }
        if poisoned || recovery_required {
            blocker_ids.insert(format!("lock:{lock_id}"));
        }
    }
    let mut blocker_ids: Vec<_> = blocker_ids.into_iter().collect();
    blocker_ids.sort();
    Ok(RecoveryResultV1 {
        recovery_required: !blocker_ids.is_empty(),
        blocker_ids,
        runs: input.runs,
        locks: input.locks,
    })
}

pub fn apply_pre_risky_storage_transaction_v1(
    before: &StorageStateV1,
    incoming: &LedgerEventV1,
    failure_point: Option<TransactionFailurePointV1>,
) -> Result<StorageTransactionResultV1, StorageContractError> {
    let initial = before.clone();
    if replay_ledger_v1(&initial.events)? != initial.projection {
        return Err(StorageContractError::new(
            "stored projection does not match deterministic replay",
        ));
    }
    let mut steps = vec!["beginImmediate".to_string(), "operationLookup".to_string()];
    match classify_operation_v1(&initial.events, incoming)? {
        OperationDecisionV1::DuplicateNoOp => {
            steps.extend(["duplicateNoOp".to_string(), "commit".to_string()]);
            return Ok(StorageTransactionResultV1 {
                outcome: TransactionOutcomeV1::DuplicateNoOp,
                steps,
                state: initial,
                risky_operation_started: false,
            });
        }
        OperationDecisionV1::Conflict => {
            steps.extend(["operationConflict".to_string(), "rollback".to_string()]);
            return Ok(StorageTransactionResultV1 {
                outcome: TransactionOutcomeV1::Conflict,
                steps,
                state: initial,
                risky_operation_started: false,
            });
        }
        OperationDecisionV1::Append => {}
    }

    let mut events = initial.events.clone();
    events.push(incoming.clone());
    steps.push("eventInserted".to_string());
    if failure_point == Some(TransactionFailurePointV1::AfterEventInsertBeforeProjection) {
        steps.extend(["failureInjected".to_string(), "rollback".to_string()]);
        return Ok(StorageTransactionResultV1 {
            outcome: TransactionOutcomeV1::RolledBack,
            steps,
            state: initial,
            risky_operation_started: false,
        });
    }
    let projection = replay_ledger_v1(&events)?;
    steps.extend([
        "projectionUpdated".to_string(),
        "commit".to_string(),
        "riskyOperationStarted".to_string(),
    ]);
    Ok(StorageTransactionResultV1 {
        outcome: TransactionOutcomeV1::Committed,
        steps,
        state: StorageStateV1 { events, projection },
        risky_operation_started: true,
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

fn immutable_operation_content_equal(left: &LedgerEventV1, right: &LedgerEventV1) -> bool {
    left.event_id == right.event_id
        && left.aggregate_type == right.aggregate_type
        && left.aggregate_id == right.aggregate_id
        && left.aggregate_sequence == right.aggregate_sequence
        && left.event_type == right.event_type
        && left.payload_schema_version == right.payload_schema_version
        && left.canonical_json_payload == right.canonical_json_payload
        && left.timestamp == right.timestamp
}

fn validate_previous_app_instance(
    value: Option<&Map<String, Value>>,
) -> Result<Option<&Map<String, Value>>, StorageContractError> {
    let Some(value) = value else {
        return Ok(None);
    };
    const FIELDS: &[&str] = &[
        "sidecar_instance_id",
        "pid",
        "platform",
        "started_at",
        "heartbeat_at",
        "clean_shutdown_at",
        "last_event_sequence",
    ];
    if value.len() != FIELDS.len() || FIELDS.iter().any(|field| !value.contains_key(*field)) {
        return Err(StorageContractError::new(
            "previous app instance must contain the exact persisted fields",
        ));
    }
    require_id(
        value
            .get("sidecar_instance_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "sidecar_instance_id",
    )?;
    if value
        .get("pid")
        .and_then(Value::as_u64)
        .is_none_or(|pid| pid == 0 || pid > u64::from(u32::MAX))
    {
        return Err(StorageContractError::new("previous pid is invalid"));
    }
    require_id(
        value
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "platform",
    )?;
    for field in ["started_at", "heartbeat_at"] {
        require_timestamp(value.get(field).and_then(Value::as_str), field)?;
    }
    match value.get("clean_shutdown_at") {
        Some(Value::Null) => {}
        Some(Value::String(timestamp)) => require_timestamp(Some(timestamp), "clean_shutdown_at")?,
        _ => {
            return Err(StorageContractError::new(
                "clean_shutdown_at must be an explicit nullable timestamp",
            ))
        }
    }
    if value
        .get("last_event_sequence")
        .and_then(Value::as_u64)
        .is_none_or(|sequence| sequence == 0 || sequence > super::MAX_SEQUENCE)
    {
        return Err(StorageContractError::new(
            "previous last_event_sequence is invalid",
        ));
    }
    Ok(Some(value))
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

fn require_timestamp(value: Option<&str>, name: &str) -> Result<(), StorageContractError> {
    let Some(value) = value else {
        return Err(StorageContractError::new(format!(
            "{name} must be an RFC 3339 UTC second timestamp"
        )));
    };
    let bytes = value.as_bytes();
    let separators = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'Z'),
    ];
    let valid = bytes.len() == 20
        && separators
            .iter()
            .all(|(index, expected)| bytes.get(*index) == Some(expected))
        && bytes.iter().enumerate().all(|(index, byte)| {
            separators.iter().any(|(separator, _)| *separator == index) || byte.is_ascii_digit()
        });
    if valid {
        Ok(())
    } else {
        Err(StorageContractError::new(format!(
            "{name} must be an RFC 3339 UTC second timestamp"
        )))
    }
}

#[cfg(test)]
mod tests;
