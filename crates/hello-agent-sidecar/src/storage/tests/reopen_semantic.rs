//! S3 tests use whole JSON only as legacy acceptance oracles.
use super::*;
use crate::storage::reopen::bytes::{
    ByteUsage, RawColumn, SourceIdentity, FRAGMENT_BYTES, INPUT_BYTES,
};
use crate::storage::reopen::semantic::{
    number_for_test, ProjectionCompatibilitySession, ProjectionCompatibilityStep,
};

const STAMP: &str = "2026-09-03T01:00:00Z";

#[test]
fn s3_transaction_local_progress_preserves_enclosing_budget_and_data() {
    let (_root, path) = fixture(json!({"array":[1,2,3],"text":"hello"}));
    let mut c = connection(&path);
    let before = snapshot(&c);
    let file = fs::read(&path).unwrap();
    let expected = identity(&c, "project");
    let calls =
        crate::storage::reopen::semantic::transaction_local_for_test(&mut c, expected).unwrap();
    assert!(calls.len() > 4);
    for measured in calls {
        usage(&measured);
        assert!(measured.metadata_bytes >= 19);
        assert!(measured.fragment_bytes >= 23);
        assert!(measured.work_units >= 2);
    }
    assert!(c.is_autocommit());
    assert_eq!(snapshot(&c), before);
    assert_eq!(fs::read(&path).unwrap(), file);
}

fn fixture(probe: Value) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for (aggregate, id, payload) in [
        (
            "project",
            "project_semantic",
            json!({"project_id":"project_semantic","canonical_path":"/fixture/semantic","probe":probe}),
        ),
        (
            "chat",
            "chat_semantic",
            materializable_chat_payload("chat_semantic", "project_semantic", 0, Value::Null),
        ),
        (
            "run",
            "run_semantic",
            json!({"run_id":"run_semantic","chat_id":null,"state":"succeeded","worktree_path":null}),
        ),
        (
            "lock",
            "lock_semantic",
            json!({"lock_id":"lock_semantic","project_id":"project_semantic","writer_run_id":null,"worktree_path":"/fixture/semantic/worktree","poisoned":true,"recovery_required":false,"evidence":{"a":"a","b":2}}),
        ),
    ] {
        assert_eq!(
            storage
                .append_event(&event(
                    &format!("evt_{id}"),
                    &format!("op_{id}"),
                    aggregate,
                    id,
                    1,
                    payload,
                    STAMP
                ))
                .unwrap(),
            AppendOutcome::Committed
        );
    }
    run_bounded_materializer_to_ready(&mut storage);
    verify_projection_replay(&storage.connection).unwrap();
    storage.checkpoint().unwrap();
    drop(storage);
    drop(Storage::open(&path).unwrap());
    (root, path)
}
fn connection(path: &Path) -> Connection {
    let c = Connection::open(path).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=0;")
        .unwrap();
    c
}
fn identity(c: &Connection, aggregate: &str) -> SourceIdentity {
    c.query_row("SELECT rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,payload_schema_version,event_type,request_id,timestamp FROM events WHERE aggregate_type=?1",[aggregate],|r|Ok(SourceIdentity { source_rowid:r.get(0)?,event_id:r.get(1)?,operation_id:r.get(2)?,aggregate_type:r.get(3)?,aggregate_id:r.get(4)?,aggregate_sequence:r.get(5)?,payload_schema_version:r.get(6)?,event_type:r.get(7)?,request_id:r.get(8)?,timestamp:r.get(9)? })).unwrap()
}
fn table(aggregate: &str) -> (&'static str, &'static str) {
    match aggregate {
        "project" => ("projects", "project_id"),
        "chat" => ("chats", "chat_id"),
        "run" => ("runs", "run_id"),
        "lock" => ("locks", "lock_id"),
        _ => panic!("unknown fixture aggregate"),
    }
}
fn projection(c: &Connection, aggregate: &str) -> String {
    let (table, pk) = table(aggregate);
    c.query_row(
        &format!("SELECT projection_json FROM {table} WHERE {pk}=?1"),
        [format!("{aggregate}_semantic")],
        |r| r.get(0),
    )
    .unwrap()
}
fn set_projection(c: &Connection, aggregate: &str, raw: &str) {
    let (table, pk) = table(aggregate);
    assert_eq!(
        c.execute(
            &format!("UPDATE {table} SET projection_json=?1 WHERE {pk}=?2"),
            params![raw, format!("{aggregate}_semantic")]
        )
        .unwrap(),
        1
    );
}
fn member(raw: &str, key: &str, replacement: &str) -> String {
    let value: Value = serde_json::from_str(raw).unwrap();
    let old = format!("\"{key}\":{}", serde_json::to_string(&value[key]).unwrap());
    assert_eq!(raw.matches(&old).count(), 1);
    raw.replacen(&old, replacement, 1)
}
fn usage(u: &ByteUsage) {
    assert!(u.fragment_bytes <= FRAGMENT_BYTES);
    assert!(u.encoded_bytes() <= INPUT_BYTES);
    assert!(u.work_units <= 100);
}
fn observe(
    c: &mut Connection,
    path: &Path,
    aggregate: &str,
    expected: bool,
    limit: usize,
) -> (usize, Vec<&'static str>) {
    observe_column(c, path, aggregate, RawColumn::EventPayload, expected, limit)
}
fn observe_column(
    c: &mut Connection,
    path: &Path,
    aggregate: &str,
    column: RawColumn,
    expected: bool,
    limit: usize,
) -> (usize, Vec<&'static str>) {
    let before = snapshot(c);
    let file = fs::read(path).unwrap();
    let id = identity(c, aggregate);
    let mut calls = 0;
    let mut max_fresh = 0;
    let mut phases = Vec::new();
    let mut compatible = false;
    let mut stopped = false;
    match ProjectionCompatibilitySession::start(c, column, id) {
        Err(e) => {
            assert_eq!(e.kind, "storageSchemaMismatch");
            stopped = true;
        }
        Ok(mut s) => {
            s.limit_for_test(limit);
            usage(s.usage_for_test());
            for _ in 0..100_000 {
                let phase = s.phase_for_test().unwrap();
                if phases.last() != Some(&phase) {
                    phases.push(phase);
                }
                calls += 1;
                match s.step(&mut || false) {
                    Ok(ProjectionCompatibilityStep::Pending(u)) => {
                        usage(&u);
                        max_fresh = max_fresh.max(u.fragment_bytes);
                    }
                    Ok(ProjectionCompatibilityStep::ProjectionCompatible { usage: u }) => {
                        usage(&u);
                        max_fresh = max_fresh.max(u.fragment_bytes);
                        compatible = true;
                        stopped = true;
                        break;
                    }
                    Err(e) => {
                        assert_eq!(e.kind, "storageSchemaMismatch");
                        stopped = true;
                        break;
                    }
                    other => panic!("{other:?}"),
                }
                assert!(s.autocommit_for_test());
            }
            assert!(s.phase_for_test().is_none());
            assert!(s.step(&mut || false).is_err());
            assert!(s.autocommit_for_test());
        }
    }
    assert!(stopped);
    assert_eq!(
        compatible, expected,
        "aggregate={aggregate} limit={limit} phases={phases:?}"
    );
    assert_eq!(snapshot(c), before);
    assert_eq!(fs::read(path).unwrap(), file);
    if compatible {
        let (table, column) = match column {
            RawColumn::EventPayload => ("events", "canonical_json_payload"),
            RawColumn::FrozenPayload => ("frozen_operation_sources", "canonical_payload_json"),
            _ => unreachable!(),
        };
        let length: i64 = c
            .query_row(
                &format!(
                    "SELECT length(CAST({column} AS BLOB)) FROM {table} WHERE aggregate_type=?1"
                ),
                [aggregate],
                |r| r.get(0),
            )
            .unwrap();
        if length > FRAGMENT_BYTES as i64 && limit == 64 * 1024 {
            assert_eq!(max_fresh, FRAGMENT_BYTES);
        }
    }
    (calls, phases)
}

