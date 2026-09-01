use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_protocol::storage_lifetime_v1::{
    validate_frozen_operation_source_binding_v1, validate_storage_event_source_index_transition_v1,
    validate_storage_lifetime_materialization_job_v1,
    validate_storage_lifetime_materialization_job_v2,
    validate_storage_lifetime_materialization_job_v3,
    validate_storage_lifetime_materialization_job_v4,
    validate_storage_lifetime_materialization_pair_v1,
    validate_storage_lifetime_materialization_pair_v2,
    validate_storage_lifetime_materialization_pair_v3,
    validate_storage_lifetime_materialization_pair_v4, validate_storage_lifetime_reopen_pair_v2,
    validate_storage_lifetime_reopen_pair_v3, validate_storage_lifetime_reopen_pair_v4,
    StorageStatusV1, STORAGE_LIFETIME_SCHEMA_VERSION_V1, STORAGE_LIFETIME_SCHEMA_VERSION_V3,
    STORAGE_LIFETIME_SCHEMA_VERSION_V4, STORAGE_LIFETIME_SCHEMA_VERSION_V5,
    STORAGE_LIFETIME_SCHEMA_VERSION_V6, STORAGE_LIFETIME_SCHEMA_VERSION_V7,
    STORAGE_LIFETIME_SCHEMA_VERSION_V8,
};
use hello_agent_protocol::storage_v1::{
    canonical_json_v1, classify_operation_v1, recover_unclean_shutdown_v1, replay_ledger_v1,
    LedgerEventV1, OperationDecisionV1, ProjectionStateV1, RecoveryInputV1, SQLITE_SCHEMA_VERSION,
};
use hello_agent_protocol::{ReadyPayloadV1, APP_SCHEMA_VERSION, PROTOCOL_VERSION};
use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MIGRATION_V1: &str = include_str!("../../../tests/fixtures/m5/storage-v1/migration.sql");
const MIGRATION_V2: &str =
    include_str!("../../../tests/fixtures/m5/storage-v2/migration-v1-to-v2.sql");
const MIGRATION_V3: &str =
    include_str!("../../../tests/fixtures/m5/storage-v3/migration-v2-to-v3.sql");
const MIGRATION_V4: &str =
    include_str!("../../../tests/fixtures/m5/storage-v4/migration-v3-to-v4.sql");
const MIGRATION_V5: &str =
    include_str!("../../../tests/fixtures/m5/storage-v5/migration-v4-to-v5.sql");
const MIGRATION_V6: &str =
    include_str!("../../../tests/fixtures/m5/storage-v6/migration-v5-to-v6.sql");
const MIGRATION_V7: &str =
    include_str!("../../../tests/fixtures/m5/storage-v7/migration-v6-to-v7.sql");
const MIGRATION_V8: &str =
    include_str!("../../../tests/fixtures/m5/storage-v8/migration-v7-to-v8.sql");
