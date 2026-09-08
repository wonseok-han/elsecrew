use super::*;
use crate::storage_lifetime::compaction_writer::{
    run_private_compaction_step, CompactionCancellationPoint, CompactionFailurePoint,
    CompactionStepResult, CompactionTarget,
};
use hello_agent_protocol::storage_lifetime_v1::StorageCompactionPhaseV3;
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
            Ok(CompactionStepResult::Completed { job_id }) => return job_id,
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
            CompactionStepResult::Progress {
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

#[test]
fn v10_writer_threshold_and_exact_thousand_delete_candidate() {
    let (_root, _path, mut below) = exact_v10(1000);
    let mut never = |_: CompactionCancellationPoint| false;
    assert_eq!(
        run_private_compaction_step(&mut below.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::NoCandidate
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
        CompactionStepResult::NoCandidate
    );
    assert_eq!(table_rows(&exact.connection, "maintenance_jobs").len(), 0);

    assert!(matches!(
        run_private_compaction_step(&mut exact.connection, &target(), &mut never, None).unwrap(),
        CompactionStepResult::Progress {
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
        CompactionStepResult::Progress {
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
            CompactionStepResult::Progress {
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