// Raw cells preserve invalid TEXT bytes without pretending they were a BLOB.
type RawCells = Vec<Vec<(u8, SqlValue)>>;
type RawSnapshot = (Vec<SchemaObject>, Vec<(String, RawCells)>);
fn snapshot(c: &Connection) -> RawSnapshot {
    let schema = schema_objects(c).unwrap();
    let tables = schema
        .iter()
        .filter(|(kind, _, _, _)| kind == "table")
        .map(|(_, table, _, _)| {
            let order = if table == "frozen_operation_sources" {
                "operation_id"
            } else {
                "rowid"
            };
            let mut q = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
                .unwrap();
            let columns = q.column_count();
            let rows = q
                .query_map([], |r| {
                    (0..columns)
                        .map(|i| {
                            Ok(match r.get_ref(i)? {
                                rusqlite::types::ValueRef::Null => (0, SqlValue::Null),
                                rusqlite::types::ValueRef::Integer(n) => (1, SqlValue::Integer(n)),
                                rusqlite::types::ValueRef::Real(n) => (2, SqlValue::Real(n)),
                                rusqlite::types::ValueRef::Text(b) => {
                                    (3, SqlValue::Blob(b.to_vec()))
                                }
                                rusqlite::types::ValueRef::Blob(b) => {
                                    (4, SqlValue::Blob(b.to_vec()))
                                }
                            })
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (table.clone(), rows)
        })
        .collect();
    (schema, tables)
}
fn unchanged(c: &Connection, path: &Path, before: &RawSnapshot, file: &[u8]) {
    assert!(c.is_autocommit());
    assert_eq!(&snapshot(c), before);
    assert_eq!(fs::read(path).unwrap(), file);
}
fn alter(c: &Connection, sql: &str, params: impl rusqlite::Params) {
    let triggers = c
        .prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger'")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    c.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    for (name, _) in &triggers {
        c.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
    }
    c.execute(sql, params).unwrap();
    for (_, sql) in triggers {
        c.execute_batch(&sql).unwrap();
    }
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA ignore_check_constraints=OFF;")
        .unwrap();
}
fn source_bytes(c: &Connection, column: RawColumn, raw: &[u8]) {
    let (table, col) = match column {
        RawColumn::EventPayload => ("events", "canonical_json_payload"),
        RawColumn::FrozenPayload => ("frozen_operation_sources", "canonical_payload_json"),
        _ => panic!("not a payload"),
    };
    alter(
        c,
        &format!("UPDATE {table} SET {col}=CAST(?1 AS TEXT) WHERE aggregate_type='project'"),
        [raw],
    );
}
fn freeze_project(path: &Path) {
    let mut storage = Storage::open(path).unwrap();
    let source = load_events(&storage.connection)
        .unwrap()
        .into_iter()
        .find(|e| e.aggregate_type == "project")
        .unwrap();
    insert_exact_v8_frozen_operation(&mut storage, &source, STAMP);
    storage.checkpoint().unwrap();
    drop(storage);
}

#[test]
fn semantic_projection_live_frozen_large_keys_values_and_late_mismatch() {
    let (_root, path) = fixture(json!(1));
    freeze_project(&path);
    let mut c = connection(&path);
    for (key_size, value_size) in [
        (0, FRAGMENT_BYTES + 1),
        (FRAGMENT_BYTES + 3, 3),
        (0, INPUT_BYTES + 1),
    ] {
        let key = format!("{}a", "k".repeat(key_size));
        let probe = json!({key.clone():"é".repeat(value_size/2+1)});
        let mut source: Value = serde_json::from_str(&projection(&c, "project")).unwrap();
        source["probe"] = probe;
        let raw = canonical_json_v1(source.clone()).unwrap();
        let mut projection = source.clone();
        // An overwritten huge extra-looking value is syntax-checked but is not
        // a surviving name/value in serde's final object.
        let normal = canonical_json_v1(projection.clone()).unwrap();
        set_projection(&c, "project", &normal);
        for column in [RawColumn::EventPayload, RawColumn::FrozenPayload] {
            source_bytes(&c, column, raw.as_bytes());
            let (calls, phases) = observe_column(&mut c, &path, "project", column, true, 64 * 1024);
            assert!(calls > 4);
            assert_eq!(phases, ["canonical", "syntax", "meaning", "mirror"]);
            println!("semantic-large: column={column:?} key_bytes={key_size} source_bytes={} calls={calls}",raw.len());
            projection["probe"][&key] =
                json!(format!("{}x", projection["probe"][&key].as_str().unwrap()));
            set_projection(
                &c,
                "project",
                &canonical_json_v1(projection.clone()).unwrap(),
            );
            let (_, phases) = observe_column(&mut c, &path, "project", column, false, 64 * 1024);
            assert_eq!(phases.last(), Some(&"meaning"));
            projection = source.clone();
            set_projection(&c, "project", &normal);
        }
    }
}

#[test]
fn semantic_projection_long_numerals_are_not_subject_to_canonical_token_cap() {
    let (_root, path) = fixture(json!(1.0));
    let mut c = connection(&path);
    let original = projection(&c, "project");
    for raw in [
        format!(
            "1{}e-{}",
            "0".repeat(FRAGMENT_BYTES + 7),
            FRAGMENT_BYTES + 7
        ),
        format!("1.{}", "0".repeat(FRAGMENT_BYTES + 7)),
    ] {
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), json!(1.0));
        set_projection(
            &c,
            "project",
            &member(&original, "probe", &format!("\"probe\":{raw}")),
        );
        let (calls, _) = observe(&mut c, &path, "project", true, 64 * 1024);
        assert!(calls > 4);
    }
    for raw in [
        format!("0.{}1", "0".repeat(FRAGMENT_BYTES + 7)),
        format!("-1e-{}", "9".repeat(FRAGMENT_BYTES + 7)),
    ] {
        assert!(serde_json::from_str::<Value>(&raw).is_ok());
        set_projection(
            &c,
            "project",
            &member(&original, "probe", &format!("\"probe\":{raw}")),
        );
        let (_, phases) = observe(&mut c, &path, "project", false, 64 * 1024);
        assert_eq!(phases.last(), Some(&"meaning"));
    }
}

#[test]
fn semantic_projection_malformed_utf8_source_canonical_and_late_projection_eof() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let original = projection(&c, "project");
    for raw in [
        format!("{original}x").into_bytes(),
        format!("{original}{}x", " ".repeat(FRAGMENT_BYTES + 1)).into_bytes(),
        b"{\"probe\":\"\xff\"}".to_vec(),
    ] {
        alter(
            &c,
            "UPDATE projects SET projection_json=CAST(?1 AS TEXT)",
            [raw.as_slice()],
        );
        let (_, phases) = observe(&mut c, &path, "project", false, 64 * 1024);
        assert_eq!(phases.last(), Some(&"syntax"));
    }
    set_projection(&c, "project", &original);
    for raw in [
        format!(" {original}").into_bytes(),
        b"{\"probe\":\"\xff\"}".to_vec(),
    ] {
        source_bytes(&c, RawColumn::EventPayload, &raw);
        let (_, phases) = observe(&mut c, &path, "project", false, 64 * 1024);
        assert_eq!(phases, ["canonical"]);
    }
}