type SchemaObject = (String, String, String, Option<String>);
type LifetimeJobRow = (
    u32,
    String,
    String,
    i64,
    String,
    i64,
    Option<String>,
    String,
    String,
    Option<String>,
);
type LifetimeJobRowV4 = (
    u32,
    String,
    String,
    i64,
    String,
    i64,
    i64,
    Option<String>,
    String,
    String,
    Option<String>,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendFailurePoint {
    AfterEventInsertBeforeProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Committed,
    DuplicateNoOp,
    Conflict,
}

pub struct Storage {
    connection: Connection,
    path: PathBuf,
}

#[derive(Debug)]
pub struct ProjectionUpsert {
    pub aggregate_type: &'static str,
    pub aggregate_id: String,
    pub operation_id: String,
    pub request_id: Option<String>,
    pub payload: Map<String, Value>,
}

#[derive(Debug)]
pub struct ChatBindInput {
    pub preparation_id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub workspace_identity: String,
    pub expected_settings_revision: u64,
    pub chat_id: String,
    pub request_id: String,
    pub chat_payload: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatBindOutcome {
    Committed,
    PreparationUnknown,
    SettingsDrift,
    PreparationNotUnbound,
    PreparationMismatch,
    BindingConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatBindFailurePoint {
    AfterPreparationBindBeforeChat,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, SidecarError> {
        let path = safe_database_path(path)?;
        let mut connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(sqlite_open_error)?;
        configure_connection(&connection)?;
        migrate(&mut connection, MIGRATION_V1)?;
        verify_schema(&connection)?;
        match storage_schema_version(&connection)? {
            None => {
                migrate_v2(&mut connection)?;
                verify_schema_v2(&connection)?;
                migrate_v3(&mut connection)?;
                verify_schema_v3(&connection)?;
                migrate_v4(&mut connection)?;
                verify_schema_v4(&connection)?;
                migrate_v5(&mut connection)?;
                verify_schema_v5(&connection)?;
                migrate_v6(&mut connection)?;
                verify_schema_v6(&connection)?;
                migrate_v7(&mut connection)?;
                verify_schema_v7(&connection)?;
                migrate_v8(&mut connection)?;
                verify_schema_v8_for_open(&connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V1) => {
                verify_schema_v2(&connection)?;
                migrate_v3(&mut connection)?;
                verify_schema_v3(&connection)?;
                migrate_v4(&mut connection)?;
                verify_schema_v4(&connection)?;
                migrate_v5(&mut connection)?;
                verify_schema_v5(&connection)?;
                migrate_v6(&mut connection)?;
                verify_schema_v6(&connection)?;
                migrate_v7(&mut connection)?;
                verify_schema_v7(&connection)?;
                migrate_v8(&mut connection)?;
                verify_schema_v8_for_open(&connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V3) => {
                verify_schema_v3(&connection)?;
                migrate_v4(&mut connection)?;
                verify_schema_v4(&connection)?;
                migrate_v5(&mut connection)?;
                verify_schema_v5(&connection)?;
                migrate_v6(&mut connection)?;
                verify_schema_v6(&connection)?;
                migrate_v7(&mut connection)?;
                verify_schema_v7(&connection)?;
                migrate_v8(&mut connection)?;
                verify_schema_v8_for_open(&connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V4) => {
                verify_schema_v4(&connection)?;
                migrate_v5(&mut connection)?;
                verify_schema_v5(&connection)?;
                migrate_v6(&mut connection)?;
                verify_schema_v6(&connection)?;
                migrate_v7(&mut connection)?;
                verify_schema_v7(&connection)?;
                migrate_v8(&mut connection)?;
                verify_schema_v8_for_open(&connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V5) => {
                verify_schema_v5(&connection)?;
                migrate_v6(&mut connection)?;
                verify_schema_v6(&connection)?;
                migrate_v7(&mut connection)?;
                verify_schema_v7(&connection)?;
                migrate_v8(&mut connection)?;
                verify_schema_v8_for_open(&connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V6) => {
                verify_schema_v6(&connection)?;
                migrate_v7(&mut connection)?;
                verify_schema_v7(&connection)?;
                migrate_v8(&mut connection)?;
                verify_schema_v8_for_open(&connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V7) => {
                verify_schema_v7(&connection)?;
                migrate_v8(&mut connection)?;
                verify_schema_v8_for_open(&connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V8) => {
                verify_schema_v8_for_open(&connection)?;
            }
            Some(_) => {
                return Err(SidecarError::new(
                    "storageSchemaMismatch",
                    "storage schema metadata version is unsupported",
                ));
            }
        }
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(sqlite_error)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(sqlite_error)?;
        verify_integrity(&connection)?;
        verify_projection_replay(&connection)?;
        Ok(Self { connection, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    #[allow(dead_code)] // D1A verifies the internal result before dispatcher activation.
    pub(crate) fn lifetime_status(&self) -> Result<StorageStatusV1, SidecarError> {
        crate::storage_lifetime::collect_status(&self.connection, &self.path)
    }

    #[allow(dead_code)] // D1B is internal until a later wire/idle-scheduler wave.
    pub(crate) fn materialize_lifetime_records(
        &mut self,
        failure: Option<crate::storage_lifetime::MaterializationFailurePoint>,
    ) -> Result<crate::storage_lifetime::LifetimeMaterializationResult, SidecarError> {
        crate::storage_lifetime::materialize_lifetime_records(&mut self.connection, failure)
    }

    #[cfg(test)]
    pub(crate) fn materialize_lifetime_batch_for_test(
        &mut self,
        cancellation: &mut dyn crate::storage_lifetime::MaterializationCancellation,
        failure: Option<crate::storage_lifetime::BatchMaterializationFailurePoint>,
    ) -> Result<crate::storage_lifetime::LifetimeMaterializationBatchResult, SidecarError> {
        crate::storage_lifetime::materialize_lifetime_batch(
            &mut self.connection,
            cancellation,
            failure,
        )
    }

    #[cfg(test)]
    pub(crate) fn connection_for_test(&self) -> &Connection {
        &self.connection
    }

    #[cfg(test)]
    pub(crate) fn open_v3_for_test(path: &Path) -> Result<Self, SidecarError> {
        let path = safe_database_path(path)?;
        let mut connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(sqlite_open_error)?;
        configure_connection(&connection)?;
        migrate(&mut connection, MIGRATION_V1)?;
        verify_schema(&connection)?;
        match storage_schema_version(&connection)? {
            None => {
                migrate_v2(&mut connection)?;
                verify_schema_v2(&connection)?;
                migrate_v3(&mut connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V1) => {
                verify_schema_v2(&connection)?;
                migrate_v3(&mut connection)?;
            }
            Some(STORAGE_LIFETIME_SCHEMA_VERSION_V3) => {}
            Some(_) => {
                return Err(SidecarError::new(
                    "storageSchemaMismatch",
                    "test storage is not an exact v3 database",
                ));
            }
        }
        verify_schema_v3(&connection)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(sqlite_error)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(sqlite_error)?;
        Ok(Self { connection, path })
    }

    #[cfg(test)]
    pub(crate) fn open_v4_for_test(path: &Path) -> Result<Self, SidecarError> {
        let mut storage = Self::open_v3_for_test(path)?;
        migrate_v4(&mut storage.connection)?;
        verify_schema_v4_for_open(&storage.connection)?;
        Ok(storage)
    }

    #[cfg(test)]
    pub(crate) fn open_v5_for_test(path: &Path) -> Result<Self, SidecarError> {
        let mut storage = Self::open_v4_for_test(path)?;
        migrate_v5(&mut storage.connection)?;
        verify_schema_v5_for_open(&storage.connection)?;
        Ok(storage)
    }

    #[cfg(test)]
    pub(crate) fn open_v6_for_test(path: &Path) -> Result<Self, SidecarError> {
        let mut storage = Self::open_v5_for_test(path)?;
        migrate_v6(&mut storage.connection)?;
        verify_schema_v6_for_open(&storage.connection)?;
        Ok(storage)
    }

    #[cfg(test)]
    pub(crate) fn open_v7_for_test(path: &Path) -> Result<Self, SidecarError> {
        let mut storage = Self::open_v6_for_test(path)?;
        migrate_v7(&mut storage.connection)?;
        verify_schema_v7_for_open(&storage.connection)?;
        Ok(storage)
    }

    pub fn append_event(&mut self, event: &LedgerEventV1) -> Result<AppendOutcome, SidecarError> {
        self.append_event_with_failure(event, None)
    }

    pub fn append_event_with_failure(
        &mut self,
        event: &LedgerEventV1,
        failure: Option<AppendFailurePoint>,
    ) -> Result<AppendOutcome, SidecarError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let existing_events = load_events(&transaction)?;
        match classify_operation_v1(&existing_events, event).map_err(storage_contract_error)? {
            OperationDecisionV1::DuplicateNoOp => {
                transaction.commit().map_err(sqlite_error)?;
                return Ok(AppendOutcome::DuplicateNoOp);
            }
            OperationDecisionV1::Conflict => {
                transaction.rollback().map_err(sqlite_error)?;
                return Ok(AppendOutcome::Conflict);
            }
            OperationDecisionV1::Append => {}
        }

        insert_event(&transaction, event)?;
        if failure == Some(AppendFailurePoint::AfterEventInsertBeforeProjection) {
            return Err(SidecarError::new(
                "injectedFailure",
                "failure injected after event insert before projection update",
            ));
        }
        let events = load_events(&transaction)?;
        let projection = replay_ledger_v1(&events).map_err(storage_contract_error)?;
        write_projection(&transaction, event, &projection)?;
        transaction.commit().map_err(sqlite_error)?;
        Ok(AppendOutcome::Committed)
    }

    pub fn recover_unclean_shutdown(&mut self) -> Result<(), SidecarError> {
        let previous = latest_app_instance(&self.connection)?;
        let projection = load_projection(&self.connection)?;
        let input = RecoveryInputV1 {
            previous_app_instance: previous.clone(),
            runs: projection.runs.into_values().collect(),
            locks: projection.locks.into_values().collect(),
        };
        let recovered = recover_unclean_shutdown_v1(input).map_err(storage_contract_error)?;
        let Some(previous) = previous else {
            return Ok(());
        };
        let recovery_timestamp = required_string(&previous, "heartbeat_at")?.to_string();
        let previous_id = required_string(&previous, "sidecar_instance_id")?;
        let current = load_projection(&self.connection)?;
        let mut events = Vec::new();

        for record in recovered.runs {
            let id = required_string(&record, "run_id")?.to_string();
            if current.runs.get(&id) != Some(&record) {
                events.push(recovery_event(
                    previous_id,
                    "run",
                    &id,
                    record,
                    &recovery_timestamp,
                )?);
            }
        }
        for record in recovered.locks {
            let id = required_string(&record, "lock_id")?.to_string();
            if current.locks.get(&id) != Some(&record) {
                events.push(recovery_event(
                    previous_id,
                    "lock",
                    &id,
                    record,
                    &recovery_timestamp,
                )?);
            }
        }
        self.append_recovery_events(&events)
    }

    pub fn register_app_instance(&mut self, ready: &ReadyPayloadV1) -> Result<(), SidecarError> {
        let timestamp = now_utc();
        let payload = json!({
            "clean_shutdown_at": Value::Null,
            "heartbeat_at": timestamp,
            "pid": ready.pid,
            "platform": ready.platform,
            "sidecar_instance_id": ready.sidecar_instance_id,
            "started_at": timestamp,
        });
        let event = LedgerEventV1 {
            event_id: format!("evt_start_{}", Uuid::new_v4().simple()),
            operation_id: format!("op_start_{}", ready.sidecar_instance_id),
            request_id: None,
            aggregate_type: "app_instance".to_string(),
            aggregate_id: ready.sidecar_instance_id.clone(),
            aggregate_sequence: 1,
            event_type: "app_instance.upserted".to_string(),
            payload_schema_version: SQLITE_SCHEMA_VERSION,
            canonical_json_payload: canonical_json_v1(payload).map_err(storage_contract_error)?,
            timestamp,
        };
        match self.append_event(&event)? {
            AppendOutcome::Committed | AppendOutcome::DuplicateNoOp => Ok(()),
            AppendOutcome::Conflict => Err(SidecarError::new(
                "operationConflict",
                "startup app-instance operation conflicted with durable history",
            )),
        }
    }

    pub fn checkpoint(&self) -> Result<(), SidecarError> {
        let (busy, _log, _checkpointed): (u32, i64, i64) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(FULL);", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(sqlite_error)?;
        if busy != 0 {
            return Err(SidecarError::new(
                "checkpointBusy",
                "SQLite checkpoint did not complete because another owner was present",
            ));
        }
        Ok(())
    }

    pub fn mark_app_instance_clean(
        &mut self,
        sidecar_instance_id: &str,
    ) -> Result<(), SidecarError> {
        let projection = load_projection(&self.connection)?;
        let current = projection
            .app_instances
            .get(sidecar_instance_id)
            .ok_or_else(|| {
                SidecarError::new(
                    "appInstanceMissing",
                    "current app instance projection is missing",
                )
            })?;
        if current.get("clean_shutdown_at") != Some(&Value::Null) {
            return match current.get("clean_shutdown_at") {
                Some(Value::String(_)) => Ok(()),
                _ => Err(SidecarError::new(
                    "cleanMarkerInvalid",
                    "current app instance clean marker is invalid",
                )),
            };
        }
        let timestamp = now_utc();
        let mut payload = current.clone();
        let sequence = required_u64(&payload, "last_event_sequence")?
            .checked_add(1)
            .ok_or_else(|| SidecarError::new("sequenceOverflow", "event sequence overflow"))?;
        payload.remove("last_event_sequence");
        payload.insert(
            "clean_shutdown_at".to_string(),
            Value::String(timestamp.clone()),
        );
        let event = LedgerEventV1 {
            event_id: format!("evt_clean_{}", Uuid::new_v4().simple()),
            operation_id: format!("op_clean_{sidecar_instance_id}"),
            request_id: None,
            aggregate_type: "app_instance".to_string(),
            aggregate_id: sidecar_instance_id.to_string(),
            aggregate_sequence: sequence,
            event_type: "app_instance.upserted".to_string(),
            payload_schema_version: SQLITE_SCHEMA_VERSION,
            canonical_json_payload: canonical_json_v1(Value::Object(payload))
                .map_err(storage_contract_error)?,
            timestamp,
        };
        match self.append_event(&event)? {
            AppendOutcome::Committed | AppendOutcome::DuplicateNoOp => Ok(()),
            AppendOutcome::Conflict => Err(SidecarError::new(
                "operationConflict",
                "clean-shutdown operation conflicted with durable history",
            )),
        }
    }

    pub fn blocker_ids(&self) -> Result<Vec<String>, SidecarError> {
        let projection = load_projection(&self.connection)?;
        let mut blockers = BTreeSet::new();
        for (run_id, run) in projection.runs {
            if run
                .get("state")
                .and_then(Value::as_str)
                .is_some_and(|state| {
                    ["preparing", "running", "cancelling", "recoveryRequired"].contains(&state)
                })
            {
                blockers.insert(format!("run:{run_id}"));
            }
        }
        for (lock_id, lock) in projection.locks {
            let poisoned = required_bool(&lock, "poisoned")?;
            let recovery_required = required_bool(&lock, "recovery_required")?;
            if poisoned || recovery_required {
                blockers.insert(format!("lock:{lock_id}"));
            }
        }
        Ok(blockers.into_iter().collect())
    }

    pub fn projection_record(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Option<Map<String, Value>>, SidecarError> {
        let projection = load_projection(&self.connection)?;
        let table = match aggregate_type {
            "project" => &projection.projects,
            "chat" => &projection.chats,
            "lock" => &projection.locks,
            "run" => &projection.runs,
            _ => {
                return Err(SidecarError::new(
                    "aggregateUnsupported",
                    "projection lookup supports only project, chat, lock, and run",
                ))
            }
        };
        Ok(table.get(aggregate_id).cloned())
    }

    pub fn projection_records(
        &self,
        aggregate_type: &str,
    ) -> Result<Vec<Map<String, Value>>, SidecarError> {
        let projection = load_projection(&self.connection)?;
        let table = match aggregate_type {
            "project" => &projection.projects,
            "chat" => &projection.chats,
            "lock" => &projection.locks,
            "run" => &projection.runs,
            _ => {
                return Err(SidecarError::new(
                    "aggregateUnsupported",
                    "projection listing contains an unknown aggregate type",
                ))
            }
        };
        Ok(table.values().cloned().collect())
    }

    pub fn aggregate_event_payloads(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<Map<String, Value>>, SidecarError> {
        load_events(&self.connection)?
            .into_iter()
            .filter(|event| {
                event.aggregate_type == aggregate_type && event.aggregate_id == aggregate_id
            })
            .map(|event| {
                let value: Value =
                    serde_json::from_str(&event.canonical_json_payload).map_err(|error| {
                        SidecarError::new(
                            "projectionInvalid",
                            format!("ledger payload is invalid: {error}"),
                        )
                    })?;
                value.as_object().cloned().ok_or_else(|| {
                    SidecarError::new("projectionInvalid", "ledger payload is not an object")
                })
            })
            .collect()
    }

    pub fn upsert_projection_record(
        &mut self,
        aggregate_type: &str,
        aggregate_id: &str,
        operation_id: &str,
        request_id: Option<&str>,
        mut payload: Map<String, Value>,
    ) -> Result<AppendOutcome, SidecarError> {
        let sequence = self
            .projection_record(aggregate_type, aggregate_id)?
            .as_ref()
            .map(|record| required_u64(record, "last_event_sequence"))
            .transpose()?
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| SidecarError::new("sequenceOverflow", "event sequence overflow"))?;
        payload.remove("last_event_sequence");
        let timestamp = now_utc();
        let event = LedgerEventV1 {
            event_id: format!("evt_{}", Uuid::new_v4().simple()),
            operation_id: operation_id.to_string(),
            request_id: request_id.map(str::to_string),
            aggregate_type: aggregate_type.to_string(),
            aggregate_id: aggregate_id.to_string(),
            aggregate_sequence: sequence,
            event_type: format!("{aggregate_type}.upserted"),
            payload_schema_version: SQLITE_SCHEMA_VERSION,
            canonical_json_payload: canonical_json_v1(Value::Object(payload))
                .map_err(storage_contract_error)?,
            timestamp,
        };
        self.append_event(&event)
    }

    pub fn upsert_projection_records_atomic(
        &mut self,
        upserts: Vec<ProjectionUpsert>,
    ) -> Result<Vec<AppendOutcome>, SidecarError> {
        if upserts.is_empty() {
            return Ok(Vec::new());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let mut events = load_events(&transaction)?;
        let mut projection = replay_ledger_v1(&events).map_err(storage_contract_error)?;
        let mut outcomes = Vec::with_capacity(upserts.len());
        let mut appended = Vec::new();

        for mut upsert in upserts {
            upsert.payload.remove("last_event_sequence");
            let current = match upsert.aggregate_type {
                "project" => projection.projects.get(&upsert.aggregate_id),
                "chat" => projection.chats.get(&upsert.aggregate_id),
                "run" => projection.runs.get(&upsert.aggregate_id),
                "lock" => projection.locks.get(&upsert.aggregate_id),
                _ => {
                    return Err(SidecarError::new(
                        "aggregateUnsupported",
                        "atomic projection upsert contains an unknown aggregate type",
                    ))
                }
            };
            let sequence = current
                .map(|record| required_u64(record, "last_event_sequence"))
                .transpose()?
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| SidecarError::new("sequenceOverflow", "event sequence overflow"))?;
            let event = LedgerEventV1 {
                event_id: format!("evt_{}", Uuid::new_v4().simple()),
                operation_id: upsert.operation_id,
                request_id: upsert.request_id,
                aggregate_type: upsert.aggregate_type.to_string(),
                aggregate_id: upsert.aggregate_id,
                aggregate_sequence: sequence,
                event_type: format!("{}.upserted", upsert.aggregate_type),
                payload_schema_version: SQLITE_SCHEMA_VERSION,
                canonical_json_payload: canonical_json_v1(Value::Object(upsert.payload))
                    .map_err(storage_contract_error)?,
                timestamp: now_utc(),
            };
            match classify_operation_v1(&events, &event).map_err(storage_contract_error)? {
                OperationDecisionV1::Append => {
                    insert_event(&transaction, &event)?;
                    events.push(event.clone());
                    projection = replay_ledger_v1(&events).map_err(storage_contract_error)?;
                    appended.push(event);
                    outcomes.push(AppendOutcome::Committed);
                }
                OperationDecisionV1::DuplicateNoOp => outcomes.push(AppendOutcome::DuplicateNoOp),
                OperationDecisionV1::Conflict => {
                    return Err(SidecarError::new(
                        "operationConflict",
                        "atomic operation conflicted with durable history",
                    ))
                }
            }
        }
        for event in &appended {
            write_projection(&transaction, event, &projection)?;
        }
        transaction.commit().map_err(sqlite_error)?;
        Ok(outcomes)
    }

    pub fn bind_prepared_chat_atomic(
        &mut self,
        input: ChatBindInput,
    ) -> Result<ChatBindOutcome, SidecarError> {
        self.bind_prepared_chat_atomic_inner(input, None)
    }

    #[cfg(test)]
    pub(crate) fn bind_prepared_chat_atomic_with_failure(
        &mut self,
        input: ChatBindInput,
        failure: ChatBindFailurePoint,
    ) -> Result<ChatBindOutcome, SidecarError> {
        self.bind_prepared_chat_atomic_inner(input, Some(failure))
    }

    fn bind_prepared_chat_atomic_inner(
        &mut self,
        input: ChatBindInput,
        failure: Option<ChatBindFailurePoint>,
    ) -> Result<ChatBindOutcome, SidecarError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let mut events = load_events(&transaction)?;
        let projection = replay_ledger_v1(&events).map_err(storage_contract_error)?;
        let Some(project) = projection.projects.get(&input.project_id) else {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(ChatBindOutcome::PreparationUnknown);
        };
        let settings_revision = project
            .get("role_settings")
            .and_then(Value::as_object)
            .and_then(|settings| settings.get("revision"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        if settings_revision != input.expected_settings_revision {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(ChatBindOutcome::SettingsDrift);
        }
        let Some(preparations) = project
            .get("prepared_workspaces")
            .and_then(Value::as_object)
        else {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(ChatBindOutcome::PreparationUnknown);
        };
        let Some(prepared) = preparations.get(&input.preparation_id) else {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(ChatBindOutcome::PreparationUnknown);
        };
        if prepared.get("bindingState").and_then(Value::as_str) != Some("unbound") {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(ChatBindOutcome::PreparationNotUnbound);
        }
        let manifest = prepared.get("manifest").and_then(Value::as_object);
        let matches = prepared.get("preparationId").and_then(Value::as_str)
            == Some(input.preparation_id.as_str())
            && manifest.and_then(|value| value.get("projectId"))
                == Some(&Value::String(input.project_id.clone()))
            && manifest.and_then(|value| value.get("worktreeId"))
                == Some(&Value::String(input.worktree_id.clone()))
            && manifest.and_then(|value| value.get("workspaceIdentity"))
                == Some(&Value::String(input.workspace_identity.clone()))
            && manifest
                .and_then(|value| value.get("lifecycle"))
                .and_then(Value::as_str)
                == Some("active");
        if !matches {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(ChatBindOutcome::PreparationMismatch);
        }
        if projection.chats.iter().any(|(chat_id, chat)| {
            chat_id == &input.chat_id
                || chat.get("chat_id").and_then(Value::as_str) == Some(input.chat_id.as_str())
                || chat.get("worktree_id").and_then(Value::as_str)
                    == Some(input.worktree_id.as_str())
                || chat.get("workspace_identity").and_then(Value::as_str)
                    == Some(input.workspace_identity.as_str())
        }) {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(ChatBindOutcome::BindingConflict);
        }

        let mut bound = prepared.clone();
        bound["bindingState"] = Value::String("bound".to_string());
        bound["boundChatId"] = Value::String(input.chat_id.clone());
        bound["updatedAt"] = input
            .chat_payload
            .get("record")
            .and_then(|chat| chat.get("updatedAt"))
            .cloned()
            .ok_or_else(|| SidecarError::new("projectionInvalid", "chat updatedAt is missing"))?;
        hello_agent_protocol::run_recovery_v1::validate_g5_payload_v1(
            "preparedChatWorkspace",
            &bound,
        )
        .map_err(|error| SidecarError::new("projectionInvalid", error.to_string()))?;
        let mut project_payload = project.clone();
        project_payload.remove("last_event_sequence");
        let mut updated_preparations = preparations.clone();
        updated_preparations.insert(input.preparation_id.clone(), bound);
        project_payload.insert(
            "prepared_workspaces".to_string(),
            Value::Object(updated_preparations),
        );
        let upserts = vec![
            ProjectionUpsert {
                aggregate_type: "project",
                aggregate_id: input.project_id,
                operation_id: format!("op_chat_bind_{}", input.request_id),
                request_id: Some(input.request_id.clone()),
                payload: project_payload,
            },
            ProjectionUpsert {
                aggregate_type: "chat",
                aggregate_id: input.chat_id,
                operation_id: format!("op_chat_create_{}", input.request_id),
                request_id: Some(input.request_id),
                payload: input.chat_payload,
            },
        ];
        let mut appended = Vec::new();
        for (index, mut upsert) in upserts.into_iter().enumerate() {
            upsert.payload.remove("last_event_sequence");
            let current = match upsert.aggregate_type {
                "project" => projection.projects.get(&upsert.aggregate_id),
                "chat" => projection.chats.get(&upsert.aggregate_id),
                _ => unreachable!("fixed chat binding aggregates"),
            };
            let sequence = current
                .map(|record| required_u64(record, "last_event_sequence"))
                .transpose()?
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| SidecarError::new("sequenceOverflow", "event sequence overflow"))?;
            let event = LedgerEventV1 {
                event_id: format!("evt_{}", Uuid::new_v4().simple()),
                operation_id: upsert.operation_id,
                request_id: upsert.request_id,
                aggregate_type: upsert.aggregate_type.to_string(),
                aggregate_id: upsert.aggregate_id,
                aggregate_sequence: sequence,
                event_type: format!("{}.upserted", upsert.aggregate_type),
                payload_schema_version: SQLITE_SCHEMA_VERSION,
                canonical_json_payload: canonical_json_v1(Value::Object(upsert.payload))
                    .map_err(storage_contract_error)?,
                timestamp: now_utc(),
            };
            match classify_operation_v1(&events, &event).map_err(storage_contract_error)? {
                OperationDecisionV1::Append => {
                    insert_event(&transaction, &event)?;
                    events.push(event.clone());
                    appended.push(event);
                    if index == 0
                        && failure == Some(ChatBindFailurePoint::AfterPreparationBindBeforeChat)
                    {
                        return Err(SidecarError::new(
                            "injectedFailure",
                            "failure injected after preparation bind before chat projection",
                        ));
                    }
                }
                OperationDecisionV1::DuplicateNoOp => {}
                OperationDecisionV1::Conflict => {
                    transaction.rollback().map_err(sqlite_error)?;
                    return Ok(ChatBindOutcome::BindingConflict);
                }
            }
        }
        let final_projection = replay_ledger_v1(&events).map_err(storage_contract_error)?;
        for event in &appended {
            write_projection(&transaction, event, &final_projection)?;
        }
        transaction.commit().map_err(sqlite_error)?;
        Ok(ChatBindOutcome::Committed)
    }

    fn append_recovery_events(&mut self, incoming: &[LedgerEventV1]) -> Result<(), SidecarError> {
        if incoming.is_empty() {
            return Ok(());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let mut events = load_events(&transaction)?;
        let mut appended = Vec::new();
        for event in incoming {
            match classify_operation_v1(&events, event).map_err(storage_contract_error)? {
                OperationDecisionV1::Append => {
                    insert_event(&transaction, event)?;
                    events.push(event.clone());
                    appended.push(event);
                }
                OperationDecisionV1::DuplicateNoOp => {}
                OperationDecisionV1::Conflict => {
                    return Err(SidecarError::new(
                        "operationConflict",
                        "recovery operation conflicted with durable history",
                    ))
                }
            }
        }
        let projection = replay_ledger_v1(&events).map_err(storage_contract_error)?;
        for event in appended {
            write_projection(&transaction, event, &projection)?;
        }
        transaction.commit().map_err(sqlite_error)
    }
}

fn safe_database_path(path: &Path) -> Result<PathBuf, SidecarError> {
    let parent = path.parent().ok_or_else(|| {
        SidecarError::new(
            "databasePathInvalid",
            "database path has no parent directory",
        )
    })?;
    if !parent.is_dir() {
        return Err(SidecarError::new(
            "databasePathInvalid",
            "database parent is not an existing directory",
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SidecarError::new(
                "databasePathInvalid",
                "database path must be a regular file and cannot be a symlink",
            ));
        }
    }
    let file_name = path.file_name().ok_or_else(|| {
        SidecarError::new("databasePathInvalid", "database path has no file name")
    })?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        SidecarError::new(
            "databasePathInvalid",
            format!("could not canonicalize database parent: {error}"),
        )
    })?;
    Ok(canonical_parent.join(file_name))
}

fn configure_connection(connection: &Connection) -> Result<(), SidecarError> {
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(sqlite_error)?;
    let enabled: u32 = connection
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if enabled != 1 {
        return Err(SidecarError::new(
            "foreignKeysDisabled",
            format!("PRAGMA foreign_keys verification returned {enabled}"),
        ));
    }
    let locking_mode: String = connection
        .query_row("PRAGMA locking_mode = EXCLUSIVE;", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if locking_mode != "exclusive" {
        return Err(SidecarError::new(
            "databaseOwnershipFailed",
            format!("SQLite locking_mode verification returned {locking_mode}"),
        ));
    }
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(sqlite_error)
}

fn migrate(connection: &mut Connection, migration: &str) -> Result<(), SidecarError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_meta');",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists {
        return Ok(());
    }
    execute_migration(connection, migration, "v1")
}

fn migrate_v2(connection: &mut Connection) -> Result<(), SidecarError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='storage_schema_meta');",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists {
        return Ok(());
    }

    if schema_objects(connection)? != reference_schema_v1()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema is not the exact storage v1 migration base",
        ));
    }

    execute_migration(connection, MIGRATION_V2, "v2")
}

fn storage_schema_version(connection: &Connection) -> Result<Option<u32>, SidecarError> {
    let metadata_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='storage_schema_meta');",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if !metadata_exists {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT storage_schema_version FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema metadata version is unreadable",
            )
        })?
        .ok_or_else(|| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema metadata version is missing",
            )
        })
        .map(Some)
}

