//! One prepared-workspace member at a time. The two compatibility paths have
//! no V1 length cap, so even a single member is read in fragments.
use super::*;
use hello_agent_protocol::project_v1::{
    validate_worktree_manifest_policy_fields_v1, AbsolutePathScanV1, WorktreeManifestPolicyFieldsV1,
};
use sha2::{Digest, Sha256};

pub(super) struct Member {
    source: BoundColumn,
    frozen: bool,
    key: String,
    lengths: [usize; 3],
    raw_offset: usize,
    path_offset: usize,
    paths: [AbsolutePathScanV1; 2],
    root_prefix: bool,
    descendant_separator: bool,
    digest: Sha256,
}

fn source_parts(frozen: bool) -> (&'static str, &'static str, &'static str) {
    if frozen {
        (
            "frozen_operation_sources",
            "source_event_rowid",
            "canonical_payload_json",
        )
    } else {
        ("events", "rowid", "canonical_json_payload")
    }
}

fn member_from(frozen: bool) -> String {
    let (table, rowid, payload) = source_parts(frozen);
    format!("FROM {table} s JOIN json_each(s.{payload},'$.prepared_workspaces') j WHERE s.{rowid}=?1 AND j.key=?2")
}

impl Member {
    pub(super) fn start(
        connection: &Connection,
        identity: SourceIdentity,
        frozen: bool,
        key: String,
        usage: &mut ByteUsage,
    ) -> Result<Self, SidecarError> {
        if identity.aggregate_type != "project" || key.is_empty() || key.len() > 512 {
            return Err(session_error());
        }
        let source = BoundColumn::bind(
            connection,
            if frozen {
                RawColumn::FrozenPayload
            } else {
                RawColumn::EventPayload
            },
            identity,
            usage,
        )?;
        let from = member_from(frozen);
        let valid: bool = connection.query_row(
            &format!("SELECT j.type='object' \
             AND (SELECT count(*) FROM json_each(j.value))=7 \
             AND NOT EXISTS(SELECT 1 FROM json_each(j.value) WHERE key NOT IN ('schemaVersion','preparationId','manifest','bindingState','boundChatId','createdAt','updatedAt')) \
             AND json_type(j.value,'$.schemaVersion')='integer' AND json_extract(j.value,'$.schemaVersion')=1 \
             AND json_type(j.value,'$.preparationId')='text' AND CAST(json_extract(j.value,'$.preparationId') AS BLOB)=CAST(j.key AS BLOB) \
             AND json_type(j.value,'$.manifest')='object' \
             AND (SELECT count(*) FROM json_each(j.value,'$.manifest'))=15 \
             AND NOT EXISTS(SELECT 1 FROM json_each(j.value,'$.manifest') WHERE key NOT IN ('schemaVersion','projectId','canonicalGitRoot','worktreeId','worktreePath','workspaceKind','snapshotTransport','sourceMutationPolicy','objectFormat','baseCommit','baseTree','sourceIdentity','workspaceIdentity','headMode','lifecycle')) \
             AND json_type(j.value,'$.manifest.schemaVersion')='integer' AND json_extract(j.value,'$.manifest.schemaVersion')=1 \
             AND json_type(j.value,'$.manifest.projectId')='text' AND CAST(json_extract(j.value,'$.manifest.projectId') AS BLOB)=CAST(s.aggregate_id AS BLOB) \
             AND json_type(j.value,'$.manifest.canonicalGitRoot')='text' AND json_type(j.value,'$.manifest.worktreePath')='text' \
             AND json_type(j.value,'$.bindingState')='text' AND json_extract(j.value,'$.bindingState') IN ('unbound','bound','abandoned') \
             AND ((json_extract(j.value,'$.bindingState')='bound' AND json_type(j.value,'$.boundChatId')='text' AND length(CAST(json_extract(j.value,'$.boundChatId') AS BLOB)) BETWEEN 1 AND 512) \
               OR (json_extract(j.value,'$.bindingState')!='bound' AND json_type(j.value,'$.boundChatId')='null')) {from}"),
            params![source.identity().source_rowid, key], |r| r.get(0),
        ).map_err(|_| session_error())?;
        usage.work_units += 1;
        if !valid {
            return Err(session_error());
        }
        // The accepted scalar fields are bounded by their existing V1 rules.
        // Reject an overlong invalid field before allocating its TEXT.
        let fields: [&str; 16] = [
            "$.manifest.projectId",
            "$.manifest.worktreeId",
            "$.manifest.sourceIdentity",
            "$.manifest.workspaceIdentity",
            "$.manifest.workspaceKind",
            "$.manifest.snapshotTransport",
            "$.manifest.sourceMutationPolicy",
            "$.manifest.objectFormat",
            "$.manifest.baseCommit",
            "$.manifest.baseTree",
            "$.manifest.headMode",
            "$.manifest.lifecycle",
            "$.bindingState",
            "$.createdAt",
            "$.updatedAt",
            "$.preparationId",
        ];
        let mut values = Vec::with_capacity(fields.len());
        for field in fields {
            let length: i64 = connection.query_row(
                &format!("SELECT CASE WHEN json_type(j.value,?3)='text' THEN length(CAST(json_extract(j.value,?3) AS BLOB)) ELSE -1 END {from}"),
                params![source.identity().source_rowid, key, field], |r| r.get(0),
            ).map_err(|_| session_error())?;
            usage.work_units += 1;
            if !(1..=512).contains(&length)
                || usage
                    .encoded_bytes()
                    .checked_add(length as usize)
                    .is_none_or(|n| n > INPUT_BYTES)
            {
                return Err(session_error());
            }
            let value: String = connection
                .query_row(
                    &format!("SELECT json_extract(j.value,?3) {from}"),
                    params![source.identity().source_rowid, key, field],
                    |r| r.get(0),
                )
                .map_err(|_| session_error())?;
            usage.work_units += 1;
            if value.len() != length as usize {
                return Err(session_error());
            }
            usage.metadata_bytes += value.len();
            values.push(value);
        }
        validate_worktree_manifest_policy_fields_v1(&WorktreeManifestPolicyFieldsV1 {
            workspace_kind: &values[4],
            snapshot_transport: &values[5],
            source_mutation_policy: &values[6],
            object_format: &values[7],
            base_commit: &values[8],
            base_tree: &values[9],
            head_mode: &values[10],
            lifecycle: &values[11],
        })
        .map_err(|_| session_error())?;
        hello_agent_protocol::run_recovery_v1::validate_prepared_workspace_state_fields_v1(
            &values[12],
            &values[11],
            &values[13],
            &values[14],
        )
        .map_err(|_| session_error())?;
        let lengths: [i64;3] = connection.query_row(
            &format!("SELECT length(CAST(j.value AS BLOB)),length(CAST(json_extract(j.value,'$.manifest.canonicalGitRoot') AS BLOB)),length(CAST(json_extract(j.value,'$.manifest.worktreePath') AS BLOB)) {from}"),
            params![source.identity().source_rowid,key], |r| Ok([r.get(0)?,r.get(1)?,r.get(2)?]),
        ).map_err(|_| session_error())?;
        usage.work_units += 1;
        let lengths = lengths.map(|n| usize::try_from(n).unwrap_or(0));
        if lengths.contains(&0) {
            return Err(session_error());
        }
        Ok(Self {
            source,
            frozen,
            key,
            lengths,
            raw_offset: 0,
            path_offset: 0,
            paths: [AbsolutePathScanV1::default(), AbsolutePathScanV1::default()],
            root_prefix: true,
            descendant_separator: false,
            digest: Sha256::new(),
        })
    }

