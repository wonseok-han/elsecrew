//! Volatile R1/R2, R3A ledger/projection and R3B derived-record verification.
//! None is a reopen/admission proof: integrity and startup remain separate checks.
//! No transaction survives a call, no cursor is persisted or accepted from callers,
//! and cancellation/error/drop discards all progress. Production fences are unchanged.

use super::*;

pub(crate) mod bytes;
pub(crate) mod canonical;
mod closure;
mod derived;
mod projection;
pub(crate) mod semantic;
pub(crate) use closure::CompactionPhase;
#[cfg(test)]
pub(crate) use closure::{phase_batch_for_test, phase_queries_for_test};
#[cfg(test)]
pub(crate) use derived::{queries_for_test as derived_queries_for_test, DerivedProbe};
pub(crate) use derived::{DerivedPhase, DerivedUsage};
pub(crate) use projection::ProjectionPhase;
#[cfg(test)]
pub(crate) use projection::{projection_batch_for_test, projection_queries_for_test};

#[derive(Clone, Copy)]
enum VerificationMode {
    OperationsOnly,
    CompactionClosure,
    LedgerProjection,
    DerivedRecords,
}

#[derive(Clone, Copy)]
enum VerificationPhase {
    Compaction(CompactionPhase),
    Projection(ProjectionPhase),
}

// Independent of the 1 MiB canonical-source budget. A canonical JSON string's
// embedding in the frozen envelope can double its bytes; frozen payload + envelope
// + receipt + live payload therefore need up to roughly 5 MiB per 1 MiB source.
// 8 MiB leaves room for duplicated identifiers/metadata, while bounding even corrupt
// dependencies before String/JSON decoding. This is an encoded-input bound, not an
// exact Rust heap bound (validators clone/parse a finite number of representations).
const MAX_CLOSURE_BYTES: usize = 8 * COMPACTION_VERIFY_MAX_PAYLOAD_BYTES;

enum SessionState {
    Scanning {
        after: u64,
        phase: VerificationPhase,
    },
    Finished,
    Invalid,
    Derived(Box<derived::Progress>),
}

/// The exclusive borrow pins connection identity and prevents same-connection
/// writes between calls (data_version only detects OTHER connections' commits).
pub(crate) struct FrozenOperationVerificationSession<'connection> {
    connection: &'connection mut Connection,
    data_version: i64,
    cutoff: u64,
    mode: VerificationMode,
    source_index_required: bool,
    state: SessionState,
}

