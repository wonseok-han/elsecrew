use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_protocol::project_v1::sha256_file_version_v1;
use hello_agent_protocol::storage_lifetime_v1::{
    canonical_frozen_operation_record_v1, validate_storage_compaction_job_v3,
    validate_storage_compaction_plan_v1, FrozenOperationSourceV1, StorageCompactionJobV3,
    StorageCompactionPhaseV3, StorageCompactionPlanV1, STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1,
    STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1, STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1,
    STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
    STORAGE_LIFETIME_SCHEMA_VERSION_V10,
};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
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
    NoCandidate,
    Progress {
        job_id: String,
        phase: StorageCompactionPhaseV3,
        after_plan_ordinal: u32,
    },
    Completed {
        job_id: String,
    },
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
        self.dependency = self
            .dependency
            .checked_add(dependency)
            .ok_or_else(compaction_error)?;
        if self.dependency > DEPENDENCY_BYTES {
            return Err(compaction_error());
        }
        Ok(())
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
                return Ok(CompactionStepResult::NoCandidate);
            }
            let Some(job) = create_candidate(connection, target)? else {
                return Ok(CompactionStepResult::NoCandidate);
            };
            job
        }
    };

    if cancellation.cancelled(CompactionCancellationPoint::BeforeArm) {
        return Ok(progress(&job));
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
        != hello_agent_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Running
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
        == hello_agent_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Succeeded
    {
        Ok(CompactionStepResult::Completed {
            job_id: next.job_id,
        })
    } else {
        Ok(progress(&next))
    }
}

fn validate_target(target: &CompactionTarget) -> Result<(), SidecarError> {
    if !matches!(
        target.aggregate_type.as_str(),
        "project" | "chat" | "run" | "lock"
    ) || target.aggregate_id.is_empty()
        || target.aggregate_id.len() > DEPENDENCY_BYTES
    {
        return Err(compaction_error());
    }
    Ok(())
}

