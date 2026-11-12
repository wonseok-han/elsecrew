//! R2 compaction-local closure. No global baseline/message replay or admission.
use super::*;
use elsecrew_protocol::storage_lifetime_v1::{
    validate_frozen_message_source_v1, FrozenMessageSourceV1, StorageCompactionPrefixV1,
};
use rusqlite::types::ValueRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionPhase {
    Operations,
    Prefixes,
    FrozenMessages,
}

impl CompactionPhase {
    pub(super) fn next(self) -> Self {
        match self {
            Self::Operations => Self::Prefixes,
            Self::Prefixes | Self::FrozenMessages => Self::FrozenMessages,
        }
    }
    fn primary(self) -> (&'static str, &'static str) {
        match self {
            Self::Operations => ("frozen_operation_sources", "source_event_rowid"),
            Self::Prefixes => ("ledger_compaction_prefixes", "rowid"),
            Self::FrozenMessages => ("frozen_message_sources", "rowid"),
        }
    }
    fn primary_role(self) -> Role {
        match self {
            Self::Operations => Role::Source,
            Self::Prefixes => Role::Baseline,
            Self::FrozenMessages => Role::Message,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Source,
    Receipt,
    Live,
    Index,
    Prefix,
    Baseline,
    Predecessor,
    Message,
    Normalized,
    PreviousPrefix,
    Anchor,
    AnchorReceipt,
    AnchorLive,
    AnchorIndex,
}

impl Role {
    fn columns(self) -> &'static str {
        match self {
            Self::Source | Self::Predecessor | Self::Anchor => "operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at",
            Self::Receipt | Self::AnchorReceipt => "operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,canonical_payload_digest_version,canonical_payload_digest_format,canonical_payload_digest,event_timestamp",
            Self::Live | Self::AnchorLive => "event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp",
            Self::Index | Self::AnchorIndex => "event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,introduced_message_id,chat_durable_sequence,chat_snapshot_message_id",
            Self::Prefix | Self::PreviousPrefix => "prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,baseline_projection_digest,created_at,verified_at",
            Self::Baseline => "baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at",
            Self::Message => "message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at",
            Self::Normalized => "message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at",
        }
    }
    fn primary_payload(self) -> &'static str {
        match self {
            Self::Source => "canonical_payload_json",
            Self::Baseline => "canonical_projection_json",
            Self::Message => "canonical_message_json",
            _ => unreachable!("fixed primary role"),
        }
    }
}

struct ReadSpec {
    role: Role,
    alias: &'static str,
    from: String,
    predicate: &'static str,
}

impl ReadSpec {
    fn query(&self, projection: &str) -> String {
        format!(
            "SELECT {projection} FROM {} WHERE {} LIMIT 1;",
            self.from, self.predicate
        )
    }
    fn data_query(&self) -> String {
        self.query(
            &self
                .role
                .columns()
                .split(',')
                .map(|column| format!("{}.{column}", self.alias))
                .collect::<Vec<_>>()
                .join(","),
        )
    }
    fn size_query(&self, primary: Role) -> String {
        // Count every selected field, including numeric text representations
        // conservatively. No decoded persisted field is outside this preflight.
        let columns = self
            .role
            .columns()
            .split(',')
            .map(|column| format!("{}.{column}", self.alias))
            .collect::<Vec<_>>();
        let lengths = byte_lengths(&columns.iter().map(String::as_str).collect::<Vec<_>>());
        let payload = if self.role == primary {
            format!(
                "length(CAST({}.{} AS BLOB))",
                self.alias,
                primary.primary_payload()
            )
        } else {
            "0".into()
        };
        self.query(&format!("{lengths},{payload}"))
    }
}

