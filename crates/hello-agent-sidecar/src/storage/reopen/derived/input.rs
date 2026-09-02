//! Fixed raw input shapes. Length probes never decode persisted TEXT. Every
//! actual load, including repeated source/predecessor loads, is charged first.
use super::*;
use rusqlite::types::ValueRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Table {
    Event,
    Receipt,
    Baseline,
    Message,
    Workspace,
    Frozen,
    FrozenMessage,
}
impl Table {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Event => "events",
            Self::Receipt => "operation_receipts",
            Self::Baseline => "ledger_baselines",
            Self::Message => "chat_messages",
            Self::Workspace => "prepared_workspaces",
            Self::Frozen => "frozen_operation_sources",
            Self::FrozenMessage => "frozen_message_sources",
        }
    }
    fn key(self) -> &'static str {
        if self == Self::Frozen {
            "source_event_rowid"
        } else {
            "rowid"
        }
    }
    pub(super) fn columns(self) -> &'static str {
        match self {
        Self::Event=>"rowid,event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_json_payload,timestamp",
        Self::Receipt=>"operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,canonical_payload_digest_version,canonical_payload_digest_format,canonical_payload_digest,event_timestamp,materialized_at",
        Self::Baseline=>"baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at",
        Self::Message=>"message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at",
        Self::Workspace=>"preparation_id,project_id,worktree_id,workspace_identity,binding_state,bound_chat_id,canonical_record_json,record_digest,created_at,updated_at,materialized_at",
        Self::Frozen=>"operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at",
        Self::FrozenMessage=>"message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at",
    }
    }
    fn payload(self) -> &'static str {
        match self {
            Self::Event => "canonical_json_payload",
            Self::Receipt | Self::Frozen => "canonical_payload_json",
            Self::Baseline => "canonical_projection_json",
            Self::Message | Self::FrozenMessage => "canonical_message_json",
            Self::Workspace => "canonical_record_json",
        }
    }
    fn query(self, select: &str) -> String {
        format!(
            "SELECT {select} FROM {} WHERE {}=?1 LIMIT 1",
            self.name(),
            self.key()
        )
    }
    pub(super) fn size_query(self) -> String {
        self.query(&format!(
            "{},length(CAST({} AS BLOB))",
            byte_lengths(&self.columns().split(',').collect::<Vec<_>>()),
            self.payload()
        ))
    }
    pub(super) fn data_query(self) -> String {
        self.query(self.columns())
    }
    pub(super) fn cost(
        self,
        connection: &Connection,
        key: u64,
    ) -> Result<(usize, usize), SidecarError> {
        connection
            .query_row(&self.size_query(), [sql_i64(key)?], |row| {
                Ok((
                    row_bytes(row, 1).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    row_bytes(row, 0).map_err(|_| rusqlite::Error::InvalidQuery)?,
                ))
            })
            .map_err(|_| session_error())
    }
    fn load(self, connection: &Connection, key: u64) -> Result<Value, SidecarError> {
        connection
            .query_row(&self.data_query(), [sql_i64(key)?], |row| {
                let mut object = Map::new();
                for (i, name) in self.columns().split(',').enumerate() {
                    let value = match row.get_ref(i)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(n) => json!(n),
                        ValueRef::Text(_) => json!(row.get::<_, String>(i)?),
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    object.insert(name.into(), value);
                }
                Ok(Value::Object(object))
            })
            .map_err(|_| session_error())
    }
}

