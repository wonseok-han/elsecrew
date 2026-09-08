use super::bytes::{ByteUsage, FRAGMENT_BYTES, INPUT_BYTES};
use super::*;
use hello_agent_protocol::storage_lifetime_v1::{
    validate_storage_compaction_job_v2, validate_storage_compaction_job_v3,
    validate_storage_compaction_plan_row_v1, validate_storage_compaction_plan_v1,
    StorageCompactionJobV3, StorageCompactionPhaseV3, StorageCompactionPlanV1,
    STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1,
};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AdmissionPlanMarker {
    Terminal(String, bool),
    TerminalReleased(String, u32),
    PlanRows(u32),
    Released(u32),
    Complete,
}

enum Stage {
    Terminal {
        after_job_id: String,
        seen_v2: bool,
    },
    TerminalReleased {
        plan: Box<StorageCompactionPlanV1>,
        next: u32,
        after_job_id: String,
        seen_v2: bool,
    },
    PlanRows {
        next: u32,
        end: u32,
    },
    Released {
        next: u32,
        end: u32,
    },
    Complete,
}

struct ActivePlan {
    job: StorageCompactionJobV3,
    plan: StorageCompactionPlanV1,
    row_start: u32,
    row_end: u32,
    released_end: u32,
}

pub(crate) struct AdmissionPlanWitness {
    stage: Stage,
    active: Option<ActivePlan>,
    cutoff_event_rowid: u64,
    compacted: bool,
    saw_terminal: bool,
}

pub(crate) enum AdmissionPlanStep {
    Pending,
    Cancelled,
    Complete { cutoff_event_rowid: u64 },
}

impl AdmissionPlanWitness {
    pub(crate) fn start(
        connection: &Connection,
        compacted: bool,
    ) -> Result<Option<Self>, SidecarError> {
        if connection
            .query_row(
                "SELECT 1 FROM ledger_compaction_delete_authority LIMIT 1;",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| session_error())?
            .is_some()
        {
            return Err(session_error());
        }

        let mut statement = connection
            .prepare(
                "SELECT rowid,length(CAST(job_id AS BLOB))+length(CAST(state AS BLOB))+\
                        length(CAST(cutoff_json AS BLOB))+\
                        coalesce(length(CAST(failure_kind AS BLOB)),0)+\
                        coalesce(length(CAST(completed_at AS BLOB)),0) \
                 FROM maintenance_jobs WHERE job_kind='losslessCompaction' \
                   AND state IN ('pending','running') ORDER BY job_id LIMIT 2;",
            )
            .map_err(|_| session_error())?;
        let mut rows = statement.query([]).map_err(|_| session_error())?;
        let active_row = rows
            .next()
            .map_err(|_| session_error())?
            .map(|row| Ok::<_, rusqlite::Error>((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .transpose()
            .map_err(|_| session_error())?;
        if rows.next().map_err(|_| session_error())?.is_some() {
            return Err(session_error());
        }
        drop(rows);
        drop(statement);
        let active_row = active_row
            .map(|(rowid, bytes)| {
                let bytes = usize::try_from(bytes).map_err(|_| session_error())?;
                if bytes > INPUT_BYTES {
                    return Err(session_error());
                }
                connection
                    .query_row(
                        "SELECT job_id,state,cutoff_json,failure_kind,completed_at \
                         FROM maintenance_jobs WHERE rowid=?1;",
                        [rowid],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, Option<String>>(4)?,
                            ))
                        },
                    )
                    .map_err(|_| session_error())
            })
            .transpose()?;

        let active = match active_row {
            None => None,
            Some((job_id, state, cutoff_json, failure_kind, completed_at)) => {
                if state != "pending" {
                    return Err(session_error());
                }
                let job = parse_v3_job(job_id, state, cutoff_json, failure_kind, completed_at)?;
                let plan = load_plan(connection, &job.job_id)?;
                if job.private_state.cursor.cutoff_event_rowid != plan.cutoff_event_rowid {
                    return Err(session_error());
                }
                verify_plan_baseline(connection, &plan, job_retained_bytes(&job)?)?;
                verify_plan_predecessor(connection, &plan)?;
                verify_active_phase_artifacts(connection, &job, &plan)?;
                let (row_start, row_end, released_end) = active_row_bounds(&job)?;
                reject_rows_outside_active_bounds(
                    connection,
                    &job.job_id,
                    row_start,
                    row_end,
                    released_end,
                )?;
                Some(ActivePlan {
                    job,
                    plan,
                    row_start,
                    row_end,
                    released_end,
                })
            }
        };

        let has_relevant: bool = active.is_some()
            || connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM maintenance_jobs \
                     WHERE job_kind='losslessCompaction' AND state='succeeded' LIMIT 1) \
                     OR EXISTS(SELECT 1 FROM storage_compaction_plans LIMIT 1);",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| session_error())?;
        if !has_relevant {
            if compacted {
                return Err(session_error());
            }
            return Ok(None);
        }

