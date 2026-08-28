use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_protocol::project_v1::sha256_file_version_v1;
use hello_agent_protocol::run_recovery_v1::validate_g5_payload_v1;
use hello_agent_protocol::storage_lifetime_v1::{
    classify_operation_against_receipt_v2, validate_storage_status_v1, OperationReceiptV2,
    PhysicalCleanupDisabledReasonV1, PhysicalCleanupStatusV1, StorageCompactionAttemptStatusV1,
    StorageCompactionStatusV1, StorageDatabaseFactsV1, StorageLedgerTailFactsV1,
    StoragePressureCollectionStatusV1, StoragePressureFactsV1, StorageSnapshotAggregateFactsV1,
    StorageStatusV1, OPERATION_RECEIPT_DIGEST_ALGORITHM_V2, OPERATION_RECEIPT_DIGEST_FORMAT_V2,
    OPERATION_RECEIPT_DIGEST_VERSION_V2, STORAGE_STATUS_SCHEMA_VERSION_V1,
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
