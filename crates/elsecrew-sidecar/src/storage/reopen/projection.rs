//! R3A: bounded snapshot-upsert equivalence, not global derived-record admission.
use super::*;
use rusqlite::types::ValueRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionPhase {
    Ledger,
    AppInstances,
    Projects,
    Chats,
    Runs,
    Locks,
}

impl ProjectionPhase {
    pub(super) fn next(self) -> Option<Self> {
        match self {
            Self::Ledger => Some(Self::AppInstances),
            Self::AppInstances => Some(Self::Projects),
            Self::Projects => Some(Self::Chats),
            Self::Chats => Some(Self::Runs),
            Self::Runs => Some(Self::Locks),
            Self::Locks => None,
        }
    }
    fn table(self) -> &'static str {
        match self {
            Self::Ledger => "events",
            Self::AppInstances => "app_instances",
            Self::Projects => "projects",
            Self::Chats => "chats",
            Self::Runs => "runs",
            Self::Locks => "locks",
        }
    }
    fn aggregate(self) -> &'static str {
        match self {
            Self::AppInstances => "app_instance",
            Self::Projects => "project",
            Self::Chats => "chat",
            Self::Runs => "run",
            Self::Locks => "lock",
            Self::Ledger => unreachable!("ledger is not a projection"),
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::AppInstances => "sidecar_instance_id",
            Self::Projects => "project_id",
            Self::Chats => "chat_id",
            Self::Runs => "run_id",
            Self::Locks => "lock_id",
            Self::Ledger => unreachable!("ledger is not a projection"),
        }
    }
    fn from_aggregate(aggregate: &str) -> Result<Self, SidecarError> {
        match aggregate {
            "app_instance" => Ok(Self::AppInstances),
            "project" => Ok(Self::Projects),
            "chat" => Ok(Self::Chats),
            "run" => Ok(Self::Runs),
            "lock" => Ok(Self::Locks),
            _ => Err(session_error()),
        }
    }
}
const PROJECTIONS: [ProjectionPhase; 5] = [
    ProjectionPhase::AppInstances,
    ProjectionPhase::Projects,
    ProjectionPhase::Chats,
    ProjectionPhase::Runs,
    ProjectionPhase::Locks,
];
const PHASES: [ProjectionPhase; 6] = [
    ProjectionPhase::Ledger,
    ProjectionPhase::AppInstances,
    ProjectionPhase::Projects,
    ProjectionPhase::Chats,
    ProjectionPhase::Runs,
    ProjectionPhase::Locks,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Event,
    Previous,
    Index,
    PreviousIndex,
    Prefix,
    FrozenPrevious,
    Latest,
    Projection(ProjectionPhase),
}
impl Role {
    fn columns(self) -> &'static str {
        match self {
        Self::Event|Self::Previous=>"rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp",
        Self::Index|Self::PreviousIndex=>"event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,introduced_message_id,chat_durable_sequence,chat_snapshot_message_id",
        Self::Prefix=>"prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,anchor_event_rowid,anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,baseline_id,baseline_projection_digest,created_at,verified_at",
        Self::FrozenPrevious=>"operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at",
        Self::Latest=>"rowid",
        Self::Projection(ProjectionPhase::AppInstances)=>"sidecar_instance_id,pid,platform,started_at,heartbeat_at,clean_shutdown_at,last_event_sequence",
        Self::Projection(ProjectionPhase::Projects)=>"project_id,canonical_path,projection_json,last_event_sequence",
        Self::Projection(ProjectionPhase::Chats)=>"chat_id,project_id,projection_json,last_event_sequence",
        Self::Projection(ProjectionPhase::Runs)=>"run_id,chat_id,state,worktree_path,projection_json,last_event_sequence",
        Self::Projection(ProjectionPhase::Locks)=>"lock_id,project_id,writer_run_id,worktree_path,poisoned,recovery_required,evidence_json,projection_json,last_event_sequence",
        Self::Projection(ProjectionPhase::Ledger)=>unreachable!("ledger is not a projection"),
    }
    }
}

