//! Same-connection fragment consumer. This module does not publish product
//! readiness or weaken the product's compaction/admission fences.

use super::bytes::{BoundColumn, ByteUsage, RawColumn, SourceIdentity, INPUT_BYTES};
use super::canonical::{CanonicalUsage, Disposition, Progress as CanonicalProgress};
use super::*;
mod catchup;
mod workspace;

const CHUNK: usize = 64 * 1024;

enum SourceStage {
    Payload(CanonicalProgress),
    FrozenEnvelope {
        scan: CanonicalProgress,
        payload_digest: [u8; 32],
    },
    Fields {
        bound: BoundColumn,
        payload_digest: [u8; 32],
        record_digest: Option<[u8; 32]>,
    },
}
#[derive(Clone, PartialEq, Eq)]
enum SourceStageMarker {
    Payload(canonical::ScanPosition),
    FrozenEnvelope(canonical::ScanPosition),
    Fields(i64, bool),
}
impl SourceStage {
    fn retained_bytes(&self) -> Result<usize, SidecarError> {
        match self {
            Self::Payload(p) | Self::FrozenEnvelope { scan: p, .. } => p.retained_bytes(),
            Self::Fields { bound, .. } => bound.retained_bytes(),
        }
    }
    fn progress_marker(&self) -> SourceStageMarker {
        match self {
            Self::Payload(progress) => SourceStageMarker::Payload(progress.position()),
            Self::FrozenEnvelope { scan, .. } => SourceStageMarker::FrozenEnvelope(scan.position()),
            Self::Fields {
                bound,
                record_digest,
                ..
            } => SourceStageMarker::Fields(bound.identity().source_rowid, record_digest.is_some()),
        }
    }
}

struct SourceScan {
    frozen: bool,
    require_index: bool,
    plan_aware: bool,
    after: i64,
    active: Option<SourceStage>,
}
impl SourceScan {
    fn new(frozen: bool, require_index: bool, plan_aware: bool) -> Self {
        Self {
            frozen,
            require_index,
            plan_aware,
            after: 0,
            active: None,
        }
    }
    fn retained_bytes(&self) -> Result<usize, SidecarError> {
        self.active
            .as_ref()
            .map_or(Ok(0), SourceStage::retained_bytes)
    }

