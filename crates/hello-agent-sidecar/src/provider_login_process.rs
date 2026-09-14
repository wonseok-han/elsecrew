//! Internal login lifecycle, not a vendor launch authorization or a public IPC API.
//! A reviewed launcher must supply the isolated child; no existing auth is copied.
//! A durable reservation precedes launch. Uncertain exits retain it across restarts.
//! Like the profile binding, identity checks are point-in-time: hostile same-UID
//! directory/path replacement after a check is outside this capability's guarantee.
use crate::{provider_login_io::LoginIo, provider_profile::CodexProfileBinding, SidecarError};
use cap_std::fs::{Dir, File, MetadataExt, OpenOptions, OpenOptionsExt};
use hello_agent_core::proctree::{
    identity_of, ProcIdentity, ProcTable, SystemClock, SystemProcTable, TerminationConfig,
    TreeTracker,
};
use hello_agent_core::provider::vendor_observation::codex_login::LoginRequest;
use hello_agent_core::provider::BoundedObservation;
use std::collections::BTreeSet;
use std::io::Write;
use std::process::Child;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const PENDING: &str = "codex-login.pending";
const MAX_TRACKED: usize = 256;
const OBSERVE_INTERVAL: Duration = Duration::from_millis(100);

fn failure() -> SidecarError {
    SidecarError::new(
        "providerLoginRecoveryRequired",
        "login process cleanup could not be confirmed",
    )
}

/// The file's existence is the inter-session/restart fence, even if empty or malformed.
/// Never read an old reservation, repair it or automatically adopt its PIDs.
struct Reservation {
    directory: Dir,
    file: File,
    recorded: BTreeSet<ProcIdentity>,
    failed: bool,
}

impl Reservation {
    fn create(profile: &CodexProfileBinding) -> Result<Self, SidecarError> {
        let directory = profile.login_lease_directory()?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = directory
            .open_with(PENDING, &options)
            .map_err(|_| failure())?;
        file.write_all(b"hello-agent codex login pending v1\n")
            .map_err(|_| failure())?;
        file.sync_all().map_err(|_| failure())?;
        sync_directory(&directory)?;
        Ok(Self {
            directory,
            file,
            recorded: BTreeSet::new(),
            failed: false,
        })
    }

    fn verify(&self) -> Result<(), SidecarError> {
        let held = self.file.metadata().map_err(|_| failure())?;
        let current = self
            .directory
            .symlink_metadata(PENDING)
            .map_err(|_| failure())?;
        if !current.is_file()
            || held.dev() != current.dev()
            || held.ino() != current.ino()
            || current.nlink() != 1
            || current.uid() != unsafe { libc::geteuid() }
            || current.mode() & 0o777 != 0o600
        {
            return Err(failure());
        }
        Ok(())
    }

    fn record(&mut self, identities: &[ProcIdentity]) -> Result<(), SidecarError> {
        let result = self.record_inner(identities);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn record_inner(&mut self, identities: &[ProcIdentity]) -> Result<(), SidecarError> {
        if self.failed {
            return Err(failure());
        }
        self.verify()?;
        if identities.len() > MAX_TRACKED {
            return Err(failure());
        }
        let mut changed = false;
        for identity in identities {
            if self.recorded.insert(*identity) {
                // Process identity only: never command lines, account data or OAuth URLs.
                writeln!(self.file, "{} {}", identity.pid, identity.start_us)
                    .map_err(|_| failure())?;
                changed = true;
            }
        }
        if changed {
            self.file.sync_all().map_err(|_| failure())?;
        }
        Ok(())
    }

    fn release(&self) -> Result<(), SidecarError> {
        if self.failed {
            return Err(failure());
        }
        self.verify()?;
        // POSIX unlink is name-based, not an atomic unlink-if-inode operation.
        // This detects prior substitution, not a hostile same-UID race after verify.
        self.directory.remove_file(PENDING).map_err(|_| failure())?;
        sync_directory(&self.directory)
    }
}

fn sync_directory(directory: &Dir) -> Result<(), SidecarError> {
    directory
        .try_clone()
        .map_err(|_| failure())?
        .into_std_file()
        .sync_all()
        .map_err(|_| failure())
}

/// Owns the Child before any fallible identity/pipe operation. No Debug/raw error logging.
/// Dropping attempts cleanup but never releases the durable fence: only explicit finish does.
pub(crate) struct LoginProcess<'a> {
    profile: &'a CodexProfileBinding,
    reservation: Reservation,
    child: Option<Child>,
    tracker: Option<TreeTracker>,
    io: Option<LoginIo>,
    last_observed: Option<Instant>,
    failed: bool,
    evidence_failed: bool,
    attempted: bool,
    finished: bool,
}