// The two fixed plans have at most twelve exact reads. Persisted strings are never
// used to build SQL or decoded before the complete row plan's length preflight.
struct Read {
    role: Role,
    alias: &'static str,
    from: String,
    predicate: &'static str,
}
impl Read {
    fn query(&self, projection: &str) -> String {
        format!(
            "SELECT {projection} FROM {} WHERE {} LIMIT 1",
            self.from, self.predicate
        )
    }
    fn columns(&self) -> Vec<String> {
        self.role
            .columns()
            .split(',')
            .map(|c| format!("{}.{c}", self.alias))
            .collect()
    }
    fn data_query(&self) -> String {
        self.query(&self.columns().join(","))
    }
    fn size_query(&self, primary: Role) -> String {
        let columns = self.columns();
        let bytes = byte_lengths(&columns.iter().map(String::as_str).collect::<Vec<_>>());
        let payload = if self.role != primary {
            "0".into()
        } else {
            match primary {
                Role::Event => format!(
                    "length(CAST({}.canonical_json_payload AS BLOB))",
                    self.alias
                ),
                Role::Projection(ProjectionPhase::AppInstances) => bytes.clone(),
                Role::Projection(_) => {
                    format!("length(CAST({}.projection_json AS BLOB))", self.alias)
                }
                _ => unreachable!("fixed primary role"),
            }
        };
        self.query(&format!("{bytes},{payload}"))
    }
}

fn read_plan(phase: ProjectionPhase) -> Vec<Read> {
    if phase != ProjectionPhase::Ledger {
        let root = format!("{} p", phase.table());
        return vec![Read{role:Role::Projection(phase),alias:"p",from:root.clone(),predicate:"p.rowid=?1"},Read{role:Role::Event,alias:"e",from:format!("{root} JOIN events e ON e.rowid=(SELECT latest.rowid FROM events latest WHERE latest.aggregate_type='{}' AND latest.aggregate_id=p.{} ORDER BY latest.aggregate_sequence DESC LIMIT 1)",phase.aggregate(),phase.id()),predicate:"p.rowid=?1"}];
    }
    let previous="events e JOIN events prev ON prev.aggregate_type=e.aggregate_type AND prev.aggregate_id=e.aggregate_id AND prev.aggregate_sequence=e.aggregate_sequence-1";
    let prefix="events e JOIN ledger_compaction_prefixes prefix ON prefix.prefix_id=(SELECT p.prefix_id FROM ledger_compaction_prefixes p WHERE p.aggregate_type=e.aggregate_type AND p.aggregate_id=e.aggregate_id ORDER BY p.deleted_through_sequence DESC LIMIT 1)";
    let latest="events e JOIN events latest ON latest.rowid=(SELECT l.rowid FROM events l WHERE l.aggregate_type=e.aggregate_type AND l.aggregate_id=e.aggregate_id ORDER BY l.aggregate_sequence DESC LIMIT 1)";
    let mut plan=vec![
        Read{role:Role::Event,alias:"e",from:"events e".into(),predicate:"e.rowid=?1"},
        Read{role:Role::Previous,alias:"prev",from:previous.into(),predicate:"e.rowid=?1"},
        Read{role:Role::Index,alias:"i",from:"events e JOIN event_source_index i ON i.event_rowid=e.rowid".into(),predicate:"e.rowid=?1"},
        Read{role:Role::PreviousIndex,alias:"i",from:format!("{previous} JOIN event_source_index i ON i.event_rowid=prev.rowid"),predicate:"e.rowid=?1"},
        Read{role:Role::Prefix,alias:"prefix",from:prefix.into(),predicate:"e.rowid=?1"},
        Read{role:Role::FrozenPrevious,alias:"f",from:format!("{prefix} JOIN frozen_operation_sources f ON e.aggregate_sequence=prefix.anchor_aggregate_sequence AND f.aggregate_type=e.aggregate_type AND f.aggregate_id=e.aggregate_id AND f.aggregate_sequence=prefix.deleted_through_sequence"),predicate:"e.rowid=?1"},
        Read{role:Role::Latest,alias:"latest",from:latest.into(),predicate:"e.rowid=?1"},
    ];
    for phase in PROJECTIONS {
        plan.push(Read{role:Role::Projection(phase),alias:"p",from:format!("{latest} JOIN {} p ON latest.rowid=e.rowid AND e.aggregate_type='{}' AND p.{}=e.aggregate_id",phase.table(),phase.aggregate(),phase.id()),predicate:"e.rowid=?1"});
    }
    plan
}

