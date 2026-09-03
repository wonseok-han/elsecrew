//! Derived-only catch-up following the private global scan in this connection.
//! No byte/member offset is durable. A committed partial project restarts at
//! the source boundary; exact existing members are idempotent no-ops.
use super::*;
use sha2::{Digest, Sha256};

struct WriteSource {
    bound: BoundColumn,
    digest: Option<[u8; 32]>,
}
pub(super) struct Catchup {
    pub(super) job_id: String,
    phase: usize,
    after: i64,
    canonical: Option<CanonicalProgress>,
    source: Option<WriteSource>,
    workspaces: workspace::Scan,
    pub(super) armed: bool,
}
const PHASES: [&str; 5] = [
    "indexSource",
    "receipts",
    "messages",
    "workspaces",
    "baselines",
];

impl Catchup {
    pub(super) fn start(
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<Self, SidecarError> {
        let existing:Option<(i64,bool)>=connection.query_row("SELECT length(CAST(job_id AS BLOB)),state='pending' FROM lifetime_materialization_job WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|session_error())?;
        usage.work_units += 1;
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let job_id = if let Some((length, pending)) = existing {
            let length = usize::try_from(length).map_err(|_| session_error())?;
            if !pending
                || length == 0
                || usage
                    .encoded_bytes()
                    .checked_add(length)
                    .is_none_or(|n| n > INPUT_BYTES)
            {
                return Err(session_error());
            }
            let job: String = connection
                .query_row(
                    "SELECT job_id FROM lifetime_materialization_job WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(|_| session_error())?;
            usage.work_units += 1;
            usage.metadata_bytes += job.len();
            if job.len() != length {
                return Err(session_error());
            }
            // Reverification was from the beginning in this epoch. Reset only
            // the source-boundary cursor, never invent a durable byte offset.
            let changed=connection.execute("UPDATE lifetime_materialization_job SET cutoff_event_rowid=(SELECT coalesce(max(rowid),0) FROM events),phase='indexSource',after_scan_rowid=0,scan_exhausted=0,updated_at=?2 WHERE singleton=1 AND job_id=?1 AND state='pending'",params![job,now]).map_err(|_|session_error())?;
            usage.work_units += 1;
            if changed != 1 {
                return Err(session_error());
            }
            job
        } else {
            let job = format!("materialize_{}", Uuid::new_v4());
            connection.execute("INSERT INTO lifetime_materialization_job(singleton,job_id,state,cutoff_event_rowid,phase,after_scan_rowid,scan_exhausted,failure_kind,created_at,updated_at,completed_at) SELECT 1,?1,'pending',coalesce(max(rowid),0),'indexSource',0,0,NULL,?2,?2,NULL FROM events",params![job,now]).map_err(|_|session_error())?;
            usage.work_units += 1;
            job
        };
        Ok(Self {
            job_id,
            phase: 0,
            after: 0,
            canonical: None,
            source: None,
            workspaces: workspace::Scan::writer(),
            armed: false,
        })
    }
    pub(super) fn retained_bytes(&self) -> Result<usize, SidecarError> {
        self.job_id
            .len()
            .checked_add(
                self.canonical
                    .as_ref()
                    .map_or(Ok(0), CanonicalProgress::retained_bytes)?,
            )
            .and_then(|n| {
                n.checked_add(
                    self.source
                        .as_ref()
                        .map_or(Ok(0), |s| s.bound.retained_bytes())
                        .ok()?,
                )
            })
            .and_then(|n| n.checked_add(self.workspaces.retained_bytes().ok()?))
            .ok_or_else(session_error)
    }
    pub(super) fn step(
        &mut self,
        connection: &Connection,
        cutoff: u64,
        usage: &mut ByteUsage,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Disposition, SidecarError> {
        if cancelled() {
            return Ok(Disposition::Cancelled);
        }
        let valid:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM lifetime_materialization_job WHERE singleton=1 AND job_id=?1 AND phase=?2 AND state=?3 AND scan_exhausted=0)",params![self.job_id,PHASES[self.phase],if self.armed{"attempting"}else{"pending"}],|r|r.get(0)).map_err(|_|session_error())?;
        usage.work_units += 1;
        if !valid {
            return Err(session_error());
        }
        if self.armed {
            if self.phase == 3 {
                self.workspaces.write_pending(connection, usage)?;
                self.after = self.workspaces.completed_source();
            } else {
                let source = self.source.take().ok_or_else(session_error)?;
                source.bound.recheck(connection, usage)?;
                self.write_source(connection, &source, cutoff, usage)?;
                self.after = source.bound.identity().source_rowid;
            }
            let changed=connection.execute("UPDATE lifetime_materialization_job SET state='pending',after_scan_rowid=?2,updated_at=?3 WHERE singleton=1 AND job_id=?1 AND state='attempting' AND phase=?4",params![self.job_id,self.after,Utc::now().to_rfc3339_opts(SecondsFormat::Secs,true),PHASES[self.phase]]).map_err(|_|session_error())?;
            usage.work_units += 1;
            if changed != 1 {
                return Err(session_error());
            }
            self.armed = false;
            return Ok(Disposition::Pending);
        }
        if self.source.is_some() || (self.phase == 3 && self.workspaces.pending_write()) {
            let changed=connection.execute("UPDATE lifetime_materialization_job SET state='attempting',updated_at=?2 WHERE singleton=1 AND job_id=?1 AND state='pending' AND phase=?3 AND scan_exhausted=0",params![self.job_id,Utc::now().to_rfc3339_opts(SecondsFormat::Secs,true),PHASES[self.phase]]).map_err(|_|session_error())?;
            usage.work_units += 1;
            if changed != 1 {
                return Err(session_error());
            }
            self.armed = true;
            return Ok(Disposition::Pending);
        }
        if self.phase == 3 {
            let result = self.workspaces.step(connection, usage, cancelled)?;
            if matches!(result, Disposition::Complete) {
                return self.advance(connection, usage);
            }
            return Ok(result);
        }
        if let Some(scan) = self.canonical.as_mut() {
            let result = scan_column(connection, scan, usage, cancelled)?;
            if matches!(result, Disposition::Cancelled) {
                return Ok(Disposition::Cancelled);
            }
            if matches!(result, Disposition::Complete) {
                let (bound, digest) = self
                    .canonical
                    .take()
                    .ok_or_else(session_error)?
                    .into_complete()?;
                self.source = Some(WriteSource {
                    bound,
                    digest: Some(digest),
                });
            }
            return Ok(Disposition::Pending);
        }
        let condition=match self.phase{
            0|1=>"1",
            2=>"EXISTS(SELECT 1 FROM event_source_index i WHERE i.event_rowid=e.rowid AND i.introduced_message_id IS NOT NULL)",
            4=>"e.aggregate_type!='app_instance' AND e.rowid=(SELECT rowid FROM events WHERE aggregate_type=e.aggregate_type AND aggregate_id=e.aggregate_id ORDER BY aggregate_sequence DESC LIMIT 1)",
            _=>return Err(session_error()),
        };
        let next:Option<i64>=connection.query_row(&format!("SELECT e.rowid FROM events e WHERE e.rowid>?1 AND ({condition}) ORDER BY e.rowid LIMIT 1"),[self.after],|r|r.get(0)).optional().map_err(|_|session_error())?;
        usage.work_units += 1;
        let Some(next) = next else {
            return self.advance(connection, usage);
        };
        let identity = SourceIdentity::load(connection, RawColumn::EventPayload, next, usage)?;
        let bound = BoundColumn::bind(connection, RawColumn::EventPayload, identity, usage)?;
        if matches!(self.phase, 1 | 4) {
            self.canonical = Some(CanonicalProgress::new(bound));
        } else {
            self.source = Some(WriteSource {
                bound,
                digest: None,
            });
        }
        Ok(Disposition::Pending)
    }
    fn advance(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<Disposition, SidecarError> {
        let next = PHASES
            .get(self.phase + 1)
            .copied()
            .unwrap_or("verifyBaselines");
        let changed=connection.execute("UPDATE lifetime_materialization_job SET phase=?2,after_scan_rowid=0,scan_exhausted=0,updated_at=?3 WHERE singleton=1 AND job_id=?1 AND state='pending' AND phase=?4",params![self.job_id,next,Utc::now().to_rfc3339_opts(SecondsFormat::Secs,true),PHASES[self.phase]]).map_err(|_|session_error())?;
        usage.work_units += 1;
        if changed != 1 {
            return Err(session_error());
        }
        self.phase += 1;
        self.after = 0;
        Ok(if self.phase == PHASES.len() {
            Disposition::Complete
        } else {
            Disposition::Pending
        })
    }
    fn write_source(
        &self,
        connection: &Connection,
        source: &WriteSource,
        cutoff: u64,
        usage: &mut ByteUsage,
    ) -> Result<(), SidecarError> {
        let identity = source.bound.identity();
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        match self.phase {
            0 => {
                // Every live predecessor must already have its exact index;
                // a deleted predecessor is permitted only by verified closure.
                let indexed:bool=connection.query_row("SELECT NOT EXISTS(SELECT 1 FROM events p WHERE p.aggregate_type=?1 AND p.aggregate_id=?2 AND p.aggregate_sequence=?3 AND NOT EXISTS(SELECT 1 FROM event_source_index i WHERE i.event_rowid=p.rowid))",params![identity.aggregate_type,identity.aggregate_id,identity.aggregate_sequence-1],|r|r.get(0)).map_err(|_|session_error())?;
                usage.work_units += 1;
                if !indexed {
                    return Err(session_error());
                }
                let (chat, introduced) =
                    verify_live_relation(connection, identity, cutoff, false, usage)?;
                connection.execute("INSERT OR IGNORE INTO event_source_index(event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,introduced_message_id,chat_durable_sequence,chat_snapshot_message_id) SELECT rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,?2,?3,?4 FROM events WHERE rowid=?1",params![identity.source_rowid,introduced,chat.as_ref().map(|c|c.durable as i64),chat.as_ref().and_then(|c|c.snapshot.as_deref())]).map_err(|_|session_error())?;
                usage.work_units += 1;
                source_index_matches(
                    connection,
                    identity,
                    chat.as_ref(),
                    introduced.as_deref(),
                    true,
                    usage,
                )?;
            }
            1 => {
                let digest = hex(&source.digest.ok_or_else(session_error)?);
                connection.execute("INSERT OR IGNORE INTO operation_receipts(operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,canonical_payload_digest_version,canonical_payload_digest_format,canonical_payload_digest,event_timestamp,materialized_at) SELECT operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,'sha256',1,'lowercaseHex64',?2,timestamp,?3 FROM events WHERE rowid=?1",params![identity.source_rowid,digest,now]).map_err(|_|session_error())?;
                usage.work_units += 1;
            }
            2 => {
                let (chat, introduced) =
                    verify_live_relation(connection, identity, cutoff, false, usage)?;
                let message = chat
                    .as_ref()
                    .and_then(|c| c.message.as_ref())
                    .ok_or_else(session_error)?;
                let canonical = canonical_json_v1(message.clone()).map_err(|_| session_error())?;
                let digest = sha256_file_version_v1(canonical.as_bytes());
                if introduced.is_none() {
                    return Err(session_error());
                }
                connection.execute("INSERT OR IGNORE INTO chat_messages(message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at) SELECT i.introduced_message_id,e.aggregate_id,i.chat_durable_sequence,json_extract(e.canonical_json_payload,'$.last_message'),?2,json_extract(e.canonical_json_payload,'$.last_message.createdAt'),?3 FROM events e JOIN event_source_index i ON i.event_rowid=e.rowid WHERE e.rowid=?1 AND i.introduced_message_id IS NOT NULL",params![identity.source_rowid,digest,now]).map_err(|_|session_error())?;
                usage.work_units += 1;
                verify_message_copy(connection, identity, message, false, true, usage)?;
            }
            4 => {
                let mut hash = Sha256::new();
                hash.update(identity.aggregate_type.as_bytes());
                hash.update([0]);
                hash.update(identity.aggregate_id.as_bytes());
                hash.update([0]);
                hash.update(identity.aggregate_sequence.to_string().as_bytes());
                let id = format!("baseline_{}", hex(&hash.finalize().into()));
                let digest = format!("sha256:{}", hex(&source.digest.ok_or_else(session_error)?));
                connection.execute("INSERT OR IGNORE INTO ledger_baselines(baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at) SELECT ?2,aggregate_type,aggregate_id,aggregate_sequence,payload_schema_version,canonical_json_payload,?3,?4,?4 FROM events WHERE rowid=?1",params![identity.source_rowid,id,digest,now]).map_err(|_|session_error())?;
                usage.work_units += 1;
            }
            _ => return Err(session_error()),
        }
        Ok(())
    }
}
