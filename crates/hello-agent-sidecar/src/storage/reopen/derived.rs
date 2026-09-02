//! R3B global derived-record verification. All state is volatile; the enclosing
//! session commits before publishing this owned state, and discards it on error.
use super::*;
mod check;
mod input;
use check::text;
use input::{Budget, Input, Loaded, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DerivedPhase {
    SourceIndexes,
    Receipts,
    Baselines,
    IntroductionIdentity,
    Messages,
    Workspaces,
    Coverage,
}
impl DerivedPhase {
    pub(super) fn next(self) -> Option<Self> {
        match self {
            Self::SourceIndexes => Some(Self::Receipts),
            Self::Receipts => Some(Self::Baselines),
            Self::Baselines => Some(Self::IntroductionIdentity),
            Self::IntroductionIdentity => Some(Self::Messages),
            Self::Messages => Some(Self::Workspaces),
            Self::Workspaces => Some(Self::Coverage),
            Self::Coverage => None,
        }
    }
    fn table(self) -> &'static str {
        match self {
            Self::SourceIndexes => "event_source_index",
            Self::Receipts => "operation_receipts",
            Self::Baselines => "ledger_baselines",
            Self::IntroductionIdentity | Self::Coverage => "events",
            Self::Messages => "chat_messages",
            Self::Workspaces => "prepared_workspaces",
        }
    }
}
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DerivedUsage {
    pub(crate) rows: usize,
    pub(crate) work_units: usize,
    pub(crate) source_candidates: usize,
    pub(crate) members: usize,
    pub(crate) canonical_source_bytes: usize,
    pub(crate) encoded_closure_bytes: usize,
    pub(crate) retained_bytes: usize,
}
impl From<BatchUsage> for DerivedUsage {
    fn from(usage: BatchUsage) -> Self {
        Self {
            rows: usage.rows,
            work_units: usage.rows,
            canonical_source_bytes: usage.canonical_source_bytes,
            encoded_closure_bytes: usage.encoded_closure_bytes,
            ..Default::default()
        }
    }
}

pub(super) struct Progress {
    pub(super) phase: DerivedPhase,
    after: u64,
    outer: Option<Outer>,
}
impl Progress {
    pub(super) fn new() -> Self {
        Self {
            phase: DerivedPhase::SourceIndexes,
            after: 0,
            outer: None,
        }
    }
    pub(super) fn next_phase(mut self) -> Option<Self> {
        self.phase = self.phase.next()?;
        self.after = 0;
        self.outer = None;
        Some(self)
    }
}
struct Outer {
    key: u64,
    bytes: usize,
    kind: OuterKind,
}
enum OuterKind {
    Unique {
        source: projection::SourceFacts,
        after: u64,
    },
    Message {
        row: Value,
        stage: MessageStage,
    },
    Workspace {
        row: Value,
        after: u64,
        frozen: bool,
    },
    Coverage(projection::SourceFacts),
    // One bounded source payload representation, consumed via pop_first. Never
    // retain both canonical source JSON and a second decoded members tree.
    Members {
        project: String,
        members: BTreeMap<String, Value>,
    },
}
enum MessageStage {
    Exact,
    Raw(u64),
}
impl Outer {
    fn new(key: u64, bytes: usize, kind: OuterKind) -> Result<Self, SidecarError> {
        if bytes > MAX_CLOSURE_BYTES {
            return Err(session_error());
        }
        Ok(Self { key, bytes, kind })
    }
}

const LATEST:&str="SELECT rowid FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 ORDER BY aggregate_sequence DESC LIMIT 1";
const CHAT_CANDIDATE:&str="SELECT rowid FROM events WHERE aggregate_type=?1 AND aggregate_id=?2 AND aggregate_sequence>?3 ORDER BY aggregate_sequence LIMIT 1";
const FROZEN_CANDIDATE:&str="SELECT source_event_rowid FROM frozen_operation_sources WHERE aggregate_type=?1 AND aggregate_id=?2 AND aggregate_sequence>?3 ORDER BY aggregate_sequence LIMIT 1";
const INDEXED_INTRO: &str =
    "SELECT event_rowid FROM event_source_index WHERE introduced_message_id=?1 LIMIT 1";
