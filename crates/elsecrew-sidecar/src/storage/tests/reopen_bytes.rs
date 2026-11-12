//! S1 byte-only tests deliberately include non-JSON and damaged temporary rows.
//! They are not evidence of source semantics, canonical JSON, or admission.
use super::*;
use crate::storage::reopen::bytes::{
    arithmetic_for_test, ByteUsage, RawColumn, RawColumnSession, RawColumnStep, SourceIdentity,
    FRAGMENT_BYTES, INPUT_BYTES,
};
use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq)]
enum RawCell {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
type RawRows = Vec<Vec<RawCell>>;
type ByteSnapshot = (Vec<SchemaObject>, Vec<(String, RawRows)>);

// Unlike the legacy helper, preserve invalid UTF-8 and SQLite storage types.
fn byte_snapshot(c: &Connection) -> ByteSnapshot {
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
            let mut stmt = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
                .unwrap();
            let columns = stmt.column_count();
            let rows = stmt
                .query_map([], |row| {
                    (0..columns)
                        .map(|i| {
                            Ok(match row.get_ref(i)? {
                                rusqlite::types::ValueRef::Null => RawCell::Null,
                                rusqlite::types::ValueRef::Integer(v) => RawCell::Integer(v),
                                rusqlite::types::ValueRef::Real(v) => RawCell::Real(v),
                                rusqlite::types::ValueRef::Text(v) => RawCell::Text(v.to_vec()),
                                rusqlite::types::ValueRef::Blob(v) => RawCell::Blob(v.to_vec()),
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

const STAMP: &str = "2026-09-02T01:00:00Z";
const COLUMNS: [RawColumn; 3] = [
    RawColumn::EventPayload,
    RawColumn::FrozenPayload,
    RawColumn::FrozenRecord,
];

fn identity() -> SourceIdentity {
    SourceIdentity {
        source_rowid: 1,
        event_id: "evt_bytes".into(),
        operation_id: "op_bytes".into(),
        aggregate_type: "project".into(),
        aggregate_id: "project_bytes".into(),
        aggregate_sequence: 1,
        payload_schema_version: 1,
        event_type: "project.upserted".into(),
        request_id: None,
        timestamp: STAMP.into(),
    }
}
fn identity_bytes(id: &SourceIdentity) -> usize {
    id.event_id.len()
        + id.operation_id.len()
        + id.aggregate_type.len()
        + id.aggregate_id.len()
        + id.event_type.len()
        + id.request_id.as_ref().map_or(0, String::len)
        + id.timestamp.len()
}
fn parts(column: RawColumn) -> (&'static str, &'static str) {
    match column {
        RawColumn::EventPayload => ("events", "canonical_json_payload"),
        RawColumn::FrozenPayload => ("frozen_operation_sources", "canonical_payload_json"),
        RawColumn::FrozenRecord => ("frozen_operation_sources", "canonical_record_json"),
    }
}
fn fixture() -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    let source = event(
        "evt_bytes",
        "op_bytes",
        "project",
        "project_bytes",
        1,
        json!({"project_id":"project_bytes", "canonical_path":"/temporary/bytes"}),
        STAMP,
    );
    storage.append_event(&source).unwrap();
    insert_exact_v8_frozen_operation(&mut storage, &source, STAMP);
    storage.checkpoint().unwrap();
    drop(storage);
    (root, path)
}
fn connection(path: &Path) -> Connection {
    let c = Connection::open(path).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=0;")
        .unwrap();
    c
}
fn alter(connection: &Connection, sql: &str, parameters: impl rusqlite::Params) {
    // Temporary corruption setup only. Restore every exact schema object before
    // the verifier is invoked, including for non-JSON/invalid UTF-8 transport.
    let triggers = connection
        .prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger'")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    for (name, _) in &triggers {
        connection
            .execute_batch(&format!("DROP TRIGGER {name}"))
            .unwrap();
    }
    connection.execute(sql, parameters).unwrap();
    for (_, sql) in triggers {
        connection.execute_batch(&sql).unwrap();
    }
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA ignore_check_constraints=OFF;")
        .unwrap();
}
fn set_bytes(connection: &Connection, column: RawColumn, bytes: &[u8]) {
    let (table, column) = parts(column);
    alter(
        connection,
        &format!("UPDATE {table} SET {column}=CAST(?1 AS TEXT)"),
        [bytes],
    );
}
fn assert_usage(usage: &ByteUsage) {
    assert!(usage.fragment_bytes <= FRAGMENT_BYTES);
    assert!(usage.encoded_bytes() <= INPUT_BYTES);
    assert!(usage.work_units <= 100);
}
fn unchanged(connection: &Connection, path: &Path, before: &ByteSnapshot, bytes: &[u8]) {
    assert!(connection.is_autocommit());
    assert_eq!(&byte_snapshot(connection), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
}
fn complete(
    connection: &mut Connection,
    column: RawColumn,
    expected: SourceIdentity,
    oracle: &[u8],
) -> Vec<ByteUsage> {
    let retained = identity_bytes(&expected);
    let mut s = RawColumnSession::start(connection, column, expected).unwrap();
    assert!(s.autocommit_for_test());
    let (start, _, cursor) = s.usage_for_test();
    assert_eq!(cursor, Some(0));
    assert_eq!(start.retained_bytes, retained);
    assert_eq!(start.metadata_bytes, retained);
    assert_eq!(start.fragment_bytes, 0);
    assert_eq!(start.work_units, 2);
    assert_usage(start);
    let mut total = 0;
    let mut usages = Vec::new();
    loop {
        let result = s.step(&mut || false).unwrap();
        assert!(s.autocommit_for_test());
        match result {
            RawColumnStep::Pending(usage) => {
                assert_usage(&usage);
                assert!(usage.fragment_bytes > 0);
                assert_eq!(usage.metadata_bytes, retained);
                assert_eq!(usage.work_units, 3);
                total += usage.fragment_bytes;
                assert_eq!(s.usage_for_test().2, Some(total as u64));
                assert!(total < oracle.len());
                usages.push(usage);
            }
            RawColumnStep::BytesComplete {
                byte_length,
                sha256,
                usage,
            } => {
                assert_usage(&usage);
                assert_eq!(usage.metadata_bytes, retained);
                assert_eq!(usage.work_units, 3);
                total += usage.fragment_bytes;
                assert_eq!(total, oracle.len());
                assert_eq!(byte_length, oracle.len() as u64);
                assert_eq!(sha256, <[u8; 32]>::from(Sha256::digest(oracle)));
                assert_eq!(s.usage_for_test().2, None);
                usages.push(usage);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(
        s.step(&mut || false).unwrap_err().kind,
        "storageSchemaMismatch"
    );
    usages
}

#[test]
fn raw_column_bytes_all_addresses_size_boundaries_and_non_json() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for length in [0, 1, FRAGMENT_BYTES, FRAGMENT_BYTES + 1, INPUT_BYTES + 17] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut c = connection(&path);
            let raw = vec![b'x'; length];
            set_bytes(&c, column, &raw);
            let before = byte_snapshot(&c);
            let file = fs::read(&path).unwrap();
            let usage = complete(&mut c, column, identity(), &raw);
            assert_eq!(usage.len(), length.div_ceil(FRAGMENT_BYTES).max(1));
            unchanged(&c, &path, &before, &file);
        }
    }
}

#[test]
fn raw_column_bytes_utf8_carries_and_malformed_first_or_late_bytes() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for scalar in ["é", "한", "🦀"] {
            for split in 1..scalar.len() {
                let (_root, path) = database();
                fs::copy(&template, &path).unwrap();
                let mut c = connection(&path);
                let mut raw = vec![b'a'; FRAGMENT_BYTES - split];
                raw.extend_from_slice(scalar.as_bytes());
                raw.extend_from_slice(b"tail");
                set_bytes(&c, column, &raw);
                let before = byte_snapshot(&c);
                let file = fs::read(&path).unwrap();
                let usage = complete(&mut c, column, identity(), &raw);
                assert_eq!(usage.len(), 2);
                assert_eq!(usage[1].retained_bytes, identity_bytes(&identity()) + split);
                unchanged(&c, &path, &before, &file);
            }
        }
        for late in [false, true] {
            for tail in [
                vec![0xff],
                vec![0xc0, 0x80],
                vec![0xed, 0xa0, 0x80],
                vec![0xf4, 0x90, 0x80, 0x80],
                vec![0xe2],
                vec![0xf0, 0x9f, 0xa6],
            ] {
                let (_root, path) = database();
                fs::copy(&template, &path).unwrap();
                let mut c = connection(&path);
                let mut raw = if late {
                    vec![b'x'; FRAGMENT_BYTES - 1]
                } else {
                    Vec::new()
                };
                raw.extend_from_slice(&tail);
                set_bytes(&c, column, &raw);
                let before = byte_snapshot(&c);
                let file = fs::read(&path).unwrap();
                {
                    let mut s = RawColumnSession::start(&mut c, column, identity()).unwrap();
                    let mut failed = false;
                    for _ in 0..3 {
                        match s.step(&mut || false) {
                            Ok(RawColumnStep::Pending(u)) => assert_usage(&u),
                            Err(e) => {
                                assert_eq!(e.kind, "storageSchemaMismatch");
                                failed = true;
                                break;
                            }
                            other => panic!("accepted invalid bytes: {other:?}"),
                        }
                    }
                    assert!(failed);
                    assert!(s.autocommit_for_test());
                    assert_eq!(s.usage_for_test().2, None);
                    assert_usage(s.usage_for_test().1);
                    assert!(s.step(&mut || false).is_err());
                }
                unchanged(&c, &path, &before, &file);
            }
        }
    }
}

#[test]
fn raw_column_bytes_identity_tuple_and_missing_rows() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for field in 0..10 {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut c = connection(&path);
            let mut expected = identity();
            match field {
                0 => expected.source_rowid += 1,
                1 => expected.event_id.push('x'),
                2 => expected.operation_id.push('x'),
                3 => expected.aggregate_type.push('x'),
                4 => expected.aggregate_id.push('x'),
                5 => expected.aggregate_sequence += 1,
                6 => expected.payload_schema_version += 1,
                7 => expected.event_type.push('x'),
                8 => expected.request_id = Some("request".into()),
                _ => expected.timestamp.push('x'),
            }
            let before = byte_snapshot(&c);
            let file = fs::read(&path).unwrap();
            assert!(
                matches!(RawColumnSession::start(&mut c,column,expected),Err(e) if e.kind=="storageSchemaMismatch")
            );
            unchanged(&c, &path, &before, &file);
        }
        {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut c = connection(&path);
            let (table, _) = parts(column);
            alter(&c, &format!("DELETE FROM {table}"), []);
            let before = byte_snapshot(&c);
            let file = fs::read(&path).unwrap();
            assert!(RawColumnSession::start(&mut c, column, identity()).is_err());
            unchanged(&c, &path, &before, &file);
        }
    }
}

#[test]
fn raw_column_bytes_metadata_exact_budget_progress_and_intrinsic_failure() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for available in [0, 2, FRAGMENT_BYTES] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut c = connection(&path);
            let mut expected = identity();
            // Every metadata byte is counted twice: retained expected tuple +
            // fresh exact lookup. Even totals give exact 8MiB and +2 controls.
            let padding = (INPUT_BYTES - available) / 2 - identity_bytes(&expected);
            expected.request_id = Some("r".repeat(padding));
            let (table, _) = parts(column);
            alter(
                &c,
                &format!("UPDATE {table} SET request_id=?1"),
                [expected.request_id.as_deref()],
            );
            let raw = vec![b'x'; if available == 0 { 0 } else { 3 }];
            set_bytes(&c, column, &raw);
            let before = byte_snapshot(&c);
            let file = fs::read(&path).unwrap();
            let usage = complete(&mut c, column, expected, &raw);
            assert_eq!(
                usage[0].encoded_bytes(),
                if available <= 2 {
                    INPUT_BYTES
                } else {
                    INPUT_BYTES - FRAGMENT_BYTES + 3
                }
            );
            unchanged(&c, &path, &before, &file);
        }
        for oversize in [false, true] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut c = connection(&path);
            let mut expected = identity();
            expected.request_id = Some(
                "r".repeat(INPUT_BYTES / 2 - identity_bytes(&expected) + usize::from(oversize)),
            );
            let (table, _) = parts(column);
            alter(
                &c,
                &format!("UPDATE {table} SET request_id=?1"),
                [expected.request_id.as_deref()],
            );
            set_bytes(&c, column, b"x");
            let before = byte_snapshot(&c);
            let file = fs::read(&path).unwrap();
            assert!(RawColumnSession::start(&mut c, column, expected).is_err());
            unchanged(&c, &path, &before, &file);
        }
    }
}