fn at_phase<'a>(c: &'a mut Connection, phase: &str) -> ProjectionCompatibilitySession<'a> {
    let id = identity(c, "project");
    let mut s = ProjectionCompatibilitySession::start(c, RawColumn::EventPayload, id).unwrap();
    for _ in 0..10_000 {
        if s.phase_for_test() == Some(phase) {
            return s;
        }
        assert!(matches!(
            s.step(&mut || false).unwrap(),
            ProjectionCompatibilityStep::Pending(_)
        ));
    }
    panic!("phase not reached {phase}");
}

#[test]
fn semantic_projection_each_phase_cancel_entry_middle_precommit_discards_progress() {
    let (_root, path) = fixture(json!({"a":[1,2,3],"b":"value","c":{"d":true}}));
    let mut c = connection(&path);
    let before = snapshot(&c);
    let file = fs::read(&path).unwrap();
    for phase in ["canonical", "syntax", "meaning", "mirror"] {
        let mut total = 0usize;
        {
            let mut s = at_phase(&mut c, phase);
            s.step(&mut || {
                total += 1;
                false
            })
            .unwrap();
        }
        assert!(total >= 2);
        for cancel_at in [1, total.div_ceil(2), total] {
            {
                let mut s = at_phase(&mut c, phase);
                let mut polls = 0;
                assert_eq!(
                    s.step(&mut || {
                        polls += 1;
                        polls == cancel_at
                    })
                    .unwrap(),
                    ProjectionCompatibilityStep::Cancelled
                );
                assert_eq!(polls, cancel_at);
                assert!(s.phase_for_test().is_none());
                assert!(s.autocommit_for_test());
                usage(s.usage_for_test());
                assert!(s.step(&mut || false).is_err());
            }
            unchanged(&c, &path, &before, &file);
        }
        drop(at_phase(&mut c, phase));
    }
    observe(&mut c, &path, "project", true, 64 * 1024);
}