const FROZEN_INTRO: &str =
    "SELECT source_event_rowid FROM frozen_message_sources WHERE message_id=?1 LIMIT 1";
fn key(
    connection: &Connection,
    query: &str,
    parameters: impl rusqlite::Params,
) -> Result<Option<u64>, SidecarError> {
    connection
        .query_row(query, parameters, |r| row_u64(r, 0))
        .optional()
        .map_err(|_| session_error())
}
fn next(connection: &Connection, table: &str, after: u64) -> Result<Option<u64>, SidecarError> {
    key(
        connection,
        &format!("SELECT rowid FROM {table} WHERE rowid>?1 ORDER BY rowid LIMIT 1"),
        [sql_i64(after)?],
    )
}
fn latest(connection: &Connection, aggregate: &str, id: &str) -> Result<Option<u64>, SidecarError> {
    key(connection, LATEST, params![aggregate, id])
}
fn by_id(
    connection: &Connection,
    table: Table,
    column: &str,
    id: &str,
) -> Result<Option<u64>, SidecarError> {
    key(
        connection,
        &format!(
            "SELECT rowid FROM {} WHERE {column}=?1 LIMIT 1",
            table.name()
        ),
        [id],
    )
}
fn tuple_query(table: Table, frozen: bool) -> String {
    let (source, column) = if frozen {
        ("frozen_operation_sources", "source_event_rowid")
    } else {
        ("events", "rowid")
    };
    let sequence = if table == Table::Baseline {
        "cutoff_aggregate_sequence"
    } else {
        "aggregate_sequence"
    };
    format!("SELECT s.{column} FROM {} d JOIN {source} s ON s.aggregate_type=d.aggregate_type AND s.aggregate_id=d.aggregate_id AND s.aggregate_sequence=d.{sequence} WHERE d.rowid=?1 LIMIT 1",table.name())
}
fn tuple_source(
    connection: &Connection,
    table: Table,
    outer: u64,
    frozen: bool,
) -> Result<Option<u64>, SidecarError> {
    key(connection, &tuple_query(table, frozen), [sql_i64(outer)?])
}
fn event_from(loaded: Loaded, table: Table, cutoff: u64) -> Result<LedgerEventV1, SidecarError> {
    let row = loaded.record()?;
    if table == Table::Frozen {
        check::frozen(row, cutoff)
    } else {
        Ok(projection::event(&row)?.1)
    }
}
fn exact_identity(
    connection: &Connection,
    source: &projection::SourceFacts,
) -> Result<(), SidecarError> {
    let Some(id) = source.introduced.as_deref() else {
        return Ok(());
    };
    for query in [INDEXED_INTRO, FROZEN_INTRO] {
        if key(connection, query, [id])?.is_some_and(|other| other != source.rowid) {
            return Err(session_error());
        }
    }
    Ok(())
}
fn bound_key(connection: &Connection, member: &Value) -> Result<Option<u64>, SidecarError> {
    match member.get("boundChatId").and_then(Value::as_str) {
        Some(id) => Ok(Some(
            latest(connection, "chat", id)?.ok_or_else(session_error)?,
        )),
        None => Ok(None),
    }
}