/// Informational, non-serializable result, never a capability accepted by storage.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FrozenOperationVerificationStep {
    Pending(BatchUsage),
    Cancelled,
    FrozenOperationsVerified(BatchUsage),
    CompactionClosurePending {
        phase: CompactionPhase,
        usage: BatchUsage,
    },
    CompactionClosureVerified(BatchUsage),
    LedgerProjectionPending {
        phase: LedgerProjectionPhase,
        usage: BatchUsage,
    },
    LedgerProjectionVerified(BatchUsage),
    DerivedRecordsPending {
        phase: DerivedVerificationPhase,
        usage: DerivedUsage,
    },
    DerivedRecordsVerified(DerivedUsage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DerivedVerificationPhase {
    Prerequisite(LedgerProjectionPhase),
    Derived(DerivedPhase),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LedgerProjectionPhase {
    Compaction(CompactionPhase),
    Projection(ProjectionPhase),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct BatchUsage {
    pub(crate) rows: usize,
    // Phase-primary bytes: R1 source, R2 source/baseline/message, R3A event or
    // projection snapshot (all persisted typed fields for app instances).
    pub(crate) canonical_source_bytes: usize,
    pub(crate) encoded_closure_bytes: usize,
}

impl<'connection> FrozenOperationVerificationSession<'connection> {
    /// Exact-schema bootstrap is separate from the per-step 100-row work bound.
    pub(crate) fn start(connection: &'connection mut Connection) -> Result<Self, SidecarError> {
        Self::start_mode(connection, VerificationMode::OperationsOnly)
    }

    pub(crate) fn start_compaction_closure(
        connection: &'connection mut Connection,
    ) -> Result<Self, SidecarError> {
        Self::start_mode(connection, VerificationMode::CompactionClosure)
    }

    pub(crate) fn start_ledger_projection(
        connection: &'connection mut Connection,
    ) -> Result<Self, SidecarError> {
        Self::start_mode(connection, VerificationMode::LedgerProjection)
    }

    pub(crate) fn start_derived_records(
        connection: &'connection mut Connection,
    ) -> Result<Self, SidecarError> {
        Self::start_mode(connection, VerificationMode::DerivedRecords)
    }

    fn start_mode(
        connection: &'connection mut Connection,
        mode: VerificationMode,
    ) -> Result<Self, SidecarError> {
        if !connection.is_autocommit() {
            return Err(session_error());
        }
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| session_error())?;
        let checked = (|| {
            let marker = data_version(&transaction)?;
            preflight_schema_and_control_rows(&transaction)?;
            let materialization_state = verify_schema_v8(&transaction)?;
            verify_v6_materialization_snapshot(&transaction, &materialization_state, true)?;
            let forbidden: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM ledger_compaction_delete_authority LIMIT 1) \
                 OR EXISTS(SELECT 1 FROM maintenance_jobs WHERE job_kind='losslessCompaction' \
                           AND state IN ('pending','running') LIMIT 1);",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| session_error())?;
            if forbidden {
                return Err(session_error());
            }
            if matches!(
                mode,
                VerificationMode::CompactionClosure
                    | VerificationMode::LedgerProjection
                    | VerificationMode::DerivedRecords
            ) {
                closure::reject_hidden_rowids(&transaction)?;
            }
            if matches!(mode, VerificationMode::DerivedRecords) {
                derived::reject_hidden_rowids(&transaction)?;
            }
            let uncompacted = if matches!(
                mode,
                VerificationMode::LedgerProjection | VerificationMode::DerivedRecords
            ) {
                projection::reject_hidden_rowids(&transaction)?;
                projection::compaction_tables_empty(&transaction)?
            } else {
                false
            };
            let cutoff = if uncompacted {
                0
            } else {
                verify_terminal_compaction_proof(&transaction)?
            };
            let phase = if uncompacted {
                VerificationPhase::Projection(ProjectionPhase::Ledger)
            } else {
                VerificationPhase::Compaction(CompactionPhase::Operations)
            };
            Ok((marker, cutoff, phase, materialization_state == "ready"))
        })();
        let (marker, cutoff, phase, source_index_required) = match checked {
            Ok(values) => {
                transaction.commit().map_err(|_| session_error())?;
                values
            }
            Err(error) => {
                transaction.rollback().map_err(|_| session_error())?;
                return Err(error);
            }
        };
        if !connection.is_autocommit() {
            return Err(session_error());
        }
        Ok(Self {
            connection,
            data_version: marker,
            cutoff,
            mode,
            source_index_required,
            state: SessionState::Scanning { after: 0, phase },
        })
    }

    /// The cancellation predicate is polled before work, each row, and before
    /// commit. Cancellation is terminal and rolls back the current batch; a fresh
    /// session must scan from zero. Finished sessions cannot re-mint an outcome.
    pub(crate) fn step(
        &mut self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<FrozenOperationVerificationStep, SidecarError> {
        // Invalidate first: all errors, cancellation, panic/unwind and uncertain
        // transaction completion lose the old cursor, never publish partial work.
        let previous = std::mem::replace(&mut self.state, SessionState::Invalid);
        if matches!(previous, SessionState::Invalid) || !self.connection.is_autocommit() {
            return Err(session_error());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| session_error())?;
        if let SessionState::Derived(progress) = previous {
            let phase = progress.phase;
            let checked = (|| {
                if data_version(&transaction)? != self.data_version {
                    return Err(session_error());
                }
                derived::scan_batch(
                    &transaction,
                    *progress,
                    self.cutoff,
                    self.source_index_required,
                    cancelled,
                )
            })();
            return match checked {
                Ok(Some((progress, usage, exhausted))) if !cancelled() => {
                    transaction.commit().map_err(|_| session_error())?;
                    if !self.connection.is_autocommit() {
                        return Err(session_error());
                    }
                    let next = if exhausted {
                        progress.next_phase()
                    } else {
                        Some(progress)
                    };
                    if let Some(progress) = next {
                        self.state = SessionState::Derived(Box::new(progress));
                        Ok(FrozenOperationVerificationStep::DerivedRecordsPending {
                            phase: DerivedVerificationPhase::Derived(phase),
                            usage,
                        })
                    } else {
                        self.state = SessionState::Finished;
                        Ok(FrozenOperationVerificationStep::DerivedRecordsVerified(
                            usage,
                        ))
                    }
                }
                Ok(_) => {
                    transaction.rollback().map_err(|_| session_error())?;
                    Ok(FrozenOperationVerificationStep::Cancelled)
                }
                Err(error) => {
                    transaction.rollback().map_err(|_| session_error())?;
                    Err(error)
                }
            };
        }
        let checked = (|| {
            // Must be read under the acquired writer lock, not before BEGIN.
            if data_version(&transaction)? != self.data_version {
                return Err(session_error());
            }
            let SessionState::Scanning { after, phase } = previous else {
                return Err(session_error());
            };
            let batch = match (self.mode, phase) {
                (VerificationMode::OperationsOnly, _) => {
                    scan_batch(&transaction, after, self.cutoff, cancelled)
                }
                (_, VerificationPhase::Compaction(phase)) => {
                    closure::scan_batch(&transaction, phase, after, self.cutoff, cancelled)
                }
                (
                    VerificationMode::LedgerProjection | VerificationMode::DerivedRecords,
                    VerificationPhase::Projection(phase),
                ) => projection::scan_batch(
                    &transaction,
                    phase,
                    after,
                    self.cutoff,
                    self.source_index_required,
                    cancelled,
                ),
                _ => Err(session_error()),
            }?;
            Ok(batch.map(|(next, usage, exhausted)| (phase, next, usage, exhausted)))
        })();
        match checked {
            Ok(Some((phase, next, usage, exhausted))) if !cancelled() => {
                transaction.commit().map_err(|_| session_error())?;
                if !self.connection.is_autocommit() {
                    return Err(session_error());
                }
                if matches!(
                    self.mode,
                    VerificationMode::LedgerProjection | VerificationMode::DerivedRecords
                ) {
                    let reported = match phase {
                        VerificationPhase::Compaction(p) => LedgerProjectionPhase::Compaction(p),
                        VerificationPhase::Projection(p) => LedgerProjectionPhase::Projection(p),
                    };
                    let next_phase = if exhausted {
                        match phase {
                            VerificationPhase::Compaction(CompactionPhase::FrozenMessages) => {
                                Some(VerificationPhase::Projection(ProjectionPhase::Ledger))
                            }
                            VerificationPhase::Compaction(p) => {
                                Some(VerificationPhase::Compaction(p.next()))
                            }
                            VerificationPhase::Projection(p) => {
                                p.next().map(VerificationPhase::Projection)
                            }
                        }
                    } else {
                        Some(phase)
                    };
                    let Some(next_phase) = next_phase else {
                        if matches!(self.mode, VerificationMode::DerivedRecords) {
                            self.state = SessionState::Derived(Box::new(derived::Progress::new()));
                            return Ok(FrozenOperationVerificationStep::DerivedRecordsPending {
                                phase: DerivedVerificationPhase::Prerequisite(reported),
                                usage: usage.into(),
                            });
                        }
                        self.state = SessionState::Finished;
                        return Ok(FrozenOperationVerificationStep::LedgerProjectionVerified(
                            usage,
                        ));
                    };
                    self.state = SessionState::Scanning {
                        after: if exhausted { 0 } else { next },
                        phase: next_phase,
                    };
                    if matches!(self.mode, VerificationMode::DerivedRecords) {
                        return Ok(FrozenOperationVerificationStep::DerivedRecordsPending {
                            phase: DerivedVerificationPhase::Prerequisite(reported),
                            usage: usage.into(),
                        });
                    }
                    return Ok(FrozenOperationVerificationStep::LedgerProjectionPending {
                        phase: reported,
                        usage,
                    });
                }
                if matches!(self.mode, VerificationMode::CompactionClosure) {
                    let VerificationPhase::Compaction(phase) = phase else {
                        return Err(session_error());
                    };
                    if exhausted && phase == CompactionPhase::FrozenMessages {
                        self.state = SessionState::Finished;
                        return Ok(FrozenOperationVerificationStep::CompactionClosureVerified(
                            usage,
                        ));
                    }
                    self.state = SessionState::Scanning {
                        after: if exhausted { 0 } else { next },
                        phase: VerificationPhase::Compaction(if exhausted {
                            phase.next()
                        } else {
                            phase
                        }),
                    };
                    Ok(FrozenOperationVerificationStep::CompactionClosurePending { phase, usage })
                } else if exhausted {
                    self.state = SessionState::Finished;
                    Ok(FrozenOperationVerificationStep::FrozenOperationsVerified(
                        usage,
                    ))
                } else {
                    self.state = SessionState::Scanning { after: next, phase };
                    Ok(FrozenOperationVerificationStep::Pending(usage))
                }
            }
            Ok(_) => {
                transaction.rollback().map_err(|_| session_error())?;
                Ok(FrozenOperationVerificationStep::Cancelled)
            }
            Err(error) => {
                transaction.rollback().map_err(|_| session_error())?;
                Err(error)
            }
        }
    }
}

fn data_version(connection: &Connection) -> Result<i64, SidecarError> {
    connection
        .query_row("PRAGMA data_version;", [], |row| row.get(0))
        .map_err(|_| session_error())
}

fn session_error() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "frozen operation verification session is invalid",
    )
}