fn migrate_v3(connection: &mut Connection) -> Result<(), SidecarError> {
    if schema_objects(connection)? != reference_schema_v2()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema is not the exact storage v2 migration base",
        ));
    }
    verify_schema_v2(connection)?;
    execute_migration(connection, MIGRATION_V3, "v3")
}

fn migrate_v4(connection: &mut Connection) -> Result<(), SidecarError> {
    if schema_objects(connection)? != reference_schema_v3()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema is not the exact storage v3 migration base",
        ));
    }
    verify_schema_v3(connection)?;
    execute_migration(connection, MIGRATION_V4, "v4")
}

fn migrate_v5(connection: &mut Connection) -> Result<(), SidecarError> {
    if schema_objects(connection)? != reference_schema_v4()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema is not the exact storage v4 migration base",
        ));
    }
    verify_schema_v4(connection)?;
    execute_migration(connection, MIGRATION_V5, "v5")
}

fn migrate_v6(connection: &mut Connection) -> Result<(), SidecarError> {
    if schema_objects(connection)? != reference_schema_v5()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema is not the exact storage v5 migration base",
        ));
    }
    verify_schema_v5(connection)?;
    execute_migration(connection, MIGRATION_V6, "v6")
}

fn migrate_v7(connection: &mut Connection) -> Result<(), SidecarError> {
    if schema_objects(connection)? != reference_schema_v6()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema is not the exact storage v6 migration base",
        ));
    }
    verify_schema_v6(connection)?;
    let attempting_materialization: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM lifetime_materialization_job \
             WHERE singleton=1 AND state='attempting');",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_materialization_snapshot_error())?;
    let active_compaction: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM maintenance_jobs \
             WHERE job_kind='losslessCompaction' AND state IN ('pending','running'));",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_compaction_state_error())?;
    if attempting_materialization {
        return Err(storage_materialization_snapshot_error());
    }
    if active_compaction {
        return Err(storage_compaction_state_error());
    }
    execute_migration(connection, MIGRATION_V7, "v7")
}