impl<'a> LoginProcess<'a> {
    pub(crate) fn reserve(profile: &'a CodexProfileBinding) -> Result<Self, SidecarError> {
        Ok(Self {
            profile,
            reservation: Reservation::create(profile)?,
            child: None,
            tracker: None,
            io: None,
            last_observed: None,
            failed: false,
            evidence_failed: false,
            attempted: false,
            finished: false,
        })
    }

    /// Trusted backend launcher callback only. It must enforce executable/version,
    /// environment/cwd isolation and a pre-exec tracking barrier. This owner is not
    /// proof of those properties. The callback must return every spawned child.
    /// There is deliberately no renderer command/path input or default vendor spawn.
    pub(crate) fn launch(
        &mut self,
        spawn: impl FnOnce() -> std::io::Result<Child>,
    ) -> Result<(), SidecarError> {
        if self.attempted || self.finished || self.failed {
            return Err(failure());
        }
        self.attempted = true;
        let result = (|| {
            self.profile.revalidate()?;
            self.reservation.verify()?;
            self.child = Some(spawn().map_err(|_| failure())?);
            let child = self.child.as_mut().ok_or_else(failure)?;
            let root = identity_of(child.id()).map_err(|_| failure())?;
            self.tracker = Some(TreeTracker::new(root));
            self.reservation.record(&[root])?;
            self.io = Some(LoginIo::attach(child)?);
            self.observe_with(&SystemProcTable, true)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Caller must tick while waiting for user/browser input (at most 100 ms apart).
    /// Blocking stdio waits also invoke this observer, not just the final shutdown.
    pub(crate) fn tick(&mut self) -> Result<(), SidecarError> {
        let result = if self.failed || self.finished {
            Err(failure())
        } else {
            self.observe_with(&SystemProcTable, false)
        };
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn observe_with(&mut self, table: &impl ProcTable, force: bool) -> Result<(), SidecarError> {
        if !force
            && self
                .last_observed
                .is_some_and(|at| at.elapsed() < OBSERVE_INTERVAL)
        {
            return Ok(());
        }
        let tracker = self.tracker.as_mut().ok_or_else(failure)?;
        let observed = tracker.observe(table);
        // Persist the cumulative set even when this observation invalidated integrity.
        let saved = self.reservation.record(&tracker.tracked_identities());
        if saved.is_err() || observed.is_err() {
            self.evidence_failed = true;
            self.failed = true;
            return Err(failure());
        }
        self.last_observed = Some(Instant::now());
        Ok(())
    }

    pub(crate) fn send(
        &mut self,
        request: LoginRequest,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        if self.failed || self.finished {
            return Err(failure());
        }
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.send_observed(request, timeout, cancel, &mut || self.tick());
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn receive(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<BoundedObservation, SidecarError> {
        if self.failed || self.finished {
            return Err(failure());
        }
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.receive_observed(timeout, cancel, &mut || self.tick());
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// This proves cleanup only, never authentication or model readiness.
    /// Cancel/timeouts can finish cleanly; observation/storage failures cannot.
    pub(crate) fn finish(&mut self) -> Result<(), SidecarError> {
        self.finish_with(&SystemProcTable)
    }

    fn finish_with(&mut self, table: &impl ProcTable) -> Result<(), SidecarError> {
        if self.finished {
            return Err(failure());
        }
        self.io.take();
        let clean = self.cleanup(table);
        if !clean {
            self.failed = true;
            self.evidence_failed = true;
            return Err(failure());
        }
        self.finished = true;
        self.profile.revalidate()?;
        self.reservation.release()
    }

    fn cleanup(&mut self, table: &impl ProcTable) -> bool {
        let Some(child) = self.child.as_mut() else {
            return !self.attempted;
        };
        let Some(tracker) = self.tracker.as_mut() else {
            // Child handle still owns this root. Do not claim its unknown tree gone.
            let _ = child.kill();
            let _ = child.try_wait();
            return false;
        };
        let report =
            tracker.terminate(table, &SystemClock::default(), TerminationConfig::default());
        let saved = self.reservation.record(&report.tracked).is_ok();
        // Never call blocking wait after an unconfirmed termination.
        let reaped = matches!(child.try_wait(), Ok(Some(_)));
        report.confirmed() && saved && reaped && !self.evidence_failed
    }
}

impl Drop for LoginProcess<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.io.take();
            self.cleanup(&SystemProcTable);
        }
        // A crash, early return or dropped owner leaves the durable reservation.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hello_agent_core::proctree::{ProbeResult, ProcEntry, TreeSignal};
    use hello_agent_core::provider::vendor_observation::codex_login::LoginExchange;
    use std::process::{Command, Stdio};

    fn profile(temp: &tempfile::TempDir) -> CodexProfileBinding {
        let path = temp
            .path()
            .canonicalize()
            .unwrap()
            .join("hello-agent.sqlite3");
        let storage = crate::Storage::open(&path).unwrap();
        CodexProfileBinding::from_storage(&storage).unwrap()
    }

    // Owned toy process only; no provider, user auth, browser, project or paid model.
    fn cat() -> std::io::Result<Child> {
        Command::new("/bin/cat")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    }

    #[test]
    fn reservation_precedes_launch_and_confirmed_finish_releases_it() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        assert!(LoginProcess::reserve(&binding).is_err());
        process
            .launch(|| {
                assert!(temp.path().join(PENDING).is_file());
                cat()
            })
            .unwrap();
        let root = process.tracker.as_ref().unwrap().tracked_identities()[0];
        let saved = std::fs::read_to_string(temp.path().join(PENDING)).unwrap();
        assert!(saved.contains(&format!("{} {}", root.pid, root.start_us)));
        let (_, request) = LoginExchange::begin(1).unwrap();
        process
            .send(request, Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process
            .receive(Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process.finish().unwrap();
        assert!(!temp.path().join(PENDING).exists());
        assert!(matches!(SystemProcTable.probe(root), ProbeResult::Gone));
        assert!(process.finish().is_err());
        assert!(process.tick().is_err());
        LoginProcess::reserve(&binding).unwrap().finish().unwrap();
    }

    #[test]
    fn timeout_is_not_success_and_cleanup_is_separate() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch(cat).unwrap();
        let initial_observation = process.last_observed.unwrap();
        assert!(process
            .receive(Duration::from_millis(230), &AtomicBool::new(false))
            .is_err());
        assert!(
            process
                .last_observed
                .unwrap()
                .duration_since(initial_observation)
                >= OBSERVE_INTERVAL
        );
        assert!(process.failed);
        assert!(temp.path().join(PENDING).exists());
        // This clears only the process lock, never grants login readiness.
        process.finish().unwrap();
        assert!(!temp.path().join(PENDING).exists());
    }

    #[test]
    fn tracked_descendant_is_reaped_after_root_exits_first() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process
            .launch(|| {
                Command::new("/bin/sh")
                    .args(["-c", "/bin/sleep 30 & read hold"])
                    .env_clear()
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while process.tracker.as_ref().unwrap().tracked_len() < 2 {
            assert!(Instant::now() < deadline, "fixture child not observed");
            process.observe_with(&SystemProcTable, true).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        let tracked = process.tracker.as_ref().unwrap().tracked_identities();
        let root = process.child.as_ref().unwrap().id();
        let (_, request) = LoginExchange::begin(1).unwrap();
        process
            .send(request, Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while process
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none()
        {
            assert!(Instant::now() < deadline, "fixture root did not exit");
            process.tick().unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(tracked.iter().any(
            |id| id.pid != root && matches!(SystemProcTable.probe(*id), ProbeResult::Alive(_))
        ));
        process.finish().unwrap();
        assert!(tracked
            .iter()
            .all(|id| matches!(SystemProcTable.probe(*id), ProbeResult::Gone)));
    }

    #[test]
    fn incomplete_existing_reservation_and_failed_evidence_never_self_clear() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        std::fs::write(temp.path().join(PENDING), b"").unwrap();
        assert!(LoginProcess::reserve(&binding).is_err());
        assert_eq!(std::fs::read(temp.path().join(PENDING)).unwrap(), b"");
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch(cat).unwrap();
        let oversized: Vec<_> = (1..=257)
            .map(|pid| ProcIdentity { pid, start_us: 1 })
            .collect();
        assert!(process.reservation.record(&oversized).is_err());
        assert!(process.finish().is_err());
        assert!(LoginProcess::reserve(&binding).is_err());
    }

    #[test]
    fn dropped_owner_and_failed_spawn_leave_restart_fence() {
        for spawn in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let mut process = LoginProcess::reserve(&binding).unwrap();
            if spawn {
                process.launch(cat).unwrap();
            }
            drop(process);
            let reopened = profile(&temp);
            assert!(LoginProcess::reserve(&reopened).is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        assert!(process
            .launch(|| Err(std::io::Error::other("SECRET")))
            .is_err());
        assert!(process.launch(|| panic!("must not launch twice")).is_err());
        assert!(process.finish().is_err());
        let saved = std::fs::read_to_string(temp.path().join(PENDING)).unwrap();
        assert!(!saved.contains("SECRET"));
    }

    struct FailedTable;
    impl ProcTable for FailedTable {
        fn list(&self) -> Result<Vec<ProcEntry>, String> {
            Err("SECRET".into())
        }
        fn probe(&self, _: ProcIdentity) -> ProbeResult {
            ProbeResult::Unconfirmable("SECRET".into())
        }
        fn signal(&self, _: u32, _: TreeSignal) -> Result<(), String> {
            panic!("never signal")
        }
    }

    #[test]
    fn observation_failure_remains_poisoned_after_os_recovers() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch(cat).unwrap();
        assert!(process.observe_with(&FailedTable, true).is_err());
        assert!(process.evidence_failed);
        assert!(process.finish().is_err());
        assert!(LoginProcess::reserve(&binding).is_err());
        assert!(!std::fs::read_to_string(temp.path().join(PENDING))
            .unwrap()
            .contains("SECRET"));
    }

    #[test]
    fn uncertain_finish_keeps_cleanup_retryable_but_never_releases_fence() {
        for retry in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let mut process = LoginProcess::reserve(&binding).unwrap();
            process.launch(cat).unwrap();
            let root = process.tracker.as_ref().unwrap().tracked_identities()[0];
            assert!(process.finish_with(&FailedTable).is_err());
            assert!(!process.finished);
            assert!(matches!(SystemProcTable.probe(root), ProbeResult::Alive(_)));
            if retry {
                assert!(process.finish().is_err());
            }
            drop(process);
            assert!(matches!(SystemProcTable.probe(root), ProbeResult::Gone));
            assert!(LoginProcess::reserve(&binding).is_err());
        }
    }

    #[test]
    fn replaced_marker_or_profile_never_releases_foreign_file_or_launches() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch(cat).unwrap();
        std::fs::rename(temp.path().join(PENDING), temp.path().join("old-pending")).unwrap();
        std::fs::write(temp.path().join("sentinel"), b"preserve").unwrap();
        std::os::unix::fs::symlink(temp.path().join("sentinel"), temp.path().join(PENDING))
            .unwrap();
        assert!(process.finish().is_err());
        assert_eq!(
            std::fs::read(temp.path().join("sentinel")).unwrap(),
            b"preserve"
        );
        assert!(LoginProcess::reserve(&binding).is_err());
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        std::fs::remove_file(binding.revalidate().unwrap().join("config.toml")).unwrap();
        assert!(process
            .launch(|| panic!("profile check precedes launch"))
            .is_err());
        assert!(temp.path().join(PENDING).exists());
    }
}