// Only fixed, internal identifiers are passed here. NULL optional IDs cost zero;
// wrong SQLite types are still rejected when existing validators load the row.
fn byte_lengths(columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("coalesce(length(CAST({column} AS BLOB)),0)"))
        .collect::<Vec<_>>()
        .join("+")
}

fn encoded_rows_bytes(
    connection: &Connection,
    query: &str,
    parameters: impl rusqlite::Params,
) -> Result<usize, SidecarError> {
    let mut statement = connection.prepare(query).map_err(|_| session_error())?;
    let mut rows = statement.query(parameters).map_err(|_| session_error())?;
    let mut bytes = 0_usize;
    // Every caller's query has a fixed LIMIT (1, 2, or golden schema size + 1).
    while let Some(row) = rows.next().map_err(|_| session_error())? {
        let length = row_bytes(row, 0)?;
        bytes = bytes.checked_add(length).ok_or_else(session_error)?;
        if bytes > MAX_CLOSURE_BYTES {
            return Err(session_error());
        }
    }
    Ok(bytes)
}

fn preflight_schema_and_control_rows(connection: &Connection) -> Result<(), SidecarError> {
    let expected = reference_schema_v8()?;
    encoded_rows_bytes(
        connection,
        &format!(
        "SELECT {} FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name LIMIT {};",
        byte_lengths(&["type", "name", "tbl_name", "sql"]), expected.len() + 1,
    ),
        [],
    )?;
    // Bound the existing schema collector before invoking it. Reading one extra
    // row proves an unexpected object without draining an arbitrary schema.
    let actual = connection
        .prepare(
            "SELECT type,name,tbl_name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' \
         ORDER BY type,name LIMIT ?1;",
        )
        .map_err(|_| session_error())?
        .query_map([sql_i64((expected.len() + 1) as u64)?], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|_| session_error())?
        .collect::<Result<Vec<SchemaObject>, _>>()
        .map_err(|_| session_error())?;
    if actual != expected {
        return Err(session_error());
    }
    // Do not rely on CHECK having been honored by a corrupt writer. With an
    // integer primary key, at most one row can equal 1: this probes at most two
    // rows before rejecting a forged singleton, bounding the helpers' COUNTs.
    for table in ["storage_schema_meta", "lifetime_materialization_job"] {
        if connection
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE singleton<>1 LIMIT 1;"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| session_error())?
            .is_some()
        {
            return Err(session_error());
        }
    }
    for (columns, from) in [
        (
            &["migration_state", "materialization_state", "migrated_at"][..],
            "storage_schema_meta LIMIT 2",
        ),
        (
            &[
                "job_id",
                "state",
                "phase",
                "failure_kind",
                "created_at",
                "updated_at",
                "completed_at",
            ][..],
            "lifetime_materialization_job LIMIT 2",
        ),
        (
            &["job_id", "cutoff_json", "completed_at"][..],
            "maintenance_jobs WHERE job_kind='losslessCompaction' AND state='succeeded' LIMIT 2",
        ),
    ] {
        encoded_rows_bytes(
            connection,
            &format!("SELECT {} FROM {from};", byte_lengths(columns)),
            [],
        )?;
    }
    Ok(())
}