/// Fixed per-primary graph. Each role has one exact indexed query and is loaded
/// once; shared data is passed to validators, never secretly reloaded by helpers.
/// All potential role loads are preflighted before ANY persisted String/JSON is
/// decoded, so an oversized late row fails rather than partially reading/yielding.
fn read_plan(phase: CompactionPhase) -> Vec<ReadSpec> {
    let (source, predicate) = match phase {
        CompactionPhase::Operations => ("frozen_operation_sources o".to_owned(), "o.source_event_rowid=?1"),
        CompactionPhase::Prefixes => ("ledger_compaction_prefixes p JOIN frozen_operation_sources o ON o.aggregate_type=p.aggregate_type AND o.aggregate_id=p.aggregate_id AND o.aggregate_sequence=p.deleted_through_sequence".to_owned(), "p.rowid=?1"),
        CompactionPhase::FrozenMessages => ("frozen_message_sources m JOIN frozen_operation_sources o ON o.operation_id=m.source_operation_id".to_owned(), "m.rowid=?1"),
    };
    let prefix = if phase == CompactionPhase::Prefixes {
        "ledger_compaction_prefixes p".to_owned()
    } else {
        format!("{source} JOIN ledger_compaction_prefixes p ON p.aggregate_type=o.aggregate_type AND p.aggregate_id=o.aggregate_id AND p.deleted_through_sequence=o.aggregate_sequence")
    };
    let message = if phase == CompactionPhase::FrozenMessages {
        "frozen_message_sources m".to_owned()
    } else {
        format!("{source} JOIN frozen_message_sources m ON m.source_operation_id=o.operation_id")
    };
    let mut plan = vec![
        ReadSpec { role: Role::Source, alias: "o", from: source.clone(), predicate },
        ReadSpec { role: Role::Receipt, alias: "r", from: format!("{source} JOIN operation_receipts r ON r.operation_id=o.operation_id"), predicate },
        ReadSpec { role: Role::Live, alias: "e", from: format!("{source} JOIN events e ON e.rowid=o.source_event_rowid"), predicate },
        ReadSpec { role: Role::Index, alias: "i", from: format!("{source} JOIN event_source_index i ON i.event_rowid=o.source_event_rowid"), predicate },
        ReadSpec { role: Role::Prefix, alias: "p", from: prefix.clone(), predicate },
        ReadSpec { role: Role::Baseline, alias: "b", from: format!("{prefix} JOIN ledger_baselines b ON b.baseline_id=p.baseline_id"), predicate },
        ReadSpec { role: Role::Predecessor, alias: "prev", from: format!("{source} JOIN frozen_operation_sources prev ON o.aggregate_type='chat' AND prev.aggregate_type=o.aggregate_type AND prev.aggregate_id=o.aggregate_id AND prev.aggregate_sequence=o.aggregate_sequence-1"), predicate },
        ReadSpec { role: Role::Message, alias: "m", from: message.clone(), predicate },
        ReadSpec { role: Role::Normalized, alias: "n", from: format!("{message} JOIN chat_messages n ON n.message_id=m.message_id"), predicate },
    ];
    if phase == CompactionPhase::Prefixes {
        let anchor = format!("{prefix} JOIN frozen_operation_sources a ON a.aggregate_type=p.aggregate_type AND a.aggregate_id=p.aggregate_id AND a.aggregate_sequence=p.anchor_aggregate_sequence");
        plan.extend([
            ReadSpec { role: Role::PreviousPrefix, alias: "pp", from: format!("{prefix} JOIN ledger_compaction_prefixes pp ON pp.aggregate_type=p.aggregate_type AND pp.aggregate_id=p.aggregate_id AND pp.deleted_through_sequence=p.deleted_through_sequence-1"), predicate },
            ReadSpec { role: Role::Anchor, alias: "a", from: anchor.clone(), predicate },
            ReadSpec { role: Role::AnchorReceipt, alias: "ar", from: format!("{anchor} JOIN operation_receipts ar ON ar.operation_id=a.operation_id"), predicate },
            ReadSpec { role: Role::AnchorLive, alias: "ae", from: format!("{anchor} JOIN events ae ON ae.rowid=a.source_event_rowid"), predicate },
            ReadSpec { role: Role::AnchorIndex, alias: "ai", from: format!("{anchor} JOIN event_source_index ai ON ai.event_rowid=a.source_event_rowid"), predicate },
        ]);
    }
    plan
}

struct RowClosure {
    values: Vec<(Role, Option<Value>)>,
}
impl RowClosure {
    fn get(&self, role: Role) -> Option<&Value> {
        self.values
            .iter()
            .find(|(key, _)| *key == role)
            .and_then(|(_, value)| value.as_ref())
    }
    fn required(&self, role: Role) -> Result<&Value, SidecarError> {
        self.get(role).ok_or_else(session_error)
    }
}