fn migrate_v8(connection: &mut Connection) -> Result<(), SidecarError> {
    if schema_objects(connection)? != reference_schema_v7()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema is not the exact storage v7 migration base",
        ));
    }
    verify_schema_v7(connection)?;
    let attempting_materialization: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM lifetime_materialization_job \
             WHERE singleton=1 AND state='attempting');",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_materialization_snapshot_error())?;
    if attempting_materialization {
        return Err(storage_materialization_snapshot_error());
    }
    verify_v7_uncompacted_runtime_state(connection)?;
    execute_migration(connection, MIGRATION_V8, "v8")
}

fn execute_migration(
    connection: &mut Connection,
    migration: &str,
    version: &str,
) -> Result<(), SidecarError> {
    if let Err(error) = connection.execute_batch(migration) {
        let rollback_error = if connection.is_autocommit() {
            None
        } else {
            connection.execute_batch("ROLLBACK;").err()
        };
        let detail = match rollback_error {
            Some(rollback) => {
                format!("migration {version} failed: {error}; rollback failed: {rollback}")
            }
            None => format!("migration {version} failed: {error}"),
        };
        return Err(SidecarError::new("migrationFailed", detail));
    }
    if !connection.is_autocommit() {
        let _ = connection.execute_batch("ROLLBACK;");
        return Err(SidecarError::new(
            "migrationFailed",
            format!("migration {version} did not finish its transaction"),
        ));
    }
    Ok(())
}

fn verify_schema(connection: &Connection) -> Result<(), SidecarError> {
    let row: (u32, u32, String) = connection
        .query_row(
            "SELECT schema_version, protocol_version, migration_state FROM schema_meta WHERE singleton = 1;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(sqlite_error)?;
    if row != (SQLITE_SCHEMA_VERSION, PROTOCOL_VERSION, "ready".to_string())
        || SQLITE_SCHEMA_VERSION != APP_SCHEMA_VERSION
    {
        return Err(SidecarError::new(
            "schemaMismatch",
            "SQLite schema/protocol version is not the supported v1 contract",
        ));
    }
    let count: u32 = connection
        .query_row("SELECT count(*) FROM schema_meta;", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if count != 1 {
        return Err(SidecarError::new(
            "schemaMismatch",
            "schema_meta must contain exactly one row",
        ));
    }
    let append_only_triggers: u32 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name IN ('events_are_append_only_update', 'events_are_append_only_delete');",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if append_only_triggers != 2 {
        return Err(SidecarError::new(
            "schemaMismatch",
            "append-only ledger triggers are missing",
        ));
    }
    Ok(())
}

fn verify_schema_v2(connection: &Connection) -> Result<(), SidecarError> {
    let metadata: (u32, u32, String, String, String) = connection
        .query_row(
            "SELECT storage_schema_version, protocol_version, migration_state, materialization_state, migrated_at FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v2 metadata is missing or unreadable",
            )
        })?;
    let metadata_count: u32 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let migrated_at_valid: Option<bool> = connection
        .query_row(
            "SELECT migrated_at = strftime('%Y-%m-%d %H:%M:%S', migrated_at) FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v2 migration timestamp is unreadable",
            )
        })?;
    if metadata.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V1
        || metadata.1 != PROTOCOL_VERSION
        || metadata.2 != "ready"
        || !matches!(metadata.3.as_str(), "pending" | "ready")
        || metadata_count != 1
        || migrated_at_valid != Some(true)
    {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "storage schema v2 metadata does not match the supported additive migration",
        ));
    }

    let expected = reference_schema_v2()?;
    let actual = schema_objects(connection)?;
    if actual != expected {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema does not exactly match the storage v2 golden schema",
        ));
    }
    Ok(())
}

fn verify_schema_v3(connection: &Connection) -> Result<(), SidecarError> {
    let metadata: (u32, u32, String, String, String) = connection
        .query_row(
            "SELECT storage_schema_version, protocol_version, migration_state, materialization_state, migrated_at FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v3 metadata is missing or unreadable",
            )
        })?;
    let metadata_count: u32 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let migrated_at_valid: Option<bool> = connection
        .query_row(
            "SELECT migrated_at = strftime('%Y-%m-%d %H:%M:%S', migrated_at) FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v3 migration timestamp is unreadable",
            )
        })?;
    if metadata.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V3
        || metadata.1 != PROTOCOL_VERSION
        || metadata.2 != "ready"
        || metadata_count != 1
        || migrated_at_valid != Some(true)
    {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "storage schema v3 metadata does not match the supported migration",
        ));
    }
    if schema_objects(connection)? != reference_schema_v3()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema does not exactly match the storage v3 golden schema",
        ));
    }
    verify_v3_materialization_snapshot(connection, &metadata.3)?;
    Ok(())
}

fn verify_v3_materialization_snapshot(
    connection: &Connection,
    materialization_state: &str,
) -> Result<(), SidecarError> {
    let job_count: u32 = connection
        .query_row(
            "SELECT count(*) FROM lifetime_materialization_job;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_materialization_snapshot_error())?;
    let row: Option<LifetimeJobRow> = connection
        .query_row(
            "SELECT singleton,job_id,state,cutoff_event_rowid,phase,after_event_rowid,\
                    failure_kind,created_at,updated_at,completed_at \
             FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .optional()
        .map_err(|_| storage_materialization_snapshot_error())?;
    if job_count != u32::from(row.is_some()) {
        return Err(storage_materialization_snapshot_error());
    }
    let job = row.map(
        |(
            singleton,
            job_id,
            state,
            cutoff_event_rowid,
            phase,
            after_event_rowid,
            failure_kind,
            created_at,
            updated_at,
            completed_at,
        )| {
            json!({
                "singleton": singleton,
                "jobId": job_id,
                "state": state,
                "cutoffEventRowid": cutoff_event_rowid,
                "phase": phase,
                "afterEventRowid": after_event_rowid,
                "failureKind": failure_kind,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "completedAt": completed_at,
            })
        },
    );
    if let Some(job) = job.as_ref() {
        validate_storage_lifetime_materialization_job_v1(job)
            .map_err(|_| storage_materialization_snapshot_error())?;
    }
    validate_storage_lifetime_materialization_pair_v1(&json!({
        "materializationState": materialization_state,
        "job": job,
    }))
    .map_err(|_| storage_materialization_snapshot_error())?;
    Ok(())
}

#[cfg(test)]
fn verify_schema_v4_for_open(connection: &Connection) -> Result<(), SidecarError> {
    let materialization_state = verify_schema_v4(connection)?;
    verify_v4_materialization_snapshot(connection, &materialization_state, true)
}

fn verify_schema_v4(connection: &Connection) -> Result<String, SidecarError> {
    let metadata: (u32, u32, String, String, String) = connection
        .query_row(
            "SELECT storage_schema_version, protocol_version, migration_state, materialization_state, migrated_at FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v4 metadata is missing or unreadable",
            )
        })?;
    let metadata_count: u32 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let migrated_at_valid: Option<bool> = connection
        .query_row(
            "SELECT migrated_at = strftime('%Y-%m-%d %H:%M:%S', migrated_at) FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v4 migration timestamp is unreadable",
            )
        })?;
    if metadata.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V4
        || metadata.1 != PROTOCOL_VERSION
        || metadata.2 != "ready"
        || metadata_count != 1
        || migrated_at_valid != Some(true)
    {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "storage schema v4 metadata does not match the supported migration",
        ));
    }
    if schema_objects(connection)? != reference_schema_v4()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema does not exactly match the storage v4 golden schema",
        ));
    }
    verify_v4_materialization_snapshot(connection, &metadata.3, false)?;
    Ok(metadata.3)
}