fn verify_runtime_fences(connection: &Connection) -> Result<(), SidecarError> {
    let facts: (u32, String, u32, u32, u32) = connection
        .query_row(
            "SELECT storage_schema_version,materialization_state,\
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
    if facts.0 != STORAGE_LIFETIME_SCHEMA_VERSION_V10
        || facts.1 != "ready"
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
    let baseline_lengths: (i64, i64, i64) = transaction
        .query_row(
            "SELECT length(CAST(baseline_id AS BLOB)),\
                    length(CAST(canonical_projection_json AS BLOB)),\
                    length(CAST(projection_digest AS BLOB)) \
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
    let dependency =
        checked_lengths(&[baseline_lengths.0, baseline_lengths.1, baseline_lengths.2])?;
    if dependency > DEPENDENCY_BYTES
        || usize::try_from(baseline_lengths.1).map_err(|_| compaction_error())? > PRIMARY_BYTES
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
    let source_lengths: (i64, i64) = transaction
        .query_row(
            "SELECT length(CAST(e.canonical_json_payload AS BLOB)),\
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
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    if length(source_lengths.0)? > PRIMARY_BYTES
        || checked_lengths(&[source_lengths.0, source_lengths.1])? > DEPENDENCY_BYTES
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
    let mut statement = connection
        .prepare(
            "SELECT p.job_id FROM storage_compaction_plans p JOIN maintenance_jobs j ON j.job_id=p.job_id \
             WHERE p.aggregate_type=?1 AND p.aggregate_id=?2 AND p.delete_through_sequence=?3 \
               AND j.state='succeeded' ORDER BY p.job_id LIMIT 2;",
        )
        .map_err(sqlite_error)?;
    let mut rows = statement
        .query(params![
            target.aggregate_type,
            target.aggregate_id,
            sql_i64(boundary)?
        ])
        .map_err(sqlite_error)?;
    let first = rows
        .next()
        .map_err(sqlite_error)?
        .map(|row| row.get::<_, String>(0))
        .transpose()
        .map_err(sqlite_error)?;
    if first.is_none() || rows.next().map_err(sqlite_error)?.is_some() {
        return Err(compaction_error());
    }
    Ok(first)
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
        return advance_phase(transaction, job, plan);
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
                freeze_operation(transaction, &row)?;
            }
            StorageCompactionPhaseV3::FreezeMessages => {
                let row = load_planned_row(transaction, plan, ordinal, budget)?;
                freeze_message(transaction, &row, budget)?;
            }
            StorageCompactionPhaseV3::VerifyPrefix | StorageCompactionPhaseV3::AuthorizeDelete => {
                let row = load_planned_row(transaction, plan, ordinal, budget)?;
                verify_planned_row(transaction, &row, true)?;
            }
            StorageCompactionPhaseV3::DeletePrefix => {}
            StorageCompactionPhaseV3::VerifyOperations
            | StorageCompactionPhaseV3::VerifyEquivalence => {
                let row = load_planned_row(transaction, plan, ordinal, budget)?;
                verify_planned_row(transaction, &row, ordinal == 1001)?;
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
        verify_equivalence(transaction, plan)?;
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
) -> Result<(), SidecarError> {
    if job.private_state.cursor.phase == StorageCompactionPhaseV3::VerifyPrefix {
        insert_prefix(transaction, plan)?;
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
            != hello_agent_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Pending
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
        != hello_agent_protocol::storage_lifetime_v1::StorageCompactionJobStateV1::Succeeded
    {
        return Err(compaction_error());
    }
    Ok(CompactionStepResult::Completed {
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
    let lengths: (i64, i64) = connection
        .query_row(
            "SELECT length(CAST(e.canonical_json_payload AS BLOB)),\
                    length(CAST(e.event_id AS BLOB))+length(CAST(e.operation_id AS BLOB))+\
                    coalesce(length(CAST(e.request_id AS BLOB)),0)+length(CAST(e.aggregate_type AS BLOB))+\
                    length(CAST(e.aggregate_id AS BLOB))+length(CAST(e.event_type AS BLOB))+\
                    length(CAST(e.timestamp AS BLOB))+coalesce(length(CAST(i.introduced_message_id AS BLOB)),0)+\
                    coalesce(length(CAST(m.message_digest AS BLOB)),0) \
             FROM events e JOIN event_source_index i ON i.event_rowid=e.rowid \
             JOIN operation_receipts r ON r.operation_id=e.operation_id \
             LEFT JOIN chat_messages m ON m.message_id=i.introduced_message_id \
             WHERE e.aggregate_type=?1 AND e.aggregate_id=?2 AND e.aggregate_sequence=?3 \
               AND e.rowid<=?4 AND r.canonical_payload_json=e.canonical_json_payload;",
            params![plan.aggregate_type, plan.aggregate_id, sql_i64(sequence)?, sql_i64(plan.cutoff_event_rowid)?],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    budget.add(length(lengths.0)?, length(lengths.1)?)?;
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
    let lengths: (i64, i64) = connection
        .query_row(
            "SELECT length(CAST(canonical_payload_json AS BLOB)),\
                    length(CAST(event_id AS BLOB))+length(CAST(operation_id AS BLOB))+\
                    coalesce(length(CAST(request_id AS BLOB)),0)+length(CAST(aggregate_type AS BLOB))+\
                    length(CAST(aggregate_id AS BLOB))+length(CAST(event_type AS BLOB))+\
                    length(CAST(event_timestamp AS BLOB))+length(CAST(canonical_payload_digest AS BLOB))+\
                    coalesce(length(CAST(introduced_message_id AS BLOB)),0)+\
                    coalesce(length(CAST(introduced_message_digest AS BLOB)),0)+\
                    coalesce(length(CAST(baseline_id AS BLOB)),0)+\
                    coalesce(length(CAST(baseline_projection_digest AS BLOB)),0) \
             FROM storage_compaction_plan_rows WHERE job_id=?1 AND ordinal=?2;",
            params![plan.job_id, ordinal],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    budget.add(length(lengths.0)?, length(lengths.1)?)?;
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

fn freeze_operation(transaction: &Transaction<'_>, row: &PlanRow) -> Result<(), SidecarError> {
    if exact_frozen_operation(transaction, row)? {
        return Ok(());
    }
    let frozen_at = now();
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

fn exact_frozen_operation(connection: &Connection, row: &PlanRow) -> Result<bool, SidecarError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM frozen_operation_sources WHERE operation_id=?1 \
             AND source_event_rowid=?2 AND source_event_id=?3 AND request_id IS ?4 \
             AND aggregate_type=?5 AND aggregate_id=?6 AND aggregate_sequence=?7 \
             AND event_type=?8 AND payload_schema_version=?9 AND canonical_payload_json=?10 \
             AND canonical_payload_digest=?11 AND event_timestamp=?12);",
            params![
                row.operation_id,
                sql_i64(row.event_rowid)?,
                row.event_id,
                row.request_id,
                row.aggregate_type,
                row.aggregate_id,
                sql_i64(row.aggregate_sequence)?,
                row.event_type,
                row.payload_schema_version,
                row.canonical_payload_json,
                row.canonical_payload_digest,
                row.event_timestamp,
            ],
            |r| r.get(0),
        )
        .map_err(sqlite_error)
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
    let lengths: (i64, i64) = transaction
        .query_row(
            "SELECT length(CAST(canonical_message_json AS BLOB)),length(CAST(message_digest AS BLOB)) \
             FROM chat_messages WHERE message_id=?1;",
            [message_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    budget.add_dependency(checked_lengths(&[lengths.0, lengths.1])?)?;
    let (chat_id, durable, canonical, digest): (String, i64, String, String) = transaction
        .query_row(
            "SELECT m.chat_id,m.event_sequence,m.canonical_message_json,m.message_digest \
             FROM chat_messages m WHERE m.message_id=?1;",
            [message_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| compaction_error())?;
    if digest.strip_prefix("sha256:") != row.introduced_message_digest.as_deref()
        || sha256_file_version_v1(canonical.as_bytes()) != digest
    {
        return Err(compaction_error());
    }
    let exact: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM frozen_message_sources WHERE message_id=?1 \
             AND source_event_rowid=?2 AND source_event_id=?3 AND source_operation_id=?4 \
             AND chat_id=?5 AND source_aggregate_sequence=?6 AND durable_sequence=?7 \
             AND canonical_message_json=?8 AND message_digest=?9);",
            params![
                message_id,
                sql_i64(row.event_rowid)?,
                row.event_id,
                row.operation_id,
                chat_id,
                sql_i64(row.aggregate_sequence)?,
                durable,
                canonical,
                digest
            ],
            |r| r.get(0),
        )
        .map_err(sqlite_error)?;
    if !exact {
        transaction
            .execute(
                "INSERT INTO frozen_message_sources(\
                   message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,\
                   source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10);",
                params![message_id, sql_i64(row.event_rowid)?, row.event_id, row.operation_id,
                    chat_id, sql_i64(row.aggregate_sequence)?, durable, canonical, digest, now()],
            )
            .map_err(sqlite_error)?;
    }
    Ok(())
}

fn verify_planned_row(
    connection: &Connection,
    row: &PlanRow,
    expect_live: bool,
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
    if live != expect_live || !exact_frozen_operation(connection, row)? {
        return Err(compaction_error());
    }
    if let Some(message_id) = row.introduced_message_id.as_deref() {
        let exact: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM frozen_message_sources f JOIN chat_messages m \
                 ON m.message_id=f.message_id WHERE f.message_id=?1 AND f.source_event_rowid=?2 \
                 AND f.source_event_id=?3 AND f.source_operation_id=?4 \
                 AND f.message_digest=m.message_digest AND f.canonical_message_json=m.canonical_message_json);",
                params![message_id, sql_i64(row.event_rowid)?, row.event_id, row.operation_id],
                |r| r.get(0),
            )
            .map_err(sqlite_error)?;
        if !exact {
            return Err(compaction_error());
        }
    }
    Ok(())
}

fn insert_prefix(
    transaction: &Transaction<'_>,
    plan: &StorageCompactionPlanV1,
) -> Result<(), SidecarError> {
    let anchor = load_planned_row(transaction, plan, 1001, &mut StepBudget::default())?;
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
        let state: String = transaction
            .query_row(
                "SELECT state FROM maintenance_jobs WHERE job_id=?1;",
                [&job.job_id],
                |r| r.get(0),
            )
            .map_err(sqlite_error)?;
        if state != "pending" {
            return Err(compaction_error());
        }
    }
    Ok(())
}

fn verify_equivalence(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
) -> Result<(), SidecarError> {
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
    let projection_table = match latest.1.as_str() {
        "project" => "projects",
        "chat" => "chats",
        "run" => "runs",
        "lock" => "locks",
        _ => return Err(compaction_error()),
    };
    let key = match latest.1.as_str() {
        "project" => "project_id",
        "chat" => "chat_id",
        "run" => "run_id",
        "lock" => "lock_id",
        _ => return Err(compaction_error()),
    };
    let query = format!("SELECT projection_json FROM {projection_table} WHERE {key}=?1;");
    let projection: String = connection
        .query_row(&query, [&plan.aggregate_id], |r| r.get(0))
        .map_err(|_| compaction_error())?;
    if projection != latest.0 {
        return Err(compaction_error());
    }
    Ok(())
}

fn load_active_job(
    connection: &Connection,
) -> Result<Option<StorageCompactionJobV3>, SidecarError> {
    let mut statement = connection
        .prepare(
            "SELECT job_id FROM maintenance_jobs WHERE job_kind='losslessCompaction' \
             AND state IN ('pending','running') ORDER BY job_id LIMIT 2;",
        )
        .map_err(sqlite_error)?;
    let mut rows = statement.query([]).map_err(sqlite_error)?;
    let first = rows
        .next()
        .map_err(sqlite_error)?
        .map(|row| row.get::<_, String>(0))
        .transpose()
        .map_err(sqlite_error)?;
    if rows.next().map_err(sqlite_error)?.is_some() {
        return Err(compaction_error());
    }
    first.map(|id| load_exact_job(connection, &id)).transpose()
}

fn load_exact_job(
    connection: &Connection,
    job_id: &str,
) -> Result<StorageCompactionJobV3, SidecarError> {
    let lengths: (i64, i64, i64) = connection
        .query_row(
            "SELECT length(CAST(job_id AS BLOB)),length(CAST(cutoff_json AS BLOB)),\
                    coalesce(length(CAST(failure_kind AS BLOB)),0)+\
                    coalesce(length(CAST(completed_at AS BLOB)),0) \
             FROM maintenance_jobs WHERE job_id=?1 AND job_kind='losslessCompaction';",
            [job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| compaction_error())?;
    if checked_lengths(&[lengths.0, lengths.1, lengths.2])? > DEPENDENCY_BYTES {
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
    let lengths: (i64, i64) = connection
        .query_row(
            "SELECT length(CAST(job_id AS BLOB))+length(CAST(aggregate_type AS BLOB))+\
                    length(CAST(aggregate_id AS BLOB))+length(CAST(baseline_id AS BLOB))+\
                    length(CAST(baseline_projection_digest AS BLOB))+\
                    coalesce(length(CAST(previous_successful_job_id AS BLOB)),0),\
                    length(CAST(created_at AS BLOB))+length(CAST(planned_at AS BLOB)) \
             FROM storage_compaction_plans WHERE job_id=?1;",
            [job_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| compaction_error())?;
    if checked_lengths(&[lengths.0, lengths.1])? > DEPENDENCY_BYTES {
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
    CompactionStepResult::Progress {
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
}
