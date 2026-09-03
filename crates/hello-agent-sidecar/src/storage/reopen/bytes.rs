//! R3C-S1: raw UTF-8 byte length/hash transport only. No JSON, canonical,
//! provenance, replay, or admission proof is produced or consumed here.
use super::{data_version, preflight_schema_and_control_rows, verify_schema_v8};
use crate::SidecarError;
use rusqlite::{types::ValueRef, Connection, TransactionBehavior};
use sha2::{Digest, Sha256};

pub(crate) const FRAGMENT_BYTES: usize = 1024 * 1024;
pub(crate) const INPUT_BYTES: usize = 8 * FRAGMENT_BYTES;

/// All SQL identifiers are selected here, never supplied by a caller.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RawColumn {
    EventPayload,
    FrozenPayload,
    FrozenRecord,
}

/// An expected comparison tuple, NOT an already-verified source capability.
/// The locator borrows rowid/operation_id from this tuple, without another copy.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SourceIdentity {
    pub(crate) source_rowid: i64,
    pub(crate) event_id: String,
    pub(crate) operation_id: String,
    pub(crate) aggregate_type: String,
    pub(crate) aggregate_id: String,
    pub(crate) aggregate_sequence: i64,
    pub(crate) payload_schema_version: i64,
    pub(crate) event_type: String,
    pub(crate) request_id: Option<String>,
    pub(crate) timestamp: String,
}