const FROZEN_TEXT: &[&str] = &[
    "operation_id",
    "source_event_id",
    "request_id",
    "aggregate_type",
    "aggregate_id",
    "event_type",
    "canonical_payload_json",
    "canonical_payload_digest",
    "event_timestamp",
    "canonical_record_json",
    "record_digest",
    "frozen_at",
];

fn row_bytes(row: &rusqlite::Row<'_>, index: usize) -> Result<usize, SidecarError> {
    usize::try_from(row_u64(row, index).map_err(|_| session_error())?).map_err(|_| session_error())
}

fn dependent_bytes(connection: &Connection, rowid: u64) -> Result<usize, SidecarError> {
    let mut bytes = 0_usize;
    for (columns, from) in [
        (&["r.operation_id", "r.event_id", "r.aggregate_type", "r.aggregate_id", "r.event_type",
            "r.canonical_payload_json", "r.canonical_payload_digest_algorithm", "r.canonical_payload_digest_format",
            "r.canonical_payload_digest", "r.event_timestamp"][..],
         "operation_receipts r JOIN frozen_operation_sources f ON r.operation_id=f.operation_id WHERE f.source_event_rowid=?1 LIMIT 1"),
        (&["event_id", "operation_id", "request_id", "aggregate_type", "aggregate_id", "event_type", "canonical_json_payload", "timestamp"][..],
         "events WHERE rowid=?1 LIMIT 1"),
        (&["event_id", "operation_id", "aggregate_type", "aggregate_id", "introduced_message_id", "chat_snapshot_message_id"][..],
         "event_source_index WHERE event_rowid=?1 LIMIT 1"),
    ] {
        bytes = bytes.checked_add(encoded_rows_bytes(connection,
            &format!("SELECT {} FROM {from};", byte_lengths(columns)), [sql_i64(rowid)?])?)
            .ok_or_else(session_error)?;
        if bytes > MAX_CLOSURE_BYTES {
            return Err(session_error());
        }
    }
    Ok(bytes)
}