#[test]
fn semantic_projection_external_commit_at_every_phase_and_after_terminal() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let other = connection(&path);
    let original = projection(&c, "project");
    for phase in ["canonical", "syntax", "meaning", "mirror"] {
        let before;
        let file;
        {
            let mut s = at_phase(&mut c, phase);
            set_projection(
                &other,
                "project",
                &member(&original, "probe", r#""probe":2"#),
            );
            before = snapshot(&other);
            file = fs::read(&path).unwrap();
            assert_eq!(
                s.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            assert!(s.phase_for_test().is_none());
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
        set_projection(&other, "project", &original);
        let s = at_phase(&mut c, "canonical");
        assert_eq!(s.phase_for_test(), Some("canonical"));
        drop(s);
    }
    let before;
    let file;
    {
        let mut s = at_phase(&mut c, "mirror");
        assert!(matches!(
            s.step(&mut || false).unwrap(),
            ProjectionCompatibilityStep::ProjectionCompatible { .. }
        ));
        set_projection(
            &other,
            "project",
            &member(&original, "probe", r#""probe":2"#),
        );
        before = snapshot(&other);
        file = fs::read(&path).unwrap();
        assert!(s.step(&mut || false).is_err());
    }
    unchanged(&c, &path, &before, &file);
}

struct SqlFault {
    mode: Box<std::sync::atomic::AtomicU8>,
    handle: *mut rusqlite::ffi::sqlite3,
}
impl SqlFault {
    fn install(c: &Connection) -> Self {
        unsafe extern "C" fn authorize(
            context: *mut std::ffi::c_void,
            action: std::ffi::c_int,
            first: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
        ) -> std::ffi::c_int {
            // Stable context until hook removal; SQLite owns NUL-terminated args.
            let mode = unsafe { &*context.cast::<std::sync::atomic::AtomicU8>() }
                .load(std::sync::atomic::Ordering::Relaxed);
            let first = if first.is_null() {
                b"".as_slice()
            } else {
                unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes()
            };
            if mode == 1 && action == rusqlite::ffi::SQLITE_TRANSACTION && first == b"COMMIT"
                || mode == 2
                    && action == rusqlite::ffi::SQLITE_READ
                    && [b"events".as_slice(), b"projects"].contains(&first)
            {
                rusqlite::ffi::SQLITE_DENY
            } else {
                rusqlite::ffi::SQLITE_OK
            }
        }
        let mode = Box::new(std::sync::atomic::AtomicU8::new(0));
        let handle = unsafe { c.handle() };
        unsafe {
            assert_eq!(
                rusqlite::ffi::sqlite3_set_authorizer(
                    handle,
                    Some(authorize),
                    (&*mode as *const std::sync::atomic::AtomicU8)
                        .cast_mut()
                        .cast()
                ),
                rusqlite::ffi::SQLITE_OK
            );
        }
        Self { mode, handle }
    }
    fn set(&self, mode: u8) {
        self.mode.store(mode, std::sync::atomic::Ordering::Relaxed);
    }
}
impl Drop for SqlFault {
    fn drop(&mut self) {
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(self.handle, None, std::ptr::null_mut());
        }
    }
}

#[test]
fn semantic_projection_sql_read_commit_busy_and_nested_transaction_fail_closed() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let other = connection(&path);
    let fault = SqlFault::install(&c);
    let before = snapshot(&c);
    let file = fs::read(&path).unwrap();
    for mode in [1, 2] {
        let id = identity(&c, "project");
        fault.set(mode);
        assert!(
            ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).is_err()
        );
        fault.set(0);
        unchanged(&c, &path, &before, &file);
    }
    for phase in ["canonical", "syntax", "meaning", "mirror"] {
        {
            let mut s = at_phase(&mut c, phase);
            fault.set(1);
            assert_eq!(
                s.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            fault.set(0);
            assert!(s.phase_for_test().is_none());
            assert!(s.autocommit_for_test());
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
        {
            let mut s = at_phase(&mut c, phase);
            fault.set(2);
            assert!(s.step(&mut || false).is_err());
            fault.set(0);
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
        {
            let mut s = at_phase(&mut c, phase);
            other.execute_batch("BEGIN IMMEDIATE").unwrap();
            assert!(s.step(&mut || false).is_err());
            other.execute_batch("ROLLBACK").unwrap();
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
    }
    let id = identity(&c, "project");
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).is_err());
    other.execute_batch("ROLLBACK").unwrap();
    let id = identity(&c, "project");
    c.execute_batch("BEGIN").unwrap();
    assert!(ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).is_err());
    assert!(!c.is_autocommit());
    c.execute_batch("ROLLBACK").unwrap();
    unchanged(&c, &path, &before, &file);
}

#[test]
fn semantic_projection_metadata_budget_utf8_and_exact_primary_index_plans() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    for aggregate in ["project", "chat", "run", "lock"] {
        let id = identity(&c, aggregate);
        let key = id.aggregate_id.clone();
        let queries = {
            let s =
                ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).unwrap();
            s.projection_queries_for_test()
        };
        for sql in queries {
            assert!(sql.contains(" WHERE "));
            assert!(!sql.contains("LIMIT"));
            let mut q = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let values: Vec<SqlValue> = if q.parameter_count() == 1 {
                vec![SqlValue::Text(key.clone())]
            } else {
                vec![
                    SqlValue::Text(key.clone()),
                    SqlValue::Integer(1),
                    SqlValue::Integer(1),
                ]
            };
            let detail = q
                .query_map(rusqlite::params_from_iter(values), |r| {
                    r.get::<_, String>(3)
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
                .join(" ");
            assert!(detail.contains("SEARCH"), "{detail}");
            assert!(
                !detail.contains("SCAN ") && !detail.contains("TEMP B-TREE"),
                "{detail}"
            );
        }
    }
    let base = {
        let id = identity(&c, "project");
        let s = ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).unwrap();
        s.usage_for_test().encoded_bytes()
    };
    assert_eq!(base % 2, 0);
    let padding = (INPUT_BYTES - 128 - base) / 2;
    alter(
        &c,
        "UPDATE events SET timestamp=?1 WHERE aggregate_type='project'",
        [format!("{STAMP}{}", "m".repeat(padding))],
    );
    let before = snapshot(&c);
    let file = fs::read(&path).unwrap();
    let mut maximum = 0;
    let mut calls = 0;
    {
        let id = identity(&c, "project");
        let mut s =
            ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).unwrap();
        assert_eq!(s.usage_for_test().encoded_bytes(), INPUT_BYTES - 128);
        loop {
            calls += 1;
            assert!(calls < 1000);
            match s.step(&mut || false).unwrap() {
                ProjectionCompatibilityStep::Pending(u) => {
                    usage(&u);
                    maximum = maximum.max(u.encoded_bytes());
                }
                ProjectionCompatibilityStep::ProjectionCompatible { usage: u } => {
                    usage(&u);
                    maximum = maximum.max(u.encoded_bytes());
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(s.step(&mut || false).is_err());
    }
    assert_eq!(maximum, INPUT_BYTES);
    unchanged(&c, &path, &before, &file);
    println!("semantic-metadata: exact_encoded={maximum} calls={calls}");
    for (label, padding) in [
        ("exact-no-room", "m".repeat((INPUT_BYTES - base) / 2)),
        ("plus-two", "m".repeat((INPUT_BYTES - base) / 2 + 1)),
        ("multibyte-overflow", "é".repeat((INPUT_BYTES - base) / 2)),
    ] {
        assert!(padding.chars().count() < INPUT_BYTES);
        alter(
            &c,
            "UPDATE events SET timestamp=?1 WHERE aggregate_type='project'",
            [format!("{STAMP}{padding}")],
        );
        let before = snapshot(&c);
        let file = fs::read(&path).unwrap();
        let id = identity(&c, "project");
        assert!(
            ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).is_err(),
            "{label}"
        );
        unchanged(&c, &path, &before, &file);
    }
}

#[test]
fn semantic_projection_nested_rescan_cancel_epoch_commit_failure_and_late_missing_extra() {
    let probe = Value::Object((0..101).map(|i| (format!("key{i:03}"), json!(i))).collect());
    let (_root, path) = fixture(probe);
    let mut c = connection(&path);
    let other = connection(&path);
    let original = projection(&c, "project");
    let (calls, _) = observe(&mut c, &path, "project", true, 64 * 1024);
    assert!(calls > 4);
    println!("semantic-members: 101 members calls={calls}");
    let mut altered: Value = serde_json::from_str(&original).unwrap();
    altered["probe"].as_object_mut().unwrap().remove("key100");
    set_projection(&c, "project", &canonical_json_v1(altered).unwrap());
    observe(&mut c, &path, "project", false, 64 * 1024);
    let mut altered: Value = serde_json::from_str(&original).unwrap();
    altered["probe"]["key101"] = json!(101);
    set_projection(&c, "project", &canonical_json_v1(altered).unwrap());
    observe(&mut c, &path, "project", false, 64 * 1024);
    set_projection(&c, "project", &original);
    fn nested(c: &mut Connection) -> ProjectionCompatibilitySession<'_> {
        let mut s = at_phase(c, "meaning");
        for _ in 0..10_000 {
            let ProjectionCompatibilityStep::Pending(u) = s.step(&mut || false).unwrap() else {
                panic!("expected nested pending");
            };
            usage(&u);
            if s.nested_meaning_for_test() {
                return s;
            }
        }
        panic!("no nested rescan");
    }
    let before = snapshot(&c);
    let file = fs::read(&path).unwrap();
    {
        let mut s = nested(&mut c);
        let mut polls = 0;
        assert_eq!(
            s.step(&mut || {
                polls += 1;
                polls == 5
            })
            .unwrap(),
            ProjectionCompatibilityStep::Cancelled
        );
        assert_eq!(polls, 5);
        assert!(s.step(&mut || false).is_err());
    }
    unchanged(&c, &path, &before, &file);
    {
        let fault = SqlFault::install(&c);
        let mut s = nested(&mut c);
        fault.set(1);
        assert!(s.step(&mut || false).is_err());
        fault.set(0);
        assert!(s.step(&mut || false).is_err());
    }
    unchanged(&c, &path, &before, &file);
    let after_external;
    let bytes_after_external;
    {
        let mut s = nested(&mut c);
        set_projection(&other, "project", &format!(" {original}"));
        after_external = snapshot(&other);
        bytes_after_external = fs::read(&path).unwrap();
        assert!(s.step(&mut || false).is_err());
        assert!(s.step(&mut || false).is_err());
    }
    unchanged(&c, &path, &after_external, &bytes_after_external);
}

