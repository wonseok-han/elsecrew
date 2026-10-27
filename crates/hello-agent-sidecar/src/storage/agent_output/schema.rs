//! Atomic V10 -> V11 schema preparation. Product open does not call this yet.
//! Existing run arrays remain legacy; only new runs will opt into output rows.
use super::*;
use crate::storage::{
    schema_identity_v10_golden, verify_schema_v10, verify_v6_materialization_snapshot,
};
use rusqlite::{Connection, TransactionBehavior};

pub(in crate::storage) const VERSION: u32 =
    hello_agent_protocol::storage_lifetime_v1::STORAGE_LIFETIME_SCHEMA_VERSION_V11;
const META: &str = "CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 11),
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    migration_state TEXT NOT NULL CHECK (migration_state = 'ready'),
    materialization_state TEXT NOT NULL CHECK (
        materialization_state IN ('pending', 'materializing', 'ready', 'failed')
    ),
    migrated_at TEXT NOT NULL
) STRICT";

type Identity = (String, String, String, String);

pub(in crate::storage) fn expected_schema() -> Result<Vec<Identity>, SidecarError> {
    // The frozen V10 tuples contain SQL digests, not SQL source. Recover the
    // trusted reference DDL and cross-check its digests before extending it.
    let mut expected: Vec<Identity> = crate::storage::reference_schema_v10()?
        .into_iter()
        .map(|(kind, name, table, sql)| {
            Ok((
                kind,
                name,
                table,
                sql.ok_or_else(|| invalid("missing reference SQL"))?,
            ))
        })
        .collect::<Result<_, SidecarError>>()?;
    let digests: Vec<Identity> = expected
        .iter()
        .map(|(kind, name, table, sql)| {
            (
                kind.clone(),
                name.clone(),
                table.clone(),
                hello_agent_protocol::project_v1::sha256_file_version_v1(sql.as_bytes())
                    .trim_start_matches("sha256:")
                    .to_owned(),
            )
        })
        .collect();
    if digests != schema_identity_v10_golden()? {
        return Err(invalid("reference V10 schema differs from frozen identity"));
    }
    let metadata = expected
        .iter_mut()
        .find(|row| row.0 == "table" && row.1 == "storage_schema_meta")
        .ok_or_else(|| invalid("missing metadata schema identity"))?;
    metadata.3 = META.to_string();
    // Let SQLite normalize the exact new DDL in the same manner as sqlite_schema.
    let reference = Connection::open_in_memory().map_err(sqlite_error)?;
    reference.execute_batch(SCHEMA).map_err(sqlite_error)?;
    let mut statement = reference.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_autoindex%'").map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(sqlite_error)?;
    for row in rows {
        expected.push(row.map_err(sqlite_error)?);
    }
    expected.sort();
    Ok(expected)
}

pub(in crate::storage) fn verify(connection: &Connection) -> Result<String, SidecarError> {
    let expected = expected_schema()?;
    // Bound schema input before String decoding; an extra object is an error.
    let mut statement = connection.prepare("SELECT CASE WHEN length(CAST(type AS BLOB))<=64 AND length(CAST(name AS BLOB))<=256 AND length(CAST(tbl_name AS BLOB))<=256 AND length(CAST(sql AS BLOB))<=65536 THEN 1 ELSE 0 END, type,name,tbl_name, CASE WHEN length(CAST(sql AS BLOB))<=65536 THEN sql ELSE NULL END FROM sqlite_schema WHERE name NOT LIKE 'sqlite_autoindex%' ORDER BY type,name LIMIT ?1").map_err(sqlite_error)?;
    let mut rows = statement
        .query([(expected.len() + 1) as i64])
        .map_err(sqlite_error)?;
    let mut index = 0;
    while let Some(row) = rows.next().map_err(sqlite_error)? {
        if !row.get::<_, bool>(0).map_err(sqlite_error)? || index >= expected.len() {
            return Err(invalid("output schema identity mismatch"));
        }
        let actual: Identity = (
            row.get(1).map_err(sqlite_error)?,
            row.get(2).map_err(sqlite_error)?,
            row.get(3).map_err(sqlite_error)?,
            row.get(4).map_err(sqlite_error)?,
        );
        if actual != expected[index] {
            return Err(invalid("output schema identity mismatch"));
        }
        index += 1;
    }
    if index != expected.len() {
        return Err(invalid("output schema object missing"));
    }
    let (version, protocol, state, materialization, timestamp_valid): (u32,u32,String,String,bool) = connection.query_row(
        "SELECT storage_schema_version,protocol_version,migration_state,materialization_state, migrated_at=strftime('%Y-%m-%d %H:%M:%S',migrated_at) FROM storage_schema_meta WHERE singleton=1",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).map_err(sqlite_error)?;
    let count: i64 = connection
        .query_row("SELECT count(*) FROM storage_schema_meta", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error)?;
    let foreign_keys: bool = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if version != VERSION
        || protocol != 1
        || state != "ready"
        || !timestamp_valid
        || count != 1
        || !foreign_keys
    {
        return Err(invalid("output schema metadata mismatch"));
    }
    verify_v6_materialization_snapshot(connection, &materialization, false)?;
    Ok(materialization)
}

