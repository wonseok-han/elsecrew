use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_protocol::project_v1::sha256_file_version_v1;
use hello_agent_protocol::run_recovery_v1::validate_g5_payload_v1;
use hello_agent_protocol::storage_lifetime_v1::{
    classify_operation_against_receipt_v2, validate_storage_event_source_index_binding_v1,
    validate_storage_event_source_index_transition_v1,
    validate_storage_lifetime_materialization_job_v4,
    validate_storage_lifetime_materialization_pair_v4,
    validate_storage_lifetime_workspace_project_source_v1, validate_storage_status_v1,
    OperationReceiptV2, PhysicalCleanupDisabledReasonV1, PhysicalCleanupStatusV1,
    StorageCompactionAttemptStatusV1, StorageCompactionStatusV1, StorageDatabaseFactsV1,
    StorageLedgerTailFactsV1, StorageLifetimePhaseV4, StoragePressureCollectionStatusV1,
    StoragePressureFactsV1, StorageSnapshotAggregateFactsV1, StorageStatusV1,
    OPERATION_RECEIPT_DIGEST_ALGORITHM_V2, OPERATION_RECEIPT_DIGEST_FORMAT_V2,
    OPERATION_RECEIPT_DIGEST_VERSION_V2,
    STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    STORAGE_LIFETIME_SCHEMA_VERSION_V6, STORAGE_LIFETIME_SCHEMA_VERSION_V7,
    STORAGE_LIFETIME_SCHEMA_VERSION_V8, STORAGE_STATUS_SCHEMA_VERSION_V1,
};
use hello_agent_protocol::storage_v1::{
    canonical_json_v1, classify_operation_v1, LedgerEventV1, OperationDecisionV1, ProjectionStateV1,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterializationFailurePoint {
    Receipts,
    MessagesAndWorkspaces,
    BaselinesBeforeMetadata,
}

type ProjectionTable<'a> = (&'static str, &'a BTreeMap<String, Map<String, Value>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LifetimeMaterializationResult {
    pub cutoff_event_rowid: u64,
    pub operation_receipts: u64,
    pub chat_messages: u64,
    pub prepared_workspaces: u64,
    pub ledger_baselines: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LifetimeMaterializationCursorFacts {
    pub cutoff_event_rowid: u64,
    pub phase: StorageLifetimePhaseV4,
    pub after_scan_rowid: u64,
    pub scan_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LifetimeMaterializationBatchResult {
    Progress {
        cursor: LifetimeMaterializationCursorFacts,
        source_rows: u64,
        source_payload_bytes: u64,
    },
    Suspended {
        cursor: Option<LifetimeMaterializationCursorFacts>,
    },
    Completed {
        cutoff_event_rowid: u64,
    },
    Failed {
        cursor: LifetimeMaterializationCursorFacts,
        failure_kind: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterializationCancellationPoint {
    BeforeBegin,
    BeforeSourceRow,
    AfterSourceRow,
    ImmediatelyPrecommit,
    AfterCommit,
    FinalBeforeBegin,
    FinalBeforeMutation,
    FinalImmediatelyPrecommit,
}

pub(crate) trait MaterializationCancellation {
    fn cancelled(&mut self, point: MaterializationCancellationPoint) -> bool;
}

impl<F> MaterializationCancellation for F
where
    F: FnMut(MaterializationCancellationPoint) -> bool,
{
    fn cancelled(&mut self, point: MaterializationCancellationPoint) -> bool {
        self(point)
    }
}

pub(crate) struct NeverCancel;

impl MaterializationCancellation for NeverCancel {
    fn cancelled(&mut self, _point: MaterializationCancellationPoint) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchMaterializationFailurePoint {
    AfterArmCommit,
    AfterDerivedRows,
    BatchCommitUnavailable,
    FailureRecordCommitUnavailable,
}

#[derive(Debug, Clone)]
struct LifetimeJob {
    job_id: String,
    state: String,
    cutoff_event_rowid: u64,
    phase: StorageLifetimePhaseV4,
    after_scan_rowid: u64,
    scan_exhausted: bool,
    failure_kind: Option<String>,
    created_at: String,
    updated_at: String,
    completed_at: Option<String>,
}

#[derive(Debug, Clone)]
struct SourceEvent {
    rowid: u64,
    event: LedgerEventV1,
}

#[derive(Debug)]
struct IndexedSourceEvent {
    entry: Value,
    source: SourceEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MaterializedMessage {
    message_id: String,
    chat_id: String,
    event_sequence: u64,
    canonical_json: String,
    digest: String,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MaterializedWorkspace {
    preparation_id: String,
    project_id: String,
    worktree_id: String,
    workspace_identity: String,
    binding_state: String,
    bound_chat_id: Option<String>,
    canonical_json: String,
    digest: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MaterializedBaseline {
    baseline_id: String,
    aggregate_type: String,
    aggregate_id: String,
    cutoff_sequence: u64,
    canonical_json: String,
    digest: String,
}

enum BatchAttemptFailure {
    Suspended,
    Terminal(&'static str),
}

#[derive(Debug)]
enum ScanRow {
    Event(SourceEvent),
    Indexed(IndexedSourceEvent),
    Derived {
        rowid: u64,
        key: String,
        payload_bytes: u64,
    },
}

enum MaterializationPair {
    Ready,
    Absent,
    Pending(LifetimeJob),
    Attempting(LifetimeJob),
    Failed(LifetimeJob),
}

impl LifetimeJob {
    fn cursor(&self) -> LifetimeMaterializationCursorFacts {
        LifetimeMaterializationCursorFacts {
            cutoff_event_rowid: self.cutoff_event_rowid,
            phase: self.phase,
            after_scan_rowid: self.after_scan_rowid,
            scan_exhausted: self.scan_exhausted,
        }
    }
}

fn read_pending_cursor(
    connection: &Connection,
) -> Result<Option<LifetimeMaterializationCursorFacts>, SidecarError> {
    match load_materialization_pair(connection)? {
        MaterializationPair::Pending(job)
        | MaterializationPair::Attempting(job)
        | MaterializationPair::Failed(job) => Ok(Some(job.cursor())),
        MaterializationPair::Ready | MaterializationPair::Absent => Ok(None),
    }
}

fn load_materialization_pair(connection: &Connection) -> Result<MaterializationPair, SidecarError> {
    let state: String = connection
        .query_row(
            "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| materialization_error("storage materialization metadata is unreadable"))?;
    let job = connection
        .query_row(
            "SELECT job_id,state,cutoff_event_rowid,phase,after_scan_rowid,scan_exhausted,failure_kind,\
                    created_at,updated_at,completed_at \
             FROM lifetime_materialization_job WHERE singleton=1;",
            [],
            |row| {
                let cutoff: i64 = row.get(2)?;
                let after: i64 = row.get(4)?;
                Ok(LifetimeJob {
                    job_id: row.get(0)?,
                    state: row.get(1)?,
                    cutoff_event_rowid: u64::try_from(cutoff)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, cutoff))?,
                    phase: parse_phase(&row.get::<_, String>(3)?).map_err(|_| {
                        rusqlite::Error::InvalidColumnType(
                            3,
                            "phase".to_string(),
                            rusqlite::types::Type::Text,
                        )
                    })?,
                    after_scan_rowid: u64::try_from(after)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, after))?,
                    scan_exhausted: row.get::<_, i64>(5)? == 1,
                    failure_kind: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    completed_at: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(|_| materialization_error("storage materialization singleton is unreadable"))?;

    let job_value = job.as_ref().map(job_json);
    if let Some(value) = job_value.as_ref() {
        validate_storage_lifetime_materialization_job_v4(value)
            .map_err(|_| materialization_error("storage materialization singleton is invalid"))?;
    }
    validate_storage_lifetime_materialization_pair_v4(&serde_json::json!({
        "materializationState": state,
        "job": job_value,
    }))
    .map_err(|_| materialization_error("storage materialization meta/job pair is invalid"))?;

    match (state.as_str(), job) {
        ("ready", None) => Ok(MaterializationPair::Ready),
        ("pending", None) => Ok(MaterializationPair::Absent),
        ("pending", Some(job)) if job.state == "pending" => Ok(MaterializationPair::Pending(job)),
        ("pending", Some(job)) if job.state == "attempting" => {
            Ok(MaterializationPair::Attempting(job))
        }
        ("pending", Some(job)) if job.state == "failed" => Ok(MaterializationPair::Failed(job)),
        _ => Err(materialization_error(
            "storage materialization meta/job pair is invalid",
        )),
    }
}

fn create_materialization_job(transaction: &Transaction<'_>) -> Result<LifetimeJob, SidecarError> {
    let cutoff_event_rowid = max_event_rowid(transaction)?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let job = LifetimeJob {
        job_id: format!("lifetime_{}", Uuid::new_v4().simple()),
        state: "pending".to_string(),
        cutoff_event_rowid,
        phase: StorageLifetimePhaseV4::IndexSource,
        after_scan_rowid: 0,
        scan_exhausted: false,
        failure_kind: None,
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
    };
    validate_storage_lifetime_materialization_job_v4(&job_json(&job))
        .map_err(|_| materialization_error("new storage materialization singleton is invalid"))?;
    transaction
        .execute(
            "INSERT INTO lifetime_materialization_job(\
               singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,scan_exhausted,\
               failure_kind,created_at,updated_at,completed_at\
             ) VALUES (1,?1,'pending',?2,'indexSource',0,0,NULL,?3,?3,NULL);",
            params![job.job_id, sql_i64(job.cutoff_event_rowid)?, job.created_at],
        )
        .map_err(sqlite_error)?;
    match load_materialization_pair(transaction)? {
        MaterializationPair::Pending(persisted) if persisted.job_id == job.job_id => {}
        _ => {
            return Err(materialization_error(
                "new storage materialization singleton could not be confirmed",
            ))
        }
    }
    Ok(job)
}

fn max_event_rowid(connection: &Connection) -> Result<u64, SidecarError> {
    let cutoff: i64 = connection
        .query_row("SELECT coalesce(max(rowid),0) FROM events;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    u64::try_from(cutoff).map_err(|_| materialization_error("storage event cutoff is invalid"))
}

fn load_source_batch(
    connection: &Connection,
    after_scan_rowid: u64,
    cutoff_event_rowid: u64,
) -> Result<Vec<SourceEvent>, SidecarError> {
    let mut statement = connection
        .prepare(
            "SELECT rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,\
                    aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp \
             FROM events WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid LIMIT 100;",
        )
        .map_err(sqlite_error)?;
    let mut rows = statement
        .query(params![
            sql_i64(after_scan_rowid)?,
            sql_i64(cutoff_event_rowid)?
        ])
        .map_err(sqlite_error)?;
    let mut source = Vec::new();
    let mut total = 0_u64;
    while let Some(row) = rows.next().map_err(sqlite_error)? {
        let rowid = row_u64(row, 0).map_err(sqlite_error)?;
        let payload: String = row.get(9).map_err(sqlite_error)?;
        let payload_bytes = payload.len() as u64;
        if payload_bytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
            return Err(SidecarError::new(
                "sourcePayloadOversize",
                "storage source payload exceeds the fixed batch contract",
            ));
        }
        if total.checked_add(payload_bytes).is_none_or(|next| {
            next > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1
        }) {
            break;
        }
        total += payload_bytes;
        source.push(SourceEvent {
            rowid,
            event: LedgerEventV1 {
                event_id: row.get(1).map_err(sqlite_error)?,
                operation_id: row.get(2).map_err(sqlite_error)?,
                request_id: row.get(3).map_err(sqlite_error)?,
                aggregate_type: row.get(4).map_err(sqlite_error)?,
                aggregate_id: row.get(5).map_err(sqlite_error)?,
                aggregate_sequence: row_u64(row, 6).map_err(sqlite_error)?,
                event_type: row.get(7).map_err(sqlite_error)?,
                payload_schema_version: row.get(8).map_err(sqlite_error)?,
                canonical_json_payload: payload,
                timestamp: row.get(10).map_err(sqlite_error)?,
            },
        });
    }
    Ok(source)
}

fn load_exact_event_by_rowid(
    connection: &Connection,
    event_rowid: u64,
) -> Result<Option<SourceEvent>, SidecarError> {
    connection
        .query_row(
            "SELECT event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    event_type,payload_schema_version,canonical_json_payload,timestamp \
             FROM events WHERE rowid=?1;",
            [sql_i64(event_rowid)?],
            |row| {
                Ok(SourceEvent {
                    rowid: event_rowid,
                    event: LedgerEventV1 {
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
                    },
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn source_index_value(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
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
}

fn load_source_index_by_rowid(
    connection: &Connection,
    event_rowid: u64,
) -> Result<Option<Value>, SidecarError> {
    connection
        .query_row(
            "SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE event_rowid=?1;",
            [sql_i64(event_rowid)?],
            source_index_value,
        )
        .optional()
        .map_err(sqlite_error)
}

fn load_source_index_by_aggregate_sequence(
    connection: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    aggregate_sequence: u64,
) -> Result<Option<Value>, SidecarError> {
    connection
        .query_row(
            "SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE aggregate_type=?1 AND aggregate_id=?2 \
             AND aggregate_sequence=?3;",
            params![aggregate_type, aggregate_id, sql_i64(aggregate_sequence)?],
            source_index_value,
        )
        .optional()
        .map_err(sqlite_error)
}

fn load_latest_source_index(
    connection: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    cutoff: u64,
) -> Result<Option<Value>, SidecarError> {
    connection
        .query_row(
            "SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE aggregate_type=?1 AND aggregate_id=?2 \
             AND event_rowid<=?3 ORDER BY event_rowid DESC LIMIT 1;",
            params![aggregate_type, aggregate_id, sql_i64(cutoff)?],
            source_index_value,
        )
        .optional()
        .map_err(sqlite_error)
}

fn load_source_index_by_introduced_message(
    connection: &Connection,
    message_id: &str,
) -> Result<Option<Value>, SidecarError> {
    connection
        .query_row(
            "SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE introduced_message_id=?1;",
            [message_id],
            source_index_value,
        )
        .optional()
        .map_err(sqlite_error)
}

fn indexed_source_event(
    connection: &Connection,
    entry: Value,
    cutoff: u64,
) -> Result<SourceEvent, SidecarError> {
    let rowid = entry
        .get("eventRowid")
        .and_then(Value::as_u64)
        .ok_or_else(|| materialization_error("source index rowid is invalid"))?;
    if rowid > cutoff {
        return Err(materialization_error(
            "source index row exceeds frozen cutoff",
        ));
    }
    let source = load_exact_event_by_rowid(connection, rowid)?
        .ok_or_else(|| materialization_error("indexed source event is missing"))?;
    if source.event.canonical_json_payload.len()
        > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 as usize
    {
        return Err(SidecarError::new(
            "sourcePayloadOversize",
            "storage source payload exceeds the fixed batch contract",
        ));
    }
    Ok(source)
}

fn load_source_index_batch(
    connection: &Connection,
    after_scan_rowid: u64,
    cutoff_event_rowid: u64,
) -> Result<Vec<IndexedSourceEvent>, SidecarError> {
    let mut statement = connection
        .prepare(
            "SELECT event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
                    introduced_message_id,chat_durable_sequence,chat_snapshot_message_id \
             FROM event_source_index WHERE event_rowid>?1 AND event_rowid<=?2 \
             ORDER BY event_rowid LIMIT 100;",
        )
        .map_err(sqlite_error)?;
    let entries = statement
        .query_map(
            params![sql_i64(after_scan_rowid)?, sql_i64(cutoff_event_rowid)?],
            source_index_value,
        )
        .map_err(sqlite_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sqlite_error)?;
    let mut total = 0_u64;
    let mut rows = Vec::new();
    for entry in entries {
        let event_rowid = entry
            .get("eventRowid")
            .and_then(Value::as_u64)
            .ok_or_else(|| materialization_error("source index rowid is invalid"))?;
        let source = load_exact_event_by_rowid(connection, event_rowid)?
            .ok_or_else(|| materialization_error("source index event is missing"))?;
        let payload_bytes = source.event.canonical_json_payload.len() as u64;
        if payload_bytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
            return Err(SidecarError::new(
                "sourcePayloadOversize",
                "storage source payload exceeds the fixed batch contract",
            ));
        }
        let next = total
            .checked_add(payload_bytes)
            .ok_or_else(|| materialization_error("storage source payload byte sum overflowed"))?;
        if next > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
            break;
        }
        total = next;
        rows.push(IndexedSourceEvent { entry, source });
    }
    Ok(rows)
}

fn job_json(job: &LifetimeJob) -> Value {
    serde_json::json!({
        "singleton": 1,
        "jobId": job.job_id,
        "state": job.state,
        "cutoffEventRowid": job.cutoff_event_rowid,
        "phase": phase_str(job.phase),
        "afterScanRowid": job.after_scan_rowid,
        "scanExhausted": job.scan_exhausted,
        "failureKind": job.failure_kind,
        "createdAt": job.created_at,
        "updatedAt": job.updated_at,
        "completedAt": job.completed_at,
    })
}

fn phase_str(phase: StorageLifetimePhaseV4) -> &'static str {
    match phase {
        StorageLifetimePhaseV4::IndexSource => "indexSource",
        StorageLifetimePhaseV4::VerifySourceIndex => "verifySourceIndex",
        StorageLifetimePhaseV4::Receipts => "receipts",
        StorageLifetimePhaseV4::Messages => "messages",
        StorageLifetimePhaseV4::Workspaces => "workspaces",
        StorageLifetimePhaseV4::Baselines => "baselines",
        StorageLifetimePhaseV4::VerifySource => "verifySource",
        StorageLifetimePhaseV4::VerifyProjects => "verifyProjects",
        StorageLifetimePhaseV4::VerifyChats => "verifyChats",
        StorageLifetimePhaseV4::VerifyRuns => "verifyRuns",
        StorageLifetimePhaseV4::VerifyLocks => "verifyLocks",
        StorageLifetimePhaseV4::VerifyAppInstances => "verifyAppInstances",
        StorageLifetimePhaseV4::VerifyReceipts => "verifyReceipts",
        StorageLifetimePhaseV4::VerifyMessages => "verifyMessages",
        StorageLifetimePhaseV4::VerifyWorkspaces => "verifyWorkspaces",
        StorageLifetimePhaseV4::VerifyBaselines => "verifyBaselines",
    }
}

fn parse_phase(value: &str) -> Result<StorageLifetimePhaseV4, SidecarError> {
    match value {
        "indexSource" => Ok(StorageLifetimePhaseV4::IndexSource),
        "verifySourceIndex" => Ok(StorageLifetimePhaseV4::VerifySourceIndex),
        "receipts" => Ok(StorageLifetimePhaseV4::Receipts),
        "messages" => Ok(StorageLifetimePhaseV4::Messages),
        "workspaces" => Ok(StorageLifetimePhaseV4::Workspaces),
        "baselines" => Ok(StorageLifetimePhaseV4::Baselines),
        "verifySource" => Ok(StorageLifetimePhaseV4::VerifySource),
        "verifyProjects" => Ok(StorageLifetimePhaseV4::VerifyProjects),
        "verifyChats" => Ok(StorageLifetimePhaseV4::VerifyChats),
        "verifyRuns" => Ok(StorageLifetimePhaseV4::VerifyRuns),
        "verifyLocks" => Ok(StorageLifetimePhaseV4::VerifyLocks),
        "verifyAppInstances" => Ok(StorageLifetimePhaseV4::VerifyAppInstances),
        "verifyReceipts" => Ok(StorageLifetimePhaseV4::VerifyReceipts),
        "verifyMessages" => Ok(StorageLifetimePhaseV4::VerifyMessages),
        "verifyWorkspaces" => Ok(StorageLifetimePhaseV4::VerifyWorkspaces),
        "verifyBaselines" => Ok(StorageLifetimePhaseV4::VerifyBaselines),
        _ => Err(materialization_error(
            "storage materialization phase is invalid",
        )),
    }
}

fn next_phase(phase: StorageLifetimePhaseV4) -> Option<StorageLifetimePhaseV4> {
    match phase {
        StorageLifetimePhaseV4::IndexSource => Some(StorageLifetimePhaseV4::VerifySourceIndex),
        StorageLifetimePhaseV4::VerifySourceIndex => Some(StorageLifetimePhaseV4::Receipts),
        StorageLifetimePhaseV4::Receipts => Some(StorageLifetimePhaseV4::Messages),
        StorageLifetimePhaseV4::Messages => Some(StorageLifetimePhaseV4::Workspaces),
        StorageLifetimePhaseV4::Workspaces => Some(StorageLifetimePhaseV4::Baselines),
        StorageLifetimePhaseV4::Baselines => Some(StorageLifetimePhaseV4::VerifySource),
        StorageLifetimePhaseV4::VerifySource => Some(StorageLifetimePhaseV4::VerifyProjects),
        StorageLifetimePhaseV4::VerifyProjects => Some(StorageLifetimePhaseV4::VerifyChats),
        StorageLifetimePhaseV4::VerifyChats => Some(StorageLifetimePhaseV4::VerifyRuns),
        StorageLifetimePhaseV4::VerifyRuns => Some(StorageLifetimePhaseV4::VerifyLocks),
        StorageLifetimePhaseV4::VerifyLocks => Some(StorageLifetimePhaseV4::VerifyAppInstances),
        StorageLifetimePhaseV4::VerifyAppInstances => Some(StorageLifetimePhaseV4::VerifyReceipts),
        StorageLifetimePhaseV4::VerifyReceipts => Some(StorageLifetimePhaseV4::VerifyMessages),
        StorageLifetimePhaseV4::VerifyMessages => Some(StorageLifetimePhaseV4::VerifyWorkspaces),
        StorageLifetimePhaseV4::VerifyWorkspaces => Some(StorageLifetimePhaseV4::VerifyBaselines),
        StorageLifetimePhaseV4::VerifyBaselines => None,
    }
}

fn verify_bounded_storage_runtime_state(connection: &Connection) -> Result<(), SidecarError> {
    let version: u32 = connection
        .query_row(
            "SELECT storage_schema_version FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .map_err(|_| materialization_error("storage schema version is unreadable"))?;
    match version {
        STORAGE_LIFETIME_SCHEMA_VERSION_V6 => Ok(()),
        STORAGE_LIFETIME_SCHEMA_VERSION_V7 => {
            crate::storage::verify_v7_uncompacted_runtime_state(connection)
        }
        STORAGE_LIFETIME_SCHEMA_VERSION_V8 => {
            crate::storage::verify_v8_uncompacted_runtime_state(connection)
        }
        _ => Err(materialization_error(
            "bounded materialization requires exact storage v6, v7, or v8",
        )),
    }
}

pub(crate) fn materialize_lifetime_batch(
    connection: &mut Connection,
    cancellation: &mut dyn MaterializationCancellation,
    failure: Option<BatchMaterializationFailurePoint>,
) -> Result<LifetimeMaterializationBatchResult, SidecarError> {
    // Cancellation is not an escape hatch from the V7/V8 compaction fence. Keep
    // the transaction-local check below as well so state cannot race the arm.
    verify_bounded_storage_runtime_state(connection)?;
    if cancellation.cancelled(MaterializationCancellationPoint::BeforeBegin) {
        return Ok(LifetimeMaterializationBatchResult::Suspended {
            cursor: read_pending_cursor(connection)?,
        });
    }

    let arm = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    verify_bounded_storage_runtime_state(&arm)?;
    let mut job = match load_materialization_pair(&arm)? {
        MaterializationPair::Ready => {
            let cutoff_event_rowid = max_event_rowid(&arm)?;
            arm.commit().map_err(sqlite_error)?;
            return Ok(LifetimeMaterializationBatchResult::Completed { cutoff_event_rowid });
        }
        MaterializationPair::Attempting(_) => {
            return Err(materialization_error(
                "storage materialization has an unconfirmed prior attempt",
            ));
        }
        MaterializationPair::Failed(job) => {
            return Ok(LifetimeMaterializationBatchResult::Failed {
                cursor: job.cursor(),
                failure_kind: job
                    .failure_kind
                    .unwrap_or_else(|| "validationFailed".to_string()),
            });
        }
        MaterializationPair::Absent => create_materialization_job(&arm)?,
        MaterializationPair::Pending(job) => job,
    };

    if is_verification_phase(job.phase) {
        let observed_cutoff = max_event_rowid(&arm)?;
        if observed_cutoff < job.cutoff_event_rowid {
            return Err(materialization_error(
                "storage materialization cutoff moved backwards",
            ));
        }
        if observed_cutoff > job.cutoff_event_rowid {
            let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            let changed = arm
                .execute(
                    "UPDATE lifetime_materialization_job SET cutoff_event_rowid=?2,phase='indexSource',\
                     after_scan_rowid=0,scan_exhausted=0,updated_at=?3 WHERE singleton=1 \
                     AND job_id=?1 AND state='pending';",
                    params![job.job_id, sql_i64(observed_cutoff)?, now],
                )
                .map_err(sqlite_error)?;
            if changed != 1 {
                return Err(materialization_error(
                    "storage materialization catch-up reset changed unexpectedly",
                ));
            }
            arm.commit().map_err(sqlite_error)?;
            return Ok(LifetimeMaterializationBatchResult::Progress {
                cursor: LifetimeMaterializationCursorFacts {
                    cutoff_event_rowid: observed_cutoff,
                    phase: StorageLifetimePhaseV4::IndexSource,
                    after_scan_rowid: 0,
                    scan_exhausted: false,
                },
                source_rows: 0,
                source_payload_bytes: 0,
            });
        }
    }

    if job.scan_exhausted {
        if job.phase == StorageLifetimePhaseV4::VerifyBaselines {
            arm.commit().map_err(sqlite_error)?;
            return finalize_materialization(connection, cancellation);
        }
        let next = next_phase(job.phase)
            .ok_or_else(|| materialization_error("storage materialization phase is invalid"))?;
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let changed = arm
            .execute(
                "UPDATE lifetime_materialization_job SET phase=?2,after_scan_rowid=0,\
                 scan_exhausted=0,updated_at=?3 WHERE singleton=1 AND job_id=?1 \
                 AND state='pending' AND phase=?4 AND scan_exhausted=1;",
                params![job.job_id, phase_str(next), now, phase_str(job.phase)],
            )
            .map_err(sqlite_error)?;
        if changed != 1 {
            return Err(materialization_error(
                "storage materialization phase advance changed unexpectedly",
            ));
        }
        job.phase = next;
        job.after_scan_rowid = 0;
        job.scan_exhausted = false;
        job.updated_at = now;
        arm.commit().map_err(sqlite_error)?;
        if cancellation.cancelled(MaterializationCancellationPoint::AfterCommit) {
            return Ok(LifetimeMaterializationBatchResult::Suspended {
                cursor: Some(job.cursor()),
            });
        }
        return Ok(LifetimeMaterializationBatchResult::Progress {
            cursor: job.cursor(),
            source_rows: 0,
            source_payload_bytes: 0,
        });
    }

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let changed = arm
        .execute(
            "UPDATE lifetime_materialization_job SET state='attempting',updated_at=?2 \
             WHERE singleton=1 AND job_id=?1 AND state='pending' AND scan_exhausted=0;",
            params![job.job_id, now],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(materialization_error(
            "storage materialization arm changed unexpectedly",
        ));
    }
    job.state = "attempting".to_string();
    job.updated_at = now;
    arm.commit().map_err(sqlite_error)?;
    if failure == Some(BatchMaterializationFailurePoint::AfterArmCommit) {
        return Err(materialization_error(
            "storage materialization attempt was not confirmed",
        ));
    }

    run_bounded_batch(connection, job, cancellation, failure)
}

fn is_verification_phase(phase: StorageLifetimePhaseV4) -> bool {
    matches!(
        phase,
        StorageLifetimePhaseV4::VerifySourceIndex
            | StorageLifetimePhaseV4::VerifySource
            | StorageLifetimePhaseV4::VerifyProjects
            | StorageLifetimePhaseV4::VerifyChats
            | StorageLifetimePhaseV4::VerifyRuns
            | StorageLifetimePhaseV4::VerifyLocks
            | StorageLifetimePhaseV4::VerifyAppInstances
            | StorageLifetimePhaseV4::VerifyReceipts
            | StorageLifetimePhaseV4::VerifyMessages
            | StorageLifetimePhaseV4::VerifyWorkspaces
            | StorageLifetimePhaseV4::VerifyBaselines
    )
}

fn run_bounded_batch(
    connection: &mut Connection,
    mut job: LifetimeJob,
    cancellation: &mut dyn MaterializationCancellation,
    failure: Option<BatchMaterializationFailurePoint>,
) -> Result<LifetimeMaterializationBatchResult, SidecarError> {
    let durable_attempt = job.clone();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    verify_bounded_storage_runtime_state(&transaction)?;
    match load_materialization_pair(&transaction)? {
        MaterializationPair::Attempting(current)
            if current.job_id == job.job_id && current.cursor() == job.cursor() => {}
        _ => {
            return Err(materialization_error(
                "storage materialization attempt changed unexpectedly",
            ));
        }
    }
    transaction
        .execute_batch("SAVEPOINT bounded_materialization_batch;")
        .map_err(sqlite_error)?;

    let attempt = run_batch_savepoint(&transaction, &mut job, cancellation, failure);
    let (source_rows, source_payload_bytes) = match attempt {
        Ok(facts) => facts,
        Err(BatchAttemptFailure::Suspended) => {
            rollback_batch_savepoint(&transaction)?;
            restore_pending_attempt(&transaction, &durable_attempt)?;
            transaction.commit().map_err(sqlite_error)?;
            return Ok(LifetimeMaterializationBatchResult::Suspended {
                cursor: Some(durable_attempt.cursor()),
            });
        }
        Err(BatchAttemptFailure::Terminal(failure_kind)) => {
            rollback_batch_savepoint(&transaction)?;
            fail_attempt(&transaction, &job, failure_kind)?;
            if failure == Some(BatchMaterializationFailurePoint::FailureRecordCommitUnavailable) {
                transaction.rollback().map_err(sqlite_error)?;
                return Err(materialization_error(
                    "storage materialization failure record was not confirmed",
                ));
            }
            transaction.commit().map_err(|_| {
                materialization_error("storage materialization failure record was not confirmed")
            })?;
            return Ok(LifetimeMaterializationBatchResult::Failed {
                cursor: job.cursor(),
                failure_kind: failure_kind.to_string(),
            });
        }
    };

    if cancellation.cancelled(MaterializationCancellationPoint::ImmediatelyPrecommit) {
        rollback_batch_savepoint(&transaction)?;
        restore_pending_attempt(&transaction, &durable_attempt)?;
        transaction.commit().map_err(sqlite_error)?;
        return Ok(LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(durable_attempt.cursor()),
        });
    }
    transaction
        .execute_batch("RELEASE bounded_materialization_batch;")
        .map_err(sqlite_error)?;
    if failure == Some(BatchMaterializationFailurePoint::BatchCommitUnavailable) {
        transaction.rollback().map_err(sqlite_error)?;
        return Err(materialization_error(
            "storage materialization batch commit was not confirmed",
        ));
    }
    transaction.commit().map_err(|_| {
        materialization_error("storage materialization batch commit was not confirmed")
    })?;
    if cancellation.cancelled(MaterializationCancellationPoint::AfterCommit) {
        return Ok(LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(job.cursor()),
        });
    }
    Ok(LifetimeMaterializationBatchResult::Progress {
        cursor: job.cursor(),
        source_rows,
        source_payload_bytes,
    })
}

fn run_batch_savepoint(
    transaction: &Transaction<'_>,
    job: &mut LifetimeJob,
    cancellation: &mut dyn MaterializationCancellation,
    failure: Option<BatchMaterializationFailurePoint>,
) -> Result<(u64, u64), BatchAttemptFailure> {
    let rows = load_scan_batch(transaction, job)
        .map_err(|error| BatchAttemptFailure::Terminal(sanitized_failure_kind(&error)))?;
    let source_rows = rows.len() as u64;
    let source_payload_bytes = rows.iter().try_fold(0_u64, |total, row| {
        total.checked_add(match row {
            ScanRow::Event(row) => row.event.canonical_json_payload.len() as u64,
            ScanRow::Indexed(row) => row.source.event.canonical_json_payload.len() as u64,
            ScanRow::Derived { payload_bytes, .. } => *payload_bytes,
        })
    });
    let source_payload_bytes =
        source_payload_bytes.ok_or(BatchAttemptFailure::Terminal("sourcePayloadInvalid"))?;

    if rows.is_empty() {
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let changed = transaction
            .execute(
                "UPDATE lifetime_materialization_job SET state='pending',scan_exhausted=1,\
                 updated_at=?2 WHERE singleton=1 AND job_id=?1 AND state='attempting' \
                 AND phase=?3 AND after_scan_rowid=?4 AND scan_exhausted=0;",
                params![
                    job.job_id,
                    now,
                    phase_str(job.phase),
                    sql_i64(job.after_scan_rowid)
                        .map_err(|_| BatchAttemptFailure::Terminal("validationFailed"))?
                ],
            )
            .map_err(|error| {
                BatchAttemptFailure::Terminal(sanitized_failure_kind(&sqlite_error(error)))
            })?;
        if changed != 1 {
            return Err(BatchAttemptFailure::Terminal("validationFailed"));
        }
        job.state = "pending".to_string();
        job.scan_exhausted = true;
        job.updated_at = now;
        return Ok((0, 0));
    }

    let materialized_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut last_rowid = job.after_scan_rowid;
    for row in &rows {
        if cancellation.cancelled(MaterializationCancellationPoint::BeforeSourceRow) {
            return Err(BatchAttemptFailure::Suspended);
        }
        process_scan_row(transaction, job, row, &materialized_at)
            .map_err(|error| BatchAttemptFailure::Terminal(sanitized_failure_kind(&error)))?;
        last_rowid = match row {
            ScanRow::Event(row) => row.rowid,
            ScanRow::Indexed(row) => row.source.rowid,
            ScanRow::Derived { rowid, .. } => *rowid,
        };
        if failure == Some(BatchMaterializationFailurePoint::AfterDerivedRows) {
            return Err(BatchAttemptFailure::Terminal("injectedFailure"));
        }
        if cancellation.cancelled(MaterializationCancellationPoint::AfterSourceRow) {
            return Err(BatchAttemptFailure::Suspended);
        }
    }

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let changed = transaction
        .execute(
            "UPDATE lifetime_materialization_job SET state='pending',after_scan_rowid=?2,\
             scan_exhausted=0,updated_at=?3 WHERE singleton=1 AND job_id=?1 \
             AND state='attempting' AND phase=?4 AND after_scan_rowid=?5;",
            params![
                job.job_id,
                sql_i64(last_rowid)
                    .map_err(|_| BatchAttemptFailure::Terminal("validationFailed"))?,
                now,
                phase_str(job.phase),
                sql_i64(job.after_scan_rowid)
                    .map_err(|_| BatchAttemptFailure::Terminal("validationFailed"))?
            ],
        )
        .map_err(|_| BatchAttemptFailure::Terminal("databaseFailure"))?;
    if changed != 1 {
        return Err(BatchAttemptFailure::Terminal("validationFailed"));
    }
    job.state = "pending".to_string();
    job.after_scan_rowid = last_rowid;
    job.scan_exhausted = false;
    job.updated_at = now;
    Ok((source_rows, source_payload_bytes))
}

fn rollback_batch_savepoint(transaction: &Transaction<'_>) -> Result<(), SidecarError> {
    transaction
        .execute_batch(
            "ROLLBACK TO bounded_materialization_batch; RELEASE bounded_materialization_batch;",
        )
        .map_err(sqlite_error)
}

fn restore_pending_attempt(
    transaction: &Transaction<'_>,
    job: &LifetimeJob,
) -> Result<(), SidecarError> {
    let changed = transaction
        .execute(
            "UPDATE lifetime_materialization_job SET state='pending',updated_at=?2 \
             WHERE singleton=1 AND job_id=?1 AND state='attempting';",
            params![
                job.job_id,
                Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
            ],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(materialization_error(
            "storage materialization cancellation changed unexpectedly",
        ));
    }
    Ok(())
}

fn fail_attempt(
    transaction: &Transaction<'_>,
    job: &LifetimeJob,
    failure_kind: &str,
) -> Result<(), SidecarError> {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let changed = transaction
        .execute(
            "UPDATE lifetime_materialization_job SET state='failed',failure_kind=?2,\
             updated_at=?3,completed_at=?3 WHERE singleton=1 AND job_id=?1 \
             AND state='attempting';",
            params![job.job_id, failure_kind, now],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(materialization_error(
            "storage materialization failure state changed unexpectedly",
        ));
    }
    Ok(())
}

fn sanitized_failure_kind(error: &SidecarError) -> &'static str {
    match error.kind() {
        "sourcePayloadOversize" => "sourcePayloadOversize",
        "databaseFailure" => "databaseFailure",
        "injectedFailure" => "injectedFailure",
        _ => "validationFailed",
    }
}

fn load_scan_batch(
    connection: &Connection,
    job: &LifetimeJob,
) -> Result<Vec<ScanRow>, SidecarError> {
    if job.phase == StorageLifetimePhaseV4::VerifySourceIndex {
        return Ok(load_source_index_batch(
            connection,
            job.after_scan_rowid,
            job.cutoff_event_rowid,
        )?
        .into_iter()
        .map(ScanRow::Indexed)
        .collect());
    }
    if matches!(
        job.phase,
        StorageLifetimePhaseV4::IndexSource
            | StorageLifetimePhaseV4::Receipts
            | StorageLifetimePhaseV4::Messages
            | StorageLifetimePhaseV4::Workspaces
            | StorageLifetimePhaseV4::Baselines
            | StorageLifetimePhaseV4::VerifySource
    ) {
        return Ok(
            load_source_batch(connection, job.after_scan_rowid, job.cutoff_event_rowid)?
                .into_iter()
                .map(ScanRow::Event)
                .collect(),
        );
    }
    let (table, key, payload) = match job.phase {
        StorageLifetimePhaseV4::VerifyProjects => {
            ("projects", "project_id", "projection_json")
        }
        StorageLifetimePhaseV4::VerifyChats => ("chats", "chat_id", "projection_json"),
        StorageLifetimePhaseV4::VerifyRuns => ("runs", "run_id", "projection_json"),
        StorageLifetimePhaseV4::VerifyLocks => ("locks", "lock_id", "projection_json"),
        StorageLifetimePhaseV4::VerifyAppInstances => (
            "app_instances",
            "sidecar_instance_id",
            "json_object('sidecar_instance_id',sidecar_instance_id,'pid',pid,'platform',platform,\
             'started_at',started_at,'heartbeat_at',heartbeat_at,'clean_shutdown_at',clean_shutdown_at)",
        ),
        StorageLifetimePhaseV4::VerifyReceipts => (
            "operation_receipts",
            "operation_id",
            "canonical_payload_json",
        ),
        StorageLifetimePhaseV4::VerifyMessages => {
            ("chat_messages", "message_id", "canonical_message_json")
        }
        StorageLifetimePhaseV4::VerifyWorkspaces => (
            "prepared_workspaces",
            "preparation_id",
            "canonical_record_json",
        ),
        StorageLifetimePhaseV4::VerifyBaselines => (
            "ledger_baselines",
            "baseline_id",
            "canonical_projection_json",
        ),
        _ => {
            return Err(materialization_error(
                "storage materialization phase is invalid",
            ))
        }
    };
    let sql = format!(
        "SELECT rowid,{key},{payload} FROM {table} WHERE rowid>?1 ORDER BY rowid LIMIT 100;"
    );
    let mut statement = connection.prepare(&sql).map_err(sqlite_error)?;
    let rows = statement
        .query_map([sql_i64(job.after_scan_rowid)?], |row| {
            let payload: String = row.get(2)?;
            Ok((
                row_u64(row, 0)?,
                row.get::<_, String>(1)?,
                payload.len() as u64,
            ))
        })
        .map_err(sqlite_error)?;
    let candidates = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sqlite_error)?;
    bounded_derived_rows(candidates)
}

fn bounded_derived_rows(candidates: Vec<(u64, String, u64)>) -> Result<Vec<ScanRow>, SidecarError> {
    let mut total = 0_u64;
    let mut rows = Vec::new();
    for (rowid, key, payload_bytes) in candidates {
        if payload_bytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
            return Err(SidecarError::new(
                "sourcePayloadOversize",
                "storage source payload exceeds the fixed batch contract",
            ));
        }
        let Some(next) = total.checked_add(payload_bytes) else {
            return Err(materialization_error(
                "storage source payload byte sum overflowed",
            ));
        };
        if next > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 {
            break;
        }
        total = next;
        rows.push(ScanRow::Derived {
            rowid,
            key,
            payload_bytes,
        });
    }
    Ok(rows)
}

fn process_scan_row(
    transaction: &Transaction<'_>,
    job: &LifetimeJob,
    row: &ScanRow,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    match row {
        ScanRow::Event(source) => match job.phase {
            StorageLifetimePhaseV4::IndexSource => index_source_event(transaction, source),
            StorageLifetimePhaseV4::Receipts => {
                materialize_receipt_event(transaction, &source.event, materialized_at)
            }
            StorageLifetimePhaseV4::Messages => {
                materialize_message_event(transaction, source, materialized_at)
            }
            StorageLifetimePhaseV4::Workspaces => materialize_workspace_event(
                transaction,
                source,
                job.cutoff_event_rowid,
                materialized_at,
            ),
            StorageLifetimePhaseV4::Baselines => materialize_baseline_event(
                transaction,
                source,
                job.cutoff_event_rowid,
                materialized_at,
            ),
            StorageLifetimePhaseV4::VerifySource => {
                verify_source_event(transaction, source, job.cutoff_event_rowid)
            }
            _ => Err(materialization_error(
                "storage materialization phase is invalid",
            )),
        },
        ScanRow::Indexed(indexed) => {
            if job.phase != StorageLifetimePhaseV4::VerifySourceIndex {
                return Err(materialization_error(
                    "storage materialization phase is invalid",
                ));
            }
            verify_source_index_event(transaction, indexed)
        }
        ScanRow::Derived { key, .. } => {
            if matches!(
                job.phase,
                StorageLifetimePhaseV4::VerifyProjects
                    | StorageLifetimePhaseV4::VerifyChats
                    | StorageLifetimePhaseV4::VerifyRuns
                    | StorageLifetimePhaseV4::VerifyLocks
                    | StorageLifetimePhaseV4::VerifyAppInstances
            ) {
                verify_projection_row(transaction, job.phase, key, job.cutoff_event_rowid)
            } else {
                verify_derived_row(transaction, job.phase, key, job.cutoff_event_rowid)
            }
        }
    }
}

fn chat_index_state(source: &LedgerEventV1) -> Result<(Option<u64>, Option<String>), SidecarError> {
    if source.aggregate_type != "chat" {
        return Ok((None, None));
    }
    let payload = canonical_object(&source.canonical_json_payload, "chat source index")?;
    let durable = payload
        .get("durable_event_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| materialization_error("chat source durable sequence is invalid"))?;
    let snapshot = match payload.get("last_message") {
        Some(Value::Null) => None,
        Some(Value::Object(message)) => message
            .get("messageId")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => return Err(materialization_error("chat source snapshot is invalid")),
    };
    Ok((Some(durable), snapshot))
}

fn source_index_predecessor(
    connection: &Connection,
    source: &LedgerEventV1,
) -> Result<(Option<Value>, Option<SourceEvent>), SidecarError> {
    if source.aggregate_type != "chat" || source.aggregate_sequence == 1 {
        return Ok((None, None));
    }
    let predecessor = load_source_index_by_aggregate_sequence(
        connection,
        "chat",
        &source.aggregate_id,
        source.aggregate_sequence - 1,
    )?
    .ok_or_else(|| materialization_error("chat source index predecessor is missing"))?;
    let predecessor_rowid = predecessor
        .get("eventRowid")
        .and_then(Value::as_u64)
        .ok_or_else(|| materialization_error("chat source index predecessor is invalid"))?;
    let predecessor_source = load_exact_event_by_rowid(connection, predecessor_rowid)?
        .ok_or_else(|| materialization_error("chat source index predecessor source is missing"))?;
    Ok((Some(predecessor), Some(predecessor_source)))
}

fn expected_source_index_entry(
    connection: &Connection,
    source: &SourceEvent,
) -> Result<Value, SidecarError> {
    let (chat_durable_sequence, chat_snapshot_message_id) = chat_index_state(&source.event)?;
    let (predecessor, predecessor_source) = source_index_predecessor(connection, &source.event)?;
    let previous_durable = predecessor
        .as_ref()
        .and_then(|value| value.get("chatDurableSequence"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let introduced_message_id = chat_durable_sequence
        .filter(|current| previous_durable.checked_add(1) == Some(*current))
        .and_then(|_| chat_snapshot_message_id.clone());
    let entry = serde_json::json!({
        "eventRowid": source.rowid,
        "eventId": source.event.event_id,
        "operationId": source.event.operation_id,
        "aggregateType": source.event.aggregate_type,
        "aggregateId": source.event.aggregate_id,
        "aggregateSequence": source.event.aggregate_sequence,
        "introducedMessageId": introduced_message_id,
        "chatDurableSequence": chat_durable_sequence,
        "chatSnapshotMessageId": chat_snapshot_message_id,
    });
    validate_storage_event_source_index_transition_v1(
        source.rowid,
        &entry,
        &source.event,
        predecessor.as_ref(),
        predecessor_source.as_ref().map(|source| &source.event),
    )
    .map_err(|_| materialization_error("source index transition is invalid"))?;
    Ok(entry)
}

fn index_source_event(
    transaction: &Transaction<'_>,
    source: &SourceEvent,
) -> Result<(), SidecarError> {
    let expected = expected_source_index_entry(transaction, source)?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO event_source_index(\
               event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,\
               introduced_message_id,chat_durable_sequence,chat_snapshot_message_id\
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9);",
            params![
                sql_i64(source.rowid)?,
                source.event.event_id,
                source.event.operation_id,
                source.event.aggregate_type,
                source.event.aggregate_id,
                sql_i64(source.event.aggregate_sequence)?,
                expected.get("introducedMessageId").and_then(Value::as_str),
                expected
                    .get("chatDurableSequence")
                    .and_then(Value::as_u64)
                    .map(sql_i64)
                    .transpose()?,
                expected
                    .get("chatSnapshotMessageId")
                    .and_then(Value::as_str),
            ],
        )
        .map_err(sqlite_error)?;
    if load_source_index_by_rowid(transaction, source.rowid)? != Some(expected) {
        return Err(materialization_error(
            "source index row differs from its exact source event",
        ));
    }
    Ok(())
}

fn verify_source_index_event(
    transaction: &Transaction<'_>,
    indexed: &IndexedSourceEvent,
) -> Result<(), SidecarError> {
    let expected = expected_source_index_entry(transaction, &indexed.source)?;
    if indexed.entry != expected {
        return Err(materialization_error(
            "source index row differs from its exact source event",
        ));
    }
    Ok(())
}

fn verify_projection_row(
    connection: &Connection,
    phase: StorageLifetimePhaseV4,
    key: &str,
    cutoff: u64,
) -> Result<(), SidecarError> {
    let aggregate_type = match phase {
        StorageLifetimePhaseV4::VerifyProjects => "project",
        StorageLifetimePhaseV4::VerifyChats => "chat",
        StorageLifetimePhaseV4::VerifyRuns => "run",
        StorageLifetimePhaseV4::VerifyLocks => "lock",
        StorageLifetimePhaseV4::VerifyAppInstances => "app_instance",
        _ => {
            return Err(materialization_error(
                "projection verification phase is invalid",
            ))
        }
    };
    let source_index = load_latest_source_index(connection, aggregate_type, key, cutoff)?
        .ok_or_else(|| materialization_error("projection row has no latest frozen source"))?;
    let source = indexed_source_event(connection, source_index, cutoff)?;
    let canonical_source = source.event.canonical_json_payload;
    let sequence = source.event.aggregate_sequence;
    let payload = canonical_object(&canonical_source, "projection source")?;
    let exact = match phase {
        StorageLifetimePhaseV4::VerifyProjects => {
            let stored: Option<(String, String, u64)> = connection
                .query_row(
                    "SELECT canonical_path,projection_json,last_event_sequence FROM projects \
                     WHERE project_id=?1 LIMIT 1;",
                    [key],
                    |row| Ok((row.get(0)?, row.get(1)?, row_u64(row, 2)?)),
                )
                .optional()
                .map_err(sqlite_error)?;
            stored.is_some_and(|stored| {
                stored
                    == (
                        payload
                            .get("canonical_path")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        canonical_source.clone(),
                        sequence,
                    )
            })
        }
        StorageLifetimePhaseV4::VerifyChats => {
            let stored: Option<(String, String, u64)> = connection
                .query_row(
                    "SELECT project_id,projection_json,last_event_sequence FROM chats \
                     WHERE chat_id=?1 LIMIT 1;",
                    [key],
                    |row| Ok((row.get(0)?, row.get(1)?, row_u64(row, 2)?)),
                )
                .optional()
                .map_err(sqlite_error)?;
            stored.is_some_and(|stored| {
                stored
                    == (
                        payload
                            .get("project_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        canonical_source.clone(),
                        sequence,
                    )
            })
        }
        StorageLifetimePhaseV4::VerifyRuns => {
            type RunFacts = (Option<String>, String, Option<String>, String, u64);
            let stored: Option<RunFacts> = connection
                .query_row(
                    "SELECT chat_id,state,worktree_path,projection_json,last_event_sequence \
                     FROM runs WHERE run_id=?1 LIMIT 1;",
                    [key],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row_u64(row, 4)?,
                        ))
                    },
                )
                .optional()
                .map_err(sqlite_error)?;
            stored.is_some_and(|stored| {
                stored
                    == (
                        payload
                            .get("chat_id")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        payload
                            .get("state")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        payload
                            .get("worktree_path")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        canonical_source.clone(),
                        sequence,
                    )
            })
        }
        StorageLifetimePhaseV4::VerifyLocks => {
            type LockFacts = (
                Option<String>,
                Option<String>,
                String,
                i64,
                i64,
                String,
                String,
                u64,
            );
            let stored: Option<LockFacts> = connection
                .query_row(
                    "SELECT project_id,writer_run_id,worktree_path,poisoned,recovery_required,\
                     evidence_json,projection_json,last_event_sequence FROM locks \
                     WHERE lock_id=?1 LIMIT 1;",
                    [key],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row_u64(row, 7)?,
                        ))
                    },
                )
                .optional()
                .map_err(sqlite_error)?;
            let evidence = payload
                .get("evidence")
                .cloned()
                .ok_or_else(|| materialization_error("lock evidence is missing"))
                .and_then(|value| {
                    canonical_json_v1(value)
                        .map_err(|_| materialization_error("lock evidence is invalid"))
                })?;
            stored.is_some_and(|stored| {
                stored
                    == (
                        payload
                            .get("project_id")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        payload
                            .get("writer_run_id")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        payload
                            .get("worktree_path")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        i64::from(
                            payload
                                .get("poisoned")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        ),
                        i64::from(
                            payload
                                .get("recovery_required")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        ),
                        evidence,
                        canonical_source.clone(),
                        sequence,
                    )
            })
        }
        StorageLifetimePhaseV4::VerifyAppInstances => {
            type AppFacts = (u64, String, String, String, Option<String>, u64);
            let stored: Option<AppFacts> = connection
                .query_row(
                    "SELECT pid,platform,started_at,heartbeat_at,clean_shutdown_at,\
                     last_event_sequence FROM app_instances WHERE sidecar_instance_id=?1 LIMIT 1;",
                    [key],
                    |row| {
                        Ok((
                            row_u64(row, 0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row_u64(row, 5)?,
                        ))
                    },
                )
                .optional()
                .map_err(sqlite_error)?;
            stored.is_some_and(|stored| {
                stored
                    == (
                        payload.get("pid").and_then(Value::as_u64).unwrap_or(0),
                        payload
                            .get("platform")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        payload
                            .get("started_at")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        payload
                            .get("heartbeat_at")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        payload
                            .get("clean_shutdown_at")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        sequence,
                    )
            })
        }
        _ => false,
    };
    if exact {
        Ok(())
    } else {
        Err(materialization_error(
            "projection row differs from latest frozen source",
        ))
    }
}

fn verify_derived_row(
    connection: &Connection,
    phase: StorageLifetimePhaseV4,
    key: &str,
    cutoff: u64,
) -> Result<(), SidecarError> {
    let exact = match phase {
        StorageLifetimePhaseV4::VerifyReceipts => {
            let receipt = load_receipt(connection, key)?
                .ok_or_else(|| materialization_error("operation receipt is missing"))?;
            let source = load_source_index_by_aggregate_sequence(
                connection,
                &receipt.aggregate_type,
                &receipt.aggregate_id,
                receipt.aggregate_sequence,
            )?
            .map(|entry| indexed_source_event(connection, entry, cutoff))
            .transpose()?;
            source.is_some_and(|event| {
                classify_operation_against_receipt_v2(&receipt, &event.event)
                    .is_ok_and(|decision| decision == OperationDecisionV1::DuplicateNoOp)
            })
        }
        StorageLifetimePhaseV4::VerifyMessages => {
            let message = load_message(connection, key)?
                .ok_or_else(|| materialization_error("chat message is missing"))?;
            source_message_matches(connection, &message, cutoff)?
        }
        StorageLifetimePhaseV4::VerifyWorkspaces => {
            let workspace = load_workspace(connection, key)?
                .ok_or_else(|| materialization_error("prepared workspace is missing"))?;
            source_workspace_matches(connection, &workspace, cutoff)?
        }
        StorageLifetimePhaseV4::VerifyBaselines => {
            let baseline = load_baseline(connection, key)?
                .ok_or_else(|| materialization_error("projection baseline is missing"))?;
            source_baseline_matches(connection, &baseline, cutoff)?
        }
        _ => false,
    };
    if exact {
        Ok(())
    } else {
        Err(materialization_error(
            "materialized record has no exact frozen source",
        ))
    }
}

fn source_message_matches(
    connection: &Connection,
    message: &MaterializedMessage,
    cutoff: u64,
) -> Result<bool, SidecarError> {
    let source = load_source_index_by_introduced_message(connection, &message.message_id)?
        .map(|entry| indexed_source_event(connection, entry, cutoff))
        .transpose()?;
    Ok(source
        .as_ref()
        .and_then(|source| {
            canonical_object(&source.event.canonical_json_payload, "chat projection")
                .ok()
                .and_then(|object| object.get("last_message").cloned())
        })
        .and_then(|value| message_from_value(&value).ok())
        .is_some_and(|expected| expected == *message))
}

fn message_from_value(value: &Value) -> Result<MaterializedMessage, SidecarError> {
    validate_g5_payload_v1("teamMessage", value)
        .map_err(|_| materialization_error("chat message contract is invalid"))?;
    let object = value
        .as_object()
        .ok_or_else(|| materialization_error("chat message is not an object"))?;
    let canonical_json = canonical_json_v1(value.clone())
        .map_err(|_| materialization_error("chat message is not canonicalizable"))?;
    Ok(MaterializedMessage {
        message_id: required_json_string(object, "messageId", "chat message")?,
        chat_id: required_json_string(object, "chatId", "chat message")?,
        event_sequence: required_json_u64(object, "eventSequence", "chat message")?,
        digest: sha256_file_version_v1(canonical_json.as_bytes()),
        canonical_json,
        created_at: required_json_string(object, "createdAt", "chat message")?,
    })
}

fn source_workspace_matches(
    connection: &Connection,
    workspace: &MaterializedWorkspace,
    cutoff: u64,
) -> Result<bool, SidecarError> {
    let Some(entry) =
        load_latest_source_index(connection, "project", &workspace.project_id, cutoff)?
    else {
        return Ok(false);
    };
    let source = indexed_source_event(connection, entry, cutoff)?;
    let payload = validate_storage_lifetime_workspace_project_source_v1(
        &workspace.project_id,
        &source.event.aggregate_type,
        &source.event.aggregate_id,
        &source.event.canonical_json_payload,
    )
    .map_err(|_| materialization_error("prepared workspace project source is invalid"))?;
    let expected = payload
        .get("prepared_workspaces")
        .and_then(Value::as_object)
        .and_then(|records| records.get(&workspace.preparation_id))
        .map(|value| canonical_json_v1(value.clone()))
        .transpose()
        .map_err(|_| materialization_error("prepared workspace source is invalid"))?;
    Ok(
        expected.as_deref() == Some(workspace.canonical_json.as_str())
            && workspace.digest == sha256_file_version_v1(workspace.canonical_json.as_bytes()),
    )
}

fn source_baseline_matches(
    connection: &Connection,
    baseline: &MaterializedBaseline,
    cutoff: u64,
) -> Result<bool, SidecarError> {
    let source = load_source_index_by_aggregate_sequence(
        connection,
        &baseline.aggregate_type,
        &baseline.aggregate_id,
        baseline.cutoff_sequence,
    )?
    .map(|entry| indexed_source_event(connection, entry, cutoff))
    .transpose()?;
    Ok(baseline.baseline_id
        == baseline_id(
            &baseline.aggregate_type,
            &baseline.aggregate_id,
            baseline.cutoff_sequence,
        )
        && baseline.digest == sha256_file_version_v1(baseline.canonical_json.as_bytes())
        && source
            .as_ref()
            .map(|source| source.event.canonical_json_payload.as_str())
            == Some(baseline.canonical_json.as_str()))
}

fn materialize_receipt_event(
    transaction: &Transaction<'_>,
    event: &LedgerEventV1,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    let digest = digest_hex(event.canonical_json_payload.as_bytes())?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO operation_receipts(\
               operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,\
               payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,\
               canonical_payload_digest_version,canonical_payload_digest_format,\
               canonical_payload_digest,event_timestamp,materialized_at\
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14);",
            params![
                event.operation_id,
                event.event_id,
                event.aggregate_type,
                event.aggregate_id,
                sql_i64(event.aggregate_sequence)?,
                event.event_type,
                event.payload_schema_version,
                event.canonical_json_payload,
                OPERATION_RECEIPT_DIGEST_ALGORITHM_V2,
                OPERATION_RECEIPT_DIGEST_VERSION_V2,
                OPERATION_RECEIPT_DIGEST_FORMAT_V2,
                digest,
                event.timestamp,
                materialized_at,
            ],
        )
        .map_err(sqlite_error)?;
    let receipt = load_receipt(transaction, &event.operation_id)?
        .ok_or_else(|| materialization_error("operation receipt identity conflicted"))?;
    if classify_operation_against_receipt_v2(&receipt, event)
        .map_err(|_| materialization_error("operation receipt failed exact validation"))?
        != OperationDecisionV1::DuplicateNoOp
    {
        return Err(materialization_error(
            "operation receipt differs from immutable ledger content",
        ));
    }
    Ok(())
}

fn message_from_event(event: &LedgerEventV1) -> Result<Option<MaterializedMessage>, SidecarError> {
    if event.aggregate_type != "chat" {
        return Ok(None);
    }
    let payload = canonical_object(&event.canonical_json_payload, "chat projection")?;
    let Some(message) = payload.get("last_message").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    validate_g5_payload_v1("teamMessage", message)
        .map_err(|_| materialization_error("chat projection contains an invalid public message"))?;
    let object = message
        .as_object()
        .ok_or_else(|| materialization_error("public chat message is not an object"))?;
    let record = payload
        .get("record")
        .and_then(Value::as_object)
        .ok_or_else(|| materialization_error("chat projection record is missing"))?;
    let message_id = required_json_string(object, "messageId", "public chat message")?;
    let chat_id = required_json_string(object, "chatId", "public chat message")?;
    let event_sequence = required_json_u64(object, "eventSequence", "public chat message")?;
    if chat_id != event.aggregate_id
        || event_sequence
            != required_json_u64(&payload, "durable_event_sequence", "chat projection")?
        || required_json_string(record, "chatId", "chat record")? != chat_id
        || required_json_string(object, "taskId", "public chat message")?
            != required_json_string(record, "taskId", "chat record")?
        || required_json_string(object, "worktreeId", "public chat message")?
            != required_json_string(record, "worktreeId", "chat record")?
    {
        return Err(materialization_error(
            "public chat message identity or order differs from its projection",
        ));
    }
    let canonical_json = canonical_json_v1(message.clone())
        .map_err(|_| materialization_error("public chat message is not canonicalizable"))?;
    Ok(Some(MaterializedMessage {
        message_id,
        chat_id,
        event_sequence,
        digest: sha256_file_version_v1(canonical_json.as_bytes()),
        canonical_json,
        created_at: required_json_string(object, "createdAt", "public chat message")?,
    }))
}

fn materialize_message_event(
    transaction: &Transaction<'_>,
    source: &SourceEvent,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    let index = load_source_index_by_rowid(transaction, source.rowid)?
        .ok_or_else(|| materialization_error("chat message source index is missing"))?;
    let introduced_message_id = index.get("introducedMessageId").and_then(Value::as_str);
    if introduced_message_id.is_none() {
        return Ok(());
    }
    let message = message_from_event(&source.event)?
        .ok_or_else(|| materialization_error("introduced chat message is missing"))?;
    if introduced_message_id != Some(message.message_id.as_str()) {
        return Err(materialization_error(
            "introduced chat message identity differs from source index",
        ));
    }
    transaction
        .execute(
            "INSERT OR IGNORE INTO chat_messages(\
               message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at\
             ) VALUES (?1,?2,?3,?4,?5,?6,?7);",
            params![
                message.message_id,
                message.chat_id,
                sql_i64(message.event_sequence)?,
                message.canonical_json,
                message.digest,
                message.created_at,
                materialized_at,
            ],
        )
        .map_err(sqlite_error)?;
    if load_message(transaction, &message.message_id)? != Some(message) {
        return Err(materialization_error(
            "chat message differs from immutable public history",
        ));
    }
    Ok(())
}

fn workspace_records_from_event(
    connection: &Connection,
    event: &LedgerEventV1,
    cutoff: u64,
) -> Result<Vec<MaterializedWorkspace>, SidecarError> {
    if event.aggregate_type != "project" {
        return Ok(Vec::new());
    }
    let payload = canonical_object(&event.canonical_json_payload, "project projection")?;
    let Some(prepared) = payload.get("prepared_workspaces") else {
        return Ok(Vec::new());
    };
    let prepared = prepared.as_object().ok_or_else(|| {
        materialization_error("project prepared workspace projection is not an object")
    })?;
    let mut records = Vec::with_capacity(prepared.len());
    for (key, value) in prepared {
        validate_g5_payload_v1("preparedChatWorkspace", value)
            .map_err(|_| materialization_error("project contains an invalid prepared workspace"))?;
        let object = value
            .as_object()
            .ok_or_else(|| materialization_error("prepared workspace record is not an object"))?;
        let preparation_id = required_json_string(object, "preparationId", "prepared workspace")?;
        let manifest = object
            .get("manifest")
            .and_then(Value::as_object)
            .ok_or_else(|| materialization_error("prepared workspace manifest is missing"))?;
        let project_id = required_json_string(manifest, "projectId", "workspace manifest")?;
        let worktree_id = required_json_string(manifest, "worktreeId", "workspace manifest")?;
        let workspace_identity =
            required_json_string(manifest, "workspaceIdentity", "workspace manifest")?;
        let bound_chat_id = optional_json_string(object, "boundChatId", "prepared workspace")?;
        if key != &preparation_id || project_id != event.aggregate_id {
            return Err(materialization_error(
                "prepared workspace identity differs from its project projection",
            ));
        }
        if let Some(chat_id) = bound_chat_id.as_deref() {
            verify_bound_chat_snapshot(
                connection,
                cutoff,
                chat_id,
                &project_id,
                &worktree_id,
                &workspace_identity,
            )?;
        }
        let canonical_json = canonical_json_v1(value.clone())
            .map_err(|_| materialization_error("prepared workspace is not canonicalizable"))?;
        records.push(MaterializedWorkspace {
            preparation_id,
            project_id,
            worktree_id,
            workspace_identity,
            binding_state: required_json_string(object, "bindingState", "prepared workspace")?,
            bound_chat_id,
            digest: sha256_file_version_v1(canonical_json.as_bytes()),
            canonical_json,
            created_at: required_json_string(object, "createdAt", "prepared workspace")?,
            updated_at: required_json_string(object, "updatedAt", "prepared workspace")?,
        });
    }
    Ok(records)
}

fn verify_bound_chat_snapshot(
    connection: &Connection,
    cutoff: u64,
    chat_id: &str,
    project_id: &str,
    worktree_id: &str,
    workspace_identity: &str,
) -> Result<(), SidecarError> {
    let source_index = load_latest_source_index(connection, "chat", chat_id, cutoff)?
        .ok_or_else(|| materialization_error("bound prepared workspace chat source is missing"))?;
    let source = indexed_source_event(connection, source_index, cutoff)?;
    let chat = canonical_object(&source.event.canonical_json_payload, "chat projection")?;
    if required_json_string(&chat, "project_id", "chat projection")? != project_id
        || required_json_string(&chat, "worktree_id", "chat projection")? != worktree_id
        || required_json_string(&chat, "workspace_identity", "chat projection")?
            != workspace_identity
    {
        return Err(materialization_error(
            "bound prepared workspace differs from its frozen chat projection",
        ));
    }
    Ok(())
}

fn materialize_workspace_event(
    transaction: &Transaction<'_>,
    source: &SourceEvent,
    cutoff: u64,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    if source.event.aggregate_type != "project" {
        return Ok(());
    }
    let latest =
        load_latest_source_index(transaction, "project", &source.event.aggregate_id, cutoff)?
            .ok_or_else(|| materialization_error("project source index is missing"))?;
    if latest.get("eventRowid").and_then(Value::as_u64) != Some(source.rowid) {
        return Ok(());
    }
    for workspace in workspace_records_from_event(transaction, &source.event, cutoff)? {
        match load_workspace(transaction, &workspace.preparation_id)? {
            None => {
                transaction
                    .execute(
                        "INSERT INTO prepared_workspaces(\
                           preparation_id,project_id,worktree_id,workspace_identity,binding_state,bound_chat_id,\
                           canonical_record_json,record_digest,created_at,updated_at,materialized_at\
                         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11);",
                        params![
                            workspace.preparation_id,
                            workspace.project_id,
                            workspace.worktree_id,
                            workspace.workspace_identity,
                            workspace.binding_state,
                            workspace.bound_chat_id,
                            workspace.canonical_json,
                            workspace.digest,
                            workspace.created_at,
                            workspace.updated_at,
                            materialized_at,
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
            Some(existing) if existing == workspace => {}
            Some(existing) => {
                verify_workspace_source(transaction, &existing, cutoff)?;
                if existing.project_id != workspace.project_id
                    || existing.worktree_id != workspace.worktree_id
                    || existing.workspace_identity != workspace.workspace_identity
                    || existing.created_at != workspace.created_at
                    || existing.updated_at > workspace.updated_at
                {
                    return Err(materialization_error(
                        "prepared workspace immutable identity or ordering changed",
                    ));
                }
                transaction
                    .execute(
                        "UPDATE prepared_workspaces SET binding_state=?2,bound_chat_id=?3,\
                           canonical_record_json=?4,record_digest=?5,updated_at=?6,materialized_at=?7 \
                         WHERE preparation_id=?1;",
                        params![
                            workspace.preparation_id,
                            workspace.binding_state,
                            workspace.bound_chat_id,
                            workspace.canonical_json,
                            workspace.digest,
                            workspace.updated_at,
                            materialized_at,
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
        }
        if load_workspace(transaction, &workspace.preparation_id)? != Some(workspace) {
            return Err(materialization_error(
                "prepared workspace differs from its frozen projection",
            ));
        }
    }
    Ok(())
}

fn verify_workspace_source(
    connection: &Connection,
    workspace: &MaterializedWorkspace,
    cutoff: u64,
) -> Result<(), SidecarError> {
    if sha256_file_version_v1(workspace.canonical_json.as_bytes()) != workspace.digest {
        return Err(materialization_error(
            "materialized prepared workspace digest is invalid",
        ));
    }
    if !source_workspace_matches(connection, workspace, cutoff)? {
        return Err(materialization_error(
            "materialized prepared workspace has no frozen source event",
        ));
    }
    Ok(())
}

fn materialize_baseline_event(
    transaction: &Transaction<'_>,
    source: &SourceEvent,
    cutoff: u64,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    let event = &source.event;
    if !matches!(
        event.aggregate_type.as_str(),
        "project" | "chat" | "run" | "lock"
    ) {
        return Ok(());
    }
    let latest = load_latest_source_index(
        transaction,
        &event.aggregate_type,
        &event.aggregate_id,
        cutoff,
    )?
    .ok_or_else(|| materialization_error("baseline source index is missing"))?;
    if latest.get("eventRowid").and_then(Value::as_u64) != Some(source.rowid) {
        return Ok(());
    }
    let baseline = MaterializedBaseline {
        baseline_id: baseline_id(
            &event.aggregate_type,
            &event.aggregate_id,
            event.aggregate_sequence,
        ),
        aggregate_type: event.aggregate_type.clone(),
        aggregate_id: event.aggregate_id.clone(),
        cutoff_sequence: event.aggregate_sequence,
        canonical_json: event.canonical_json_payload.clone(),
        digest: sha256_file_version_v1(event.canonical_json_payload.as_bytes()),
    };
    transaction
        .execute(
            "INSERT OR IGNORE INTO ledger_baselines(\
               baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
               payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at\
             ) VALUES (?1,?2,?3,?4,1,?5,?6,?7,?7);",
            params![
                baseline.baseline_id,
                baseline.aggregate_type,
                baseline.aggregate_id,
                sql_i64(baseline.cutoff_sequence)?,
                baseline.canonical_json,
                baseline.digest,
                materialized_at,
            ],
        )
        .map_err(sqlite_error)?;
    if load_baseline(transaction, &baseline.baseline_id)? != Some(baseline) {
        return Err(materialization_error(
            "projection baseline differs from immutable source projection",
        ));
    }
    Ok(())
}

fn verify_source_event(
    transaction: &Transaction<'_>,
    source: &SourceEvent,
    cutoff: u64,
) -> Result<(), SidecarError> {
    let expected_index = expected_source_index_entry(transaction, source)?;
    let persisted_index = load_source_index_by_rowid(transaction, source.rowid)?
        .ok_or_else(|| materialization_error("source event index row is missing"))?;
    if persisted_index != expected_index {
        return Err(materialization_error("source event index row is different"));
    }
    if source.event.aggregate_sequence > 1 {
        let predecessor = load_source_index_by_aggregate_sequence(
            transaction,
            &source.event.aggregate_type,
            &source.event.aggregate_id,
            source.event.aggregate_sequence - 1,
        )?
        .ok_or_else(|| materialization_error("aggregate source predecessor is missing"))?;
        let predecessor_rowid = predecessor
            .get("eventRowid")
            .and_then(Value::as_u64)
            .ok_or_else(|| materialization_error("aggregate source predecessor is invalid"))?;
        if predecessor_rowid >= source.rowid {
            return Err(materialization_error(
                "aggregate source predecessor ordering is invalid",
            ));
        }
        let predecessor_source = load_exact_event_by_rowid(transaction, predecessor_rowid)?
            .ok_or_else(|| materialization_error("aggregate predecessor source is missing"))?;
        validate_storage_event_source_index_binding_v1(
            predecessor_rowid,
            &predecessor,
            &predecessor_source.event,
        )
        .map_err(|_| materialization_error("aggregate predecessor source is invalid"))?;
    }
    let receipt = load_receipt(transaction, &source.event.operation_id)?
        .ok_or_else(|| materialization_error("operation receipt is missing"))?;
    if classify_operation_against_receipt_v2(&receipt, &source.event)
        .map_err(|_| materialization_error("operation receipt failed exact validation"))?
        != OperationDecisionV1::DuplicateNoOp
    {
        return Err(materialization_error("operation receipt is not exact"));
    }
    let introduced_message_id = persisted_index
        .get("introducedMessageId")
        .and_then(Value::as_str);
    if let Some(message_id) = introduced_message_id {
        let message = message_from_event(&source.event)?
            .ok_or_else(|| materialization_error("introduced chat message is missing"))?;
        if message.message_id != message_id {
            return Err(materialization_error(
                "introduced chat message identity is different",
            ));
        }
        if load_message(transaction, &message.message_id)? != Some(message) {
            return Err(materialization_error(
                "chat message is missing or different",
            ));
        }
    }
    let latest = load_latest_source_index(
        transaction,
        &source.event.aggregate_type,
        &source.event.aggregate_id,
        cutoff,
    )?
    .ok_or_else(|| materialization_error("latest source index row is missing"))?;
    if latest.get("eventRowid").and_then(Value::as_u64) == Some(source.rowid) {
        require_projection_row(transaction, &source.event)?;
        if source.event.aggregate_type == "project" {
            for workspace in workspace_records_from_event(transaction, &source.event, cutoff)? {
                if load_workspace(transaction, &workspace.preparation_id)? != Some(workspace) {
                    return Err(materialization_error(
                        "prepared workspace is missing or different",
                    ));
                }
            }
        }
        if matches!(
            source.event.aggregate_type.as_str(),
            "project" | "chat" | "run" | "lock"
        ) {
            let id = baseline_id(
                &source.event.aggregate_type,
                &source.event.aggregate_id,
                source.event.aggregate_sequence,
            );
            let baseline = load_baseline(transaction, &id)?
                .ok_or_else(|| materialization_error("current projection baseline is missing"))?;
            if baseline.canonical_json != source.event.canonical_json_payload
                || baseline.digest
                    != sha256_file_version_v1(source.event.canonical_json_payload.as_bytes())
            {
                return Err(materialization_error(
                    "current projection baseline is different",
                ));
            }
        }
    }
    Ok(())
}

fn require_projection_row(
    connection: &Connection,
    event: &LedgerEventV1,
) -> Result<(), SidecarError> {
    let Some((table, id_column)) = (match event.aggregate_type.as_str() {
        "project" => Some(("projects", "project_id")),
        "chat" => Some(("chats", "chat_id")),
        "run" => Some(("runs", "run_id")),
        "lock" => Some(("locks", "lock_id")),
        "app_instance" => Some(("app_instances", "sidecar_instance_id")),
        _ => return Err(materialization_error("storage aggregate type is invalid")),
    }) else {
        return Ok(());
    };
    let sql = format!("SELECT rowid FROM {table} WHERE {id_column}=?1 LIMIT 1;");
    let stored: Option<u64> = connection
        .query_row(&sql, [&event.aggregate_id], |row| row_u64(row, 0))
        .optional()
        .map_err(sqlite_error)?;
    if stored.is_none() {
        return Err(materialization_error(
            "latest frozen source projection is missing",
        ));
    }
    Ok(())
}

fn finalize_materialization(
    connection: &mut Connection,
    cancellation: &mut dyn MaterializationCancellation,
) -> Result<LifetimeMaterializationBatchResult, SidecarError> {
    if cancellation.cancelled(MaterializationCancellationPoint::FinalBeforeBegin) {
        return Ok(LifetimeMaterializationBatchResult::Suspended {
            cursor: read_pending_cursor(connection)?,
        });
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    verify_bounded_storage_runtime_state(&transaction)?;
    let job = match load_materialization_pair(&transaction)? {
        MaterializationPair::Pending(job)
            if job.phase == StorageLifetimePhaseV4::VerifyBaselines && job.scan_exhausted =>
        {
            job
        }
        _ => {
            return Err(materialization_error(
                "storage materialization final publication cursor is invalid",
            ));
        }
    };
    let observed_cutoff = max_event_rowid(&transaction)?;
    if observed_cutoff < job.cutoff_event_rowid {
        return Err(materialization_error(
            "storage materialization cutoff moved backwards",
        ));
    }
    if cancellation.cancelled(MaterializationCancellationPoint::FinalBeforeMutation) {
        transaction.rollback().map_err(sqlite_error)?;
        return Ok(LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(job.cursor()),
        });
    }
    if observed_cutoff > job.cutoff_event_rowid {
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let changed = transaction
            .execute(
                "UPDATE lifetime_materialization_job SET cutoff_event_rowid=?2,phase='indexSource',\
                 after_scan_rowid=0,scan_exhausted=0,updated_at=?3 WHERE singleton=1 \
                 AND job_id=?1 AND state='pending' AND phase='verifyBaselines' \
                 AND scan_exhausted=1;",
                params![job.job_id, sql_i64(observed_cutoff)?, now],
            )
            .map_err(sqlite_error)?;
        if changed != 1 {
            return Err(materialization_error(
                "storage materialization catch-up reset changed unexpectedly",
            ));
        }
        if cancellation.cancelled(MaterializationCancellationPoint::FinalImmediatelyPrecommit) {
            transaction.rollback().map_err(sqlite_error)?;
            return Ok(LifetimeMaterializationBatchResult::Suspended {
                cursor: Some(job.cursor()),
            });
        }
        transaction.commit().map_err(sqlite_error)?;
        return Ok(LifetimeMaterializationBatchResult::Progress {
            cursor: LifetimeMaterializationCursorFacts {
                cutoff_event_rowid: observed_cutoff,
                phase: StorageLifetimePhaseV4::IndexSource,
                after_scan_rowid: 0,
                scan_exhausted: false,
            },
            source_rows: 0,
            source_payload_bytes: 0,
        });
    }

    let deleted = transaction
        .execute(
            "DELETE FROM lifetime_materialization_job WHERE singleton=1 AND job_id=?1 \
             AND state='pending' AND phase='verifyBaselines' AND scan_exhausted=1;",
            [&job.job_id],
        )
        .map_err(sqlite_error)?;
    let published = transaction
        .execute(
            "UPDATE storage_schema_meta SET materialization_state='ready' \
             WHERE singleton=1 AND materialization_state='pending';",
            [],
        )
        .map_err(sqlite_error)?;
    if deleted != 1 || published != 1 {
        return Err(materialization_error(
            "storage materialization ready publication changed unexpectedly",
        ));
    }
    if cancellation.cancelled(MaterializationCancellationPoint::FinalImmediatelyPrecommit) {
        transaction.rollback().map_err(sqlite_error)?;
        return Ok(LifetimeMaterializationBatchResult::Suspended {
            cursor: Some(job.cursor()),
        });
    }
    transaction.commit().map_err(sqlite_error)?;
    Ok(LifetimeMaterializationBatchResult::Completed {
        cutoff_event_rowid: observed_cutoff,
    })
}

pub(crate) fn materialize_lifetime_records(
    connection: &mut Connection,
    failure: Option<MaterializationFailurePoint>,
) -> Result<LifetimeMaterializationResult, SidecarError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let result = materialize_in_transaction(&transaction, failure)?;
    transaction.commit().map_err(sqlite_error)?;
    Ok(result)
}

fn materialize_in_transaction(
    transaction: &Transaction<'_>,
    failure: Option<MaterializationFailurePoint>,
) -> Result<LifetimeMaterializationResult, SidecarError> {
    let storage_schema_version: u32 = transaction
        .query_row(
            "SELECT storage_schema_version FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if storage_schema_version >= STORAGE_LIFETIME_SCHEMA_VERSION_V6 {
        return Err(materialization_error(
            "legacy storage materialization is disabled for bounded v6 storage",
        ));
    }
    let lifetime_job_present: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM lifetime_materialization_job WHERE singleton=1);",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if lifetime_job_present {
        return Err(materialization_error(
            "legacy storage materialization cannot run while bounded materialization state exists",
        ));
    }
    let state: String = transaction
        .query_row(
            "SELECT materialization_state FROM storage_schema_meta WHERE singleton=1;",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if !matches!(state.as_str(), "pending" | "ready") {
        return Err(materialization_error(
            "storage materialization metadata is not restart-safe",
        ));
    }
    let changed = transaction
        .execute(
            "UPDATE storage_schema_meta SET materialization_state='materializing' \
             WHERE singleton=1 AND materialization_state IN ('pending','ready');",
            [],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(materialization_error(
            "storage materialization metadata changed unexpectedly",
        ));
    }

    let cutoff: i64 = transaction
        .query_row("SELECT coalesce(max(rowid), 0) FROM events;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let cutoff_event_rowid = u64::try_from(cutoff)
        .map_err(|_| materialization_error("storage event cutoff is invalid"))?;
    let events = crate::storage::load_events(transaction)?;
    let projections = crate::storage::load_projection(transaction)?;
    crate::storage::verify_projection_replay(transaction)?;

    let materialized_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    materialize_receipts(transaction, &events, &materialized_at)?;
    if failure == Some(MaterializationFailurePoint::Receipts) {
        return Err(injected_failure("operation receipts"));
    }

    materialize_messages(transaction, &events, &projections, &materialized_at)?;
    materialize_workspaces(transaction, &events, &projections, &materialized_at)?;
    if failure == Some(MaterializationFailurePoint::MessagesAndWorkspaces) {
        return Err(injected_failure("messages and workspaces"));
    }

    materialize_baselines(transaction, &events, &projections, &materialized_at)?;
    verify_materialization(transaction, &events, &projections)?;
    if failure == Some(MaterializationFailurePoint::BaselinesBeforeMetadata) {
        return Err(injected_failure("ledger baselines"));
    }

    let changed = transaction
        .execute(
            "UPDATE storage_schema_meta SET materialization_state='ready' \
             WHERE singleton=1 AND materialization_state='materializing';",
            [],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(materialization_error(
            "storage materialization could not publish ready atomically",
        ));
    }

    Ok(LifetimeMaterializationResult {
        cutoff_event_rowid,
        operation_receipts: table_count(transaction, "operation_receipts")?,
        chat_messages: table_count(transaction, "chat_messages")?,
        prepared_workspaces: table_count(transaction, "prepared_workspaces")?,
        ledger_baselines: table_count(transaction, "ledger_baselines")?,
    })
}

fn materialize_receipts(
    transaction: &Transaction<'_>,
    events: &[LedgerEventV1],
    materialized_at: &str,
) -> Result<(), SidecarError> {
    for event in events {
        let digest = digest_hex(event.canonical_json_payload.as_bytes())?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO operation_receipts(\
                   operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,\
                   payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,\
                   canonical_payload_digest_version,canonical_payload_digest_format,\
                   canonical_payload_digest,event_timestamp,materialized_at\
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14);",
                params![
                    event.operation_id,
                    event.event_id,
                    event.aggregate_type,
                    event.aggregate_id,
                    sql_i64(event.aggregate_sequence)?,
                    event.event_type,
                    event.payload_schema_version,
                    event.canonical_json_payload,
                    OPERATION_RECEIPT_DIGEST_ALGORITHM_V2,
                    OPERATION_RECEIPT_DIGEST_VERSION_V2,
                    OPERATION_RECEIPT_DIGEST_FORMAT_V2,
                    digest,
                    event.timestamp,
                    materialized_at,
                ],
            )
            .map_err(sqlite_error)?;
        let receipt = load_receipt(transaction, &event.operation_id)?
            .ok_or_else(|| materialization_error("operation receipt identity conflicted"))?;
        if classify_operation_against_receipt_v2(&receipt, event)
            .map_err(|_| materialization_error("operation receipt failed exact validation"))?
            != OperationDecisionV1::DuplicateNoOp
        {
            return Err(materialization_error(
                "operation receipt differs from immutable ledger content",
            ));
        }
    }
    if table_count(transaction, "operation_receipts")? != events.len() as u64 {
        return Err(materialization_error(
            "operation receipt set contains unknown or missing records",
        ));
    }
    Ok(())
}

fn load_receipt(
    connection: &Connection,
    operation_id: &str,
) -> Result<Option<OperationReceiptV2>, SidecarError> {
    connection
        .query_row(
            "SELECT operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,\
                    payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,\
                    canonical_payload_digest_version,canonical_payload_digest_format,\
                    canonical_payload_digest,event_timestamp \
             FROM operation_receipts WHERE operation_id=?1;",
            [operation_id],
            |row| {
                Ok(OperationReceiptV2 {
                    operation_id: row.get(0)?,
                    event_id: row.get(1)?,
                    aggregate_type: row.get(2)?,
                    aggregate_id: row.get(3)?,
                    aggregate_sequence: row_u64(row, 4)?,
                    event_type: row.get(5)?,
                    payload_schema_version: row.get(6)?,
                    canonical_payload_json: row.get(7)?,
                    canonical_payload_digest_algorithm: row.get(8)?,
                    canonical_payload_digest_version: row.get(9)?,
                    canonical_payload_digest_format: row.get(10)?,
                    canonical_payload_digest: row.get(11)?,
                    event_timestamp: row.get(12)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn materialize_messages(
    transaction: &Transaction<'_>,
    events: &[LedgerEventV1],
    projections: &ProjectionStateV1,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    let expected = expected_messages(events, projections)?;
    for message in expected.values() {
        transaction
            .execute(
                "INSERT OR IGNORE INTO chat_messages(\
                   message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at\
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7);",
                params![
                    message.message_id,
                    message.chat_id,
                    sql_i64(message.event_sequence)?,
                    message.canonical_json,
                    message.digest,
                    message.created_at,
                    materialized_at,
                ],
            )
            .map_err(sqlite_error)?;
        let stored = load_message(transaction, &message.message_id)?
            .ok_or_else(|| materialization_error("chat message identity conflicted"))?;
        if &stored != message {
            return Err(materialization_error(
                "chat message differs from immutable public history",
            ));
        }
    }
    if table_count(transaction, "chat_messages")? != expected.len() as u64 {
        return Err(materialization_error(
            "chat message set contains unknown or missing records",
        ));
    }
    Ok(())
}

fn expected_messages(
    events: &[LedgerEventV1],
    projections: &ProjectionStateV1,
) -> Result<BTreeMap<String, MaterializedMessage>, SidecarError> {
    let mut expected = BTreeMap::new();
    let mut sequences = BTreeMap::<String, BTreeSet<u64>>::new();
    for event in events.iter().filter(|event| event.aggregate_type == "chat") {
        let payload = canonical_object(&event.canonical_json_payload, "chat projection")?;
        let Some(message) = payload.get("last_message").filter(|value| !value.is_null()) else {
            continue;
        };
        validate_g5_payload_v1("teamMessage", message).map_err(|_| {
            materialization_error("chat projection contains an invalid public message")
        })?;
        let object = message
            .as_object()
            .ok_or_else(|| materialization_error("public chat message is not an object"))?;
        let chat_record = payload
            .get("record")
            .ok_or_else(|| materialization_error("chat projection record is missing"))?;
        validate_g5_payload_v1("chatRecord", chat_record)
            .map_err(|_| materialization_error("chat projection record is invalid"))?;
        let chat_record = chat_record
            .as_object()
            .ok_or_else(|| materialization_error("chat projection record is not an object"))?;
        let message_id = required_json_string(object, "messageId", "public chat message")?;
        let chat_id = required_json_string(object, "chatId", "public chat message")?;
        let event_sequence = required_json_u64(object, "eventSequence", "public chat message")?;
        let durable_sequence =
            required_json_u64(&payload, "durable_event_sequence", "chat projection")?;
        if chat_id != event.aggregate_id
            || event_sequence != durable_sequence
            || required_json_string(chat_record, "chatId", "chat record")? != chat_id
            || required_json_string(object, "taskId", "public chat message")?
                != required_json_string(chat_record, "taskId", "chat record")?
            || required_json_string(object, "worktreeId", "public chat message")?
                != required_json_string(chat_record, "worktreeId", "chat record")?
        {
            return Err(materialization_error(
                "public chat message identity or order differs from its projection",
            ));
        }
        if !sequences
            .entry(chat_id.clone())
            .or_default()
            .insert(event_sequence)
        {
            return Err(materialization_error(
                "public chat message order is not unique",
            ));
        }
        let canonical_json = canonical_json_v1(message.clone())
            .map_err(|_| materialization_error("public chat message is not canonicalizable"))?;
        let record = MaterializedMessage {
            message_id: message_id.clone(),
            chat_id,
            event_sequence,
            digest: sha256_file_version_v1(canonical_json.as_bytes()),
            canonical_json,
            created_at: required_json_string(object, "createdAt", "public chat message")?,
        };
        if expected.insert(message_id, record).is_some() {
            return Err(materialization_error(
                "public chat message identity is not unique",
            ));
        }
    }
    for (chat_id, projection) in &projections.chats {
        let durable_sequence =
            required_json_u64(projection, "durable_event_sequence", "chat projection")?;
        let chat_sequences = sequences.get(chat_id);
        if durable_sequence == 0
            || chat_sequences.map(BTreeSet::len).unwrap_or(0) != durable_sequence as usize
            || chat_sequences.is_some_and(|values| values.iter().copied().ne(1..=durable_sequence))
        {
            return Err(materialization_error(
                "public chat message history is not contiguous",
            ));
        }
    }
    Ok(expected)
}

fn load_message(
    connection: &Connection,
    message_id: &str,
) -> Result<Option<MaterializedMessage>, SidecarError> {
    connection
        .query_row(
            "SELECT message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at \
             FROM chat_messages WHERE message_id=?1;",
            [message_id],
            |row| {
                Ok(MaterializedMessage {
                    message_id: row.get(0)?,
                    chat_id: row.get(1)?,
                    event_sequence: row_u64(row, 2)?,
                    canonical_json: row.get(3)?,
                    digest: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn materialize_workspaces(
    transaction: &Transaction<'_>,
    events: &[LedgerEventV1],
    projections: &ProjectionStateV1,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    let expected = expected_workspaces(projections)?;
    for workspace in expected.values() {
        let existing = load_workspace(transaction, &workspace.preparation_id)?;
        match existing {
            None => {
                transaction
                    .execute(
                        "INSERT INTO prepared_workspaces(\
                           preparation_id,project_id,worktree_id,workspace_identity,binding_state,bound_chat_id,\
                           canonical_record_json,record_digest,created_at,updated_at,materialized_at\
                         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11);",
                        params![
                            workspace.preparation_id,
                            workspace.project_id,
                            workspace.worktree_id,
                            workspace.workspace_identity,
                            workspace.binding_state,
                            workspace.bound_chat_id,
                            workspace.canonical_json,
                            workspace.digest,
                            workspace.created_at,
                            workspace.updated_at,
                            materialized_at,
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
            Some(existing) if existing == *workspace => {}
            Some(existing) => {
                verify_historical_workspace(&existing, events)?;
                if existing.preparation_id != workspace.preparation_id
                    || existing.project_id != workspace.project_id
                    || existing.worktree_id != workspace.worktree_id
                    || existing.workspace_identity != workspace.workspace_identity
                    || existing.created_at != workspace.created_at
                    || existing.updated_at > workspace.updated_at
                {
                    return Err(materialization_error(
                        "prepared workspace immutable identity or ordering changed",
                    ));
                }
                transaction
                    .execute(
                        "UPDATE prepared_workspaces SET binding_state=?2,bound_chat_id=?3,\
                           canonical_record_json=?4,record_digest=?5,updated_at=?6,materialized_at=?7 \
                         WHERE preparation_id=?1;",
                        params![
                            workspace.preparation_id,
                            workspace.binding_state,
                            workspace.bound_chat_id,
                            workspace.canonical_json,
                            workspace.digest,
                            workspace.updated_at,
                            materialized_at,
                        ],
                    )
                    .map_err(sqlite_error)?;
            }
        }
        let stored = load_workspace(transaction, &workspace.preparation_id)?
            .ok_or_else(|| materialization_error("prepared workspace was not materialized"))?;
        if stored != *workspace {
            return Err(materialization_error(
                "prepared workspace differs from its durable projection",
            ));
        }
    }
    if table_count(transaction, "prepared_workspaces")? != expected.len() as u64 {
        return Err(materialization_error(
            "prepared workspace set contains unknown or missing records",
        ));
    }
    Ok(())
}

fn expected_workspaces(
    projections: &ProjectionStateV1,
) -> Result<BTreeMap<String, MaterializedWorkspace>, SidecarError> {
    let mut expected = BTreeMap::new();
    for (project_id, project) in &projections.projects {
        let Some(prepared) = project.get("prepared_workspaces") else {
            continue;
        };
        let prepared = prepared.as_object().ok_or_else(|| {
            materialization_error("project prepared workspace projection is not an object")
        })?;
        for (key, value) in prepared {
            validate_g5_payload_v1("preparedChatWorkspace", value).map_err(|_| {
                materialization_error("project contains an invalid prepared workspace")
            })?;
            let object = value.as_object().ok_or_else(|| {
                materialization_error("prepared workspace record is not an object")
            })?;
            let preparation_id =
                required_json_string(object, "preparationId", "prepared workspace")?;
            let manifest = object
                .get("manifest")
                .and_then(Value::as_object)
                .ok_or_else(|| materialization_error("prepared workspace manifest is missing"))?;
            let manifest_project =
                required_json_string(manifest, "projectId", "prepared workspace manifest")?;
            let worktree_id =
                required_json_string(manifest, "worktreeId", "prepared workspace manifest")?;
            let workspace_identity =
                required_json_string(manifest, "workspaceIdentity", "prepared workspace manifest")?;
            let binding_state = required_json_string(object, "bindingState", "prepared workspace")?;
            let bound_chat_id = optional_json_string(object, "boundChatId", "prepared workspace")?;
            if key != &preparation_id || manifest_project != *project_id {
                return Err(materialization_error(
                    "prepared workspace identity differs from its project projection",
                ));
            }
            if let Some(chat_id) = &bound_chat_id {
                let chat = projections.chats.get(chat_id).ok_or_else(|| {
                    materialization_error("bound prepared workspace chat is missing")
                })?;
                if required_json_string(chat, "project_id", "chat projection")? != *project_id
                    || required_json_string(chat, "worktree_id", "chat projection")? != worktree_id
                    || required_json_string(chat, "workspace_identity", "chat projection")?
                        != workspace_identity
                {
                    return Err(materialization_error(
                        "bound prepared workspace differs from its chat projection",
                    ));
                }
            }
            let canonical_json = canonical_json_v1(value.clone())
                .map_err(|_| materialization_error("prepared workspace is not canonicalizable"))?;
            let record = MaterializedWorkspace {
                preparation_id: preparation_id.clone(),
                project_id: project_id.clone(),
                worktree_id,
                workspace_identity,
                binding_state,
                bound_chat_id,
                digest: sha256_file_version_v1(canonical_json.as_bytes()),
                canonical_json,
                created_at: required_json_string(object, "createdAt", "prepared workspace")?,
                updated_at: required_json_string(object, "updatedAt", "prepared workspace")?,
            };
            if expected.insert(preparation_id, record).is_some() {
                return Err(materialization_error(
                    "prepared workspace identity is not globally unique",
                ));
            }
        }
    }
    Ok(expected)
}

fn verify_historical_workspace(
    workspace: &MaterializedWorkspace,
    events: &[LedgerEventV1],
) -> Result<(), SidecarError> {
    let value: Value = serde_json::from_str(&workspace.canonical_json)
        .map_err(|_| materialization_error("materialized prepared workspace JSON is invalid"))?;
    validate_g5_payload_v1("preparedChatWorkspace", &value).map_err(|_| {
        materialization_error("materialized prepared workspace violates its contract")
    })?;
    if sha256_file_version_v1(workspace.canonical_json.as_bytes()) != workspace.digest {
        return Err(materialization_error(
            "materialized prepared workspace digest is invalid",
        ));
    }
    let appeared = events.iter().any(|event| {
        if event.aggregate_type != "project" || event.aggregate_id != workspace.project_id {
            return false;
        }
        serde_json::from_str::<Value>(&event.canonical_json_payload)
            .ok()
            .and_then(|payload| payload.get("prepared_workspaces").cloned())
            .and_then(|prepared| prepared.get(&workspace.preparation_id).cloned())
            .is_some_and(|record| record == value)
    });
    if !appeared {
        return Err(materialization_error(
            "materialized prepared workspace has no durable source event",
        ));
    }
    Ok(())
}

fn load_workspace(
    connection: &Connection,
    preparation_id: &str,
) -> Result<Option<MaterializedWorkspace>, SidecarError> {
    connection
        .query_row(
            "SELECT preparation_id,project_id,worktree_id,workspace_identity,binding_state,\
                    bound_chat_id,canonical_record_json,record_digest,created_at,updated_at \
             FROM prepared_workspaces WHERE preparation_id=?1;",
            [preparation_id],
            |row| {
                Ok(MaterializedWorkspace {
                    preparation_id: row.get(0)?,
                    project_id: row.get(1)?,
                    worktree_id: row.get(2)?,
                    workspace_identity: row.get(3)?,
                    binding_state: row.get(4)?,
                    bound_chat_id: row.get(5)?,
                    canonical_json: row.get(6)?,
                    digest: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn materialize_baselines(
    transaction: &Transaction<'_>,
    events: &[LedgerEventV1],
    projections: &ProjectionStateV1,
    materialized_at: &str,
) -> Result<(), SidecarError> {
    for (aggregate_type, records) in projection_tables(projections) {
        for (aggregate_id, record) in records {
            let cutoff_sequence = required_json_u64(record, "last_event_sequence", "projection")?;
            let canonical_json = canonical_projection(record)?;
            let source = events.iter().find(|event| {
                event.aggregate_type == aggregate_type
                    && event.aggregate_id == *aggregate_id
                    && event.aggregate_sequence == cutoff_sequence
            });
            if !source.is_some_and(|event| event.canonical_json_payload == canonical_json) {
                return Err(materialization_error(
                    "projection baseline has no exact durable source event",
                ));
            }
            let baseline = MaterializedBaseline {
                baseline_id: baseline_id(aggregate_type, aggregate_id, cutoff_sequence),
                aggregate_type: aggregate_type.to_string(),
                aggregate_id: aggregate_id.clone(),
                cutoff_sequence,
                digest: sha256_file_version_v1(canonical_json.as_bytes()),
                canonical_json,
            };
            transaction
                .execute(
                    "INSERT OR IGNORE INTO ledger_baselines(\
                       baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
                       payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at\
                     ) VALUES (?1,?2,?3,?4,1,?5,?6,?7,?7);",
                    params![
                        baseline.baseline_id,
                        baseline.aggregate_type,
                        baseline.aggregate_id,
                        sql_i64(baseline.cutoff_sequence)?,
                        baseline.canonical_json,
                        baseline.digest,
                        materialized_at,
                    ],
                )
                .map_err(sqlite_error)?;
            let stored = load_baseline(transaction, &baseline.baseline_id)?
                .ok_or_else(|| materialization_error("projection baseline identity conflicted"))?;
            if stored != baseline {
                return Err(materialization_error(
                    "projection baseline differs from immutable source projection",
                ));
            }
        }
    }
    verify_all_baselines(transaction, events, projections)
}

fn verify_all_baselines(
    connection: &Connection,
    events: &[LedgerEventV1],
    projections: &ProjectionStateV1,
) -> Result<(), SidecarError> {
    let mut statement = connection
        .prepare(
            "SELECT baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
                    canonical_projection_json,projection_digest FROM ledger_baselines \
             ORDER BY aggregate_type,aggregate_id,cutoff_aggregate_sequence;",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(MaterializedBaseline {
                baseline_id: row.get(0)?,
                aggregate_type: row.get(1)?,
                aggregate_id: row.get(2)?,
                cutoff_sequence: row_u64(row, 3)?,
                canonical_json: row.get(4)?,
                digest: row.get(5)?,
            })
        })
        .map_err(sqlite_error)?;
    for row in rows {
        let baseline = row.map_err(sqlite_error)?;
        if baseline.baseline_id
            != baseline_id(
                &baseline.aggregate_type,
                &baseline.aggregate_id,
                baseline.cutoff_sequence,
            )
            || sha256_file_version_v1(baseline.canonical_json.as_bytes()) != baseline.digest
        {
            return Err(materialization_error(
                "projection baseline identity or digest is invalid",
            ));
        }
        let source = events.iter().find(|event| {
            event.aggregate_type == baseline.aggregate_type
                && event.aggregate_id == baseline.aggregate_id
                && event.aggregate_sequence == baseline.cutoff_sequence
        });
        if !source.is_some_and(|event| event.canonical_json_payload == baseline.canonical_json) {
            return Err(materialization_error(
                "projection baseline source event is missing or different",
            ));
        }
        let mut replayed = canonical_object(&baseline.canonical_json, "projection baseline")?;
        let mut sequence = baseline.cutoff_sequence;
        for event in events.iter().filter(|event| {
            event.aggregate_type == baseline.aggregate_type
                && event.aggregate_id == baseline.aggregate_id
                && event.aggregate_sequence > baseline.cutoff_sequence
        }) {
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| materialization_error("projection baseline sequence overflowed"))?;
            if event.aggregate_sequence != sequence {
                return Err(materialization_error(
                    "projection baseline tail is not contiguous",
                ));
            }
            replayed = canonical_object(&event.canonical_json_payload, "projection tail event")?;
        }
        replayed.insert("last_event_sequence".to_string(), Value::from(sequence));
        let current = projection_record(
            projections,
            &baseline.aggregate_type,
            &baseline.aggregate_id,
        )?;
        if current != &replayed {
            return Err(materialization_error(
                "projection baseline and ledger tail do not reproduce current state",
            ));
        }
    }
    Ok(())
}

fn load_baseline(
    connection: &Connection,
    baseline_id: &str,
) -> Result<Option<MaterializedBaseline>, SidecarError> {
    connection
        .query_row(
            "SELECT baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
                    canonical_projection_json,projection_digest \
             FROM ledger_baselines WHERE baseline_id=?1;",
            [baseline_id],
            |row| {
                Ok(MaterializedBaseline {
                    baseline_id: row.get(0)?,
                    aggregate_type: row.get(1)?,
                    aggregate_id: row.get(2)?,
                    cutoff_sequence: row_u64(row, 3)?,
                    canonical_json: row.get(4)?,
                    digest: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn verify_materialization(
    transaction: &Transaction<'_>,
    events_before: &[LedgerEventV1],
    projections_before: &ProjectionStateV1,
) -> Result<(), SidecarError> {
    crate::storage::verify_projection_replay(transaction)?;
    if crate::storage::load_events(transaction)? != events_before
        || crate::storage::load_projection(transaction)? != *projections_before
    {
        return Err(materialization_error(
            "materialization changed ledger, namespace, recovery, or projection facts",
        ));
    }
    for event in events_before {
        let receipt = load_receipt(transaction, &event.operation_id)?
            .ok_or_else(|| materialization_error("operation receipt is missing"))?;
        let legacy_exact = classify_operation_v1(events_before, event)
            .map_err(|_| materialization_error("legacy operation classification failed"))?;
        let receipt_exact = classify_operation_against_receipt_v2(&receipt, event)
            .map_err(|_| materialization_error("receipt operation classification failed"))?;
        let mut conflict = event.clone();
        conflict.event_id.push_str("_conflict_probe");
        let legacy_conflict = classify_operation_v1(events_before, &conflict)
            .map_err(|_| materialization_error("legacy conflict classification failed"))?;
        let receipt_conflict = classify_operation_against_receipt_v2(&receipt, &conflict)
            .map_err(|_| materialization_error("receipt conflict classification failed"))?;
        if legacy_exact != OperationDecisionV1::DuplicateNoOp
            || legacy_exact != receipt_exact
            || legacy_conflict != OperationDecisionV1::Conflict
            || legacy_conflict != receipt_conflict
        {
            return Err(materialization_error(
                "operation duplicate or conflict semantics changed",
            ));
        }
    }
    verify_all_baselines(transaction, events_before, projections_before)
}

fn projection_tables(projection: &ProjectionStateV1) -> [ProjectionTable<'_>; 4] {
    [
        ("project", &projection.projects),
        ("chat", &projection.chats),
        ("run", &projection.runs),
        ("lock", &projection.locks),
    ]
}

fn projection_record<'a>(
    projection: &'a ProjectionStateV1,
    aggregate_type: &str,
    aggregate_id: &str,
) -> Result<&'a Map<String, Value>, SidecarError> {
    let records = match aggregate_type {
        "project" => &projection.projects,
        "chat" => &projection.chats,
        "run" => &projection.runs,
        "lock" => &projection.locks,
        _ => {
            return Err(materialization_error(
                "projection baseline aggregate type is unsupported",
            ))
        }
    };
    records
        .get(aggregate_id)
        .ok_or_else(|| materialization_error("projection baseline aggregate is missing"))
}

fn canonical_projection(record: &Map<String, Value>) -> Result<String, SidecarError> {
    let mut value = record.clone();
    value.remove("last_event_sequence");
    canonical_json_v1(Value::Object(value))
        .map_err(|_| materialization_error("projection could not be canonicalized"))
}

fn canonical_object(source: &str, label: &str) -> Result<Map<String, Value>, SidecarError> {
    let value: Value = serde_json::from_str(source)
        .map_err(|_| materialization_error(format!("{label} JSON is invalid")))?;
    let canonical = canonical_json_v1(value.clone())
        .map_err(|_| materialization_error(format!("{label} could not be canonicalized")))?;
    if canonical != source {
        return Err(materialization_error(format!(
            "{label} JSON is not canonical"
        )));
    }
    value
        .as_object()
        .cloned()
        .ok_or_else(|| materialization_error(format!("{label} is not an object")))
}

fn baseline_id(aggregate_type: &str, aggregate_id: &str, sequence: u64) -> String {
    let source = format!("{aggregate_type}\0{aggregate_id}\0{sequence}");
    format!("baseline_{}", digest_hex_unchecked(source.as_bytes()))
}

fn digest_hex(bytes: &[u8]) -> Result<String, SidecarError> {
    sha256_file_version_v1(bytes)
        .strip_prefix("sha256:")
        .map(str::to_string)
        .ok_or_else(|| materialization_error("SHA-256 digest encoding is invalid"))
}

fn digest_hex_unchecked(bytes: &[u8]) -> String {
    sha256_file_version_v1(bytes)
        .strip_prefix("sha256:")
        .expect("protocol SHA-256 digest prefix")
        .to_string()
}

fn required_json_string(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<String, SidecarError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| materialization_error(format!("{label} {field} is invalid")))
}

fn optional_json_string(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<Option<String>, SidecarError> {
    match object.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
        _ => Err(materialization_error(format!("{label} {field} is invalid"))),
    }
}

fn required_json_u64(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<u64, SidecarError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| materialization_error(format!("{label} {field} is invalid")))
}

fn table_count(connection: &Connection, table: &str) -> Result<u64, SidecarError> {
    let sql = format!("SELECT count(*) FROM {table};");
    let count: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(sqlite_error)?;
    u64::try_from(count).map_err(|_| materialization_error("materialized row count is invalid"))
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
    i64::try_from(value)
        .map_err(|_| materialization_error("protocol integer cannot be represented by SQLite"))
}

fn injected_failure(stage: &str) -> SidecarError {
    SidecarError::new(
        "injectedFailure",
        format!("failure injected after {stage} materialization"),
    )
}

fn materialization_error(detail: impl Into<String>) -> SidecarError {
    SidecarError::new("storageMaterializationFailed", detail.into())
}

pub(crate) fn collect_status(
    connection: &Connection,
    database_path: &Path,
) -> Result<StorageStatusV1, SidecarError> {
    let page_size = pragma_u64(connection, "page_size")?;
    let page_count = pragma_u64(connection, "page_count")?;
    let logical_bytes = page_size.checked_mul(page_count).ok_or_else(|| {
        SidecarError::new(
            "storageStatusUnavailable",
            "database logical byte count overflowed",
        )
    })?;
    let physical_bytes = regular_file_len(database_path, false)?;
    let wal_bytes = regular_file_len(&wal_path(database_path), true)?;
    let (event_count, payload_bytes) = aggregate_count_and_bytes(
        connection,
        "SELECT count(*), coalesce(sum(length(CAST(canonical_json_payload AS BLOB))), 0) FROM events;",
    )?;
    let (snapshot_count, snapshot_bytes) = aggregate_count_and_bytes(
        connection,
        "SELECT count(*), coalesce(sum(length(CAST(projection_json AS BLOB))), 0) FROM (\
           SELECT projection_json FROM projects UNION ALL \
           SELECT projection_json FROM chats UNION ALL \
           SELECT projection_json FROM runs UNION ALL \
           SELECT projection_json FROM locks\
         );",
    )?;

    let status = StorageStatusV1 {
        schema_version: STORAGE_STATUS_SCHEMA_VERSION_V1,
        collected_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        database: StorageDatabaseFactsV1 {
            logical_bytes,
            physical_bytes,
            wal_bytes,
        },
        ledger_tail: StorageLedgerTailFactsV1 {
            event_count,
            payload_bytes,
        },
        snapshot_aggregates: StorageSnapshotAggregateFactsV1 {
            count: snapshot_count,
            payload_bytes: snapshot_bytes,
        },
        pressure_facts: StoragePressureFactsV1 {
            collection_status: StoragePressureCollectionStatusV1::Unavailable,
            volume_capacity_bytes: None,
            volume_available_bytes: None,
        },
        compaction: compaction_status(connection)?,
        physical_cleanup: PhysicalCleanupStatusV1 {
            enabled: false,
            reason: PhysicalCleanupDisabledReasonV1::UserPolicyNotConfigured,
        },
    };
    let value = serde_json::to_value(&status).map_err(|_| {
        SidecarError::new(
            "storageStatusInvalid",
            "storage status could not be validated",
        )
    })?;
    validate_storage_status_v1(&value).map_err(|error| {
        SidecarError::new(
            "storageStatusInvalid",
            format!("storage status facts failed validation: {error}"),
        )
    })?;
    Ok(status)
}

fn pragma_u64(connection: &Connection, name: &str) -> Result<u64, SidecarError> {
    let sql = format!("PRAGMA {name};");
    let value: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(sqlite_error)?;
    u64::try_from(value).map_err(|_| {
        SidecarError::new(
            "storageStatusUnavailable",
            "database size fact was negative",
        )
    })
}

fn aggregate_count_and_bytes(
    connection: &Connection,
    sql: &str,
) -> Result<(u64, u64), SidecarError> {
    let (count, bytes): (i64, i64) = connection
        .query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(sqlite_error)?;
    let count = u64::try_from(count).map_err(|_| {
        SidecarError::new("storageStatusUnavailable", "aggregate count was negative")
    })?;
    let bytes = u64::try_from(bytes).map_err(|_| {
        SidecarError::new(
            "storageStatusUnavailable",
            "aggregate payload byte count was negative",
        )
    })?;
    Ok((count, bytes))
}

fn regular_file_len(path: &Path, missing_is_zero: bool) -> Result<u64, SidecarError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(SidecarError::new(
                "storageStatusUnavailable",
                "database size fact did not refer to a regular file",
            ))
        }
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if missing_is_zero && error.kind() == ErrorKind::NotFound => Ok(0),
        Err(_) => Err(SidecarError::new(
            "storageStatusUnavailable",
            "database file size metadata could not be read",
        )),
    }
}

fn wal_path(database_path: &Path) -> PathBuf {
    let mut name = database_path.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

fn compaction_status(connection: &Connection) -> Result<StorageCompactionStatusV1, SidecarError> {
    let row: Option<(String, Option<String>, Option<String>)> = connection
        .query_row(
            "SELECT state, completed_at, last_successful_at FROM maintenance_jobs \
             WHERE state IN ('succeeded', 'failed') \
             ORDER BY completed_at DESC, created_at DESC, job_id DESC LIMIT 1;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sqlite_error)?;
    match row {
        None => Ok(StorageCompactionStatusV1 {
            last_attempt_status: StorageCompactionAttemptStatusV1::NeverRun,
            last_attempted_at: None,
            last_successful_at: None,
        }),
        Some((state, completed_at, last_successful_at)) => {
            let last_attempt_status = match state.as_str() {
                "succeeded" => StorageCompactionAttemptStatusV1::Succeeded,
                "failed" => StorageCompactionAttemptStatusV1::Failed,
                _ => {
                    return Err(SidecarError::new(
                        "storageStatusInvalid",
                        "maintenance job state was invalid",
                    ))
                }
            };
            Ok(StorageCompactionStatusV1 {
                last_attempt_status,
                last_attempted_at: completed_at,
                last_successful_at,
            })
        }
    }
}

fn sqlite_error(error: rusqlite::Error) -> SidecarError {
    SidecarError::new("databaseFailure", error.to_string())
}

#[cfg(test)]
mod tests;