    fn step(
        &mut self,
        connection: &Connection,
        cutoff: u64,
        usage: &mut ByteUsage,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Disposition, SidecarError> {
        if cancelled() {
            return Ok(Disposition::Cancelled);
        }
        let Some(active) = self.active.take() else {
            let query = if self.frozen {
                "SELECT source_event_rowid FROM frozen_operation_sources WHERE source_event_rowid>?1 ORDER BY source_event_rowid LIMIT 1"
            } else if self.plan_aware {
                // AdmissionPlanWitness has already checked every persisted plan
                // row in this epoch. Scan every live row outside that witness.
                "SELECT e.rowid FROM events e WHERE e.rowid>?1 \
                 AND NOT EXISTS(SELECT 1 FROM storage_compaction_plan_rows planned \
                   WHERE planned.event_rowid=e.rowid) ORDER BY e.rowid LIMIT 1"
            } else {
                "SELECT rowid FROM events WHERE rowid>?1 ORDER BY rowid LIMIT 1"
            };
            if usage.work_units > 95 {
                return Ok(Disposition::Pending);
            }
            let next: Option<i64> = connection
                .query_row(query, [self.after], |r| r.get(0))
                .optional()
                .map_err(|_| session_error())?;
            usage.work_units += 1;
            let Some(next) = next else {
                return Ok(Disposition::Complete);
            };
            let column = if self.frozen {
                RawColumn::FrozenPayload
            } else {
                RawColumn::EventPayload
            };
            let identity = SourceIdentity::load(connection, column, next, usage)?;
            identity.validate_metadata()?;
            if self.frozen && (identity.aggregate_type == "app_instance" || next as u64 > cutoff) {
                return Err(session_error());
            }
            let bound = BoundColumn::bind(connection, column, identity, usage)?;
            self.active = Some(SourceStage::Payload(CanonicalProgress::new(bound)));
            return Ok(Disposition::Pending);
        };

        match active {
            SourceStage::Payload(mut scan) => {
                let outcome = scan_column(connection, &mut scan, usage, cancelled)?;
                match outcome {
                    Disposition::Pending => self.active = Some(SourceStage::Payload(scan)),
                    Disposition::Cancelled => return Ok(Disposition::Cancelled),
                    Disposition::Complete => {
                        let (bound, payload_digest) = scan.into_complete()?;
                        self.active = Some(SourceStage::Fields {
                            bound,
                            payload_digest,
                            record_digest: None,
                        });
                    }
                }
            }
            SourceStage::FrozenEnvelope {
                mut scan,
                payload_digest,
            } => {
                let outcome = scan_column(connection, &mut scan, usage, cancelled)?;
                match outcome {
                    Disposition::Pending => {
                        self.active = Some(SourceStage::FrozenEnvelope {
                            scan,
                            payload_digest,
                        })
                    }
                    Disposition::Cancelled => return Ok(Disposition::Cancelled),
                    Disposition::Complete => {
                        let (bound, record_digest) = scan.into_complete()?;
                        self.active = Some(SourceStage::Fields {
                            bound,
                            payload_digest,
                            record_digest: Some(record_digest),
                        });
                    }
                }
            }
            SourceStage::Fields {
                bound,
                payload_digest,
                record_digest,
            } => {
                bound.recheck(connection, usage)?;
                let identity = bound.identity();
                if self.frozen && record_digest.is_none() {
                    let expected = bound.into_identity();
                    let record =
                        BoundColumn::bind(connection, RawColumn::FrozenRecord, expected, usage)?;
                    self.active = Some(SourceStage::FrozenEnvelope {
                        scan: CanonicalProgress::new(record),
                        payload_digest,
                    });
                } else {
                    if let Some(record_digest) = record_digest {
                        frozen_envelope_fields(
                            connection,
                            identity,
                            &payload_digest,
                            &record_digest,
                            usage,
                        )?;
                    }
                    source_payload_identity(connection, identity, self.frozen, usage)?;
                    verify_source_copies(
                        connection,
                        identity,
                        self.frozen,
                        self.require_index,
                        &payload_digest,
                        usage,
                    )?;
                    if !self.frozen {
                        verify_live_relation(
                            connection,
                            identity,
                            cutoff,
                            self.require_index,
                            usage,
                        )?;
                    } else {
                        verify_frozen_relation(connection, identity, cutoff, usage)?;
                    }
                    self.after = identity.source_rowid;
                }
            }
        }
        if cancelled() {
            return Ok(Disposition::Cancelled);
        }
        Ok(Disposition::Pending)
    }
}

fn scan_column(
    connection: &Connection,
    scan: &mut CanonicalProgress,
    usage: &mut ByteUsage,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Disposition, SidecarError> {
    scan.bound().recheck(connection, usage)?;
    let mut canonical = CanonicalUsage {
        input: std::mem::take(usage),
        ..Default::default()
    };
    let result = scan.scan(connection, &mut canonical, CHUNK, cancelled);
    *usage = canonical.input;
    result
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

struct ChatFields {
    durable: u64,
    snapshot: Option<String>,
    message: Option<Value>,
}

fn chat_fields(
    connection: &Connection,
    identity: &SourceIdentity,
    frozen: bool,
    usage: &mut ByteUsage,
) -> Result<ChatFields, SidecarError> {
    let (table, rowid, payload) = if frozen {
        (
            "frozen_operation_sources",
            "source_event_rowid",
            "canonical_payload_json",
        )
    } else {
        ("events", "rowid", "canonical_json_payload")
    };
    let (kind, bytes, durable_kind, durable): (String, i64, String, i64) = connection.query_row(
        &format!("SELECT json_type({payload},'$.last_message'),coalesce(length(CAST(json_extract({payload},'$.last_message') AS BLOB)),0),json_type({payload},'$.durable_event_sequence'),json_extract({payload},'$.durable_event_sequence') FROM {table} WHERE {rowid}=?1"),
        [identity.source_rowid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).map_err(|_| session_error())?;
    usage.work_units += 1;
    // A valid canonical TeamMessage has at most 64 KiB text, five 512-byte
    // identifiers and fixed keys/tags; even six-byte JSON escapes fit 1 MiB.
    // Invalid unbounded members are rejected before their TEXT is allocated.
    let bytes = usize::try_from(bytes).map_err(|_| session_error())?;
    if !matches!(kind.as_str(), "null" | "object")
        || durable_kind != "integer"
        || bytes > super::bytes::FRAGMENT_BYTES
        || usage
            .fragment_bytes
            .checked_add(bytes)
            .is_none_or(|n| n > super::bytes::FRAGMENT_BYTES)
        || usage
            .encoded_bytes()
            .checked_add(bytes)
            .is_none_or(|n| n > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let message = if kind == "null" {
        Value::Null
    } else {
        let raw: String = connection
            .query_row(
                &format!(
                    "SELECT json_extract({payload},'$.last_message') FROM {table} WHERE {rowid}=?1"
                ),
                [identity.source_rowid],
                |r| r.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        usage.fragment_bytes += bytes;
        if raw.len() != bytes {
            return Err(session_error());
        }
        serde_json::from_str(&raw).map_err(|_| session_error())?
    };
    let (durable, snapshot, message) =
        hello_agent_protocol::storage_lifetime_v1::storage_chat_snapshot_fields_v1(
            &identity.aggregate_id,
            u64::try_from(durable).map_err(|_| session_error())?,
            Some(&message),
        )
        .map_err(|_| session_error())?;
    Ok(ChatFields {
        durable,
        snapshot,
        message,
    })
}

fn source_index_matches(
    connection: &Connection,
    identity: &SourceIdentity,
    fields: Option<&ChatFields>,
    introduced: Option<&str>,
    required: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    source_index_fields_match(
        connection,
        identity,
        fields.map(|fields| fields.durable as i64),
        fields.and_then(|fields| fields.snapshot.as_deref()),
        introduced,
        required,
        usage,
    )
}

pub(super) fn source_index_fields_match(
    connection: &Connection,
    identity: &SourceIdentity,
    durable: Option<i64>,
    snapshot: Option<&str>,
    introduced: Option<&str>,
    required: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let matches =
        source_index_fields_status(connection, identity, durable, snapshot, introduced, usage)?;
    if matches == Some(false) || (required && matches.is_none()) {
        return Err(session_error());
    }
    Ok(())
}

pub(super) fn source_index_fields_status(
    connection: &Connection,
    identity: &SourceIdentity,
    durable: Option<i64>,
    snapshot: Option<&str>,
    introduced: Option<&str>,
    usage: &mut ByteUsage,
) -> Result<Option<bool>, SidecarError> {
    let matches: Option<bool> = connection.query_row(
        "SELECT (typeof(event_rowid)='integer' AND typeof(aggregate_sequence)='integer' \
         AND typeof(event_id)='text' AND typeof(operation_id)='text' AND typeof(aggregate_type)='text' AND typeof(aggregate_id)='text' \
         AND event_id=?2 AND operation_id=?3 AND aggregate_type=?4 AND aggregate_id=?5 AND aggregate_sequence=?6 \
         AND ((?7 IS NULL AND typeof(chat_durable_sequence)='null') OR (typeof(chat_durable_sequence)='integer' AND chat_durable_sequence=?7)) \
         AND ((?8 IS NULL AND typeof(chat_snapshot_message_id)='null') OR (typeof(chat_snapshot_message_id)='text' AND chat_snapshot_message_id=?8)) \
         AND ((?9 IS NULL AND typeof(introduced_message_id)='null') OR (typeof(introduced_message_id)='text' AND introduced_message_id=?9))) \
         FROM event_source_index WHERE event_rowid=?1",
        params![identity.source_rowid, identity.event_id, identity.operation_id, identity.aggregate_type, identity.aggregate_id, identity.aggregate_sequence,
            durable, snapshot, introduced],
        |r| r.get(0),
    ).optional().map_err(|_| session_error())?;
    usage.work_units += 1;
    Ok(matches)
}

fn verify_live_relation(
    connection: &Connection,
    identity: &SourceIdentity,
    cutoff: u64,
    require_index: bool,
    usage: &mut ByteUsage,
) -> Result<(Option<ChatFields>, Option<String>), SidecarError> {
    let previous: Option<i64> = connection.query_row(
        "SELECT rowid FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 AND aggregate_sequence=?3",
        params![identity.aggregate_type, identity.aggregate_id, identity.aggregate_sequence - 1], |r| r.get(0),
    ).optional().map_err(|_| session_error())?;
    usage.work_units += 1;
    let previous = match previous {
        Some(rowid) if rowid < identity.source_rowid => Some((rowid, false)),
        Some(_) => return Err(session_error()),
        None if identity.aggregate_sequence == 1 => None,
        None => {
            // A missing live predecessor is justified only by the exact
            // terminal compaction anchor and its frozen operation source.
            let frozen: Option<i64> = connection.query_row(
                "SELECT f.source_event_rowid FROM ledger_compaction_prefixes p \
                 JOIN frozen_operation_sources f ON f.aggregate_type=p.aggregate_type AND f.aggregate_id=p.aggregate_id AND f.aggregate_sequence=p.deleted_through_sequence \
                 WHERE p.aggregate_type=?1 AND p.aggregate_id=?2 AND p.anchor_aggregate_sequence=?3 \
                 AND p.anchor_event_rowid=?4 AND p.anchor_event_id=?5 AND p.anchor_operation_id=?6 \
                 AND p.deleted_through_sequence=?7 ORDER BY p.deleted_through_sequence DESC LIMIT 1",
                params![identity.aggregate_type, identity.aggregate_id, identity.aggregate_sequence, identity.source_rowid, identity.event_id, identity.operation_id, identity.aggregate_sequence - 1],
                |r| r.get(0),
            ).optional().map_err(|_| session_error())?;
            usage.work_units += 1;
            let frozen = frozen.ok_or_else(session_error)?;
            if frozen <= 0
                || frozen >= identity.source_rowid
                || frozen as u64 > cutoff
                || identity.aggregate_type == "app_instance"
            {
                return Err(session_error());
            }
            Some((frozen, true))
        }
    };
    let prefix_bad: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ledger_compaction_prefixes WHERE aggregate_type=?1 AND aggregate_id=?2 AND (deleted_through_sequence>=?3 OR aggregate_type='app_instance') LIMIT 1)",
        params![identity.aggregate_type, identity.aggregate_id, identity.aggregate_sequence], |r| r.get(0),
    ).map_err(|_| session_error())?;
    usage.work_units += 1;
    if prefix_bad {
        return Err(session_error());
    }

    if identity.aggregate_type != "chat" {
        source_index_matches(connection, identity, None, None, require_index, usage)?;
        return Ok((None, None));
    }
    let current = chat_fields(connection, identity, false, usage)?;
    let previous = match previous {
        None => None,
        Some((rowid, frozen)) => {
            let previous_identity = SourceIdentity::load(
                connection,
                if frozen {
                    RawColumn::FrozenPayload
                } else {
                    RawColumn::EventPayload
                },
                rowid,
                usage,
            )?;
            let fields = chat_fields(connection, &previous_identity, frozen, usage)?;
            // The predecessor's own transition was checked earlier in the
            // ascending live scan. Here its index must still mirror its source.
            if !frozen {
                let bad: bool = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM event_source_index WHERE event_rowid=?1 AND (event_id IS NOT ?2 OR operation_id IS NOT ?3 OR aggregate_type IS NOT 'chat' OR aggregate_id IS NOT ?4 OR aggregate_sequence IS NOT ?5 OR chat_durable_sequence IS NOT ?6 OR chat_snapshot_message_id IS NOT ?7))",
                    params![rowid, previous_identity.event_id, previous_identity.operation_id, previous_identity.aggregate_id, previous_identity.aggregate_sequence, fields.durable as i64, fields.snapshot], |r| r.get(0),
                ).map_err(|_| session_error())?;
                usage.work_units += 1;
                if bad {
                    return Err(session_error());
                }
            }
            Some(fields)
        }
    };
    let prior_durable = previous.as_ref().map_or(0, |f| f.durable);
    let introduced = (prior_durable.checked_add(1) == Some(current.durable))
        .then_some(current.snapshot.as_deref())
        .flatten();
    hello_agent_protocol::storage_lifetime_v1::validate_storage_chat_transition_fields_v1(
        &identity.aggregate_id,
        current.durable,
        current.snapshot.as_deref(),
        current.message.as_ref(),
        prior_durable,
        previous.as_ref().and_then(|f| f.snapshot.as_deref()),
        introduced,
    )
    .map_err(|_| session_error())?;
    source_index_matches(
        connection,
        identity,
        Some(&current),
        introduced,
        require_index,
        usage,
    )?;
    let introduced = introduced.map(str::to_owned);
    if introduced.is_some() {
        verify_message_copy(
            connection,
            identity,
            current.message.as_ref().ok_or_else(session_error)?,
            false,
            require_index,
            usage,
        )?;
    }
    Ok((Some(current), introduced))
}

fn verify_message_copy(
    connection: &Connection,
    identity: &SourceIdentity,
    message: &Value,
    frozen: bool,
    required: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let text = |key: &str| {
        message
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(session_error)
    };
    let canonical = canonical_json_v1(message.clone()).map_err(|_| session_error())?;
    let digest = sha256_file_version_v1(canonical.as_bytes());
    let sequence = message
        .get("eventSequence")
        .and_then(Value::as_u64)
        .ok_or_else(session_error)?;
    let matched:Option<bool>=connection.query_row(
        "SELECT typeof(chat_id)='text' AND chat_id=?2 AND typeof(event_sequence)='integer' AND event_sequence=?3 AND typeof(canonical_message_json)='text' AND CAST(canonical_message_json AS BLOB)=CAST(?4 AS BLOB) AND typeof(message_digest)='text' AND message_digest=?5 AND typeof(created_at)='text' AND created_at=?6 FROM chat_messages WHERE message_id=?1",
        params![text("messageId")?,identity.aggregate_id,sequence as i64,canonical,digest,text("createdAt")?],|r|r.get(0),
    ).optional().map_err(|_|session_error())?;
    usage.work_units += 1;
    if matched == Some(false) || (required && matched.is_none()) {
        return Err(session_error());
    }
    if !frozen {
        let composite:bool=connection.query_row(
            "SELECT json_type(canonical_json_payload,'$.record.chatId')='text' AND json_extract(canonical_json_payload,'$.record.chatId')=aggregate_id AND json_type(canonical_json_payload,'$.record.taskId')='text' AND json_extract(canonical_json_payload,'$.record.taskId')=?2 AND json_type(canonical_json_payload,'$.record.worktreeId')='text' AND json_extract(canonical_json_payload,'$.record.worktreeId')=?3 FROM events WHERE rowid=?1",
            params![identity.source_rowid,text("taskId")?,text("worktreeId")?],|r|r.get(0),
        ).map_err(|_|session_error())?;
        usage.work_units += 1;
        if !composite {
            return Err(session_error());
        }
    }
    let frozen_copy:Option<(bool,bool)>=connection.query_row(
        "SELECT (typeof(source_event_rowid)='integer' AND source_event_rowid=?2 AND typeof(source_event_id)='text' AND source_event_id=?3 AND typeof(source_operation_id)='text' AND source_operation_id=?4 AND typeof(chat_id)='text' AND chat_id=?5 AND typeof(source_aggregate_sequence)='integer' AND source_aggregate_sequence=?6 AND typeof(durable_sequence)='integer' AND durable_sequence=?7 AND typeof(canonical_message_json)='text' AND CAST(canonical_message_json AS BLOB)=CAST(?8 AS BLOB) AND typeof(message_digest)='text' AND message_digest=?9),typeof(frozen_at)='text' AND length(CAST(frozen_at AS BLOB))=20 FROM frozen_message_sources WHERE message_id=?1",
        params![text("messageId")?,identity.source_rowid,identity.event_id,identity.operation_id,identity.aggregate_id,identity.aggregate_sequence,sequence as i64,canonical,digest],|r|Ok((r.get(0)?,r.get(1)?)),
    ).optional().map_err(|_|session_error())?;
    usage.work_units += 1;
    if frozen_copy.is_some_and(|(tuple, time)| !tuple || !time)
        || (frozen && required && frozen_copy.is_none())
    {
        return Err(session_error());
    }
    if frozen_copy.is_some() {
        if usage
            .encoded_bytes()
            .checked_add(20)
            .is_none_or(|n| n > INPUT_BYTES)
        {
            return Err(session_error());
        }
        let stamp: String = connection
            .query_row(
                "SELECT frozen_at FROM frozen_message_sources WHERE message_id=?1",
                [text("messageId")?],
                |r| r.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        usage.metadata_bytes += stamp.len();
        hello_agent_protocol::storage_lifetime_v1::validate_frozen_operation_timestamps_v1(
            &stamp, &stamp,
        )
        .map_err(|_| session_error())?;
    }
    Ok(())
}

fn verify_frozen_relation(
    connection: &Connection,
    identity: &SourceIdentity,
    cutoff: u64,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let (live,index,matched):(bool,bool,bool)=connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM events e WHERE e.rowid=f.source_event_rowid),EXISTS(SELECT 1 FROM event_source_index i WHERE i.event_rowid=f.source_event_rowid),EXISTS(SELECT 1 FROM events e WHERE e.rowid=f.source_event_rowid AND e.event_id=f.source_event_id AND e.operation_id=f.operation_id AND e.request_id IS f.request_id AND e.aggregate_type=f.aggregate_type AND e.aggregate_id=f.aggregate_id AND e.aggregate_sequence=f.aggregate_sequence AND e.event_type=f.event_type AND e.payload_schema_version=f.payload_schema_version AND e.timestamp=f.event_timestamp AND CAST(e.canonical_json_payload AS BLOB)=CAST(f.canonical_payload_json AS BLOB)) FROM frozen_operation_sources f WHERE f.source_event_rowid=?1",
        [identity.source_rowid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).map_err(|_|session_error())?;
    usage.work_units += 1;
    if live != index || (live && !matched) {
        return Err(session_error());
    }
    if live {
        // The later live scan checks its predecessor and typed index fields.
        return Ok(());
    }
    let deleted:bool=connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ledger_compaction_prefixes p JOIN ledger_baselines b ON b.baseline_id=p.baseline_id WHERE p.aggregate_type=?1 AND p.aggregate_id=?2 AND (p.deleted_through_sequence=?3 OR ((SELECT storage_schema_version FROM storage_schema_meta WHERE singleton=1) IN (9,10) AND p.deleted_through_sequence>=?3)) AND b.aggregate_type=p.aggregate_type AND b.aggregate_id=p.aggregate_id AND b.cutoff_aggregate_sequence=p.deleted_through_sequence AND b.projection_digest=p.baseline_projection_digest)",
        params![identity.aggregate_type,identity.aggregate_id,identity.aggregate_sequence],|r|r.get(0),
    ).map_err(|_|session_error())?;
    usage.work_units += 1;
    if !deleted {
        return Err(session_error());
    }
    if identity.aggregate_type != "chat" {
        return Ok(());
    }
    let current = chat_fields(connection, identity, true, usage)?;
    let previous = if identity.aggregate_sequence == 1 {
        None
    } else {
        let rowid:i64=connection.query_row("SELECT source_event_rowid FROM frozen_operation_sources WHERE aggregate_type='chat' AND aggregate_id=?1 AND aggregate_sequence=?2",params![identity.aggregate_id,identity.aggregate_sequence-1],|r|r.get(0)).map_err(|_|session_error())?;
        usage.work_units += 1;
        if rowid <= 0 || rowid >= identity.source_rowid || rowid as u64 > cutoff {
            return Err(session_error());
        }
        let previous = SourceIdentity::load(connection, RawColumn::FrozenPayload, rowid, usage)?;
        Some(chat_fields(connection, &previous, true, usage)?)
    };
    let durable = previous.as_ref().map_or(0, |p| p.durable);
    let introduced = (durable.checked_add(1) == Some(current.durable))
        .then_some(current.snapshot.as_deref())
        .flatten();
    hello_agent_protocol::storage_lifetime_v1::validate_storage_chat_transition_fields_v1(
        &identity.aggregate_id,
        current.durable,
        current.snapshot.as_deref(),
        current.message.as_ref(),
        durable,
        previous.as_ref().and_then(|p| p.snapshot.as_deref()),
        introduced,
    )
    .map_err(|_| session_error())?;
    if introduced.is_some() {
        verify_message_copy(
            connection,
            identity,
            current.message.as_ref().ok_or_else(session_error)?,
            true,
            true,
            usage,
        )?;
    } else {
        let extra: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM frozen_message_sources WHERE source_operation_id=?1)",
                [&identity.operation_id],
                |r| r.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        if extra {
            return Err(session_error());
        }
    }
    Ok(())
}