#[test]
fn raw_column_bytes_cancellation_entry_mid_precommit_discards_early_and_late() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for preceding in [0, 1] {
            for poll in [1, 2, 3] {
                let (_root, path) = database();
                fs::copy(&template, &path).unwrap();
                let mut c = connection(&path);
                let raw = vec![b'x'; FRAGMENT_BYTES + 1];
                set_bytes(&c, column, &raw);
                let before = byte_snapshot(&c);
                let file = fs::read(&path).unwrap();
                {
                    let mut s = RawColumnSession::start(&mut c, column, identity()).unwrap();
                    if preceding == 1 {
                        assert!(matches!(
                            s.step(&mut || false).unwrap(),
                            RawColumnStep::Pending(_)
                        ));
                    }
                    let mut calls = 0;
                    assert_eq!(
                        s.step(&mut || {
                            calls += 1;
                            calls == poll
                        })
                        .unwrap(),
                        RawColumnStep::Cancelled
                    );
                    assert_eq!(s.usage_for_test().2, None);
                    assert_usage(s.usage_for_test().1);
                    assert_eq!(
                        s.usage_for_test().1.fragment_bytes,
                        if poll == 1 {
                            0
                        } else if preceding == 0 {
                            FRAGMENT_BYTES
                        } else {
                            1
                        }
                    );
                    assert!(s.autocommit_for_test());
                    assert!(s.step(&mut || false).is_err());
                }
                unchanged(&c, &path, &before, &file);
                complete(&mut c, column, identity(), &raw); // a fresh session starts at zero
            }
        }
    }
}