        Ok(Some(Self {
            stage: Stage::Terminal {
                after_job_id: String::new(),
                seen_v2: false,
            },
            active,
            cutoff_event_rowid: 0,
            compacted,
            saw_terminal: false,
        }))
    }

    pub(crate) fn marker(&self) -> AdmissionPlanMarker {
        match &self.stage {
            Stage::Terminal {
                after_job_id,
                seen_v2,
            } => AdmissionPlanMarker::Terminal(after_job_id.clone(), *seen_v2),
            Stage::TerminalReleased { plan, next, .. } => {
                AdmissionPlanMarker::TerminalReleased(plan.job_id.clone(), *next)
            }
            Stage::PlanRows { next, .. } => AdmissionPlanMarker::PlanRows(*next),
            Stage::Released { next, .. } => AdmissionPlanMarker::Released(*next),
            Stage::Complete => AdmissionPlanMarker::Complete,
        }
    }

    pub(crate) fn retained_bytes(&self) -> Result<usize, SidecarError> {
        let active = self.active.as_ref().map_or(0, |active| {
            active.job.job_id.len()
                + active.plan.job_id.len()
                + active.plan.aggregate_type.len()
                + active.plan.aggregate_id.len()
                + active.plan.baseline_id.len()
                + active.plan.baseline_projection_digest.len()
                + active.plan.created_at.len()
                + active.plan.planned_at.len()
                + active
                    .plan
                    .previous_successful_job_id
                    .as_ref()
                    .map_or(0, String::len)
        });
        let stage = match &self.stage {
            Stage::Terminal { after_job_id, .. } => after_job_id.len(),
            Stage::TerminalReleased {
                plan, after_job_id, ..
            } => {
                plan.job_id.len()
                    + plan.aggregate_type.len()
                    + plan.aggregate_id.len()
                    + plan.baseline_id.len()
                    + plan.baseline_projection_digest.len()
                    + plan.created_at.len()
                    + plan.planned_at.len()
                    + plan
                        .previous_successful_job_id
                        .as_ref()
                        .map_or(0, String::len)
                    + after_job_id.len()
            }
            _ => 0,
        };
        active.checked_add(stage).ok_or_else(session_error)
    }

    pub(crate) fn step(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<AdmissionPlanStep, SidecarError> {
        if cancelled() {
            return Ok(AdmissionPlanStep::Cancelled);
        }
        match &mut self.stage {
            Stage::Terminal {
                after_job_id,
                seen_v2,
            } => {
                let next = load_next_succeeded_job(connection, after_job_id, usage)?;
                let Some((job_id, cutoff_json, completed_at)) = next else {
                    reject_unowned_plan_headers(connection, self.active.as_ref(), usage)?;
                    if !self.saw_terminal && self.active.is_none() && self.compacted {
                        return Err(session_error());
                    }
                    self.stage = match self.active.as_ref() {
                        Some(active) if active.row_start <= active.row_end => Stage::PlanRows {
                            next: active.row_start,
                            end: active.row_end,
                        },
                        Some(active) if active.released_end > 0 => Stage::Released {
                            next: 1,
                            end: active.released_end,
                        },
                        Some(_) => Stage::Complete,
                        None => Stage::Complete,
                    };
                    return Ok(AdmissionPlanStep::Pending);
                };
                if cutoff_json.len() > INPUT_BYTES {
                    return Err(session_error());
                }
                let envelope: Value =
                    serde_json::from_str(&cutoff_json).map_err(|_| session_error())?;
                let object = envelope.as_object().ok_or_else(session_error)?;
                if object.len() != 1 {
                    return Err(session_error());
                }
                if object.contains_key("storageCompactionV2PrivateState") {
                    if *seen_v2 || plan_exists(connection, &job_id, usage)? {
                        return Err(session_error());
                    }
                    let cutoff = validate_terminal_v2(&job_id, &envelope, &completed_at)?;
                    self.cutoff_event_rowid = self.cutoff_event_rowid.max(cutoff);
                    *seen_v2 = true;
                } else if object.contains_key("storageCompactionV3PrivateState") {
                    drop(cutoff_json);
                    let job = validate_v3_job(
                        job_id,
                        "succeeded".to_string(),
                        envelope,
                        None,
                        Some(completed_at),
                    )?;
                    let plan = load_plan(connection, &job.job_id)?;
                    if job.private_state.cursor.cutoff_event_rowid != plan.cutoff_event_rowid {
                        return Err(session_error());
                    }
                    verify_terminal_v3(connection, &job, &plan, usage)?;
                    self.cutoff_event_rowid = self.cutoff_event_rowid.max(plan.cutoff_event_rowid);
                    self.saw_terminal = true;
                    self.stage = Stage::TerminalReleased {
                        plan: Box::new(plan),
                        next: 1,
                        after_job_id: job.job_id,
                        seen_v2: *seen_v2,
                    };
                    return Ok(AdmissionPlanStep::Pending);
                } else {
                    return Err(session_error());
                }
                self.saw_terminal = true;
                *after_job_id = job_id;
                Ok(AdmissionPlanStep::Pending)
            }
            Stage::TerminalReleased {
                plan,
                next,
                after_job_id,
                seen_v2,
            } => {
                verify_released_ordinal(connection, plan, *next, usage)?;
                *next += 1;
                if *next > 1000 {
                    self.stage = Stage::Terminal {
                        after_job_id: std::mem::take(after_job_id),
                        seen_v2: *seen_v2,
                    };
                }
                Ok(AdmissionPlanStep::Pending)
            }
            Stage::PlanRows { next, end } => {
                let active = self.active.as_ref().ok_or_else(session_error)?;
                verify_plan_row(connection, &active.job, &active.plan, *next, false, usage)?;
                *next += 1;
                if *next > *end {
                    self.cutoff_event_rowid =
                        self.cutoff_event_rowid.max(active.plan.cutoff_event_rowid);
                    self.stage = if active.released_end > 0 {
                        Stage::Released {
                            next: 1,
                            end: active.released_end,
                        }
                    } else {
                        Stage::Complete
                    };
                }
                Ok(AdmissionPlanStep::Pending)
            }
            Stage::Released { next, end } => {
                let active = self.active.as_ref().ok_or_else(session_error)?;
                verify_released_ordinal(connection, &active.plan, *next, usage)?;
                *next += 1;
                if *next > *end {
                    self.cutoff_event_rowid =
                        self.cutoff_event_rowid.max(active.plan.cutoff_event_rowid);
                    self.stage = Stage::Complete;
                }
                Ok(AdmissionPlanStep::Pending)
            }
            Stage::Complete => Ok(AdmissionPlanStep::Complete {
                cutoff_event_rowid: self.cutoff_event_rowid,
            }),
        }
    }
}

fn parse_v3_job(
    job_id: String,
    state: String,
    cutoff_json: String,
    failure_kind: Option<String>,
    completed_at: Option<String>,
) -> Result<StorageCompactionJobV3, SidecarError> {
    let envelope: Value = serde_json::from_str(&cutoff_json).map_err(|_| session_error())?;
    validate_v3_job(job_id, state, envelope, failure_kind, completed_at)
}

fn validate_v3_job(
    job_id: String,
    state: String,
    envelope: Value,
    failure_kind: Option<String>,
    completed_at: Option<String>,
) -> Result<StorageCompactionJobV3, SidecarError> {
    let object = envelope.as_object().ok_or_else(session_error)?;
    if object.len() != 1 {
        return Err(session_error());
    }
    let private = object
        .get("storageCompactionV3PrivateState")
        .cloned()
        .ok_or_else(session_error)?;
    validate_storage_compaction_job_v3(&json!({
        "jobId": job_id,
        "state": state,
        "privateState": private,
        "authorityCount": 0,
        "failureKind": failure_kind,
        "completedAt": completed_at,
    }))
    .map_err(|_| session_error())
}

fn validate_terminal_v2(
    job_id: &str,
    envelope: &Value,
    completed_at: &str,
) -> Result<u64, SidecarError> {
    let private = envelope
        .get("storageCompactionV2PrivateState")
        .and_then(Value::as_object)
        .ok_or_else(session_error)?;
    if private.len() != 4 || private.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
        return Err(session_error());
    }
    let job = validate_storage_compaction_job_v2(&json!({
        "jobId": job_id,
        "state": "succeeded",
        "cursor": private.get("cursor").cloned().unwrap_or(Value::Null),
        "authorityCount": 0,
        "operationsVerified": private.get("operationsVerified").cloned().unwrap_or(Value::Null),
        "equivalenceVerified": private.get("equivalenceVerified").cloned().unwrap_or(Value::Null),
        "failureKind": null,
        "completedAt": completed_at,
    }))
    .map_err(|_| session_error())?;
    if job.cursor.after_event_rowid != job.cursor.cutoff_event_rowid {
        return Err(session_error());
    }
    Ok(job.cursor.cutoff_event_rowid)
}