pub(super) fn source_payload_identity(
    connection: &Connection,
    identity: &SourceIdentity,
    frozen: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let path = match identity.aggregate_type.as_str() {
        "app_instance" => "$.sidecar_instance_id",
        "project" => "$.project_id",
        "chat" => "$.chat_id",
        "run" => "$.run_id",
        "lock" => "$.lock_id",
        _ => return Err(session_error()),
    };
    let (table, key, payload) = if frozen {
        (
            "frozen_operation_sources",
            "source_event_rowid",
            "canonical_payload_json",
        )
    } else {
        ("events", "rowid", "canonical_json_payload")
    };
    let matches: bool = connection.query_row(
        &format!("SELECT json_type({payload})='object' AND json_type({payload},?2)='text' AND CAST(json_extract({payload},?2) AS BLOB)=CAST(aggregate_id AS BLOB) FROM {table} WHERE {key}=?1"),
        params![identity.source_rowid, path], |r| r.get(0),
    ).map_err(|_| session_error())?;
    usage.work_units += 1;
    if !matches {
        return Err(session_error());
    }
    if !frozen {
        let (projection, pk) = match identity.aggregate_type.as_str() {
            "app_instance" => ("app_instances", "sidecar_instance_id"),
            "project" => ("projects", "project_id"),
            "chat" => ("chats", "chat_id"),
            "run" => ("runs", "run_id"),
            "lock" => ("locks", "lock_id"),
            _ => return Err(session_error()),
        };
        let present: bool = connection
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {projection} WHERE {pk}=?1)"),
                [&identity.aggregate_id],
                |r| r.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        if !present {
            return Err(session_error());
        }
    }
    Ok(())
}