#[test]
fn raw_column_bytes_external_commits_same_length_tail_growth_and_terminal() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for change in [0, 1, 2, 3] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut c = connection(&path);
            let mut raw = vec![b'x'; FRAGMENT_BYTES + 1];
            set_bytes(&c, column, &raw);
            let other = connection(&path);
            let after;
            let bytes;
            {
                let mut s = RawColumnSession::start(&mut c, column, identity()).unwrap();
                assert!(matches!(
                    s.step(&mut || false).unwrap(),
                    RawColumnStep::Pending(_)
                ));
                if change == 3 {
                    assert!(matches!(
                        s.step(&mut || false).unwrap(),
                        RawColumnStep::BytesComplete { .. }
                    ));
                }
                match change {
                    0 => raw[0] = b'y',
                    1 => raw[FRAGMENT_BYTES] = b'y',
                    2 => raw.push(b'y'),
                    _ => raw[0] = b'z',
                }
                set_bytes(&other, column, &raw);
                after = byte_snapshot(&other);
                bytes = fs::read(&path).unwrap();
                assert_eq!(
                    s.step(&mut || false).unwrap_err().kind,
                    "storageSchemaMismatch"
                );
                assert!(s.autocommit_for_test());
                assert_eq!(s.usage_for_test().2, None);
                assert!(s.step(&mut || false).is_err());
            }
            unchanged(&c, &path, &after, &bytes);
            complete(&mut c, column, identity(), &raw);
        }
    }
}

