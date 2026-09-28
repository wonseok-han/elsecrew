//! Internal login lifecycle, not a vendor launch authorization or a public IPC API.
//! A reviewed launcher must supply the isolated child; no existing auth is copied.
//! A durable reservation precedes launch. Uncertain exits retain it across restarts.
//! Like the profile binding, identity checks are point-in-time: hostile same-UID
//! directory/path replacement after a check is outside this capability's guarantee.
use crate::{provider_login_io::LoginIo, provider_profile::LoginProfile, SidecarError};
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

#[cfg(test)]
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
    name: &'static str,
    directory: Dir,
    file: File,
    recorded: BTreeSet<ProcIdentity>,
    failed: bool,
}

impl Reservation {
    fn create(profile: &dyn LoginProfile) -> Result<Self, SidecarError> {
        let directory = profile.login_lease_directory()?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = directory
            .open_with(profile.pending_name(), &options)
            .map_err(|_| failure())?;
        file.write_all(b"hello-agent provider login pending v1\n")
            .map_err(|_| failure())?;
        file.sync_all().map_err(|_| failure())?;
        sync_directory(&directory)?;
        Ok(Self {
            name: profile.pending_name(),
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
            .symlink_metadata(self.name)
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
        self.directory
            .remove_file(self.name)
            .map_err(|_| failure())?;
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

/// Issued only by explicit finish; callers cannot manufacture a clean receipt.
pub(crate) struct ProcessClosureEvidence {
    identities: Vec<ProcIdentity>,
    confirmed: bool,
}

impl ProcessClosureEvidence {
    pub(crate) fn identities(&self) -> &[ProcIdentity] {
        &self.identities
    }
    pub(crate) fn confirmed(&self) -> bool {
        self.confirmed
    }
}

/// Owns the Child before any fallible identity/pipe operation. No Debug/raw error logging.
/// Dropping attempts cleanup but never releases the durable fence: only explicit finish does.
pub(crate) struct LoginProcess<'a> {
    profile: &'a dyn LoginProfile,
    reservation: Reservation,
    child: Option<Child>,
    tracker: Option<TreeTracker>,
    io: Option<LoginIo>,
    last_observed: Option<Instant>,
    failed: bool,
    evidence_failed: bool,
    attempted: bool,
    finished: bool,
    transport_used: bool,
    preflight_claimed: bool,
    closure: Option<ProcessClosureEvidence>,
    retain_for_terminal: bool,
    #[cfg(test)]
    fail_next_cleanup: bool,
}

impl<'a> LoginProcess<'a> {
    /// Trusted run owner only, before claiming this fresh transport. Ordinary
    /// login sessions retain their existing finish-and-release behavior.
    pub(crate) fn retain_fence_until_terminal(&mut self) -> Result<(), SidecarError> {
        if self.finished
            || self.failed
            || self.transport_used
            || self.preflight_claimed
            || self.retain_for_terminal
            || self.io.is_none()
        {
            return Err(failure());
        }
        self.reservation.verify()?;
        self.retain_for_terminal = true;
        Ok(())
    }

    /// Called by the run owner only after its terminal transaction succeeded.
    /// A failed release remains blocked; ordinary finish/drop cannot bypass it.
    pub(crate) fn release_after_run_terminal(&mut self) -> Result<(), SidecarError> {
        if !self.retain_for_terminal
            || !self.finished
            || !self
                .closure
                .as_ref()
                .is_some_and(|receipt| receipt.confirmed)
        {
            return Err(failure());
        }
        self.profile.revalidate()?;
        self.reservation.release()?;
        self.retain_for_terminal = false;
        Ok(())
    }

    pub(crate) fn closure_evidence(&self) -> Option<&ProcessClosureEvidence> {
        self.closure.as_ref()
    }
    pub(crate) fn codex_preflight_home(&self) -> Result<std::path::PathBuf, SidecarError> {
        if self.profile.pending_name() != "codex-login.pending" {
            return Err(failure());
        }
        self.reservation.verify()?;
        self.profile.revalidate()
    }

    pub(crate) fn reserve(profile: &'a dyn LoginProfile) -> Result<Self, SidecarError> {
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
            transport_used: false,
            preflight_claimed: false,
            closure: None,
            retain_for_terminal: false,
            #[cfg(test)]
            fail_next_cleanup: false,
        })
    }

    /// Trusted backend launcher only: spawn must return a child whose vendor
    /// execution is still gated. Release is called once, only after durable
    /// identity recording and a successful observation. Both callbacks must be
    /// bounded; executable/environment validation remains the launcher's job.
    /// No renderer command/path input or default vendor spawn is provided.
    pub(crate) fn launch_with_barrier(
        &mut self,
        spawn: impl FnOnce() -> std::io::Result<Child>,
        release: impl FnOnce(&mut Child) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        self.launch_with_identity_barrier(spawn, |child, _| release(child))
    }

    /// The trusted run owner can durably bind the already observed identities
    /// before releasing the gate. A failed callback never attaches protocol I/O.
    pub(crate) fn launch_with_identity_barrier(
        &mut self,
        spawn: impl FnOnce() -> std::io::Result<Child>,
        release: impl FnOnce(&mut Child, &[ProcIdentity]) -> std::io::Result<()>,
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
            self.observe_with(&SystemProcTable, true)?;
            self.profile.revalidate()?;
            self.reservation.verify()?;
            let child = self.child.as_mut().ok_or_else(failure)?;
            // Do not release an unusable transport, even if the root was recorded.
            if child.stdin.is_none() || child.stdout.is_none() {
                return Err(failure());
            }
            let identities = self
                .tracker
                .as_ref()
                .ok_or_else(failure)?
                .tracked_identities();
            release(child, &identities).map_err(|_| failure())?;
            self.io = Some(LoginIo::attach(child)?);
            self.observe_with(&SystemProcTable, true)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// External durable run bookkeeping failed. Stop all protocol input while
    /// retaining the observed child/tracker for the ordinary explicit cleanup.
    pub(crate) fn invalidate_transport(&mut self) {
        self.failed = true;
    }

    /// Existing toy peers have no vendor payload. Production has no ungated shortcut.
    #[cfg(test)]
    pub(crate) fn launch(
        &mut self,
        spawn: impl FnOnce() -> std::io::Result<Child>,
    ) -> Result<(), SidecarError> {
        self.launch_with_barrier(spawn, |_| Ok(()))
    }

    /// Empty-environment gate for an already vetted backend target. This is not
    /// a Codex launcher: vendor-specific environment/admission is still required.
    pub(crate) fn launch_gated_command(
        &mut self,
        program: &std::path::Path,
        args: &[&std::ffi::OsStr],
        cwd: &std::path::Path,
    ) -> Result<(), SidecarError> {
        self.launch_with_barrier(
            || crate::provider_login_gate::spawn(program, args, cwd),
            crate::provider_login_gate::release,
        )
    }

    /// Compare backend-pinned bytes both before the waiting shell starts and
    /// immediately before release. Signature and vendor closure admission are separate.
    pub(crate) fn launch_pinned_command(
        &mut self,
        executable: &crate::provider_executable::PinnedExecutable,
        args: &[&std::ffi::OsStr],
        cwd: &std::path::Path,
    ) -> Result<(), SidecarError> {
        self.launch_with_barrier(
            || {
                let path = executable
                    .revalidate()
                    .map_err(|_| std::io::Error::other("executable changed"))?;
                crate::provider_login_gate::spawn(path, args, cwd)
            },
            |child| {
                executable
                    .revalidate()
                    .map_err(|_| std::io::Error::other("executable changed"))?;
                crate::provider_login_gate::release(child)
            },
        )
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

    /// Return the cumulative identities even when observation fails, so the run
    /// ledger can retain known descendants without treating the observation as safe.
    pub(crate) fn observe_for_run(&mut self) -> (Vec<ProcIdentity>, Result<(), SidecarError>) {
        let observed = self.tick();
        let identities = self
            .tracker
            .as_ref()
            .map(TreeTracker::tracked_identities)
            .unwrap_or_default();
        (identities, observed)
    }

    fn tick_with_run(
        &mut self,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        let before = self.last_observed;
        let observed = self.tick();
        // I/O polls every few milliseconds; write/read the run ledger only when
        // the process observer advances (or fails), not on every empty poll.
        let saved = if observed.is_err() || before != self.last_observed {
            observer(self)
        } else {
            Ok(())
        };
        observed.and(saved)
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

    /// Reserve an untouched Codex transport for exactly one preflight exchange.
    pub(crate) fn begin_preflight(&mut self) -> Result<(), SidecarError> {
        if self.failed
            || self.finished
            || self.transport_used
            || self.preflight_claimed
            || self.io.is_none()
            || self.profile.pending_name() != "codex-login.pending"
        {
            return Err(failure());
        }
        self.preflight_claimed = true;
        Ok(())
    }

    pub(crate) fn send(
        &mut self,
        request: LoginRequest,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        if self.failed || self.finished || self.preflight_claimed {
            return Err(failure());
        }
        self.transport_used = true;
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.send_observed(request, timeout, cancel, &mut || self.tick());
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn send_preflight(
        &mut self,
        request: hello_agent_core::provider::vendor_observation::codex_preflight::exchange::PreflightRequest,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        self.send_preflight_observed(request, timeout, cancel, &mut |_| Ok(()))
    }

    pub(crate) fn send_preflight_observed(
        &mut self,
        request: hello_agent_core::provider::vendor_observation::codex_preflight::exchange::PreflightRequest,
        timeout: Duration,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if self.failed || self.finished || !self.preflight_claimed {
            return Err(failure());
        }
        self.transport_used = true;
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.send_preflight(request, timeout, cancel, &mut || {
            self.tick_with_run(observer)
        });
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn send_turn_observed(
        &mut self,
        request: hello_agent_core::provider::vendor_observation::codex_turn::TurnRequest,
        timeout: Duration,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if self.failed || self.finished || !self.preflight_claimed {
            return Err(failure());
        }
        self.transport_used = true;
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.send_turn(request, timeout, cancel, &mut || {
            self.tick_with_run(observer)
        });
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
        if self.preflight_claimed {
            return Err(failure());
        }
        self.receive_inner(timeout, cancel, &mut |_| Ok(()))
    }

    pub(crate) fn send_approval_observed(
        &mut self,
        reply: hello_agent_core::provider::vendor_observation::codex_approval::ApprovalReply,
        timeout: Duration,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if self.failed || self.finished || !self.preflight_claimed {
            return Err(failure());
        }
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.send_approval(reply, timeout, cancel, &mut || self.tick_with_run(observer));
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn poll_preflight_observed(
        &mut self,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<Option<BoundedObservation>, SidecarError> {
        if self.failed || self.finished || !self.preflight_claimed {
            return Err(failure());
        }
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.poll_observed(cancel, &mut || self.tick_with_run(observer));
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn receive_preflight(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<BoundedObservation, SidecarError> {
        self.receive_preflight_observed(timeout, cancel, &mut |_| Ok(()))
    }

    pub(crate) fn receive_preflight_observed(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<BoundedObservation, SidecarError> {
        if !self.preflight_claimed {
            return Err(failure());
        }
        self.receive_inner(timeout, cancel, observer)
    }

    fn receive_inner(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<BoundedObservation, SidecarError> {
        if self.failed || self.finished {
            return Err(failure());
        }
        self.transport_used = true;
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.receive_observed(timeout, cancel, &mut || self.tick_with_run(observer));
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Poll without blocking the owner's idle/browser event loop.
    pub(crate) fn poll(
        &mut self,
        cancel: &AtomicBool,
    ) -> Result<Option<BoundedObservation>, SidecarError> {
        if self.failed || self.finished || self.preflight_claimed {
            return Err(failure());
        }
        self.transport_used = true;
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.poll_observed(cancel, &mut || self.tick());
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// This proves cleanup only, never authentication or model readiness.
    pub(crate) fn poll_output(
        &mut self,
        cancel: &AtomicBool,
    ) -> Result<crate::provider_login_io::OutputChunk, SidecarError> {
        if self.preflight_claimed {
            return Err(failure());
        }
        self.transport_used = true;
        self.tick()?;
        let result = self.io.as_mut().ok_or_else(failure)?.poll_chunk(cancel);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn submit_claude_code(
        &mut self,
        code: &str,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        if self.failed
            || self.finished
            || self.preflight_claimed
            || self.profile.pending_name() != "claude-login.pending"
        {
            return Err(failure());
        }
        self.transport_used = true;
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.submit_claude_code(code, cancel, &mut || self.tick());
        self.io = Some(io);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn exit_status(&mut self) -> Result<Option<std::process::ExitStatus>, SidecarError> {
        self.tick()?;
        self.child
            .as_mut()
            .ok_or_else(failure)?
            .try_wait()
            .map_err(|_| failure())
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
        self.closure = Some(ProcessClosureEvidence {
            identities: self
                .tracker
                .as_ref()
                .map(|tracker| tracker.tracked_identities())
                .unwrap_or_default(),
            confirmed: false,
        });
        if !clean {
            self.failed = true;
            self.evidence_failed = true;
            return Err(failure());
        }
        self.finished = true;
        self.profile.revalidate()?;
        if self.retain_for_terminal {
            self.reservation.verify()?;
        } else {
            self.reservation.release()?;
        }
        self.closure.as_mut().unwrap().confirmed = true;
        Ok(())
    }

    fn cleanup(&mut self, table: &impl ProcTable) -> bool {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_cleanup) {
            return false;
        }
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

#[cfg(test)]
impl LoginProcess<'_> {
    pub(crate) fn inject_uncertain_cleanup_once(&mut self) {
        self.fail_next_cleanup = true;
    }
    pub(crate) fn root_for_test(&self) -> hello_agent_core::proctree::ProcIdentity {
        self.tracker.as_ref().unwrap().tracked_identities()[0]
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
    use crate::provider_profile::CodexProfileBinding;
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
    fn pinned_gate_rejects_changed_executable_without_spawning() {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let root = temp.path().canonicalize().unwrap();
        let path = root.join("toy");
        let bytes = b"#!/bin/sh\nexec /bin/cat\n";
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let pin =
            crate::provider_executable::PinnedExecutable::open(&path, Sha256::digest(bytes).into())
                .unwrap();
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch_pinned_command(&pin, &[], &root).unwrap();
        let (_, request) = LoginExchange::begin(1).unwrap();
        process
            .send(request, Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process
            .receive(Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process.finish().unwrap();
        std::fs::write(&path, b"changed").unwrap();
        let mut process = LoginProcess::reserve(&binding).unwrap();
        assert!(process.launch_pinned_command(&pin, &[], &root).is_err());
        assert!(process.child.is_none());
        // Existing failed-launch policy retains the durable fence, even when
        // no child was returned. Do not silently weaken that recovery contract.
        assert!(process.finish().is_err());
        assert!(temp.path().join(PENDING).exists());
    }

    #[test]
    fn pin_change_while_gate_waits_prevents_release_and_drop_keeps_fence() {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let root = temp.path().canonicalize().unwrap();
        let path = root.join("toy");
        let bytes = b"#!/bin/sh\ntouch EXECUTED\n";
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let pin =
            crate::provider_executable::PinnedExecutable::open(&path, Sha256::digest(bytes).into())
                .unwrap();
        let mut process = LoginProcess::reserve(&binding).unwrap();
        let mut released = false;
        assert!(process
            .launch_with_barrier(
                || {
                    let child =
                        crate::provider_login_gate::spawn(pin.revalidate().unwrap(), &[], &root)?;
                    std::fs::write(&path, b"changed")?;
                    Ok(child)
                },
                |child| {
                    pin.revalidate()
                        .map_err(|_| std::io::Error::other("changed"))?;
                    released = true;
                    crate::provider_login_gate::release(child)
                },
            )
            .is_err());
        let identity = process.root_for_test();
        assert!(!released);
        drop(process);
        assert!(matches!(SystemProcTable.probe(identity), ProbeResult::Gone));
        assert!(!root.join("EXECUTED").exists());
        assert!(root.join(PENDING).exists());
    }

    #[test]
    fn gated_command_preserves_literal_arguments_and_cleans_tracked_root() {
        use std::{ffi::OsStr, path::Path};
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process
            .launch_gated_command(Path::new("/bin/cat"), &[], temp.path())
            .unwrap();
        let root = process.root_for_test();
        let (_, request) = LoginExchange::begin(1).unwrap();
        process
            .send(request, Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process
            .receive(Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process.finish().unwrap();
        assert!(matches!(SystemProcTable.probe(root), ProbeResult::Gone));

        // Shell metacharacters in arguments remain literal. No command interpolation.
        let mut child = crate::provider_login_gate::spawn(
            Path::new("/usr/bin/printf"),
            &[
                OsStr::new("%s"),
                OsStr::new("$(touch SHOULD_NOT_EXIST); 'quoted'"),
            ],
            temp.path(),
        )
        .unwrap();
        crate::provider_login_gate::release(&mut child).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"$(touch SHOULD_NOT_EXIST); 'quoted'");
        assert!(!temp.path().join("SHOULD_NOT_EXIST").exists());
    }

    #[test]
    fn unreleased_gate_eof_or_wrong_token_never_executes_target() {
        use std::{ffi::OsStr, path::Path};
        let temp = tempfile::tempdir().unwrap();
        for wrong in [false, true] {
            let mut child = crate::provider_login_gate::spawn(
                Path::new("/usr/bin/touch"),
                &[OsStr::new("SHOULD_NOT_EXIST")],
                temp.path(),
            )
            .unwrap();
            if wrong {
                child.stdin.as_mut().unwrap().write_all(b"wrong\n").unwrap();
            }
            drop(child.stdin.take());
            assert!(!child.wait().unwrap().success());
            assert!(!temp.path().join("SHOULD_NOT_EXIST").exists());
        }
        assert!(crate::provider_login_gate::spawn(Path::new("cat"), &[], temp.path()).is_err());
    }

    #[test]
    fn barrier_release_follows_durable_identity_and_precedes_protocol() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        let mut released = false;
        process
            .launch_with_barrier(
                || {
                    Command::new("/bin/sh")
                        .args([
                            "-c",
                            "IFS= read -r gate && test \"$gate\" = release && exec /bin/cat",
                        ])
                        .env_clear()
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                },
                |child| {
                    let root = identity_of(child.id()).unwrap();
                    let saved = std::fs::read_to_string(temp.path().join(PENDING)).unwrap();
                    assert!(saved.contains(&format!("{} {}", root.pid, root.start_us)));
                    assert!(!released);
                    released = true;
                    child.stdin.as_mut().unwrap().write_all(b"release\n")
                },
            )
            .unwrap();
        assert!(released);
        let (_, request) = LoginExchange::begin(1).unwrap();
        process
            .send(request, Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process
            .receive(Duration::from_secs(1), &AtomicBool::new(false))
            .unwrap();
        process.finish().unwrap();
        assert!(!temp.path().join(PENDING).exists());
    }

    #[test]
    fn failed_preparation_never_releases_and_release_failure_retains_child_for_cleanup() {
        for fail_before_release in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let mut process = LoginProcess::reserve(&binding).unwrap();
            let mut released = false;
            let result = process.launch_with_barrier(
                || {
                    let mut command = Command::new("/bin/cat");
                    command
                        .env_clear()
                        .stdin(Stdio::piped())
                        .stderr(Stdio::null());
                    command.stdout(if fail_before_release {
                        Stdio::null()
                    } else {
                        Stdio::piped()
                    });
                    command.spawn()
                },
                |_| {
                    released = true;
                    Err(std::io::Error::other("synthetic release failure"))
                },
            );
            assert!(result.is_err());
            assert_eq!(released, !fail_before_release);
            assert!(process.failed);
            let root = process.tracker.as_ref().unwrap().tracked_identities()[0];
            assert!(process.child.is_some());
            assert!(process
                .launch_with_barrier(cat, |_| panic!("second release"))
                .is_err());
            process.finish().unwrap();
            assert!(matches!(SystemProcTable.probe(root), ProbeResult::Gone));
        }
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
            // Closing stdin must not end the peer before the injected OS
            // observation failure is checked (cat raced EOF with this assertion).
            process
                .launch(|| {
                    Command::new("/bin/sleep")
                        .arg("30")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                })
                .unwrap();
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