fn verify_source_copies(
    connection: &Connection,
    identity: &SourceIdentity,
    frozen: bool,
    ready: bool,
    payload_digest: &[u8; 32],
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let (source, rowid, payload, event, timestamp) = if frozen {
        (
            "frozen_operation_sources",
            "source_event_rowid",
            "canonical_payload_json",
            "source_event_id",
            "event_timestamp",
        )
    } else {
        (
            "events",
            "rowid",
            "canonical_json_payload",
            "event_id",
            "timestamp",
        )
    };
    let receipt: Option<bool> = connection.query_row(
        &format!("SELECT (typeof(r.event_id)='text' AND typeof(r.aggregate_type)='text' AND typeof(r.aggregate_id)='text' \
         AND typeof(r.aggregate_sequence)='integer' AND typeof(r.event_type)='text' AND typeof(r.payload_schema_version)='integer' \
         AND typeof(r.canonical_payload_json)='text' AND typeof(r.canonical_payload_digest)='text' \
         AND r.event_id=s.{event} AND r.aggregate_type=s.aggregate_type AND r.aggregate_id=s.aggregate_id \
         AND r.aggregate_sequence=s.aggregate_sequence AND r.event_type=s.event_type AND r.payload_schema_version=s.payload_schema_version \
         AND r.canonical_payload_digest_algorithm='sha256' AND typeof(r.canonical_payload_digest_version)='integer' AND r.canonical_payload_digest_version=1 \
         AND r.canonical_payload_digest_format='lowercaseHex64' AND r.canonical_payload_digest=?2 \
         AND CAST(r.canonical_payload_json AS BLOB)=CAST(s.{payload} AS BLOB) \
         AND typeof(r.event_timestamp)='text' AND r.event_timestamp=s.{timestamp}) \
         FROM {source} s JOIN operation_receipts r ON r.operation_id=s.operation_id WHERE s.{rowid}=?1"),
        params![identity.source_rowid, hex(payload_digest)], |r| r.get(0),
    ).optional().map_err(|_| session_error())?;
    usage.work_units += 1;
    if receipt == Some(false) || ((ready || frozen) && receipt.is_none()) {
        return Err(session_error());
    }
    if identity.aggregate_type == "app_instance" {
        return Ok(());
    }

    verify_source_baseline_copy(
        connection,
        identity,
        frozen,
        ready,
        &hex(payload_digest),
        usage,
    )
}

pub(super) fn verify_source_baseline_copy(
    connection: &Connection,
    identity: &SourceIdentity,
    frozen: bool,
    require_latest: bool,
    payload_digest: &str,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    if identity.aggregate_type == "app_instance" {
        return Ok(());
    }
    let (source, rowid, payload) = if frozen {
        (
            "frozen_operation_sources",
            "source_event_rowid",
            "canonical_payload_json",
        )
    } else {
        ("events", "rowid", "canonical_json_payload")
    };
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(identity.aggregate_type.as_bytes());
    hash.update([0]);
    hash.update(identity.aggregate_id.as_bytes());
    hash.update([0]);
    hash.update(identity.aggregate_sequence.to_string().as_bytes());
    let expected_id = format!("baseline_{}", hex(&hash.finalize().into()));
    let baseline: Option<bool> = connection.query_row(
        &format!("SELECT (typeof(b.baseline_id)='text' AND b.baseline_id=?2 \
         AND typeof(b.aggregate_type)='text' AND typeof(b.aggregate_id)='text' AND typeof(b.cutoff_aggregate_sequence)='integer' \
         AND typeof(b.payload_schema_version)='integer' AND b.payload_schema_version=1 \
         AND typeof(b.canonical_projection_json)='text' AND typeof(b.projection_digest)='text' \
         AND CAST(b.canonical_projection_json AS BLOB)=CAST(s.{payload} AS BLOB) AND b.projection_digest=?3) \
         FROM {source} s JOIN ledger_baselines b ON b.aggregate_type=s.aggregate_type AND b.aggregate_id=s.aggregate_id AND b.cutoff_aggregate_sequence=s.aggregate_sequence WHERE s.{rowid}=?1"),
        params![identity.source_rowid, expected_id, format!("sha256:{payload_digest}")], |r| r.get(0),
    ).optional().map_err(|_| session_error())?;
    usage.work_units += 1;
    if baseline == Some(false) {
        return Err(session_error());
    }
    if require_latest && !frozen {
        let latest: i64 = connection.query_row(
            "SELECT rowid FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 ORDER BY aggregate_sequence DESC LIMIT 1",
            params![identity.aggregate_type, identity.aggregate_id], |r| r.get(0),
        ).map_err(|_| session_error())?;
        usage.work_units += 1;
        if latest == identity.source_rowid && baseline.is_none() {
            return Err(session_error());
        }
    }
    Ok(())
}