fn load_plan(
    connection: &Connection,
    job_id: &str,
) -> Result<StorageCompactionPlanV1, SidecarError> {
    let bytes: Option<i64> = connection
        .query_row(
            "SELECT length(CAST(job_id AS BLOB))+length(CAST(aggregate_type AS BLOB))+\
                    length(CAST(aggregate_id AS BLOB))+length(CAST(baseline_id AS BLOB))+\
                    length(CAST(baseline_projection_digest AS BLOB))+\
                    coalesce(length(CAST(previous_successful_job_id AS BLOB)),0)+\
                    length(CAST(created_at AS BLOB))+length(CAST(planned_at AS BLOB))\
             FROM storage_compaction_plans WHERE job_id=?1;",
            [job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| session_error())?;
    let bytes = usize::try_from(bytes.ok_or_else(session_error)?).map_err(|_| session_error())?;
    if bytes > INPUT_BYTES {
        return Err(session_error());
    }
    let value = connection
        .query_row(
            "SELECT job_id,plan_schema_version,cutoff_event_rowid,aggregate_type,aggregate_id,\
                    retained_boundary_sequence,delete_through_sequence,anchor_aggregate_sequence,\
                    expected_rows,baseline_id,baseline_projection_digest,previous_successful_job_id,\
                    created_at,planned_at FROM storage_compaction_plans WHERE job_id=?1;",
            [job_id],
            |row| {
                Ok(json!({
                    "jobId": row.get::<_, String>(0)?,
                    "planSchemaVersion": row.get::<_, u32>(1)?,
                    "cutoffEventRowid": row_u64(row, 2)?,
                    "aggregateType": row.get::<_, String>(3)?,
                    "aggregateId": row.get::<_, String>(4)?,
                    "retainedBoundarySequence": row_u64(row, 5)?,
                    "deleteThroughSequence": row_u64(row, 6)?,
                    "anchorAggregateSequence": row_u64(row, 7)?,
                    "expectedRows": row.get::<_, u32>(8)?,
                    "baselineId": row.get::<_, String>(9)?,
                    "baselineProjectionDigest": row.get::<_, String>(10)?,
                    "previousSuccessfulJobId": row.get::<_, Option<String>>(11)?,
                    "createdAt": row.get::<_, String>(12)?,
                    "plannedAt": row.get::<_, String>(13)?,
                }))
            },
        )
        .map_err(|_| session_error())?;
    validate_storage_compaction_plan_v1(&value).map_err(|_| session_error())
}

fn verify_plan_baseline(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
    job_retained_bytes: usize,
) -> Result<(), SidecarError> {
    let lengths: (i64, i64) = connection
        .query_row(
            "SELECT length(CAST(canonical_projection_json AS BLOB)),\
                    length(CAST(projection_digest AS BLOB)) \
             FROM ledger_baselines WHERE baseline_id=?1 \
                    AND aggregate_type=?2 AND aggregate_id=?3 AND cutoff_aggregate_sequence=?4;",
            params![
                plan.baseline_id,
                plan.aggregate_type,
                plan.aggregate_id,
                i64::try_from(plan.delete_through_sequence).map_err(|_| session_error())?
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| session_error())?;
    let canonical_bytes = usize::try_from(lengths.0).map_err(|_| session_error())?;
    let digest_bytes = usize::try_from(lengths.1).map_err(|_| session_error())?;
    let dependency_bytes = canonical_bytes
        .checked_add(digest_bytes)
        .ok_or_else(session_error)?;
    if canonical_bytes > FRAGMENT_BYTES
        || digest_bytes != "sha256:".len() + 64
        || job_retained_bytes
            .checked_add(plan_retained_bytes(plan)?)
            .and_then(|value| value.checked_add(dependency_bytes))
            .is_none_or(|value| value > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let (canonical, stored): (String, String) = connection
        .query_row(
            "SELECT canonical_projection_json,projection_digest \
             FROM ledger_baselines WHERE baseline_id=?1 \
                    AND aggregate_type=?2 AND aggregate_id=?3 AND cutoff_aggregate_sequence=?4;",
            params![
                plan.baseline_id,
                plan.aggregate_type,
                plan.aggregate_id,
                i64::try_from(plan.delete_through_sequence).map_err(|_| session_error())?
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| session_error())?;
    if canonical.len() != canonical_bytes
        || stored.len() != digest_bytes
        || canonical_json_v1(serde_json::from_str(&canonical).map_err(|_| session_error())?)
            .map_err(|_| session_error())?
            != canonical
    {
        return Err(session_error());
    }
    let digest = sha256_file_version_v1(canonical.as_bytes());
    if stored != digest || digest.strip_prefix("sha256:") != Some(&plan.baseline_projection_digest)
    {
        return Err(session_error());
    }
    Ok(())
}

fn plan_retained_bytes(plan: &StorageCompactionPlanV1) -> Result<usize, SidecarError> {
    [
        plan.job_id.as_str(),
        plan.aggregate_type.as_str(),
        plan.aggregate_id.as_str(),
        plan.baseline_id.as_str(),
        plan.baseline_projection_digest.as_str(),
        plan.previous_successful_job_id.as_deref().unwrap_or(""),
        plan.created_at.as_str(),
        plan.planned_at.as_str(),
    ]
    .into_iter()
    .try_fold(0usize, |total, value| {
        total.checked_add(value.len()).ok_or_else(session_error)
    })
}

fn job_retained_bytes(job: &StorageCompactionJobV3) -> Result<usize, SidecarError> {
    [
        job.job_id.as_str(),
        job.failure_kind.as_deref().unwrap_or(""),
        job.completed_at.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .try_fold(0usize, |total, value| {
        total.checked_add(value.len()).ok_or_else(session_error)
    })
}

fn verify_plan_predecessor(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
) -> Result<(), SidecarError> {
    match plan.previous_successful_job_id.as_deref() {
        Some(previous) => {
            let exact: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM storage_compaction_plans p \
                     JOIN maintenance_jobs j ON j.job_id=p.job_id \
                     WHERE p.job_id=?1 AND j.state='succeeded' AND p.aggregate_type=?2 \
                       AND p.aggregate_id=?3 AND p.delete_through_sequence=?4);",
                    params![
                        previous,
                        plan.aggregate_type,
                        plan.aggregate_id,
                        i64::try_from(plan.retained_boundary_sequence)
                            .map_err(|_| session_error())?
                    ],
                    |row| row.get(0),
                )
                .map_err(|_| session_error())?;
            let mut successors = connection
                .prepare(
                    "SELECT job_id=?2 FROM storage_compaction_plans \
                     WHERE previous_successful_job_id=?1 ORDER BY job_id LIMIT 2;",
                )
                .map_err(|_| session_error())?;
            let mut rows = successors
                .query(params![previous, plan.job_id])
                .map_err(|_| session_error())?;
            let first = rows
                .next()
                .map_err(|_| session_error())?
                .map(|row| row.get::<_, bool>(0))
                .transpose()
                .map_err(|_| session_error())?;
            let second = rows.next().map_err(|_| session_error())?.is_some();
            if !exact || first != Some(true) || second {
                return Err(session_error());
            }
        }
        None => {
            let prior_v9: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM storage_compaction_plans WHERE aggregate_type=?1 \
                     AND aggregate_id=?2 AND delete_through_sequence<?3 LIMIT 1);",
                    params![
                        plan.aggregate_type,
                        plan.aggregate_id,
                        i64::try_from(plan.delete_through_sequence).map_err(|_| session_error())?
                    ],
                    |row| row.get(0),
                )
                .map_err(|_| session_error())?;
            if prior_v9 {
                return Err(session_error());
            }
            if plan.retained_boundary_sequence > 0 {
                let prior_prefix: bool = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM ledger_compaction_prefixes \
                         WHERE aggregate_type=?1 AND aggregate_id=?2 \
                           AND deleted_through_sequence=?3 LIMIT 1);",
                        params![
                            plan.aggregate_type,
                            plan.aggregate_id,
                            i64::try_from(plan.retained_boundary_sequence)
                                .map_err(|_| session_error())?
                        ],
                        |row| row.get(0),
                    )
                    .map_err(|_| session_error())?;
                if !prior_prefix {
                    return Err(session_error());
                }
            }
        }
    }
    Ok(())
}