#[test]
fn semantic_projection_closed_addresses_and_row_identity_fail_closed() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let before = snapshot(&c);
    let file = fs::read(&path).unwrap();
    let id = identity(&c, "project");
    assert!(ProjectionCompatibilitySession::start(&mut c, RawColumn::FrozenRecord, id).is_err());
    let mut id = identity(&c, "project");
    id.event_id.push('x');
    assert!(ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).is_err());
    unchanged(&c, &path, &before, &file);
    alter(&c, "DELETE FROM projects", []);
    let before = snapshot(&c);
    let file = fs::read(&path).unwrap();
    let id = identity(&c, "project");
    assert!(ProjectionCompatibilitySession::start(&mut c, RawColumn::EventPayload, id).is_err());
    unchanged(&c, &path, &before, &file);
    // §13's valid indexed pid control is not a fifth S3 projection binding.
    // Typed app-instance comparison remains the existing consumer's job.
    let (_app_root, app_path) = database();
    let mut storage = Storage::open(&app_path).unwrap();
    storage.append_event(&event("evt_app","op_app","app_instance","app_semantic",1,json!({"sidecar_instance_id":"app_semantic","pid":1,"platform":"macos","started_at":STAMP,"heartbeat_at":STAMP,"clean_shutdown_at":null}),STAMP)).unwrap();
    storage.checkpoint().unwrap();
    drop(storage);
    let mut app = connection(&app_path);
    let before = snapshot(&app);
    let file = fs::read(&app_path).unwrap();
    let id = identity(&app, "app_instance");
    assert!(ProjectionCompatibilitySession::start(&mut app, RawColumn::EventPayload, id).is_err());
    unchanged(&app, &app_path, &before, &file);
}