fn verify_v4_materialization_snapshot(
    connection: &Connection,
    materialization_state: &str,
    reopen: bool,
) -> Result<(), SidecarError> {
    let job_count: u32 = connection
        .query_row(
            "SELECT count(*) FROM lifetime_materialization_job;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_materialization_snapshot_error())?;
    let row: Option<LifetimeJobRowV4> = connection
        .query_row(
            "SELECT singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,\
                    scan_exhausted,failure_kind,created_at,updated_at,completed_at \
             FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()
        .map_err(|_| storage_materialization_snapshot_error())?;
    if job_count != u32::from(row.is_some()) {
        return Err(storage_materialization_snapshot_error());
    }
    if row.as_ref().is_some_and(|row| !matches!(row.6, 0 | 1)) {
        return Err(storage_materialization_snapshot_error());
    }
    let job = row.map(
        |(
            singleton,
            job_id,
            state,
            cutoff_event_rowid,
            phase,
            after_scan_rowid,
            scan_exhausted,
            failure_kind,
            created_at,
            updated_at,
            completed_at,
        )| {
            json!({
                "singleton": singleton,
                "jobId": job_id,
                "state": state,
                "cutoffEventRowid": cutoff_event_rowid,
                "phase": phase,
                "afterScanRowid": after_scan_rowid,
                "scanExhausted": scan_exhausted == 1,
                "failureKind": failure_kind,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "completedAt": completed_at,
            })
        },
    );
    if let Some(job) = job.as_ref() {
        validate_storage_lifetime_materialization_job_v2(job)
            .map_err(|_| storage_materialization_snapshot_error())?;
    }
    let pair = json!({
        "materializationState": materialization_state,
        "job": job,
    });
    validate_storage_lifetime_materialization_pair_v2(&pair)
        .map_err(|_| storage_materialization_snapshot_error())?;
    if reopen {
        validate_storage_lifetime_reopen_pair_v2(&pair)
            .map_err(|_| storage_materialization_snapshot_error())?;
    }
    Ok(())
}

#[cfg(test)]
fn verify_schema_v5_for_open(connection: &Connection) -> Result<(), SidecarError> {
    let materialization_state = verify_schema_v5(connection)?;
    verify_v5_materialization_snapshot(connection, &materialization_state, true)
}

fn verify_schema_v5(connection: &Connection) -> Result<String, SidecarError> {
    let metadata: (u32, u32, String, String, String) = connection
        .query_row(
            "SELECT storage_schema_version, protocol_version, migration_state, materialization_state, migrated_at FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v5 metadata is missing or unreadable",
            )
        })?;
    let metadata_count: u32 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let migrated_at_valid: Option<bool> = connection
        .query_row(
            "SELECT migrated_at = strftime('%Y-%m-%d %H:%M:%S', migrated_at) FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v5 migration timestamp is unreadable",
            )
        })?;
    if metadata.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V5
        || metadata.1 != PROTOCOL_VERSION
        || metadata.2 != "ready"
        || metadata_count != 1
        || migrated_at_valid != Some(true)
    {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "storage schema v5 metadata does not match the supported migration",
        ));
    }
    if schema_objects(connection)? != reference_schema_v5()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema does not exactly match the storage v5 golden schema",
        ));
    }
    verify_v5_materialization_snapshot(connection, &metadata.3, false)?;
    Ok(metadata.3)
}

fn verify_v5_materialization_snapshot(
    connection: &Connection,
    materialization_state: &str,
    reopen: bool,
) -> Result<(), SidecarError> {
    let job_count: u32 = connection
        .query_row(
            "SELECT count(*) FROM lifetime_materialization_job;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_materialization_snapshot_error())?;
    let row: Option<LifetimeJobRowV4> = connection
        .query_row(
            "SELECT singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,\
                    scan_exhausted,failure_kind,created_at,updated_at,completed_at \
             FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()
        .map_err(|_| storage_materialization_snapshot_error())?;
    if job_count != u32::from(row.is_some()) {
        return Err(storage_materialization_snapshot_error());
    }
    if row.as_ref().is_some_and(|row| !matches!(row.6, 0 | 1)) {
        return Err(storage_materialization_snapshot_error());
    }
    let job = row.map(
        |(
            singleton,
            job_id,
            state,
            cutoff_event_rowid,
            phase,
            after_scan_rowid,
            scan_exhausted,
            failure_kind,
            created_at,
            updated_at,
            completed_at,
        )| {
            json!({
                "singleton": singleton,
                "jobId": job_id,
                "state": state,
                "cutoffEventRowid": cutoff_event_rowid,
                "phase": phase,
                "afterScanRowid": after_scan_rowid,
                "scanExhausted": scan_exhausted == 1,
                "failureKind": failure_kind,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "completedAt": completed_at,
            })
        },
    );
    if let Some(job) = job.as_ref() {
        validate_storage_lifetime_materialization_job_v3(job)
            .map_err(|_| storage_materialization_snapshot_error())?;
    }
    let pair = json!({
        "materializationState": materialization_state,
        "job": job,
    });
    validate_storage_lifetime_materialization_pair_v3(&pair)
        .map_err(|_| storage_materialization_snapshot_error())?;
    if reopen {
        validate_storage_lifetime_reopen_pair_v3(&pair)
            .map_err(|_| storage_materialization_snapshot_error())?;
    }
    Ok(())
}

#[cfg(test)]
fn verify_schema_v6_for_open(connection: &Connection) -> Result<(), SidecarError> {
    let materialization_state = verify_schema_v6(connection)?;
    verify_v6_materialization_snapshot(connection, &materialization_state, true)
}

fn verify_schema_v6(connection: &Connection) -> Result<String, SidecarError> {
    let metadata: (u32, u32, String, String, String) = connection
        .query_row(
            "SELECT storage_schema_version, protocol_version, migration_state, materialization_state, migrated_at FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v6 metadata is missing or unreadable",
            )
        })?;
    let metadata_count: u32 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let migrated_at_valid: Option<bool> = connection
        .query_row(
            "SELECT migrated_at = strftime('%Y-%m-%d %H:%M:%S', migrated_at) FROM storage_schema_meta WHERE singleton = 1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v6 migration timestamp is unreadable",
            )
        })?;
    let foreign_keys: u32 = connection
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if metadata.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V6
        || metadata.1 != PROTOCOL_VERSION
        || metadata.2 != "ready"
        || metadata_count != 1
        || migrated_at_valid != Some(true)
        || foreign_keys != 1
    {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "storage schema v6 metadata or foreign-key enforcement does not match the supported migration",
        ));
    }
    if schema_objects(connection)? != reference_schema_v6()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema does not exactly match the storage v6 golden schema",
        ));
    }
    verify_v6_materialization_snapshot(connection, &metadata.3, false)?;
    Ok(metadata.3)
}

fn verify_v6_materialization_snapshot(
    connection: &Connection,
    materialization_state: &str,
    reopen: bool,
) -> Result<(), SidecarError> {
    let job_count: u32 = connection
        .query_row(
            "SELECT count(*) FROM lifetime_materialization_job;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_materialization_snapshot_error())?;
    let row: Option<LifetimeJobRowV4> = connection
        .query_row(
            "SELECT singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,\
                    scan_exhausted,failure_kind,created_at,updated_at,completed_at \
             FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()
        .map_err(|_| storage_materialization_snapshot_error())?;
    if job_count != u32::from(row.is_some()) {
        return Err(storage_materialization_snapshot_error());
    }
    if row.as_ref().is_some_and(|row| !matches!(row.6, 0 | 1)) {
        return Err(storage_materialization_snapshot_error());
    }
    let job = row.map(
        |(
            singleton,
            job_id,
            state,
            cutoff_event_rowid,
            phase,
            after_scan_rowid,
            scan_exhausted,
            failure_kind,
            created_at,
            updated_at,
            completed_at,
        )| {
            json!({
                "singleton": singleton,
                "jobId": job_id,
                "state": state,
                "cutoffEventRowid": cutoff_event_rowid,
                "phase": phase,
                "afterScanRowid": after_scan_rowid,
                "scanExhausted": scan_exhausted == 1,
                "failureKind": failure_kind,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "completedAt": completed_at,
            })
        },
    );
    if let Some(job) = job.as_ref() {
        validate_storage_lifetime_materialization_job_v4(job)
            .map_err(|_| storage_materialization_snapshot_error())?;
    }
    let pair = json!({
        "materializationState": materialization_state,
        "job": job,
    });
    validate_storage_lifetime_materialization_pair_v4(&pair)
        .map_err(|_| storage_materialization_snapshot_error())?;
    if reopen {
        validate_storage_lifetime_reopen_pair_v4(&pair)
            .map_err(|_| storage_materialization_snapshot_error())?;
    }
    Ok(())
}

#[cfg(test)]
fn verify_schema_v7_for_open(connection: &Connection) -> Result<(), SidecarError> {
    let materialization_state = verify_schema_v7(connection)?;
    verify_v6_materialization_snapshot(connection, &materialization_state, true)?;
    verify_v7_uncompacted_runtime_state(connection)
}

fn verify_schema_v7(connection: &Connection) -> Result<String, SidecarError> {
    let metadata: (u32, u32, String, String, String) = connection
        .query_row(
            "SELECT storage_schema_version,protocol_version,migration_state,materialization_state,migrated_at \
             FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v7 metadata is missing or unreadable",
            )
        })?;
    let metadata_count: u32 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let migrated_at_valid: Option<bool> = connection
        .query_row(
            "SELECT migrated_at = strftime('%Y-%m-%d %H:%M:%S', migrated_at) \
             FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v7 migration timestamp is unreadable",
            )
        })?;
    let foreign_keys: u32 = connection
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if metadata.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V7
        || metadata.1 != PROTOCOL_VERSION
        || metadata.2 != "ready"
        || metadata_count != 1
        || migrated_at_valid != Some(true)
        || foreign_keys != 1
    {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "storage schema v7 metadata or foreign-key enforcement does not match the supported migration",
        ));
    }
    if schema_objects(connection)? != reference_schema_v7()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema does not exactly match the storage v7 golden schema",
        ));
    }
    verify_v6_materialization_snapshot(connection, &metadata.3, false)?;
    Ok(metadata.3)
}

pub(crate) fn verify_v7_uncompacted_runtime_state(
    connection: &Connection,
) -> Result<(), SidecarError> {
    let state: (u32, u32, u32, u32) = connection
        .query_row(
            "SELECT \
               (SELECT count(*) FROM ledger_compaction_prefixes),\
               (SELECT count(*) FROM frozen_message_sources),\
               (SELECT count(*) FROM ledger_compaction_delete_authority),\
               (SELECT count(*) FROM maintenance_jobs \
                WHERE job_kind='losslessCompaction' AND state IN ('pending','running'));",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| storage_compaction_state_error())?;
    if state != (0, 0, 0, 0) {
        return Err(storage_compaction_state_error());
    }
    Ok(())
}

fn verify_schema_v8_for_open(connection: &Connection) -> Result<(), SidecarError> {
    let materialization_state = verify_schema_v8(connection)?;
    verify_v6_materialization_snapshot(connection, &materialization_state, true)?;
    verify_v8_uncompacted_runtime_state(connection)
}

fn verify_schema_v8(connection: &Connection) -> Result<String, SidecarError> {
    let metadata: (u32, u32, String, String, String) = connection
        .query_row(
            "SELECT storage_schema_version,protocol_version,migration_state,materialization_state,migrated_at \
             FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v8 metadata is missing or unreadable",
            )
        })?;
    let metadata_count: u32 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let migrated_at_valid: Option<bool> = connection
        .query_row(
            "SELECT migrated_at = strftime('%Y-%m-%d %H:%M:%S', migrated_at) \
             FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            SidecarError::new(
                "storageSchemaMismatch",
                "storage schema v8 migration timestamp is unreadable",
            )
        })?;
    let foreign_keys: u32 = connection
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if metadata.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V8
        || metadata.1 != PROTOCOL_VERSION
        || metadata.2 != "ready"
        || metadata_count != 1
        || migrated_at_valid != Some(true)
        || foreign_keys != 1
    {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "storage schema v8 metadata or foreign-key enforcement does not match the supported migration",
        ));
    }
    if schema_objects(connection)? != reference_schema_v8()? {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "SQLite schema does not exactly match the storage v8 golden schema",
        ));
    }
    verify_v6_materialization_snapshot(connection, &metadata.3, false)?;
    Ok(metadata.3)
}