impl SourceIdentity {
    pub(super) fn bytes(&self) -> Result<usize, SidecarError> {
        [
            self.event_id.as_str(),
            self.operation_id.as_str(),
            self.aggregate_type.as_str(),
            self.aggregate_id.as_str(),
            self.event_type.as_str(),
            self.request_id.as_deref().unwrap_or(""),
            self.timestamp.as_str(),
        ]
        .into_iter()
        .try_fold(0_usize, |n, s| n.checked_add(s.len()).ok_or_else(invalid))
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ByteUsage {
    pub(crate) retained_bytes: usize,
    pub(crate) metadata_bytes: usize,
    pub(crate) fragment_bytes: usize,
    pub(crate) work_units: usize,
}
impl ByteUsage {
    pub(crate) fn encoded_bytes(&self) -> usize {
        // All terms were checked before the corresponding input was read.
        self.retained_bytes + self.metadata_bytes + self.fragment_bytes
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RawColumnStep {
    Pending(ByteUsage),
    Cancelled,
    BytesComplete {
        byte_length: u64,
        sha256: [u8; 32],
        usage: ByteUsage,
    },
}

struct Progress {
    bound: BoundColumn,
    after: u64,
    hash: Sha256,
    carry: Utf8Carry,
}

/// Concrete internal read binding shared by S1 transport and S2 scanning.
/// Offsets are session-owned, never a caller resume token or an admission proof.
pub(super) struct BoundColumn {
    expected: SourceIdentity,
    column: RawColumn,
    pub(super) byte_length: u64,
}

impl BoundColumn {
    pub(super) fn bind(
        connection: &Connection,
        column: RawColumn,
        expected: SourceIdentity,
        usage: &mut ByteUsage,
    ) -> Result<Self, SidecarError> {
        let byte_length = metadata(connection, column, &expected, usage)?;
        fragment_size(byte_length, 0, usage.encoded_bytes())?;
        Ok(Self {
            expected,
            column,
            byte_length,
        })
    }

    pub(super) fn retained_bytes(&self) -> Result<usize, SidecarError> {
        self.expected.bytes()
    }
    pub(super) fn bind_projection(
        &self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<ProjectionBinding, SidecarError> {
        ProjectionBinding::bind(connection, &self.expected, usage)
    }

    pub(super) fn recheck(
        &self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<(), SidecarError> {
        if metadata(connection, self.column, &self.expected, usage)? != self.byte_length {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn read_span(
        &self,
        connection: &Connection,
        after: u64,
        amount: usize,
        usage: &mut ByteUsage,
    ) -> Result<Vec<u8>, SidecarError> {
        if after
            .checked_add(amount as u64)
            .is_none_or(|end| end > self.byte_length)
            || usage
                .fragment_bytes
                .checked_add(amount)
                .is_none_or(|n| n > FRAGMENT_BYTES)
            || usage
                .encoded_bytes()
                .checked_add(amount)
                .is_none_or(|n| n > INPUT_BYTES)
            || usage.work_units >= 100
        {
            return Err(invalid());
        }
        let fragment = with_locator(
            connection,
            &self.column.slice_query(),
            self.column,
            &self.expected,
            sql_position(after)?,
            i64::try_from(amount).map_err(|_| invalid())?,
            |row| match row.get_ref(0)? {
                // Type and exact length precede the bounded allocation.
                ValueRef::Blob(bytes) if bytes.len() == amount => Ok(bytes.to_vec()),
                _ => Err(rusqlite::Error::InvalidQuery),
            },
        )?;
        usage.fragment_bytes += fragment.len();
        usage.work_units += 1;
        Ok(fragment)
    }
}

/// Borrow pins the connection, excluding same-connection writes; other
/// connections are fenced by data_version under every BEGIN IMMEDIATE.
pub(crate) struct RawColumnSession<'connection> {
    connection: &'connection mut Connection,
    epoch: i64,
    progress: Option<Progress>,
    #[cfg(test)]
    start_usage: ByteUsage,
    #[cfg(test)]
    last_usage: ByteUsage,
}

impl<'connection> RawColumnSession<'connection> {
    pub(crate) fn start(
        connection: &'connection mut Connection,
        column: RawColumn,
        expected: SourceIdentity,
    ) -> Result<Self, SidecarError> {
        if !connection.is_autocommit() || expected.source_rowid <= 0 {
            return Err(invalid());
        }
        let mut usage = ByteUsage {
            retained_bytes: expected.bytes()?,
            ..ByteUsage::default()
        };
        if usage.retained_bytes > INPUT_BYTES {
            return Err(invalid());
        }
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| invalid())?;
        let epoch = data_version(&tx)?;
        // Existing exact-schema bootstrap is separate from source-call usage;
        // it never loads a source column. No bootstrap latency/heap claim.
        preflight_schema_and_control_rows(&tx)?;
        verify_schema_v8(&tx)?;
        let bound = BoundColumn::bind(&tx, column, expected, &mut usage)?;
        tx.commit().map_err(|_| invalid())?;
        if !connection.is_autocommit() {
            return Err(invalid());
        }
        Ok(Self {
            connection,
            epoch,
            progress: Some(Progress {
                bound,
                after: 0,
                hash: Sha256::new(),
                carry: Utf8Carry::default(),
            }),
            #[cfg(test)]
            start_usage: usage,
            #[cfg(test)]
            last_usage: ByteUsage::default(),
        })
    }

    pub(crate) fn step(
        &mut self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<RawColumnStep, SidecarError> {
        // Taking state first makes every failure/unwind/cancel terminal. Local
        // cursor/hash changes become reusable only after successful COMMIT.
        let mut progress = self.progress.take().ok_or_else(invalid)?;
        #[cfg(test)]
        {
            self.last_usage = ByteUsage::default();
        }
        if !self.connection.is_autocommit() {
            return Err(invalid());
        }
        let mut usage = ByteUsage {
            retained_bytes: progress
                .bound
                .retained_bytes()?
                .checked_add(progress.carry.len)
                .ok_or_else(invalid)?,
            ..ByteUsage::default()
        };
        if usage.retained_bytes > INPUT_BYTES {
            return Err(invalid());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| invalid())?;
        let checked = (|| {
            if data_version(&tx)? != self.epoch {
                return Err(invalid());
            }
            if cancelled() {
                return Ok(false);
            }
            progress.bound.recheck(&tx, &mut usage)?;
            let length = progress.bound.byte_length;
            let amount = fragment_size(length, progress.after, usage.encoded_bytes())?;
            let fragment = progress
                .bound
                .read_span(&tx, progress.after, amount, &mut usage)?;
            if cancelled() {
                return Ok(false);
            }
            progress.carry.accept(&fragment)?;
            progress.hash.update(&fragment);
            progress.after = progress
                .after
                .checked_add(amount as u64)
                .ok_or_else(invalid)?;
            // Exact length was rechecked on the same row under this transaction.
            // Reading exactly its remaining bytes establishes EOF, without a
            // second fragment or an empty-drain step.
            if progress.after == length && progress.carry.len != 0 {
                return Err(invalid());
            }
            if cancelled() {
                return Ok(false);
            }
            Ok(true)
        })();
        #[cfg(test)]
        {
            self.last_usage = ByteUsage {
                retained_bytes: usage.retained_bytes,
                metadata_bytes: usage.metadata_bytes,
                fragment_bytes: usage.fragment_bytes,
                work_units: usage.work_units,
            };
        }
        match checked {
            Ok(true) => {
                tx.commit().map_err(|_| invalid())?;
                if !self.connection.is_autocommit() {
                    return Err(invalid());
                }
                if progress.after == progress.bound.byte_length {
                    Ok(RawColumnStep::BytesComplete {
                        byte_length: progress.bound.byte_length,
                        sha256: progress.hash.finalize().into(),
                        usage,
                    })
                } else {
                    self.progress = Some(progress);
                    Ok(RawColumnStep::Pending(usage))
                }
            }
            Ok(false) => {
                tx.rollback().map_err(|_| invalid())?;
                Ok(RawColumnStep::Cancelled)
            }
            Err(error) => {
                tx.rollback().map_err(|_| invalid())?;
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn usage_for_test(&self) -> (&ByteUsage, &ByteUsage, Option<u64>) {
        (
            &self.start_usage,
            &self.last_usage,
            self.progress.as_ref().map(|p| p.after),
        )
    }
    #[cfg(test)]
    pub(crate) fn autocommit_for_test(&self) -> bool {
        self.connection.is_autocommit()
    }
}

fn invalid() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "raw column byte transport is invalid",
    )
}

/// Closed projection address. Source allowlists and S1 transport remain separate.
pub(super) struct ProjectionBinding {
    kind: ProjectionKind,
    id: String,
    pub(super) byte_length: u64,
    pub(super) sequence: i64,
}
enum ProjectionKind {
    Project,
    Chat,
    Run,
    Lock,
}
impl ProjectionBinding {
    pub(super) fn bind(
        connection: &Connection,
        source: &SourceIdentity,
        usage: &mut ByteUsage,
    ) -> Result<Self, SidecarError> {
        let kind = match source.aggregate_type.as_str() {
            "project" => ProjectionKind::Project,
            "chat" => ProjectionKind::Chat,
            "run" => ProjectionKind::Run,
            "lock" => ProjectionKind::Lock,
            _ => return Err(invalid()),
        };
        let mut binding = Self {
            kind,
            id: source.aggregate_id.clone(),
            byte_length: 0,
            sequence: source.aggregate_sequence,
        };
        if binding.sequence <= 0 {
            return Err(invalid());
        }
        binding.byte_length = binding.metadata(connection, usage)?;
        Ok(binding)
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.id.len()
    }
    fn parts(&self) -> (&'static str, &'static str, &'static str) {
        match self.kind {
            ProjectionKind::Project => ("projects", "project_id", "json_extract(projection_json, '$.project_id') IS NOT project_id OR json_extract(projection_json, '$.canonical_path') IS NOT canonical_path"),
            ProjectionKind::Chat => ("chats", "chat_id", "json_extract(projection_json, '$.chat_id') IS NOT chat_id OR json_extract(projection_json, '$.project_id') IS NOT project_id"),
            ProjectionKind::Run => ("runs", "run_id", "json_extract(projection_json, '$.run_id') IS NOT run_id OR json_extract(projection_json, '$.chat_id') IS NOT chat_id OR json_extract(projection_json, '$.state') IS NOT state OR json_extract(projection_json, '$.worktree_path') IS NOT worktree_path"),
            ProjectionKind::Lock => ("locks", "lock_id", "json_extract(projection_json, '$.lock_id') IS NOT lock_id OR json_extract(projection_json, '$.project_id') IS NOT project_id OR json_extract(projection_json, '$.writer_run_id') IS NOT writer_run_id OR json_extract(projection_json, '$.worktree_path') IS NOT worktree_path OR json_extract(projection_json, '$.poisoned') IS NOT poisoned OR json_extract(projection_json, '$.recovery_required') IS NOT recovery_required OR json(json_extract(projection_json, '$.evidence')) IS NOT json(evidence_json)"),
        }
    }
    fn metadata(
        &self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<u64, SidecarError> {
        let (table, id, _) = self.parts();
        let query = format!("SELECT typeof({id})='text' AND typeof(projection_json)='text' AND typeof(last_event_sequence)='integer',length(CAST({id} AS BLOB)),length(CAST(projection_json AS BLOB)),last_event_sequence FROM {table} WHERE {id}=?1");
        let (typed, size, length, sequence): (bool, i64, i64, i64) = connection
            .query_row(&query, [&self.id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(|_| invalid())?;
        usage.work_units += 1;
        let size = usize::try_from(size).map_err(|_| invalid())?;
        let length = u64::try_from(length).map_err(|_| invalid())?;
        if !typed
            || sequence != self.sequence
            || usage
                .encoded_bytes()
                .checked_add(size)
                .is_none_or(|n| n > INPUT_BYTES)
        {
            return Err(invalid());
        }
        let matches: bool = connection
            .query_row(
                &format!("SELECT {id} FROM {table} WHERE {id}=?1"),
                [&self.id],
                |r| match r.get_ref(0)? {
                    ValueRef::Text(bytes) => {
                        usage.metadata_bytes += bytes.len();
                        Ok(bytes == self.id.as_bytes() && bytes.len() == size)
                    }
                    _ => Err(rusqlite::Error::InvalidQuery),
                },
            )
            .map_err(|_| invalid())?;
        usage.work_units += 1;
        if !matches {
            return Err(invalid());
        }
        Ok(length)
    }
    pub(super) fn recheck(
        &self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<(), SidecarError> {
        if self.metadata(connection, usage)? != self.byte_length {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn read_span(
        &self,
        connection: &Connection,
        after: u64,
        amount: usize,
        usage: &mut ByteUsage,
    ) -> Result<Vec<u8>, SidecarError> {
        if after
            .checked_add(amount as u64)
            .is_none_or(|n| n > self.byte_length)
            || usage
                .fragment_bytes
                .checked_add(amount)
                .is_none_or(|n| n > FRAGMENT_BYTES)
            || usage
                .encoded_bytes()
                .checked_add(amount)
                .is_none_or(|n| n > INPUT_BYTES)
            || usage.work_units >= 100
        {
            return Err(invalid());
        }
        let (table, id, _) = self.parts();
        let query = format!("SELECT CASE WHEN length(CAST(projection_json AS BLOB))=0 THEN X'' ELSE substr(CAST(projection_json AS BLOB),?2,?3) END FROM {table} WHERE {id}=?1");
        let bytes = connection
            .query_row(
                &query,
                rusqlite::params![
                    self.id,
                    sql_position(after)?,
                    i64::try_from(amount).map_err(|_| invalid())?
                ],
                |r| match r.get_ref(0)? {
                    ValueRef::Blob(bytes) if bytes.len() == amount => Ok(bytes.to_vec()),
                    _ => Err(rusqlite::Error::InvalidQuery),
                },
            )
            .map_err(|_| invalid())?;
        usage.fragment_bytes += bytes.len();
        usage.work_units += 1;
        Ok(bytes)
    }
    pub(super) fn mirror(
        &self,
        connection: &Connection,
        usage: &mut ByteUsage,
    ) -> Result<(), SidecarError> {
        if usage.work_units >= 100 {
            return Err(invalid());
        }
        let (table, id, predicate) = self.parts();
        // Exact legacy predicates, restricted by the typed primary key. SQLite
        // JSON evaluation is not claimed to have bounded internal allocation.
        let mismatch: bool = connection
            .query_row(
                &format!("SELECT ({predicate}) FROM {table} WHERE {id}=?1"),
                [&self.id],
                |r| r.get(0),
            )
            .map_err(|_| invalid())?;
        usage.work_units += 1;
        if mismatch {
            return Err(invalid());
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn queries_for_test(&self) -> Vec<String> {
        let (table, id, predicate) = self.parts();
        vec![
            format!("SELECT ({predicate}) FROM {table} WHERE {id}=?1"),
            format!(
                "SELECT substr(CAST(projection_json AS BLOB),?2,?3) FROM {table} WHERE {id}=?1"
            ),
            format!("SELECT typeof({id}),length(CAST({id} AS BLOB)),length(CAST(projection_json AS BLOB)),last_event_sequence FROM {table} WHERE {id}=?1"),
            format!("SELECT {id} FROM {table} WHERE {id}=?1"),
        ]
    }
}

fn sql_position(after: u64) -> Result<i64, SidecarError> {
    i64::try_from(after.checked_add(1).ok_or_else(invalid)?).map_err(|_| invalid())
}

fn fragment_size(length: u64, after: u64, used: usize) -> Result<usize, SidecarError> {
    let remaining = length.checked_sub(after).ok_or_else(invalid)?;
    let available = INPUT_BYTES.checked_sub(used).ok_or_else(invalid)?;
    if remaining > 0 && available == 0 {
        return Err(invalid());
    }
    Ok(remaining.min(FRAGMENT_BYTES as u64).min(available as u64) as usize)
}

impl RawColumn {
    fn parts(
        self,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) {
        match self {
            Self::EventPayload => (
                "events",
                "rowid",
                "canonical_json_payload",
                "event_id",
                "timestamp",
            ),
            Self::FrozenPayload => (
                "frozen_operation_sources",
                "source_event_rowid",
                "canonical_payload_json",
                "source_event_id",
                "event_timestamp",
            ),
            Self::FrozenRecord => (
                "frozen_operation_sources",
                "source_event_rowid",
                "canonical_record_json",
                "source_event_id",
                "event_timestamp",
            ),
        }
    }
    fn filter(self) -> &'static str {
        match self {
            Self::EventPayload => "rowid=?1",
            _ => "operation_id=?1",
        }
    }
    fn metadata_queries(self) -> (String, String) {
        let (table, rowid, payload, event, timestamp) = self.parts();
        let texts = [
            event,
            "operation_id",
            "aggregate_type",
            "aggregate_id",
            "event_type",
            "request_id",
            timestamp,
        ];
        let lengths = texts
            .iter()
            .map(|c| format!("coalesce(length(CAST({c} AS BLOB)),0)"))
            .collect::<Vec<_>>()
            .join("+");
        let types = texts
            .iter()
            .map(|c| {
                if *c == "request_id" {
                    "typeof(request_id) IN ('text','null')".into()
                } else {
                    format!("typeof({c})='text'")
                }
            })
            .chain([
                format!("typeof({rowid})='integer'"),
                "typeof(aggregate_sequence)='integer'".into(),
                "typeof(payload_schema_version)='integer'".into(),
                format!("typeof({payload})='text'"),
            ])
            .collect::<Vec<_>>()
            .join(" AND ");
        (format!("SELECT {lengths},({types}),length(CAST({payload} AS BLOB)) FROM {table} WHERE {}", self.filter()),
         format!("SELECT {rowid},{event},operation_id,aggregate_type,aggregate_id,aggregate_sequence,payload_schema_version,event_type,request_id,{timestamp} FROM {table} WHERE {}", self.filter()))
    }
    fn slice_query(self) -> String {
        let (table, _, payload, _, _) = self.parts();
        format!(
            "SELECT CASE WHEN length(CAST({payload} AS BLOB))=0 THEN X'' ELSE substr(CAST({payload} AS BLOB),?2,?3) END FROM {table} WHERE {}",
            self.filter()
        )
    }
    #[cfg(test)]
    pub(crate) fn queries_for_test(self) -> [String; 3] {
        let (preflight, metadata) = self.metadata_queries();
        [preflight, metadata, self.slice_query()]
    }
}

fn with_locator<T>(
    connection: &Connection,
    query: &str,
    column: RawColumn,
    expected: &SourceIdentity,
    position: i64,
    amount: i64,
    read: impl FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<T, SidecarError> {
    let locator: &dyn rusqlite::ToSql = match column {
        RawColumn::EventPayload => &expected.source_rowid,
        _ => &expected.operation_id,
    };
    let mut statement = connection.prepare(query).map_err(|_| invalid())?;
    if statement.parameter_count() == 1 {
        statement.query_row([locator], read)
    } else {
        statement.query_row(rusqlite::params![locator, position, amount], read)
    }
    .map_err(|_| invalid())
}

fn metadata(
    connection: &Connection,
    column: RawColumn,
    expected: &SourceIdentity,
    usage: &mut ByteUsage,
) -> Result<u64, SidecarError> {
    let (preflight, query) = column.metadata_queries();
    let (size, typed, length) =
        with_locator(connection, &preflight, column, expected, 0, 0, |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?;
    usage.work_units += 1;
    let size = usize::try_from(size).map_err(|_| invalid())?;
    let length = u64::try_from(length.ok_or_else(invalid)?).map_err(|_| invalid())?;
    if !typed
        || usage
            .encoded_bytes()
            .checked_add(size)
            .ok_or_else(invalid)?
            > INPUT_BYTES
    {
        return Err(invalid());
    }
    let matched = with_locator(connection, &query, column, expected, 0, 0, |row| {
        let mut matched = row.get::<_, i64>(0)? == expected.source_rowid
            && row.get::<_, i64>(5)? == expected.aggregate_sequence
            && row.get::<_, i64>(6)? == expected.payload_schema_version;
        for (index, expected) in [
            (1, Some(expected.event_id.as_str())),
            (2, Some(expected.operation_id.as_str())),
            (3, Some(expected.aggregate_type.as_str())),
            (4, Some(expected.aggregate_id.as_str())),
            (7, Some(expected.event_type.as_str())),
            (8, expected.request_id.as_deref()),
            (9, Some(expected.timestamp.as_str())),
        ] {
            match row.get_ref(index)? {
                ValueRef::Text(bytes) => {
                    usage.metadata_bytes += bytes.len();
                    matched &= expected.is_some_and(|text| bytes == text.as_bytes());
                }
                ValueRef::Null => matched &= expected.is_none(),
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
        }
        Ok(matched)
    })?;
    usage.work_units += 1;
    if !matched || usage.metadata_bytes != size {
        return Err(invalid());
    }
    Ok(length)
}

#[derive(Default)]
struct Utf8Carry {
    bytes: [u8; 3],
    len: usize,
}
impl Utf8Carry {
    fn accept(&mut self, mut bytes: &[u8]) -> Result<(), SidecarError> {
        if self.len > 0 {
            let width = match self.bytes[0] {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Err(invalid()),
            };
            let take = (width - self.len).min(bytes.len());
            let mut scalar = [0_u8; 4];
            scalar[..self.len].copy_from_slice(&self.bytes[..self.len]);
            scalar[self.len..self.len + take].copy_from_slice(&bytes[..take]);
            let size = self.len + take;
            let decoded = std::str::from_utf8(&scalar[..size]);
            if size < width {
                if decoded.as_ref().is_err_and(|e| e.error_len().is_none()) {
                    self.bytes[..size].copy_from_slice(&scalar[..size]);
                    self.len = size;
                    return Ok(());
                }
                return Err(invalid());
            }
            decoded.map_err(|_| invalid())?;
            self.len = 0;
            bytes = &bytes[take..];
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => Ok(()),
            Err(error) if error.error_len().is_none() => {
                let tail = &bytes[error.valid_up_to()..];
                if tail.len() > 3 {
                    return Err(invalid());
                }
                self.bytes[..tail.len()].copy_from_slice(tail);
                self.len = tail.len();
                Ok(())
            }
            Err(_) => Err(invalid()),
        }
    }
}

#[cfg(test)]
pub(crate) fn arithmetic_for_test(
    length: u64,
    after: u64,
    used: usize,
) -> Result<(i64, usize), SidecarError> {
    Ok((sql_position(after)?, fragment_size(length, after, used)?))
}
