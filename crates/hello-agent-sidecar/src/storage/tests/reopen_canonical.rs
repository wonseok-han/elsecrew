//! Whole Value/bytes here are test-only acceptance oracles, not scanner inputs.
use super::*;
use crate::storage::reopen::bytes::{RawColumn, SourceIdentity, FRAGMENT_BYTES, INPUT_BYTES};
use crate::storage::reopen::canonical::{
    CanonicalColumnSession, CanonicalColumnStep, CanonicalUsage,
};
use sha2::{Digest, Sha256};

const STAMP: &str = "2026-09-03T01:00:00Z";
const COLUMNS: [RawColumn; 3] = [
    RawColumn::EventPayload,
    RawColumn::FrozenPayload,
    RawColumn::FrozenRecord,
];

fn identity() -> SourceIdentity {
    SourceIdentity {
        source_rowid: 1,
        event_id: "evt_canonical".into(),
        operation_id: "op_canonical".into(),
        aggregate_type: "project".into(),
        aggregate_id: "project_canonical".into(),
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
fn fixture() -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    let source = event(
        "evt_canonical",
        "op_canonical",
        "project",
        "project_canonical",
        1,
        json!({"project_id":"project_canonical","canonical_path":"/fixture/canonical"}),
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
fn parts(column: RawColumn) -> (&'static str, &'static str) {
    match column {
        RawColumn::EventPayload => ("events", "canonical_json_payload"),
        RawColumn::FrozenPayload => ("frozen_operation_sources", "canonical_payload_json"),
        RawColumn::FrozenRecord => ("frozen_operation_sources", "canonical_record_json"),
    }
}
fn alter(c: &Connection, sql: &str, parameters: impl rusqlite::Params) {
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
    c.execute(sql, parameters).unwrap();
    for (_, sql) in triggers {
        c.execute_batch(&sql).unwrap();
    }
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA ignore_check_constraints=OFF;")
        .unwrap();
}
fn set_bytes(c: &Connection, column: RawColumn, bytes: &[u8]) {
    let (table, column) = parts(column);
    alter(
        c,
        &format!("UPDATE {table} SET {column}=CAST(?1 AS TEXT)"),
        [bytes],
    );
}

#[derive(Debug, PartialEq)]
enum RawCell {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
type RawTable = (String, Vec<Vec<RawCell>>);
type Snapshot = (Vec<SchemaObject>, Vec<RawTable>);
fn snapshot(c: &Connection) -> Snapshot {
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
            let mut statement = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
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
fn unchanged(c: &Connection, path: &Path, before: &Snapshot, bytes: &[u8]) {
    assert!(c.is_autocommit());
    assert_eq!(&snapshot(c), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
}
fn oracle(raw: &[u8]) -> (bool, bool) {
    match serde_json::from_slice::<Value>(raw) {
        Ok(value) => (true, canonical_json_v1(value).unwrap().as_bytes() == raw),
        Err(_) => (false, false),
    }
}
fn usage(u: &CanonicalUsage) {
    assert_eq!(u.main_read_bytes + u.key_read_bytes, u.input.fragment_bytes);
    assert!(u.input.fragment_bytes <= FRAGMENT_BYTES);
    assert!(u.input.encoded_bytes() <= INPUT_BYTES);
    assert!(u.input.work_units <= 100);
    assert!(u.main_consumed_bytes <= u.main_read_bytes);
}
fn compare(
    c: &mut Connection,
    path: &Path,
    column: RawColumn,
    raw: &[u8],
    limit: usize,
) -> Vec<CanonicalUsage> {
    let expected = oracle(raw).1;
    let before = snapshot(c);
    let file = fs::read(path).unwrap();
    let mut batches = Vec::new();
    {
        let mut scanner = CanonicalColumnSession::start(c, column, identity()).unwrap();
        scanner.fragment_limit_for_test(limit);
        usage(scanner.usage_for_test().0);
        assert_eq!(
            scanner.usage_for_test().0.input.retained_bytes,
            identity_bytes(&identity())
        );
        let mut previous = scanner.usage_for_test().2;
        let mut complete = false;
        let mut stopped = false;
        let mut consumed = 0;
        for _ in 0..100_000 {
            let result = scanner.step(&mut || false);
            assert!(scanner.autocommit_for_test());
            usage(scanner.usage_for_test().1);
            match result {
                Ok(CanonicalColumnStep::Pending(u)) => {
                    assert_ne!(scanner.usage_for_test().2, previous, "no progress");
                    previous = scanner.usage_for_test().2;
                    consumed += u.main_consumed_bytes;
                    batches.push(u);
                }
                Ok(CanonicalColumnStep::CanonicalComplete {
                    byte_length,
                    sha256,
                    usage: u,
                }) => {
                    consumed += u.main_consumed_bytes;
                    assert_eq!(consumed, raw.len());
                    assert_eq!(byte_length, raw.len() as u64);
                    assert_eq!(sha256, <[u8; 32]>::from(Sha256::digest(raw)));
                    batches.push(u);
                    complete = true;
                    stopped = true;
                    break;
                }
                Err(error) => {
                    assert_eq!(error.kind, "storageSchemaMismatch");
                    stopped = true;
                    break;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(stopped, "scanner did not terminate");
        assert_eq!(
            complete,
            expected,
            "column={column:?} limit={limit} raw={:?}",
            String::from_utf8_lossy(&raw[..raw.len().min(128)])
        );
        assert!(scanner.usage_for_test().2.is_none());
        assert!(scanner.step(&mut || false).is_err());
    }
    unchanged(c, path, &before, &file);
    batches
}

#[test]
fn canonical_column_small_corpus_all_split_positions_matches_value_oracle() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    let corpus = [
        ("null", true),
        ("true", true),
        ("false", true),
        ("[]", true),
        ("{}", true),
        (r#"[null,true,false,1,-0.0]"#, true),
        (r#"{"":0,"\b":1,"\n":2,"a":3,"é":4,"🦀":5}"#, true),
        (r#"{"\n":0,"a":1}"#, true),
        (r#"{"a":0,"\n":1}"#, false),
        (r#"{"\n":0,"A":1}"#, true),
        (r#"{"A":0,"\n":1}"#, false),
        (r#"{"\"":0,"\\":1}"#, true),
        (r#"{"a":0,"a":1}"#, false),
        (r#""\u0000\u001f\b\t\n\f\r\"\\é🦀""#, true),
        (r#""\/""#, false),
        (r#""\u0061""#, false),
        (r#""\ud83e\udd80""#, false),
        (r#""\u000a""#, false),
        (r#""\u001F""#, false),
        ("\"", false),
        ("\"tail", false),
        ("[", false),
        ("[1,]", false),
        ("{\"a\":}", false),
        ("{\"a\":0,}", false),
        ("tru", false),
        ("nullx", false),
        (" null", false),
        ("null ", false),
        ("", false),
        ("1", true),
        ("1.0", true),
        ("1e0", false),
        ("-0", false),
        ("-0.0", true),
        ("01", false),
        ("1.", false),
        ("1e", false),
        ("1e+1", false),
        ("+1", false),
    ];
    for (raw, canonical) in corpus {
        assert_eq!(oracle(raw.as_bytes()).1, canonical, "declared oracle {raw}");
        set_bytes(&c, RawColumn::EventPayload, raw.as_bytes());
        for limit in 1..=raw.len().max(1) {
            compare(
                &mut c,
                &path,
                RawColumn::EventPayload,
                raw.as_bytes(),
                limit,
            );
        }
    }
}

#[test]
fn canonical_column_mixed_container_depth_counts_the_envelope() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    for depth in [126, 127, 128] {
        let mut raw = "0".to_string();
        for level in 0..depth {
            raw = if level % 2 == 0 {
                format!("[{raw}]")
            } else {
                format!("{{\"a\":{raw}}}")
            };
        }
        assert_eq!(oracle(raw.as_bytes()), (depth <= 127, depth <= 127));
        for column in COLUMNS {
            set_bytes(&c, column, raw.as_bytes());
            compare(&mut c, &path, column, raw.as_bytes(), 1);
        }
    }
}

#[test]
fn canonical_column_number_parser_serializer_output_bound_guard() {
    // Version guard plus behavior guard: arbitrary_precision would retain the
    // long integer verbatim and invalidate the fixed Number output-image proof.
    let lock = include_str!("../../../../../Cargo.lock");
    assert!(lock.contains("name = \"serde_json\"\nversion = \"1.0.150\""));
    assert!(lock.contains("name = \"zmij\"\nversion = \"1.0.23\""));
    let long_integer = "1234567890123456789012345678901234567890";
    let number: serde_json::Number = serde_json::from_str(long_integer).unwrap();
    assert!(number.is_f64());
    assert!(serde_json::to_vec(&number).unwrap().len() <= 24);
    assert_eq!(oracle(long_integer.as_bytes()), (true, false));
    let (_root, path) = fixture();
    let mut c = connection(&path);
    let mut corpus = [
        "0",
        "1",
        "1.0",
        "1e0",
        "-0",
        "-0.0",
        "1.00",
        "0.0000000000000000000000000000000000001",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
        "1.7976931348623157e308",
        "-1.7976931348623157e308",
        "5e-324",
        "2.2250738585072014e-308",
        "1e309",
        "-1e309",
        "1e-400",
        "1e0000000000000000000000000000000000000000000000000000000",
        long_integer,
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    let mut longest = 0;
    // Reproducible finite-f64 serializer samples include subnormals and broad
    // exponent/significand ranges. This is regression evidence, not the proof
    // replacing zmij's fixed BUFFER_SIZE=24 source bound.
    let mut bits = 0x9e3779b97f4a7c15_u64;
    for index in 0..16_384 {
        bits = bits.wrapping_mul(6364136223846793005).wrapping_add(1);
        if let Some(number) = serde_json::Number::from_f64(f64::from_bits(bits)) {
            let encoded = serde_json::to_string(&number).unwrap();
            longest = longest.max(encoded.len());
            assert!(encoded.len() <= 24);
            if index % 257 == 0 {
                corpus.push(encoded);
            }
        }
    }
    assert_eq!(longest, 24, "exercise the output-image bound itself");
    for raw in corpus {
        set_bytes(&c, RawColumn::EventPayload, raw.as_bytes());
        for limit in [1, 7, 64] {
            compare(
                &mut c,
                &path,
                RawColumn::EventPayload,
                raw.as_bytes(),
                limit,
            );
        }
    }
    println!("canonical-number-output: locked serde_json=1.0.150 zmij=1.0.23 sampled_max={longest} long_parse_valid_noncanonical=true");
}

#[test]
fn canonical_column_all_addresses_large_hash_utf8_and_late_errors() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    for column in COLUMNS {
        for length in [0, 1, FRAGMENT_BYTES, FRAGMENT_BYTES + 1, INPUT_BYTES + 1] {
            let raw = match length {
                0 => Vec::new(),
                1 => b"0".to_vec(),
                _ => format!("\"{}\"", "x".repeat(length - 2)).into_bytes(),
            };
            set_bytes(&c, column, &raw);
            let batches = compare(&mut c, &path, column, &raw, 64 * 1024);
            if length > FRAGMENT_BYTES {
                assert!(batches.len() > 1);
                assert_eq!(batches[0].input.fragment_bytes, FRAGMENT_BYTES);
            }
        }
        for scalar in ["é", "€", "🦀"] {
            for split in 1..scalar.len() {
                let mut raw = format!("\"{}", "x".repeat(FRAGMENT_BYTES - split - 1)).into_bytes();
                raw.extend_from_slice(scalar.as_bytes());
                raw.push(b'"');
                set_bytes(&c, column, &raw);
                let batches = compare(&mut c, &path, column, &raw, 64 * 1024);
                assert_eq!(batches.len(), 2);
                assert_eq!(
                    batches[1].input.retained_bytes,
                    identity_bytes(&identity()) + split
                );
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
                b"\\u001F".to_vec(),
            ] {
                let mut raw = vec![b'"'];
                if late {
                    raw.extend(std::iter::repeat_n(b'x', FRAGMENT_BYTES + 5));
                }
                raw.extend_from_slice(&tail);
                raw.push(b'"');
                assert!(!oracle(&raw).1);
                set_bytes(&c, column, &raw);
                let batches = compare(&mut c, &path, column, &raw, 64 * 1024);
                if late {
                    assert!(!batches.is_empty());
                }
            }
            for suffix in [b"".as_slice(), b"x", b" ", b",0", b"\\"] {
                let mut raw = format!(
                    "\"{}",
                    "x".repeat(if late { FRAGMENT_BYTES + 5 } else { 0 })
                )
                .into_bytes();
                if !suffix.is_empty() {
                    raw.push(b'"');
                    raw.extend_from_slice(suffix);
                }
                assert!(!oracle(&raw).1);
                set_bytes(&c, column, &raw);
                compare(&mut c, &path, column, &raw, 64 * 1024);
            }
        }
    }
}

fn key_source(prefix: usize, order: usize) -> Vec<u8> {
    let common = "x".repeat(prefix);
    let (left, right) = match order {
        0 => ("\\n", "A"),
        1 => ("A", "\\n"),
        _ => ("A", "A"),
    };
    format!("{{\"{common}{left}\":0,\"{common}{right}\":1}}").into_bytes()
}

#[test]
fn canonical_column_adjacent_decoded_keys_large_prefix_order_and_duplicate() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    for prefix in [17, FRAGMENT_BYTES + 3, INPUT_BYTES + 3] {
        for order in 0..3 {
            let raw = key_source(prefix, order);
            assert_eq!(oracle(&raw).1, order == 0);
            let columns = if prefix > INPUT_BYTES {
                &COLUMNS[..1]
            } else {
                &COLUMNS[..]
            };
            for &column in columns {
                set_bytes(&c, column, &raw);
                let batches = compare(&mut c, &path, column, &raw, 64 * 1024);
                if order == 0 {
                    // This fixed two-key shape must not regress to one decoded
                    // byte per full speculative chunk after a budget split.
                    assert!(batches.len() <= 2 * raw.len().div_ceil(FRAGMENT_BYTES) + 4);
                    assert!(
                        batches.iter().map(|u| u.key_read_bytes).sum::<usize>()
                            <= 2 * prefix + 4 * 64 * 1024
                    );
                    assert!(batches.iter().map(|u| u.key_read_bytes).sum::<usize>() >= 2 * prefix);
                    if prefix > FRAGMENT_BYTES {
                        assert!(batches
                            .iter()
                            .any(|u| u.key_read_bytes > 0 && u.main_consumed_bytes == 0));
                    }
                    println!("canonical-keys: column={column:?} prefix={prefix} calls={} main_read={} key_read={} consumed={}", batches.len(), batches.iter().map(|u| u.main_read_bytes).sum::<usize>(), batches.iter().map(|u| u.key_read_bytes).sum::<usize>(), raw.len());
                }
            }
        }
    }
}

#[test]
fn canonical_column_discarded_suffix_work_budget_and_tight_metadata_progress() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    let raw = canonical_json_v1(Value::Array(
        (0..400)
            .map(|n| json!({format!("{n:04}"):n,"z":0}))
            .collect(),
    ))
    .unwrap()
    .into_bytes();
    set_bytes(&c, RawColumn::EventPayload, &raw);
    let batches = compare(&mut c, &path, RawColumn::EventPayload, &raw, 64 * 1024);
    assert!(batches.len() > 1);
    assert!(batches.iter().any(|u| u.input.work_units >= 98));
    assert!(
        batches.iter().map(|u| u.main_read_bytes).sum::<usize>() > raw.len(),
        "discarded suffix must be counted on every reread"
    );

    // Matched identity is charged twice; leave two fresh bytes per call.
    let mut expected = identity();
    expected
        .event_id
        .push_str(&"m".repeat(INPUT_BYTES / 2 - identity_bytes(&expected) - 1));
    alter(&c, "UPDATE events SET event_id=?1", [&expected.event_id]);
    for raw in [
        b"1".as_slice(),
        b"12",
        b"{\"a\":0,\"b\":1}",
        b"{\"a\":0,\"aa\":1}",
    ] {
        set_bytes(&c, RawColumn::EventPayload, raw);
        let before = snapshot(&c);
        let file = fs::read(&path).unwrap();
        {
            let mut s = CanonicalColumnSession::start(
                &mut c,
                RawColumn::EventPayload,
                SourceIdentity {
                    event_id: expected.event_id.clone(),
                    ..identity()
                },
            )
            .unwrap();
            let mut done = false;
            for _ in 0..100 {
                match s.step(&mut || false).unwrap() {
                    CanonicalColumnStep::Pending(u) => usage(&u),
                    CanonicalColumnStep::CanonicalComplete {
                        sha256, usage: u, ..
                    } => {
                        usage(&u);
                        assert_eq!(sha256, <[u8; 32]>::from(Sha256::digest(raw)));
                        done = true;
                        break;
                    }
                    other => panic!("tight progress {other:?}"),
                }
            }
            assert!(done);
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
    }
    // Parser UTF-8/number carry is retained input too; it must cause a terminal
    // intrinsic refusal, never an endless zero-progress Pending retry.
    for raw in ["123", "\"🦀\""] {
        assert!(oracle(raw.as_bytes()).1);
        set_bytes(&c, RawColumn::EventPayload, raw.as_bytes());
        let before = snapshot(&c);
        let file = fs::read(&path).unwrap();
        {
            let mut s = CanonicalColumnSession::start(
                &mut c,
                RawColumn::EventPayload,
                SourceIdentity {
                    event_id: expected.event_id.clone(),
                    ..identity()
                },
            )
            .unwrap();
            let mut failed = false;
            for _ in 0..4 {
                match s.step(&mut || false) {
                    Ok(CanonicalColumnStep::Pending(u)) => usage(&u),
                    Err(error) => {
                        assert_eq!(error.kind, "storageSchemaMismatch");
                        failed = true;
                        break;
                    }
                    other => panic!("intrinsic carry {other:?}"),
                }
            }
            assert!(failed);
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
    }
}

#[test]
fn canonical_column_tiny_fresh_key_comparison_unequal_escape_widths() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    // Different escape widths make one decoded byte wait while the other key
    // consumes escape carry. Fresh space must remain available to that side.
    for fresh in [10, 12, 14, 16] {
        let mut expected = identity();
        expected
            .event_id
            .push_str(&"m".repeat(INPUT_BYTES / 2 - identity_bytes(&expected) - fresh / 2));
        alter(&c, "UPDATE events SET event_id=?1", [&expected.event_id]);
        for raw in [
            r#"{"\bZ":0,"\u000bZ":1}"#,
            r#"{"\u0000":0,"\b":1}"#,
            r#"{"a\u001f":0,"a\"":1}"#,
            r#"{"\n":0,"A":1}"#,
        ] {
            assert!(oracle(raw.as_bytes()).1);
            set_bytes(&c, RawColumn::EventPayload, raw.as_bytes());
            let before = snapshot(&c);
            let file = fs::read(&path).unwrap();
            for limit in [1, 2, 3, 64 * 1024] {
                let mut s = CanonicalColumnSession::start(
                    &mut c,
                    RawColumn::EventPayload,
                    SourceIdentity {
                        event_id: expected.event_id.clone(),
                        ..identity()
                    },
                )
                .unwrap();
                s.fragment_limit_for_test(limit);
                let mut done = false;
                for _ in 0..100 {
                    match s.step(&mut || false).unwrap() {
                        CanonicalColumnStep::Pending(u) => usage(&u),
                        CanonicalColumnStep::CanonicalComplete {
                            sha256, usage: u, ..
                        } => {
                            usage(&u);
                            assert_eq!(sha256, <[u8; 32]>::from(Sha256::digest(raw.as_bytes())));
                            done = true;
                            break;
                        }
                        other => panic!("fresh={fresh} limit={limit}: {other:?}"),
                    }
                }
                assert!(done, "fresh={fresh} limit={limit} raw={raw}");
                assert!(s.step(&mut || false).is_err());
            }
            unchanged(&c, &path, &before, &file);
        }
    }
}

fn at_phase(c: &mut Connection, column: RawColumn, key: bool) -> CanonicalColumnSession<'_> {
    let mut s = CanonicalColumnSession::start(c, column, identity()).unwrap();
    for _ in 0..16 {
        let CanonicalColumnStep::Pending(u) = s.step(&mut || false).unwrap() else {
            panic!("expected retained phase");
        };
        usage(&u);
        if s.usage_for_test().2.unwrap().1.is_some() == key {
            return s;
        }
    }
    panic!("did not reach retained key comparison");
}

#[test]
fn canonical_column_main_and_key_cancellation_entry_mid_precommit_discard() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    let raw = key_source(FRAGMENT_BYTES + 7, 0);
    for column in COLUMNS {
        set_bytes(&c, column, &raw);
        let before = snapshot(&c);
        let file = fs::read(&path).unwrap();
        for key in [false, true] {
            let mut last_poll = 0;
            {
                let mut s = at_phase(&mut c, column, key);
                s.step(&mut || {
                    last_poll += 1;
                    false
                })
                .unwrap();
            }
            assert!(last_poll >= 4);
            for cancel_at in [1, 2, 4, last_poll] {
                {
                    let mut s = at_phase(&mut c, column, key);
                    let mut polls = 0;
                    assert_eq!(
                        s.step(&mut || {
                            polls += 1;
                            polls == cancel_at
                        })
                        .unwrap(),
                        CanonicalColumnStep::Cancelled
                    );
                    assert_eq!(polls, cancel_at);
                    assert!(s.usage_for_test().2.is_none());
                    assert!(s.autocommit_for_test());
                    usage(s.usage_for_test().1);
                    if cancel_at >= 2 {
                        let u = s.usage_for_test().1;
                        assert!(if key {
                            u.key_read_bytes > 0
                        } else {
                            u.main_read_bytes > 0
                        });
                    }
                    if cancel_at >= 4 && !key {
                        assert!(s.usage_for_test().1.main_consumed_bytes > 0);
                    }
                    assert!(s.step(&mut || false).is_err());
                }
                unchanged(&c, &path, &before, &file);
            }
        }
        // Dropping a retained main/key session is not a durable resume token.
        for key in [false, true] {
            drop(at_phase(&mut c, column, key));
        }
        compare(&mut c, &path, column, &raw, 64 * 1024);
    }
}

#[test]
fn canonical_column_external_epoch_same_length_during_main_key_and_after_terminal() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    let other = connection(&path);
    for column in COLUMNS {
        for key in [false, true] {
            let mut raw = key_source(FRAGMENT_BYTES + 7, 0);
            set_bytes(&c, column, &raw);
            let before;
            let file;
            {
                let mut s = at_phase(&mut c, column, key);
                let last_value = raw.len() - 2;
                raw[last_value] = b'2'; // same byte length, late source change
                set_bytes(&other, column, &raw);
                // Exclude the intentional external write from zero-mutation
                // evidence: snapshot AFTER its commit, before verifier step.
                before = snapshot(&other);
                file = fs::read(&path).unwrap();
                assert_eq!(
                    s.step(&mut || false).unwrap_err().kind,
                    "storageSchemaMismatch"
                );
                assert!(s.usage_for_test().2.is_none());
                assert!(s.autocommit_for_test());
                assert!(s.step(&mut || false).is_err());
            }
            unchanged(&c, &path, &before, &file);
            compare(&mut c, &path, column, &raw, 64 * 1024);
        }
        set_bytes(&c, column, b"0");
        let before;
        let file;
        {
            let mut s = CanonicalColumnSession::start(&mut c, column, identity()).unwrap();
            assert!(matches!(
                s.step(&mut || false).unwrap(),
                CanonicalColumnStep::CanonicalComplete { .. }
            ));
            set_bytes(&other, column, b"1");
            before = snapshot(&other);
            file = fs::read(&path).unwrap();
            assert!(s.step(&mut || false).is_err());
        }
        unchanged(&c, &path, &before, &file);
    }
}

// Actual SQLite authorization faults. No production failure-injection hook.
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
            // SAFETY: the hook's context is stable and unregistered before drop;
            // SQLite arguments are NUL-terminated, with NULL handled explicitly.
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
                    && [b"events".as_slice(), b"frozen_operation_sources"].contains(&first)
            {
                rusqlite::ffi::SQLITE_DENY
            } else {
                rusqlite::ffi::SQLITE_OK
            }
        }
        let mode = Box::new(std::sync::atomic::AtomicU8::new(0));
        // SAFETY: tests drop this hook before dropping the owning Connection.
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
        // SAFETY: Connection remains alive until after this hook is dropped.
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(self.handle, None, std::ptr::null_mut());
        }
    }
}

#[test]
fn canonical_column_real_read_commit_and_busy_failures_at_start_main_and_key() {
    let (_root, path) = fixture();
    let mut c = connection(&path);
    let other = connection(&path);
    let fault = SqlFault::install(&c);
    let raw = key_source(FRAGMENT_BYTES + 7, 0);
    for column in COLUMNS {
        set_bytes(&c, column, &raw);
        let before = snapshot(&c);
        let file = fs::read(&path).unwrap();
        for mode in [1, 2] {
            fault.set(mode);
            assert_eq!(
                CanonicalColumnSession::start(&mut c, column, identity())
                    .err()
                    .unwrap()
                    .kind,
                "storageSchemaMismatch"
            );
            fault.set(0);
            unchanged(&c, &path, &before, &file);
        }
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(CanonicalColumnSession::start(&mut c, column, identity()).is_err());
        other.execute_batch("ROLLBACK").unwrap();
        unchanged(&c, &path, &before, &file);
        for key in [false, true] {
            for mode in [1, 2, 3] {
                {
                    let mut s = at_phase(&mut c, column, key);
                    if mode == 1 {
                        fault.set(1);
                    }
                    if mode == 3 {
                        other.execute_batch("BEGIN IMMEDIATE").unwrap();
                    }
                    let mut polls = 0;
                    let error = s
                        .step(&mut || {
                            polls += 1;
                            // Allow metadata + first actual source span, then deny
                            // the next span in this main/key phase, not preflight.
                            if mode == 2 && polls == 2 {
                                fault.set(2);
                            }
                            false
                        })
                        .unwrap_err();
                    assert_eq!(error.kind, "storageSchemaMismatch");
                    fault.set(0);
                    if mode == 3 {
                        other.execute_batch("ROLLBACK").unwrap();
                    }
                    assert!(s.usage_for_test().2.is_none());
                    assert!(s.autocommit_for_test());
                    usage(s.usage_for_test().1);
                    if mode != 3 {
                        let u = s.usage_for_test().1;
                        assert!(if key {
                            u.key_read_bytes > 0
                        } else {
                            u.main_read_bytes > 0
                        });
                    }
                    assert!(s.step(&mut || false).is_err());
                }
                unchanged(&c, &path, &before, &file);
            }
        }
    }
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(CanonicalColumnSession::start(&mut c, RawColumn::EventPayload, identity()).is_err());
    assert!(!c.is_autocommit()); // do not rollback a caller-owned transaction
    c.execute_batch("ROLLBACK").unwrap();
}