#[test]
fn raw_column_bytes_exact_lookup_plans_and_checked_arithmetic() {
    let (_root, path) = fixture();
    let c = connection(&path);
    for column in COLUMNS {
        for query in column.queries_for_test() {
            assert!(!query.contains("OFFSET"));
            assert!(!query.contains("json_extract("));
            let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {query}")).unwrap();
            let count = stmt.parameter_count();
            let locator: SqlValue = match column {
                RawColumn::EventPayload => SqlValue::Integer(1),
                _ => SqlValue::Text("op_bytes".into()),
            };
            let params = if count == 1 {
                vec![locator]
            } else {
                vec![locator, SqlValue::Integer(1), SqlValue::Integer(10)]
            };
            let plan = stmt
                .query_map(rusqlite::params_from_iter(params), |row| {
                    row.get::<_, String>(3)
                })
                .unwrap()
                .map(Result::unwrap)
                .collect::<Vec<_>>()
                .join(" ");
            assert!(plan.contains("SEARCH"), "{plan}");
            assert!(plan.contains("PRIMARY KEY"), "{plan}");
            assert!(!plan.contains("SCAN") && !plan.contains("TEMP"), "{plan}");
        }
        assert!(column.queries_for_test()[2].contains("substr(CAST("));
    }
    for (len, after, used) in [
        (0, u64::MAX, 0),
        (u64::MAX, i64::MAX as u64, 0),
        (0, 1, 0),
        (1, 0, INPUT_BYTES + 1),
        (1, 0, INPUT_BYTES),
    ] {
        assert!(arithmetic_for_test(len, after, used).is_err());
    }
    assert_eq!(arithmetic_for_test(0, 0, INPUT_BYTES).unwrap(), (1, 0));
    assert_eq!(arithmetic_for_test(2, 0, INPUT_BYTES - 1).unwrap(), (1, 1));
}

