use crate::SidecarError;
use chrono::{SecondsFormat, Utc};
use hello_agent_protocol::storage_lifetime_v1::{
    validate_storage_status_v1, PhysicalCleanupDisabledReasonV1, PhysicalCleanupStatusV1,
    StorageCompactionAttemptStatusV1, StorageCompactionStatusV1, StorageDatabaseFactsV1,
    StorageLedgerTailFactsV1, StoragePressureCollectionStatusV1, StoragePressureFactsV1,
    StorageSnapshotAggregateFactsV1, StorageStatusV1, STORAGE_STATUS_SCHEMA_VERSION_V1,
};
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub(crate) fn collect_status(
    connection: &Connection,
    database_path: &Path,
) -> Result<StorageStatusV1, SidecarError> {
    let page_size = pragma_u64(connection, "page_size")?;
    let page_count = pragma_u64(connection, "page_count")?;
    let logical_bytes = page_size.checked_mul(page_count).ok_or_else(|| {
        SidecarError::new(
            "storageStatusUnavailable",
            "database logical byte count overflowed",
        )
    })?;
    let physical_bytes = regular_file_len(database_path, false)?;
    let wal_bytes = regular_file_len(&wal_path(database_path), true)?;
    let (event_count, payload_bytes) = aggregate_count_and_bytes(
        connection,
        "SELECT count(*), coalesce(sum(length(CAST(canonical_json_payload AS BLOB))), 0) FROM events;",
    )?;
    let (snapshot_count, snapshot_bytes) = aggregate_count_and_bytes(
        connection,
        "SELECT count(*), coalesce(sum(length(CAST(projection_json AS BLOB))), 0) FROM (\
           SELECT projection_json FROM projects UNION ALL \
           SELECT projection_json FROM chats UNION ALL \
           SELECT projection_json FROM runs UNION ALL \
           SELECT projection_json FROM locks\
         );",
    )?;

    let status = StorageStatusV1 {
        schema_version: STORAGE_STATUS_SCHEMA_VERSION_V1,
        collected_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        database: StorageDatabaseFactsV1 {
            logical_bytes,
            physical_bytes,
            wal_bytes,
        },
        ledger_tail: StorageLedgerTailFactsV1 {
            event_count,
            payload_bytes,
        },
        snapshot_aggregates: StorageSnapshotAggregateFactsV1 {
            count: snapshot_count,
            payload_bytes: snapshot_bytes,
        },
        pressure_facts: StoragePressureFactsV1 {
            collection_status: StoragePressureCollectionStatusV1::Unavailable,
            volume_capacity_bytes: None,
            volume_available_bytes: None,
        },
        compaction: compaction_status(connection)?,
        physical_cleanup: PhysicalCleanupStatusV1 {
            enabled: false,
            reason: PhysicalCleanupDisabledReasonV1::UserPolicyNotConfigured,
        },
    };
    let value = serde_json::to_value(&status).map_err(|_| {
        SidecarError::new(
            "storageStatusInvalid",
            "storage status could not be validated",
        )
    })?;
    validate_storage_status_v1(&value).map_err(|error| {
        SidecarError::new(
            "storageStatusInvalid",
            format!("storage status facts failed validation: {error}"),
        )
    })?;
    Ok(status)
}

fn pragma_u64(connection: &Connection, name: &str) -> Result<u64, SidecarError> {
    let sql = format!("PRAGMA {name};");
    let value: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(sqlite_error)?;
    u64::try_from(value).map_err(|_| {
        SidecarError::new(
            "storageStatusUnavailable",
            "database size fact was negative",
        )
    })
}

fn aggregate_count_and_bytes(
    connection: &Connection,
    sql: &str,
) -> Result<(u64, u64), SidecarError> {
    let (count, bytes): (i64, i64) = connection
        .query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(sqlite_error)?;
    let count = u64::try_from(count).map_err(|_| {
        SidecarError::new("storageStatusUnavailable", "aggregate count was negative")
    })?;
    let bytes = u64::try_from(bytes).map_err(|_| {
        SidecarError::new(
            "storageStatusUnavailable",
            "aggregate payload byte count was negative",
        )
    })?;
    Ok((count, bytes))
}

fn regular_file_len(path: &Path, missing_is_zero: bool) -> Result<u64, SidecarError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(SidecarError::new(
                "storageStatusUnavailable",
                "database size fact did not refer to a regular file",
            ))
        }
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if missing_is_zero && error.kind() == ErrorKind::NotFound => Ok(0),
        Err(_) => Err(SidecarError::new(
            "storageStatusUnavailable",
            "database file size metadata could not be read",
        )),
    }
}

fn wal_path(database_path: &Path) -> PathBuf {
    let mut name = database_path.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

fn compaction_status(connection: &Connection) -> Result<StorageCompactionStatusV1, SidecarError> {
    let row: Option<(String, Option<String>, Option<String>)> = connection
        .query_row(
            "SELECT state, completed_at, last_successful_at FROM maintenance_jobs \
             WHERE state IN ('succeeded', 'failed') \
             ORDER BY completed_at DESC, created_at DESC, job_id DESC LIMIT 1;",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sqlite_error)?;
    match row {
        None => Ok(StorageCompactionStatusV1 {
            last_attempt_status: StorageCompactionAttemptStatusV1::NeverRun,
            last_attempted_at: None,
            last_successful_at: None,
        }),
        Some((state, completed_at, last_successful_at)) => {
            let last_attempt_status = match state.as_str() {
                "succeeded" => StorageCompactionAttemptStatusV1::Succeeded,
                "failed" => StorageCompactionAttemptStatusV1::Failed,
                _ => {
                    return Err(SidecarError::new(
                        "storageStatusInvalid",
                        "maintenance job state was invalid",
                    ))
                }
            };
            Ok(StorageCompactionStatusV1 {
                last_attempt_status,
                last_attempted_at: completed_at,
                last_successful_at,
            })
        }
    }
}

fn sqlite_error(error: rusqlite::Error) -> SidecarError {
    SidecarError::new("databaseFailure", error.to_string())
}

#[cfg(test)]
mod tests;