pub(crate) fn verify_v8_uncompacted_runtime_state(
    connection: &Connection,
) -> Result<(), SidecarError> {
    let state: (u32, u32, u32, u32, u32) = connection
        .query_row(
            "SELECT \
               (SELECT count(*) FROM ledger_compaction_prefixes),\
               (SELECT count(*) FROM frozen_message_sources),\
               (SELECT count(*) FROM ledger_compaction_delete_authority),\
               (SELECT count(*) FROM frozen_operation_sources),\
               (SELECT count(*) FROM maintenance_jobs \
                WHERE job_kind='losslessCompaction' AND state IN ('pending','running'));",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| storage_v8_compaction_state_error())?;
    if state.3 > 0 {
        let operation_id: String = connection
            .query_row(
                "SELECT operation_id FROM frozen_operation_sources ORDER BY source_event_rowid LIMIT 1;",
                [],
                |row| row.get(0),
            )
            .map_err(|_| storage_frozen_operation_error())?;
        validate_v8_frozen_operation_source(connection, &operation_id)?;
    }
    if state != (0, 0, 0, 0, 0) {
        return Err(storage_v8_compaction_state_error());
    }
    Ok(())
}

pub(crate) fn validate_v8_frozen_operation_source(
    connection: &Connection,
    operation_id: &str,
) -> Result<(), SidecarError> {
    let payload_bytes: i64 = connection
        .query_row(
            "SELECT length(CAST(canonical_payload_json AS BLOB)) \
             FROM frozen_operation_sources WHERE operation_id=?1;",
            [operation_id],
            |row| row.get(0),
        )
        .map_err(|_| storage_frozen_operation_error())?;
    if !(0..=1_048_576).contains(&payload_bytes) {
        return Err(storage_frozen_operation_error());
    }
    let frozen = connection
        .query_row(
            "SELECT operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,\
                    aggregate_id,aggregate_sequence,event_type,payload_schema_version,\
                    canonical_payload_json,canonical_payload_digest,event_timestamp,\
                    canonical_record_json,record_digest,frozen_at \
             FROM frozen_operation_sources WHERE operation_id=?1;",
            [operation_id],
            |row| {
                Ok(json!({
                    "operationId": row.get::<_, String>(0)?,
                    "sourceEventRowid": row.get::<_, i64>(1)?,
                    "sourceEventId": row.get::<_, String>(2)?,
                    "requestId": row.get::<_, Option<String>>(3)?,
                    "aggregateType": row.get::<_, String>(4)?,
                    "aggregateId": row.get::<_, String>(5)?,
                    "aggregateSequence": row.get::<_, i64>(6)?,
                    "eventType": row.get::<_, String>(7)?,
                    "payloadSchemaVersion": row.get::<_, u32>(8)?,
                    "canonicalPayloadJson": row.get::<_, String>(9)?,
                    "canonicalPayloadDigest": row.get::<_, String>(10)?,
                    "eventTimestamp": row.get::<_, String>(11)?,
                    "canonicalRecordJson": row.get::<_, String>(12)?,
                    "recordDigest": row.get::<_, String>(13)?,
                    "frozenAt": row.get::<_, String>(14)?,
                }))
            },
        )
        .map_err(|_| storage_frozen_operation_error())?;
    let source_event_rowid = frozen
        .get("sourceEventRowid")
        .and_then(Value::as_u64)
        .ok_or_else(storage_frozen_operation_error)?;
    let event = connection
        .query_row(
            "SELECT event_id,operation_id,request_id,aggregate_type,aggregate_id,\
                    aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp \
             FROM events WHERE rowid=?1;",
            [sql_i64(source_event_rowid)?],
            |row| {
                Ok(LedgerEventV1 {
                    event_id: row.get(0)?,
                    operation_id: row.get(1)?,
                    request_id: row.get(2)?,
                    aggregate_type: row.get(3)?,
                    aggregate_id: row.get(4)?,
                    aggregate_sequence: row_u64(row, 5)?,
                    event_type: row.get(6)?,
                    payload_schema_version: row.get(7)?,
                    canonical_json_payload: row.get(8)?,
                    timestamp: row.get(9)?,
                })
            },
        )
        .map_err(|_| storage_frozen_operation_error())?;
    let source_index = connection
        .query_row(
            "SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,\
                    aggregate_sequence,introduced_message_id,chat_durable_sequence,\
                    chat_snapshot_message_id \
             FROM event_source_index WHERE event_rowid=?1;",
            [sql_i64(source_event_rowid)?],
            |row| {
                Ok(json!({
                    "eventRowid": row.get::<_, i64>(0)?,
                    "eventId": row.get::<_, String>(1)?,
                    "operationId": row.get::<_, String>(2)?,
                    "aggregateType": row.get::<_, String>(3)?,
                    "aggregateId": row.get::<_, String>(4)?,
                    "aggregateSequence": row.get::<_, i64>(5)?,
                    "introducedMessageId": row.get::<_, Option<String>>(6)?,
                    "chatDurableSequence": row.get::<_, Option<i64>>(7)?,
                    "chatSnapshotMessageId": row.get::<_, Option<String>>(8)?,
                }))
            },
        )
        .map_err(|_| storage_frozen_operation_error())?;
    let receipt = connection
        .query_row(
            "SELECT operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,\
                    payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,\
                    canonical_payload_digest_version,canonical_payload_digest_format,\
                    canonical_payload_digest,event_timestamp \
             FROM operation_receipts WHERE operation_id=?1;",
            [operation_id],
            |row| {
                Ok(json!({
                    "operationId": row.get::<_, String>(0)?,
                    "eventId": row.get::<_, String>(1)?,
                    "aggregateType": row.get::<_, String>(2)?,
                    "aggregateId": row.get::<_, String>(3)?,
                    "aggregateSequence": row.get::<_, i64>(4)?,
                    "eventType": row.get::<_, String>(5)?,
                    "payloadSchemaVersion": row.get::<_, u32>(6)?,
                    "canonicalPayloadJson": row.get::<_, String>(7)?,
                    "canonicalPayloadDigestAlgorithm": row.get::<_, String>(8)?,
                    "canonicalPayloadDigestVersion": row.get::<_, u32>(9)?,
                    "canonicalPayloadDigestFormat": row.get::<_, String>(10)?,
                    "canonicalPayloadDigest": row.get::<_, String>(11)?,
                    "eventTimestamp": row.get::<_, String>(12)?,
                }))
            },
        )
        .map_err(|_| storage_frozen_operation_error())?;
    validate_frozen_operation_source_binding_v1(
        &frozen,
        source_event_rowid,
        &event,
        &source_index,
        &receipt,
    )
    .map_err(|_| storage_frozen_operation_error())?;
    Ok(())
}

fn storage_materialization_snapshot_error() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "storage lifetime materialization state is invalid",
    )
}

fn storage_compaction_state_error() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "storage compaction state is not runnable before the C1B consumer",
    )
}

fn storage_v8_compaction_state_error() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "storage v8 compaction state is not runnable before the next reviewed consumer",
    )
}

fn storage_frozen_operation_error() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "storage frozen operation evidence is invalid",
    )
}

fn reference_schema_v2() -> Result<Vec<SchemaObject>, SidecarError> {
    let connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V2)
        .map_err(sqlite_error)?;
    schema_objects(&connection)
}

fn reference_schema_v3() -> Result<Vec<SchemaObject>, SidecarError> {
    let mut connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V2)
        .map_err(sqlite_error)?;
    execute_migration(&mut connection, MIGRATION_V3, "reference v3")?;
    schema_objects(&connection)
}

fn reference_schema_v4() -> Result<Vec<SchemaObject>, SidecarError> {
    let mut connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V2)
        .map_err(sqlite_error)?;
    execute_migration(&mut connection, MIGRATION_V3, "reference v3")?;
    execute_migration(&mut connection, MIGRATION_V4, "reference v4")?;
    schema_objects(&connection)
}

fn reference_schema_v5() -> Result<Vec<SchemaObject>, SidecarError> {
    let mut connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V2)
        .map_err(sqlite_error)?;
    execute_migration(&mut connection, MIGRATION_V3, "reference v3")?;
    execute_migration(&mut connection, MIGRATION_V4, "reference v4")?;
    execute_migration(&mut connection, MIGRATION_V5, "reference v5")?;
    schema_objects(&connection)
}

fn reference_schema_v6() -> Result<Vec<SchemaObject>, SidecarError> {
    let mut connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V2)
        .map_err(sqlite_error)?;
    execute_migration(&mut connection, MIGRATION_V3, "reference v3")?;
    execute_migration(&mut connection, MIGRATION_V4, "reference v4")?;
    execute_migration(&mut connection, MIGRATION_V5, "reference v5")?;
    execute_migration(&mut connection, MIGRATION_V6, "reference v6")?;
    schema_objects(&connection)
}

fn reference_schema_v7() -> Result<Vec<SchemaObject>, SidecarError> {
    let mut connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V2)
        .map_err(sqlite_error)?;
    execute_migration(&mut connection, MIGRATION_V3, "reference v3")?;
    execute_migration(&mut connection, MIGRATION_V4, "reference v4")?;
    execute_migration(&mut connection, MIGRATION_V5, "reference v5")?;
    execute_migration(&mut connection, MIGRATION_V6, "reference v6")?;
    execute_migration(&mut connection, MIGRATION_V7, "reference v7")?;
    schema_objects(&connection)
}

fn reference_schema_v8() -> Result<Vec<SchemaObject>, SidecarError> {
    let mut connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V2)
        .map_err(sqlite_error)?;
    execute_migration(&mut connection, MIGRATION_V3, "reference v3")?;
    execute_migration(&mut connection, MIGRATION_V4, "reference v4")?;
    execute_migration(&mut connection, MIGRATION_V5, "reference v5")?;
    execute_migration(&mut connection, MIGRATION_V6, "reference v6")?;
    execute_migration(&mut connection, MIGRATION_V7, "reference v7")?;
    execute_migration(&mut connection, MIGRATION_V8, "reference v8")?;
    schema_objects(&connection)
}

fn reference_schema_v1() -> Result<Vec<SchemaObject>, SidecarError> {
    let connection = Connection::open_in_memory().map_err(sqlite_error)?;
    connection
        .execute_batch(MIGRATION_V1)
        .map_err(sqlite_error)?;
    schema_objects(&connection)
}

fn schema_objects(connection: &Connection) -> Result<Vec<SchemaObject>, SidecarError> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name;",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(sqlite_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)
}