pub(super) enum Input {
    Record(Table, u64, bool),
    Source(u64, bool),
}
pub(super) enum Loaded {
    Record(Value),
    Source(Box<projection::SourceFacts>),
}
impl Loaded {
    pub(super) fn record(self) -> Result<Value, SidecarError> {
        if let Self::Record(v) = self {
            Ok(v)
        } else {
            Err(session_error())
        }
    }
    pub(super) fn source(self) -> Result<projection::SourceFacts, SidecarError> {
        if let Self::Source(v) = self {
            Ok(*v)
        } else {
            Err(session_error())
        }
    }
}
pub(super) struct Budget {
    pub(super) usage: DerivedUsage,
}
impl Budget {
    pub(super) fn new(retained: usize) -> Result<Self, SidecarError> {
        if retained > MAX_CLOSURE_BYTES {
            return Err(session_error());
        }
        Ok(Self {
            usage: DerivedUsage {
                encoded_closure_bytes: retained,
                retained_bytes: retained,
                ..Default::default()
            },
        })
    }
    pub(super) fn read(
        &mut self,
        connection: &Connection,
        inputs: &[Input],
        retained: usize,
        cutoff: u64,
        ready: bool,
    ) -> Result<Option<Vec<Loaded>>, SidecarError> {
        let mut primary = 0_usize;
        let mut closure = 0_usize;
        for input in inputs {
            let (cost, charged) = match input {
                Input::Record(table, key, primary) => (table.cost(connection, *key)?, *primary),
                Input::Source(key, primary) => {
                    (projection::source_cost(connection, *key)?, *primary)
                }
            };
            if charged {
                if cost.0 > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES {
                    return Err(session_error());
                }
                primary = primary.checked_add(cost.0).ok_or_else(session_error)?;
            }
            closure = closure.checked_add(cost.1).ok_or_else(session_error)?;
        }
        // Fresh-call fit includes the retained outer even when this call already
        // read earlier candidates. Never turn an impossible row into endless yield.
        if primary > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES
            || retained.checked_add(closure).ok_or_else(session_error)? > MAX_CLOSURE_BYTES
        {
            return Err(session_error());
        }
        if primary > COMPACTION_VERIFY_MAX_PAYLOAD_BYTES - self.usage.canonical_source_bytes
            || closure > MAX_CLOSURE_BYTES - self.usage.encoded_closure_bytes
        {
            return Ok(None);
        }
        self.usage.canonical_source_bytes += primary;
        self.usage.encoded_closure_bytes += closure;
        let mut loaded = Vec::with_capacity(inputs.len());
        for input in inputs {
            loaded.push(match input {
                Input::Record(table, key, _) => Loaded::Record(table.load(connection, *key)?),
                Input::Source(key, _) => Loaded::Source(Box::new(projection::load_source(
                    connection, *key, cutoff, ready,
                )?)),
            });
        }
        Ok(Some(loaded))
    }
    pub(super) fn unit(&mut self, outer: bool, candidate: bool, member: bool) {
        self.usage.work_units += 1;
        self.usage.rows += usize::from(outer);
        self.usage.source_candidates += usize::from(candidate);
        self.usage.members += usize::from(member);
    }
}

pub(super) fn retained_record_bytes(value: &Value) -> Result<usize, SidecarError> {
    value
        .as_object()
        .ok_or_else(session_error)?
        .values()
        .try_fold(0_usize, |n, v| {
            n.checked_add(match v {
                Value::String(s) => s.len(),
                Value::Number(n) => n.to_string().len(),
                Value::Null => 0,
                _ => return Err(session_error()),
            })
            .ok_or_else(session_error)
        })
}
pub(super) fn retained_source_bytes(
    source: &projection::SourceFacts,
) -> Result<usize, SidecarError> {
    let event = &source.event;
    [
        source.rowid.to_string().len(),
        event.event_id.len(),
        event.operation_id.len(),
        event.request_id.as_ref().map_or(0, String::len),
        event.aggregate_type.len(),
        event.aggregate_id.len(),
        event.aggregate_sequence.to_string().len(),
        event.event_type.len(),
        event.payload_schema_version.to_string().len(),
        event.canonical_json_payload.len(),
        event.timestamp.len(),
        source.introduced.as_ref().map_or(0, String::len),
    ]
    .into_iter()
    .try_fold(0_usize, |a, b| a.checked_add(b).ok_or_else(session_error))
}
