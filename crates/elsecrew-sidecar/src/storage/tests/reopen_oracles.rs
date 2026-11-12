//! Actual V8 acceptance oracles, not a change to product validation policy.
//! Legacy replay requires BOTH serde equality and the SQLite column mirror.
//! R3A is an informational private verifier and does not replace that gate.

use super::*;

const STAMP: &str = "2026-09-03T01:00:00Z";

// Follow reopen_projection::seed_mixed's ordinary five-aggregate fixture. The
// shared parent helpers keep chat/source-index materialization valid. No source
// JSON, ledger identity, digest, schema or typed column is patched by JSON cases.
fn fixture(probe: Value) -> (TempDir, PathBuf) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for (aggregate, id, payload) in [
        (
            "app_instance",
            "app_oracle",
            app_payload("app_oracle", json!(1)),
        ),
        (
            "project",
            "project_oracle",
            json!({"project_id":"project_oracle","canonical_path":"/fixture/oracle","probe":probe}),
        ),
        (
            "chat",
            "chat_oracle",
            materializable_chat_payload("chat_oracle", "project_oracle", 0, Value::Null),
        ),
        (
            "run",
            "run_oracle",
            json!({"run_id":"run_oracle","chat_id":null,"state":"succeeded","worktree_path":null}),
        ),
        (
            "lock",
            "lock_oracle",
            json!({"lock_id":"lock_oracle","project_id":"project_oracle","writer_run_id":null,"worktree_path":"/fixture/oracle/worktree","poisoned":true,"recovery_required":false,"evidence":{"a":"a","b":2}}),
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
                    STAMP,
                ))
                .unwrap(),
            AppendOutcome::Committed
        );
    }
    run_bounded_materializer_to_ready(&mut storage);
    assert_eq!(verify_schema_v8(&storage.connection).unwrap(), "ready");
    verify_projection_replay(&storage.connection).unwrap();
    storage.checkpoint().unwrap();
    drop(storage);
    // Establish an actual legacy-readable V8 before any deliberate alteration.
    drop(Storage::open(&path).unwrap());
    (root, path)
}

fn app_payload(id: &str, pid: Value) -> Value {
    json!({"sidecar_instance_id":id,"pid":pid,"platform":"macos","started_at":STAMP,"heartbeat_at":STAMP,"clean_shutdown_at":null})
}