#[cfg(test)]
pub(in crate::storage) fn migrate(connection: &mut Connection) -> Result<(), SidecarError> {
    migrate_cancellable(connection, &mut || false)
}

pub(in crate::storage) fn migrate_cancellable(
    connection: &mut Connection,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SidecarError> {
    let mut check = || {
        if cancelled() {
            Err(SidecarError::new("storageAdmissionCancelled", "output migration cancelled"))
        } else { Ok(()) }
    };
    check()?;
    if !connection.is_autocommit() {
        return Err(invalid("migration requires idle connection"));
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    verify_schema_v10(&tx)?;
    let (materialization, migrated_at): (String, String) = tx
        .query_row(
            "SELECT materialization_state,migrated_at FROM storage_schema_meta WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sqlite_error)?;
    tx.execute_batch(SCHEMA).map_err(sqlite_error)?;
    check()?;
    // No ALTER RENAME: existing trigger references retain the metadata name.
    // Replacement is invisible outside this transaction and rolls back on error.
    tx.execute_batch("DROP TABLE storage_schema_meta")
        .map_err(sqlite_error)?;
    tx.execute_batch(META).map_err(sqlite_error)?;
    tx.execute(
        "INSERT INTO storage_schema_meta VALUES (1,11,1,'ready',?1,?2)",
        params![materialization, migrated_at],
    )
    .map_err(sqlite_error)?;
    check()?;
    verify(&tx)?;
    check()?;
    tx.commit().map_err(sqlite_error)
}

/// Compose existing ledger admission and output closure under one unchanged
/// connection epoch. This prepares the future product-open integration only.
pub(in crate::storage) fn verify_admission(
    connection: &mut Connection,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SidecarError> {
    use crate::storage::reopen::snapshot::{ReopenMaintenanceSession, ReopenMaintenanceStep};
    if cancelled() || !connection.is_autocommit() {
        return Err(invalid("output admission interrupted"));
    }
    let marker: i64 = connection
        .query_row("PRAGMA data_version", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    {
        let mut existing = ReopenMaintenanceSession::start_for_admission(connection)?;
        loop {
            match existing.step(cancelled)? {
                ReopenMaintenanceStep::Pending(_) => {}
                ReopenMaintenanceStep::Cancelled => {
                    return Err(invalid("output admission cancelled"))
                }
                ReopenMaintenanceStep::ReadyVerified(_) => break,
            }
        }
    }
    {
        let mut output = super::verification::AllOutputVerification::start(connection)?;
        while !output.step(cancelled)? {}
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let current: i64 = tx
        .query_row("PRAGMA data_version", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if cancelled() || marker != current {
        return Err(invalid("output admission source changed"));
    }
    verify(&tx)?;
    tx.commit().map_err(sqlite_error)
}

#[cfg(test)]
mod tests {
    use super::migrate;
    use super::*;
    use crate::storage::*;

    fn v10() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(MIGRATION_V1).unwrap();
        db.execute_batch(MIGRATION_V2).unwrap();
        for migration in [
            MIGRATION_V3,
            MIGRATION_V4,
            MIGRATION_V5,
            MIGRATION_V6,
            MIGRATION_V7,
            MIGRATION_V8,
            MIGRATION_V9,
            MIGRATION_V10,
        ] {
            execute_migration(&mut db, migration, "output migration fixture").unwrap();
        }
        db
    }

    #[test]
    fn output_migration_cancellation_rolls_back_every_boundary() {
        for boundary in 1..=4 {
            let mut db = v10();
            let before: String = db.query_row(
                "SELECT migrated_at FROM storage_schema_meta", [], |row| row.get(0),
            ).unwrap();
            let mut calls = 0;
            assert!(migrate_cancellable(&mut db, &mut || {
                calls += 1;
                calls == boundary
            }).is_err());
            assert_eq!(calls, boundary);
            assert!(db.is_autocommit());
            verify_schema_v10(&db).unwrap();
            let after: String = db.query_row(
                "SELECT migrated_at FROM storage_schema_meta", [], |row| row.get(0),
            ).unwrap();
            assert_eq!(before, after);
            let tables: i64 = db.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name IN ('run_output_heads','run_output_records')",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(tables, 0);
            migrate(&mut db).unwrap();
            verify_admission(&mut db, &mut || false).unwrap();
        }
    }

    #[test]
    fn preserves_legacy_schema_and_adds_empty_output_ledger() {
        let mut db = v10();
        let triggers_before: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='trigger'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        migrate(&mut db).unwrap();
        verify(&db).unwrap();
        verify_admission(&mut db, &mut || false).unwrap();
        assert!(verify_schema_v10(&db).is_err());
        let count: i64 = db
            .query_row("SELECT count(*) FROM run_output_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        let triggers_after: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='trigger'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(triggers_before, triggers_after);
        assert!(migrate(&mut db).is_err());
        verify(&db).unwrap();
    }

    #[test]
    fn output_lifecycle_rolls_back_with_run_projection() {
        let mut db = v10();
        migrate(&mut db).unwrap();
        let envelope: Value = serde_json::from_str(
            include_str!(
                "../../../../../tests/fixtures/m5/protocol-v1/valid/agent-run-state-changed.jsonl"
            )
            .lines()
            .last()
            .unwrap(),
        )
        .unwrap();
        let record = envelope["payload"]["run"].clone();
        let mut projection = serde_json::json!({"run_id":"agent_run_01","state":"running","source_kind":"actualAgent","output_format_version":1,"record":record}).as_object().unwrap().clone();
        let tx = db.transaction().unwrap();
        tx.execute("INSERT INTO runs(run_id,state,projection_json,last_event_sequence) VALUES ('agent_run_01','running',?1,1)", [encode(&projection).unwrap()]).unwrap();
        sync_run_projection(&tx, &projection, "agent_run_01", 1).unwrap();
        let binding: AgentRunBindingV1 = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/m5/agent-output-page.json"
        ))
        .map(|page: OutputPage| page.request.binding)
        .unwrap();
        assert!(!head(&tx, &binding).unwrap().sealed);
        projection.insert("state".into(), Value::String("succeeded".into()));
        projection.get_mut("record").unwrap()["state"] = Value::String("succeeded".into());
        projection.get_mut("record").unwrap()["terminationOutcome"] =
            Value::String("cleanExit".into());
        tx.execute(
            "UPDATE runs SET state='succeeded',projection_json=?1,last_event_sequence=2",
            [encode(&projection).unwrap()],
        )
        .unwrap();
        sync_run_projection(&tx, &projection, "agent_run_01", 2).unwrap();
        assert!(head(&tx, &binding).unwrap().sealed);
        tx.rollback().unwrap();
        let runs: i64 = db
            .query_row("SELECT count(*) FROM runs", [], |row| row.get(0))
            .unwrap();
        let heads: i64 = db
            .query_row("SELECT count(*) FROM run_output_heads", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((runs, heads), (0, 0));
    }

    #[test]
    fn rejects_drift_before_creating_output_tables() {
        let mut db = v10();
        db.execute_batch("CREATE TABLE unexpected(value TEXT)")
            .unwrap();
        assert!(migrate(&mut db).is_err());
        let version: u32 = db
            .query_row(
                "SELECT storage_schema_version FROM storage_schema_meta",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 10);
        let tables: i64 = db.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('run_output_heads','run_output_records')", [], |row| row.get(0)).unwrap();
        assert_eq!(tables, 0);
    }
}