fn active_row_bounds(job: &StorageCompactionJobV3) -> Result<(u32, u32, u32), SidecarError> {
    let cursor = &job.private_state.cursor;
    Ok(match cursor.phase {
        StorageCompactionPhaseV3::PlanEligibility => {
            if cursor.after_plan_ordinal == 0 {
                (1, 0, 0)
            } else {
                (1, cursor.after_plan_ordinal, 0)
            }
        }
        StorageCompactionPhaseV3::ReleasePlan | StorageCompactionPhaseV3::Finalize => (
            cursor.after_plan_ordinal + 1,
            STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1,
            cursor.after_plan_ordinal,
        ),
        _ => (1, STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1, 0),
    })
}

fn reject_rows_outside_active_bounds(
    connection: &Connection,
    job_id: &str,
    start: u32,
    end: u32,
    released_end: u32,
) -> Result<(), SidecarError> {
    let invalid: bool = if end == 0 {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_compaction_plan_rows WHERE job_id=?1 LIMIT 1);",
            [job_id],
            |row| row.get(0),
        )
    } else {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_compaction_plan_rows WHERE job_id=?1 \
             AND (ordinal<?2 OR ordinal>?3) LIMIT 1);",
            params![job_id, start, end],
            |row| row.get(0),
        )
    }
    .map_err(|_| session_error())?;
    if invalid {
        return Err(session_error());
    }
    if released_end > 0 {
        let released_still_present: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM storage_compaction_plan_rows WHERE job_id=?1 \
                 AND ordinal<=?2 LIMIT 1);",
                params![job_id, released_end],
                |row| row.get(0),
            )
            .map_err(|_| session_error())?;
        if released_still_present {
            return Err(session_error());
        }
    }
    Ok(())
}

fn verify_active_phase_artifacts(
    connection: &Connection,
    job: &StorageCompactionJobV3,
    plan: &StorageCompactionPlanV1,
) -> Result<(), SidecarError> {
    let cursor = &job.private_state.cursor;
    let prefix_required = match cursor.phase {
        StorageCompactionPhaseV3::PlanEligibility
        | StorageCompactionPhaseV3::FreezeOperations
        | StorageCompactionPhaseV3::FreezeMessages => false,
        StorageCompactionPhaseV3::VerifyPrefix => cursor.scan_exhausted,
        _ => true,
    };
    let prefix_exists = exact_prefix_exists(connection, plan)?;
    if prefix_exists != prefix_required {
        return Err(session_error());
    }
    Ok(())
}

fn exact_prefix_exists(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
) -> Result<bool, SidecarError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ledger_compaction_prefixes p \
             JOIN storage_compaction_plan_rows anchor ON anchor.job_id=?1 AND anchor.ordinal=1001 \
             WHERE p.aggregate_type=?2 AND p.aggregate_id=?3 AND p.deleted_through_sequence=?4 \
               AND p.anchor_aggregate_sequence=?5 AND p.anchor_event_rowid=anchor.event_rowid \
               AND p.anchor_event_id=anchor.event_id AND p.anchor_operation_id=anchor.operation_id \
               AND p.baseline_id=?6 AND p.baseline_projection_digest='sha256:'||?7);",
            params![
                plan.job_id,
                plan.aggregate_type,
                plan.aggregate_id,
                i64::try_from(plan.delete_through_sequence).map_err(|_| session_error())?,
                i64::try_from(plan.anchor_aggregate_sequence).map_err(|_| session_error())?,
                plan.baseline_id,
                plan.baseline_projection_digest,
            ],
            |row| row.get(0),
        )
        .map_err(|_| session_error())
}

fn load_next_succeeded_job(
    connection: &Connection,
    after: &str,
    usage: &mut ByteUsage,
) -> Result<Option<(String, String, String)>, SidecarError> {
    let lengths: Option<(i64, i64, i64)> = connection
        .query_row(
            "SELECT length(CAST(job_id AS BLOB)),length(CAST(cutoff_json AS BLOB)),\
                    length(CAST(completed_at AS BLOB)) FROM maintenance_jobs \
             WHERE job_kind='losslessCompaction' AND state='succeeded' AND job_id>?1 \
             ORDER BY job_id LIMIT 1;",
            [after],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let Some((a, b, c)) = lengths else {
        return Ok(None);
    };
    let bytes = [a, b, c].into_iter().try_fold(0_usize, |sum, value| {
        sum.checked_add(usize::try_from(value).map_err(|_| session_error())?)
            .ok_or_else(session_error)
    })?;
    if usage
        .encoded_bytes()
        .checked_add(bytes)
        .is_none_or(|total| total > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let row = connection
        .query_row(
            "SELECT job_id,cutoff_json,completed_at FROM maintenance_jobs \
             WHERE job_kind='losslessCompaction' AND state='succeeded' AND job_id>?1 \
             ORDER BY job_id LIMIT 1;",
            [after],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    usage.metadata_bytes = usage
        .metadata_bytes
        .checked_add(bytes)
        .ok_or_else(session_error)?;
    Ok(row)
}

fn plan_exists(
    connection: &Connection,
    job_id: &str,
    usage: &mut ByteUsage,
) -> Result<bool, SidecarError> {
    usage.work_units += 1;
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_compaction_plans WHERE job_id=?1);",
            [job_id],
            |row| row.get(0),
        )
        .map_err(|_| session_error())
}

fn verify_terminal_v3(
    connection: &Connection,
    job: &StorageCompactionJobV3,
    plan: &StorageCompactionPlanV1,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    verify_plan_baseline(connection, plan, job_retained_bytes(job)?)?;
    verify_plan_predecessor(connection, plan)?;
    let wrong_row: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_compaction_plan_rows WHERE job_id=?1 \
             AND ordinal<>1001 LIMIT 1);",
            [&plan.job_id],
            |row| row.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    if wrong_row || !exact_prefix_exists(connection, plan)? {
        return Err(session_error());
    }
    let consumed_by_successor: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_compaction_plans \
             WHERE previous_successful_job_id=?1 LIMIT 1);",
            [&plan.job_id],
            |row| row.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    verify_plan_row(connection, job, plan, 1001, consumed_by_successor, usage)
}