fn verify_integrity(connection: &Connection) -> Result<(), SidecarError> {
    let integrity: String = connection
        .query_row("PRAGMA integrity_check;", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if integrity != "ok" {
        return Err(SidecarError::new(
            "integrityFailed",
            format!("SQLite integrity_check failed: {integrity}"),
        ));
    }
    let foreign_key_failure: Option<String> = connection
        .query_row("PRAGMA foreign_key_check;", [], |row| row.get(0))
        .optional()
        .map_err(sqlite_error)?;
    if let Some(table) = foreign_key_failure {
        return Err(SidecarError::new(
            "integrityFailed",
            format!("SQLite foreign_key_check failed in {table}"),
        ));
    }
    Ok(())
}

pub(crate) fn verify_projection_replay(connection: &Connection) -> Result<(), SidecarError> {
    let events = load_events(connection)?;
    let replayed = replay_ledger_v1(&events).map_err(storage_contract_error)?;
    let stored = load_projection(connection)?;
    verify_projection_columns(connection)?;
    if replayed != stored {
        return Err(SidecarError::new(
            "projectionMismatch",
            "stored projection does not match deterministic ledger replay",
        ));
    }
    Ok(())
}

fn verify_projection_columns(connection: &Connection) -> Result<(), SidecarError> {
    let mismatch: bool = connection
        .query_row(
            "SELECT EXISTS(\
               SELECT 1 FROM projects WHERE \
                 json_extract(projection_json, '$.project_id') IS NOT project_id OR \
                 json_extract(projection_json, '$.canonical_path') IS NOT canonical_path \
               UNION ALL SELECT 1 FROM chats WHERE \
                 json_extract(projection_json, '$.chat_id') IS NOT chat_id OR \
                 json_extract(projection_json, '$.project_id') IS NOT project_id \
               UNION ALL SELECT 1 FROM runs WHERE \
                 json_extract(projection_json, '$.run_id') IS NOT run_id OR \
                 json_extract(projection_json, '$.chat_id') IS NOT chat_id OR \
                 json_extract(projection_json, '$.state') IS NOT state OR \
                 json_extract(projection_json, '$.worktree_path') IS NOT worktree_path \
               UNION ALL SELECT 1 FROM locks WHERE \
                 json_extract(projection_json, '$.lock_id') IS NOT lock_id OR \
                 json_extract(projection_json, '$.project_id') IS NOT project_id OR \
                 json_extract(projection_json, '$.writer_run_id') IS NOT writer_run_id OR \
                 json_extract(projection_json, '$.worktree_path') IS NOT worktree_path OR \
                 json_extract(projection_json, '$.poisoned') IS NOT poisoned OR \
                 json_extract(projection_json, '$.recovery_required') IS NOT recovery_required OR \
                 json(json_extract(projection_json, '$.evidence')) IS NOT json(evidence_json)\
             );",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if mismatch {
        return Err(SidecarError::new(
            "projectionMismatch",
            "projection JSON and indexed projection columns disagree",
        ));
    }
    Ok(())
}

pub(crate) fn load_events(connection: &Connection) -> Result<Vec<LedgerEventV1>, SidecarError> {
    let mut statement = connection
        .prepare(
            "SELECT event_id, operation_id, request_id, aggregate_type, aggregate_id, \
             aggregate_sequence, event_type, payload_schema_version, canonical_json_payload, timestamp \
             FROM events ORDER BY rowid;",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(LedgerEventV1 {
                event_id: row.get(0)?,
                operation_id: row.get(1)?,
                request_id: row.get(2)?,
                aggregate_type: row.get(3)?,
                aggregate_id: row.get(4)?,
                aggregate_sequence: row_u64(row, 5)?,
                event_type: row.get(6)?,
                payload_schema_version: row.get(7)?,
                canonical_json_payload: row.get(8)?,
                timestamp: row.get(9)?,
            })
        })
        .map_err(sqlite_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)
}

pub(crate) fn load_projection(connection: &Connection) -> Result<ProjectionStateV1, SidecarError> {
    let mut projection = ProjectionStateV1::default();
    let mut statement = connection
        .prepare(
            "SELECT sidecar_instance_id, pid, platform, started_at, heartbeat_at, clean_shutdown_at, last_event_sequence FROM app_instances;",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row_u64(row, 6)?,
            ))
        })
        .map_err(sqlite_error)?;
    for row in rows {
        let (id, pid, platform, started, heartbeat, clean, sequence) = row.map_err(sqlite_error)?;
        projection.app_instances.insert(
            id.clone(),
            json!({
                "sidecar_instance_id": id,
                "pid": pid,
                "platform": platform,
                "started_at": started,
                "heartbeat_at": heartbeat,
                "clean_shutdown_at": clean,
                "last_event_sequence": sequence,
            })
            .as_object()
            .expect("object literal")
            .clone(),
        );
    }
    drop(statement);

    load_json_projection(
        connection,
        "projects",
        "project_id",
        &mut projection.projects,
    )?;
    load_json_projection(connection, "chats", "chat_id", &mut projection.chats)?;
    load_json_projection(connection, "runs", "run_id", &mut projection.runs)?;
    load_json_projection(connection, "locks", "lock_id", &mut projection.locks)?;
    Ok(projection)
}

fn load_json_projection(
    connection: &Connection,
    table: &str,
    id_column: &str,
    target: &mut std::collections::BTreeMap<String, Map<String, Value>>,
) -> Result<(), SidecarError> {
    let sql = format!("SELECT {id_column}, projection_json, last_event_sequence FROM {table};");
    let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row_u64(row, 2)?,
            ))
        })
        .map_err(sqlite_error)?;
    for row in rows {
        let (id, source, sequence) = row.map_err(sqlite_error)?;
        let mut value: Map<String, Value> = serde_json::from_str(&source).map_err(|error| {
            SidecarError::new(
                "projectionInvalid",
                format!("{table}.{id} projection_json is invalid: {error}"),
            )
        })?;
        value.insert("last_event_sequence".to_string(), Value::from(sequence));
        target.insert(id, value);
    }
    Ok(())
}

fn insert_event(transaction: &Transaction<'_>, event: &LedgerEventV1) -> Result<(), SidecarError> {
    transaction
        .execute(
            "INSERT INTO events(event_id, operation_id, request_id, aggregate_type, aggregate_id, aggregate_sequence, event_type, payload_schema_version, canonical_json_payload, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
            params![
                event.event_id,
                event.operation_id,
                event.request_id,
                event.aggregate_type,
                event.aggregate_id,
                sql_i64(event.aggregate_sequence)?,
                event.event_type,
                event.payload_schema_version,
                event.canonical_json_payload,
                event.timestamp,
            ],
        )
        .map_err(sqlite_error)?;
    match storage_schema_version(transaction)? {
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V6) => {
            insert_v6_source_index_and_invalidate_ready(transaction, event)?;
        }
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V7) => {
            verify_v7_uncompacted_runtime_state(transaction)?;
            insert_v6_source_index_and_invalidate_ready(transaction, event)?;
        }
        Some(STORAGE_LIFETIME_SCHEMA_VERSION_V8) => {
            verify_v8_uncompacted_runtime_state(transaction)?;
            insert_v6_source_index_and_invalidate_ready(transaction, event)?;
        }
        None
        | Some(STORAGE_LIFETIME_SCHEMA_VERSION_V1)
        | Some(STORAGE_LIFETIME_SCHEMA_VERSION_V3)
        | Some(STORAGE_LIFETIME_SCHEMA_VERSION_V4)
        | Some(STORAGE_LIFETIME_SCHEMA_VERSION_V5) => {}
        Some(_) => {
            return Err(SidecarError::new(
                "storageSchemaMismatch",
                "event append encountered an unsupported storage schema version",
            ));
        }
    }
    Ok(())
}

fn insert_v6_source_index_and_invalidate_ready(
    transaction: &Transaction<'_>,
    event: &LedgerEventV1,
) -> Result<(), SidecarError> {
    let event_rowid = u64::try_from(transaction.last_insert_rowid()).map_err(|_| {
        SidecarError::new(
            "storageSchemaMismatch",
            "inserted event rowid is outside the supported unsigned range",
        )
    })?;
    if event_rowid == 0 {
        return Err(SidecarError::new(
            "storageSchemaMismatch",
            "inserted event rowid must be positive",
        ));
    }

    let (chat_durable_sequence, chat_snapshot_message_id) = chat_source_state(event)?;
    let (predecessor_entry, predecessor_source, previous_durable) =
        if event.aggregate_type != "chat" || event.aggregate_sequence == 1 {
            (None, None, 0)
        } else {
            let predecessor =
                load_exact_source_index_predecessor(transaction, event)?.ok_or_else(|| {
                    SidecarError::new(
                        "storageContractViolation",
                        "chat append requires the exact indexed predecessor",
                    )
                })?;
            let predecessor_rowid = predecessor
                .get("eventRowid")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    SidecarError::new(
                        "storageSchemaMismatch",
                        "indexed predecessor rowid is unreadable",
                    )
                })?;
            let predecessor_durable = predecessor
                .get("chatDurableSequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    SidecarError::new(
                        "storageSchemaMismatch",
                        "indexed chat predecessor durable sequence is unreadable",
                    )
                })?;
            let source =
                load_exact_event_by_rowid(transaction, predecessor_rowid)?.ok_or_else(|| {
                    SidecarError::new(
                        "storageSchemaMismatch",
                        "indexed predecessor source event is missing",
                    )
                })?;
            (Some(predecessor), Some(source), predecessor_durable)
        };
    let introduced_message_id = chat_durable_sequence
        .filter(|current| previous_durable.checked_add(1) == Some(*current))
        .and_then(|_| chat_snapshot_message_id.clone());
    let entry = json!({
        "eventRowid": event_rowid,
        "eventId": event.event_id,
        "operationId": event.operation_id,
        "aggregateType": event.aggregate_type,
        "aggregateId": event.aggregate_id,
        "aggregateSequence": event.aggregate_sequence,
        "introducedMessageId": introduced_message_id,
        "chatDurableSequence": chat_durable_sequence,
        "chatSnapshotMessageId": chat_snapshot_message_id,
    });
    validate_storage_event_source_index_transition_v1(
        event_rowid,
        &entry,
        event,
        predecessor_entry.as_ref(),
        predecessor_source.as_ref(),
    )
    .map_err(storage_contract_error)?;
    transaction
        .execute(
            "INSERT INTO event_source_index(\
               event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
               introduced_message_id,chat_durable_sequence,chat_snapshot_message_id\
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9);",
            params![
                sql_i64(event_rowid)?,
                event.event_id,
                event.operation_id,
                event.aggregate_type,
                event.aggregate_id,
                sql_i64(event.aggregate_sequence)?,
                introduced_message_id,
                chat_durable_sequence.map(sql_i64).transpose()?,
                chat_snapshot_message_id,
            ],
        )
        .map_err(sqlite_error)?;

    let materialization_state: String = transaction
        .query_row(
            "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage_materialization_snapshot_error())?;
    verify_v6_materialization_snapshot(transaction, &materialization_state, false)?;
    if materialization_state == "ready" {
        let changed = transaction
            .execute(
                "UPDATE storage_schema_meta SET materialization_state='pending' \
                 WHERE singleton=1 AND materialization_state='ready' \
                   AND NOT EXISTS(SELECT 1 FROM lifetime_materialization_job);",
                [],
            )
            .map_err(sqlite_error)?;
        if changed != 1 {
            return Err(storage_materialization_snapshot_error());
        }
    }
    Ok(())
}