    pub(super) fn retained_bytes(&self) -> Result<usize, SidecarError> {
        self.source
            .retained_bytes()?
            .checked_add(self.key.len())
            .ok_or_else(session_error)
    }

    pub(super) fn scan(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<Disposition, SidecarError> {
        self.source.recheck(connection, usage)?;
        let from = member_from(self.frozen);
        let offset = if self.raw_offset < self.lengths[0] {
            self.raw_offset
        } else {
            self.path_offset
        };
        let raw = self.raw_offset < self.lengths[0];
        let available = super::super::bytes::FRAGMENT_BYTES
            .saturating_sub(usage.fragment_bytes)
            .min(INPUT_BYTES.saturating_sub(usage.encoded_bytes()));
        let amount = CHUNK.min(available / if raw { 1 } else { 2 });
        if amount == 0 {
            // This call starts with only this member's retained state and its
            // unavoidable binding read. A fresh call cannot make more room.
            return Err(session_error());
        }
        if usage.work_units > 98 {
            return Ok(Disposition::Pending);
        }
        if raw {
            let length = amount.min(self.lengths[0] - offset);
            let bytes = self.fragment(connection, &from, "j.value", offset, length, usage)?;
            self.digest.update(&bytes);
            self.raw_offset += length;
        } else if offset < self.lengths[1].max(self.lengths[2]) {
            let root_len = amount.min(self.lengths[1].saturating_sub(offset));
            let work_len = amount.min(self.lengths[2].saturating_sub(offset));
            let root = self.fragment(
                connection,
                &from,
                "json_extract(j.value,'$.manifest.canonicalGitRoot')",
                offset,
                root_len,
                usage,
            )?;
            let work = self.fragment(
                connection,
                &from,
                "json_extract(j.value,'$.manifest.worktreePath')",
                offset,
                work_len,
                usage,
            )?;
            self.paths[0].push(&root).map_err(|_| session_error())?;
            self.paths[1].push(&work).map_err(|_| session_error())?;
            let shared = root.len().min(work.len());
            self.root_prefix &= root[..shared] == work[..shared];
            if offset <= self.lengths[1] && self.lengths[1] < offset + work.len() {
                self.descendant_separator = work[self.lengths[1] - offset] == b'/';
            }
            self.path_offset += amount;
        } else {
            self.paths[0]
                .finish("canonicalGitRoot")
                .map_err(|_| session_error())?;
            self.paths[1]
                .finish("worktreePath")
                .map_err(|_| session_error())?;
            if self.root_prefix
                && self.lengths[2] >= self.lengths[1]
                && (self.lengths[2] == self.lengths[1] || self.descendant_separator)
            {
                return Err(session_error());
            }
            return Ok(Disposition::Complete);
        }
        Ok(Disposition::Pending)
    }

    fn fragment(
        &self,
        connection: &Connection,
        from: &str,
        expression: &str,
        offset: usize,
        length: usize,
        usage: &mut ByteUsage,
    ) -> Result<Vec<u8>, SidecarError> {
        if length == 0 {
            return Ok(Vec::new());
        }
        let bytes: Vec<u8> = connection
            .query_row(
                &format!("SELECT substr(CAST({expression} AS BLOB),?3,?4) {from}"),
                params![
                    self.source.identity().source_rowid,
                    self.key,
                    i64::try_from(offset + 1).map_err(|_| session_error())?,
                    length as i64
                ],
                |r| r.get(0),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        usage.fragment_bytes += bytes.len();
        if bytes.len() != length {
            return Err(session_error());
        }
        Ok(bytes)
    }

    pub(super) fn verify_bound_chat(
        &self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<(), SidecarError> {
        let from = member_from(self.frozen);
        let valid:bool=connection.query_row(
            &format!("SELECT json_type(j.value,'$.boundChatId')='null' OR EXISTS(SELECT 1 FROM events c \
             WHERE c.rowid=(SELECT rowid FROM events WHERE aggregate_type='chat' AND aggregate_id=json_extract(j.value,'$.boundChatId') ORDER BY aggregate_sequence DESC LIMIT 1) \
             AND json_type(c.canonical_json_payload,'$.project_id')='text' AND json_type(c.canonical_json_payload,'$.worktree_id')='text' AND json_type(c.canonical_json_payload,'$.workspace_identity')='text' \
             AND CAST(json_extract(c.canonical_json_payload,'$.project_id') AS BLOB)=CAST(json_extract(j.value,'$.manifest.projectId') AS BLOB) \
             AND CAST(json_extract(c.canonical_json_payload,'$.worktree_id') AS BLOB)=CAST(json_extract(j.value,'$.manifest.worktreeId') AS BLOB) \
             AND CAST(json_extract(c.canonical_json_payload,'$.workspace_identity') AS BLOB)=CAST(json_extract(j.value,'$.manifest.workspaceIdentity') AS BLOB)) {from}"),
            params![self.source.identity().source_rowid,self.key],|r|r.get(0),
        ).map_err(|_|session_error())?;
        usage.work_units += 1;
        if !valid {
            return Err(session_error());
        }
        Ok(())
    }

    pub(super) fn matches_row(
        &self,
        connection: &Connection,
        rowid: i64,
        usage: &mut ByteUsage,
    ) -> Result<bool, SidecarError> {
        let from = member_from(self.frozen);
        let digest = format!("sha256:{}", hex(&self.digest.clone().finalize().into()));
        let matches:bool=connection.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM prepared_workspaces w WHERE w.rowid=?3 \
             AND typeof(w.preparation_id)='text' AND w.preparation_id=j.key \
             AND typeof(w.project_id)='text' AND w.project_id=s.aggregate_id \
             AND typeof(w.worktree_id)='text' AND w.worktree_id=json_extract(j.value,'$.manifest.worktreeId') \
             AND typeof(w.workspace_identity)='text' AND w.workspace_identity=json_extract(j.value,'$.manifest.workspaceIdentity') \
             AND typeof(w.binding_state)='text' AND w.binding_state=json_extract(j.value,'$.bindingState') \
             AND w.bound_chat_id IS json_extract(j.value,'$.boundChatId') \
             AND typeof(w.created_at)='text' AND w.created_at=json_extract(j.value,'$.createdAt') \
             AND typeof(w.updated_at)='text' AND w.updated_at=json_extract(j.value,'$.updatedAt') \
             AND typeof(w.canonical_record_json)='text' AND CAST(w.canonical_record_json AS BLOB)=CAST(j.value AS BLOB) \
             AND typeof(w.record_digest)='text' AND w.record_digest=?4) {from}"),
            params![self.source.identity().source_rowid,self.key,rowid,digest],|r|r.get(0),
        ).map_err(|_|session_error())?;
        usage.work_units += 1;
        Ok(matches)
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }
    pub(super) fn into_key(self) -> String {
        self.key
    }
}

pub(super) struct Scan {
    ready: bool,
    write_mode: bool,
    pending_write: bool,
    backward: bool,
    after_row: i64,
    project_source: Option<(i64, String)>,
    outer: Option<Historical>,
    member: Option<Member>,
}
struct Historical {
    rowid: i64,
    preparation: String,
    project: String,
    sequence: i64,
    frozen: bool,
}
impl Scan {
    pub(super) fn new(ready: bool) -> Self {
        Self {
            ready,
            write_mode: false,
            pending_write: false,
            backward: false,
            after_row: 0,
            project_source: None,
            outer: None,
            member: None,
        }
    }
    pub(super) fn writer() -> Self {
        Self {
            write_mode: true,
            ..Self::new(false)
        }
    }
    pub(super) fn pending_write(&self) -> bool {
        self.pending_write
    }
    pub(super) fn completed_source(&self) -> i64 {
        self.after_row
    }
    pub(super) fn write_pending(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<(), SidecarError> {
        if !self.write_mode || !self.pending_write {
            return Err(session_error());
        }
        let member = self.member.as_ref().ok_or_else(session_error)?;
        member.source.recheck(connection, usage)?;
        member.verify_bound_chat(connection, usage)?;
        let existing: Option<i64> = connection
            .query_row(
                "SELECT rowid FROM prepared_workspaces WHERE preparation_id=?1",
                [member.key()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        if let Some(rowid) = existing {
            if member.matches_row(connection, rowid, usage)? {
                self.project_source.as_mut().ok_or_else(session_error)?.1 =
                    self.member.take().ok_or_else(session_error)?.into_key();
                self.pending_write = false;
                return Ok(());
            }
        }
        let from = member_from(false);
        let digest = format!("sha256:{}", hex(&member.digest.clone().finalize().into()));
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        // Existing rows were bound to an actual historical source earlier in
        // this same exclusive connection/epoch. Only the state/time fields may
        // advance; identity and creation time are immutable.
        let changed=connection.execute(
            &format!("INSERT INTO prepared_workspaces(preparation_id,project_id,worktree_id,workspace_identity,binding_state,bound_chat_id,canonical_record_json,record_digest,created_at,updated_at,materialized_at) \
             SELECT j.key,s.aggregate_id,json_extract(j.value,'$.manifest.worktreeId'),json_extract(j.value,'$.manifest.workspaceIdentity'),json_extract(j.value,'$.bindingState'),json_extract(j.value,'$.boundChatId'),j.value,?3,json_extract(j.value,'$.createdAt'),json_extract(j.value,'$.updatedAt'),?4 {from} \
             ON CONFLICT(preparation_id) DO UPDATE SET binding_state=excluded.binding_state,bound_chat_id=excluded.bound_chat_id,canonical_record_json=excluded.canonical_record_json,record_digest=excluded.record_digest,updated_at=excluded.updated_at,materialized_at=excluded.materialized_at \
             WHERE prepared_workspaces.project_id=excluded.project_id AND prepared_workspaces.worktree_id=excluded.worktree_id AND prepared_workspaces.workspace_identity=excluded.workspace_identity AND prepared_workspaces.created_at=excluded.created_at AND prepared_workspaces.updated_at<=excluded.updated_at"),
            params![member.source.identity().source_rowid,member.key,digest,now],
        ).map_err(|_|session_error())?;
        usage.work_units += 1;
        if changed != 1 {
            return Err(session_error());
        }
        self.project_source.as_mut().ok_or_else(session_error)?.1 =
            self.member.take().ok_or_else(session_error)?.into_key();
        self.pending_write = false;
        Ok(())
    }
    pub(super) fn retained_bytes(&self) -> Result<usize, SidecarError> {
        let source = self.project_source.as_ref().map_or(0, |(_, key)| key.len());
        let historical = self
            .outer
            .as_ref()
            .map_or(0, |o| o.preparation.len() + o.project.len());
        source
            .checked_add(historical)
            .and_then(|n| {
                n.checked_add(
                    self.member
                        .as_ref()
                        .map_or(Ok(0), Member::retained_bytes)
                        .ok()?,
                )
            })
            .ok_or_else(session_error)
    }
    pub(super) fn step(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Disposition, SidecarError> {
        if cancelled() {
            return Ok(Disposition::Cancelled);
        }
        if self.pending_write {
            return Ok(Disposition::Pending);
        }
        if let Some(member) = self.member.as_mut() {
            let result = member.scan(connection, usage)?;
            if !matches!(result, Disposition::Complete) {
                return Ok(result);
            }
            member.verify_bound_chat(connection, usage)?;
            if self.write_mode {
                self.pending_write = true;
                return Ok(Disposition::Pending);
            }
            if let Some(outer) = self.outer.take() {
                if !member.matches_row(connection, outer.rowid, usage)? {
                    return Err(session_error());
                }
                self.after_row = outer.rowid;
                self.member = None;
            } else {
                if self.ready {
                    let rowid: Option<i64> = connection
                        .query_row(
                            "SELECT rowid FROM prepared_workspaces WHERE preparation_id=?1",
                            [member.key()],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(|_| session_error())?;
                    usage.work_units += 1;
                    if !member.matches_row(connection, rowid.ok_or_else(session_error)?, usage)? {
                        return Err(session_error());
                    }
                }
                self.project_source.as_mut().ok_or_else(session_error)?.1 =
                    self.member.take().ok_or_else(session_error)?.into_key();
            }
            return Ok(Disposition::Pending);
        }
        if self.backward {
            return self.backward_step(connection, usage);
        }
        if let Some((rowid, after)) = self.project_source.as_ref() {
            let size:Option<i64>=connection.query_row(
                "SELECT length(CAST(j.key AS BLOB)) FROM events s JOIN json_each(s.canonical_json_payload,'$.prepared_workspaces') j WHERE s.rowid=?1 AND j.key>?2 ORDER BY j.key LIMIT 1",
                params![rowid,after],|r|r.get(0),
            ).optional().map_err(|_|session_error())?;
            usage.work_units += 1;
            let Some(size) = size else {
                self.after_row = *rowid;
                self.project_source = None;
                return Ok(Disposition::Pending);
            };
            if !(1..=512).contains(&size)
                || usage
                    .encoded_bytes()
                    .checked_add(size as usize)
                    .is_none_or(|n| n > INPUT_BYTES)
            {
                return Err(session_error());
            }
            let key:String=connection.query_row(
                "SELECT j.key FROM events s JOIN json_each(s.canonical_json_payload,'$.prepared_workspaces') j WHERE s.rowid=?1 AND j.key>?2 ORDER BY j.key LIMIT 1",
                params![rowid,after],|r|r.get(0),
            ).map_err(|_|session_error())?;
            usage.work_units += 1;
            usage.metadata_bytes += key.len();
            if key.len() != size as usize {
                return Err(session_error());
            }
            let identity =
                SourceIdentity::load(connection, RawColumn::EventPayload, *rowid, usage)?;
            self.member = Some(Member::start(connection, identity, false, key, usage)?);
            return Ok(Disposition::Pending);
        }
        let row:Option<(i64,bool)>=connection.query_row(
            "SELECT s.rowid,(json_type(s.canonical_json_payload,'$.canonical_path')='text' AND length(CAST(json_extract(s.canonical_json_payload,'$.canonical_path') AS BLOB))>0 AND (json_type(s.canonical_json_payload,'$.prepared_workspaces') IS NULL OR json_type(s.canonical_json_payload,'$.prepared_workspaces')='object')) FROM events s WHERE s.rowid>?1 AND s.aggregate_type='project' AND s.rowid=(SELECT rowid FROM events WHERE aggregate_type='project' AND aggregate_id=s.aggregate_id ORDER BY aggregate_sequence DESC LIMIT 1) ORDER BY s.rowid LIMIT 1",
            [self.after_row],|r|Ok((r.get(0)?,r.get(1)?)),
        ).optional().map_err(|_|session_error())?;
        usage.work_units += 1;
        match row {
            Some((rowid, true)) => self.project_source = Some((rowid, String::new())),
            Some((_, false)) => return Err(session_error()),
            None => {
                if self.write_mode {
                    return Ok(Disposition::Complete);
                }
                self.backward = true;
                self.after_row = 0;
            }
        }
        Ok(Disposition::Pending)
    }
    fn backward_step(
        &mut self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<Disposition, SidecarError> {
        if let Some(outer) = self.outer.as_mut() {
            let (table, rowid, payload) = source_parts(outer.frozen);
            let latest = if self.ready {
                format!(" AND s.{rowid}=(SELECT rowid FROM events WHERE aggregate_type='project' AND aggregate_id=?1 ORDER BY aggregate_sequence DESC LIMIT 1)")
            } else {
                String::new()
            };
            let candidate:Option<(i64,i64,bool)>=connection.query_row(
                &format!("SELECT s.{rowid},s.aggregate_sequence,EXISTS(SELECT 1 FROM json_each(s.{payload},'$.prepared_workspaces') j JOIN prepared_workspaces w ON w.rowid=?3 WHERE j.key=w.preparation_id AND CAST(j.value AS BLOB)=CAST(w.canonical_record_json AS BLOB)) FROM {table} s WHERE s.aggregate_type='project' AND s.aggregate_id=?1 AND s.aggregate_sequence>?2 {latest} ORDER BY s.aggregate_sequence LIMIT 1"),
                params![outer.project,outer.sequence,outer.rowid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional().map_err(|_|session_error())?;
            usage.work_units += 1;
            match candidate {
                Some((rowid, sequence, matched)) => {
                    outer.sequence = sequence;
                    if matched {
                        let identity = SourceIdentity::load(
                            connection,
                            if outer.frozen {
                                RawColumn::FrozenPayload
                            } else {
                                RawColumn::EventPayload
                            },
                            rowid,
                            usage,
                        )?;
                        if usage
                            .encoded_bytes()
                            .checked_add(outer.preparation.len())
                            .is_none_or(|n| n > INPUT_BYTES)
                        {
                            return Err(session_error());
                        }
                        usage.metadata_bytes += outer.preparation.len();
                        self.member = Some(Member::start(
                            connection,
                            identity,
                            outer.frozen,
                            outer.preparation.clone(),
                            usage,
                        )?);
                    }
                }
                None if !outer.frozen && !self.ready => {
                    outer.frozen = true;
                    outer.sequence = 0;
                }
                None => return Err(session_error()),
            }
            return Ok(Disposition::Pending);
        }
        let row:Option<(i64,i64,i64)>=connection.query_row(
            "SELECT rowid,CASE WHEN typeof(preparation_id)='text' THEN length(CAST(preparation_id AS BLOB)) ELSE -1 END,CASE WHEN typeof(project_id)='text' THEN length(CAST(project_id AS BLOB)) ELSE -1 END FROM prepared_workspaces WHERE rowid>?1 ORDER BY rowid LIMIT 1",
            [self.after_row],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional().map_err(|_|session_error())?;
        usage.work_units += 1;
        let Some((rowid, preparation_len, project_len)) = row else {
            return Ok(Disposition::Complete);
        };
        if !(1..=512).contains(&preparation_len) || !(1..=512).contains(&project_len) {
            return Err(session_error());
        }
        if usage
            .encoded_bytes()
            .checked_add((preparation_len + project_len) as usize)
            .is_none_or(|n| n > INPUT_BYTES)
        {
            return Err(session_error());
        }
        let (preparation, project): (String, String) = connection
            .query_row(
                "SELECT preparation_id,project_id FROM prepared_workspaces WHERE rowid=?1",
                [rowid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| session_error())?;
        usage.work_units += 1;
        if preparation.len() != preparation_len as usize || project.len() != project_len as usize {
            return Err(session_error());
        }
        usage.metadata_bytes += preparation.len() + project.len();
        self.outer = Some(Historical {
            rowid,
            preparation,
            project,
            sequence: 0,
            frozen: false,
        });
        Ok(Disposition::Pending)
    }
}
