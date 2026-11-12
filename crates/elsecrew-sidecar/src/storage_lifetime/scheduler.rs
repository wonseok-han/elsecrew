use super::compaction_writer::{CompactionStepResult, CompactionTarget};
use super::LifetimeMaterializationBatchResult;
use crate::{SidecarError, Storage};

const AGGREGATE_TYPES: [&str; 4] = ["project", "chat", "run", "lock"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaintenanceTickResult {
    Created,
    ArmedWorked,
    Finalized,
    NoWork,
}

trait MaintenanceDriver {
    fn invalidation_epoch(&self) -> u64;
    fn materialization_requires_work(&self) -> Result<bool, SidecarError>;
    fn materialization_step(&mut self) -> Result<LifetimeMaterializationBatchResult, SidecarError>;
    fn pending_compaction_target(&self) -> Result<Option<CompactionTarget>, SidecarError>;
    fn next_candidate(
        &mut self,
        aggregate_type: &str,
        after_aggregate_id: &str,
    ) -> Result<Option<CompactionTarget>, SidecarError>;
    fn compaction_step(
        &mut self,
        target: &CompactionTarget,
    ) -> Result<CompactionStepResult, SidecarError>;
}

impl MaintenanceDriver for Storage {
    fn invalidation_epoch(&self) -> u64 {
        self.maintenance_epoch()
    }

    fn materialization_requires_work(&self) -> Result<bool, SidecarError> {
        self.materialization_requires_work()
    }

    fn materialization_step(&mut self) -> Result<LifetimeMaterializationBatchResult, SidecarError> {
        self.run_materialization_scheduler_step()
    }

    fn pending_compaction_target(&self) -> Result<Option<CompactionTarget>, SidecarError> {
        self.pending_compaction_target()
    }

    fn next_candidate(
        &mut self,
        aggregate_type: &str,
        after_aggregate_id: &str,
    ) -> Result<Option<CompactionTarget>, SidecarError> {
        self.next_compaction_candidate(aggregate_type, after_aggregate_id)
    }

    fn compaction_step(
        &mut self,
        target: &CompactionTarget,
    ) -> Result<CompactionStepResult, SidecarError> {
        self.run_compaction_scheduler_step(target)
    }
}

#[derive(Debug, Default)]
pub(crate) struct CooperativeScheduler {
    observed_epoch: u64,
    aggregate_type_index: usize,
    after_aggregate_id: String,
    dormant: bool,
}

impl CooperativeScheduler {
    pub(crate) fn is_dormant(&self) -> bool {
        self.dormant
    }

    pub(crate) fn tick(
        &mut self,
        storage: &mut Storage,
    ) -> Result<MaintenanceTickResult, SidecarError> {
        self.tick_with_driver(storage)
    }

    fn tick_with_driver(
        &mut self,
        driver: &mut dyn MaintenanceDriver,
    ) -> Result<MaintenanceTickResult, SidecarError> {
        let epoch = driver.invalidation_epoch();
        if epoch != self.observed_epoch {
            self.observed_epoch = epoch;
            self.aggregate_type_index = 0;
            self.after_aggregate_id.clear();
            self.dormant = false;
        }
        if self.dormant {
            return Ok(MaintenanceTickResult::NoWork);
        }

        if driver.materialization_requires_work()? {
            return match driver.materialization_step()? {
                LifetimeMaterializationBatchResult::Progress { .. } => {
                    Ok(MaintenanceTickResult::ArmedWorked)
                }
                LifetimeMaterializationBatchResult::Completed { .. } => {
                    Ok(MaintenanceTickResult::Finalized)
                }
                LifetimeMaterializationBatchResult::Suspended { .. }
                | LifetimeMaterializationBatchResult::Failed { .. } => Err(maintenance_error()),
            };
        }

        if let Some(target) = driver.pending_compaction_target()? {
            return map_compaction_result(driver.compaction_step(&target)?);
        }

        while self.aggregate_type_index < AGGREGATE_TYPES.len() {
            let aggregate_type = AGGREGATE_TYPES[self.aggregate_type_index];
            let candidate = driver.next_candidate(aggregate_type, &self.after_aggregate_id)?;
            let Some(target) = candidate else {
                self.aggregate_type_index += 1;
                self.after_aggregate_id.clear();
                continue;
            };
            let result = driver.compaction_step(&target)?;
            if result == CompactionStepResult::NoWork {
                self.after_aggregate_id = target.aggregate_id;
            }
            return map_compaction_result(result);
        }

        self.dormant = true;
        Ok(MaintenanceTickResult::NoWork)
    }
}

fn map_compaction_result(
    result: CompactionStepResult,
) -> Result<MaintenanceTickResult, SidecarError> {
    Ok(match result {
        CompactionStepResult::Created { .. } => MaintenanceTickResult::Created,
        CompactionStepResult::ArmedWorked { .. } => MaintenanceTickResult::ArmedWorked,
        CompactionStepResult::Finalized { .. } => MaintenanceTickResult::Finalized,
        CompactionStepResult::NoWork => MaintenanceTickResult::NoWork,
    })
}

fn maintenance_error() -> SidecarError {
    SidecarError::new(
        "storageMaintenanceFailed",
        "cooperative storage maintenance could not prove a safe bounded step",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_lifetime::{LifetimeMaterializationCursorFacts, StorageLifetimePhaseV4};
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeDriver {
        epoch: u64,
        materialization_pending: bool,
        materialization: VecDeque<LifetimeMaterializationBatchResult>,
        persisted: VecDeque<Option<CompactionTarget>>,
        candidates: VecDeque<(&'static str, Option<CompactionTarget>)>,
        compaction: VecDeque<CompactionStepResult>,
        calls: Vec<String>,
    }

    impl MaintenanceDriver for FakeDriver {
        fn invalidation_epoch(&self) -> u64 {
            self.epoch
        }

        fn materialization_requires_work(&self) -> Result<bool, SidecarError> {
            Ok(self.materialization_pending)
        }

        fn materialization_step(
            &mut self,
        ) -> Result<LifetimeMaterializationBatchResult, SidecarError> {
            self.calls.push("materializer".to_string());
            self.materialization
                .pop_front()
                .ok_or_else(maintenance_error)
        }

        fn pending_compaction_target(&self) -> Result<Option<CompactionTarget>, SidecarError> {
            Ok(self.persisted.front().cloned().unwrap_or(None))
        }

        fn next_candidate(
            &mut self,
            aggregate_type: &str,
            _after_aggregate_id: &str,
        ) -> Result<Option<CompactionTarget>, SidecarError> {
            let next = self.candidates.pop_front();
            match next {
                Some((expected, target)) if expected == aggregate_type => Ok(target),
                Some(_) => Err(maintenance_error()),
                None => Ok(None),
            }
        }

        fn compaction_step(
            &mut self,
            target: &CompactionTarget,
        ) -> Result<CompactionStepResult, SidecarError> {
            self.calls
                .push(format!("compactor:{}", target.aggregate_id));
            if self.persisted.front().is_some_and(Option::is_some) {
                self.persisted.pop_front();
            }
            self.compaction.pop_front().ok_or_else(maintenance_error)
        }
    }

    fn target(kind: &str, id: &str) -> CompactionTarget {
        CompactionTarget {
            aggregate_type: kind.to_string(),
            aggregate_id: id.to_string(),
        }
    }

    #[test]
    fn materializer_has_priority_and_completion_never_compacts_in_same_tick() {
        let mut scheduler = CooperativeScheduler::default();
        let mut driver = FakeDriver {
            epoch: 1,
            materialization_pending: true,
            materialization: VecDeque::from([
                LifetimeMaterializationBatchResult::Progress {
                    cursor: LifetimeMaterializationCursorFacts {
                        cutoff_event_rowid: 1,
                        phase: StorageLifetimePhaseV4::IndexSource,
                        after_scan_rowid: 1,
                        scan_exhausted: false,
                    },
                    source_rows: 1,
                    source_payload_bytes: 1,
                },
                LifetimeMaterializationBatchResult::Completed {
                    cutoff_event_rowid: 1,
                },
            ]),
            persisted: VecDeque::from([Some(target("project", "project_a"))]),
            compaction: VecDeque::from([CompactionStepResult::Created {
                job_id: "job_a".to_string(),
            }]),
            ..FakeDriver::default()
        };

        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::ArmedWorked
        );
        assert_eq!(driver.calls, ["materializer"]);
        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::Finalized
        );
        assert_eq!(driver.calls, ["materializer", "materializer"]);
        driver.materialization_pending = false;
        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::Created
        );
        assert_eq!(
            driver.calls,
            ["materializer", "materializer", "compactor:project_a"]
        );
    }

    #[test]
    fn fixed_keyset_order_repeats_target_until_no_work_then_sleeps_and_wakes() {
        let mut scheduler = CooperativeScheduler::default();
        let mut driver = FakeDriver {
            epoch: 1,
            candidates: VecDeque::from([
                ("project", None),
                ("chat", Some(target("chat", "chat_a"))),
                ("chat", Some(target("chat", "chat_a"))),
                ("chat", None),
                ("run", None),
                ("lock", None),
            ]),
            compaction: VecDeque::from([
                CompactionStepResult::Finalized {
                    job_id: "job_1".to_string(),
                },
                CompactionStepResult::NoWork,
            ]),
            ..FakeDriver::default()
        };

        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::Finalized
        );
        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::NoWork
        );
        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::NoWork
        );
        assert!(scheduler.is_dormant());
        let calls = driver.calls.clone();
        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::NoWork
        );
        assert_eq!(driver.calls, calls, "dormant tick must do zero driver work");

        driver.epoch = 2;
        driver.candidates = VecDeque::from([("project", Some(target("project", "project_b")))]);
        driver.compaction = VecDeque::from([CompactionStepResult::Created {
            job_id: "job_2".to_string(),
        }]);
        assert_eq!(
            scheduler.tick_with_driver(&mut driver).unwrap(),
            MaintenanceTickResult::Created
        );
        assert!(!scheduler.is_dormant());
    }
}