fn camel_case(column: &str) -> String {
    let mut parts = column.split('_');
    let mut result = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.extend(chars);
        }
    }
    result
}

fn preflight(
    connection: &Connection,
    plan: &[ReadSpec],
    phase: CompactionPhase,
    rowid: u64,
) -> Result<(usize, usize), SidecarError> {
    let mut primary = None;
    let mut closure = 0_usize;
    for spec in plan {
        let size = connection
            .query_row(
                &spec.size_query(phase.primary_role()),
                [sql_i64(rowid)?],
                |row| {
                    Ok((
                        row_bytes(row, 0).map_err(|_| rusqlite::Error::InvalidQuery)?,
                        row_bytes(row, 1).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| session_error())?;
        if let Some((encoded, payload)) = size {
            closure = closure.checked_add(encoded).ok_or_else(session_error)?;
            if spec.role == phase.primary_role() {
                primary = Some(payload);
            }
            if closure > MAX_CLOSURE_BYTES || payload > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES {
                return Err(session_error());
            }
        }
    }
    Ok((primary.ok_or_else(session_error)?, closure))
}

fn load_closure(
    connection: &Connection,
    plan: &[ReadSpec],
    rowid: u64,
) -> Result<RowClosure, SidecarError> {
    let mut values = Vec::with_capacity(plan.len());
    for spec in plan {
        let value = connection
            .query_row(&spec.data_query(), [sql_i64(rowid)?], |row| {
                let mut object = Map::new();
                for (index, column) in spec.role.columns().split(',').enumerate() {
                    let value = match row.get_ref(index)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(value) => json!(value),
                        ValueRef::Text(_) => Value::String(row.get::<_, String>(index)?),
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    let key = if matches!(spec.role, Role::Live | Role::AnchorLive) {
                        column.to_owned()
                    } else {
                        camel_case(column)
                    };
                    object.insert(key, value);
                }
                Ok(Value::Object(object))
            })
            .optional()
            .map_err(|_| session_error())?;
        values.push((spec.role, value));
    }
    Ok(RowClosure { values })
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, SidecarError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(session_error)
}
fn number(value: &Value, field: &str) -> Result<u64, SidecarError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(session_error)
}
fn frozen(value: &Value, cutoff: u64) -> Result<FrozenOperationSourceV1, SidecarError> {
    let source = validate_frozen_operation_source_v1(value).map_err(|_| session_error())?;
    if source.source_event_rowid > cutoff {
        return Err(session_error());
    }
    Ok(source)
}
fn prefix(value: &Value) -> Result<StorageCompactionPrefixV1, SidecarError> {
    validate_storage_compaction_prefix_v1(value).map_err(|_| session_error())
}

fn verify_baseline(
    p: &StorageCompactionPrefixV1,
    source: &FrozenOperationSourceV1,
    baseline: &Value,
) -> Result<(), SidecarError> {
    let identity = format!(
        "{}\0{}\0{}",
        p.aggregate_type, p.aggregate_id, p.deleted_through_sequence
    );
    let expected_id = format!(
        "baseline_{}",
        sha256_file_version_v1(identity.as_bytes()).trim_start_matches("sha256:")
    );
    let payload = string(baseline, "canonicalProjectionJson")?;
    if p.baseline_id != expected_id
        || string(baseline, "baselineId")? != p.baseline_id
        || string(baseline, "aggregateType")? != p.aggregate_type
        || string(baseline, "aggregateId")? != p.aggregate_id
        || number(baseline, "cutoffAggregateSequence")? != p.deleted_through_sequence
        || number(baseline, "payloadSchemaVersion")? != 1
        || string(baseline, "projectionDigest")? != p.baseline_projection_digest
        || sha256_file_version_v1(payload.as_bytes()) != p.baseline_projection_digest
        || source.aggregate_type != p.aggregate_type
        || source.aggregate_id != p.aggregate_id
        || source.aggregate_sequence != p.deleted_through_sequence
        || source.canonical_payload_json != payload
    {
        return Err(session_error());
    }
    Ok(())
}

fn synthetic_index(
    source: &FrozenOperationSourceV1,
    introduced: Option<String>,
) -> Result<Value, SidecarError> {
    let (durable, snapshot) = chat_source_state(&ledger_event_from_frozen_operation(source))?;
    Ok(
        json!({"eventRowid":source.source_event_rowid,"eventId":source.source_event_id,"operationId":source.operation_id,
        "aggregateType":source.aggregate_type,"aggregateId":source.aggregate_id,"aggregateSequence":source.aggregate_sequence,
        "introducedMessageId":introduced,"chatDurableSequence":durable,"chatSnapshotMessageId":snapshot}),
    )
}

/// The protocol validator, not snapshot equality, owns null-or-same semantics.
fn chat_transition(
    source: &FrozenOperationSourceV1,
    previous: Option<&Value>,
    cutoff: u64,
    live_index: Option<&Value>,
) -> Result<Option<Value>, SidecarError> {
    let previous = if source.aggregate_sequence == 1 {
        if previous.is_some() {
            return Err(session_error());
        }
        None
    } else {
        Some(frozen(previous.ok_or_else(session_error)?, cutoff)?)
    };
    let previous_event = previous.as_ref().map(ledger_event_from_frozen_operation);
    let previous_index = previous
        .as_ref()
        .map(|source| synthetic_index(source, None))
        .transpose()?;
    let previous_durable = previous_index
        .as_ref()
        .map(|index| number(index, "chatDurableSequence"))
        .transpose()?
        .unwrap_or(0);
    let event = ledger_event_from_frozen_operation(source);
    let (durable, snapshot) = chat_source_state(&event)?;
    let introduced = if durable == previous_durable.checked_add(1) {
        snapshot
    } else {
        None
    };
    let generated = synthetic_index(source, introduced.clone())?;
    validate_storage_event_source_index_transition_v1(
        source.source_event_rowid,
        live_index.unwrap_or(&generated),
        &event,
        previous_index.as_ref(),
        previous_event.as_ref(),
    )
    .map_err(|_| session_error())?;
    if introduced.is_none() {
        return Ok(None);
    }
    let payload: Value =
        serde_json::from_str(&source.canonical_payload_json).map_err(|_| session_error())?;
    Ok(Some(
        payload
            .get("last_message")
            .cloned()
            .ok_or_else(session_error)?,
    ))
}

fn verify_message(
    source: &FrozenOperationSourceV1,
    expected: &Value,
    stored: &Value,
    normalized: &Value,
) -> Result<(), SidecarError> {
    let stored: FrozenMessageSourceV1 =
        validate_frozen_message_source_v1(stored).map_err(|_| session_error())?;
    let canonical = canonical_json_v1(expected.clone()).map_err(|_| session_error())?;
    let digest = sha256_file_version_v1(canonical.as_bytes());
    if stored.message_id != string(expected, "messageId")?
        || stored.chat_id != source.aggregate_id
        || stored.source_event_rowid != source.source_event_rowid
        || stored.source_event_id != source.source_event_id
        || stored.source_operation_id != source.operation_id
        || stored.source_aggregate_sequence != source.aggregate_sequence
        || stored.durable_sequence != number(expected, "eventSequence")?
        || stored.canonical_message_json != canonical
        || stored.message_digest != digest
        || string(normalized, "messageId")? != stored.message_id
        || string(normalized, "chatId")? != stored.chat_id
        || number(normalized, "eventSequence")? != stored.durable_sequence
        || string(normalized, "canonicalMessageJson")? != canonical
        || string(normalized, "messageDigest")? != digest
        || string(normalized, "createdAt")? != string(expected, "createdAt")?
    {
        return Err(session_error());
    }
    Ok(())
}

fn verify_deleted(
    rows: &RowClosure,
    source: &FrozenOperationSourceV1,
    cutoff: u64,
) -> Result<(), SidecarError> {
    if rows.get(Role::Live).is_some() || rows.get(Role::Index).is_some() {
        return Err(session_error());
    }
    let p = prefix(rows.required(Role::Prefix)?)?;
    verify_baseline(&p, source, rows.required(Role::Baseline)?)?;
    if source.aggregate_type == "chat" {
        match chat_transition(source, rows.get(Role::Predecessor), cutoff, None)? {
            Some(message) => verify_message(
                source,
                &message,
                rows.required(Role::Message)?,
                rows.required(Role::Normalized)?,
            )?,
            None if rows.get(Role::Message).is_some() => return Err(session_error()),
            None => {}
        }
    }
    Ok(())
}

fn verify_operation(
    rows: &RowClosure,
    cutoff: u64,
) -> Result<FrozenOperationSourceV1, SidecarError> {
    let value = rows.required(Role::Source)?;
    let source = frozen(value, cutoff)?;
    let receipt = rows.required(Role::Receipt)?;
    validate_frozen_operation_receipt_binding_v1(value, receipt).map_err(|_| session_error())?;
    match (rows.get(Role::Live), rows.get(Role::Index)) {
        (Some(event), Some(index)) => {
            let event: LedgerEventV1 =
                serde_json::from_value(event.clone()).map_err(|_| session_error())?;
            validate_frozen_operation_source_binding_v1(
                value,
                source.source_event_rowid,
                &event,
                index,
                receipt,
            )
            .map_err(|_| session_error())?;
        }
        (None, None) => verify_deleted(rows, &source, cutoff)?,
        _ => return Err(session_error()),
    }
    Ok(source)
}

fn anchor_matches(p: &StorageCompactionPrefixV1, anchor: &FrozenOperationSourceV1) -> bool {
    anchor.aggregate_type == p.aggregate_type
        && anchor.aggregate_id == p.aggregate_id
        && anchor.aggregate_sequence == p.deleted_through_sequence + 1
        && anchor.aggregate_sequence == p.anchor_aggregate_sequence
        && anchor.source_event_rowid == p.anchor_event_rowid
        && anchor.source_event_id == p.anchor_event_id
        && anchor.operation_id == p.anchor_operation_id
}

fn verify_prefix(
    connection: &Connection,
    rows: &RowClosure,
    cutoff: u64,
) -> Result<(), SidecarError> {
    let source = verify_operation(rows, cutoff)?;
    verify_deleted(rows, &source, cutoff)?;
    let p = prefix(rows.required(Role::Prefix)?)?;
    if p.deleted_through_sequence > 1 {
        let previous = prefix(rows.required(Role::PreviousPrefix)?)?;
        if !anchor_matches(&previous, &source) {
            return Err(session_error());
        }
    }
    let anchor_value = rows.required(Role::Anchor)?;
    let anchor = frozen(anchor_value, cutoff)?;
    if !anchor_matches(&p, &anchor) {
        return Err(session_error());
    }
    let receipt = rows.required(Role::AnchorReceipt)?;
    validate_frozen_operation_receipt_binding_v1(anchor_value, receipt)
        .map_err(|_| session_error())?;
    let newer = connection.query_row(
        "SELECT 1 FROM ledger_compaction_prefixes WHERE aggregate_type=?1 AND aggregate_id=?2 AND deleted_through_sequence>?3 ORDER BY deleted_through_sequence LIMIT 1;",
        params![p.aggregate_type, p.aggregate_id, sql_i64(p.deleted_through_sequence)?], |row| row.get::<_, i64>(0),
    ).optional().map_err(|_| session_error())?.is_some();
    if newer {
        if rows.get(Role::AnchorLive).is_some() || rows.get(Role::AnchorIndex).is_some() {
            return Err(session_error());
        }
    } else {
        let event: LedgerEventV1 = serde_json::from_value(rows.required(Role::AnchorLive)?.clone())
            .map_err(|_| session_error())?;
        let index = rows.required(Role::AnchorIndex)?;
        validate_frozen_operation_source_binding_v1(
            anchor_value,
            anchor.source_event_rowid,
            &event,
            index,
            receipt,
        )
        .map_err(|_| session_error())?;
        if anchor.aggregate_type == "chat" {
            chat_transition(
                &anchor,
                Some(rows.required(Role::Source)?),
                cutoff,
                Some(index),
            )?;
        }
    }
    Ok(())
}

pub(super) fn reject_hidden_rowids(connection: &Connection) -> Result<(), SidecarError> {
    for phase in [
        CompactionPhase::Operations,
        CompactionPhase::Prefixes,
        CompactionPhase::FrozenMessages,
    ] {
        let (table, key) = phase.primary();
        if connection
            .query_row(
                &format!("SELECT {key} FROM {table} WHERE {key}<=0 ORDER BY {key} LIMIT 1;"),
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
    Ok(())
}

pub(super) fn scan_batch(
    connection: &Connection,
    phase: CompactionPhase,
    mut after: u64,
    cutoff: u64,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<(u64, BatchUsage, bool)>, SidecarError> {
    if cancelled() {
        return Ok(None);
    }
    let (table, key) = phase.primary();
    let plan = read_plan(phase);
    let mut statement = connection
        .prepare(&format!(
            "SELECT {key} FROM {table} WHERE {key}>?1 ORDER BY {key} LIMIT 100;"
        ))
        .map_err(|_| session_error())?;
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
        let (primary_bytes, closure_bytes) = preflight(connection, &plan, phase, rowid)?;
        if primary_bytes > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES - usage.canonical_source_bytes
            || closure_bytes > MAX_CLOSURE_BYTES - usage.encoded_closure_bytes
        {
            return Ok(Some((after, usage, false)));
        }
        let closure = load_closure(connection, &plan, rowid)?;
        match phase {
            CompactionPhase::Operations => {
                verify_operation(&closure, cutoff)?;
            }
            CompactionPhase::Prefixes => verify_prefix(connection, &closure, cutoff)?,
            CompactionPhase::FrozenMessages => {
                let source = verify_operation(&closure, cutoff)?;
                verify_deleted(&closure, &source, cutoff)?;
                if source.aggregate_type != "chat"
                    || chat_transition(&source, closure.get(Role::Predecessor), cutoff, None)?
                        .is_none()
                {
                    return Err(session_error());
                }
            }
        }
        after = rowid;
        usage.rows += 1;
        usage.canonical_source_bytes += primary_bytes;
        usage.encoded_closure_bytes += closure_bytes;
    }
    if after == 0 && phase != CompactionPhase::FrozenMessages {
        return Err(session_error());
    }
    let successor = connection
        .query_row(
            &format!("SELECT {key} FROM {table} WHERE {key}>?1 ORDER BY {key} LIMIT 1;"),
            [sql_i64(after)?],
            |row| row_u64(row, 0),
        )
        .optional()
        .map_err(|_| session_error())?;
    Ok(Some((after, usage, successor.is_none())))
}

#[cfg(test)]
pub(crate) fn phase_batch_for_test(
    connection: &mut Connection,
    phase: CompactionPhase,
    after: u64,
    cutoff: u64,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<(u64, BatchUsage, bool)>, SidecarError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| session_error())?;
    let result =
        reject_hidden_rowids(&tx).and_then(|()| scan_batch(&tx, phase, after, cutoff, cancelled));
    match result {
        Ok(value) => {
            tx.commit().map_err(|_| session_error())?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback().map_err(|_| session_error())?;
            Err(error)
        }
    }
}

#[cfg(test)]
pub(crate) fn phase_queries_for_test(phase: CompactionPhase) -> Vec<String> {
    let (table, key) = phase.primary();
    let mut queries = vec![
        format!("SELECT {key} FROM {table} WHERE {key}>?1 ORDER BY {key} LIMIT 100;"),
        format!("SELECT {key} FROM {table} WHERE {key}>?1 ORDER BY {key} LIMIT 1;"),
        format!("SELECT {key} FROM {table} WHERE {key}<=0 ORDER BY {key} LIMIT 1;"),
    ];
    if phase == CompactionPhase::Prefixes {
        queries.push("SELECT 1 FROM ledger_compaction_prefixes WHERE aggregate_type=?1 AND aggregate_id=?2 AND deleted_through_sequence>?3 ORDER BY deleted_through_sequence LIMIT 1;".into());
    }
    for spec in read_plan(phase) {
        queries.push(spec.data_query());
        queries.push(spec.size_query(phase.primary_role()));
    }
    queries
}