fn project_json(connection: &Connection) -> String {
    connection
        .query_row(
            "SELECT projection_json FROM projects WHERE project_id='project_oracle'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn lock_json(connection: &Connection) -> String {
    connection
        .query_row(
            "SELECT projection_json FROM locks WHERE lock_id='lock_oracle'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

// Only used on the ordinary writer's original canonical object, before adding
// duplicates. Keep the replacement text literal rather than normalizing it.
fn member(source: &str, key: &str, replacement: &str) -> String {
    let parsed: Value = serde_json::from_str(source).unwrap();
    let old = format!("\"{key}\":{}", serde_json::to_string(&parsed[key]).unwrap());
    assert_eq!(source.matches(&old).count(), 1);
    source.replacen(&old, replacement, 1)
}

fn set_project(connection: &Connection, source: &str) {
    assert_eq!(
        connection
            .execute(
                "UPDATE projects SET projection_json=?1 WHERE project_id='project_oracle'",
                [source],
            )
            .unwrap(),
        1
    );
}

fn set_lock(connection: &Connection, source: &str) {
    assert_eq!(
        connection
            .execute(
                "UPDATE locks SET projection_json=?1 WHERE lock_id='lock_oracle'",
                [source],
            )
            .unwrap(),
        1
    );
}

fn private_r3a(connection: &mut Connection) -> Result<(), SidecarError> {
    let mut session =
        FrozenOperationVerificationSession::start_ledger_projection(connection).unwrap();
    for _ in 0..6 {
        let result = session.step(&mut || false);
        let usage = match result {
            Ok(FrozenOperationVerificationStep::LedgerProjectionPending { usage, .. }) => usage,
            Ok(FrozenOperationVerificationStep::LedgerProjectionVerified(usage)) => {
                assert!(usage.rows <= 100);
                assert!(session.step(&mut || false).is_err());
                return Ok(());
            }
            Err(error) => {
                assert_eq!(error.kind, "storageSchemaMismatch");
                assert!(session.step(&mut || false).is_err());
                return Err(error);
            }
            other => panic!("unexpected R3A result {other:?}"),
        };
        assert!(usage.rows <= 100);
        assert!(usage.canonical_source_bytes <= 1024 * 1024);
        assert!(usage.encoded_closure_bytes <= 8 * 1024 * 1024);
    }
    panic!("small five-row fixture did not finish all six R3A phases");
}

fn assert_legacy(result: Result<(), SidecarError>, expected: bool, name: &str) {
    if expected {
        result.unwrap_or_else(|error| panic!("{name}: {error:?}"));
    } else {
        assert_eq!(result.unwrap_err().kind, "projectionMismatch", "{name}");
    }
}

// Expectations below follow the existing loaders: serde keeps the final key,
// SQLite json_extract sees the first key, json(...) preserves key/escape text,
// and both projection loaders overlay the typed sequence. They are assertions,
// not a snapshot of whichever verdict an implementation happens to produce.
fn observe(path: &Path, name: &str, serde_equal: bool, mirror: bool, r3a: bool) {
    let mut connection = Connection::open(path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    let before = database_snapshot(&connection);
    let bytes = fs::read(path).unwrap();
    verify_schema_v8_for_open(&connection).unwrap();
    verify_integrity(&connection).unwrap();
    let events = load_events(&connection).unwrap();
    assert_eq!(events.len(), 5, "{name}");
    assert_eq!(
        replay_storage_ledger(&connection, &events).unwrap()
            == load_projection(&connection).unwrap(),
        serde_equal,
        "{name}: serde replay comparison"
    );
    assert_legacy(verify_projection_columns(&connection), mirror, name);
    assert_legacy(
        verify_projection_replay(&connection),
        serde_equal && mirror,
        name,
    );
    assert_eq!(
        database_snapshot(&connection),
        before,
        "{name}: legacy reads"
    );
    assert_eq!(fs::read(path).unwrap(), bytes, "{name}: legacy file");
    assert_eq!(private_r3a(&mut connection).is_ok(), r3a, "{name}: R3A");
    assert!(connection.is_autocommit());
    assert_eq!(database_snapshot(&connection), before, "{name}: R3A reads");
    assert_eq!(fs::read(path).unwrap(), bytes, "{name}: R3A file");
    drop(connection);

    // Storage::open is the unchanged production path, including its mirror.
    assert_legacy(Storage::open(path).map(drop), serde_equal && mirror, name);
    let connection = Connection::open(path).unwrap();
    assert_eq!(
        database_snapshot(&connection),
        before,
        "{name}: reopen reads"
    );
    assert_eq!(fs::read(path).unwrap(), bytes, "{name}: reopen file");
    println!(
        "{name}: serde={serde_equal} sqlite_mirror={mirror} legacy_replay={} storage_reopen={} private_r3a={r3a} unchanged=true",
        serde_equal && mirror,
        serde_equal && mirror,
    );
}

#[test]
fn s2_legacy_oracle_duplicate_first_last_and_nonindexed_values() {
    for (name, key, replacement, serde_equal, mirror, r3a) in [
        (
            "indexed-first-wrong-last-correct",
            "project_id",
            r#""project_id":"wrong","project_id":"project_oracle""#,
            true,
            false,
            true,
        ),
        (
            "indexed-first-correct-last-wrong",
            "project_id",
            r#""project_id":"project_oracle","project_id":"wrong""#,
            false,
            true,
            false,
        ),
        (
            "indexed-identical-duplicate",
            "project_id",
            r#""project_id":"project_oracle","project_id":"project_oracle""#,
            true,
            true,
            true,
        ),
        (
            "nonindexed-first-wrong-last-correct",
            "probe",
            r#""probe":2,"probe":1"#,
            true,
            true,
            true,
        ),
        (
            "nonindexed-first-correct-last-wrong",
            "probe",
            r#""probe":1,"probe":2"#,
            false,
            true,
            false,
        ),
        (
            "nonindexed-identical-duplicate",
            "probe",
            r#""probe":1,"probe":1"#,
            true,
            true,
            true,
        ),
    ] {
        let (_root, path) = fixture(json!(1));
        let connection = Connection::open(&path).unwrap();
        set_project(
            &connection,
            &member(&project_json(&connection), key, replacement),
        );
        drop(connection);
        observe(&path, name, serde_equal, mirror, r3a);
    }
}

#[test]
fn s2_legacy_oracle_evidence_key_order_escape_and_whitespace() {
    for (label, evidence, mirror_if_one_side) in [
        ("key-order", r#"{"b":2,"a":"a"}"#, false),
        ("escape", r#"{"a":"\u0061","b":2}"#, false),
        ("whitespace", " { \"a\" : \"a\", \"b\" : 2 } ", true),
    ] {
        for side in ["projection", "column", "both"] {
            let (_root, path) = fixture(json!(1));
            let connection = Connection::open(&path).unwrap();
            if side != "column" {
                set_lock(
                    &connection,
                    &member(
                        &lock_json(&connection),
                        "evidence",
                        &format!("\"evidence\":{evidence}"),
                    ),
                );
            }
            if side != "projection" {
                assert_eq!(
                    connection
                        .execute(
                            "UPDATE locks SET evidence_json=?1 WHERE lock_id='lock_oracle'",
                            [evidence]
                        )
                        .unwrap(),
                    1
                );
            }
            drop(connection);
            observe(
                &path,
                &format!("evidence-{label}-{side}"),
                true,
                side == "both" || mirror_if_one_side,
                true,
            );
        }
    }
}

#[test]
fn s2_legacy_oracle_typed_sequence_overlay_and_drift() {
    for raw_sequence in ["1", "999", "1.0", "true", "null", "\"untrusted\""] {
        let (_root, path) = fixture(json!(1));
        let connection = Connection::open(&path).unwrap();
        let original = project_json(&connection);
        assert!(serde_json::from_str::<Value>(&original)
            .unwrap()
            .get("last_event_sequence")
            .is_none());
        let altered = format!(
            "{{\"last_event_sequence\":{raw_sequence},{}",
            &original[1..]
        );
        set_project(&connection, &altered);
        drop(connection);
        observe(
            &path,
            &format!("sequence-overlay-{raw_sequence}"),
            true,
            true,
            true,
        );
    }
    let (_root, path) = fixture(json!(1));
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE projects SET last_event_sequence=2 WHERE project_id='project_oracle'",
                []
            )
            .unwrap(),
        1
    );
    drop(connection);
    observe(&path, "typed-sequence-drift", false, true, false);
}

#[test]
fn s2_legacy_oracle_nonindexed_scalar_types_are_not_coerced() {
    // Project's nonindexed extension is retained by the reducer and accepts all
    // four types through ordinary append. It is not a bool/pid type violation.
    let values = [
        ("integer", "1"),
        ("float", "1.0"),
        ("bool", "true"),
        ("null", "null"),
    ];
    for (expected_name, expected_json) in values {
        for (actual_name, actual_json) in values {
            let (_root, path) = fixture(serde_json::from_str(expected_json).unwrap());
            let connection = Connection::open(&path).unwrap();
            set_project(
                &connection,
                &member(
                    &project_json(&connection),
                    "probe",
                    &format!("\"probe\":{actual_json}"),
                ),
            );
            drop(connection);
            let equal = expected_name == actual_name;
            observe(
                &path,
                &format!("scalar-{expected_name}-to-{actual_name}"),
                equal,
                true,
                equal,
            );
        }
    }
}

#[test]
fn s2_legacy_oracle_indexed_boolean_sql_coercion_is_not_replay_equality() {
    // Ordinary lock writes require bool; JSON 1/1.0 below are deliberate
    // projection corruption, NOT supposedly valid typed source values.
    for (raw, equal, mirror) in [
        ("true", true, true),
        ("1", false, true),
        ("1.0", false, true),
        ("null", false, false),
    ] {
        let (_root, path) = fixture(json!(1));
        let connection = Connection::open(&path).unwrap();
        set_lock(
            &connection,
            &member(
                &lock_json(&connection),
                "poisoned",
                &format!("\"poisoned\":{raw}"),
            ),
        );
        drop(connection);
        observe(
            &path,
            &format!("indexed-bool-to-{raw}"),
            equal,
            mirror,
            equal,
        );
    }
}

#[test]
fn s2_legacy_oracle_numeric_index_uses_valid_pid_and_rejects_invalid_sources() {
    let (_root, path) = fixture(json!(1));
    observe(
        &path,
        "valid-indexed-pid-1-and-nullable-columns",
        true,
        true,
        true,
    );
    let mut storage = Storage::open(&path).unwrap();
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT pid FROM app_instances WHERE sidecar_instance_id='app_oracle'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    for raw in ["1.0", "true", "null"] {
        storage.checkpoint().unwrap();
        let before = database_snapshot(&storage.connection);
        let bytes = fs::read(&path).unwrap();
        let error = storage
            .append_event(&event(
                "evt_invalid_pid",
                "op_invalid_pid",
                "app_instance",
                "app_invalid_pid",
                1,
                app_payload("app_invalid_pid", serde_json::from_str(raw).unwrap()),
                STAMP,
            ))
            .unwrap_err();
        assert_eq!(error.kind, "projectionInvalid");
        assert!(error.detail.contains("pid"));
        assert!(storage.connection.is_autocommit());
        assert_eq!(database_snapshot(&storage.connection), before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        println!("invalid-indexed-pid-{raw}: ordinary_append=projectionInvalid unchanged=true; not a valid source acceptance case");
    }
}