pub(super) fn reject_hidden_rowids(connection: &Connection) -> Result<(), SidecarError> {
    for table in [
        "event_source_index",
        "operation_receipts",
        "ledger_baselines",
        "chat_messages",
        "prepared_workspaces",
    ] {
        if connection
            .query_row(
                &format!("SELECT rowid FROM {table} WHERE rowid<=0 ORDER BY rowid LIMIT 1"),
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

enum Advance {
    Done,
    Keep,
    Yield,
}
fn advance_outer(
    connection: &Connection,
    outer: &mut Outer,
    budget: &mut Budget,
    cutoff: u64,
    ready: bool,
) -> Result<Advance, SidecarError> {
    match &mut outer.kind {
        OuterKind::Unique { source, after } => {
            let Some(candidate) = next(connection, "events", *after)? else {
                return Ok(Advance::Done);
            };
            let Some(mut loaded) = budget.read(
                connection,
                &[Input::Source(candidate, true)],
                outer.bytes,
                cutoff,
                ready,
            )?
            else {
                return Ok(Advance::Yield);
            };
            let candidate = loaded.remove(0).source()?;
            budget.unit(false, true, false);
            if candidate.rowid != source.rowid && candidate.introduced == source.introduced {
                return Err(session_error());
            }
            *after = candidate.rowid;
            Ok(Advance::Keep)
        }
        OuterKind::Message { row, stage } => {
            if matches!(stage, MessageStage::Exact) {
                let indexed = key(connection, INDEXED_INTRO, [text(row, "message_id")?])?;
                let frozen = by_id(
                    connection,
                    Table::FrozenMessage,
                    "message_id",
                    text(row, "message_id")?,
                )?;
                if indexed.is_some() && frozen.is_some() {
                    return Err(session_error());
                }
                if let Some(id) = indexed {
                    let Some(mut loaded) = budget.read(
                        connection,
                        &[Input::Source(id, false)],
                        outer.bytes,
                        cutoff,
                        ready,
                    )?
                    else {
                        return Ok(Advance::Yield);
                    };
                    check::message(row, &loaded.remove(0).source()?)?;
                    return Ok(Advance::Done);
                }
                if let Some(id) = frozen {
                    let Some(mut loaded) = budget.read(
                        connection,
                        &[Input::Record(Table::FrozenMessage, id, false)],
                        outer.bytes,
                        cutoff,
                        ready,
                    )?
                    else {
                        return Ok(Advance::Yield);
                    };
                    check::frozen_message(row, &loaded.remove(0).record()?)?;
                    return Ok(Advance::Done);
                }
                *stage = MessageStage::Raw(0);
            }
            let MessageStage::Raw(after) = stage else {
                return Err(session_error());
            };
            let candidate = key(
                connection,
                CHAT_CANDIDATE,
                params!["chat", text(row, "chat_id")?, sql_i64(*after)?],
            )?
            .ok_or_else(session_error)?;
            let Some(mut loaded) = budget.read(
                connection,
                &[Input::Source(candidate, true)],
                outer.bytes,
                cutoff,
                ready,
            )?
            else {
                return Ok(Advance::Yield);
            };
            let source = loaded.remove(0).source()?;
            budget.unit(false, true, false);
            if source.introduced.as_deref() == Some(text(row, "message_id")?) {
                check::message(row, &source)?;
                return Ok(Advance::Done);
            }
            *after = source.event.aggregate_sequence;
            Ok(Advance::Keep)
        }
        OuterKind::Workspace { row, after, frozen } => {
            let member = check::workspace_row(row)?;
            let candidate = if ready {
                latest(connection, "project", text(row, "project_id")?)?
            } else {
                key(
                    connection,
                    if *frozen {
                        FROZEN_CANDIDATE
                    } else {
                        CHAT_CANDIDATE
                    },
                    params!["project", text(row, "project_id")?, sql_i64(*after)?],
                )?
            };
            let Some(candidate) = candidate else {
                if !ready && !*frozen {
                    *frozen = true;
                    *after = 0;
                    return Ok(Advance::Keep);
                }
                return Err(session_error());
            };
            let source_table = if *frozen { Table::Frozen } else { Table::Event };
            let chat = bound_key(connection, &member)?;
            let mut inputs = vec![Input::Record(source_table, candidate, true)];
            if let Some(chat) = chat {
                inputs.push(Input::Record(Table::Event, chat, false));
            }
            let Some(mut loaded) = budget.read(connection, &inputs, outer.bytes, cutoff, ready)?
            else {
                return Ok(Advance::Yield);
            };
            let source = event_from(loaded.remove(0), source_table, cutoff)?;
            let chat = if loaded.is_empty() {
                None
            } else {
                Some(event_from(loaded.remove(0), Table::Event, cutoff)?)
            };
            budget.unit(false, true, false);
            let payload = check::project_payload(&source, text(row, "project_id")?)?;
            let records = match payload.get("prepared_workspaces") {
                None => None,
                Some(Value::Object(records)) => Some(records),
                _ => return Err(session_error()),
            };
            if records.and_then(|r| r.get(text(row, "preparation_id").ok()?)) == Some(&member) {
                check::bound_chat(&member, chat.as_ref())?;
                return Ok(Advance::Done);
            }
            if ready {
                return Err(session_error());
            }
            *after = source.aggregate_sequence;
            Ok(Advance::Keep)
        }
        OuterKind::Coverage(source) => {
            let is_latest = latest(
                connection,
                &source.event.aggregate_type,
                &source.event.aggregate_id,
            )? == Some(source.rowid);
            let mut inputs = Vec::new();
            let mut checks = Vec::new();
            if ready {
                let receipt = by_id(
                    connection,
                    Table::Receipt,
                    "operation_id",
                    &source.event.operation_id,
                )?
                .ok_or_else(session_error)?;
                inputs.push(Input::Record(Table::Receipt, receipt, false));
                checks.push(Table::Receipt);
                if is_latest && source.event.aggregate_type != "app_instance" {
                    let baseline=key(connection,"SELECT rowid FROM ledger_baselines WHERE aggregate_type=?1 AND aggregate_id=?2 AND cutoff_aggregate_sequence=?3 LIMIT 1",params![source.event.aggregate_type,source.event.aggregate_id,sql_i64(source.event.aggregate_sequence)?])?.ok_or_else(session_error)?;
                    inputs.push(Input::Record(Table::Baseline, baseline, false));
                    checks.push(Table::Baseline);
                }
                if let Some(id) = source.introduced.as_deref() {
                    let message = by_id(connection, Table::Message, "message_id", id)?
                        .ok_or_else(session_error)?;
                    inputs.push(Input::Record(Table::Message, message, false));
                    checks.push(Table::Message);
                }
            }
            let Some(loaded) = budget.read(connection, &inputs, outer.bytes, cutoff, ready)? else {
                return Ok(Advance::Yield);
            };
            for (table, record) in checks.into_iter().zip(loaded) {
                let row = record.record()?;
                match table {
                    Table::Receipt => check::receipt(&row, &source.event)?,
                    Table::Baseline => check::baseline(&row, &source.event)?,
                    Table::Message => check::message(&row, source)?,
                    _ => return Err(session_error()),
                }
            }
            if is_latest && source.event.aggregate_type == "project" {
                let members = check::project_members(&source.event)?;
                let project = source.event.aggregate_id.clone();
                outer.kind = OuterKind::Members { project, members };
                Ok(Advance::Keep)
            } else {
                Ok(Advance::Done)
            }
        }
        OuterKind::Members { project, members } => {
            let Some((id, member)) = members.first_key_value() else {
                return Ok(Advance::Done);
            };
            check::workspace_member(id, project, member)?;
            let workspace = if ready {
                Some(
                    by_id(connection, Table::Workspace, "preparation_id", id)?
                        .ok_or_else(session_error)?,
                )
            } else {
                None
            };
            let chat = bound_key(connection, member)?;
            let mut inputs = Vec::new();
            if let Some(workspace) = workspace {
                inputs.push(Input::Record(Table::Workspace, workspace, false));
            }
            if let Some(chat) = chat {
                inputs.push(Input::Record(Table::Event, chat, false));
            }
            let Some(mut loaded) = budget.read(connection, &inputs, outer.bytes, cutoff, ready)?
            else {
                return Ok(Advance::Yield);
            };
            if workspace.is_some() {
                check::workspace_matches(&loaded.remove(0).record()?, member)?;
            }
            let chat = if loaded.is_empty() {
                None
            } else {
                Some(event_from(loaded.remove(0), Table::Event, cutoff)?)
            };
            check::bound_chat(member, chat.as_ref())?;
            budget.unit(false, false, true);
            members.pop_first();
            Ok(Advance::Keep)
        }
    }
}

pub(super) fn scan_batch(
    connection: &Connection,
    mut progress: Progress,
    cutoff: u64,
    ready: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<(Progress, DerivedUsage, bool)>, SidecarError> {
    if cancelled() {
        return Ok(None);
    }
    let mut budget = Budget::new(progress.outer.as_ref().map_or(0, |outer| outer.bytes))?;
    while budget.usage.work_units < COMPACTION_VERIFY_MAX_ROWS {
        if cancelled() {
            return Ok(None);
        }
        if let Some(outer) = progress.outer.as_mut() {
            match advance_outer(connection, outer, &mut budget, cutoff, ready)? {
                Advance::Done => {
                    progress.after = outer.key;
                    progress.outer = None;
                }
                Advance::Keep => {}
                Advance::Yield => return Ok(Some((progress, budget.usage, false))),
            }
            continue;
        }
        let Some(id) = next(connection, progress.phase.table(), progress.after)? else {
            return Ok(Some((progress, budget.usage, true)));
        };
        match progress.phase {
            DerivedPhase::SourceIndexes
            | DerivedPhase::IntroductionIdentity
            | DerivedPhase::Coverage => {
                let Some(mut loaded) =
                    budget.read(connection, &[Input::Source(id, true)], 0, cutoff, ready)?
                else {
                    return Ok(Some((progress, budget.usage, false)));
                };
                let source = loaded.remove(0).source()?;
                budget.unit(true, false, false);
                let bytes = input::retained_source_bytes(&source)?;
                match progress.phase {
                    DerivedPhase::SourceIndexes => {
                        if !source.indexed {
                            return Err(session_error());
                        }
                        progress.after = id;
                    }
                    DerivedPhase::IntroductionIdentity => {
                        exact_identity(connection, &source)?;
                        if source.introduced.is_some() && !source.indexed {
                            progress.outer = Some(Outer::new(
                                id,
                                bytes,
                                OuterKind::Unique { source, after: 0 },
                            )?);
                        } else {
                            progress.after = id;
                        }
                    }
                    DerivedPhase::Coverage => {
                        progress.outer = Some(Outer::new(id, bytes, OuterKind::Coverage(source))?)
                    }
                    _ => return Err(session_error()),
                }
            }
            DerivedPhase::Receipts | DerivedPhase::Baselines => {
                let table = if progress.phase == DerivedPhase::Receipts {
                    Table::Receipt
                } else {
                    Table::Baseline
                };
                let (source_table, source) =
                    if let Some(source) = tuple_source(connection, table, id, false)? {
                        (Table::Event, source)
                    } else {
                        (
                            Table::Frozen,
                            tuple_source(connection, table, id, true)?.ok_or_else(session_error)?,
                        )
                    };
                let Some(mut loaded) = budget.read(
                    connection,
                    &[
                        Input::Record(table, id, true),
                        Input::Record(source_table, source, false),
                    ],
                    0,
                    cutoff,
                    ready,
                )?
                else {
                    return Ok(Some((progress, budget.usage, false)));
                };
                let row = loaded.remove(0).record()?;
                let source = event_from(loaded.remove(0), source_table, cutoff)?;
                if table == Table::Receipt {
                    check::receipt(&row, &source)?;
                } else {
                    check::baseline(&row, &source)?;
                }
                budget.unit(true, false, false);
                progress.after = id;
            }
            DerivedPhase::Messages | DerivedPhase::Workspaces => {
                let table = if progress.phase == DerivedPhase::Messages {
                    Table::Message
                } else {
                    Table::Workspace
                };
                let Some(mut loaded) = budget.read(
                    connection,
                    &[Input::Record(table, id, true)],
                    0,
                    cutoff,
                    ready,
                )?
                else {
                    return Ok(Some((progress, budget.usage, false)));
                };
                let row = loaded.remove(0).record()?;
                let bytes = input::retained_record_bytes(&row)?;
                let kind = if table == Table::Message {
                    check::message_row(&row)?;
                    OuterKind::Message {
                        row,
                        stage: MessageStage::Exact,
                    }
                } else {
                    check::workspace_row(&row)?;
                    OuterKind::Workspace {
                        row,
                        after: 0,
                        frozen: false,
                    }
                };
                budget.unit(true, false, false);
                progress.outer = Some(Outer::new(id, bytes, kind)?);
            }
        }
    }
    let exhausted = progress.outer.is_none()
        && next(connection, progress.phase.table(), progress.after)?.is_none();
    Ok(Some((progress, budget.usage, exhausted)))
}

#[cfg(test)]
pub(crate) struct DerivedProbe {
    progress: Option<Progress>,
}
#[cfg(test)]
impl DerivedProbe {
    pub(crate) fn new(phase: DerivedPhase) -> Self {
        Self {
            progress: Some(Progress {
                phase,
                after: 0,
                outer: None,
            }),
        }
    }
    pub(crate) fn step(
        &mut self,
        connection: &mut Connection,
        cutoff: u64,
        ready: bool,
    ) -> Result<(DerivedUsage, bool), SidecarError> {
        let progress = self.progress.take().ok_or_else(session_error)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| session_error())?;
        reject_hidden_rowids(&tx)?;
        let (progress, usage, exhausted) =
            scan_batch(&tx, progress, cutoff, ready, &mut || false)?.ok_or_else(session_error)?;
        tx.commit().map_err(|_| session_error())?;
        if !exhausted {
            self.progress = Some(progress);
        }
        Ok((usage, exhausted))
    }
}
#[cfg(test)]
pub(crate) fn queries_for_test() -> Vec<String> {
    let mut queries = vec![
        LATEST.into(),
        CHAT_CANDIDATE.into(),
        FROZEN_CANDIDATE.into(),
        INDEXED_INTRO.into(),
        FROZEN_INTRO.into(),
    ];
    for table in [
        Table::Event,
        Table::Receipt,
        Table::Baseline,
        Table::Message,
        Table::Workspace,
        Table::Frozen,
        Table::FrozenMessage,
    ] {
        queries.push(table.size_query());
        queries.push(table.data_query());
    }
    for table in [
        "event_source_index",
        "operation_receipts",
        "ledger_baselines",
        "chat_messages",
        "prepared_workspaces",
        "events",
    ] {
        queries.push(format!(
            "SELECT rowid FROM {table} WHERE rowid>?1 ORDER BY rowid LIMIT 1"
        ));
        queries.push(format!(
            "SELECT rowid FROM {table} WHERE rowid<=0 ORDER BY rowid LIMIT 1"
        ));
    }
    for table in [Table::Receipt, Table::Baseline] {
        for frozen in [false, true] {
            queries.push(tuple_query(table, frozen));
        }
    }
    for (table, column) in [
        (Table::Receipt, "operation_id"),
        (Table::Message, "message_id"),
        (Table::FrozenMessage, "message_id"),
        (Table::Workspace, "preparation_id"),
    ] {
        queries.push(format!(
            "SELECT rowid FROM {} WHERE {column}=?1 LIMIT 1",
            table.name()
        ));
    }
    queries.push("SELECT rowid FROM ledger_baselines WHERE aggregate_type=?1 AND aggregate_id=?2 AND cutoff_aggregate_sequence=?3 LIMIT 1".into());
    queries
}