fn chat_source_state(event: &LedgerEventV1) -> Result<(Option<u64>, Option<String>), SidecarError> {
    if event.aggregate_type != "chat" {
        return Ok((None, None));
    }
    let payload: Value =
        serde_json::from_str(&event.canonical_json_payload).map_err(storage_contract_error)?;
    let object = payload.as_object().ok_or_else(|| {
        SidecarError::new(
            "storageContractViolation",
            "chat source index payload must be an object",
        )
    })?;
    let durable = object
        .get("durable_event_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            SidecarError::new(
                "storageContractViolation",
                "chat source index requires a safe unsigned durable_event_sequence",
            )
        })?;
    let snapshot = match object.get("last_message") {
        Some(Value::Null) => None,
        Some(Value::Object(message)) => message
            .get("messageId")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => {
            return Err(SidecarError::new(
                "storageContractViolation",
                "chat source index last_message must be an object or null",
            ));
        }
    };
    Ok((Some(durable), snapshot))
}

fn load_exact_source_index_predecessor(
    transaction: &Transaction<'_>,
    event: &LedgerEventV1,
) -> Result<Option<Value>, SidecarError> {
    transaction
        .query_row(
            "SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index \
             WHERE aggregate_type='chat' AND aggregate_id=?1 AND aggregate_sequence=?2;",
            params![event.aggregate_id, sql_i64(event.aggregate_sequence - 1)?],
            |row| {
                Ok(json!({
                    "eventRowid": row_u64(row, 0)?,
                    "eventId": row.get::<_, String>(1)?,
                    "operationId": row.get::<_, String>(2)?,
                    "aggregateType": row.get::<_, String>(3)?,
                    "aggregateId": row.get::<_, String>(4)?,
                    "aggregateSequence": row_u64(row, 5)?,
                    "introducedMessageId": row.get::<_, Option<String>>(6)?,
                    "chatDurableSequence": optional_row_u64(row, 7)?,
                    "chatSnapshotMessageId": row.get::<_, Option<String>>(8)?,
                }))
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn load_exact_event_by_rowid(
    transaction: &Transaction<'_>,
    event_rowid: u64,
) -> Result<Option<LedgerEventV1>, SidecarError> {
    transaction
        .query_row(
            "SELECT event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    event_type,payload_schema_version,canonical_json_payload,timestamp \
             FROM events WHERE rowid=?1;",
            [sql_i64(event_rowid)?],
            |row| {
                Ok(LedgerEventV1 {
                    event_id: row.get(0)?,
                    operation_id: row.get(1)?,
                    request_id: row.get(2)?,
                    aggregate_type: row.get(3)?,
                    aggregate_id: row.get(4)?,
                    aggregate_sequence: row_u64(row, 5)?,
                    event_type: row.get(6)?,
                    payload_schema_version: row.get(7)?,
                    canonical_json_payload: row.get(8)?,
                    timestamp: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn write_projection(
    transaction: &Transaction<'_>,
    event: &LedgerEventV1,
    projection: &ProjectionStateV1,
) -> Result<(), SidecarError> {
    let record = match event.aggregate_type.as_str() {
        "app_instance" => projection.app_instances.get(&event.aggregate_id),
        "project" => projection.projects.get(&event.aggregate_id),
        "chat" => projection.chats.get(&event.aggregate_id),
        "run" => projection.runs.get(&event.aggregate_id),
        "lock" => projection.locks.get(&event.aggregate_id),
        _ => None,
    }
    .ok_or_else(|| SidecarError::new("projectionMissing", "reducer omitted event aggregate"))?;
    let sequence = required_u64(record, "last_event_sequence")?;
    let mut payload = record.clone();
    payload.remove("last_event_sequence");
    let projection_json =
        canonical_json_v1(Value::Object(payload)).map_err(storage_contract_error)?;

    match event.aggregate_type.as_str() {
        "app_instance" => {
            let clean = nullable_string(record, "clean_shutdown_at")?;
            transaction.execute(
                "INSERT INTO app_instances(sidecar_instance_id, pid, platform, started_at, heartbeat_at, clean_shutdown_at, last_event_sequence) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(sidecar_instance_id) DO UPDATE SET pid=excluded.pid, platform=excluded.platform, started_at=excluded.started_at, heartbeat_at=excluded.heartbeat_at, clean_shutdown_at=excluded.clean_shutdown_at, last_event_sequence=excluded.last_event_sequence;",
                params![
                    required_string(record, "sidecar_instance_id")?,
                    sql_i64(required_u64(record, "pid")?)?,
                    required_string(record, "platform")?,
                    required_string(record, "started_at")?,
                    required_string(record, "heartbeat_at")?,
                    clean,
                    sql_i64(sequence)?,
                ],
            ).map_err(sqlite_error)?;
        }
        "project" => {
            transaction.execute(
                "INSERT INTO projects(project_id, canonical_path, projection_json, last_event_sequence) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(project_id) DO UPDATE SET canonical_path=excluded.canonical_path, projection_json=excluded.projection_json, last_event_sequence=excluded.last_event_sequence;",
                params![event.aggregate_id, required_string(record, "canonical_path")?, projection_json, sql_i64(sequence)?],
            ).map_err(sqlite_error)?;
        }
        "chat" => {
            transaction.execute(
                "INSERT INTO chats(chat_id, project_id, projection_json, last_event_sequence) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(chat_id) DO UPDATE SET project_id=excluded.project_id, projection_json=excluded.projection_json, last_event_sequence=excluded.last_event_sequence;",
                params![event.aggregate_id, required_string(record, "project_id")?, projection_json, sql_i64(sequence)?],
            ).map_err(sqlite_error)?;
        }
        "run" => {
            transaction.execute(
                "INSERT INTO runs(run_id, chat_id, state, worktree_path, projection_json, last_event_sequence) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(run_id) DO UPDATE SET chat_id=excluded.chat_id, state=excluded.state, worktree_path=excluded.worktree_path, projection_json=excluded.projection_json, last_event_sequence=excluded.last_event_sequence;",
                params![
                    event.aggregate_id,
                    optional_string(record, "chat_id")?,
                    required_string(record, "state")?,
                    optional_string(record, "worktree_path")?,
                    projection_json,
                    sql_i64(sequence)?,
                ],
            ).map_err(sqlite_error)?;
        }
        "lock" => {
            let evidence = record.get("evidence").cloned().ok_or_else(|| {
                SidecarError::new("projectionInvalid", "lock evidence is required")
            })?;
            if !evidence.is_object() {
                return Err(SidecarError::new(
                    "projectionInvalid",
                    "lock evidence must be an object",
                ));
            }
            let evidence_json = canonical_json_v1(evidence).map_err(storage_contract_error)?;
            transaction.execute(
                "INSERT INTO locks(lock_id, project_id, writer_run_id, worktree_path, poisoned, recovery_required, evidence_json, projection_json, last_event_sequence) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(lock_id) DO UPDATE SET project_id=excluded.project_id, writer_run_id=excluded.writer_run_id, worktree_path=excluded.worktree_path, poisoned=excluded.poisoned, recovery_required=excluded.recovery_required, evidence_json=excluded.evidence_json, projection_json=excluded.projection_json, last_event_sequence=excluded.last_event_sequence;",
                params![
                    event.aggregate_id,
                    optional_string(record, "project_id")?,
                    optional_string(record, "writer_run_id")?,
                    required_string(record, "worktree_path")?,
                    i64::from(required_bool(record, "poisoned")?),
                    i64::from(required_bool(record, "recovery_required")?),
                    evidence_json,
                    projection_json,
                    sql_i64(sequence)?,
                ],
            ).map_err(sqlite_error)?;
        }
        _ => {
            return Err(SidecarError::new(
                "aggregateUnsupported",
                "unknown aggregate type",
            ))
        }
    }
    Ok(())
}

fn latest_app_instance(
    connection: &Connection,
) -> Result<Option<Map<String, Value>>, SidecarError> {
    let projection = load_projection(connection)?;
    let latest_id: Option<String> = connection
        .query_row(
            "SELECT aggregate_id FROM events WHERE aggregate_type='app_instance' AND aggregate_sequence=1 ORDER BY rowid DESC LIMIT 1;",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)?;
    latest_id
        .map(|id| {
            projection.app_instances.get(&id).cloned().ok_or_else(|| {
                SidecarError::new(
                    "projectionMissing",
                    "latest app-instance ledger entry has no projection",
                )
            })
        })
        .transpose()
}

fn recovery_event(
    previous_sidecar_id: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    mut record: Map<String, Value>,
    timestamp: &str,
) -> Result<LedgerEventV1, SidecarError> {
    let sequence = required_u64(&record, "last_event_sequence")?
        .checked_add(1)
        .ok_or_else(|| SidecarError::new("sequenceOverflow", "event sequence overflow"))?;
    record.remove("last_event_sequence");
    Ok(LedgerEventV1 {
        event_id: format!("evt_recovery_{previous_sidecar_id}_{aggregate_type}_{aggregate_id}"),
        operation_id: format!("op_recovery_{previous_sidecar_id}_{aggregate_type}_{aggregate_id}"),
        request_id: None,
        aggregate_type: aggregate_type.to_string(),
        aggregate_id: aggregate_id.to_string(),
        aggregate_sequence: sequence,
        event_type: format!("{aggregate_type}.upserted"),
        payload_schema_version: SQLITE_SCHEMA_VERSION,
        canonical_json_payload: canonical_json_v1(Value::Object(record))
            .map_err(storage_contract_error)?,
        timestamp: timestamp.to_string(),
    })
}

fn required_string<'a>(
    record: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, SidecarError> {
    record
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SidecarError::new("projectionInvalid", format!("{field} is required")))
}

fn optional_string<'a>(
    record: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, SidecarError> {
    match record.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value)),
        _ => Err(SidecarError::new(
            "projectionInvalid",
            format!("{field} must be a non-empty string or null"),
        )),
    }
}

fn nullable_string<'a>(
    record: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, SidecarError> {
    match record.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value)),
        _ => Err(SidecarError::new(
            "projectionInvalid",
            format!("{field} must be an explicit string or null"),
        )),
    }
}

fn required_u64(record: &Map<String, Value>, field: &str) -> Result<u64, SidecarError> {
    record
        .get(field)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| SidecarError::new("projectionInvalid", format!("{field} is invalid")))
}

fn required_bool(record: &Map<String, Value>, field: &str) -> Result<bool, SidecarError> {
    record
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| SidecarError::new("projectionInvalid", format!("{field} is invalid")))
}

fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn optional_row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

fn sql_i64(value: u64) -> Result<i64, SidecarError> {
    i64::try_from(value).map_err(|_| {
        SidecarError::new(
            "integerOutOfRange",
            "protocol integer cannot be represented by SQLite",
        )
    })
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn sqlite_open_error(error: rusqlite::Error) -> SidecarError {
    SidecarError::new(
        "databaseOpenFailed",
        format!("could not open SQLite: {error}"),
    )
}

fn sqlite_error(error: rusqlite::Error) -> SidecarError {
    SidecarError::new("databaseFailure", error.to_string())
}

fn storage_contract_error(error: impl std::fmt::Display) -> SidecarError {
    SidecarError::new("storageContractViolation", error.to_string())
}

#[cfg(test)]
mod tests;
