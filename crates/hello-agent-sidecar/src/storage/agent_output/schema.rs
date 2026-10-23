//! Atomic V10 -> V11 schema preparation. Product open does not call this yet.
//! Existing run arrays remain legacy; only new runs will opt into output rows.
use super::*;
use crate::storage::{
    schema_identity_v10_golden, verify_schema_v10, verify_v6_materialization_snapshot,
};
use rusqlite::{Connection, TransactionBehavior};

pub(in crate::storage) const VERSION: u32 = 11;
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

fn expected_schema() -> Result<Vec<Identity>, SidecarError> {
    let mut expected = schema_identity_v10_golden()?;
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

pub(in crate::storage) fn migrate(connection: &mut Connection) -> Result<(), SidecarError> {
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
    verify(&tx)?;
    tx.commit().map_err(sqlite_error)
}

#[cfg(test)]
mod tests {
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