// Real SQLite authorization failures, without a production failure-injection API.
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
            second: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
            _: *const std::ffi::c_char,
        ) -> std::ffi::c_int {
            // SAFETY: SqlFault unregisters before its context drops; SQLite
            // supplies NUL-terminated arguments, with NULL explicitly handled.
            let mode = unsafe { &*context.cast::<std::sync::atomic::AtomicU8>() }
                .load(std::sync::atomic::Ordering::Relaxed);
            let first = if first.is_null() {
                b"".as_slice()
            } else {
                unsafe { std::ffi::CStr::from_ptr(first) }.to_bytes()
            };
            let second = if second.is_null() {
                b"".as_slice()
            } else {
                unsafe { std::ffi::CStr::from_ptr(second) }.to_bytes()
            };
            if mode == 1 && action == rusqlite::ffi::SQLITE_TRANSACTION && first == b"COMMIT"
                || mode == 2
                    && action == rusqlite::ffi::SQLITE_READ
                    && [b"events".as_slice(), b"frozen_operation_sources"].contains(&first)
            {
                rusqlite::ffi::SQLITE_DENY
            } else if mode == 3
                && action == rusqlite::ffi::SQLITE_READ
                && [
                    b"canonical_json_payload".as_slice(),
                    b"canonical_payload_json",
                    b"canonical_record_json",
                ]
                .contains(&second)
            {
                // SQLITE_IGNORE substitutes NULL on read; STRICT/NOT NULL
                // correctly prevents an ordinary writer storing this shape.
                rusqlite::ffi::SQLITE_IGNORE
            } else {
                rusqlite::ffi::SQLITE_OK
            }
        }
        let mode = Box::new(std::sync::atomic::AtomicU8::new(0));
        // SAFETY: this hook is dropped before its exclusively owned connection.
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
        // SAFETY: every test declares the hook after, and drops it before, c.
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(self.handle, None, std::ptr::null_mut());
        }
    }
}

#[test]
fn raw_column_bytes_actual_read_commit_and_null_read_failures_at_start_and_steps() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for mode in [1, 2, 3] {
            for stage in [0, 1, 2] {
                let (_root, path) = database();
                fs::copy(&template, &path).unwrap();
                let mut c = connection(&path);
                let raw = vec![b'x'; FRAGMENT_BYTES + 1];
                set_bytes(&c, column, &raw);
                let before = byte_snapshot(&c);
                let file = fs::read(&path).unwrap();
                let fault = SqlFault::install(&c);
                if stage == 0 {
                    fault.set(mode);
                    assert!(
                        matches!(RawColumnSession::start(&mut c,column,identity()),Err(e) if e.kind=="storageSchemaMismatch")
                    );
                    fault.set(0);
                } else {
                    let mut s = RawColumnSession::start(&mut c, column, identity()).unwrap();
                    if stage == 2 {
                        assert!(matches!(
                            s.step(&mut || false).unwrap(),
                            RawColumnStep::Pending(_)
                        ));
                    }
                    fault.set(mode);
                    assert_eq!(
                        s.step(&mut || false).unwrap_err().kind,
                        "storageSchemaMismatch"
                    );
                    fault.set(0);
                    assert!(s.autocommit_for_test());
                    assert_eq!(s.usage_for_test().2, None);
                    assert_usage(s.usage_for_test().1);
                    if mode == 1 {
                        assert_eq!(
                            s.usage_for_test().1.fragment_bytes,
                            if stage == 1 { FRAGMENT_BYTES } else { 1 }
                        );
                    } else {
                        assert_eq!(s.usage_for_test().1.fragment_bytes, 0);
                    }
                    assert!(s.step(&mut || false).is_err());
                }
                drop(fault);
                unchanged(&c, &path, &before, &file);
                complete(&mut c, column, identity(), &raw);
            }
        }
    }
}