fn reject_unowned_plan_headers(
    connection: &Connection,
    active: Option<&ActivePlan>,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let active_id = active.map(|value| value.plan.job_id.as_str());
    let orphan: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storage_compaction_plans p \
             JOIN maintenance_jobs j ON j.job_id=p.job_id \
             WHERE j.state<>'succeeded' AND (?1 IS NULL OR p.job_id<>?1) LIMIT 1);",
            [active_id],
            |row| row.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    if orphan {
        return Err(session_error());
    }
    Ok(())
}

fn verify_plan_row(
    connection: &Connection,
    job: &StorageCompactionJobV3,
    plan: &StorageCompactionPlanV1,
    ordinal: u32,
    terminal_anchor_consumed: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let lengths: Option<(i64, i64)> = connection
        .query_row(
            "SELECT length(CAST(canonical_payload_json AS BLOB)),\
                    length(CAST(job_id AS BLOB))+length(CAST(event_id AS BLOB))+\
                    length(CAST(operation_id AS BLOB))+coalesce(length(CAST(request_id AS BLOB)),0)+\
                    length(CAST(aggregate_type AS BLOB))+length(CAST(aggregate_id AS BLOB))+\
                    length(CAST(event_type AS BLOB))+length(CAST(event_timestamp AS BLOB))+\
                    length(CAST(canonical_payload_digest AS BLOB))+\
                    coalesce(length(CAST(introduced_message_id AS BLOB)),0)+\
                    coalesce(length(CAST(introduced_message_digest AS BLOB)),0)+\
                    coalesce(length(CAST(baseline_id AS BLOB)),0)+\
                    coalesce(length(CAST(baseline_projection_digest AS BLOB)),0)\
             FROM storage_compaction_plan_rows WHERE job_id=?1 AND ordinal=?2;",
            params![plan.job_id, ordinal],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let (payload_bytes, metadata_bytes) = lengths.ok_or_else(session_error)?;
    let payload_bytes = usize::try_from(payload_bytes).map_err(|_| session_error())?;
    let metadata_bytes = usize::try_from(metadata_bytes).map_err(|_| session_error())?;
    if payload_bytes > FRAGMENT_BYTES
        || usage
            .encoded_bytes()
            .checked_add(payload_bytes)
            .and_then(|value| value.checked_add(metadata_bytes))
            .is_none_or(|value| value > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let value = connection
        .query_row(
            "SELECT job_id,ordinal,event_rowid,event_id,operation_id,request_id,aggregate_type,\
                    aggregate_id,aggregate_sequence,event_type,payload_schema_version,event_timestamp,\
                    canonical_payload_json,canonical_payload_digest,introduced_message_id,\
                    introduced_message_digest,decision,baseline_id,baseline_projection_digest,\
                    operation_frozen_before_plan,message_frozen_before_plan \
             FROM storage_compaction_plan_rows WHERE job_id=?1 AND ordinal=?2;",
            params![plan.job_id, ordinal],
            |row| {
                Ok(json!({
                    "jobId": row.get::<_, String>(0)?, "ordinal": row.get::<_, u32>(1)?,
                    "eventRowid": row_u64(row, 2)?, "eventId": row.get::<_, String>(3)?,
                    "operationId": row.get::<_, String>(4)?, "requestId": row.get::<_, Option<String>>(5)?,
                    "aggregateType": row.get::<_, String>(6)?, "aggregateId": row.get::<_, String>(7)?,
                    "aggregateSequence": row_u64(row, 8)?, "eventType": row.get::<_, String>(9)?,
                    "payloadSchemaVersion": row.get::<_, u32>(10)?, "eventTimestamp": row.get::<_, String>(11)?,
                    "canonicalPayloadJson": row.get::<_, String>(12)?, "canonicalPayloadDigest": row.get::<_, String>(13)?,
                    "introducedMessageId": row.get::<_, Option<String>>(14)?,
                    "introducedMessageDigest": row.get::<_, Option<String>>(15)?,
                    "decision": row.get::<_, String>(16)?, "baselineId": row.get::<_, Option<String>>(17)?,
                    "baselineProjectionDigest": row.get::<_, Option<String>>(18)?,
                    "operationFrozenBeforePlan": row.get::<_, bool>(19)?,
                    "messageFrozenBeforePlan": row.get::<_, bool>(20)?,
                }))
            },
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    usage.fragment_bytes = usage
        .fragment_bytes
        .checked_add(payload_bytes)
        .ok_or_else(session_error)?;
    usage.metadata_bytes = usage
        .metadata_bytes
        .checked_add(metadata_bytes)
        .ok_or_else(session_error)?;
    let row = validate_storage_compaction_plan_row_v1(plan, &value).map_err(|_| session_error())?;
    let parsed: Value =
        serde_json::from_str(&row.canonical_payload_json).map_err(|_| session_error())?;
    let chat_state = (row.aggregate_type == "chat")
        .then(|| planned_chat_state(&row.aggregate_id, &parsed))
        .transpose()?;
    if canonical_json_v1(parsed).map_err(|_| session_error())? != row.canonical_payload_json {
        return Err(session_error());
    }
    let computed = sha256_file_version_v1(row.canonical_payload_json.as_bytes());
    if computed.strip_prefix("sha256:") != Some(&row.canonical_payload_digest) {
        return Err(session_error());
    }

    let phase = job.private_state.cursor.phase;
    let after = job.private_state.cursor.after_plan_ordinal;
    let expect_live = (ordinal == STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
        && !terminal_anchor_consumed)
        || !matches!(
            phase,
            StorageCompactionPhaseV3::DeletePrefix
                | StorageCompactionPhaseV3::VerifyOperations
                | StorageCompactionPhaseV3::VerifyEquivalence
                | StorageCompactionPhaseV3::ReleasePlan
                | StorageCompactionPhaseV3::Finalize
        )
        || (phase == StorageCompactionPhaseV3::DeletePrefix && ordinal > after);
    let expect_frozen = match phase {
        StorageCompactionPhaseV3::PlanEligibility => row.operation_frozen_before_plan,
        StorageCompactionPhaseV3::FreezeOperations => {
            ordinal <= after || row.operation_frozen_before_plan
        }
        _ => true,
    };
    verify_source_receipt_and_index(connection, &row, expect_live, expect_frozen, usage)?;
    verify_planned_chat_transition(
        connection,
        plan,
        &row,
        chat_state.as_ref(),
        expect_live,
        usage,
    )?;
    verify_message_witness(connection, job, &row, chat_state.as_ref(), usage)?;
    Ok(())
}

struct PlannedChatState {
    durable: u64,
    snapshot: Option<String>,
    message: Option<Value>,
}

fn planned_chat_state(
    aggregate_id: &str,
    payload: &Value,
) -> Result<PlannedChatState, SidecarError> {
    let object = payload.as_object().ok_or_else(session_error)?;
    let durable = object
        .get("durable_event_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(session_error)?;
    let (durable, snapshot, message) =
        hello_agent_protocol::storage_lifetime_v1::storage_chat_snapshot_fields_v1(
            aggregate_id,
            durable,
            object.get("last_message"),
        )
        .map_err(|_| session_error())?;
    Ok(PlannedChatState {
        durable,
        snapshot,
        message,
    })
}

fn persisted_chat_state(
    connection: &Connection,
    frozen: bool,
    rowid: i64,
    aggregate_id: &str,
    aggregate_sequence: u64,
    usage: &mut ByteUsage,
) -> Result<PlannedChatState, SidecarError> {
    let (table, key, payload) = if frozen {
        (
            "frozen_operation_sources",
            "source_event_rowid",
            "canonical_payload_json",
        )
    } else {
        ("events", "rowid", "canonical_json_payload")
    };
    let (kind, bytes, durable_kind, durable): (String, i64, String, i64) = connection
        .query_row(
            &format!(
                "SELECT json_type({payload},'$.last_message'),\
                        coalesce(length(CAST(json_extract({payload},'$.last_message') AS BLOB)),0),\
                        json_type({payload},'$.durable_event_sequence'),\
                        json_extract({payload},'$.durable_event_sequence') \
                 FROM {table} WHERE {key}=?1 AND aggregate_type='chat' \
                   AND aggregate_id=?2 AND aggregate_sequence=?3"
            ),
            params![
                rowid,
                aggregate_id,
                i64::try_from(aggregate_sequence).map_err(|_| session_error())?
            ],
            |record| {
                Ok((
                    record.get(0)?,
                    record.get(1)?,
                    record.get(2)?,
                    record.get(3)?,
                ))
            },
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let bytes = usize::try_from(bytes).map_err(|_| session_error())?;
    if !matches!(kind.as_str(), "null" | "object")
        || durable_kind != "integer"
        || bytes > FRAGMENT_BYTES
        || usage
            .encoded_bytes()
            .checked_add(bytes)
            .is_none_or(|total| total > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let message = if kind == "null" {
        Value::Null
    } else {
        let raw: String = connection
            .query_row(
                &format!(
                    "SELECT json_extract({payload},'$.last_message') FROM {table} \
                     WHERE {key}=?1"
                ),
                [rowid],
                |record| record.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        usage.metadata_bytes = usage
            .metadata_bytes
            .checked_add(bytes)
            .ok_or_else(session_error)?;
        if raw.len() != bytes {
            return Err(session_error());
        }
        serde_json::from_str(&raw).map_err(|_| session_error())?
    };
    let (durable, snapshot, message) =
        hello_agent_protocol::storage_lifetime_v1::storage_chat_snapshot_fields_v1(
            aggregate_id,
            u64::try_from(durable).map_err(|_| session_error())?,
            Some(&message),
        )
        .map_err(|_| session_error())?;
    Ok(PlannedChatState {
        durable,
        snapshot,
        message,
    })
}

fn verify_planned_chat_transition(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
    row: &hello_agent_protocol::storage_lifetime_v1::StorageCompactionPlanRowV1,
    current: Option<&PlannedChatState>,
    expect_live: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    if row.aggregate_type != "chat" {
        return Ok(());
    }
    // Snapshot/catch-up may exclude this row only after the complete plan witness succeeds.
    // Rebind the transition here to the immediately preceding verified live/frozen source so
    // that exclusion cannot bypass the generic chat transition invariant.
    let current = current.ok_or_else(session_error)?;
    let (previous_durable, previous_snapshot) = if row.aggregate_sequence == 1 {
        if row.ordinal != 1 || plan.retained_boundary_sequence != 0 {
            return Err(session_error());
        }
        (0, None)
    } else {
        let previous_sequence = row.aggregate_sequence - 1;
        let expected_rowid: i64 = if row.ordinal > 1 {
            connection
                .query_row(
                    "SELECT event_rowid FROM storage_compaction_plan_rows \
                     WHERE job_id=?1 AND ordinal=?2 AND aggregate_type='chat' \
                       AND aggregate_id=?3 AND aggregate_sequence=?4",
                    params![
                        plan.job_id,
                        row.ordinal - 1,
                        row.aggregate_id,
                        i64::try_from(previous_sequence).map_err(|_| session_error())?
                    ],
                    |record| record.get(0),
                )
                .map_err(|_| session_error())?
        } else {
            connection
                .query_row(
                    "SELECT source_event_rowid FROM frozen_operation_sources \
                     WHERE aggregate_type='chat' AND aggregate_id=?1 AND aggregate_sequence=?2",
                    params![
                        row.aggregate_id,
                        i64::try_from(previous_sequence).map_err(|_| session_error())?
                    ],
                    |record| record.get(0),
                )
                .map_err(|_| session_error())?
        };
        usage.work_units += 1;
        if expected_rowid <= 0
            || u64::try_from(expected_rowid).map_err(|_| session_error())? >= row.event_rowid
        {
            return Err(session_error());
        }
        let live: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE rowid=?1 AND aggregate_type='chat' \
                 AND aggregate_id=?2 AND aggregate_sequence=?3)",
                params![
                    expected_rowid,
                    row.aggregate_id,
                    i64::try_from(previous_sequence).map_err(|_| session_error())?
                ],
                |record| record.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        let previous = persisted_chat_state(
            connection,
            !live,
            expected_rowid,
            &row.aggregate_id,
            previous_sequence,
            usage,
        )?;
        (previous.durable, previous.snapshot)
    };
    hello_agent_protocol::storage_lifetime_v1::validate_storage_chat_transition_fields_v1(
        &row.aggregate_id,
        current.durable,
        current.snapshot.as_deref(),
        current.message.as_ref(),
        previous_durable,
        previous_snapshot.as_deref(),
        row.introduced_message_id.as_deref(),
    )
    .map_err(|_| session_error())?;
    if expect_live {
        let mirror: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM event_source_index WHERE event_rowid=?1 \
                 AND typeof(chat_durable_sequence)='integer' AND chat_durable_sequence=?2 \
                 AND ((?3 IS NULL AND typeof(chat_snapshot_message_id)='null') \
                   OR (typeof(chat_snapshot_message_id)='text' AND chat_snapshot_message_id=?3)))",
                params![
                    i64::try_from(row.event_rowid).map_err(|_| session_error())?,
                    i64::try_from(current.durable).map_err(|_| session_error())?,
                    current.snapshot
                ],
                |record| record.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        if !mirror {
            return Err(session_error());
        }
    }
    Ok(())
}

fn verify_source_receipt_and_index(
    connection: &Connection,
    row: &hello_agent_protocol::storage_lifetime_v1::StorageCompactionPlanRowV1,
    expect_live: bool,
    expect_frozen: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let event_rowid = i64::try_from(row.event_rowid).map_err(|_| session_error())?;
    let aggregate_sequence = i64::try_from(row.aggregate_sequence).map_err(|_| session_error())?;
    let live_exact: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM events e WHERE e.rowid=?1 AND e.event_id=?2 \
             AND e.operation_id=?3 AND e.request_id IS ?4 AND e.aggregate_type=?5 \
             AND e.aggregate_id=?6 AND e.aggregate_sequence=?7 AND e.event_type=?8 \
             AND e.payload_schema_version=?9 AND e.timestamp=?10 \
             AND CAST(e.canonical_json_payload AS BLOB)=CAST(?11 AS BLOB));",
            params![
                event_rowid,
                row.event_id,
                row.operation_id,
                row.request_id,
                row.aggregate_type,
                row.aggregate_id,
                aggregate_sequence,
                row.event_type,
                row.payload_schema_version,
                row.event_timestamp,
                row.canonical_payload_json
            ],
            |record| record.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let frozen_exact: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM frozen_operation_sources f WHERE f.source_event_rowid=?1 \
             AND f.source_event_id=?2 AND f.operation_id=?3 AND f.request_id IS ?4 \
             AND f.aggregate_type=?5 AND f.aggregate_id=?6 AND f.aggregate_sequence=?7 \
             AND f.event_type=?8 AND f.payload_schema_version=?9 AND f.event_timestamp=?10 \
             AND CAST(f.canonical_payload_json AS BLOB)=CAST(?11 AS BLOB) \
             AND f.canonical_payload_digest=?12);",
            params![
                event_rowid,
                row.event_id,
                row.operation_id,
                row.request_id,
                row.aggregate_type,
                row.aggregate_id,
                aggregate_sequence,
                row.event_type,
                row.payload_schema_version,
                row.event_timestamp,
                row.canonical_payload_json,
                row.canonical_payload_digest
            ],
            |record| record.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let receipt_exact: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM operation_receipts r WHERE r.operation_id=?1 \
             AND r.event_id=?2 AND r.aggregate_type=?3 AND r.aggregate_id=?4 \
             AND r.aggregate_sequence=?5 AND r.event_type=?6 AND r.payload_schema_version=?7 \
             AND CAST(r.canonical_payload_json AS BLOB)=CAST(?8 AS BLOB) \
             AND r.canonical_payload_digest_algorithm='sha256' \
             AND r.canonical_payload_digest_version=1 \
             AND r.canonical_payload_digest_format='lowercaseHex64' \
             AND r.canonical_payload_digest=?9 AND r.event_timestamp=?10);",
            params![
                row.operation_id,
                row.event_id,
                row.aggregate_type,
                row.aggregate_id,
                aggregate_sequence,
                row.event_type,
                row.payload_schema_version,
                row.canonical_payload_json,
                row.canonical_payload_digest,
                row.event_timestamp
            ],
            |record| record.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let index_exact: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM event_source_index i WHERE i.event_rowid=?1 \
             AND i.event_id=?2 AND i.operation_id=?3 AND i.aggregate_type=?4 \
             AND i.aggregate_id=?5 AND i.aggregate_sequence=?6 \
             AND i.introduced_message_id IS ?7);",
            params![
                event_rowid,
                row.event_id,
                row.operation_id,
                row.aggregate_type,
                row.aggregate_id,
                aggregate_sequence,
                row.introduced_message_id
            ],
            |record| record.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    if live_exact != expect_live || frozen_exact != expect_frozen || !receipt_exact {
        return Err(session_error());
    }
    if (expect_live && !index_exact) || (!expect_live && index_exact) {
        return Err(session_error());
    }
    Ok(())
}

fn verify_message_witness(
    connection: &Connection,
    job: &StorageCompactionJobV3,
    row: &hello_agent_protocol::storage_lifetime_v1::StorageCompactionPlanRowV1,
    current_chat: Option<&PlannedChatState>,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let Some(message_id) = row.introduced_message_id.as_deref() else {
        if row.introduced_message_digest.is_some() || row.message_frozen_before_plan {
            return Err(session_error());
        }
        return Ok(());
    };
    let expected_raw = row
        .introduced_message_digest
        .as_deref()
        .ok_or_else(session_error)?;
    let current = current_chat.ok_or_else(session_error)?;
    let message = current.message.as_ref().ok_or_else(session_error)?;
    if current.snapshot.as_deref() != Some(message_id) {
        return Err(session_error());
    }
    let message_sequence = message
        .get("eventSequence")
        .and_then(Value::as_u64)
        .ok_or_else(session_error)?;
    let created_at = message
        .get("createdAt")
        .and_then(Value::as_str)
        .ok_or_else(session_error)?;
    let expected_canonical = canonical_json_v1(message.clone()).map_err(|_| session_error())?;
    let event_rowid = i64::try_from(row.event_rowid).map_err(|_| session_error())?;
    let aggregate_sequence = i64::try_from(row.aggregate_sequence).map_err(|_| session_error())?;
    let lengths: Option<(i64, i64)> = connection
        .query_row(
            "SELECT length(CAST(canonical_message_json AS BLOB)),length(CAST(message_digest AS BLOB)) \
             FROM chat_messages WHERE message_id=?1;",
            [message_id],
            |record| Ok((record.get(0)?, record.get(1)?)),
        )
        .optional()
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let (canonical_bytes, digest_bytes) = lengths.ok_or_else(session_error)?;
    let canonical_bytes = usize::try_from(canonical_bytes).map_err(|_| session_error())?;
    let digest_bytes = usize::try_from(digest_bytes).map_err(|_| session_error())?;
    if canonical_bytes > FRAGMENT_BYTES
        || usage
            .encoded_bytes()
            .checked_add(
                canonical_bytes
                    .checked_add(digest_bytes)
                    .ok_or_else(session_error)?,
            )
            .is_none_or(|value| value > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let (canonical, stored, metadata_exact): (String, String, bool) = connection
        .query_row(
            "SELECT canonical_message_json,message_digest,\
                    typeof(chat_id)='text' AND chat_id=?2 \
                    AND typeof(event_sequence)='integer' AND event_sequence=?3 \
                    AND typeof(created_at)='text' AND created_at=?4 \
             FROM chat_messages WHERE message_id=?1;",
            params![
                message_id,
                row.aggregate_id,
                i64::try_from(message_sequence).map_err(|_| session_error())?,
                created_at
            ],
            |record| Ok((record.get(0)?, record.get(1)?, record.get(2)?)),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    usage.fragment_bytes = usage
        .fragment_bytes
        .checked_add(canonical_bytes)
        .ok_or_else(session_error)?;
    usage.metadata_bytes = usage
        .metadata_bytes
        .checked_add(digest_bytes)
        .ok_or_else(session_error)?;
    let computed = sha256_file_version_v1(canonical.as_bytes());
    if !metadata_exact
        || canonical != expected_canonical
        || stored != computed
        || computed.strip_prefix("sha256:") != Some(expected_raw)
    {
        return Err(session_error());
    }
    let phase = job.private_state.cursor.phase;
    let after = job.private_state.cursor.after_plan_ordinal;
    let expected_frozen = match phase {
        StorageCompactionPhaseV3::PlanEligibility | StorageCompactionPhaseV3::FreezeOperations => {
            row.message_frozen_before_plan
        }
        StorageCompactionPhaseV3::FreezeMessages => {
            row.ordinal <= after || row.message_frozen_before_plan
        }
        _ => true,
    };
    let frozen_copy: Option<(bool, bool)> = connection
        .query_row(
            "SELECT typeof(source_event_rowid)='integer' AND source_event_rowid=?2 \
                    AND typeof(source_event_id)='text' AND source_event_id=?3 \
                    AND typeof(source_operation_id)='text' AND source_operation_id=?4 \
                    AND typeof(chat_id)='text' AND chat_id=?5 \
                    AND typeof(source_aggregate_sequence)='integer' \
                    AND source_aggregate_sequence=?6 \
                    AND typeof(durable_sequence)='integer' AND durable_sequence=?7 \
                    AND typeof(canonical_message_json)='text' \
                    AND CAST(canonical_message_json AS BLOB)=CAST(?8 AS BLOB) \
                    AND typeof(message_digest)='text' AND message_digest='sha256:'||?9,\
                    typeof(frozen_at)='text' AND length(CAST(frozen_at AS BLOB))=20 \
             FROM frozen_message_sources WHERE message_id=?1;",
            params![
                message_id,
                event_rowid,
                row.event_id,
                row.operation_id,
                row.aggregate_id,
                aggregate_sequence,
                i64::try_from(message_sequence).map_err(|_| session_error())?,
                canonical,
                expected_raw
            ],
            |record| Ok((record.get(0)?, record.get(1)?)),
        )
        .optional()
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    if frozen_copy.is_some_and(|(tuple, timestamp)| !tuple || !timestamp)
        || frozen_copy.is_some() != expected_frozen
    {
        return Err(session_error());
    }
    if frozen_copy.is_some() {
        if usage
            .encoded_bytes()
            .checked_add(20)
            .is_none_or(|total| total > INPUT_BYTES)
        {
            return Err(session_error());
        }
        let frozen_at: String = connection
            .query_row(
                "SELECT frozen_at FROM frozen_message_sources WHERE message_id=?1;",
                [message_id],
                |record| record.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        usage.metadata_bytes = usage
            .metadata_bytes
            .checked_add(frozen_at.len())
            .ok_or_else(session_error)?;
        hello_agent_protocol::storage_lifetime_v1::validate_frozen_operation_timestamps_v1(
            &frozen_at, &frozen_at,
        )
        .map_err(|_| session_error())?;
    }
    Ok(())
}

fn verify_released_ordinal(
    connection: &Connection,
    plan: &StorageCompactionPlanV1,
    ordinal: u32,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let sequence = plan
        .retained_boundary_sequence
        .checked_add(u64::from(ordinal))
        .ok_or_else(session_error)?;
    let lengths: Option<(i64, i64, i64)> = connection
        .query_row(
            "SELECT length(CAST(operation_id AS BLOB)),\
                    length(CAST(canonical_payload_json AS BLOB)),\
                    length(CAST(canonical_payload_digest AS BLOB)) \
             FROM frozen_operation_sources WHERE aggregate_type=?1 AND aggregate_id=?2 \
               AND aggregate_sequence=?3;",
            params![
                plan.aggregate_type,
                plan.aggregate_id,
                i64::try_from(sequence).map_err(|_| session_error())?
            ],
            |record| Ok((record.get(0)?, record.get(1)?, record.get(2)?)),
        )
        .optional()
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let (operation_bytes, canonical_bytes, digest_bytes) = lengths.ok_or_else(session_error)?;
    let operation_bytes = usize::try_from(operation_bytes).map_err(|_| session_error())?;
    let canonical_bytes = usize::try_from(canonical_bytes).map_err(|_| session_error())?;
    let digest_bytes = usize::try_from(digest_bytes).map_err(|_| session_error())?;
    let added = operation_bytes
        .checked_add(canonical_bytes)
        .and_then(|value| value.checked_add(digest_bytes))
        .ok_or_else(session_error)?;
    if canonical_bytes > FRAGMENT_BYTES
        || usage
            .encoded_bytes()
            .checked_add(added)
            .is_none_or(|value| value > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let row: Option<(i64, String, String, String)> = connection
        .query_row(
            "SELECT source_event_rowid,operation_id,canonical_payload_json,canonical_payload_digest \
             FROM frozen_operation_sources WHERE aggregate_type=?1 AND aggregate_id=?2 \
               AND aggregate_sequence=?3;",
            params![
                plan.aggregate_type,
                plan.aggregate_id,
                i64::try_from(sequence).map_err(|_| session_error())?
            ],
            |record| Ok((record.get(0)?, record.get(1)?, record.get(2)?, record.get(3)?)),
        )
        .optional()
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    let (source_rowid, operation_id, canonical, digest) = row.ok_or_else(session_error)?;
    if source_rowid <= 0
        || u64::try_from(source_rowid).map_err(|_| session_error())? > plan.cutoff_event_rowid
    {
        return Err(session_error());
    }
    if canonical.len() != canonical_bytes
        || canonical_json_v1(serde_json::from_str(&canonical).map_err(|_| session_error())?)
            .map_err(|_| session_error())?
            != canonical
        || sha256_file_version_v1(canonical.as_bytes()).strip_prefix("sha256:") != Some(&digest)
    {
        return Err(session_error());
    }
    usage.fragment_bytes = usage
        .fragment_bytes
        .checked_add(canonical_bytes)
        .ok_or_else(session_error)?;
    usage.metadata_bytes = usage
        .metadata_bytes
        .checked_add(
            operation_bytes
                .checked_add(digest_bytes)
                .ok_or_else(session_error)?,
        )
        .ok_or_else(session_error)?;
    let exact: bool = connection
        .query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM events WHERE rowid=?1) \
             AND NOT EXISTS(SELECT 1 FROM event_source_index WHERE event_rowid=?1) \
             AND EXISTS(SELECT 1 FROM operation_receipts WHERE operation_id=?2 \
               AND canonical_payload_digest=?3 AND canonical_payload_json=?4);",
            params![source_rowid, operation_id, digest, canonical],
            |record| record.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    if !exact {
        return Err(session_error());
    }
    Ok(())
}