#[test]
fn semantic_projection_basic_four_tables_and_split_values() {
    let (_root, path) = fixture(json!({"a":[1,1.0,true,null,"é🦀"],"b":{"nested":false}}));
    let mut c = connection(&path);
    for aggregate in ["project", "chat", "run", "lock"] {
        for limit in [1, 2, 7, 64 * 1024] {
            let (_, phases) = observe(&mut c, &path, aggregate, true, limit);
            assert_eq!(phases, ["canonical", "syntax", "meaning", "mirror"]);
        }
    }
}

#[test]
fn semantic_projection_number_chunks_match_locked_number_kind_and_bits() {
    let lock = include_str!("../../../../../Cargo.lock");
    assert!(lock.contains("name = \"serde_json\"\nversion = \"1.0.150\""));
    assert!(lock.contains("name = \"zmij\"\nversion = \"1.0.23\""));
    assert!(
        serde_json::from_str::<serde_json::Number>("1234567890123456789012345678901234567890")
            .unwrap()
            .is_f64()
    );
    let mut corpus = vec![
        "0".to_string(),
        "-0".into(),
        "1".into(),
        "1.0".into(),
        "1e0".into(),
        "-0.0".into(),
        "18446744073709551615".into(),
        "18446744073709551616".into(),
        "-9223372036854775808".into(),
        "-9223372036854775809".into(),
        "1.7976931348623157e308".into(),
        "1.7976931348623159e308".into(),
        "5e-324".into(),
        "2e-324".into(),
        "-1e-9999".into(),
        "1e2147483648".into(),
        "0e2147483648".into(),
        "-0e2147483648".into(),
        "1e-2147483649".into(),
        "0.00000000000000000000000000000000000000000000000001".into(),
        "1.23456789012345678901234567890123456789".into(),
        "01".into(),
        "+1".into(),
        "1.".into(),
        "1e".into(),
        "1e+".into(),
    ];
    corpus.push(format!(
        "1{}e-{}",
        "0".repeat(1024 * 1024 + 7),
        1024 * 1024 + 7
    ));
    corpus.push(format!("0.{}1", "0".repeat(1024 * 1024 + 7)));
    corpus.push(format!("1.{}", "2".repeat(1024 * 1024 + 7)));
    corpus.push(format!("-0e{}", "9".repeat(1024 * 1024 + 7)));
    for raw in corpus {
        let expected = serde_json::from_str::<serde_json::Number>(&raw);
        for chunk in [1, 2, 7, 64 * 1024] {
            let actual = number_for_test(raw.as_bytes(), chunk);
            assert_eq!(
                actual.is_ok(),
                expected.is_ok(),
                "{} chunk={chunk}",
                &raw[..raw.len().min(64)]
            );
            if let (Ok(a), Ok(b)) = (&actual, &expected) {
                assert_eq!(a, b);
                assert_eq!(
                    (a.is_u64(), a.is_i64(), a.is_f64()),
                    (b.is_u64(), b.is_i64(), b.is_f64())
                );
                assert_eq!(a.as_f64().map(f64::to_bits), b.as_f64().map(f64::to_bits));
            }
        }
    }
}