#[test]
fn raw_column_bytes_busy_begin_nested_start_and_schema_preflight_are_fail_closed() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        for stage in [0, 1, 2] {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            let mut c = connection(&path);
            set_bytes(&c, column, &vec![b'x'; FRAGMENT_BYTES + 1]);
            let other = connection(&path);
            let before = byte_snapshot(&c);
            let file = fs::read(&path).unwrap();
            if stage == 0 {
                other.execute_batch("BEGIN IMMEDIATE").unwrap();
                assert!(RawColumnSession::start(&mut c, column, identity()).is_err());
                other.execute_batch("ROLLBACK").unwrap();
            } else {
                let mut s = RawColumnSession::start(&mut c, column, identity()).unwrap();
                if stage == 2 {
                    assert!(matches!(
                        s.step(&mut || false).unwrap(),
                        RawColumnStep::Pending(_)
                    ));
                }
                other.execute_batch("BEGIN IMMEDIATE").unwrap();
                assert!(s.step(&mut || false).is_err());
                other.execute_batch("ROLLBACK").unwrap();
                assert!(s.autocommit_for_test());
                assert_eq!(s.usage_for_test().2, None);
                assert!(s.step(&mut || false).is_err());
            }
            unchanged(&c, &path, &before, &file);
        }
    }
    let (_root, path) = fixture();
    let mut c = connection(&path);
    let before = byte_snapshot(&c);
    let file = fs::read(&path).unwrap();
    c.execute_batch("BEGIN").unwrap();
    assert!(RawColumnSession::start(&mut c, RawColumn::EventPayload, identity()).is_err());
    assert!(!c.is_autocommit(), "do not end the caller's transaction");
    c.execute_batch("ROLLBACK").unwrap();
    unchanged(&c, &path, &before, &file);
    c.execute_batch("DROP INDEX frozen_operation_sources_source_event")
        .unwrap();
    let before = byte_snapshot(&c);
    let file = fs::read(&path).unwrap();
    assert!(RawColumnSession::start(&mut c, RawColumn::EventPayload, identity()).is_err());
    unchanged(&c, &path, &before, &file);
}

#[test]
fn raw_column_bytes_unexpected_slice_type_null_short_and_long_are_rejected() {
    unsafe extern "C" fn slice_fault(
        context: *mut rusqlite::ffi::sqlite3_context,
        _: std::ffi::c_int,
        args: *mut *mut rusqlite::ffi::sqlite3_value,
    ) {
        // SAFETY: the registered mode outlives this function and exactly three
        // SQLite arguments are registered. This never reads the payload arg.
        let mode = unsafe { *rusqlite::ffi::sqlite3_user_data(context).cast::<u8>() };
        let amount = unsafe { rusqlite::ffi::sqlite3_value_int64(*args.add(2)) } as usize;
        match mode {
            0 => unsafe { rusqlite::ffi::sqlite3_result_null(context) },
            1 => unsafe {
                rusqlite::ffi::sqlite3_result_text(
                    context,
                    c"x".as_ptr(),
                    1,
                    rusqlite::ffi::SQLITE_TRANSIENT(),
                )
            },
            _ => {
                let raw = vec![b'x'; if mode == 2 { amount - 1 } else { amount + 1 }];
                unsafe {
                    rusqlite::ffi::sqlite3_result_blob(
                        context,
                        raw.as_ptr().cast(),
                        raw.len() as i32,
                        rusqlite::ffi::SQLITE_TRANSIENT(),
                    )
                };
            }
        }
    }
    let (_template, template) = fixture();
    for column in COLUMNS {
        for mode in 0_u8..4 {
            let (_root, path) = database();
            fs::copy(&template, &path).unwrap();
            // Mode is allocated before c so it outlives c even during unwinding.
            let mut mode = Box::new(mode);
            let mut c = connection(&path);
            set_bytes(&c, column, &vec![b'x'; FRAGMENT_BYTES + 1]);
            let before = byte_snapshot(&c);
            let file = fs::read(&path).unwrap();
            // SAFETY: registering a test-only connection-local function; callback
            // context remains alive until after this connection is dropped.
            unsafe {
                assert_eq!(
                    rusqlite::ffi::sqlite3_create_function_v2(
                        c.handle(),
                        c"substr".as_ptr(),
                        3,
                        rusqlite::ffi::SQLITE_UTF8,
                        (&mut *mode as *mut u8).cast(),
                        Some(slice_fault),
                        None,
                        None,
                        None
                    ),
                    rusqlite::ffi::SQLITE_OK
                );
            }
            {
                let mut s = RawColumnSession::start(&mut c, column, identity()).unwrap();
                assert_eq!(
                    s.step(&mut || false).unwrap_err().kind,
                    "storageSchemaMismatch"
                );
                assert!(s.autocommit_for_test());
                assert_eq!(s.usage_for_test().2, None);
                assert!(s.step(&mut || false).is_err());
            }
            unchanged(&c, &path, &before, &file);
            // These are defensive result-shape probes, not a claim that built-in
            // substr exceeds its bound or that STRICT permits NULL/BLOB storage.
        }
    }
}

