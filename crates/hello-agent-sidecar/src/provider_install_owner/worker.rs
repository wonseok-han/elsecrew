//! One installation transaction. Internal only: the public manager must first
//! validate and consume consent for the exact provider/destination/plan.
use super::{
    download::{self, DownloadOutcome},
    failure,
    journal::InstallJournal,
    InstallScope,
};
use crate::{SidecarError, Storage};
use hello_agent_core::install_lifecycle::InstallPhase;
use std::sync::atomic::AtomicBool;

/// No renderer-supplied command, script, destination, URL, or credentials.
/// This does not authorize a new task: callers supply an already-consented ID.
pub(crate) fn execute_consented(
    storage: &Storage,
    operation: uuid::Uuid,
    provider: &str,
    cancel: &AtomicBool,
    publish: &impl Fn(InstallPhase),
) -> Result<InstallPhase, SidecarError> {
    let scope = InstallScope::from_storage(storage)?;
    let execution = scope.execution_scope()?;
    let mut journal = InstallJournal::begin(&scope, operation, provider)?;
    journal.record("consented")?;
    publish(journal.phase());
    let result = (|| {
        match download::fetch(&mut journal, cancel)? {
            DownloadOutcome::Stopped(mut owner) => {
                if journal.phase() != InstallPhase::Stopping {
                    return Err(failure());
                }
                if let Some(owner) = owner.as_ref() {
                    if !owner.matches_startup_run(&operation.to_string())
                        || !owner.closure_evidence().is_some_and(|e| e.confirmed())
                    {
                        return Err(failure());
                    }
                }
                journal.record("cleanupConfirmed")?;
                if let Some(owner) = owner.as_mut() {
                    owner.release_after_run_terminal()?;
                }
            }
            DownloadOutcome::Ready(download) => {
                publish(journal.phase());
                // Reserve even if cancellation just arrived: run observes it
                // before spawn, then the coordinator can finalize both owners.
                let mut installer = download.reserve_installer(&journal, &execution)?;
                let run_result = installer.run(&mut journal, cancel);
                if run_result.is_err() && journal.phase() != InstallPhase::Stopping {
                    return Err(failure());
                }
                if journal.phase() == InstallPhase::Verifying {
                    publish(journal.phase());
                    let verification = installer.verify_result(&mut journal, cancel);
                    if verification.is_err() && journal.phase() != InstallPhase::Stopping {
                        return Err(failure());
                    }
                }
                publish(journal.phase());
                installer.finalize(&mut journal)?;
            }
        }
        Ok(())
    })();
    if result.is_err() {
        // A terminal record with a release error also stays blocked by its
        // active journal. Never publish Installed when finalization failed.
        let _ = journal.record("recoveryRequired");
        publish(InstallPhase::RecoveryRequired);
        return Err(failure());
    }
    let terminal = journal.phase();
    if journal.archive_terminal().is_err() {
        publish(InstallPhase::RecoveryRequired);
        return Err(failure());
    }
    publish(terminal);
    Ok(terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn pre_canceled_transaction_archives_without_network_or_processes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let phases = Mutex::new(Vec::new());
        for _ in 0..2 {
            assert_eq!(
                execute_consented(
                    &storage,
                    uuid::Uuid::new_v4(),
                    "claude",
                    &AtomicBool::new(true),
                    &|phase| phases.lock().unwrap().push(phase),
                )
                .unwrap(),
                InstallPhase::Canceled
            );
        }
        assert_eq!(
            *phases.lock().unwrap(),
            vec![
                InstallPhase::Downloading,
                InstallPhase::Canceled,
                InstallPhase::Downloading,
                InstallPhase::Canceled
            ]
        );
        assert!(!root.join(super::super::PENDING).exists());
        assert!(!root.join(super::super::EXECUTION_PENDING).exists());
        assert!(!root.join("provider-install-journal-v1.jsonl").exists());
        assert_eq!(
            std::fs::read_dir(&root)
                .unwrap()
                .filter(|entry| entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("provider-install-complete-"))
                .count(),
            2
        );
    }
}
