use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use elsecrew_protocol::project_v1::sha256_file_version_v1;
use elsecrew_protocol::storage_lifetime_v1::{
    canonical_frozen_operation_record_v1, validate_frozen_operation_source_v1,
    validate_frozen_operation_timestamps_v1, validate_storage_compaction_job_v3,
    validate_storage_compaction_plan_v1, FrozenMessageSourceV1, FrozenOperationSourceV1,
    StorageCompactionJobV3, StorageCompactionPhaseV3, StorageCompactionPlanV1,
    STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1, STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1,
    STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1,
    STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
    STORAGE_LIFETIME_SCHEMA_VERSION_V10, STORAGE_LIFETIME_SCHEMA_VERSION_V11, STORAGE_LIFETIME_SCHEMA_VERSION_V12,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use uuid::Uuid;

const PRIMARY_BYTES: usize =
    STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 as usize;
const DEPENDENCY_BYTES: usize =
    STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1 as usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompactionTarget {
    pub aggregate_type: String,
    pub aggregate_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionCancellationPoint {
    BeforeArm,
    AfterArm,
    BeforeWorkUnit,
    ImmediatelyPrecommit,
}

pub(crate) trait CompactionCancellation {
    fn cancelled(&mut self, point: CompactionCancellationPoint) -> bool;
}

impl<F> CompactionCancellation for F
where
    F: FnMut(CompactionCancellationPoint) -> bool,
{
    fn cancelled(&mut self, point: CompactionCancellationPoint) -> bool {
        self(point)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionFailurePoint {
    AfterArmCommit,
    AfterAuthorityInsert,
    AfterBatchMutation,
    ImmediatelyPrecommit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompactionStepResult {
    Created {
        job_id: String,
    },
    ArmedWorked {
        job_id: String,
        phase: StorageCompactionPhaseV3,
        after_plan_ordinal: u32,
    },
    Finalized {
        job_id: String,
    },
    NoWork,
}

#[derive(Default)]
struct StepBudget {
    work: u32,
    primary: usize,
    dependency: usize,
}

impl StepBudget {
    fn add(&mut self, primary: usize, dependency: usize) -> Result<(), SidecarError> {
        self.work = self.work.checked_add(1).ok_or_else(compaction_error)?;
        self.add_bytes(primary, dependency)
    }

    fn add_bytes(&mut self, primary: usize, dependency: usize) -> Result<(), SidecarError> {
        self.primary = self
            .primary
            .checked_add(primary)
            .ok_or_else(compaction_error)?;
        self.dependency = self
            .dependency
            .checked_add(dependency)
            .ok_or_else(compaction_error)?;
        if self.work > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1
            || self.primary > PRIMARY_BYTES
            || self.dependency > DEPENDENCY_BYTES
        {
            return Err(compaction_error());
        }
        Ok(())
    }

    fn add_dependency(&mut self, dependency: usize) -> Result<(), SidecarError> {
        self.add_bytes(0, dependency)
    }
}

#[derive(Debug)]
struct PlanRow {
    ordinal: u32,
    event_rowid: u64,
    event_id: String,
    operation_id: String,
    request_id: Option<String>,
    aggregate_type: String,
    aggregate_id: String,
    aggregate_sequence: u64,
    event_type: String,
    payload_schema_version: u32,
    event_timestamp: String,
    canonical_payload_json: String,
    canonical_payload_digest: String,
    introduced_message_id: Option<String>,
    introduced_message_digest: Option<String>,
    decision: &'static str,
    baseline_id: Option<String>,
    baseline_projection_digest: Option<String>,
    operation_frozen_before_plan: bool,
    message_frozen_before_plan: bool,
}

#[derive(Debug)]
struct ChatMessageWitness {
    chat_id: String,
    durable_sequence: u64,
    canonical_message_json: String,
    message_digest: String,
}

#[derive(Clone, Copy)]
struct FrozenOperationRecordRef<'a> {
    operation_id: &'a str,
    source_event_rowid: u64,
    source_event_id: &'a str,
    request_id: Option<&'a str>,
    aggregate_type: &'a str,
    aggregate_id: &'a str,
    aggregate_sequence: u64,
    event_type: &'a str,
    payload_schema_version: u32,
    canonical_payload_json: &'a str,
    canonical_payload_digest: &'a str,
    event_timestamp: &'a str,
    frozen_at: &'a str,
}

#[derive(Clone, Copy)]
struct FrozenMessageRef<'a> {
    message_id: &'a str,
    source_event_rowid: u64,
    source_event_id: &'a str,
    source_operation_id: &'a str,
    chat_id: &'a str,
    source_aggregate_sequence: u64,
    durable_sequence: u64,
    canonical_message_json: &'a str,
    message_digest: &'a str,
    frozen_at: &'a str,
}

impl<'a> FrozenMessageRef<'a> {
    fn from_live(
        row: &'a PlanRow,
        message: &'a ChatMessageWitness,
        frozen_at: &'a str,
    ) -> Result<Self, SidecarError> {
        Ok(Self {
            message_id: row
                .introduced_message_id
                .as_deref()
                .ok_or_else(compaction_error)?,
            source_event_rowid: row.event_rowid,
            source_event_id: &row.event_id,
            source_operation_id: &row.operation_id,
            chat_id: &message.chat_id,
            source_aggregate_sequence: row.aggregate_sequence,
            durable_sequence: message.durable_sequence,
            canonical_message_json: &message.canonical_message_json,
            message_digest: &message.message_digest,
            frozen_at,
        })
    }

    fn from_source(source: &'a FrozenMessageSourceV1) -> Self {
        Self {
            message_id: &source.message_id,
            source_event_rowid: source.source_event_rowid,
            source_event_id: &source.source_event_id,
            source_operation_id: &source.source_operation_id,
            chat_id: &source.chat_id,
            source_aggregate_sequence: source.source_aggregate_sequence,
            durable_sequence: source.durable_sequence,
            canonical_message_json: &source.canonical_message_json,
            message_digest: &source.message_digest,
            frozen_at: &source.frozen_at,
        }
    }
}

impl<'a> FrozenOperationRecordRef<'a> {
    fn from_plan(row: &'a PlanRow, frozen_at: &'a str) -> Self {
        Self {
            operation_id: &row.operation_id,
            source_event_rowid: row.event_rowid,
            source_event_id: &row.event_id,
            request_id: row.request_id.as_deref(),
            aggregate_type: &row.aggregate_type,
            aggregate_id: &row.aggregate_id,
            aggregate_sequence: row.aggregate_sequence,
            event_type: &row.event_type,
            payload_schema_version: row.payload_schema_version,
            canonical_payload_json: &row.canonical_payload_json,
            canonical_payload_digest: &row.canonical_payload_digest,
            event_timestamp: &row.event_timestamp,
            frozen_at,
        }
    }

    fn from_source(source: &'a FrozenOperationSourceV1) -> Self {
        Self {
            operation_id: &source.operation_id,
            source_event_rowid: source.source_event_rowid,
            source_event_id: &source.source_event_id,
            request_id: source.request_id.as_deref(),
            aggregate_type: &source.aggregate_type,
            aggregate_id: &source.aggregate_id,
            aggregate_sequence: source.aggregate_sequence,
            event_type: &source.event_type,
            payload_schema_version: source.payload_schema_version,
            canonical_payload_json: &source.canonical_payload_json,
            canonical_payload_digest: &source.canonical_payload_digest,
            event_timestamp: &source.event_timestamp,
            frozen_at: &source.frozen_at,
        }
    }
}

pub(crate) fn run_private_compaction_step(
    connection: &mut Connection,
    target: &CompactionTarget,
    cancellation: &mut dyn CompactionCancellation,
    failure: Option<CompactionFailurePoint>,
) -> Result<CompactionStepResult, SidecarError> {
    validate_target(target)?;
    verify_runtime_fences(connection)?;

    let active = load_active_job(connection)?;
    let job = match active {
        Some(job) => {
            let plan = load_plan(connection, &job.job_id)?;
            if plan.aggregate_type != target.aggregate_type
                || plan.aggregate_id != target.aggregate_id
            {
                return Err(compaction_error());
            }
            job
        }
        None => {
            if cancellation.cancelled(CompactionCancellationPoint::BeforeArm) {
                return Ok(CompactionStepResult::NoWork);
            }
            let Some(job) = create_candidate(connection, target)? else {
                return Ok(CompactionStepResult::NoWork);
            };
            return Ok(CompactionStepResult::Created { job_id: job.job_id });
        }
    };

    if cancellation.cancelled(CompactionCancellationPoint::BeforeArm) {
        return Ok(CompactionStepResult::NoWork);
    }
    if job.private_state.cursor.phase == StorageCompactionPhaseV3::Finalize {
        return complete_pending_job(connection, &job, cancellation, failure);
    }
    let armed = arm_job(connection, &job)?;
    if failure == Some(CompactionFailurePoint::AfterArmCommit)
        || cancellation.cancelled(CompactionCancellationPoint::AfterArm)
    {
        return Err(compaction_error());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let current = load_exact_job(&transaction, &armed.job_id)?;
    if current.state
        != elsecrew_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Running
        || current.private_state != armed.private_state
    {
        return Err(compaction_error());
    }
    let plan = load_plan(&transaction, &armed.job_id)?;
    let mut budget = StepBudget::default();
    execute_armed_step(
        &transaction,
        &armed,
        &plan,
        &mut budget,
        cancellation,
        failure,
    )?;
    if failure == Some(CompactionFailurePoint::AfterBatchMutation)
        || failure == Some(CompactionFailurePoint::ImmediatelyPrecommit)
        || cancellation.cancelled(CompactionCancellationPoint::ImmediatelyPrecommit)
    {
        transaction.rollback().map_err(sqlite_error)?;
        return Err(compaction_error());
    }
    transaction.commit().map_err(|_| compaction_error())?;

    let next = load_exact_job(connection, &armed.job_id)?;
    if next.state
        == elsecrew_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Succeeded
    {
        Ok(CompactionStepResult::ArmedWorked {
            job_id: next.job_id,
            phase: next.private_state.cursor.phase,
            after_plan_ordinal: next.private_state.cursor.after_plan_ordinal,
        })
    } else {
        Ok(progress(&next))
    }
}

pub(crate) fn pending_compaction_target(
    connection: &Connection,
) -> Result<Option<CompactionTarget>, SidecarError> {
    let facts: (u32, u32, u32) = connection
        .query_row(
            "SELECT count(*),\
                    coalesce(sum(state='running'),0),\
                    (SELECT EXISTS(SELECT 1 FROM ledger_compaction_delete_authority LIMIT 1)) \
             FROM (SELECT state FROM maintenance_jobs \
                   INDEXED BY maintenance_jobs_single_active_lossless_compaction \
                   WHERE job_kind='losslessCompaction' AND state IN ('pending','running') \
                   LIMIT 2);",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(sqlite_error)?;
    if facts.0 > 1 || facts.1 != 0 || facts.2 != 0 {
        return Err(compaction_error());
    }
    let Some(job) = load_active_job(connection)? else {
        return Ok(None);
    };
    if job.state != elsecrew_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Pending
    {
        return Err(compaction_error());
    }
    let plan = load_plan(connection, &job.job_id)?;
    Ok(Some(CompactionTarget {
        aggregate_type: plan.aggregate_type,
        aggregate_id: plan.aggregate_id,
    }))
}

pub(crate) fn next_compaction_candidate(
    connection: &Connection,
    aggregate_type: &str,
    after_aggregate_id: &str,
) -> Result<Option<CompactionTarget>, SidecarError> {
    if !matches!(aggregate_type, "project" | "chat" | "run" | "lock")
        || after_aggregate_id.len() > DEPENDENCY_BYTES
    {
        return Err(compaction_error());
    }
    let identity: Option<(i64, bool, i64)> = connection
        .query_row(
            "SELECT event_rowid,typeof(aggregate_id)='text',length(CAST(aggregate_id AS BLOB)) \
             FROM event_source_index INDEXED BY event_source_index_aggregate_latest \
             WHERE aggregate_type=?1 AND aggregate_id>?2 \
             ORDER BY aggregate_id,event_rowid DESC LIMIT 1;",
            params![aggregate_type, after_aggregate_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sqlite_error)?;
    let Some((event_rowid, valid_type, id_length)) = identity else {
        return Ok(None);
    };
    if !valid_type || length(id_length)? > DEPENDENCY_BYTES {
        return Err(compaction_error());
    }
    let (selected_type, aggregate_id): (String, String) = connection
        .query_row(
            "SELECT aggregate_type,aggregate_id FROM event_source_index WHERE event_rowid=?1;",
            [event_rowid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sqlite_error)?;
    if selected_type != aggregate_type || aggregate_id.as_str() <= after_aggregate_id {
        return Err(compaction_error());
    }
    let target = CompactionTarget {
        aggregate_type: aggregate_type.to_string(),
        aggregate_id,
    };
    validate_target(&target)?;
    Ok(Some(target))
}

fn validate_target(target: &CompactionTarget) -> Result<(), SidecarError> {
    let baseline_input_limit = DEPENDENCY_BYTES
        .checked_sub(target.aggregate_type.len())
        .and_then(|limit| limit.checked_sub(2 + 20))
        .ok_or_else(compaction_error)?;
    if !matches!(
        target.aggregate_type.as_str(),
        "project" | "chat" | "run" | "lock"
    ) || target.aggregate_id.is_empty()
        || target.aggregate_id.len() > baseline_input_limit
    {
        return Err(compaction_error());
    }
    Ok(())
}

fn verify_runtime_fences(connection: &Connection) -> Result<(), SidecarError> {
    let facts: (u32, bool, u32, u32, u32) = connection
        .query_row(
            "SELECT storage_schema_version,materialization_state='ready',\
                    (SELECT count(*) FROM lifetime_materialization_job),\
                    (SELECT count(*) FROM maintenance_jobs WHERE job_kind='losslessCompaction' \
                     AND state='running'),\
                    (SELECT count(*) FROM ledger_compaction_delete_authority)
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
        .map_err(|_| compaction_error())?;
    if !matches!(facts.0, STORAGE_LIFETIME_SCHEMA_VERSION_V10 | STORAGE_LIFETIME_SCHEMA_VERSION_V11 | STORAGE_LIFETIME_SCHEMA_VERSION_V12)
        || !facts.1
        || facts.2 != 0
        || facts.3 != 0
        || facts.4 != 0
    {
        return Err(compaction_error());
    }
    Ok(())
}

fn create_candidate(
    connection: &mut Connection,
    target: &CompactionTarget,
) -> Result<Option<StorageCompactionJobV3>, SidecarError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    verify_runtime_fences(&transaction)?;
    if load_active_job(&transaction)?.is_some() {
        return Err(compaction_error());
    }

    let boundary = retained_boundary(&transaction, target)?;
    let first_sequence = boundary.checked_add(1).ok_or_else(compaction_error)?;
    let delete_through = boundary.checked_add(1000).ok_or_else(compaction_error)?;
    let anchor_sequence = delete_through.checked_add(1).ok_or_else(compaction_error)?;
    let eligible: u32 = transaction
        .query_row(
            "SELECT count(*) FROM (SELECT aggregate_sequence FROM events \
             WHERE aggregate_type=?1 AND aggregate_id=?2 AND aggregate_sequence>?3 \
             ORDER BY aggregate_sequence LIMIT 1001);",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(boundary)?
            ],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if eligible < STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1 {
        transaction.rollback().map_err(sqlite_error)?;
        return Ok(None);
    }
    let exact: bool = transaction
        .query_row(
            "SELECT count(*)=1001 AND min(aggregate_sequence)=?3 AND max(aggregate_sequence)=?4 \
             FROM (SELECT aggregate_sequence FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 \
                   AND aggregate_sequence>?5 ORDER BY aggregate_sequence LIMIT 1001);",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(first_sequence)?,
                sql_i64(anchor_sequence)?,
                sql_i64(boundary)?
            ],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if !exact {
        return Err(compaction_error());
    }

    ensure_candidate_baseline(&transaction, target, delete_through)?;
    let baseline_lengths: (bool, i64, i64, i64) = transaction
        .query_row(
            "SELECT typeof(baseline_id)='text' AND typeof(canonical_projection_json)='text' \
                    AND typeof(projection_digest)='text',\
                    length(CAST(baseline_id AS BLOB)),\
                    length(CAST(canonical_projection_json AS BLOB)),\
                    length(CAST(projection_digest AS BLOB)) \
             FROM ledger_baselines WHERE aggregate_type=?1 AND aggregate_id=?2 \
               AND cutoff_aggregate_sequence=?3;",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(delete_through)?
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| compaction_error())?;
    let dependency =
        checked_lengths(&[baseline_lengths.1, baseline_lengths.2, baseline_lengths.3])?;
    if !baseline_lengths.0
        || dependency > DEPENDENCY_BYTES
        || length(baseline_lengths.2)? > PRIMARY_BYTES
    {
        return Err(compaction_error());
    }
    let (baseline_id, baseline_json, baseline_digest): (String, String, String) = transaction
        .query_row(
            "SELECT baseline_id,canonical_projection_json,projection_digest \
             FROM ledger_baselines WHERE aggregate_type=?1 AND aggregate_id=?2 \
               AND cutoff_aggregate_sequence=?3;",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(delete_through)?
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    let versioned = sha256_file_version_v1(baseline_json.as_bytes());
    let baseline_raw = versioned
        .strip_prefix("sha256:")
        .ok_or_else(compaction_error)?;
    if versioned != baseline_digest {
        return Err(compaction_error());
    }
    let previous = immediate_predecessor(&transaction, target, boundary)?;
    let cutoff: i64 = transaction
        .query_row("SELECT coalesce(max(rowid),0) FROM events;", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let cutoff = u64::try_from(cutoff).map_err(|_| compaction_error())?;
    if cutoff == 0 {
        return Err(compaction_error());
    }
    let now = now();
    let job_id = format!("compact_{}", Uuid::new_v4().simple());
    let private = private_state(cutoff, StorageCompactionPhaseV3::PlanEligibility, 0, false);
    transaction
        .execute(
            "INSERT INTO maintenance_jobs(\
               job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,\
               failure_kind,created_at,updated_at) \
             VALUES (?1,'losslessCompaction','pending',?2,NULL,NULL,NULL,NULL,?3,?3);",
            params![job_id, private.to_string(), now],
        )
        .map_err(sqlite_error)?;
    transaction
        .execute(
            "INSERT INTO storage_compaction_plans(\
               job_id,plan_schema_version,cutoff_event_rowid,aggregate_type,aggregate_id,\
               retained_boundary_sequence,delete_through_sequence,anchor_aggregate_sequence,\
               expected_rows,baseline_id,baseline_projection_digest,previous_successful_job_id,\
               created_at,planned_at) \
             VALUES (?1,1,?2,?3,?4,?5,?6,?7,1001,?8,?9,?10,?11,?11);",
            params![
                job_id,
                sql_i64(cutoff)?,
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(boundary)?,
                sql_i64(delete_through)?,
                sql_i64(anchor_sequence)?,
                baseline_id,
                baseline_raw,
                previous,
                now,
            ],
        )
        .map_err(sqlite_error)?;
    let job = load_exact_job(&transaction, &job_id)?;
    let plan = load_plan(&transaction, &job_id)?;
    if plan.retained_boundary_sequence != boundary
        || plan.delete_through_sequence != delete_through
        || plan.anchor_aggregate_sequence != anchor_sequence
    {
        return Err(compaction_error());
    }
    transaction.commit().map_err(|_| compaction_error())?;
    Ok(Some(job))
}

fn retained_boundary(
    connection: &Connection,
    target: &CompactionTarget,
) -> Result<u64, SidecarError> {
    let value: i64 = connection
        .query_row(
            "SELECT coalesce(max(deleted_through_sequence),0) FROM ledger_compaction_prefixes \
             WHERE aggregate_type=?1 AND aggregate_id=?2;",
            params![target.aggregate_type, target.aggregate_id],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    u64::try_from(value).map_err(|_| compaction_error())
}

fn ensure_candidate_baseline(
    transaction: &Transaction<'_>,
    target: &CompactionTarget,
    sequence: u64,
) -> Result<(), SidecarError> {
    let source_lengths: (bool, i64, i64) = transaction
        .query_row(
            "SELECT typeof(e.canonical_json_payload)='text' AND typeof(e.event_id)='text' \
                    AND typeof(e.operation_id)='text' AND typeof(e.aggregate_type)='text' \
                    AND typeof(e.aggregate_id)='text' AND typeof(e.event_type)='text' \
                    AND typeof(e.timestamp)='text' AND typeof(e.aggregate_sequence)='integer',\
                    length(CAST(e.canonical_json_payload AS BLOB)),\
                    length(CAST(e.event_id AS BLOB))+length(CAST(e.operation_id AS BLOB))+\
                    length(CAST(e.aggregate_type AS BLOB))+length(CAST(e.aggregate_id AS BLOB))+\
                    length(CAST(e.event_type AS BLOB))+length(CAST(e.timestamp AS BLOB)) \
             FROM events e JOIN event_source_index i ON i.event_rowid=e.rowid \
             JOIN operation_receipts r ON r.operation_id=e.operation_id \
             WHERE e.aggregate_type=?1 AND e.aggregate_id=?2 AND e.aggregate_sequence=?3 \
               AND i.event_id=e.event_id AND i.operation_id=e.operation_id \
               AND i.aggregate_type=e.aggregate_type AND i.aggregate_id=e.aggregate_id \
               AND i.aggregate_sequence=e.aggregate_sequence \
               AND r.event_id=e.event_id AND r.aggregate_type=e.aggregate_type \
               AND r.aggregate_id=e.aggregate_id AND r.aggregate_sequence=e.aggregate_sequence \
               AND r.canonical_payload_json=e.canonical_json_payload;",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(sequence)?
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    if !source_lengths.0
        || length(source_lengths.1)? > PRIMARY_BYTES
        || checked_lengths(&[source_lengths.1, source_lengths.2])? > DEPENDENCY_BYTES
    {
        return Err(compaction_error());
    }
    let canonical: String = transaction
        .query_row(
            "SELECT canonical_json_payload FROM events WHERE aggregate_type=?1 \
             AND aggregate_id=?2 AND aggregate_sequence=?3;",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(sequence)?
            ],
            |row| row.get(0),
        )
        .map_err(|_| compaction_error())?;
    let digest = sha256_file_version_v1(canonical.as_bytes());
    let baseline_id = baseline_id(&target.aggregate_type, &target.aggregate_id, sequence)?;
    let existing: u32 = transaction
        .query_row(
            "SELECT count(*) FROM ledger_baselines WHERE aggregate_type=?1 AND aggregate_id=?2 \
             AND cutoff_aggregate_sequence=?3;",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(sequence)?
            ],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if existing == 0 {
        let changed = transaction
            .execute(
                "INSERT INTO ledger_baselines(\
                   baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,\
                   payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at) \
                 VALUES (?1,?2,?3,?4,1,?5,?6,?7,?7);",
                params![baseline_id, target.aggregate_type, target.aggregate_id,
                    sql_i64(sequence)?, canonical, digest, now()],
            )
            .map_err(sqlite_error)?;
        if changed != 1 {
            return Err(compaction_error());
        }
    } else if existing != 1 {
        return Err(compaction_error());
    }
    let exact: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ledger_baselines WHERE baseline_id=?1 \
             AND aggregate_type=?2 AND aggregate_id=?3 AND cutoff_aggregate_sequence=?4 \
             AND payload_schema_version=1 AND canonical_projection_json=?5 AND projection_digest=?6);",
            params![baseline_id, target.aggregate_type, target.aggregate_id,
                sql_i64(sequence)?, canonical, digest],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if !exact {
        return Err(compaction_error());
    }
    Ok(())
}

fn immediate_predecessor(
    connection: &Connection,
    target: &CompactionTarget,
    boundary: u64,
) -> Result<Option<String>, SidecarError> {
    if boundary == 0 {
        let prior: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM storage_compaction_plans \
                 WHERE aggregate_type=?1 AND aggregate_id=?2 LIMIT 1);",
                params![target.aggregate_type, target.aggregate_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        return if prior {
            Err(compaction_error())
        } else {
            Ok(None)
        };
    }
    let facts: (bool, u32, i64) = connection
        .query_row(
            "SELECT coalesce(min(typeof(p.job_id)='text'),1),count(*),\
                    coalesce(max(length(CAST(p.job_id AS BLOB))),0) \
             FROM storage_compaction_plans p JOIN maintenance_jobs j ON j.job_id=p.job_id \
             WHERE p.aggregate_type=?1 AND p.aggregate_id=?2 AND p.delete_through_sequence=?3 \
               AND j.state='succeeded';",
            params![
                target.aggregate_type,
                target.aggregate_id,
                sql_i64(boundary)?
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(sqlite_error)?;
    if !facts.0 || facts.1 != 1 || length(facts.2)? > DEPENDENCY_BYTES {
        return Err(compaction_error());
    }
    connection
        .query_row(
            "SELECT p.job_id FROM storage_compaction_plans p JOIN maintenance_jobs j ON j.job_id=p.job_id \
             WHERE p.aggregate_type=?1 AND p.aggregate_id=?2 AND p.delete_through_sequence=?3 \
               AND j.state='succeeded';",
            params![target.aggregate_type, target.aggregate_id, sql_i64(boundary)?],
            |row| row.get(0),
        )
        .map(Some)
        .map_err(sqlite_error)
}

fn arm_job(
    connection: &mut Connection,
    job: &StorageCompactionJobV3,
) -> Result<StorageCompactionJobV3, SidecarError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let current = load_exact_job(&transaction, &job.job_id)?;
    if current != *job
        || current
            .private_state
            .cursor
            .step_through_plan_ordinal
            .is_some()
    {
        return Err(compaction_error());
    }
    let cursor = &current.private_state.cursor;
    let limit = phase_limit(cursor.phase);
    let step = if cursor.scan_exhausted {
        None
    } else {
        Some(
            cursor
                .after_plan_ordinal
                .checked_add(STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1)
                .ok_or_else(compaction_error)?
                .min(limit),
        )
    };
    let changed = transaction
        .execute(
            "UPDATE maintenance_jobs SET state='running',started_at=coalesce(started_at,?2),\
               updated_at=?2,cutoff_json=json_set(cutoff_json,\
               '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal',\
               CASE WHEN ?3 IS NULL THEN json('null') ELSE ?3 END) \
             WHERE job_id=?1 AND state='pending';",
            params![current.job_id, now(), step],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(compaction_error());
    }
    let armed = load_exact_job(&transaction, &current.job_id)?;
    transaction.commit().map_err(|_| compaction_error())?;
    Ok(armed)
}

fn execute_armed_step(
    transaction: &Transaction<'_>,
    job: &StorageCompactionJobV3,
    plan: &StorageCompactionPlanV1,
    budget: &mut StepBudget,
    cancellation: &mut dyn CompactionCancellation,
    failure: Option<CompactionFailurePoint>,
) -> Result<(), SidecarError> {
    let cursor = &job.private_state.cursor;
    if cursor.scan_exhausted {
        return advance_phase(transaction, job, plan, budget);
    }
    let through = cursor
        .step_through_plan_ordinal
        .ok_or_else(compaction_error)?;
    if through <= cursor.after_plan_ordinal
        || through - cursor.after_plan_ordinal > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1
        || through > phase_limit(cursor.phase)
    {
        return Err(compaction_error());
    }
    for ordinal in cursor.after_plan_ordinal + 1..=through {
        if cancellation.cancelled(CompactionCancellationPoint::BeforeWorkUnit) {
            return Err(compaction_error());
        }
        match cursor.phase {
            StorageCompactionPhaseV3::PlanEligibility => {
                let row = load_source_as_plan_row(transaction, plan, ordinal, budget)?;
                insert_plan_row(transaction, plan, &row)?;
            }
            StorageCompactionPhaseV3::FreezeOperations => {
                let row = load_planned_row(transaction, plan, ordinal, budget)?;
                freeze_operation(transaction, &row, budget)?;
            }
            StorageCompactionPhaseV3::FreezeMessages => {
                let row = load_planned_row(transaction, plan, ordinal, budget)?;
                freeze_message(transaction, &row, budget)?;
            }
            StorageCompactionPhaseV3::VerifyPrefix | StorageCompactionPhaseV3::AuthorizeDelete => {
                let row = load_planned_row(transaction, plan, ordinal, budget)?;
                verify_planned_row(transaction, &row, true, budget)?;
            }
            StorageCompactionPhaseV3::DeletePrefix => {}
            StorageCompactionPhaseV3::VerifyOperations
            | StorageCompactionPhaseV3::VerifyEquivalence => {
                let row = load_planned_row(transaction, plan, ordinal, budget)?;
                verify_planned_row(transaction, &row, ordinal == 1001, budget)?;
            }
            StorageCompactionPhaseV3::ReleasePlan => {
                let changed = transaction
                    .execute(
                        "DELETE FROM storage_compaction_plan_rows WHERE job_id=?1 AND ordinal=?2;",
                        params![job.job_id, ordinal],
                    )
                    .map_err(sqlite_error)?;
                if changed != 1 {
                    return Err(compaction_error());
                }
                budget.add(0, 0)?;
            }
            StorageCompactionPhaseV3::Finalize => return Err(compaction_error()),
        }
    }

    if cursor.phase == StorageCompactionPhaseV3::DeletePrefix {
        delete_batch(transaction, job, plan, through, budget, failure)?;
        return Ok(());
    }
    if cursor.phase == StorageCompactionPhaseV3::VerifyEquivalence && through == 1001 {
        verify_equivalence(transaction, plan, budget)?;
    }
    finish_batch(transaction, job, through)
}

fn finish_batch(
    transaction: &Transaction<'_>,
    job: &StorageCompactionJobV3,
    through: u32,
) -> Result<(), SidecarError> {
    let changed = transaction
        .execute(
            "UPDATE maintenance_jobs SET state='pending',updated_at=?2,cutoff_json=json_set(\
               cutoff_json,'$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal',?3,\
               '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal',json('null'),\
               '$.storageCompactionV3PrivateState.cursor.scanExhausted',\
               CASE WHEN ?3=?4 THEN json('true') ELSE json('false') END,\
               '$.storageCompactionV3PrivateState.planRowsComplete',\
               CASE WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.cursor.phase')='planEligibility' AND ?3=1001 THEN json('true') WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.planRowsComplete')=1 THEN json('true') ELSE json('false') END,\
               '$.storageCompactionV3PrivateState.operationsVerified',\
               CASE WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.cursor.phase')='verifyOperations' AND ?3=1001 THEN json('true') WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.operationsVerified')=1 THEN json('true') ELSE json('false') END,\
               '$.storageCompactionV3PrivateState.equivalenceVerified',\
               CASE WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.cursor.phase')='verifyEquivalence' AND ?3=1001 THEN json('true') WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.equivalenceVerified')=1 THEN json('true') ELSE json('false') END,\
               '$.storageCompactionV3PrivateState.planRowsReleased',\
               CASE WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.cursor.phase')='releasePlan' AND ?3=1000 THEN json('true') WHEN json_extract(cutoff_json,'$.storageCompactionV3PrivateState.planRowsReleased')=1 THEN json('true') ELSE json('false') END) \
             WHERE job_id=?1 AND state='running' AND \
               json_extract(cutoff_json,'$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal')=?3;",
            params![job.job_id, now(), through, phase_limit(job.private_state.cursor.phase)],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(compaction_error());
    }
    Ok(())
}

fn advance_phase(
    transaction: &Transaction<'_>,
    job: &StorageCompactionJobV3,
    plan: &StorageCompactionPlanV1,
    budget: &mut StepBudget,
) -> Result<(), SidecarError> {
    if job.private_state.cursor.phase == StorageCompactionPhaseV3::VerifyPrefix {
        insert_prefix(transaction, plan, budget)?;
    }
    let next = next_phase(job.private_state.cursor.phase).ok_or_else(compaction_error)?;
    let after = if next == StorageCompactionPhaseV3::Finalize {
        STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1
    } else {
        0
    };
    let exhausted = next == StorageCompactionPhaseV3::Finalize;
    let state = private_state(
        job.private_state.cursor.cutoff_event_rowid,
        next,
        after,
        exhausted,
    );
    let changed = transaction
        .execute(
            "UPDATE maintenance_jobs SET state='pending',cutoff_json=?2,updated_at=?3 \
             WHERE job_id=?1 AND state='running';",
            params![job.job_id, state.to_string(), now()],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(compaction_error());
    }
    Ok(())
}

fn complete_pending_job(
    connection: &mut Connection,
    job: &StorageCompactionJobV3,
    cancellation: &mut dyn CompactionCancellation,
    failure: Option<CompactionFailurePoint>,
) -> Result<CompactionStepResult, SidecarError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let current = load_exact_job(&transaction, &job.job_id)?;
    if current != *job
        || current.state
            != elsecrew_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Pending
        || current.private_state.cursor.phase != StorageCompactionPhaseV3::Finalize
        || !current.private_state.cursor.scan_exhausted
        || current.private_state.cursor.after_plan_ordinal != STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1
        || current
            .private_state
            .cursor
            .step_through_plan_ordinal
            .is_some()
    {
        return Err(compaction_error());
    }
    let exact_anchor: bool = transaction
        .query_row(
            "SELECT count(*)=1 AND min(ordinal)=1001 AND max(ordinal)=1001 \
             AND min(decision)='retainAnchor' AND max(decision)='retainAnchor' \
             FROM storage_compaction_plan_rows WHERE job_id=?1;",
            [&job.job_id],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    let authority: u32 = transaction
        .query_row(
            "SELECT count(*) FROM ledger_compaction_delete_authority WHERE job_id=?1;",
            [&job.job_id],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if !exact_anchor || authority != 0 {
        return Err(compaction_error());
    }
    if failure == Some(CompactionFailurePoint::ImmediatelyPrecommit)
        || cancellation.cancelled(CompactionCancellationPoint::ImmediatelyPrecommit)
    {
        transaction.rollback().map_err(sqlite_error)?;
        return Err(compaction_error());
    }
    let completed = now();
    let changed = transaction
        .execute(
            "UPDATE maintenance_jobs SET state='succeeded',completed_at=?2,last_successful_at=?2,\
               updated_at=?2 WHERE job_id=?1 AND state='pending';",
            params![job.job_id, completed],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(compaction_error());
    }
    transaction.commit().map_err(|_| compaction_error())?;
    let completed = load_exact_job(connection, &job.job_id)?;
    if completed.state
        != elsecrew_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Succeeded
    {
        return Err(compaction_error());
    }
    Ok(CompactionStepResult::Finalized {
        job_id: completed.job_id,
    })
}

fn load_source_as_plan_row(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
    ordinal: u32,
    budget: &mut StepBudget,
) -> Result<PlanRow, SidecarError> {
    let sequence = plan
        .retained_boundary_sequence
        .checked_add(u64::from(ordinal))
        .ok_or_else(compaction_error)?;
    let lengths: (bool, i64, i64) = connection
        .query_row(
            "SELECT typeof(e.rowid)='integer' AND typeof(e.event_id)='text' \
                    AND typeof(e.operation_id)='text' AND (e.request_id IS NULL OR typeof(e.request_id)='text') \
                    AND typeof(e.aggregate_type)='text' AND typeof(e.aggregate_id)='text' \
                    AND typeof(e.aggregate_sequence)='integer' AND typeof(e.event_type)='text' \
                    AND typeof(e.payload_schema_version)='integer' AND typeof(e.timestamp)='text' \
                    AND typeof(e.canonical_json_payload)='text' AND typeof(r.canonical_payload_digest)='text' \
                    AND (i.introduced_message_id IS NULL OR typeof(i.introduced_message_id)='text') \
                    AND (m.message_digest IS NULL OR typeof(m.message_digest)='text'),\
                    length(CAST(e.canonical_json_payload AS BLOB)),\
                    length(CAST(e.event_id AS BLOB))+length(CAST(e.operation_id AS BLOB))+\
                    coalesce(length(CAST(e.request_id AS BLOB)),0)+length(CAST(e.aggregate_type AS BLOB))+\
                    length(CAST(e.aggregate_id AS BLOB))+length(CAST(e.event_type AS BLOB))+\
                    length(CAST(e.timestamp AS BLOB))+length(CAST(r.canonical_payload_digest AS BLOB))+\
                    coalesce(length(CAST(i.introduced_message_id AS BLOB)),0)+\
                    coalesce(length(CAST(m.message_digest AS BLOB)),0) \
             FROM events e JOIN event_source_index i ON i.event_rowid=e.rowid \
             JOIN operation_receipts r ON r.operation_id=e.operation_id \
             LEFT JOIN chat_messages m ON m.message_id=i.introduced_message_id \
             WHERE e.aggregate_type=?1 AND e.aggregate_id=?2 AND e.aggregate_sequence=?3 \
               AND e.rowid<=?4 AND r.canonical_payload_json=e.canonical_json_payload;",
            params![plan.aggregate_type, plan.aggregate_id, sql_i64(sequence)?, sql_i64(plan.cutoff_event_rowid)?],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    if !lengths.0 {
        return Err(compaction_error());
    }
    budget.add(length(lengths.1)?, length(lengths.2)?)?;
    budget.add_dependency(checked_usize_sum(&[
        71,
        64,
        if ordinal <= 1000 {
            plan.baseline_id.len()
        } else {
            0
        },
        if ordinal <= 1000 {
            plan.baseline_projection_digest.len()
        } else {
            0
        },
    ])?)?;
    let mut row = connection
        .query_row(
            "SELECT e.rowid,e.event_id,e.operation_id,e.request_id,e.aggregate_type,e.aggregate_id,\
                    e.aggregate_sequence,e.event_type,e.payload_schema_version,e.timestamp,\
                    e.canonical_json_payload,r.canonical_payload_digest,i.introduced_message_id,\
                    CASE WHEN m.message_digest IS NULL THEN NULL ELSE substr(m.message_digest,8) END,\
                    EXISTS(SELECT 1 FROM frozen_operation_sources f WHERE f.operation_id=e.operation_id),\
                    EXISTS(SELECT 1 FROM frozen_message_sources fm WHERE fm.message_id=i.introduced_message_id) \
             FROM events e JOIN event_source_index i ON i.event_rowid=e.rowid \
             JOIN operation_receipts r ON r.operation_id=e.operation_id \
             LEFT JOIN chat_messages m ON m.message_id=i.introduced_message_id \
             WHERE e.aggregate_type=?1 AND e.aggregate_id=?2 AND e.aggregate_sequence=?3 \
               AND e.rowid<=?4;",
            params![plan.aggregate_type, plan.aggregate_id, sql_i64(sequence)?, sql_i64(plan.cutoff_event_rowid)?],
            |r| {
                Ok(PlanRow {
                    ordinal,
                    event_rowid: row_u64(r, 0)?,
                    event_id: r.get(1)?,
                    operation_id: r.get(2)?,
                    request_id: r.get(3)?,
                    aggregate_type: r.get(4)?,
                    aggregate_id: r.get(5)?,
                    aggregate_sequence: row_u64(r, 6)?,
                    event_type: r.get(7)?,
                    payload_schema_version: r.get(8)?,
                    event_timestamp: r.get(9)?,
                    canonical_payload_json: r.get(10)?,
                    canonical_payload_digest: r.get(11)?,
                    introduced_message_id: r.get(12)?,
                    introduced_message_digest: r.get(13)?,
                    decision: if ordinal == 1001 { "retainAnchor" } else { "delete" },
                    baseline_id: (ordinal <= 1000).then(|| plan.baseline_id.clone()),
                    baseline_projection_digest: (ordinal <= 1000)
                        .then(|| plan.baseline_projection_digest.clone()),
                    operation_frozen_before_plan: r.get(14)?,
                    message_frozen_before_plan: r.get(15)?,
                })
            },
        )
        .map_err(|_| compaction_error())?;
    let computed = sha256_file_version_v1(row.canonical_payload_json.as_bytes());
    row.canonical_payload_digest = computed
        .strip_prefix("sha256:")
        .ok_or_else(compaction_error)?
        .to_string();
    if row
        .introduced_message_digest
        .as_deref()
        .is_some_and(|digest| digest.len() != 64)
    {
        return Err(compaction_error());
    }
    Ok(row)
}

fn insert_plan_row(
    transaction: &Transaction<'_>,
    plan: &StorageCompactionPlanV1,
    row: &PlanRow,
) -> Result<(), SidecarError> {
    let changed = transaction
        .execute(
            "INSERT INTO storage_compaction_plan_rows(\
               job_id,ordinal,event_rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,\
               aggregate_sequence,event_type,payload_schema_version,event_timestamp,canonical_payload_json,\
               canonical_payload_digest,introduced_message_id,introduced_message_digest,decision,\
               baseline_id,baseline_projection_digest,operation_frozen_before_plan,message_frozen_before_plan) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21);",
            params![
                plan.job_id,
                row.ordinal,
                sql_i64(row.event_rowid)?,
                row.event_id,
                row.operation_id,
                row.request_id,
                row.aggregate_type,
                row.aggregate_id,
                sql_i64(row.aggregate_sequence)?,
                row.event_type,
                row.payload_schema_version,
                row.event_timestamp,
                row.canonical_payload_json,
                row.canonical_payload_digest,
                row.introduced_message_id,
                row.introduced_message_digest,
                row.decision,
                row.baseline_id,
                row.baseline_projection_digest,
                row.operation_frozen_before_plan,
                row.message_frozen_before_plan,
            ],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(compaction_error());
    }
    Ok(())
}

fn load_planned_row(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
    ordinal: u32,
    budget: &mut StepBudget,
) -> Result<PlanRow, SidecarError> {
    let lengths: (bool, i64, i64) = connection
        .query_row(
            "SELECT typeof(ordinal)='integer' AND typeof(event_rowid)='integer' \
                    AND typeof(event_id)='text' AND typeof(operation_id)='text' \
                    AND (request_id IS NULL OR typeof(request_id)='text') \
                    AND typeof(aggregate_type)='text' AND typeof(aggregate_id)='text' \
                    AND typeof(aggregate_sequence)='integer' AND typeof(event_type)='text' \
                    AND typeof(payload_schema_version)='integer' AND typeof(event_timestamp)='text' \
                    AND typeof(canonical_payload_json)='text' AND typeof(canonical_payload_digest)='text' \
                    AND (introduced_message_id IS NULL OR typeof(introduced_message_id)='text') \
                    AND (introduced_message_digest IS NULL OR typeof(introduced_message_digest)='text') \
                    AND typeof(decision)='text' AND (baseline_id IS NULL OR typeof(baseline_id)='text') \
                    AND (baseline_projection_digest IS NULL OR typeof(baseline_projection_digest)='text') \
                    AND typeof(operation_frozen_before_plan)='integer' \
                    AND typeof(message_frozen_before_plan)='integer',\
                    length(CAST(canonical_payload_json AS BLOB)),\
                    length(CAST(event_id AS BLOB))+length(CAST(operation_id AS BLOB))+\
                    coalesce(length(CAST(request_id AS BLOB)),0)+length(CAST(aggregate_type AS BLOB))+\
                    length(CAST(aggregate_id AS BLOB))+length(CAST(event_type AS BLOB))+\
                    length(CAST(event_timestamp AS BLOB))+length(CAST(canonical_payload_digest AS BLOB))+\
                    coalesce(length(CAST(introduced_message_id AS BLOB)),0)+\
                    coalesce(length(CAST(introduced_message_digest AS BLOB)),0)+\
                    coalesce(length(CAST(baseline_id AS BLOB)),0)+\
                    coalesce(length(CAST(baseline_projection_digest AS BLOB)),0)+\
                    length(CAST(decision AS BLOB)) \
             FROM storage_compaction_plan_rows WHERE job_id=?1 AND ordinal=?2;",
            params![plan.job_id, ordinal],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    if !lengths.0 {
        return Err(compaction_error());
    }
    budget.add(length(lengths.1)?, length(lengths.2)?)?;
    connection
        .query_row(
            "SELECT ordinal,event_rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,\
                    aggregate_sequence,event_type,payload_schema_version,event_timestamp,canonical_payload_json,\
                    canonical_payload_digest,introduced_message_id,introduced_message_digest,decision,\
                    baseline_id,baseline_projection_digest,operation_frozen_before_plan,message_frozen_before_plan \
             FROM storage_compaction_plan_rows WHERE job_id=?1 AND ordinal=?2;",
            params![plan.job_id, ordinal],
            |r| {
                let decision: String = r.get(15)?;
                let decision = match decision.as_str() {
                    "delete" => "delete",
                    "retainAnchor" => "retainAnchor",
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok(PlanRow {
                    ordinal: r.get(0)?, event_rowid: row_u64(r, 1)?, event_id: r.get(2)?,
                    operation_id: r.get(3)?, request_id: r.get(4)?, aggregate_type: r.get(5)?,
                    aggregate_id: r.get(6)?, aggregate_sequence: row_u64(r, 7)?, event_type: r.get(8)?,
                    payload_schema_version: r.get(9)?, event_timestamp: r.get(10)?,
                    canonical_payload_json: r.get(11)?, canonical_payload_digest: r.get(12)?,
                    introduced_message_id: r.get(13)?, introduced_message_digest: r.get(14)?,
                    decision, baseline_id: r.get(16)?, baseline_projection_digest: r.get(17)?,
                    operation_frozen_before_plan: r.get(18)?, message_frozen_before_plan: r.get(19)?,
                })
            },
        )
        .map_err(|_| compaction_error())
}

fn freeze_operation(
    transaction: &Transaction<'_>,
    row: &PlanRow,
    budget: &mut StepBudget,
) -> Result<(), SidecarError> {
    if exact_frozen_operation(transaction, row, budget)? {
        return Ok(());
    }
    let frozen_at = now();
    let record = FrozenOperationRecordRef::from_plan(row, &frozen_at);
    let record_len = frozen_operation_record_len(record)?;
    let borrowed_dependency = frozen_operation_borrowed_dependency_len(record)?;
    budget.add_bytes(
        checked_scale(row.canonical_payload_json.len(), 2)?,
        checked_usize_sum(&[checked_scale(borrowed_dependency, 2)?, record_len, 71, 64])?,
    )?;
    let mut source = FrozenOperationSourceV1 {
        operation_id: row.operation_id.clone(),
        source_event_rowid: row.event_rowid,
        source_event_id: row.event_id.clone(),
        request_id: row.request_id.clone(),
        aggregate_type: row.aggregate_type.clone(),
        aggregate_id: row.aggregate_id.clone(),
        aggregate_sequence: row.aggregate_sequence,
        event_type: row.event_type.clone(),
        payload_schema_version: row.payload_schema_version,
        canonical_payload_json: row.canonical_payload_json.clone(),
        canonical_payload_digest: row.canonical_payload_digest.clone(),
        event_timestamp: row.event_timestamp.clone(),
        canonical_record_json: String::new(),
        record_digest: String::new(),
        frozen_at,
    };
    source.canonical_record_json =
        canonical_frozen_operation_record_v1(&source).map_err(|_| compaction_error())?;
    if source.canonical_record_json.len() != record_len {
        return Err(compaction_error());
    }
    source.record_digest = sha256_file_version_v1(source.canonical_record_json.as_bytes())
        .strip_prefix("sha256:")
        .ok_or_else(compaction_error)?
        .to_string();
    transaction
        .execute(
            "INSERT INTO frozen_operation_sources(\
               operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,aggregate_id,\
               aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,\
               canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15);",
            params![
                source.operation_id, sql_i64(source.source_event_rowid)?, source.source_event_id,
                source.request_id, source.aggregate_type, source.aggregate_id,
                sql_i64(source.aggregate_sequence)?, source.event_type, source.payload_schema_version,
                source.canonical_payload_json, source.canonical_payload_digest, source.event_timestamp,
                source.canonical_record_json, source.record_digest, source.frozen_at,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn exact_frozen_operation(
    connection: &Connection,
    row: &PlanRow,
    budget: &mut StepBudget,
) -> Result<bool, SidecarError> {
    let count: u32 = connection
        .query_row(
            "SELECT count(*) FROM frozen_operation_sources WHERE operation_id=?1;",
            [&row.operation_id],
            |record| record.get(0),
        )
        .map_err(sqlite_error)?;
    if count == 0 {
        return Ok(false);
    }
    if count != 1 {
        return Err(compaction_error());
    }
    let (types_valid, primary, dependency): (bool, i64, i64) = connection
        .query_row(
            "SELECT \
               typeof(source_event_rowid)='integer' AND typeof(aggregate_sequence)='integer' \
               AND typeof(payload_schema_version)='integer' \
               AND typeof(operation_id)='text' AND typeof(source_event_id)='text' \
               AND (request_id IS NULL OR typeof(request_id)='text') \
               AND typeof(aggregate_type)='text' AND typeof(aggregate_id)='text' \
               AND typeof(event_type)='text' AND typeof(canonical_payload_json)='text' \
               AND typeof(canonical_payload_digest)='text' AND typeof(event_timestamp)='text' \
               AND typeof(canonical_record_json)='text' AND typeof(record_digest)='text' \
               AND typeof(frozen_at)='text',\
               length(CAST(canonical_payload_json AS BLOB)),\
               length(CAST(operation_id AS BLOB))+length(CAST(source_event_id AS BLOB))+\
               coalesce(length(CAST(request_id AS BLOB)),0)+length(CAST(aggregate_type AS BLOB))+\
               length(CAST(aggregate_id AS BLOB))+length(CAST(event_type AS BLOB))+\
               length(CAST(canonical_payload_digest AS BLOB))+length(CAST(event_timestamp AS BLOB))+\
               length(CAST(canonical_record_json AS BLOB))+length(CAST(record_digest AS BLOB))+\
               length(CAST(frozen_at AS BLOB)) \
             FROM frozen_operation_sources WHERE operation_id=?1;",
            [&row.operation_id],
            |record| Ok((record.get(0)?, record.get(1)?, record.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    if !types_valid {
        return Err(compaction_error());
    }
    let source_primary = length(primary)?;
    let source_dependency = length(dependency)?;
    budget.add_bytes(source_primary, source_dependency)?;
    let source = connection
        .query_row(
            "SELECT operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,\
                    aggregate_id,aggregate_sequence,event_type,payload_schema_version,\
                    canonical_payload_json,canonical_payload_digest,event_timestamp,\
                    canonical_record_json,record_digest,frozen_at \
             FROM frozen_operation_sources WHERE operation_id=?1;",
            [&row.operation_id],
            |record| {
                Ok(FrozenOperationSourceV1 {
                    operation_id: record.get(0)?,
                    source_event_rowid: row_u64(record, 1)?,
                    source_event_id: record.get(2)?,
                    request_id: record.get(3)?,
                    aggregate_type: record.get(4)?,
                    aggregate_id: record.get(5)?,
                    aggregate_sequence: row_u64(record, 6)?,
                    event_type: record.get(7)?,
                    payload_schema_version: record.get(8)?,
                    canonical_payload_json: record.get(9)?,
                    canonical_payload_digest: record.get(10)?,
                    event_timestamp: record.get(11)?,
                    canonical_record_json: record.get(12)?,
                    record_digest: record.get(13)?,
                    frozen_at: record.get(14)?,
                })
            },
        )
        .map_err(|_| compaction_error())?;
    let record_len = frozen_operation_record_len(FrozenOperationRecordRef::from_source(&source))?;
    budget.add_bytes(
        checked_scale(source_primary, 4)?,
        checked_usize_sum(&[checked_scale(source_dependency, 3)?, record_len, 142])?,
    )?;
    let value = serde_json::to_value(&source).map_err(|_| compaction_error())?;
    let validated = validate_frozen_operation_source_v1(&value).map_err(|_| compaction_error())?;
    if validated != source
        || source.operation_id != row.operation_id
        || source.source_event_rowid != row.event_rowid
        || source.source_event_id != row.event_id
        || source.request_id != row.request_id
        || source.aggregate_type != row.aggregate_type
        || source.aggregate_id != row.aggregate_id
        || source.aggregate_sequence != row.aggregate_sequence
        || source.event_type != row.event_type
        || source.payload_schema_version != row.payload_schema_version
        || source.canonical_payload_json != row.canonical_payload_json
        || source.canonical_payload_digest != row.canonical_payload_digest
        || source.event_timestamp != row.event_timestamp
    {
        return Err(compaction_error());
    }
    Ok(true)
}

fn freeze_message(
    transaction: &Transaction<'_>,
    row: &PlanRow,
    budget: &mut StepBudget,
) -> Result<(), SidecarError> {
    let Some(message_id) = row.introduced_message_id.as_deref() else {
        if row.introduced_message_digest.is_some() {
            return Err(compaction_error());
        }
        return Ok(());
    };
    let message = load_chat_message(transaction, message_id, budget)?;
    budget.add_dependency(71)?;
    if message.message_digest.strip_prefix("sha256:") != row.introduced_message_digest.as_deref()
        || sha256_file_version_v1(message.canonical_message_json.as_bytes())
            != message.message_digest
    {
        return Err(compaction_error());
    }
    if exact_frozen_message(transaction, row, &message, budget)? {
        return Ok(());
    }
    let frozen_at = now();
    let source = FrozenMessageRef::from_live(row, &message, &frozen_at)?;
    budget.add_dependency(71)?;
    validate_frozen_message_ref(source, row, &message)?;
    transaction
        .execute(
            "INSERT INTO frozen_message_sources(\
               message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,\
               source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10);",
            params![source.message_id, sql_i64(source.source_event_rowid)?, source.source_event_id,
                source.source_operation_id, source.chat_id, sql_i64(source.source_aggregate_sequence)?,
                sql_i64(source.durable_sequence)?, source.canonical_message_json,
                source.message_digest, source.frozen_at],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn load_chat_message(
    connection: &Connection,
    message_id: &str,
    budget: &mut StepBudget,
) -> Result<ChatMessageWitness, SidecarError> {
    let (types_valid, dependency): (bool, i64) = connection
        .query_row(
            "SELECT typeof(chat_id)='text' AND typeof(event_sequence)='integer' \
                    AND typeof(canonical_message_json)='text' AND typeof(message_digest)='text',\
                    length(CAST(chat_id AS BLOB))+length(CAST(canonical_message_json AS BLOB))+\
                    length(CAST(message_digest AS BLOB)) \
             FROM chat_messages WHERE message_id=?1;",
            [message_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    if !types_valid {
        return Err(compaction_error());
    }
    budget.add_dependency(length(dependency)?)?;
    connection
        .query_row(
            "SELECT chat_id,event_sequence,canonical_message_json,message_digest \
             FROM chat_messages WHERE message_id=?1;",
            [message_id],
            |r| {
                Ok(ChatMessageWitness {
                    chat_id: r.get(0)?,
                    durable_sequence: row_u64(r, 1)?,
                    canonical_message_json: r.get(2)?,
                    message_digest: r.get(3)?,
                })
            },
        )
        .map_err(|_| compaction_error())
}

fn exact_frozen_message(
    connection: &Connection,
    row: &PlanRow,
    message: &ChatMessageWitness,
    budget: &mut StepBudget,
) -> Result<bool, SidecarError> {
    let Some(message_id) = row.introduced_message_id.as_deref() else {
        return Err(compaction_error());
    };
    let count: u32 = connection
        .query_row(
            "SELECT count(*) FROM frozen_message_sources WHERE message_id=?1;",
            [message_id],
            |r| r.get(0),
        )
        .map_err(sqlite_error)?;
    if count == 0 {
        return Ok(false);
    }
    if count != 1 {
        return Err(compaction_error());
    }
    let (types_valid, dependency): (bool, i64) = connection
        .query_row(
            "SELECT typeof(message_id)='text' AND typeof(source_event_rowid)='integer' \
                    AND typeof(source_event_id)='text' AND typeof(source_operation_id)='text' \
                    AND typeof(chat_id)='text' AND typeof(source_aggregate_sequence)='integer' \
                    AND typeof(durable_sequence)='integer' AND typeof(canonical_message_json)='text' \
                    AND typeof(message_digest)='text' AND typeof(frozen_at)='text',\
                    length(CAST(message_id AS BLOB))+length(CAST(source_event_id AS BLOB))+\
                    length(CAST(source_operation_id AS BLOB))+length(CAST(chat_id AS BLOB))+\
                    length(CAST(canonical_message_json AS BLOB))+length(CAST(message_digest AS BLOB))+\
                    length(CAST(frozen_at AS BLOB)) \
             FROM frozen_message_sources WHERE message_id=?1;",
            [message_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    if !types_valid {
        return Err(compaction_error());
    }
    budget.add_dependency(length(dependency)?)?;
    let source = connection
        .query_row(
            "SELECT message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,\
                    source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at \
             FROM frozen_message_sources WHERE message_id=?1;",
            [message_id],
            |r| {
                Ok(FrozenMessageSourceV1 {
                    message_id: r.get(0)?,
                    source_event_rowid: row_u64(r, 1)?,
                    source_event_id: r.get(2)?,
                    source_operation_id: r.get(3)?,
                    chat_id: r.get(4)?,
                    source_aggregate_sequence: row_u64(r, 5)?,
                    durable_sequence: row_u64(r, 6)?,
                    canonical_message_json: r.get(7)?,
                    message_digest: r.get(8)?,
                    frozen_at: r.get(9)?,
                })
            },
        )
        .map_err(|_| compaction_error())?;
    budget.add_dependency(71)?;
    validate_frozen_message_ref(FrozenMessageRef::from_source(&source), row, message)?;
    Ok(true)
}

fn validate_frozen_message_ref(
    source: FrozenMessageRef<'_>,
    row: &PlanRow,
    message: &ChatMessageWitness,
) -> Result<(), SidecarError> {
    const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
    validate_frozen_operation_timestamps_v1(&row.event_timestamp, source.frozen_at)
        .map_err(|_| compaction_error())?;
    if source.message_id.is_empty()
        || source.source_event_rowid == 0
        || source.source_event_rowid > MAX_JS_SAFE_INTEGER
        || source.source_event_id.is_empty()
        || source.source_operation_id.is_empty()
        || source.chat_id.is_empty()
        || source.source_aggregate_sequence == 0
        || source.source_aggregate_sequence > MAX_JS_SAFE_INTEGER
        || source.durable_sequence == 0
        || source.durable_sequence > MAX_JS_SAFE_INTEGER
        || source.canonical_message_json.is_empty()
        || source.message_digest.is_empty()
        || Some(source.message_id) != row.introduced_message_id.as_deref()
        || source.source_event_rowid != row.event_rowid
        || source.source_event_id != row.event_id
        || source.source_operation_id != row.operation_id
        || source.chat_id != message.chat_id
        || source.source_aggregate_sequence != row.aggregate_sequence
        || source.durable_sequence != message.durable_sequence
        || source.canonical_message_json != message.canonical_message_json
        || source.message_digest != message.message_digest
        || sha256_file_version_v1(source.canonical_message_json.as_bytes()) != source.message_digest
    {
        return Err(compaction_error());
    }
    Ok(())
}

fn verify_planned_row(
    connection: &Connection,
    row: &PlanRow,
    expect_live: bool,
    budget: &mut StepBudget,
) -> Result<(), SidecarError> {
    let live: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM events e JOIN event_source_index i ON i.event_rowid=e.rowid \
             JOIN operation_receipts r ON r.operation_id=e.operation_id \
             WHERE e.rowid=?1 AND e.event_id=?2 AND e.operation_id=?3 AND e.request_id IS ?4 \
               AND e.aggregate_type=?5 AND e.aggregate_id=?6 AND e.aggregate_sequence=?7 \
               AND e.event_type=?8 AND e.payload_schema_version=?9 AND e.timestamp=?10 \
               AND e.canonical_json_payload=?11 AND r.canonical_payload_json=?11 \
               AND r.canonical_payload_digest=?12 AND i.event_id=e.event_id \
               AND i.operation_id=e.operation_id AND i.aggregate_type=e.aggregate_type \
               AND i.aggregate_id=e.aggregate_id AND i.aggregate_sequence=e.aggregate_sequence);",
            params![sql_i64(row.event_rowid)?, row.event_id, row.operation_id, row.request_id,
                row.aggregate_type, row.aggregate_id, sql_i64(row.aggregate_sequence)?, row.event_type,
                row.payload_schema_version, row.event_timestamp, row.canonical_payload_json,
                row.canonical_payload_digest],
            |r| r.get(0),
        )
        .map_err(sqlite_error)?;
    if live != expect_live || !exact_frozen_operation(connection, row, budget)? {
        return Err(compaction_error());
    }
    if let Some(message_id) = row.introduced_message_id.as_deref() {
        let message = load_chat_message(connection, message_id, budget)?;
        if !exact_frozen_message(connection, row, &message, budget)? {
            return Err(compaction_error());
        }
    }
    Ok(())
}

fn insert_prefix(
    transaction: &Transaction<'_>,
    plan: &StorageCompactionPlanV1,
    budget: &mut StepBudget,
) -> Result<(), SidecarError> {
    let anchor = load_planned_row(transaction, plan, 1001, budget)?;
    let prefix_len = "prefix_"
        .len()
        .checked_add(plan.job_id.len())
        .ok_or_else(compaction_error)?;
    budget.add_dependency(prefix_len)?;
    let prefix_id = format!("prefix_{}", plan.job_id);
    let changed = transaction
        .execute(
            "INSERT INTO ledger_compaction_prefixes(\
               prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,\
               anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,\
               baseline_projection_digest,created_at,verified_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'sha256:'||?10,?11,?11);",
            params![
                prefix_id,
                plan.aggregate_type,
                plan.aggregate_id,
                sql_i64(plan.delete_through_sequence)?,
                sql_i64(anchor.event_rowid)?,
                anchor.event_id,
                anchor.operation_id,
                sql_i64(plan.anchor_aggregate_sequence)?,
                plan.baseline_id,
                plan.baseline_projection_digest,
                now()
            ],
        )
        .map_err(sqlite_error)?;
    if changed != 1 {
        return Err(compaction_error());
    }
    Ok(())
}

fn delete_batch(
    transaction: &Transaction<'_>,
    job: &StorageCompactionJobV3,
    plan: &StorageCompactionPlanV1,
    through: u32,
    budget: &mut StepBudget,
    failure: Option<CompactionFailurePoint>,
) -> Result<(), SidecarError> {
    let (prefix_type_valid, prefix_length): (bool, i64) = transaction
        .query_row(
            "SELECT typeof(prefix_id)='text',length(CAST(prefix_id AS BLOB)) \
             FROM ledger_compaction_prefixes WHERE aggregate_type=?1 \
             AND aggregate_id=?2 AND deleted_through_sequence=?3;",
            params![
                plan.aggregate_type,
                plan.aggregate_id,
                sql_i64(plan.delete_through_sequence)?
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    if !prefix_type_valid {
        return Err(compaction_error());
    }
    budget.add_dependency(length(prefix_length)?)?;
    let prefix_id: String = transaction
        .query_row(
            "SELECT prefix_id FROM ledger_compaction_prefixes WHERE aggregate_type=?1 \
             AND aggregate_id=?2 AND deleted_through_sequence=?3;",
            params![
                plan.aggregate_type,
                plan.aggregate_id,
                sql_i64(plan.delete_through_sequence)?
            ],
            |r| r.get(0),
        )
        .map_err(|_| compaction_error())?;
    let start = job.private_state.cursor.after_plan_ordinal + 1;
    let delete_end = through.min(STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1);
    for ordinal in start..=delete_end {
        let row = load_planned_row(transaction, plan, ordinal, budget)?;
        let changed = transaction
            .execute(
                "INSERT INTO ledger_compaction_delete_authority(\
                   job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,aggregate_type,\
                   aggregate_id,aggregate_sequence,authorized_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10);",
                params![
                    job.job_id,
                    ordinal - start + 1,
                    prefix_id,
                    sql_i64(row.event_rowid)?,
                    row.event_id,
                    row.operation_id,
                    row.aggregate_type,
                    row.aggregate_id,
                    sql_i64(row.aggregate_sequence)?,
                    now()
                ],
            )
            .map_err(sqlite_error)?;
        if changed != 1 {
            return Err(compaction_error());
        }
        if failure == Some(CompactionFailurePoint::AfterAuthorityInsert) {
            return Err(compaction_error());
        }
    }
    if start > delete_end {
        budget.add(0, 0)?;
        finish_batch(transaction, job, through)?;
    } else {
        let pending: bool = transaction
            .query_row(
                "SELECT state='pending' FROM maintenance_jobs WHERE job_id=?1;",
                [&job.job_id],
                |r| r.get(0),
            )
            .map_err(sqlite_error)?;
        if !pending {
            return Err(compaction_error());
        }
    }
    Ok(())
}

fn verify_equivalence(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
    budget: &mut StepBudget,
) -> Result<(), SidecarError> {
    let projection_table = match plan.aggregate_type.as_str() {
        "project" => "projects",
        "chat" => "chats",
        "run" => "runs",
        "lock" => "locks",
        _ => return Err(compaction_error()),
    };
    let key = match plan.aggregate_type.as_str() {
        "project" => "project_id",
        "chat" => "chat_id",
        "run" => "run_id",
        "lock" => "lock_id",
        _ => return Err(compaction_error()),
    };
    let projection_query =
        format!("SELECT projection_json FROM {projection_table} WHERE {key}=?1;");
    let projection_length_query = format!(
        "SELECT typeof(projection_json)='text',length(CAST(projection_json AS BLOB)) \
         FROM {projection_table} WHERE {key}=?1;"
    );
    let baseline_lengths: (bool, i64, i64) = connection
        .query_row(
            "SELECT typeof(canonical_projection_json)='text' AND typeof(projection_digest)='text',\
                    length(CAST(canonical_projection_json AS BLOB)),\
                    length(CAST(projection_digest AS BLOB)) \
             FROM ledger_baselines WHERE baseline_id=?1 AND aggregate_type=?2 AND aggregate_id=?3 \
               AND cutoff_aggregate_sequence=?4;",
            params![
                plan.baseline_id,
                plan.aggregate_type,
                plan.aggregate_id,
                sql_i64(plan.delete_through_sequence)?
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    let latest_lengths: (bool, i64, i64) = connection
        .query_row(
            "SELECT typeof(canonical_json_payload)='text' AND typeof(aggregate_type)='text',\
                    length(CAST(canonical_json_payload AS BLOB)),length(CAST(aggregate_type AS BLOB)) \
             FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 \
             ORDER BY aggregate_sequence DESC LIMIT 1;",
            params![plan.aggregate_type, plan.aggregate_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    let projection_lengths: (bool, i64) = connection
        .query_row(&projection_length_query, [&plan.aggregate_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(|_| compaction_error())?;
    if !baseline_lengths.0 || !latest_lengths.0 || !projection_lengths.0 {
        return Err(compaction_error());
    }
    budget.add_bytes(
        checked_lengths(&[baseline_lengths.1, latest_lengths.1, projection_lengths.1])?,
        checked_lengths(&[baseline_lengths.2, latest_lengths.2])?,
    )?;
    let baseline: (String, String) = connection
        .query_row(
            "SELECT canonical_projection_json,projection_digest FROM ledger_baselines \
             WHERE baseline_id=?1 AND aggregate_type=?2 AND aggregate_id=?3 \
               AND cutoff_aggregate_sequence=?4;",
            params![
                plan.baseline_id,
                plan.aggregate_type,
                plan.aggregate_id,
                sql_i64(plan.delete_through_sequence)?
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    if sha256_file_version_v1(baseline.0.as_bytes()) != baseline.1
        || baseline.1.strip_prefix("sha256:") != Some(plan.baseline_projection_digest.as_str())
    {
        return Err(compaction_error());
    }
    let facts: (u32, u32, u32, u32, u32) = connection
        .query_row(
            "SELECT \
               (SELECT count(*) FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 \
                  AND aggregate_sequence<=?3),\
               (SELECT count(*) FROM event_source_index WHERE aggregate_type=?1 AND aggregate_id=?2 \
                  AND aggregate_sequence<=?3),\
               (SELECT count(*) FROM frozen_operation_sources WHERE aggregate_type=?1 \
                  AND aggregate_id=?2 AND aggregate_sequence>?4 AND aggregate_sequence<=?3),\
               (SELECT count(*) FROM operation_receipts WHERE aggregate_type=?1 AND aggregate_id=?2 \
                  AND aggregate_sequence>?4 AND aggregate_sequence<=?3),\
               (SELECT count(*) FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 \
                  AND aggregate_sequence=?5);",
            params![plan.aggregate_type, plan.aggregate_id, sql_i64(plan.delete_through_sequence)?,
                sql_i64(plan.retained_boundary_sequence)?, sql_i64(plan.anchor_aggregate_sequence)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(sqlite_error)?;
    if facts != (0, 0, 1000, 1000, 1) {
        return Err(compaction_error());
    }
    let latest: (String, String) = connection
        .query_row(
            "SELECT canonical_json_payload,aggregate_type FROM events WHERE aggregate_type=?1 \
             AND aggregate_id=?2 ORDER BY aggregate_sequence DESC LIMIT 1;",
            params![plan.aggregate_type, plan.aggregate_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    if latest.1 != plan.aggregate_type {
        return Err(compaction_error());
    }
    let projection: String = connection
        .query_row(&projection_query, [&plan.aggregate_id], |r| r.get(0))
        .map_err(|_| compaction_error())?;
    if projection != latest.0 {
        return Err(compaction_error());
    }
    Ok(())
}

fn load_active_job(
    connection: &Connection,
) -> Result<Option<StorageCompactionJobV3>, SidecarError> {
    let facts: (bool, u32, i64) = connection
        .query_row(
            "SELECT coalesce(min(typeof(job_id)='text'),1),count(*),\
                    coalesce(max(length(CAST(job_id AS BLOB))),0) \
             FROM (SELECT job_id FROM maintenance_jobs \
                   INDEXED BY maintenance_jobs_single_active_lossless_compaction \
                   WHERE job_kind='losslessCompaction' AND state IN ('pending','running') \
                   LIMIT 2);",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(sqlite_error)?;
    if !facts.0 || facts.1 > 1 || length(facts.2)? > DEPENDENCY_BYTES {
        return Err(compaction_error());
    }
    if facts.1 == 0 {
        return Ok(None);
    }
    let id: String = connection
        .query_row(
            "SELECT job_id FROM maintenance_jobs WHERE job_kind='losslessCompaction' \
             AND state IN ('pending','running');",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    load_exact_job(connection, &id).map(Some)
}

fn load_exact_job(
    connection: &Connection,
    job_id: &str,
) -> Result<StorageCompactionJobV3, SidecarError> {
    let lengths: (bool, i64, i64, i64) = connection
        .query_row(
            "SELECT typeof(job_id)='text' AND typeof(state)='text' AND typeof(cutoff_json)='text' \
                    AND (failure_kind IS NULL OR typeof(failure_kind)='text') \
                    AND (completed_at IS NULL OR typeof(completed_at)='text'),\
                    length(CAST(job_id AS BLOB))+length(CAST(state AS BLOB)),\
                    length(CAST(cutoff_json AS BLOB)),\
                    coalesce(length(CAST(failure_kind AS BLOB)),0)+\
                    coalesce(length(CAST(completed_at AS BLOB)),0) \
             FROM maintenance_jobs WHERE job_id=?1 AND job_kind='losslessCompaction';",
            [job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| compaction_error())?;
    if !lengths.0 || checked_lengths(&[lengths.1, lengths.2, lengths.3])? > DEPENDENCY_BYTES {
        return Err(compaction_error());
    }
    let (id, state, cutoff, failure, completed): (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = connection
        .query_row(
            "SELECT job_id,state,cutoff_json,failure_kind,completed_at FROM maintenance_jobs \
             WHERE job_id=?1 AND job_kind='losslessCompaction';",
            [job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(|_| compaction_error())?;
    let envelope: Value = serde_json::from_str(&cutoff).map_err(|_| compaction_error())?;
    let private = envelope
        .get("storageCompactionV3PrivateState")
        .cloned()
        .ok_or_else(compaction_error)?;
    let authority: u32 = connection
        .query_row(
            "SELECT count(*) FROM ledger_compaction_delete_authority WHERE job_id=?1;",
            [job_id],
            |r| r.get(0),
        )
        .map_err(sqlite_error)?;
    validate_storage_compaction_job_v3(&json!({
        "jobId": id,
        "state": state,
        "privateState": private,
        "authorityCount": authority,
        "failureKind": failure,
        "completedAt": completed,
    }))
    .map_err(|_| compaction_error())
}

fn load_plan(
    connection: &Connection,
    job_id: &str,
) -> Result<StorageCompactionPlanV1, SidecarError> {
    let lengths: (bool, i64, i64) = connection
        .query_row(
            "SELECT typeof(job_id)='text' AND typeof(aggregate_type)='text' \
                    AND typeof(aggregate_id)='text' AND typeof(baseline_id)='text' \
                    AND typeof(baseline_projection_digest)='text' \
                    AND (previous_successful_job_id IS NULL OR typeof(previous_successful_job_id)='text') \
                    AND typeof(created_at)='text' AND typeof(planned_at)='text' \
                    AND typeof(plan_schema_version)='integer' AND typeof(cutoff_event_rowid)='integer' \
                    AND typeof(retained_boundary_sequence)='integer' \
                    AND typeof(delete_through_sequence)='integer' \
                    AND typeof(anchor_aggregate_sequence)='integer' AND typeof(expected_rows)='integer',\
                    length(CAST(job_id AS BLOB))+length(CAST(aggregate_type AS BLOB))+\
                    length(CAST(aggregate_id AS BLOB))+length(CAST(baseline_id AS BLOB))+\
                    length(CAST(baseline_projection_digest AS BLOB))+\
                    coalesce(length(CAST(previous_successful_job_id AS BLOB)),0),\
                    length(CAST(created_at AS BLOB))+length(CAST(planned_at AS BLOB)) \
             FROM storage_compaction_plans WHERE job_id=?1;",
            [job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    if !lengths.0 || checked_lengths(&[lengths.1, lengths.2])? > DEPENDENCY_BYTES {
        return Err(compaction_error());
    }
    let value = connection
        .query_row(
            "SELECT job_id,plan_schema_version,cutoff_event_rowid,aggregate_type,aggregate_id,\
                    retained_boundary_sequence,delete_through_sequence,anchor_aggregate_sequence,\
                    expected_rows,baseline_id,baseline_projection_digest,previous_successful_job_id,\
                    created_at,planned_at FROM storage_compaction_plans WHERE job_id=?1;",
            [job_id],
            |r| {
                Ok(json!({
                    "jobId":r.get::<_,String>(0)?, "planSchemaVersion":r.get::<_,u32>(1)?,
                    "cutoffEventRowid":row_u64(r,2)?, "aggregateType":r.get::<_,String>(3)?,
                    "aggregateId":r.get::<_,String>(4)?, "retainedBoundarySequence":row_u64(r,5)?,
                    "deleteThroughSequence":row_u64(r,6)?, "anchorAggregateSequence":row_u64(r,7)?,
                    "expectedRows":r.get::<_,u32>(8)?, "baselineId":r.get::<_,String>(9)?,
                    "baselineProjectionDigest":r.get::<_,String>(10)?,
                    "previousSuccessfulJobId":r.get::<_,Option<String>>(11)?,
                    "createdAt":r.get::<_,String>(12)?, "plannedAt":r.get::<_,String>(13)?,
                }))
            },
        )
        .map_err(|_| compaction_error())?;
    validate_storage_compaction_plan_v1(&value).map_err(|_| compaction_error())
}

fn private_state(
    cutoff: u64,
    phase: StorageCompactionPhaseV3,
    after: u32,
    exhausted: bool,
) -> Value {
    let index = phase_index(phase);
    json!({"storageCompactionV3PrivateState":{
        "schemaVersion":3,
        "cursor":{"schemaVersion":3,"cutoffEventRowid":cutoff,"phase":phase_name(phase),
            "afterPlanOrdinal":after,"stepThroughPlanOrdinal":null,"scanExhausted":exhausted},
        "planRowsComplete":index>0 || exhausted,
        "operationsVerified":index>6 || (index==6 && exhausted),
        "equivalenceVerified":index>7 || (index==7 && exhausted),
        "planRowsReleased":index>8 || (index==8 && exhausted),
    }})
}

fn phase_limit(phase: StorageCompactionPhaseV3) -> u32 {
    if matches!(
        phase,
        StorageCompactionPhaseV3::ReleasePlan | StorageCompactionPhaseV3::Finalize
    ) {
        STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1
    } else {
        STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
    }
}

fn next_phase(phase: StorageCompactionPhaseV3) -> Option<StorageCompactionPhaseV3> {
    Some(match phase {
        StorageCompactionPhaseV3::PlanEligibility => StorageCompactionPhaseV3::FreezeOperations,
        StorageCompactionPhaseV3::FreezeOperations => StorageCompactionPhaseV3::FreezeMessages,
        StorageCompactionPhaseV3::FreezeMessages => StorageCompactionPhaseV3::VerifyPrefix,
        StorageCompactionPhaseV3::VerifyPrefix => StorageCompactionPhaseV3::AuthorizeDelete,
        StorageCompactionPhaseV3::AuthorizeDelete => StorageCompactionPhaseV3::DeletePrefix,
        StorageCompactionPhaseV3::DeletePrefix => StorageCompactionPhaseV3::VerifyOperations,
        StorageCompactionPhaseV3::VerifyOperations => StorageCompactionPhaseV3::VerifyEquivalence,
        StorageCompactionPhaseV3::VerifyEquivalence => StorageCompactionPhaseV3::ReleasePlan,
        StorageCompactionPhaseV3::ReleasePlan => StorageCompactionPhaseV3::Finalize,
        StorageCompactionPhaseV3::Finalize => return None,
    })
}

fn phase_index(phase: StorageCompactionPhaseV3) -> usize {
    match phase {
        StorageCompactionPhaseV3::PlanEligibility => 0,
        StorageCompactionPhaseV3::FreezeOperations => 1,
        StorageCompactionPhaseV3::FreezeMessages => 2,
        StorageCompactionPhaseV3::VerifyPrefix => 3,
        StorageCompactionPhaseV3::AuthorizeDelete => 4,
        StorageCompactionPhaseV3::DeletePrefix => 5,
        StorageCompactionPhaseV3::VerifyOperations => 6,
        StorageCompactionPhaseV3::VerifyEquivalence => 7,
        StorageCompactionPhaseV3::ReleasePlan => 8,
        StorageCompactionPhaseV3::Finalize => 9,
    }
}

fn phase_name(phase: StorageCompactionPhaseV3) -> &'static str {
    match phase {
        StorageCompactionPhaseV3::PlanEligibility => "planEligibility",
        StorageCompactionPhaseV3::FreezeOperations => "freezeOperations",
        StorageCompactionPhaseV3::FreezeMessages => "freezeMessages",
        StorageCompactionPhaseV3::VerifyPrefix => "verifyPrefix",
        StorageCompactionPhaseV3::AuthorizeDelete => "authorizeDelete",
        StorageCompactionPhaseV3::DeletePrefix => "deletePrefix",
        StorageCompactionPhaseV3::VerifyOperations => "verifyOperations",
        StorageCompactionPhaseV3::VerifyEquivalence => "verifyEquivalence",
        StorageCompactionPhaseV3::ReleasePlan => "releasePlan",
        StorageCompactionPhaseV3::Finalize => "finalize",
    }
}

fn progress(job: &StorageCompactionJobV3) -> CompactionStepResult {
    CompactionStepResult::ArmedWorked {
        job_id: job.job_id.clone(),
        phase: job.private_state.cursor.phase,
        after_plan_ordinal: job.private_state.cursor.after_plan_ordinal,
    }
}

fn checked_lengths(values: &[i64]) -> Result<usize, SidecarError> {
    values.iter().try_fold(0usize, |total, value| {
        total
            .checked_add(length(*value)?)
            .ok_or_else(compaction_error)
    })
}

fn checked_usize_sum(values: &[usize]) -> Result<usize, SidecarError> {
    values.iter().try_fold(0usize, |total, value| {
        total.checked_add(*value).ok_or_else(compaction_error)
    })
}

fn checked_scale(value: usize, factor: usize) -> Result<usize, SidecarError> {
    value.checked_mul(factor).ok_or_else(compaction_error)
}

fn frozen_operation_borrowed_dependency_len(
    source: FrozenOperationRecordRef<'_>,
) -> Result<usize, SidecarError> {
    checked_usize_sum(&[
        source.operation_id.len(),
        source.source_event_id.len(),
        source.request_id.map(str::len).unwrap_or(0),
        source.aggregate_type.len(),
        source.aggregate_id.len(),
        source.event_type.len(),
        source.canonical_payload_digest.len(),
        source.event_timestamp.len(),
        source.frozen_at.len(),
    ])
}

fn json_string_len(value: &str) -> Result<usize, SidecarError> {
    let body = value.chars().try_fold(0usize, |total, character| {
        let bytes = match character {
            '"' | '\\' | '\u{0008}' | '\u{000c}' | '\n' | '\r' | '\t' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => character.len_utf8(),
        };
        total.checked_add(bytes).ok_or_else(compaction_error)
    })?;
    body.checked_add(2).ok_or_else(compaction_error)
}

fn decimal_len(mut value: u64) -> usize {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

fn frozen_operation_record_len(
    source: FrozenOperationRecordRef<'_>,
) -> Result<usize, SidecarError> {
    let request_id = source
        .request_id
        .map(json_string_len)
        .transpose()?
        .unwrap_or(4);
    checked_usize_sum(&[
        r#"{"aggregateId":"#.len(),
        json_string_len(source.aggregate_id)?,
        ",\"aggregateSequence\":".len(),
        decimal_len(source.aggregate_sequence),
        ",\"aggregateType\":".len(),
        json_string_len(source.aggregate_type)?,
        ",\"canonicalPayloadDigest\":".len(),
        json_string_len(source.canonical_payload_digest)?,
        ",\"canonicalPayloadJson\":".len(),
        json_string_len(source.canonical_payload_json)?,
        ",\"eventTimestamp\":".len(),
        json_string_len(source.event_timestamp)?,
        ",\"eventType\":".len(),
        json_string_len(source.event_type)?,
        ",\"frozenAt\":".len(),
        json_string_len(source.frozen_at)?,
        ",\"operationId\":".len(),
        json_string_len(source.operation_id)?,
        ",\"payloadSchemaVersion\":".len(),
        decimal_len(u64::from(source.payload_schema_version)),
        ",\"recordSchemaVersion\":1,".len(),
        "\"requestId\":".len(),
        request_id,
        ",\"sourceEventId\":".len(),
        json_string_len(source.source_event_id)?,
        ",\"sourceEventRowid\":".len(),
        decimal_len(source.source_event_rowid),
        1,
    ])
}

fn length(value: i64) -> Result<usize, SidecarError> {
    usize::try_from(value).map_err(|_| compaction_error())
}

fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn sql_i64(value: u64) -> Result<i64, SidecarError> {
    i64::try_from(value).map_err(|_| compaction_error())
}

fn baseline_id(
    aggregate_type: &str,
    aggregate_id: &str,
    sequence: u64,
) -> Result<String, SidecarError> {
    let source = format!("{aggregate_type}\0{aggregate_id}\0{sequence}");
    let digest = sha256_file_version_v1(source.as_bytes());
    let raw = digest
        .strip_prefix("sha256:")
        .ok_or_else(compaction_error)?;
    Ok(format!("baseline_{raw}"))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn sqlite_error(_error: rusqlite::Error) -> SidecarError {
    compaction_error()
}

fn compaction_error() -> SidecarError {
    SidecarError::new(
        "storageCompactionFailed",
        "private storage compaction could not prove a safe bounded step",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_plan() -> StorageCompactionPlanV1 {
        StorageCompactionPlanV1 {
            job_id: "compact_test".to_string(),
            plan_schema_version: 1,
            cutoff_event_rowid: 1001,
            aggregate_type: "project".to_string(),
            aggregate_id: "project_test".to_string(),
            retained_boundary_sequence: 0,
            delete_through_sequence: 1000,
            anchor_aggregate_sequence: 1001,
            expected_rows: 1001,
            baseline_id: "baseline_test".to_string(),
            baseline_projection_digest: "0".repeat(64),
            previous_successful_job_id: None,
            created_at: "2026-09-09T00:00:00Z".to_string(),
            planned_at: "2026-09-09T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn step_budget_accepts_exact_limits_and_rejects_plus_one() {
        let mut work = StepBudget::default();
        for _ in 0..STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1 {
            work.add(0, 0).unwrap();
        }
        assert!(work.add(0, 0).is_err());

        let mut primary = StepBudget::default();
        primary.add(PRIMARY_BYTES, 0).unwrap();
        assert!(primary.add(1, 0).is_err());

        let mut dependency = StepBudget::default();
        dependency.add(0, DEPENDENCY_BYTES).unwrap();
        assert!(dependency.add_dependency(1).is_err());
    }

    #[test]
    fn frozen_operation_record_length_matches_canonical_output() {
        let mut source = FrozenOperationSourceV1 {
            operation_id: "op_\"\\".to_string(),
            source_event_rowid: 42,
            source_event_id: "evt_\n".to_string(),
            request_id: Some("request_\u{0001}".to_string()),
            aggregate_type: "project".to_string(),
            aggregate_id: "project_한글".to_string(),
            aggregate_sequence: 123,
            event_type: "project.updated".to_string(),
            payload_schema_version: 1,
            canonical_payload_json: "{\"value\":\"x\"}".to_string(),
            canonical_payload_digest: "a".repeat(64),
            event_timestamp: "2026-09-09T00:00:00Z".to_string(),
            canonical_record_json: String::new(),
            record_digest: String::new(),
            frozen_at: "2026-09-09T00:00:01Z".to_string(),
        };
        let expected =
            frozen_operation_record_len(FrozenOperationRecordRef::from_source(&source)).unwrap();
        source.canonical_record_json = canonical_frozen_operation_record_v1(&source).unwrap();
        assert_eq!(expected, source.canonical_record_json.len());
    }

    #[test]
    fn chat_message_preflight_accepts_exact_dependency_limit_and_rejects_plus_one() {
        for (size, accepted) in [(DEPENDENCY_BYTES, true), (DEPENDENCY_BYTES + 1, false)] {
            let connection = Connection::open_in_memory().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE chat_messages(\
                       message_id TEXT PRIMARY KEY,chat_id TEXT NOT NULL,event_sequence INTEGER NOT NULL,\
                       canonical_message_json TEXT NOT NULL,message_digest TEXT NOT NULL);\
                     CREATE TABLE mutation_marker(value INTEGER NOT NULL);\
                     INSERT INTO mutation_marker VALUES(7);",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO chat_messages VALUES('message_test',?1,1,'','');",
                    ["x".repeat(size)],
                )
                .unwrap();
            let mut budget = StepBudget::default();
            assert_eq!(
                load_chat_message(&connection, "message_test", &mut budget).is_ok(),
                accepted
            );
            assert_eq!(
                connection
                    .query_row("SELECT value FROM mutation_marker;", [], |row| row
                        .get::<_, u32>(0))
                    .unwrap(),
                7
            );
        }
    }

    #[test]
    fn equivalence_inputs_preflight_each_primary_string_before_allocation() {
        let oversized = "x".repeat(PRIMARY_BYTES + 1);
        for (table, column) in [
            ("ledger_baselines", "canonical_projection_json"),
            ("events", "canonical_json_payload"),
            ("projects", "projection_json"),
        ] {
            let connection = Connection::open_in_memory().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE ledger_baselines(\
                       baseline_id TEXT,aggregate_type TEXT,aggregate_id TEXT,\
                       cutoff_aggregate_sequence INTEGER,canonical_projection_json TEXT,projection_digest TEXT);\
                     CREATE TABLE events(aggregate_type TEXT,aggregate_id TEXT,aggregate_sequence INTEGER,\
                       canonical_json_payload TEXT);\
                     CREATE TABLE projects(project_id TEXT,projection_json TEXT);\
                     CREATE TABLE mutation_marker(value INTEGER NOT NULL);\
                     INSERT INTO ledger_baselines VALUES(\
                       'baseline_test','project','project_test',1000,'','sha256:e3b0c44298fc1c149afbf4c8996fb924\
                       27ae41e4649b934ca495991b7852b855');\
                     INSERT INTO events VALUES('project','project_test',1001,'');\
                     INSERT INTO projects VALUES('project_test','');\
                     INSERT INTO mutation_marker VALUES(9);",
                )
                .unwrap();
            connection
                .execute(&format!("UPDATE {table} SET {column}=?1;"), [&oversized])
                .unwrap();
            let mut budget = StepBudget::default();
            assert!(verify_equivalence(&connection, &test_plan(), &mut budget).is_err());
            assert!(budget.primary > PRIMARY_BYTES, "{table}.{column}");
            assert_eq!(
                connection
                    .query_row("SELECT value FROM mutation_marker;", [], |row| row
                        .get::<_, u32>(0))
                    .unwrap(),
                9
            );
        }

        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE ledger_baselines(\
                   baseline_id TEXT,aggregate_type TEXT,aggregate_id TEXT,\
                   cutoff_aggregate_sequence INTEGER,canonical_projection_json TEXT,projection_digest TEXT);\
                 CREATE TABLE events(aggregate_type TEXT,aggregate_id TEXT,aggregate_sequence INTEGER,\
                   canonical_json_payload TEXT);\
                 CREATE TABLE projects(project_id TEXT,projection_json TEXT);\
                 INSERT INTO ledger_baselines VALUES(\
                   'baseline_test','project','project_test',1000,'','sha256:e3b0c44298fc1c149afbf4c8996fb924\
                   27ae41e4649b934ca495991b7852b855');\
                 INSERT INTO events VALUES('project','project_test',1001,'');",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO projects VALUES('project_test',?1);",
                ["x".repeat(PRIMARY_BYTES)],
            )
            .unwrap();
        let mut budget = StepBudget::default();
        assert!(verify_equivalence(&connection, &test_plan(), &mut budget).is_err());
        assert_eq!(budget.primary, PRIMARY_BYTES);
    }

    #[test]
    fn existing_frozen_message_requires_exact_metadata_and_digest() {
        let canonical = "{\"messageId\":\"message_test\"}".to_string();
        let digest = sha256_file_version_v1(canonical.as_bytes());
        let row = PlanRow {
            ordinal: 1,
            event_rowid: 1,
            event_id: "event_test".to_string(),
            operation_id: "operation_test".to_string(),
            request_id: None,
            aggregate_type: "chat".to_string(),
            aggregate_id: "chat_test".to_string(),
            aggregate_sequence: 1,
            event_type: "chat.messageAppended".to_string(),
            payload_schema_version: 1,
            event_timestamp: "2026-09-09T00:00:00Z".to_string(),
            canonical_payload_json: "{}".to_string(),
            canonical_payload_digest: "0".repeat(64),
            introduced_message_id: Some("message_test".to_string()),
            introduced_message_digest: digest.strip_prefix("sha256:").map(str::to_string),
            decision: "delete",
            baseline_id: Some("baseline_test".to_string()),
            baseline_projection_digest: Some("0".repeat(64)),
            operation_frozen_before_plan: false,
            message_frozen_before_plan: false,
        };
        let message = ChatMessageWitness {
            chat_id: "chat_test".to_string(),
            durable_sequence: 1,
            canonical_message_json: canonical,
            message_digest: digest,
        };
        for mutation in [
            "UPDATE frozen_message_sources SET frozen_at='not-a-time';",
            "UPDATE frozen_message_sources SET message_digest='sha256:bad';",
            "UPDATE frozen_message_sources SET chat_id='other_chat';",
        ] {
            let connection = Connection::open_in_memory().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE frozen_message_sources(\
                       message_id TEXT PRIMARY KEY,source_event_rowid INTEGER,source_event_id TEXT,\
                       source_operation_id TEXT,chat_id TEXT,source_aggregate_sequence INTEGER,\
                       durable_sequence INTEGER,canonical_message_json TEXT,message_digest TEXT,frozen_at TEXT);",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO frozen_message_sources VALUES(\
                       'message_test',1,'event_test','operation_test','chat_test',1,1,?1,?2,\
                       '2026-09-09T00:00:01Z');",
                    params![message.canonical_message_json, message.message_digest],
                )
                .unwrap();
            let mut valid_budget = StepBudget::default();
            assert!(exact_frozen_message(&connection, &row, &message, &mut valid_budget).unwrap());
            connection.execute_batch(mutation).unwrap();
            let mut invalid_budget = StepBudget::default();
            assert!(
                exact_frozen_message(&connection, &row, &message, &mut invalid_budget).is_err()
            );
        }
    }
}