fn scan_batch(
    connection: &Connection,
    mut after: u64,
    cutoff: u64,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<(u64, BatchUsage, bool)>, SidecarError> {
    if cancelled() {
        return Ok(None);
    }
    let mut statement = connection.prepare(&format!(
        "SELECT source_event_rowid,length(CAST(canonical_payload_json AS BLOB)),{} \
         FROM frozen_operation_sources WHERE source_event_rowid>?1 ORDER BY source_event_rowid LIMIT 100;",
        byte_lengths(FROZEN_TEXT),
    )).map_err(|_| session_error())?;
    let mut rows = statement
        .query([sql_i64(after)?])
        .map_err(|_| session_error())?;
    let mut usage = BatchUsage::default();
    while usage.rows < COMPACTION_VERIFY_MAX_ROWS {
        let Some(row) = rows.next().map_err(|_| session_error())? else {
            break;
        };
        if cancelled() {
            return Ok(None);
        }
        let rowid = row_u64(row, 0).map_err(|_| session_error())?;
        let source_bytes = row_bytes(row, 1)?;
        let frozen_bytes = row_bytes(row, 2)?;
        if rowid <= after
            || rowid > cutoff
            || source_bytes > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES
            || frozen_bytes > MAX_CLOSURE_BYTES
        {
            return Err(session_error());
        }
        let closure_bytes = frozen_bytes
            .checked_add(dependent_bytes(connection, rowid)?)
            .ok_or_else(session_error)?;
        if closure_bytes > MAX_CLOSURE_BYTES {
            return Err(session_error());
        }
        if source_bytes > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES - usage.canonical_source_bytes
            || closure_bytes > MAX_CLOSURE_BYTES - usage.encoded_closure_bytes
        {
            return Ok(Some((after, usage, false)));
        }
        // All owned String/JSON allocations in these helpers are covered by the
        // top-level and dependency BLOB-length checks above, in this transaction.
        let value = connection.query_row(
            "SELECT operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,\
             aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,\
             canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at \
             FROM frozen_operation_sources WHERE source_event_rowid=?1;",
            [sql_i64(rowid)?],
            |row| frozen_operation_value_from_row(row).map_err(|_| rusqlite::Error::InvalidQuery),
        ).map_err(|_| storage_frozen_operation_error())?;
        let frozen = validate_frozen_operation_source_v1(&value)
            .map_err(|_| storage_frozen_operation_error())?;
        verify_frozen_operation_authority(connection, &value, &frozen)?;
        after = rowid;
        usage.rows += 1;
        usage.canonical_source_bytes += source_bytes;
        usage.encoded_closure_bytes += closure_bytes;
    }
    if after == 0 {
        return Err(session_error());
    }
    // An indexed successor is sufficient; never drain the remaining table or
    // filter by cutoff (that would hide corrupt out-of-cutoff frozen rows).
    let successor = connection
        .query_row(
            "SELECT source_event_rowid FROM frozen_operation_sources WHERE source_event_rowid>?1 \
         ORDER BY source_event_rowid LIMIT 1;",
            [sql_i64(after)?],
            |row| row_u64(row, 0),
        )
        .optional()
        .map_err(|_| session_error())?;
    Ok(Some((after, usage, successor.is_none())))
}
