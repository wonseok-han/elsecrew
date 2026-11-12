//! One installation transaction. Internal only: the public manager must first
//! validate and consume consent for the exact provider/destination/plan.
use super::{
    consent::AuthorizedInstall,
    download::{self, DownloadOutcome},
    failure,
    journal::InstallJournal,
    InstallScope,
};
use crate::SidecarError;
use elsecrew_core::install_lifecycle::InstallPhase;
use std::sync::atomic::AtomicBool;

/// No renderer-supplied command, script, destination, URL, or credentials.
/// Consumes the non-cloneable authorization, not a caller-asserted consent flag.
pub(crate) fn execute_consented(
    scope: InstallScope,
    authorization: AuthorizedInstall,
    cancel: &AtomicBool,
    publish: &impl Fn(InstallPhase),
) -> Result<InstallPhase, SidecarError> {
    authorization.revalidate_plan().map_err(|_| {
        SidecarError::new(
            "providerInstallConsentExpired",
            "installation consent or destination changed before admission",
        )
    })?;
    let operation = authorization.operation();
    let provider = authorization.provider();
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
                let run_result = installer.run(&mut journal, &authorization, cancel);
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
    use crate::Storage;
    use std::sync::Mutex;

    // Explicit opt-in only: downloads and runs official installers in an isolated
    // temporary HOME using the product consent, sandbox, owner and verifier.
    fn official_install_and_update(provider: &str) {
        super::super::execute::INSTALL_DIAGNOSTICS.with(|enabled| enabled.set(true));
        let temp = tempfile::Builder::new()
            .prefix("elsecrew-official-install-")
            .tempdir()
            .unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join(".zshrc"), b"preserve synthetic shell settings\n").unwrap();
        let storage = Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
        for attempt in 1..=2 {
            let mut consent = super::super::consent::ConsentStore::default();
            let plan = consent.prepare_at_home(provider, &home).unwrap();
            let authorization = consent
                .accept(plan["planId"].as_str().unwrap(), provider)
                .unwrap();
            let outcome = execute_consented(
                InstallScope::from_storage(&storage).unwrap(),
                authorization,
                &AtomicBool::new(false),
                &|phase| {
                    eprintln!(
                        "official-install provider={provider} attempt={attempt} phase={phase:?}"
                    )
                },
            );
            let closed = !root.join(super::super::PENDING).exists()
                && !root.join(super::super::EXECUTION_PENDING).exists()
                && !root.join("provider-install-journal-v1.jsonl").exists();
            if !closed {
                let retained = temp.keep();
                panic!(
                    "unconfirmed installation closure; retained={} outcome={outcome:?}",
                    retained.display()
                );
            }
            assert_eq!(
                std::fs::read(home.join(".zshrc")).unwrap(),
                b"preserve synthetic shell settings\n"
            );
            assert_eq!(
                outcome.unwrap(),
                InstallPhase::Installed,
                "{provider} attempt={attempt}; confirmed-closed temporary data will be removed"
            );
        }
        drop(storage);
        temp.close().unwrap();
        eprintln!(
            "official-install provider={provider} install+update passed; temporary files removed"
        );
    }

    #[test]
    #[ignore = "requires explicit approval: official network download and temporary HOME installation; no login/model"]
    fn official_codex_installer_temp_home_install_and_update() {
        official_install_and_update("codex");
    }

    #[test]
    #[ignore = "requires explicit approval: official network download and temporary HOME installation; no login/model"]
    fn official_claude_installer_temp_home_install_and_update() {
        official_install_and_update("claude");
    }

    #[test]
    fn pre_canceled_transaction_archives_without_network_or_processes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
        let phases = Mutex::new(Vec::new());
        for _ in 0..2 {
            let mut consent = super::super::consent::ConsentStore::default();
            let plan = consent.prepare_at_home("claude", &root).unwrap();
            let authorization = consent
                .accept(plan["planId"].as_str().unwrap(), "claude")
                .unwrap();
            assert_eq!(
                execute_consented(
                    InstallScope::from_storage(&storage).unwrap(),
                    authorization,
                    &AtomicBool::new(true),
                    &|phase| { phases.lock().unwrap().push(phase) },
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