struct Row {
    values: Vec<(Role, Option<Value>)>,
}
impl Row {
    fn get(&self, role: Role) -> Option<&Value> {
        self.values
            .iter()
            .find(|(key, _)| *key == role)
            .and_then(|(_, v)| v.as_ref())
    }
    fn required(&self, role: Role) -> Result<&Value, SidecarError> {
        self.get(role).ok_or_else(session_error)
    }
}
fn primary_role(phase: ProjectionPhase) -> Role {
    if phase == ProjectionPhase::Ledger {
        Role::Event
    } else {
        Role::Projection(phase)
    }
}

fn preflight(
    connection: &Connection,
    plan: &[Read],
    phase: ProjectionPhase,
    rowid: u64,
) -> Result<(usize, usize), SidecarError> {
    let mut primary = None;
    let mut bytes = 0_usize;
    for read in plan {
        let size = connection
            .query_row(
                &read.size_query(primary_role(phase)),
                [sql_i64(rowid)?],
                |r| {
                    Ok((
                        row_bytes(r, 0).map_err(|_| rusqlite::Error::InvalidQuery)?,
                        row_bytes(r, 1).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| session_error())?;
        if let Some((size, payload)) = size {
            bytes = bytes.checked_add(size).ok_or_else(session_error)?;
            if bytes > MAX_CLOSURE_BYTES || payload > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES {
                return Err(session_error());
            }
            if read.role == primary_role(phase) {
                primary = Some(payload);
            }
        }
    }
    Ok((primary.ok_or_else(session_error)?, bytes))
}
fn load_row(connection: &Connection, plan: &[Read], rowid: u64) -> Result<Row, SidecarError> {
    let mut values = Vec::with_capacity(plan.len());
    for read in plan {
        let value = connection
            .query_row(&read.data_query(), [sql_i64(rowid)?], |r| {
                let mut value = Map::new();
                for (i, name) in read.role.columns().split(',').enumerate() {
                    let field = match r.get_ref(i)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(n) => json!(n),
                        ValueRef::Text(_) => json!(r.get::<_, String>(i)?),
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    value.insert(name.into(), field);
                }
                Ok(Value::Object(value))
            })
            .optional()
            .map_err(|_| session_error())?;
        values.push((read.role, value));
    }
    Ok(Row { values })
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, SidecarError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(session_error)
}
fn number(value: &Value, key: &str) -> Result<u64, SidecarError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .filter(|n| *n <= elsecrew_protocol::MAX_SEQUENCE)
        .ok_or_else(session_error)
}
pub(super) fn protocol_value(value: &Value) -> Value {
    let mut mapped = Map::new();
    for (key, value) in value.as_object().expect("fixed SQL row") {
        let mut words = key.split('_');
        let mut camel = words.next().unwrap_or_default().to_string();
        for word in words {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                camel.extend(first.to_uppercase());
                camel.extend(chars);
            }
        }
        mapped.insert(camel, value.clone());
    }
    Value::Object(mapped)
}
pub(super) fn event(value: &Value) -> Result<(u64, LedgerEventV1), SidecarError> {
    let rowid = number(value, "rowid")?;
    if rowid == 0 {
        return Err(session_error());
    }
    let mut fields = value.as_object().ok_or_else(session_error)?.clone();
    fields.remove("rowid");
    let event: LedgerEventV1 =
        serde_json::from_value(Value::Object(fields)).map_err(|_| session_error())?;
    validate_event(&event)?;
    Ok((rowid, event))
}
pub(super) fn validate_event(event: &LedgerEventV1) -> Result<Map<String, Value>, SidecarError> {
    classify_operation_v1(&[], event).map_err(|_| session_error())?;
    let payload: Value =
        serde_json::from_str(&event.canonical_json_payload).map_err(|_| session_error())?;
    if canonical_json_v1(payload.clone()).map_err(|_| session_error())?
        != event.canonical_json_payload
    {
        return Err(session_error());
    }
    let payload = payload.as_object().ok_or_else(session_error)?.clone();
    if payload
        .get(aggregate_id_field_local(&event.aggregate_type)?)
        .and_then(Value::as_str)
        != Some(&event.aggregate_id)
    {
        return Err(session_error());
    }
    Ok(payload)
}
fn synthesized_index(
    rowid: u64,
    event: &LedgerEventV1,
    introduced: Option<String>,
) -> Result<Value, SidecarError> {
    let (durable, snapshot) = chat_source_state(event).map_err(|_| session_error())?;
    Ok(
        json!({"eventRowid":rowid,"eventId":event.event_id,"operationId":event.operation_id,"aggregateType":event.aggregate_type,"aggregateId":event.aggregate_id,"aggregateSequence":event.aggregate_sequence,"introducedMessageId":introduced,"chatDurableSequence":durable,"chatSnapshotMessageId":snapshot}),
    )
}

pub(super) struct SourceFacts {
    pub(super) rowid: u64,
    pub(super) event: LedgerEventV1,
    pub(super) introduced: Option<String>,
    pub(super) indexed: bool,
}

// R3B uses precisely the six source/predecessor roles, not projection loads.
// Callers must reserve the complete returned cost in their enclosing transaction
// before load_source; no source Strings are decoded by source_cost.
fn source_plan() -> Vec<Read> {
    read_plan(ProjectionPhase::Ledger)
        .into_iter()
        .take(6)
        .collect()
}
pub(super) fn source_cost(
    connection: &Connection,
    rowid: u64,
) -> Result<(usize, usize), SidecarError> {
    preflight(connection, &source_plan(), ProjectionPhase::Ledger, rowid)
}
pub(super) fn load_source(
    connection: &Connection,
    rowid: u64,
    cutoff: u64,
    require_index: bool,
) -> Result<SourceFacts, SidecarError> {
    source_facts(
        &load_row(connection, &source_plan(), rowid)?,
        cutoff,
        require_index,
    )
}

fn source_facts(row: &Row, cutoff: u64, require_index: bool) -> Result<SourceFacts, SidecarError> {
    let (rowid, current) = event(row.required(Role::Event)?)?;
    let prefix = row
        .get(Role::Prefix)
        .map(|p| {
            validate_storage_compaction_prefix_v1(&protocol_value(p)).map_err(|_| session_error())
        })
        .transpose()?;
    if let Some(p) = &prefix {
        if current.aggregate_type == "app_instance"
            || current.aggregate_sequence <= p.deleted_through_sequence
        {
            return Err(session_error());
        }
    }
    let mut predecessor = row.get(Role::Previous).map(event).transpose()?;
    let mut previous_index = row.get(Role::PreviousIndex).map(protocol_value);
    if let Some((id, previous)) = &predecessor {
        if *id >= rowid || previous.aggregate_sequence + 1 != current.aggregate_sequence {
            return Err(session_error());
        }
        if let Some(index) = &previous_index {
            validate_storage_event_source_index_binding_v1(*id, index, previous)
                .map_err(|_| session_error())?;
        } else if require_index {
            return Err(session_error());
        }
    } else if current.aggregate_sequence != 1 {
        let p = prefix.as_ref().ok_or_else(session_error)?;
        if current.aggregate_sequence != p.anchor_aggregate_sequence
            || rowid != p.anchor_event_rowid
            || current.event_id != p.anchor_event_id
            || current.operation_id != p.anchor_operation_id
        {
            return Err(session_error());
        }
        let frozen = validate_frozen_operation_source_v1(&protocol_value(
            row.required(Role::FrozenPrevious)?,
        ))
        .map_err(|_| session_error())?;
        if frozen.source_event_rowid > cutoff
            || frozen.source_event_rowid >= rowid
            || frozen.aggregate_type != current.aggregate_type
            || frozen.aggregate_id != current.aggregate_id
            || frozen.aggregate_sequence + 1 != current.aggregate_sequence
        {
            return Err(session_error());
        }
        predecessor = Some((
            frozen.source_event_rowid,
            ledger_event_from_frozen_operation(&frozen),
        ));
    }
    if let Some((id, previous)) = &predecessor {
        if previous_index.is_none() {
            previous_index = Some(synthesized_index(*id, previous, None)?);
        }
    }
    let (durable, snapshot) = chat_source_state(&current).map_err(|_| session_error())?;
    let previous_durable = previous_index
        .as_ref()
        .and_then(|p| p["chatDurableSequence"].as_u64())
        .unwrap_or(0);
    let introduced = durable
        .filter(|n| Some(*n) == previous_durable.checked_add(1))
        .and(snapshot);
    let current_index = match row.get(Role::Index) {
        Some(index) => protocol_value(index),
        None if require_index => return Err(session_error()),
        None => synthesized_index(rowid, &current, introduced.clone())?,
    };
    validate_storage_event_source_index_transition_v1(
        rowid,
        &current_index,
        &current,
        previous_index.as_ref(),
        predecessor.as_ref().map(|(_, event)| event),
    )
    .map_err(|_| session_error())?;
    Ok(SourceFacts {
        rowid,
        event: current,
        introduced,
        indexed: row.get(Role::Index).is_some(),
    })
}

fn verify_ledger(row: &Row, cutoff: u64, require_index: bool) -> Result<(), SidecarError> {
    let source = source_facts(row, cutoff, require_index)?;
    if number(row.required(Role::Latest)?, "rowid")? == source.rowid {
        let phase = ProjectionPhase::from_aggregate(&source.event.aggregate_type)?;
        verify_projection(phase, row.required(Role::Projection(phase))?, &source.event)?;
    }
    Ok(())
}

fn sqlite_scalar_equal(indexed: &Value, payload: &Value) -> bool {
    indexed == payload
        || matches!(
            (indexed.as_i64(), payload.as_bool()),
            (Some(0), Some(false)) | (Some(1), Some(true))
        )
}
fn verify_projection(
    phase: ProjectionPhase,
    stored: &Value,
    source: &LedgerEventV1,
) -> Result<(), SidecarError> {
    if source.aggregate_type != phase.aggregate()
        || string(stored, phase.id())? != source.aggregate_id
    {
        return Err(session_error());
    }
    let mut expected = validate_event(source)?;
    expected.insert(
        "last_event_sequence".into(),
        json!(source.aggregate_sequence),
    );
    let sequence = number(stored, "last_event_sequence")?;
    let mut actual = if phase == ProjectionPhase::AppInstances {
        let pid = number(stored, "pid")?;
        if pid == 0 || pid > u32::MAX as u64 {
            return Err(session_error());
        }
        stored.as_object().ok_or_else(session_error)?.clone()
    } else {
        let mut payload: Map<String, Value> =
            serde_json::from_str(string(stored, "projection_json")?)
                .map_err(|_| session_error())?;
        for column in Role::Projection(phase).columns().split(',').filter(|c| {
            !matches!(
                *c,
                "projection_json" | "last_event_sequence" | "evidence_json"
            )
        }) {
            if !sqlite_scalar_equal(&stored[column], payload.get(column).unwrap_or(&Value::Null)) {
                return Err(session_error());
            }
        }
        if phase == ProjectionPhase::Locks {
            let evidence: Value = serde_json::from_str(string(stored, "evidence_json")?)
                .map_err(|_| session_error())?;
            if payload.get("evidence") != Some(&evidence) {
                return Err(session_error());
            }
        }
        payload.insert("last_event_sequence".into(), json!(sequence));
        payload
    };
    actual.insert("last_event_sequence".into(), json!(sequence));
    if actual != expected {
        return Err(session_error());
    }
    Ok(())
}

pub(super) fn compaction_tables_empty(connection: &Connection) -> Result<bool, SidecarError> {
    for table in [
        "frozen_operation_sources",
        "ledger_compaction_prefixes",
        "frozen_message_sources",
        "ledger_compaction_delete_authority",
    ] {
        if connection
            .query_row(&format!("SELECT 1 FROM {table} LIMIT 1"), [], |r| {
                r.get::<_, i64>(0)
            })
            .optional()
            .map_err(|_| session_error())?
            .is_some()
        {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) fn reject_hidden_rowids(connection: &Connection) -> Result<(), SidecarError> {
    for phase in PHASES {
        if connection
            .query_row(
                &format!(
                    "SELECT rowid FROM {} WHERE rowid<=0 ORDER BY rowid LIMIT 1",
                    phase.table()
                ),
                [],
                |r| r.get::<_, i64>(0),
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
    phase: ProjectionPhase,
    mut after: u64,
    cutoff: u64,
    require_index: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<(u64, BatchUsage, bool)>, SidecarError> {
    if cancelled() {
        return Ok(None);
    }
    let plan = read_plan(phase);
    let mut usage = BatchUsage::default();
    let mut statement = connection
        .prepare(&format!(
            "SELECT rowid FROM {} WHERE rowid>?1 ORDER BY rowid LIMIT 100",
            phase.table()
        ))
        .map_err(|_| session_error())?;
    let mut rows = statement
        .query([sql_i64(after)?])
        .map_err(|_| session_error())?;
    while usage.rows < COMPACTION_VERIFY_MAX_ROWS {
        let Some(row) = rows.next().map_err(|_| session_error())? else {
            break;
        };
        if cancelled() {
            return Ok(None);
        }
        let rowid = row_u64(row, 0).map_err(|_| session_error())?;
        let (primary, closure) = preflight(connection, &plan, phase, rowid)?;
        if primary > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES - usage.canonical_source_bytes
            || closure > MAX_CLOSURE_BYTES - usage.encoded_closure_bytes
        {
            return Ok(Some((after, usage, false)));
        }
        let row = load_row(connection, &plan, rowid)?;
        if phase == ProjectionPhase::Ledger {
            verify_ledger(&row, cutoff, require_index)?;
        } else {
            let (_, source) = event(row.required(Role::Event)?)?;
            verify_projection(phase, row.required(Role::Projection(phase))?, &source)?;
        }
        after = rowid;
        usage.rows += 1;
        usage.canonical_source_bytes += primary;
        usage.encoded_closure_bytes += closure;
    }
    let successor = connection
        .query_row(
            &format!(
                "SELECT rowid FROM {} WHERE rowid>?1 ORDER BY rowid LIMIT 1",
                phase.table()
            ),
            [sql_i64(after)?],
            |r| row_u64(r, 0),
        )
        .optional()
        .map_err(|_| session_error())?;
    Ok(Some((after, usage, successor.is_none())))
}

#[cfg(test)]
pub(crate) fn projection_batch_for_test(
    connection: &mut Connection,
    phase: ProjectionPhase,
    after: u64,
    cutoff: u64,
    require_index: bool,
) -> Result<(u64, BatchUsage, bool), SidecarError> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| session_error())?;
    reject_hidden_rowids(&tx)?;
    let batch = scan_batch(&tx, phase, after, cutoff, require_index, &mut || false)?
        .ok_or_else(session_error)?;
    tx.commit().map_err(|_| session_error())?;
    Ok(batch)
}
#[cfg(test)]
pub(crate) fn projection_queries_for_test(phase: ProjectionPhase) -> Vec<String> {
    let mut queries = vec![
        format!(
            "SELECT rowid FROM {} WHERE rowid>?1 ORDER BY rowid LIMIT 100",
            phase.table()
        ),
        format!(
            "SELECT rowid FROM {} WHERE rowid>?1 ORDER BY rowid LIMIT 1",
            phase.table()
        ),
        format!(
            "SELECT rowid FROM {} WHERE rowid<=0 ORDER BY rowid LIMIT 1",
            phase.table()
        ),
    ];
    for read in read_plan(phase) {
        queries.push(read.data_query());
        queries.push(read.size_query(primary_role(phase)));
    }
    queries
}