#[test]
fn raw_column_bytes_large_multibyte_metadata_preflight_and_drop_restart() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        let (_root, path) = database();
        fs::copy(&template, &path).unwrap();
        let mut c = connection(&path);
        let (table, _) = parts(column);
        let request = "한".repeat(INPUT_BYTES / 3 + 1);
        assert!(request.chars().count() < INPUT_BYTES);
        assert!(request.len() > INPUT_BYTES);
        alter(&c, &format!("UPDATE {table} SET request_id=?1"), [&request]);
        let before = byte_snapshot(&c);
        let file = fs::read(&path).unwrap();
        // Expected identity is tiny: the source metadata itself must be guarded.
        assert!(RawColumnSession::start(&mut c, column, identity()).is_err());
        unchanged(&c, &path, &before, &file);
        alter(&c, &format!("UPDATE {table} SET request_id=NULL"), []);
        let raw = vec![b'x'; FRAGMENT_BYTES + 1];
        set_bytes(&c, column, &raw);
        {
            let mut s = RawColumnSession::start(&mut c, column, identity()).unwrap();
            assert!(matches!(
                s.step(&mut || false).unwrap(),
                RawColumnStep::Pending(_)
            ));
        }
        let usage = complete(&mut c, column, identity(), &raw);
        assert_eq!(usage.len(), 2);
    }
}

#[test]
fn raw_column_bytes_retained_utf8_carry_exhausts_intrinsic_budget_terminally() {
    let (_template, template) = fixture();
    for column in COLUMNS {
        let (_root, path) = database();
        fs::copy(&template, &path).unwrap();
        let mut c = connection(&path);
        let mut expected = identity();
        expected.request_id = Some("r".repeat((INPUT_BYTES - 2) / 2 - identity_bytes(&expected)));
        let (table, _) = parts(column);
        alter(
            &c,
            &format!("UPDATE {table} SET request_id=?1"),
            [expected.request_id.as_deref()],
        );
        set_bytes(&c, column, "🦀".as_bytes());
        let before = byte_snapshot(&c);
        let file = fs::read(&path).unwrap();
        {
            let mut s = RawColumnSession::start(&mut c, column, expected).unwrap();
            let RawColumnStep::Pending(first) = s.step(&mut || false).unwrap() else {
                panic!("expected split scalar")
            };
            assert_eq!(first.fragment_bytes, 2);
            assert_eq!(first.encoded_bytes(), INPUT_BYTES);
            assert_eq!(s.usage_for_test().2, Some(2));
            assert_eq!(
                s.step(&mut || false).unwrap_err().kind,
                "storageSchemaMismatch"
            );
            let (_, last, cursor) = s.usage_for_test();
            assert_eq!(last.retained_bytes, first.retained_bytes + 2);
            assert_eq!(last.metadata_bytes, first.metadata_bytes);
            assert_eq!(last.fragment_bytes, 0);
            assert_eq!(last.encoded_bytes(), INPUT_BYTES);
            assert_eq!(cursor, None);
            assert!(s.autocommit_for_test());
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
    }
}
