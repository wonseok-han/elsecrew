use super::*;
use crate::storage_lifetime::compaction_writer::{
    run_private_compaction_step, CompactionCancellationPoint, CompactionFailurePoint,
    CompactionStepResult, CompactionTarget,
};
use crate::storage_lifetime::scheduler::{CooperativeScheduler, MaintenanceTickResult};
use hello_agent_protocol::project_v1::sha256_file_version_v1;
use hello_agent_protocol::storage_lifetime_v1::{
    canonical_frozen_operation_record_v1, FrozenOperationSourceV1, StorageCompactionPhaseV3,
    STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
};
use rusqlite::OptionalExtension;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

const PROJECT: &str = "project_v10_writer";
const STAMP: &str = "2026-09-08T08:00:00Z";

fn exact_v10(count: u64) -> (TempDir, PathBuf, Storage) {
    let (root, path) = database();
    let mut storage = Storage::open(&path).unwrap();
    for sequence in 1..=count {
        storage
            .append_event(&event(
                &format!("evt_writer_{sequence}"),
                &format!("op_writer_{sequence}"),
                "project",
                PROJECT,
                sequence,
                json!({
                    "canonical_path":"/fixture/v10-writer",
                    "project_id":PROJECT,
                    "sequence":sequence,
                }),
                STAMP,
            ))
            .unwrap();
    }
    run_bounded_materializer_to_ready(&mut storage);
    storage.checkpoint().unwrap();
    drop(storage);
    let cancellation = AtomicBool::new(false);
    let storage = Storage::open_for_admission(&path, &cancellation).unwrap();
    (root, path, storage)
}

fn target() -> CompactionTarget {
    CompactionTarget {
        aggregate_type: "project".to_string(),
        aggregate_id: PROJECT.to_string(),
    }
}