fn frozen_envelope_fields(
    connection: &Connection,
    identity: &SourceIdentity,
    payload_digest: &[u8; 32],
    record_digest: &[u8; 32],
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    // Both JSON texts were scanned independently in this same epoch. Canonical
    // spelling plus the exact key set and typed decoded equality below preserves
    // the old canonical-envelope validator; matching hashes alone is insufficient.
    // SQLite's internal JSON evaluation is not a time/heap-bound claim.
    let valid: bool = connection.query_row(
        "SELECT (canonical_payload_digest=?2 AND record_digest=?3 \
         AND length(canonical_payload_digest)=64 AND length(record_digest)=64 \
         AND json_type(canonical_record_json)='object' \
         AND (SELECT count(*) FROM json_each(canonical_record_json))=14 \
         AND NOT EXISTS(SELECT 1 FROM json_each(canonical_record_json) WHERE key NOT IN (\
           'aggregateId','aggregateSequence','aggregateType','canonicalPayloadDigest',\
           'canonicalPayloadJson','eventTimestamp','eventType','frozenAt','operationId',\
           'payloadSchemaVersion','requestId','recordSchemaVersion','sourceEventId','sourceEventRowid')) \
         AND json_type(canonical_record_json,'$.recordSchemaVersion')='integer' \
         AND json_extract(canonical_record_json,'$.recordSchemaVersion')=1 \
         AND json_type(canonical_record_json,'$.aggregateSequence')='integer' \
         AND json_extract(canonical_record_json,'$.aggregateSequence')=aggregate_sequence \
         AND json_type(canonical_record_json,'$.sourceEventRowid')='integer' \
         AND json_extract(canonical_record_json,'$.sourceEventRowid')=source_event_rowid \
         AND json_type(canonical_record_json,'$.payloadSchemaVersion')='integer' \
         AND json_extract(canonical_record_json,'$.payloadSchemaVersion')=payload_schema_version \
         AND json_type(canonical_record_json,'$.aggregateId')='text' \
         AND CAST(json_extract(canonical_record_json,'$.aggregateId') AS BLOB)=CAST(aggregate_id AS BLOB) \
         AND json_type(canonical_record_json,'$.aggregateType')='text' \
         AND CAST(json_extract(canonical_record_json,'$.aggregateType') AS BLOB)=CAST(aggregate_type AS BLOB) \
         AND json_type(canonical_record_json,'$.canonicalPayloadDigest')='text' \
         AND CAST(json_extract(canonical_record_json,'$.canonicalPayloadDigest') AS BLOB)=CAST(canonical_payload_digest AS BLOB) \
         AND json_type(canonical_record_json,'$.canonicalPayloadJson')='text' \
         AND CAST(json_extract(canonical_record_json,'$.canonicalPayloadJson') AS BLOB)=CAST(canonical_payload_json AS BLOB) \
         AND json_type(canonical_record_json,'$.eventTimestamp')='text' \
         AND CAST(json_extract(canonical_record_json,'$.eventTimestamp') AS BLOB)=CAST(event_timestamp AS BLOB) \
         AND json_type(canonical_record_json,'$.eventType')='text' \
         AND CAST(json_extract(canonical_record_json,'$.eventType') AS BLOB)=CAST(event_type AS BLOB) \
         AND json_type(canonical_record_json,'$.frozenAt')='text' \
         AND CAST(json_extract(canonical_record_json,'$.frozenAt') AS BLOB)=CAST(frozen_at AS BLOB) \
         AND json_type(canonical_record_json,'$.operationId')='text' \
         AND CAST(json_extract(canonical_record_json,'$.operationId') AS BLOB)=CAST(operation_id AS BLOB) \
         AND json_type(canonical_record_json,'$.sourceEventId')='text' \
         AND CAST(json_extract(canonical_record_json,'$.sourceEventId') AS BLOB)=CAST(source_event_id AS BLOB) \
         AND ((request_id IS NULL AND json_type(canonical_record_json,'$.requestId')='null') OR \
              (request_id IS NOT NULL AND json_type(canonical_record_json,'$.requestId')='text' \
               AND CAST(json_extract(canonical_record_json,'$.requestId') AS BLOB)=CAST(request_id AS BLOB)))) \
         FROM frozen_operation_sources WHERE operation_id=?1",
        params![identity.operation_id, hex(payload_digest), hex(record_digest)], |r| r.get(0),
    ).map_err(|_| session_error())?;
    usage.work_units += 1;
    if !valid {
        return Err(session_error());
    }
    // The two persisted timestamps have the same predicate as the V1 frozen
    // validator; no date normalization or provenance is inferred from the hash.
    let timestamp_valid: bool = connection.query_row(
        "SELECT typeof(frozen_at)='text' AND length(CAST(frozen_at AS BLOB))=20 FROM frozen_operation_sources WHERE operation_id=?1",
        [&identity.operation_id], |r| r.get(0),
    ).map_err(|_| session_error())?;
    usage.work_units += 1;
    if !timestamp_valid {
        return Err(session_error());
    }
    if usage
        .encoded_bytes()
        .checked_add(20)
        .is_none_or(|n| n > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let frozen_at: String = connection
        .query_row(
            "SELECT frozen_at FROM frozen_operation_sources WHERE operation_id=?1",
            [&identity.operation_id],
            |r| r.get(0),
        )
        .map_err(|_| session_error())?;
    usage.work_units += 1;
    usage.metadata_bytes += 20;
    hello_agent_protocol::storage_lifetime_v1::validate_frozen_operation_timestamps_v1(
        &identity.timestamp,
        &frozen_at,
    )
    .map_err(|_| session_error())?;
    Ok(())
}

/// Volatile owned progress; no externally supplied cursor or verified flag.
enum SnapshotProgress {
    Plan(plan_v9::AdmissionPlanWitness),
    Sources(SourceScan),
    Copies(CopiesScan),
    Projections(ProjectionScan),
    Messages(MessageScan),
    Workspaces(workspace::Scan),
    Catchup(Box<catchup::Catchup>),
}

#[derive(Clone, PartialEq, Eq)]
enum SnapshotProgressMarker {
    Plan(plan_v9::AdmissionPlanMarker),
    Sources(bool, bool, bool, i64, Option<SourceStageMarker>),
    Copies(usize, i64, bool),
    Projections(
        usize,
        i64,
        i64,
        Option<(&'static str, canonical::ScanPosition, u64)>,
    ),
    Messages(bool, bool, i64, Option<(i64, String, String, i64)>),
    Workspaces(workspace::ProgressMarker),
    Catchup(catchup::ProgressMarker),
}

impl SnapshotProgress {
    fn retained_bytes(&self) -> Result<usize, SidecarError> {
        match self {
            Self::Plan(plan) => plan.retained_bytes(),
            Self::Sources(s) => s.retained_bytes(),
            Self::Copies(_) => Ok(0),
            Self::Messages(m) => Ok(m
                .outer
                .as_ref()
                .map_or(0, |o| o.message.len() + o.chat.len())),
            Self::Projections(p) => p
                .active
                .as_ref()
                .map_or(Ok(0), semantic::Progress::retained_bytes),
            Self::Workspaces(w) => w.retained_bytes(),
            Self::Catchup(c) => c.retained_bytes(),
        }
    }

    fn progress_marker(&self) -> SnapshotProgressMarker {
        match self {
            Self::Plan(plan) => SnapshotProgressMarker::Plan(plan.marker()),
            Self::Sources(source) => SnapshotProgressMarker::Sources(
                source.frozen,
                source.require_index,
                source.plan_aware,
                source.after,
                source.active.as_ref().map(SourceStage::progress_marker),
            ),
            Self::Copies(copies) => {
                SnapshotProgressMarker::Copies(copies.phase, copies.after, copies.plan_aware)
            }
            Self::Projections(projections) => SnapshotProgressMarker::Projections(
                projections.table,
                projections.after,
                projections.selected,
                projections
                    .active
                    .as_ref()
                    .map(semantic::Progress::progress_marker),
            ),
            Self::Messages(messages) => SnapshotProgressMarker::Messages(
                messages.backward,
                messages.plan_aware,
                messages.after,
                messages.outer.as_ref().map(|outer| {
                    (
                        outer.rowid,
                        outer.message.clone(),
                        outer.chat.clone(),
                        outer.candidate,
                    )
                }),
            ),
            Self::Workspaces(workspaces) => {
                SnapshotProgressMarker::Workspaces(workspaces.progress_marker())
            }
            Self::Catchup(catchup) => SnapshotProgressMarker::Catchup(catchup.progress_marker()),
        }
    }
}

struct MessageScan {
    backward: bool,
    plan_aware: bool,
    after: i64,
    outer: Option<MessageOuter>,
}
struct MessageOuter {
    rowid: i64,
    message: String,
    chat: String,
    candidate: i64,
}
impl MessageScan {
    fn step(
        &mut self,
        connection: &Connection,
        cutoff: u64,
        usage: &mut ByteUsage,
    ) -> Result<Disposition, SidecarError> {
        if let Some(outer) = self.outer.as_mut() {
            let next:Option<i64>=if self.backward {
                connection.query_row("SELECT rowid FROM events WHERE aggregate_type='chat' AND aggregate_id=?1 AND rowid>?2 ORDER BY rowid LIMIT 1",params![outer.chat,outer.candidate],|r|r.get(0))
            }else{
                connection.query_row("SELECT rowid FROM events WHERE aggregate_type='chat' AND rowid>?1 ORDER BY rowid LIMIT 1",[outer.candidate],|r|r.get(0))
            }.optional().map_err(|_|session_error())?;
            usage.work_units += 1;
            let Some(next) = next else {
                if self.backward {
                    return Err(session_error());
                }
                self.after = outer.rowid;
                self.outer = None;
                return Ok(Disposition::Pending);
            };
            outer.candidate = next;
            if !self.backward && next == outer.rowid {
                return Ok(Disposition::Pending);
            }
            let indexed:Option<bool>=connection.query_row("SELECT introduced_message_id IS ?2 FROM event_source_index WHERE event_rowid=?1",params![next,outer.message],|r|r.get(0)).optional().map_err(|_|session_error())?;
            usage.work_units += 1;
            let matches = match indexed {
                Some(matches) => matches,
                None => {
                    let identity =
                        SourceIdentity::load(connection, RawColumn::EventPayload, next, usage)?;
                    let (_, introduced) =
                        verify_live_relation(connection, &identity, cutoff, false, usage)?;
                    introduced.as_deref() == Some(outer.message.as_str())
                }
            };
            if matches {
                if !self.backward {
                    return Err(session_error());
                }
                // Source-side coverage already compared this exact introduction
                // with the normalized row, even if the index was absent.
                self.after = outer.rowid;
                self.outer = None;
            }
            return Ok(Disposition::Pending);
        }
        if !self.backward {
            let query = if self.plan_aware {
                "SELECT e.rowid FROM events e WHERE e.aggregate_type='chat' AND e.rowid>?1 \
                 AND NOT EXISTS(SELECT 1 FROM storage_compaction_plan_rows planned \
                   WHERE planned.event_rowid=e.rowid) ORDER BY e.rowid LIMIT 1"
            } else {
                "SELECT rowid FROM events WHERE aggregate_type='chat' AND rowid>?1 ORDER BY rowid LIMIT 1"
            };
            let rowid: Option<i64> = connection
                .query_row(query, [self.after], |r| r.get(0))
                .optional()
                .map_err(|_| session_error())?;
            usage.work_units += 1;
            let Some(rowid) = rowid else {
                self.backward = true;
                self.after = 0;
                return Ok(Disposition::Pending);
            };
            let identity = SourceIdentity::load(connection, RawColumn::EventPayload, rowid, usage)?;
            let (_, introduced) =
                verify_live_relation(connection, &identity, cutoff, false, usage)?;
            match introduced {
                Some(message) => {
                    let conflict:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM frozen_message_sources WHERE message_id=?1 AND source_event_rowid!=?2)",params![message,rowid],|r|r.get(0)).map_err(|_|session_error())?;
                    usage.work_units += 1;
                    if conflict {
                        return Err(session_error());
                    }
                    self.outer = Some(MessageOuter {
                        rowid,
                        message,
                        chat: String::new(),
                        candidate: 0,
                    });
                }
                None => self.after = rowid,
            }
            return Ok(Disposition::Pending);
        }
        let row:Option<(i64,i64,i64,bool)>=connection.query_row(
            "SELECT rowid,CASE WHEN typeof(message_id)='text' THEN length(CAST(message_id AS BLOB)) ELSE -1 END,CASE WHEN typeof(chat_id)='text' THEN length(CAST(chat_id AS BLOB)) ELSE -1 END,EXISTS(SELECT 1 FROM frozen_message_sources f WHERE f.message_id=m.message_id) FROM chat_messages m WHERE rowid>?1 ORDER BY rowid LIMIT 1",
            [self.after],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional().map_err(|_|session_error())?;
        usage.work_units += 1;
        let Some((rowid, message_len, chat_len, frozen)) = row else {
            return Ok(Disposition::Complete);
        };
        if !(1..=512).contains(&message_len) || !(1..=512).contains(&chat_len) {
            return Err(session_error());
        }
        if frozen {
            self.after = rowid;
            return Ok(Disposition::Pending);
        }
        if usage
            .encoded_bytes()
            .checked_add((message_len + chat_len) as usize)
            .is_none_or(|n| n > INPUT_BYTES)
        {
            return Err(session_error());
        }
        let (message, chat): (String, String) = connection
            .query_row(
                "SELECT message_id,chat_id FROM chat_messages WHERE rowid=?1",
                [rowid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        usage.metadata_bytes += message.len() + chat.len();
        if message.len() != message_len as usize || chat.len() != chat_len as usize {
            return Err(session_error());
        }
        self.outer = Some(MessageOuter {
            rowid,
            message,
            chat,
            candidate: 0,
        });
        Ok(Disposition::Pending)
    }
}

struct CopiesScan {
    phase: usize,
    after: i64,
    plan_aware: bool,
}
impl CopiesScan {
    fn step(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<Disposition, SidecarError> {
        const TABLES: [&str; 5] = [
            "event_source_index",
            "operation_receipts",
            "ledger_baselines",
            "ledger_compaction_prefixes",
            "frozen_message_sources",
        ];
        let Some(table) = TABLES.get(self.phase) else {
            return Ok(Disposition::Complete);
        };
        let query = if self.plan_aware && self.phase == 4 {
            // Exact planned message copies were fully rebound by AdmissionPlanWitness.
            // Rows outside that completed witness still take the generic copy path.
            "SELECT d.rowid FROM frozen_message_sources d WHERE d.rowid>?1 \
             AND NOT EXISTS(SELECT 1 FROM storage_compaction_plan_rows planned \
               WHERE planned.event_rowid=d.source_event_rowid \
                 AND planned.event_id=d.source_event_id \
                 AND planned.operation_id=d.source_operation_id \
                 AND planned.aggregate_type='chat' AND planned.aggregate_id=d.chat_id \
                 AND planned.aggregate_sequence=d.source_aggregate_sequence \
                 AND planned.introduced_message_id=d.message_id) \
             ORDER BY d.rowid LIMIT 1"
                .to_string()
        } else {
            format!("SELECT rowid FROM {table} WHERE rowid>?1 ORDER BY rowid LIMIT 1")
        };
        let next: Option<i64> = connection
            .query_row(&query, [self.after], |r| r.get(0))
            .optional()
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        let Some(rowid) = next else {
            self.phase += 1;
            self.after = 0;
            return Ok(Disposition::Pending);
        };
        let predicate=match self.phase{
            0=>"EXISTS(SELECT 1 FROM events s WHERE s.rowid=d.event_rowid)",
            1=>"EXISTS(SELECT 1 FROM events s WHERE s.operation_id=d.operation_id) OR EXISTS(SELECT 1 FROM frozen_operation_sources s WHERE s.operation_id=d.operation_id)",
            2=>"d.aggregate_type!='app_instance' AND (EXISTS(SELECT 1 FROM events s WHERE s.aggregate_type=d.aggregate_type AND s.aggregate_id=d.aggregate_id AND s.aggregate_sequence=d.cutoff_aggregate_sequence) OR EXISTS(SELECT 1 FROM frozen_operation_sources s WHERE s.aggregate_type=d.aggregate_type AND s.aggregate_id=d.aggregate_id AND s.aggregate_sequence=d.cutoff_aggregate_sequence))",
            3=>{verify_prefix_row(connection,rowid,self.plan_aware,usage)?;self.after=rowid;return Ok(Disposition::Pending);}
            4=>"EXISTS(SELECT 1 FROM frozen_operation_sources s WHERE s.operation_id=d.source_operation_id AND s.aggregate_type='chat' AND s.source_event_rowid=d.source_event_rowid AND s.source_event_id=d.source_event_id AND s.aggregate_id=d.chat_id AND s.aggregate_sequence=d.source_aggregate_sequence AND NOT EXISTS(SELECT 1 FROM events e WHERE e.rowid=s.source_event_rowid))",
            _=>return Err(session_error()),
        };
        let exists: bool = connection
            .query_row(
                &format!("SELECT ({predicate}) FROM {table} d WHERE d.rowid=?1"),
                [rowid],
                |r| r.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        if !exists {
            return Err(session_error());
        }
        self.after = rowid;
        Ok(Disposition::Pending)
    }
}

fn verify_prefix_row(
    connection: &Connection,
    rowid: i64,
    plan_aware: bool,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    use hello_agent_protocol::storage_lifetime_v1::{
        validate_storage_compaction_prefix_fields_v1, StorageCompactionPrefixV1,
    };
    let bytes:i64=connection.query_row(
        "SELECT length(CAST(prefix_id AS BLOB))+length(CAST(aggregate_type AS BLOB))+length(CAST(aggregate_id AS BLOB))+length(CAST(anchor_event_id AS BLOB))+length(CAST(anchor_operation_id AS BLOB))+length(CAST(baseline_id AS BLOB))+length(CAST(baseline_projection_digest AS BLOB))+length(CAST(created_at AS BLOB))+length(CAST(verified_at AS BLOB)) FROM ledger_compaction_prefixes WHERE rowid=?1",
        [rowid],|r|r.get(0),
    ).map_err(|_|session_error())?;
    usage.work_units += 1;
    let bytes = usize::try_from(bytes).map_err(|_| session_error())?;
    if usage
        .encoded_bytes()
        .checked_add(bytes)
        .is_none_or(|n| n > INPUT_BYTES)
    {
        return Err(session_error());
    }
    let p=connection.query_row(
        "SELECT prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,baseline_projection_digest,created_at,verified_at FROM ledger_compaction_prefixes WHERE rowid=?1",
        [rowid],|r|Ok(StorageCompactionPrefixV1{prefix_id:r.get(0)?,aggregate_type:r.get(1)?,aggregate_id:r.get(2)?,deleted_through_sequence:row_u64(r,3)?,anchor_event_rowid:row_u64(r,4)?,anchor_event_id:r.get(5)?,anchor_operation_id:r.get(6)?,anchor_aggregate_sequence:row_u64(r,7)?,baseline_id:r.get(8)?,baseline_projection_digest:r.get(9)?,created_at:r.get(10)?,verified_at:r.get(11)?}),
    ).map_err(|_|session_error())?;
    usage.work_units += 1;
    usage.metadata_bytes += bytes;
    validate_storage_compaction_prefix_fields_v1(&p).map_err(|_| session_error())?;
    let valid:bool=connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM frozen_operation_sources s JOIN frozen_operation_sources a ON a.aggregate_type=s.aggregate_type AND a.aggregate_id=s.aggregate_id AND a.aggregate_sequence=s.aggregate_sequence+1 JOIN ledger_baselines b ON b.baseline_id=p.baseline_id \
         WHERE s.aggregate_type=p.aggregate_type AND s.aggregate_id=p.aggregate_id AND s.aggregate_sequence=p.deleted_through_sequence \
         AND NOT EXISTS(SELECT 1 FROM events e WHERE e.rowid=s.source_event_rowid) AND NOT EXISTS(SELECT 1 FROM event_source_index i WHERE i.event_rowid=s.source_event_rowid) \
         AND b.aggregate_type=p.aggregate_type AND b.aggregate_id=p.aggregate_id AND b.cutoff_aggregate_sequence=p.deleted_through_sequence AND b.projection_digest=p.baseline_projection_digest \
         AND a.source_event_rowid=p.anchor_event_rowid AND a.source_event_id=p.anchor_event_id AND a.operation_id=p.anchor_operation_id AND a.aggregate_sequence=p.anchor_aggregate_sequence \
         AND ((SELECT storage_schema_version FROM storage_schema_meta WHERE singleton=1) IN (9,10) OR p.deleted_through_sequence=1 OR EXISTS(SELECT 1 FROM ledger_compaction_prefixes prev WHERE prev.aggregate_type=p.aggregate_type AND prev.aggregate_id=p.aggregate_id AND prev.deleted_through_sequence=p.deleted_through_sequence-1 AND prev.anchor_event_rowid=s.source_event_rowid AND prev.anchor_event_id=s.source_event_id AND prev.anchor_operation_id=s.operation_id AND prev.anchor_aggregate_sequence=s.aggregate_sequence)) \
         AND ((EXISTS(SELECT 1 FROM ledger_compaction_prefixes newer WHERE newer.aggregate_type=p.aggregate_type AND newer.aggregate_id=p.aggregate_id AND newer.deleted_through_sequence>p.deleted_through_sequence) \
              AND NOT EXISTS(SELECT 1 FROM events e WHERE e.rowid=a.source_event_rowid) AND NOT EXISTS(SELECT 1 FROM event_source_index i WHERE i.event_rowid=a.source_event_rowid)) \
           OR (NOT EXISTS(SELECT 1 FROM ledger_compaction_prefixes newer WHERE newer.aggregate_type=p.aggregate_type AND newer.aggregate_id=p.aggregate_id AND newer.deleted_through_sequence>p.deleted_through_sequence) \
              AND EXISTS(SELECT 1 FROM events e WHERE e.rowid=a.source_event_rowid) AND EXISTS(SELECT 1 FROM event_source_index i WHERE i.event_rowid=a.source_event_rowid)))) \
         FROM ledger_compaction_prefixes p WHERE p.rowid=?1",
        [rowid],|r|r.get(0),
    ).map_err(|_|session_error())?;
    usage.work_units += 1;
    let active_plan_valid = plan_aware
        && connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ledger_compaction_prefixes p \
                 JOIN storage_compaction_plans plan ON plan.job_id IN (\
                   SELECT job_id FROM maintenance_jobs WHERE job_kind='losslessCompaction' \
                     AND state='pending' AND json_extract(cutoff_json, \
                       '$.storageCompactionV3PrivateState.cursor.phase') IN ('authorizeDelete','deletePrefix')) \
                 JOIN storage_compaction_plan_rows anchor ON anchor.job_id=plan.job_id AND anchor.ordinal=1001 \
                 JOIN ledger_baselines b ON b.baseline_id=plan.baseline_id \
                 WHERE p.rowid=?1 AND p.aggregate_type=plan.aggregate_type \
                   AND p.aggregate_id=plan.aggregate_id \
                   AND p.deleted_through_sequence=plan.delete_through_sequence \
                   AND p.anchor_event_rowid=anchor.event_rowid AND p.anchor_event_id=anchor.event_id \
                   AND p.anchor_operation_id=anchor.operation_id \
                   AND p.anchor_aggregate_sequence=plan.anchor_aggregate_sequence \
                   AND p.baseline_id=plan.baseline_id \
                   AND p.baseline_projection_digest='sha256:'||plan.baseline_projection_digest \
                   AND b.aggregate_type=plan.aggregate_type AND b.aggregate_id=plan.aggregate_id \
                   AND b.cutoff_aggregate_sequence=plan.delete_through_sequence \
                   AND b.projection_digest=p.baseline_projection_digest);",
                [rowid],
                |r| r.get(0),
            )
            .map_err(|_| session_error())?;
    usage.work_units += usize::from(plan_aware);
    if !valid && !active_plan_valid {
        return Err(session_error());
    }
    Ok(())
}

struct ProjectionScan {
    table: usize,
    after: i64,
    selected: i64,
    active: Option<semantic::Progress>,
}
impl ProjectionScan {
    fn new() -> Self {
        Self {
            table: 0,
            after: 0,
            selected: 0,
            active: None,
        }
    }
    fn step(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Disposition, SidecarError> {
        if cancelled() {
            return Ok(Disposition::Cancelled);
        }
        if let Some(active) = self.active.as_mut() {
            match active.scan(connection, usage, CHUNK, cancelled)? {
                Disposition::Complete => {
                    self.active = None;
                    self.after = self.selected;
                }
                other => return Ok(other),
            }
            return Ok(Disposition::Pending);
        }
        const TABLES: [(&str, &str, &str); 5] = [
            ("app_instances", "sidecar_instance_id", "app_instance"),
            ("projects", "project_id", "project"),
            ("chats", "chat_id", "chat"),
            ("runs", "run_id", "run"),
            ("locks", "lock_id", "lock"),
        ];
        let Some((table, pk, aggregate)) = TABLES.get(self.table).copied() else {
            return Ok(Disposition::Complete);
        };
        let row: Option<(i64, Option<i64>)> = connection.query_row(
            &format!("SELECT p.rowid,(SELECT e.rowid FROM events e WHERE e.aggregate_type=?2 AND e.aggregate_id=p.{pk} ORDER BY e.aggregate_sequence DESC LIMIT 1) FROM {table} p WHERE p.rowid>?1 ORDER BY p.rowid LIMIT 1"),
            params![self.after, aggregate], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional().map_err(|_| session_error())?;
        usage.work_units += 1;
        let Some((rowid, source)) = row else {
            self.table += 1;
            self.after = 0;
            return Ok(Disposition::Pending);
        };
        let identity = SourceIdentity::load(
            connection,
            RawColumn::EventPayload,
            source.ok_or_else(session_error)?,
            usage,
        )?;
        identity.validate_metadata()?;
        if identity.aggregate_type != aggregate {
            return Err(session_error());
        }
        if aggregate == "app_instance" {
            app_instance_projection(connection, &identity, rowid, usage)?;
            self.after = rowid;
        } else {
            // Both owned copies in the S3 binding are accounted before clone.
            if usage
                .encoded_bytes()
                .checked_add(identity.aggregate_id.len())
                .is_none_or(|n| n > INPUT_BYTES)
            {
                return Err(session_error());
            }
            usage.retained_bytes += identity.aggregate_id.len();
            self.active = Some(semantic::Progress::bind(
                connection,
                RawColumn::EventPayload,
                identity,
                usage,
            )?);
            self.selected = rowid;
        }
        Ok(Disposition::Pending)
    }
}

fn app_instance_projection(
    connection: &Connection,
    identity: &SourceIdentity,
    rowid: i64,
    usage: &mut ByteUsage,
) -> Result<(), SidecarError> {
    let valid: bool = connection.query_row(
        "SELECT (typeof(p.sidecar_instance_id)='text' AND typeof(p.pid)='integer' \
         AND typeof(p.platform)='text' AND typeof(p.started_at)='text' AND typeof(p.heartbeat_at)='text' \
         AND typeof(p.clean_shutdown_at) IN ('text','null') AND typeof(p.last_event_sequence)='integer' \
         AND p.pid BETWEEN 1 AND 4294967295 AND p.last_event_sequence=e.aggregate_sequence \
         AND NOT EXISTS(SELECT 1 FROM json_each(e.canonical_json_payload) WHERE key NOT IN (\
           'sidecar_instance_id','pid','platform','started_at','heartbeat_at','clean_shutdown_at','last_event_sequence')) \
         AND json_type(e.canonical_json_payload,'$.pid')='integer' \
         AND json_extract(e.canonical_json_payload,'$.pid')=p.pid \
         AND json_type(e.canonical_json_payload,'$.sidecar_instance_id')='text' \
         AND CAST(json_extract(e.canonical_json_payload,'$.sidecar_instance_id') AS BLOB)=CAST(p.sidecar_instance_id AS BLOB) \
         AND json_type(e.canonical_json_payload,'$.platform')='text' \
         AND CAST(json_extract(e.canonical_json_payload,'$.platform') AS BLOB)=CAST(p.platform AS BLOB) \
         AND json_type(e.canonical_json_payload,'$.started_at')='text' \
         AND CAST(json_extract(e.canonical_json_payload,'$.started_at') AS BLOB)=CAST(p.started_at AS BLOB) \
         AND json_type(e.canonical_json_payload,'$.heartbeat_at')='text' \
         AND CAST(json_extract(e.canonical_json_payload,'$.heartbeat_at') AS BLOB)=CAST(p.heartbeat_at AS BLOB) \
         AND ((p.clean_shutdown_at IS NULL AND json_type(e.canonical_json_payload,'$.clean_shutdown_at')='null') \
          OR (p.clean_shutdown_at IS NOT NULL AND json_type(e.canonical_json_payload,'$.clean_shutdown_at')='text' \
          AND CAST(json_extract(e.canonical_json_payload,'$.clean_shutdown_at') AS BLOB)=CAST(p.clean_shutdown_at AS BLOB)))) \
         FROM app_instances p JOIN events e ON e.rowid=?2 WHERE p.rowid=?1 AND p.sidecar_instance_id=e.aggregate_id",
        params![rowid, identity.source_rowid], |r| r.get(0),
    ).map_err(|_| session_error())?;
    usage.work_units += 1;
    if !valid {
        return Err(session_error());
    }
    Ok(())
}

fn admission_compaction_cutoff_v8(
    connection: &Connection,
    compacted: bool,
) -> Result<u64, SidecarError> {
    let authority_present: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ledger_compaction_delete_authority LIMIT 1);",
            [],
            |row| row.get(0),
        )
        .map_err(|_| session_error())?;
    let (jobs, succeeded, active): (i64, i64, i64) = connection
        .query_row(
            "SELECT count(*), \
                    coalesce(sum(state='succeeded'),0), \
                    coalesce(sum(state IN ('pending','running')),0) \
             FROM maintenance_jobs WHERE job_kind='losslessCompaction';",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| session_error())?;
    if authority_present || active != 0 {
        return Err(session_error());
    }
    match (compacted, jobs, succeeded) {
        (false, 0, 0) => Ok(0),
        (true, 1, 1) => verify_terminal_compaction_proof(connection),
        _ => Err(session_error()),
    }
}

pub(crate) struct ReopenMaintenanceSession<'connection> {
    connection: &'connection mut Connection,
    epoch: i64,
    cutoff: u64,
    ready: bool,
    plan_aware: bool,
    job: Option<String>,
    progress: Option<SnapshotProgress>,
    #[cfg(test)]
    force_no_progress_once: bool,
}

#[derive(Clone, Copy)]
enum ReopenStartContext {
    Legacy,
    V8Admission,
    Admission,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReopenMaintenanceStep {
    Pending(ByteUsage),
    Cancelled,
    ReadyVerified(ByteUsage),
}

impl<'connection> ReopenMaintenanceSession<'connection> {
    pub(crate) fn start(connection: &'connection mut Connection) -> Result<Self, SidecarError> {
        Self::start_inner(connection, ReopenStartContext::Legacy)
    }

    pub(crate) fn start_for_admission(
        connection: &'connection mut Connection,
    ) -> Result<Self, SidecarError> {
        Self::start_inner(connection, ReopenStartContext::Admission)
    }

    pub(crate) fn start_for_v8_admission(
        connection: &'connection mut Connection,
    ) -> Result<Self, SidecarError> {
        Self::start_inner(connection, ReopenStartContext::V8Admission)
    }

    fn start_inner(
        connection: &'connection mut Connection,
        context: ReopenStartContext,
    ) -> Result<Self, SidecarError> {
        if !connection.is_autocommit() {
            return Err(session_error());
        }
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| session_error())?;
        let epoch = data_version(&tx)?;
        let materialization = match context {
            ReopenStartContext::Legacy | ReopenStartContext::V8Admission => {
                preflight_schema_and_control_rows(&tx)?;
                verify_schema_v8(&tx)?
            }
            ReopenStartContext::Admission => {
                preflight_schema_and_control_rows_plan(&tx)?;
                match storage_schema_version(&tx)? {
                    Some(STORAGE_LIFETIME_SCHEMA_VERSION_V9) => verify_schema_v9(&tx)?,
                    Some(STORAGE_LIFETIME_SCHEMA_VERSION_V10) => verify_schema_v10(&tx)?,
                    _ => return Err(session_error()),
                }
            }
        };
        verify_v6_materialization_snapshot(&tx, &materialization, true)?;
        closure::reject_hidden_rowids(&tx)?;
        projection::reject_hidden_rowids(&tx)?;
        derived::reject_hidden_rowids(&tx)?;
        let compacted = !projection::compaction_tables_empty(&tx)?;
        let (cutoff, plan_witness) = match context {
            ReopenStartContext::Legacy => {
                let forbidden: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM ledger_compaction_delete_authority LIMIT 1) OR EXISTS(SELECT 1 FROM maintenance_jobs WHERE job_kind='losslessCompaction' AND state IN ('pending','running') LIMIT 1)", [], |r| r.get(0),
                ).map_err(|_| session_error())?;
                if forbidden {
                    return Err(session_error());
                }
                if compacted {
                    (verify_terminal_compaction_proof(&tx)?, None)
                } else {
                    (0, None)
                }
            }
            ReopenStartContext::V8Admission => {
                (admission_compaction_cutoff_v8(&tx, compacted)?, None)
            }
            ReopenStartContext::Admission => {
                let witness = plan_v9::AdmissionPlanWitness::start(&tx, compacted)?;
                (0, witness)
            }
        };
        // This flag is consumed only after the complete plan stage. A malformed,
        // gapped, extra, or unowned plan never reaches a filtered source scan.
        let plan_aware = plan_witness.is_some();
        tx.commit().map_err(|_| session_error())?;
        if !connection.is_autocommit() {
            return Err(session_error());
        }
        Ok(Self {
            connection,
            epoch,
            cutoff,
            ready: materialization == "ready",
            plan_aware,
            job: None,
            progress: Some(match plan_witness {
                Some(witness) => SnapshotProgress::Plan(witness),
                None => SnapshotProgress::Sources(SourceScan::new(
                    compacted,
                    materialization == "ready",
                    false,
                )),
            }),
            #[cfg(test)]
            force_no_progress_once: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn force_no_progress_for_test(&mut self) {
        self.force_no_progress_once = true;
    }

    pub(crate) fn step(
        &mut self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<ReopenMaintenanceStep, SidecarError> {
        let mut progress = self.progress.take().ok_or_else(session_error)?;
        let initial_progress = progress.progress_marker();
        if !self.connection.is_autocommit() {
            return Err(session_error());
        }
        let retained_bytes = progress
            .retained_bytes()?
            .checked_add(self.job.as_ref().map_or(0, String::len))
            .ok_or_else(session_error)?;
        if retained_bytes > INPUT_BYTES {
            return Err(session_error());
        }
        let mut usage = ByteUsage {
            retained_bytes,
            ..Default::default()
        };
        let was_armed = matches!(&progress,SnapshotProgress::Catchup(c) if c.armed);
        let mut finished = false;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| session_error())?;
        let checked = (|| {
            if data_version(&tx)? != self.epoch {
                return Err(session_error());
            }
            #[cfg(test)]
            let force_no_progress = std::mem::take(&mut self.force_no_progress_once);
            #[cfg(not(test))]
            let force_no_progress = false;
            let result = if force_no_progress {
                Disposition::Pending
            } else {
                match &mut progress {
                    SnapshotProgress::Plan(plan) => match plan.step(&tx, &mut usage, cancelled)? {
                        plan_v9::AdmissionPlanStep::Pending => Disposition::Pending,
                        plan_v9::AdmissionPlanStep::Cancelled => Disposition::Cancelled,
                        plan_v9::AdmissionPlanStep::Complete { cutoff_event_rowid } => {
                            self.cutoff = cutoff_event_rowid;
                            progress = SnapshotProgress::Sources(SourceScan::new(
                                self.cutoff > 0,
                                self.ready,
                                true,
                            ));
                            Disposition::Pending
                        }
                    },
                    SnapshotProgress::Sources(sources) => {
                        let result = sources.step(&tx, self.cutoff, &mut usage, cancelled)?;
                        if matches!(result, Disposition::Complete) {
                            progress = if sources.frozen {
                                SnapshotProgress::Sources(SourceScan::new(
                                    false,
                                    sources.require_index,
                                    sources.plan_aware,
                                ))
                            } else {
                                SnapshotProgress::Copies(CopiesScan {
                                    phase: 0,
                                    after: 0,
                                    plan_aware: self.plan_aware,
                                })
                            };
                        }
                        result
                    }
                    SnapshotProgress::Copies(copies) => {
                        let result = copies.step(&tx, &mut usage)?;
                        if matches!(result, Disposition::Complete) {
                            progress = SnapshotProgress::Projections(ProjectionScan::new());
                        }
                        result
                    }
                    SnapshotProgress::Projections(projections) => {
                        let result = projections.step(&tx, &mut usage, cancelled)?;
                        if matches!(result, Disposition::Complete) {
                            progress = SnapshotProgress::Messages(MessageScan {
                                backward: false,
                                plan_aware: self.plan_aware,
                                after: 0,
                                outer: None,
                            });
                        }
                        result
                    }
                    SnapshotProgress::Messages(messages) => {
                        let result = messages.step(&tx, self.cutoff, &mut usage)?;
                        if matches!(result, Disposition::Complete) {
                            progress =
                                SnapshotProgress::Workspaces(workspace::Scan::new(self.ready));
                        }
                        result
                    }
                    SnapshotProgress::Workspaces(workspaces) => {
                        let result = workspaces.step(&tx, &mut usage, cancelled)?;
                        if matches!(result, Disposition::Complete) {
                            if self.ready {
                                if let Some(job) = self.job.as_ref() {
                                    let deleted=tx.execute("DELETE FROM lifetime_materialization_job WHERE singleton=1 AND job_id=?1 AND state='pending' AND phase='verifyBaselines' AND scan_exhausted=0",[job]).map_err(|_|session_error())?;
                                    let published=tx.execute("UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1 AND materialization_state='pending'",[]).map_err(|_|session_error())?;
                                    usage.work_units += 2;
                                    if deleted != 1 || published != 1 {
                                        return Err(session_error());
                                    }
                                }
                                finished = true;
                            } else {
                                progress = SnapshotProgress::Catchup(Box::new(
                                    catchup::Catchup::start(&tx, &mut usage, self.plan_aware)?,
                                ));
                            }
                        }
                        result
                    }
                    SnapshotProgress::Catchup(catchup) => {
                        let result = catchup.step(&tx, self.cutoff, &mut usage, cancelled)?;
                        if matches!(result, Disposition::Complete) {
                            self.job = Some(std::mem::take(&mut catchup.job_id));
                            self.ready = true;
                            progress = SnapshotProgress::Sources(SourceScan::new(
                                self.cutoff > 0,
                                true,
                                self.plan_aware,
                            ));
                        }
                        result
                    }
                }
            };
            if cancelled() {
                return Ok(Disposition::Cancelled);
            }
            if usage.work_units > 100
                || usage.fragment_bytes > super::bytes::FRAGMENT_BYTES
                || usage.encoded_bytes() > INPUT_BYTES
            {
                return Err(session_error());
            }
            if matches!(result, Disposition::Pending)
                && progress.progress_marker() == initial_progress
            {
                return Err(session_error());
            }
            Ok(result)
        })();
        match checked {
            Err(error) => {
                tx.rollback().map_err(|_| session_error())?;
                Err(error)
            }
            Ok(Disposition::Cancelled) => {
                tx.rollback().map_err(|_| session_error())?;
                if was_armed {
                    // We confirmed rollback of our own attempt before clearing
                    // its marker. An error/unknown COMMIT never takes this path.
                    let SnapshotProgress::Catchup(catchup) = &progress else {
                        return Err(session_error());
                    };
                    let cancel_tx = self
                        .connection
                        .transaction_with_behavior(TransactionBehavior::Immediate)
                        .map_err(|_| session_error())?;
                    if data_version(&cancel_tx)? != self.epoch {
                        return Err(session_error());
                    }
                    let changed=cancel_tx.execute("UPDATE lifetime_materialization_job SET state='pending' WHERE singleton=1 AND job_id=?1 AND state='attempting'",[&catchup.job_id]).map_err(|_|session_error())?;
                    if changed != 1 {
                        return Err(session_error());
                    }
                    cancel_tx.commit().map_err(|_| session_error())?;
                }
                Ok(ReopenMaintenanceStep::Cancelled)
            }
            Ok(_) => {
                tx.commit().map_err(|_| session_error())?;
                if !self.connection.is_autocommit() {
                    return Err(session_error());
                }
                if finished {
                    Ok(ReopenMaintenanceStep::ReadyVerified(usage))
                } else {
                    self.progress = Some(progress);
                    Ok(ReopenMaintenanceStep::Pending(usage))
                }
            }
        }
    }
}