#[test]
fn semantic_projection_legacy_duplicate_first_last_matrix() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let original = projection(&c, "project");
    for (key, replacement, serde_equal, mirror) in [
        (
            "project_id",
            r#""project_id":"wrong","project_id":"project_semantic""#,
            true,
            false,
        ),
        (
            "project_id",
            r#""project_id":"project_semantic","project_id":"wrong""#,
            false,
            true,
        ),
        (
            "project_id",
            r#""project_id":"project_semantic","project_id":"project_semantic""#,
            true,
            true,
        ),
        ("probe", r#""probe":2,"probe":1"#, true, true),
        ("probe", r#""probe":1,"probe":2"#, false, true),
        ("probe", r#""probe":1,"probe":1"#, true, true),
    ] {
        set_projection(&c, "project", &member(&original, key, replacement));
        assert_eq!(verify_projection_columns(&c).is_ok(), mirror);
        assert_eq!(verify_projection_replay(&c).is_ok(), serde_equal && mirror);
        for limit in [1, 64 * 1024] {
            observe(&mut c, &path, "project", serde_equal && mirror, limit);
        }
    }
}

#[test]
fn semantic_projection_legacy_evidence_order_escape_whitespace_matrix() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let original = projection(&c, "lock");
    for (evidence, one_side) in [
        (r#"{"b":2,"a":"a"}"#, false),
        (r#"{"a":"\u0061","b":2}"#, false),
        (" { \"a\" : \"a\", \"b\" : 2 } ", true),
    ] {
        for side in ["projection", "column", "both"] {
            let altered = member(&original, "evidence", &format!("\"evidence\":{evidence}"));
            set_projection(
                &c,
                "lock",
                if side == "column" {
                    &original
                } else {
                    &altered
                },
            );
            c.execute(
                "UPDATE locks SET evidence_json=?1",
                [if side == "projection" {
                    r#"{"a":"a","b":2}"#
                } else {
                    evidence
                }],
            )
            .unwrap();
            let expected = side == "both" || one_side;
            assert_eq!(verify_projection_columns(&c).is_ok(), expected);
            assert_eq!(verify_projection_replay(&c).is_ok(), expected);
            observe(&mut c, &path, "lock", expected, 7);
        }
    }
}

#[test]
fn semantic_projection_scalar_and_indexed_boolean_types_match_legacy() {
    for expected in ["1", "1.0", "true", "null"] {
        let (_root, path) = fixture(serde_json::from_str(expected).unwrap());
        let mut c = connection(&path);
        let original = projection(&c, "project");
        for actual in ["1", "1.0", "true", "null"] {
            set_projection(
                &c,
                "project",
                &member(&original, "probe", &format!("\"probe\":{actual}")),
            );
            assert!(verify_projection_columns(&c).is_ok());
            assert_eq!(verify_projection_replay(&c).is_ok(), actual == expected);
            observe(&mut c, &path, "project", actual == expected, 2);
        }
    }
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let original = projection(&c, "lock");
    for (raw, equal, mirror) in [
        ("true", true, true),
        ("1", false, true),
        ("1.0", false, true),
        ("null", false, false),
    ] {
        set_projection(
            &c,
            "lock",
            &member(&original, "poisoned", &format!("\"poisoned\":{raw}")),
        );
        assert_eq!(verify_projection_columns(&c).is_ok(), mirror);
        assert_eq!(verify_projection_replay(&c).is_ok(), equal);
        observe(&mut c, &path, "lock", equal, 2);
    }
}

#[test]
fn semantic_projection_root_only_sequence_overlay_and_typed_drift() {
    let (_root, path) = fixture(json!({"last_event_sequence":1}));
    let mut c = connection(&path);
    let original = projection(&c, "project");
    set_projection(
        &c,
        "project",
        &format!(r#"{{"\u006cast_event_sequence":null,{}"#, &original[1..]),
    );
    assert!(verify_projection_replay(&c).is_ok());
    observe(&mut c, &path, "project", true, 1);
    set_projection(
        &c,
        "project",
        &format!(r#"{{"last_event_sequenceX":1,{}"#, &original[1..]),
    );
    assert!(verify_projection_replay(&c).is_err());
    observe(&mut c, &path, "project", false, 1);
    for raw in [
        "1",
        "999",
        "1.0",
        "true",
        "null",
        r#""untrusted""#,
        "{\"x\":[1,2,3]}",
    ] {
        set_projection(
            &c,
            "project",
            &format!("{{\"last_event_sequence\":{raw},{}", &original[1..]),
        );
        assert!(verify_projection_replay(&c).is_ok());
        observe(&mut c, &path, "project", true, 1);
    }
    set_projection(
        &c,
        "project",
        &member(&original, "probe", r#""probe":{"last_event_sequence":2}"#),
    );
    assert!(verify_projection_replay(&c).is_err());
    observe(&mut c, &path, "project", false, 1);
    set_projection(&c, "project", &original);
    // Source root is overlaid too; the nested same-named field above is not.
    let mut with_overlay: Value = serde_json::from_str(&original).unwrap();
    with_overlay["last_event_sequence"] = json!({"ignored":[false,null,"text"]});
    source_bytes(
        &c,
        RawColumn::EventPayload,
        canonical_json_v1(with_overlay).unwrap().as_bytes(),
    );
    assert!(verify_projection_replay(&c).is_ok());
    observe(&mut c, &path, "project", true, 1);
    c.execute("UPDATE projects SET last_event_sequence=2", [])
        .unwrap();
    assert!(verify_projection_columns(&c).is_ok());
    assert!(verify_projection_replay(&c).is_err());
    observe(&mut c, &path, "project", false, 64 * 1024);
}

#[test]
fn semantic_projection_escaped_keys_values_nested_duplicates_and_all_splits() {
    let (_root, path) = fixture(json!({"a":["a/é🦀\n",{"z":false}],"empty":""}));
    let mut c = connection(&path);
    let original = projection(&c, "project");
    for (raw, expected) in [
        (
            r#"{"\u0061":["\u0061\/\u00E9\uD83E\uDD80\n",{"z":false}],"empty":""}"#,
            true,
        ),
        (
            r#"{"a":null,"a":["a/é🦀\n",{"z":true,"z":false}],"empty":""}"#,
            true,
        ),
        (
            r#"{"a":["a/é🦀\n",{"z":false,"z":true}],"empty":""}"#,
            false,
        ),
        (r#"{"a":["a/é🦀\n",{"z":false}]}"#, false),
        (
            r#"{"a":["a/é🦀\n",{"z":false}],"empty":"","extra":null}"#,
            false,
        ),
        (r#"{"a":[{"z":false},"a/é🦀\n"],"empty":""}"#, false),
        (r#"{"a":["a/é🦀\n",{"z":false},0],"empty":""}"#, false),
    ] {
        set_projection(
            &c,
            "project",
            &member(&original, "probe", &format!("\"probe\":{raw}")),
        );
        assert_eq!(verify_projection_replay(&c).is_ok(), expected);
        for limit in 1..=raw.len() {
            observe(&mut c, &path, "project", expected, limit);
        }
    }
}

#[test]
fn semantic_projection_all_escape_forms_and_malformed_escapes_at_each_split() {
    let probe = "\0\u{001f}\u{0008}\t\n\u{000c}\r\"\\/é🦀";
    let (_root, path) = fixture(json!(probe));
    let mut c = connection(&path);
    let original = projection(&c, "project");
    for (raw, expected) in [
        (r#""\u0000\u001f\b\t\n\f\r\"\\\/\u00E9\uD83E\uDD80""#, true),
        (
            r#""\u0000\u001F\u0008\u0009\u000a\u000C\u000d\u0022\u005c/é🦀""#,
            true,
        ),
        (r#""\q""#, false),
        (r#""\x61""#, false),
        (r#""\u000Z""#, false),
        (r#""\u123""#, false),
        (r#""\uD800x""#, false),
        (r#""\uDC00""#, false),
        (r#""\uD800\uD800""#, false),
        ("\"\u{0001}\"", false),
    ] {
        let parsed = serde_json::from_str::<Value>(raw);
        assert_eq!(parsed.is_ok(), expected);
        if expected {
            assert_eq!(parsed.unwrap(), json!(probe));
        }
        let altered = member(&original, "probe", &format!("\"probe\":{raw}"));
        set_invalid_projection(&c, &altered);
        for limit in 1..=raw.len() {
            let (_, phases) = observe(&mut c, &path, "project", expected, limit);
            if !expected {
                assert_eq!(phases.last(), Some(&"syntax"));
            }
        }
    }
}

fn set_invalid_projection(c: &Connection, raw: &str) {
    c.execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    set_projection(c, "project", raw);
    c.execute_batch("PRAGMA ignore_check_constraints=OFF;")
        .unwrap();
}
#[test]
fn semantic_projection_losing_values_and_overlay_are_validated_before_comparison() {
    let (_root, path) = fixture(json!(1));
    let mut c = connection(&path);
    let original = projection(&c, "project");
    for bad in [
        r#""\ud800""#,
        r#""\udc00""#,
        r#""\ud800\u0061""#,
        "1e9999",
        "[1,]",
        "tru",
        r#"{"x":}"#,
    ] {
        for key in ["probe", "last_event_sequence"] {
            let raw = if key == "probe" {
                member(&original, "probe", &format!("\"probe\":{bad},\"probe\":1"))
            } else {
                format!("{{\"last_event_sequence\":{bad},{}", &original[1..])
            };
            set_invalid_projection(&c, &raw);
            assert!(serde_json::from_str::<Value>(&raw).is_err());
            let (_, phases) = observe(&mut c, &path, "project", false, 1);
            assert_eq!(phases.last(), Some(&"syntax"));
        }
    }
    for depth in [125, 126, 127] {
        let losing = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let raw = member(
            &original,
            "probe",
            &format!("\"probe\":{losing},\"probe\":1"),
        );
        set_invalid_projection(&c, &raw);
        let expected = depth < 127; // projection root object counts too
        assert_eq!(serde_json::from_str::<Value>(&raw).is_ok(), expected);
        observe(&mut c, &path, "project", expected, 1);
    }
}