fn run_to_completion(storage: &mut Storage) -> String {
    let mut never = |_: CompactionCancellationPoint| false;
    let mut last = None;
    for _ in 0..160 {
        match run_private_compaction_step(&mut storage.connection, &target(), &mut never, None) {
            Ok(CompactionStepResult::Finalized { job_id }) => return job_id,
            Ok(result) => last = Some(result),
            Err(error) => {
                let durable: Option<(String, String)> = storage
                    .connection
                    .query_row(
                        "SELECT state,cutoff_json FROM maintenance_jobs ORDER BY created_at DESC LIMIT 1",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .unwrap();
                panic!("writer step failed after {last:?}; durable={durable:?}: {error:?}")
            }
        }
    }
    panic!("writer did not finish within the test-only bound");
}

fn run_until_pending(
    storage: &mut Storage,
    phase: StorageCompactionPhaseV3,
    after_plan_ordinal: u32,
) {
    let mut never = |_: CompactionCancellationPoint| false;
    for _ in 0..160 {
        let result =
            run_private_compaction_step(&mut storage.connection, &target(), &mut never, None)
                .unwrap();
        if matches!(
            result,
            CompactionStepResult::ArmedWorked {
                phase: current,
                after_plan_ordinal: after,
                ..
            } if current == phase && after == after_plan_ordinal
        ) {
            return;
        }
    }
    panic!("writer did not reach the requested pending cursor");
}

fn preseed_frozen_operation(storage: &Storage, ordinal: u32) -> String {
    let mut source = storage
        .connection
        .query_row(
            "SELECT p.operation_id,p.event_rowid,p.event_id,p.request_id,p.aggregate_type,p.aggregate_id,\
                    p.aggregate_sequence,p.event_type,p.payload_schema_version,p.canonical_payload_json,\
                    p.canonical_payload_digest,p.event_timestamp \
             FROM storage_compaction_plan_rows p JOIN maintenance_jobs j ON j.job_id=p.job_id \
             WHERE j.job_kind='losslessCompaction' AND p.ordinal=?1;",
            [ordinal],
            |row| {
                Ok(FrozenOperationSourceV1 {
                    operation_id: row.get(0)?,
                    source_event_rowid: u64::try_from(row.get::<_, i64>(1)?).unwrap(),
                    source_event_id: row.get(2)?,
                    request_id: row.get(3)?,
                    aggregate_type: row.get(4)?,
                    aggregate_id: row.get(5)?,
                    aggregate_sequence: u64::try_from(row.get::<_, i64>(6)?).unwrap(),
                    event_type: row.get(7)?,
                    payload_schema_version: row.get(8)?,
                    canonical_payload_json: row.get(9)?,
                    canonical_payload_digest: row.get(10)?,
                    event_timestamp: row.get(11)?,
                    canonical_record_json: String::new(),
                    record_digest: String::new(),
                    frozen_at: STAMP.to_string(),
                })
            },
        )
        .unwrap();
    source.canonical_record_json = canonical_frozen_operation_record_v1(&source).unwrap();
    source.record_digest = sha256_file_version_v1(source.canonical_record_json.as_bytes())
        .trim_start_matches("sha256:")
        .to_string();
    storage
        .connection
        .execute(
            "INSERT INTO frozen_operation_sources(\
               operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,aggregate_id,\
               aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,\
               canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15);",
            rusqlite::params![source.operation_id, i64::try_from(source.source_event_rowid).unwrap(),
                source.source_event_id, source.request_id, source.aggregate_type, source.aggregate_id,
                i64::try_from(source.aggregate_sequence).unwrap(), source.event_type,
                source.payload_schema_version, source.canonical_payload_json,
                source.canonical_payload_digest, source.event_timestamp, source.canonical_record_json,
                source.record_digest, source.frozen_at],
        )
        .unwrap();
    source.operation_id
}

fn assert_writer_rejects_before_delete(storage: &mut Storage, column: &str, expected: &str) {
    let before: (u32, u32, u32, u32, u32) = storage
        .connection
        .query_row(
            "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM event_source_index),\
                    (SELECT count(*) FROM operation_receipts),\
                    (SELECT count(*) FROM ledger_compaction_delete_authority),\
                    (SELECT count(*) FROM ledger_compaction_prefixes);",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    let mut never = |_: CompactionCancellationPoint| false;
    assert!(
        run_private_compaction_step(&mut storage.connection, &target(), &mut never, None,).is_err()
    );
    let after: (u32, u32, u32, u32, u32, String) = storage
        .connection
        .query_row(
            "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM event_source_index),\
                    (SELECT count(*) FROM operation_receipts),\
                    (SELECT count(*) FROM ledger_compaction_delete_authority),\
                    (SELECT count(*) FROM ledger_compaction_prefixes),\
                    (SELECT state FROM maintenance_jobs WHERE job_kind='losslessCompaction');",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!((after.0, after.1, after.2, after.3, after.4), before);
    assert_eq!(after.5, "running");
    let actual: String = storage
        .connection
        .query_row(
            &format!(
                "SELECT {column} FROM frozen_operation_sources WHERE operation_id='op_writer_101';"
            ),
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn v10_writer_rejects_corrupt_existing_frozen_operation_before_delete() {
    let (root, path, mut storage) = exact_v10(1001);
    run_until_pending(
        &mut storage,
        StorageCompactionPhaseV3::FreezeOperations,
        100,
    );
    storage.checkpoint().unwrap();
    drop(storage);

    let oversized = "x".repeat(
        usize::try_from(STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1).unwrap()
            + 1,
    );
    for (name, column, value) in [
        (
            "canonical_record",
            "canonical_record_json",
            "{}".to_string(),
        ),
        ("record_digest", "record_digest", "0".repeat(64)),
        ("frozen_at", "frozen_at", "not-a-time".to_string()),
        ("oversized_record", "canonical_record_json", oversized),
    ] {
        let copy = root.path().join(format!("corrupt-{name}.sqlite3"));
        std::fs::copy(&path, &copy).unwrap();
        let cancellation = AtomicBool::new(false);
        let mut copy = Storage::open_for_admission(&copy, &cancellation).unwrap();
        assert_eq!(preseed_frozen_operation(&copy, 101), "op_writer_101");
        super::reopen_closure::corrupt(
            &copy.connection,
            &format!(
                "UPDATE frozen_operation_sources SET {column}='{}' WHERE operation_id='op_writer_101';",
                value.replace('\'', "''")
            ),
        );
        assert_writer_rejects_before_delete(&mut copy, column, &value);
    }

    let copy = root.path().join("serialization-budget.sqlite3");
    std::fs::copy(&path, &copy).unwrap();
    let cancellation = AtomicBool::new(false);
    let mut copy = Storage::open_for_admission(&copy, &cancellation).unwrap();
    let near_limit = "x".repeat(
        usize::try_from(STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1).unwrap()
            / 2
            + 1,
    );
    super::reopen_closure::corrupt(
        &copy.connection,
        &format!(
            "UPDATE storage_compaction_plan_rows SET aggregate_id='{}' \
             WHERE ordinal=101 AND job_id=(SELECT job_id FROM maintenance_jobs \
             WHERE job_kind='losslessCompaction');",
            near_limit
        ),
    );
    let before: (u32, u32, u32, u32, u32, u32) = copy
        .connection
        .query_row(
            "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM event_source_index),\
                    (SELECT count(*) FROM operation_receipts),\
                    (SELECT count(*) FROM frozen_operation_sources),\
                    (SELECT count(*) FROM ledger_compaction_delete_authority),\
                    (SELECT count(*) FROM ledger_compaction_prefixes);",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    let mut never = |_: CompactionCancellationPoint| false;
    assert!(
        run_private_compaction_step(&mut copy.connection, &target(), &mut never, None).is_err()
    );
    let after: (u32, u32, u32, u32, u32, u32, String, u32) = copy
        .connection
        .query_row(
            "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM event_source_index),\
                    (SELECT count(*) FROM operation_receipts),\
                    (SELECT count(*) FROM frozen_operation_sources),\
                    (SELECT count(*) FROM ledger_compaction_delete_authority),\
                    (SELECT count(*) FROM ledger_compaction_prefixes),\
                    (SELECT state FROM maintenance_jobs WHERE job_kind='losslessCompaction'),\
                    (SELECT length(CAST(aggregate_id AS BLOB)) FROM storage_compaction_plan_rows \
                     WHERE ordinal=101);",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        (after.0, after.1, after.2, after.3, after.4, after.5),
        before
    );
    assert_eq!(after.6, "running");
    assert_eq!(usize::try_from(after.7).unwrap(), near_limit.len());
}

#[test]
fn v10_writer_threshold_and_exact_thousand_delete_candidate() {
    let (_root, _path, mut below) = exact_v10(1000);
    let mut never = |_: CompactionCancellationPoint| false;
    assert_eq!(
        run_private_compaction_step(&mut below.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::NoWork
    );
    assert_eq!(table_rows(&below.connection, "maintenance_jobs").len(), 0);

    let (_root, path, mut exact) = exact_v10(1001);
    let mut cancel_before_arm = |point| point == CompactionCancellationPoint::BeforeArm;
    assert_eq!(
        run_private_compaction_step(
            &mut exact.connection,
            &target(),
            &mut cancel_before_arm,
            None,
        )
        .unwrap(),
        CompactionStepResult::NoWork
    );
    assert_eq!(table_rows(&exact.connection, "maintenance_jobs").len(), 0);

    assert!(matches!(
        run_private_compaction_step(&mut exact.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::Created { .. }
    ));
    assert!(matches!(
        run_private_compaction_step(&mut exact.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::ArmedWorked {
            phase: StorageCompactionPhaseV3::PlanEligibility,
            after_plan_ordinal: 100,
            ..
        }
    ));
    let wrong_target = CompactionTarget {
        aggregate_type: "project".to_string(),
        aggregate_id: "another_project".to_string(),
    };
    assert!(
        run_private_compaction_step(&mut exact.connection, &wrong_target, &mut never, None,)
            .is_err()
    );
    exact.checkpoint().unwrap();
    drop(exact);
    let cancellation = AtomicBool::new(false);
    let mut exact = Storage::open_for_admission(&path, &cancellation).unwrap();
    run_until_pending(&mut exact, StorageCompactionPhaseV3::DeletePrefix, 0);
    assert!(matches!(
        run_private_compaction_step(&mut exact.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::ArmedWorked {
            phase: StorageCompactionPhaseV3::DeletePrefix,
            after_plan_ordinal: 100,
            ..
        }
    ));
    assert_eq!(
        exact
            .connection
            .query_row(
                "SELECT count(*) FROM events WHERE aggregate_type='project' AND aggregate_id=?1;",
                [PROJECT],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
        901
    );
    exact.checkpoint().unwrap();
    drop(exact);
    let cancellation = AtomicBool::new(false);
    let mut exact = Storage::open_for_admission(&path, &cancellation).unwrap();
    let job_id = run_to_completion(&mut exact);

    let facts: (u32, u32, u32, u32, u32, String) = exact
        .connection
        .query_row(
            "SELECT \
               (SELECT count(*) FROM events WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT count(*) FROM operation_receipts WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT count(*) FROM frozen_operation_sources WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT count(*) FROM ledger_compaction_prefixes WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT count(*) FROM storage_compaction_plan_rows WHERE job_id=?2),\
               (SELECT state FROM maintenance_jobs WHERE job_id=?2);",
            rusqlite::params![PROJECT, job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(facts, (1, 1001, 1001, 1, 1, "succeeded".to_string()));

    assert_eq!(
        exact
            .append_event(&event(
                "evt_writer_1002",
                "op_writer_1002",
                "project",
                PROJECT,
                1002,
                json!({
                    "canonical_path":"/fixture/v10-writer",
                    "project_id":PROJECT,
                    "sequence":1002,
                }),
                STAMP,
            ))
            .unwrap(),
        AppendOutcome::Committed
    );
}

#[test]
fn v10_writer_delete_failure_rolls_back_batch_and_leaves_running_sentinel() {
    let (root, path, mut storage) = exact_v10(1001);
    let mut never = |_: CompactionCancellationPoint| false;
    let mut reached = false;
    for _ in 0..80 {
        let result =
            run_private_compaction_step(&mut storage.connection, &target(), &mut never, None)
                .unwrap();
        if matches!(
            result,
            CompactionStepResult::ArmedWorked {
                phase: StorageCompactionPhaseV3::DeletePrefix,
                after_plan_ordinal: 0,
                ..
            }
        ) {
            reached = true;
            break;
        }
    }
    assert!(reached, "writer must reach deletePrefix before injection");
    storage.checkpoint().unwrap();
    let after_batch_path = root.path().join("after-batch.sqlite3");
    let precommit_path = root.path().join("precommit.sqlite3");
    let after_arm_path = root.path().join("after-arm.sqlite3");
    let cancel_after_arm_path = root.path().join("cancel-after-arm.sqlite3");
    std::fs::copy(&path, &after_batch_path).unwrap();
    std::fs::copy(&path, &precommit_path).unwrap();
    std::fs::copy(&path, &after_arm_path).unwrap();
    std::fs::copy(&path, &cancel_after_arm_path).unwrap();
    let before: u32 = storage
        .connection
        .query_row(
            "SELECT count(*) FROM events WHERE aggregate_type='project' AND aggregate_id=?1;",
            [PROJECT],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before, 1001);

    assert!(run_private_compaction_step(
        &mut storage.connection,
        &target(),
        &mut never,
        Some(CompactionFailurePoint::AfterAuthorityInsert),
    )
    .is_err());
    assert_failed_delete_is_closed(&mut storage, before);
    assert!(
        run_private_compaction_step(&mut storage.connection, &target(), &mut never, None).is_err()
    );

    for (copy, failure) in [
        (after_batch_path, CompactionFailurePoint::AfterBatchMutation),
        (precommit_path, CompactionFailurePoint::ImmediatelyPrecommit),
        (after_arm_path, CompactionFailurePoint::AfterArmCommit),
    ] {
        let cancellation = AtomicBool::new(false);
        let mut copy = Storage::open_for_admission(&copy, &cancellation).unwrap();
        assert!(run_private_compaction_step(
            &mut copy.connection,
            &target(),
            &mut never,
            Some(failure),
        )
        .is_err());
        assert_failed_delete_is_closed(&mut copy, before);
    }

    let cancellation = AtomicBool::new(false);
    let mut cancelled = Storage::open_for_admission(&cancel_after_arm_path, &cancellation).unwrap();
    let mut cancel_after_arm = |point| point == CompactionCancellationPoint::AfterArm;
    assert!(run_private_compaction_step(
        &mut cancelled.connection,
        &target(),
        &mut cancel_after_arm,
        None,
    )
    .is_err());
    assert_failed_delete_is_closed(&mut cancelled, before);
}

#[test]
fn cooperative_scheduler_prioritizes_materialization_and_persisted_pending_work() {
    let (_root, _path, mut storage) = exact_v10(1001);
    assert_eq!(
        storage
            .append_event(&event(
                "evt_writer_1002",
                "op_writer_1002",
                "project",
                PROJECT,
                1002,
                json!({
                    "canonical_path":"/fixture/v10-writer",
                    "project_id":PROJECT,
                    "sequence":1002,
                }),
                STAMP,
            ))
            .unwrap(),
        AppendOutcome::Committed
    );
    let mut scheduler = CooperativeScheduler::default();
    assert_eq!(
        scheduler.tick(&mut storage).unwrap(),
        MaintenanceTickResult::ArmedWorked
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM maintenance_jobs;", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        0,
        "materializer work must precede candidate creation"
    );
    run_bounded_materializer_to_ready(&mut storage);

    let mut never = |_: CompactionCancellationPoint| false;
    assert!(matches!(
        run_private_compaction_step(&mut storage.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::Created { .. }
    ));
    let mut scheduler = CooperativeScheduler::default();
    assert_eq!(
        scheduler.tick(&mut storage).unwrap(),
        MaintenanceTickResult::ArmedWorked,
        "persisted pending work must run before candidate discovery"
    );
}

#[test]
fn cooperative_scheduler_enforces_threshold_keyset_and_two_job_chain() {
    let (_root, _path, mut below) = exact_v10(1000);
    let mut below_scheduler = CooperativeScheduler::default();
    assert_eq!(
        below_scheduler.tick(&mut below).unwrap(),
        MaintenanceTickResult::NoWork
    );
    assert_eq!(
        below_scheduler.tick(&mut below).unwrap(),
        MaintenanceTickResult::NoWork
    );
    assert!(below_scheduler.is_dormant());
    assert_eq!(
        below
            .connection
            .query_row("SELECT count(*) FROM maintenance_jobs;", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        0
    );

    let (_root, _path, mut exact) = exact_v10(1001);
    let mut exact_scheduler = CooperativeScheduler::default();
    assert_eq!(
        exact_scheduler.tick(&mut exact).unwrap(),
        MaintenanceTickResult::Created
    );

    let (_root, _path, mut chained) = exact_v10(2001);
    let mut chained_scheduler = CooperativeScheduler::default();
    let mut finalized = 0;
    for _ in 0..360 {
        if chained_scheduler.tick(&mut chained).unwrap() == MaintenanceTickResult::Finalized {
            finalized += 1;
        }
        if chained_scheduler.is_dormant() {
            break;
        }
    }
    assert!(chained_scheduler.is_dormant());
    assert_eq!(finalized, 2);
    assert_eq!(
        chained
            .connection
            .query_row(
                "SELECT count(*) FROM maintenance_jobs WHERE job_kind='losslessCompaction' AND state='succeeded';",
                [],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        chained
            .connection
            .query_row(
                "SELECT count(*) FROM events WHERE aggregate_type='project' AND aggregate_id=?1;",
                [PROJECT],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn cooperative_scheduler_rejects_running_sentinel_without_mutation() {
    let (_root, _path, mut storage) = exact_v10(1001);
    let mut never = |_: CompactionCancellationPoint| false;
    assert!(matches!(
        run_private_compaction_step(&mut storage.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::Created { .. }
    ));
    assert!(run_private_compaction_step(
        &mut storage.connection,
        &target(),
        &mut never,
        Some(CompactionFailurePoint::AfterArmCommit),
    )
    .is_err());
    let before = table_rows(&storage.connection, "maintenance_jobs");
    let mut scheduler = CooperativeScheduler::default();
    assert!(scheduler.tick(&mut storage).is_err());
    assert_eq!(table_rows(&storage.connection, "maintenance_jobs"), before);
}

fn assert_failed_delete_is_closed(storage: &mut Storage, expected_events: u32) {
    let after: (u32, u32, u32, String) = storage
        .connection
        .query_row(
            "SELECT \
               (SELECT count(*) FROM events WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT count(*) FROM event_source_index WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT count(*) FROM ledger_compaction_delete_authority),\
               (SELECT state FROM maintenance_jobs WHERE job_kind='losslessCompaction');",
            [PROJECT],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        after,
        (expected_events, expected_events, 0, "running".to_string())
    );
}

#[test]
fn v10_writer_chains_immediate_predecessor_for_two_thousand_delete_candidates() {
    let (_root, _path, mut storage) = exact_v10(2001);
    let first = run_to_completion(&mut storage);
    let second = run_to_completion(&mut storage);
    let facts: (u32, u32, u32, String) = storage
        .connection
        .query_row(
            "SELECT \
               (SELECT count(*) FROM maintenance_jobs WHERE job_kind='losslessCompaction' AND state='succeeded'),\
               (SELECT count(*) FROM ledger_compaction_prefixes WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT count(*) FROM events WHERE aggregate_type='project' AND aggregate_id=?1),\
               (SELECT previous_successful_job_id FROM storage_compaction_plans WHERE job_id=?2);",
            rusqlite::params![PROJECT, second],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(facts, (2, 2, 1, first));
}
