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
use std::os::fd::AsRawFd;
use std::process::{Child, Command};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[cfg(test)]
const PENDING: &str = "codex-login.pending";
const MAX_TRACKED: usize = 256;
// A future explicit recovery may inspect this evidence, but its presence alone
// never admits work or releases an existing reservation.
const LOGIN_TERMINATED: &[u8] = b"login-processes-terminated-locked-v2\n";
const OBSERVE_INTERVAL: Duration = Duration::from_millis(100);

mod run_binding;
pub(crate) use run_binding::{recover_bound_run, RunFenceBinding};

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
        lock_exclusive(&file)?;
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

    fn record_login_termination(&mut self) -> Result<(), SidecarError> {
        let result = (|| {
            if self.failed {
                return Err(failure());
            }
            self.verify()?;
            self.file
                .write_all(LOGIN_TERMINATED)
                .map_err(|_| failure())?;
            self.file.sync_all().map_err(|_| failure())?;
            self.verify()
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
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

fn lock_exclusive(file: &File) -> Result<(), SidecarError> {
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(failure());
    }
    Ok(())
}

/// Explicit recovery only. Legacy/uncertain records remain fenced.
/// A v2 marker proves the writer held the same exclusive lock until it exited.
pub(crate) fn recover_confirmed_login(profile: &dyn LoginProfile) -> Result<(), SidecarError> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
    use std::io::Read;
    let home = profile.revalidate()?;
    let directory = profile.login_lease_directory()?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let mut file = directory
        .open_with(profile.pending_name(), &options)
        .map_err(|_| failure())?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_file() || meta.len() > 16 * 1024 {
        return Err(failure());
    }
    lock_exclusive(&file)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 != meta.len()
        || !bytes.ends_with(LOGIN_TERMINATED)
        || bytes.contains(&b'\r')
    {
        return Err(failure());
    }
    let text = std::str::from_utf8(&bytes[..bytes.len() - LOGIN_TERMINATED.len()])
        .map_err(|_| failure())?;
    if !text.ends_with('\n') {
        return Err(failure());
    }
    let mut lines = text.lines();
    if lines.next() != Some("hello-agent provider login pending v1") {
        return Err(failure());
    }
    let mut recorded = BTreeSet::new();
    for line in lines {
        let (pid, start) = line.split_once(' ').ok_or_else(failure)?;
        let identity = ProcIdentity {
            pid: pid.parse().map_err(|_| failure())?,
            start_us: start.parse().map_err(|_| failure())?,
        };
        if identity.pid <= 1
            || identity.start_us <= 0
            || line != format!("{} {}", identity.pid, identity.start_us)
            || !recorded.insert(identity)
            || recorded.len() > MAX_TRACKED
        {
            return Err(failure());
        }
    }
    let reservation = Reservation {
        name: profile.pending_name(),
        directory,
        file,
        recorded,
        failed: false,
    };
    reservation.verify()?;
    for identity in &reservation.recorded {
        if !matches!(
            SystemProcTable.probe(*identity),
            hello_agent_core::proctree::ProbeResult::Gone
        ) {
            return Err(failure());
        }
    }
    if profile.revalidate()? != home
        || !crate::provider_profile::same_directory(
            &reservation.directory,
            &profile.login_lease_directory()?,
        )?
    {
        return Err(failure());
    }
    // Same inode remains exclusively locked through validated unlink and sync.
    reservation.release()
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
    termination_confirmed: bool,
    confirmed: bool,
}

impl ProcessClosureEvidence {
    pub(crate) fn identities(&self) -> &[ProcIdentity] {
        &self.identities
    }
    pub(crate) fn confirmed(&self) -> bool {
        self.confirmed
    }
    /// Diagnostic only: tracked termination/reaping and evidence persistence.
    /// Profile/fence validation may still have failed.
    /// Never use this in place of confirmed() to release a fence or admit work.
    pub(crate) fn termination_confirmed(&self) -> bool {
        self.termination_confirmed
    }
}

/// Owns the Child before any fallible identity/pipe operation. No Debug/raw error logging.
/// Dropping attempts cleanup but never releases the durable fence: only explicit finish does.
pub(crate) struct LoginProcess<'a> {
    connection_id: uuid::Uuid,
    profile: ProfileOwner<'a>,
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
    claude_preflight: bool,
    closure: Option<ProcessClosureEvidence>,
    retain_for_terminal: bool,
    startup_run_id: Option<String>,
    cleanup_started: Option<Instant>,
    // Own the service with the process; uncertain cleanup quarantines its bound
    // listener after closing relays/jobs. Sidecar crashes remain a separate risk.
    model_proxy: Option<crate::provider_model_proxy::broker::Broker>,
    runtime_scope: Option<crate::provider_runtime_sandbox::RuntimeScope>,
    named_scope: Option<crate::provider_preflight_session::vendor_launch::Scope>,
    claude_scope: Option<crate::provider_claude_session::launch::Scope>,
    #[cfg(test)]
    fail_next_cleanup: bool,
    #[cfg(test)]
    cleanup_identity_once: Option<ProcIdentity>,
}

/// An owned binding retains directory capabilities, never just a path to reopen.
/// Borrowed bindings preserve the existing Codex/Claude login API.
enum ProfileOwner<'a> {
    Borrowed(&'a dyn LoginProfile),
    Codex(crate::provider_profile::CodexProfileBinding),
    Claude(crate::provider_claude_profile::ClaudeProfileBinding),
}

impl ProfileOwner<'_> {
    fn as_ref(&self) -> &dyn LoginProfile {
        match self {
            Self::Borrowed(profile) => *profile,
            Self::Codex(profile) => profile,
            Self::Claude(profile) => profile,
        }
    }
}

impl LoginProcess<'static> {
    /// Retain Claude's verified directory handles across dispatcher requests.
    /// This only reserves the existing shared login/runtime fence, not a launch.
    pub(crate) fn reserve_claude_owned(
        profile: crate::provider_claude_profile::ClaudeProfileBinding,
    ) -> Result<Self, SidecarError> {
        Self::reserve_profile(ProfileOwner::Claude(profile))
    }

    /// Move the already verified profile capability into the process lifetime.
    /// Suitable for retention between dispatcher requests; not launch authority.
    pub(crate) fn reserve_codex_owned(
        profile: crate::provider_profile::CodexProfileBinding,
    ) -> Result<Self, SidecarError> {
        Self::reserve_profile(ProfileOwner::Codex(profile))
    }
}

impl<'a> LoginProcess<'a> {
    pub(crate) fn connection_id(&self) -> uuid::Uuid {
        self.connection_id
    }
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

    /// Retain the profile fence before spawn until startup's durable terminal
    /// record exists, including failures before protocol I/O is attached.
    pub(crate) fn retain_fence_before_launch(&mut self, run_id: &str) -> Result<(), SidecarError> {
        if run_id.is_empty() || self.attempted || self.finished || self.failed || self.retain_for_terminal {
            return Err(failure());
        }
        self.profile.as_ref().revalidate()?;
        self.reservation.verify()?;
        self.retain_for_terminal = true;
        self.startup_run_id = Some(run_id.into());
        Ok(())
    }

    pub(crate) fn retains_run_fence(&self) -> bool { self.retain_for_terminal }

    pub(crate) fn matches_startup_run(&self, run_id: &str) -> bool {
        self.retain_for_terminal && self.startup_run_id.as_deref() == Some(run_id)
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
        self.profile.as_ref().revalidate()?;
        self.reservation.release()?;
        self.retain_for_terminal = false;
        Ok(())
    }

    pub(crate) fn closure_evidence(&self) -> Option<&ProcessClosureEvidence> {
        self.closure.as_ref()
    }

    /// Validate the owned Codex profile after the inspection reservation was
    /// released. This cannot be used to resume or launch the finished process.
    pub(crate) fn closed_codex_home(&self) -> Result<std::path::PathBuf, SidecarError> {
        if !self.finished || self.retain_for_terminal
            || !self.closure.as_ref().is_some_and(|receipt| receipt.confirmed)
            || self.profile.as_ref().pending_name() != "codex-login.pending"
        {
            return Err(failure());
        }
        self.profile.as_ref().revalidate()
    }
    pub(crate) fn codex_preflight_home(&self) -> Result<std::path::PathBuf, SidecarError> {
        if self.profile.as_ref().pending_name() != "codex-login.pending" {
            return Err(failure());
        }
        self.reservation.verify()?;
        self.profile.as_ref().revalidate()
    }

    pub(crate) fn claude_preflight_home(&self) -> Result<std::path::PathBuf, SidecarError> {
        if self.profile.as_ref().pending_name() != "claude-login.pending" {
            return Err(failure());
        }
        self.reservation.verify()?;
        self.profile.as_ref().revalidate()
    }

    pub(crate) fn closed_claude_home(&self) -> Result<std::path::PathBuf, SidecarError> {
        if !self.finished || self.retain_for_terminal
            || !self.closure.as_ref().is_some_and(|receipt| receipt.confirmed)
            || self.profile.as_ref().pending_name() != "claude-login.pending"
        {
            return Err(failure());
        }
        self.profile.as_ref().revalidate()
    }

    pub(crate) fn reserve(profile: &'a dyn LoginProfile) -> Result<Self, SidecarError> {
        Self::reserve_profile(ProfileOwner::Borrowed(profile))
    }

    fn reserve_profile(profile: ProfileOwner<'a>) -> Result<Self, SidecarError> {
        let reservation = Reservation::create(profile.as_ref())?;
        Ok(Self {
            connection_id: uuid::Uuid::new_v4(),
            profile,
            reservation,
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
            claude_preflight: false,
            closure: None,
            retain_for_terminal: false,
            startup_run_id: None,
            cleanup_started: None,
            model_proxy: None,
            runtime_scope: None,
            named_scope: None,
            claude_scope: None,
            #[cfg(test)]
            fail_next_cleanup: false,
            #[cfg(test)]
            cleanup_identity_once: None,
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

    #[cfg(test)]
    pub(super) fn launch_with_model_proxy(
        &mut self,
        command: Command,
        broker: crate::provider_model_proxy::broker::Broker,
        release: impl FnOnce(&mut Child) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        self.launch_with_model_proxy_identity(command, broker, |child, _| release(child))
    }

    pub(super) fn launch_with_model_proxy_identity(
        &mut self,
        mut command: Command,
        broker: crate::provider_model_proxy::broker::Broker,
        release: impl FnOnce(&mut Child, &[ProcIdentity]) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        if self.attempted || self.finished || self.failed || self.model_proxy.is_some() {
            return Err(failure());
        }
        self.model_proxy = Some(broker);
        let result = self.launch_with_identity_barrier(|| command.spawn(), release);
        if result.is_err() {
            self.stop_model_proxy();
        }
        result
    }

    /// The scope is sealed by the fixed sandbox command builder. A raw command,
    /// proxy listener, or successful preflight cannot manufacture this evidence.
    pub(super) fn launch_isolated_model_proxy(
        &mut self,
        command: Command,
        broker: crate::provider_model_proxy::broker::Broker,
        scope: crate::provider_runtime_sandbox::RuntimeScope,
        release: impl FnOnce(&mut Child, &[ProcIdentity]) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        self.launch_with_model_proxy_identity(command, broker, release)?;
        self.runtime_scope = Some(scope);
        Ok(())
    }

    /// Provenance only. The caller must still verify the live process, current
    /// role/workspace/fence and explicit bound user decision before any reply.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn matches_runtime_scope(
        &self,
        workspace: &std::path::Path,
        access: crate::provider_runtime_sandbox::WorkspaceAccess,
    ) -> bool {
        !self.failed
            && !self.finished
            && self.cleanup_started.is_none()
            && self.io.is_some()
            && self.child.is_some()
            && self
                .runtime_scope
                .as_ref()
                .is_some_and(|scope| scope.matches(workspace, access))
    }

    pub(crate) fn launch_named_codex(
        &mut self, mut command: Command,
        scope: crate::provider_preflight_session::vendor_launch::Scope,
        release: impl FnOnce(&mut Child, &[ProcIdentity]) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        scope.revalidate()?;
        self.launch_with_identity_barrier(|| command.spawn(), |child, identities| {
            scope.revalidate().map_err(std::io::Error::other)?;
            release(child, identities)
        })?;
        self.named_scope = Some(scope);
        Ok(())
    }

    pub(crate) fn matches_named_scope(
        &self, workspace: &std::path::Path,
        permission: hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedPermission,
    ) -> bool {
        !self.failed && !self.finished && self.cleanup_started.is_none()
            && self.io.is_some() && self.child.is_some()
            && self.codex_preflight_home().ok().is_some_and(|home| {
                self.named_scope.as_ref().is_some_and(|scope| scope.matches(workspace, &home, permission))
            })
    }

    pub(crate) fn uses_named_scope(&self) -> bool {
        self.named_scope.is_some()
    }

    pub(crate) fn launch_claude(
        &mut self, mut command: Command,
        scope: crate::provider_claude_session::launch::Scope,
        release: impl FnOnce(&mut Child, &[ProcIdentity]) -> std::io::Result<()>,
    ) -> Result<(), SidecarError> {
        self.claude_preflight_home()?;
        scope.revalidate()?;
        self.launch_with_identity_barrier(|| command.spawn(), |child, identities| {
            scope.revalidate().map_err(std::io::Error::other)?;
            release(child, identities)
        })?;
        self.claude_scope = Some(scope);
        Ok(())
    }

    /// Invocation identity only; this does not certify provider tool confinement.
    pub(crate) fn matches_claude_scope(&self, workspace: &std::path::Path, model: &str, effort: Option<&str>, access: crate::provider_claude_session::ToolAccess) -> bool {
        self.verify_claude_scope(workspace, model, effort, access).is_ok()
    }

    pub(crate) fn verify_claude_scope(&self, workspace: &std::path::Path, model: &str, effort: Option<&str>, access: crate::provider_claude_session::ToolAccess) -> Result<(), SidecarError> {
        let unavailable = || crate::provider_claude_session::initialization_failure("launch scope owner");
        if self.failed || self.finished || self.cleanup_started.is_some()
            || self.io.is_none() || self.child.is_none() { return Err(unavailable()); }
        let home = self.claude_preflight_home()?;
        let pid = self.child.as_ref().ok_or_else(unavailable)?.id();
        let owner = self.tracked_for_cleanup().into_iter().find(|identity| identity.pid == pid).ok_or_else(unavailable)?;
        self.claude_scope.as_ref().ok_or_else(unavailable)?.verify(workspace, &home, model, effort, access, owner)
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
            self.profile.as_ref().revalidate()?;
            self.reservation.verify()?;
            self.child = Some(spawn().map_err(|_| failure())?);
            let child = self.child.as_mut().ok_or_else(failure)?;
            let root = identity_of(child.id()).map_err(|_| failure())?;
            self.tracker = Some(TreeTracker::new(root));
            self.reservation.record(&[root])?;
            self.observe_with(&SystemProcTable, true)?;
            self.profile.as_ref().revalidate()?;
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
        self.stop_model_proxy();
    }

    pub(crate) fn tracked_for_cleanup(&self) -> Vec<ProcIdentity> {
        self.tracker
            .as_ref()
            .map(TreeTracker::tracked_identities)
            .unwrap_or_default()
    }

    /// The run ledger failed to durably retain cleanup observations. Even a
    /// subsequently clean OS observation cannot release either safety fence.
    pub(crate) fn invalidate_cleanup_evidence(&mut self) {
        self.failed = true;
        self.evidence_failed = true;
        self.stop_model_proxy();
        if let Some(receipt) = &mut self.closure {
            receipt.confirmed = false;
        }
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
            let proxy = self.model_proxy.as_mut().map_or(Ok(()), |broker| {
                broker.tick(Instant::now(), false).map_err(|_| failure())
            });
            proxy.and_then(|_| self.observe_with(&SystemProcTable, false))
        };
        if result.is_err() {
            self.failed = true;
            self.stop_model_proxy();
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
        self.begin_native_preflight(false)
    }

    pub(crate) fn begin_claude_preflight(&mut self) -> Result<(), SidecarError> {
        self.begin_native_preflight(true)
    }

    fn begin_native_preflight(&mut self, claude: bool) -> Result<(), SidecarError> {
        if self.failed
            || self.finished
            || self.transport_used
            || self.preflight_claimed
            || self.io.is_none()
            || self.profile.as_ref().pending_name()
                != if claude {
                    "claude-login.pending"
                } else {
                    "codex-login.pending"
                }
        {
            return Err(failure());
        }
        self.preflight_claimed = true;
        self.claude_preflight = claude;
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

    /// Compiled out of the product; retains the normal process/IO owner.
    #[cfg(test)]
    pub(crate) fn send_hook_probe(
        &mut self,
        request: crate::provider_login_io::HookProbeRequest<'_>,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        if self.failed || self.finished || !self.preflight_claimed || self.claude_preflight {
            return Err(failure());
        }
        self.transport_used = true;
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.send_hook_probe(request, cancel, &mut || self.tick());
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
        if self.failed || self.finished || !self.preflight_claimed || self.claude_preflight {
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

    pub(crate) fn queue_native(
        &mut self,
        frame: crate::provider_login_io::NativeFrame,
    ) -> Result<(), SidecarError> {
        if self.failed
            || self.finished
            || !self.preflight_claimed
            || self.claude_preflight
                != matches!(
                    &frame,
                    crate::provider_login_io::NativeFrame::ClaudeInitialize(_)
                        | crate::provider_login_io::NativeFrame::ClaudeAllow(_)
                        | crate::provider_login_io::NativeFrame::ClaudeDenial(_)
                        | crate::provider_login_io::NativeFrame::ClaudePrompt(_)
                )
            || matches!(&frame, crate::provider_login_io::NativeFrame::ClaudeDenial(reply)
                if reply.connection() != self.connection_id())
            || matches!(&frame, crate::provider_login_io::NativeFrame::ClaudeAllow(reply)
                if reply.connection() != self.connection_id())
            || matches!(&frame, crate::provider_login_io::NativeFrame::ClaudePrompt(prompt)
                if prompt.connection() != self.connection_id())
        {
            return Err(failure());
        }
        self.transport_used = true;
        let result = self.io.as_mut().ok_or_else(failure)?.queue_native(frame);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn flush_native_observed(
        &mut self,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut Self) -> Result<(), SidecarError>,
    ) -> Result<bool, SidecarError> {
        if self.failed || self.finished || !self.preflight_claimed {
            return Err(failure());
        }
        let mut io = self.io.take().ok_or_else(failure)?;
        let result = io.flush_native(cancel, &mut || self.tick_with_run(observer));
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

    /// Test-only fixed offline probe. Never exposes raw observations in product.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn receive_permission_probe(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<serde_json::Value, SidecarError> {
        let result = (|| {
            if !self.preflight_claimed || self.failed || self.finished {
                return Err(failure());
            }
            let started = Instant::now();
            let mut bytes = Vec::new();
            loop {
                if started.elapsed() >= timeout {
                    return Err(failure());
                }
                self.tick()?;
                match self
                    .io
                    .as_mut()
                    .ok_or_else(failure)?
                    .poll_permission_probe_chunk(cancel)?
                {
                    crate::provider_login_io::OutputChunk::Pending => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    crate::provider_login_io::OutputChunk::Eof => {
                        return Err(SidecarError::new(
                            "permissionProbeFailed",
                            "probe stdout EOF",
                        ))
                    }
                    crate::provider_login_io::OutputChunk::Bytes(chunk) => {
                        if bytes.len() + chunk.len() > 64 * 1024 {
                            return Err(failure());
                        }
                        bytes.extend_from_slice(&chunk);
                        if bytes.ends_with(b"\n") {
                            return serde_json::from_slice(&bytes).map_err(|error| {
                                SidecarError::new(
                                    "permissionProbeFailed",
                                    format!(
                                        "probe JSON {:?} at {}:{} ({} lines)",
                                        error.classify(),
                                        error.line(),
                                        error.column(),
                                        bytes.iter().filter(|byte| **byte == b'\n').count()
                                    ),
                                )
                            });
                        }
                    }
                }
            }
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
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
            || self.profile.as_ref().pending_name() != "claude-login.pending"
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
        if !self.finished && self.closure.is_some() {
            // Explicit retry may still reclaim survivors, but cannot erase the
            // failed receipt or turn a previously uncertain fence into success.
            self.cleanup(table);
            let identities = self.tracked_for_cleanup();
            self.closure.as_mut().unwrap().identities = identities;
            return Err(failure());
        }
        loop {
            if self.poll_finish_with(table)? {
                return Ok(());
            }
            std::thread::sleep(OBSERVE_INTERVAL);
        }
    }

    /// One cleanup observation, without waiting through TERM/KILL grace periods.
    /// false means still closing; no terminal receipt or fence release exists yet.
    pub(crate) fn poll_finish(&mut self) -> Result<bool, SidecarError> {
        self.poll_finish_with(&SystemProcTable)
    }

    fn poll_finish_with(&mut self, table: &impl ProcTable) -> Result<bool, SidecarError> {
        if self.finished || self.closure.is_some() {
            return Err(failure());
        }
        self.stop_model_proxy();
        self.io.take();
        self.failed = true; // No further launch or protocol I/O during cleanup.
        let Some(clean) = self.poll_cleanup(table) else {
            return Ok(false);
        };
        self.closure = Some(ProcessClosureEvidence {
            identities: self
                .tracker
                .as_ref()
                .map(|tracker| tracker.tracked_identities())
                .unwrap_or_default(),
            termination_confirmed: clean,
            confirmed: false,
        });
        if !clean {
            self.failed = true;
            self.evidence_failed = true;
            return Err(failure());
        }
        if !self.retain_for_terminal
            || matches!(self.profile.as_ref().pending_name(),
                "provider-install.pending" | "provider-install-execute.pending")
        {
            // Only after tracked termination/reaping and identity persistence.
            // Installation retains this proven-closed fence until its journal
            // is terminal; explicit restart recovery can inspect this marker.
            // Agent run reservations still rely on their terminal DB transaction.
            self.reservation.record_login_termination()?;
        }
        self.finished = true;
        self.profile.as_ref().revalidate()?;
        if self.retain_for_terminal {
            self.reservation.verify()?;
        } else {
            self.reservation.release()?;
        }
        self.closure.as_mut().unwrap().confirmed = true;
        self.model_proxy.take();
        Ok(true)
    }

    fn poll_cleanup(&mut self, table: &impl ProcTable) -> Option<bool> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_cleanup) {
            return Some(false);
        }
        let Some(child) = self.child.as_mut() else {
            return Some(!self.attempted);
        };
        let Some(tracker) = self.tracker.as_mut() else {
            let _ = child.kill();
            let _ = child.try_wait();
            return Some(false);
        };
        let started = self.cleanup_started.get_or_insert_with(Instant::now);
        #[cfg(test)]
        if let Some(identity) = self.cleanup_identity_once.take() {
            tracker.track_identity(identity);
        }
        let config = TerminationConfig::default();
        let report = tracker.poll_termination(table, started.elapsed().as_millis() as u64, config);
        // Newly discovered descendants must survive a crash between polls too.
        if self
            .reservation
            .record(&tracker.tracked_identities())
            .is_err()
        {
            self.evidence_failed = true;
        }
        let report = report?;
        let reaped = child.try_wait();
        if !report.confirmed() || self.evidence_failed {
            return Some(false);
        }
        match reaped {
            Ok(Some(_)) => Some(true),
            // macOS can report the identity Gone before waitpid exposes status.
            // Keep the fence and listener, sharing the original cleanup deadline.
            Ok(None)
                if started.elapsed()
                    < Duration::from_millis(config.term_grace_ms + config.kill_grace_ms) =>
            {
                None
            }
            Ok(None) | Err(_) => Some(false),
        }
    }

    fn cleanup(&mut self, table: &impl ProcTable) -> bool {
        self.stop_model_proxy();
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_cleanup) {
            return false;
        }
        let Some(child) = self.child.as_mut() else {
            return !self.attempted;
        };
        let Some(tracker) = self.tracker.as_mut() else {
            let _ = child.kill();
            let _ = child.try_wait();
            return false;
        };
        // terminate starts a fresh best-effort reclamation attempt without
        // clearing earlier integrity failures or upgrading failed receipts.
        let started = Instant::now();
        let config = TerminationConfig::default();
        let report = tracker.terminate(table, &SystemClock::default(), config);
        let saved = self.reservation.record(&report.tracked).is_ok();
        loop {
            let reaped = child.try_wait();
            if !report.confirmed() || !saved || self.evidence_failed {
                return false;
            }
            match reaped {
                Ok(Some(_)) => return true,
                Ok(None)
                    if started.elapsed()
                        < Duration::from_millis(config.term_grace_ms + config.kill_grace_ms) => {}
                Ok(None) | Err(_) => return false,
            }
            std::thread::sleep(OBSERVE_INTERVAL);
        }
    }

    fn stop_model_proxy(&mut self) {
        if let Some(broker) = self.model_proxy.as_mut() {
            broker.stop();
        }
    }
}

#[cfg(test)]
impl LoginProcess<'_> {
    pub(crate) fn inject_cleanup_identity_once(&mut self, identity: ProcIdentity) {
        self.cleanup_identity_once = Some(identity);
    }
    pub(crate) fn inject_uncertain_cleanup_once(&mut self) {
        self.fail_next_cleanup = true;
    }
    pub(crate) fn root_for_test(&self) -> hello_agent_core::proctree::ProcIdentity {
        self.tracker.as_ref().unwrap().tracked_identities()[0]
    }
}

impl Drop for LoginProcess<'_> {
    fn drop(&mut self) {
        let clean = if !self.finished {
            self.io.take();
            self.cleanup(&SystemProcTable)
        } else {
            true // finished is set only after clean process termination.
        };
        if !clean {
            if let Some(broker) = self.model_proxy.take() {
                // Uncertain survivors must not gain access to a replacement local
                // server. Quarantine this descriptor until sidecar process exit.
                // This is not crash-survival protection or broker authentication.
                std::mem::forget(broker.into_stopped_listener());
            }
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

    #[test]
    fn run_fence_binding_is_durable_and_does_not_authorize_login_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let mut process = LoginProcess::reserve_codex_owned(profile(&temp)).unwrap();
        process.retain_fence_before_launch("run-bound").unwrap();
        let binding = process.bind_run_fence("run-bound").unwrap();
        binding.validate("run-bound", "codex").unwrap();
        assert!(binding.validate("other-run", "codex").is_err());
        assert!(binding.validate("run-bound", "claude-code").is_err());
        let bytes = std::fs::read(temp.path().join(PENDING)).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        let line = text.lines().nth(1).unwrap().strip_prefix("run-binding-v1 ").unwrap();
        let value = serde_json::from_str(line).unwrap();
        assert_eq!(RunFenceBinding::from_value(&value).unwrap(), binding);
        let mut invalid = value;
        invalid["extra"] = true.into();
        assert!(RunFenceBinding::from_value(&invalid).is_err());
        process.finish().unwrap();
        drop(process);
        assert!(recover_confirmed_login(&profile(&temp)).is_err());
        assert_eq!(std::fs::read(temp.path().join(PENDING)).unwrap(), bytes);
    }

    #[test]
    fn bound_run_recovery_rejects_unsafe_or_unfinished_fences() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        for case in ["live", "nonce", "inode", "directory", "provider", "run", "duplicate", "partial", "legacy", "permissions", "symlink"] {
            let temp = tempfile::tempdir().unwrap();
            let binding_profile = profile(&temp);
            let mut process = LoginProcess::reserve(&binding_profile).unwrap();
            process.retain_fence_before_launch("run-bound").unwrap();
            let binding = process.bind_run_fence("run-bound").unwrap();
            let own = identity_of(std::process::id()).unwrap();
            process.reservation.record(&[own]).unwrap();
            process.finish().unwrap();
            drop(process);
            let path = temp.path().join(PENDING);
            let original = std::fs::read_to_string(&path).unwrap();
            let mut value = binding.to_value();
            match case {
                "nonce" => value["nonce"] = uuid::Uuid::new_v4().to_string().into(),
                "inode" => value["inode"] = (value["inode"].as_u64().unwrap() + 1).into(),
                "directory" => value["directoryInode"] = (value["directoryInode"].as_u64().unwrap() + 1).into(),
                "provider" => value["provider"] = "claude-code".into(),
                "run" => value["runId"] = "another-run".into(),
                _ => {},
            }
            let mut bytes = format!("hello-agent provider login pending v1\nrun-binding-v1 {}\n{} {}\n", serde_json::to_string(&value).unwrap(), own.pid, own.start_us);
            match case {
                "duplicate" => bytes.push_str(&format!("{} {}\n", own.pid, own.start_us)),
                "partial" => { bytes.pop(); },
                "legacy" => bytes = format!("hello-agent provider login pending v1\n{} {}\n", own.pid, own.start_us),
                _ => {},
            }
            std::fs::write(&path, &bytes).unwrap();
            if case == "permissions" { std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap(); }
            if case == "symlink" {
                let held = temp.path().join("held.pending");
                std::fs::rename(&path, &held).unwrap();
                symlink(&held, &path).unwrap();
            }
            let result = recover_bound_run(&binding_profile, |observed, identities| {
                if observed != &binding || identities != &BTreeSet::from([own]) { return Err(failure()); }
                Ok(())
            });
            assert!(result.is_err(), "{case}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes, "{case}");
            assert!(original.contains("run-binding-v1"));
        }
    }

    #[test]
    fn run_fence_binding_rejects_rebinding_without_overwriting_record() {
        let temp = tempfile::tempdir().unwrap();
        let mut process = LoginProcess::reserve_codex_owned(profile(&temp)).unwrap();
        process.retain_fence_before_launch("run-bound").unwrap();
        process.bind_run_fence("run-bound").unwrap();
        let bytes = std::fs::read(temp.path().join(PENDING)).unwrap();
        assert!(process.bind_run_fence("run-bound").is_err());
        assert_eq!(std::fs::read(temp.path().join(PENDING)).unwrap(), bytes);
        assert!(process.release_after_run_terminal().is_err());
    }

    #[test]
    fn startup_fence_remains_after_no_spawn_cleanup_until_terminal_release() {
        let temp = tempfile::tempdir().unwrap();
        let mut process = LoginProcess::reserve_codex_owned(profile(&temp)).unwrap();
        process.retain_fence_before_launch("run-startup").unwrap();
        assert!(process.matches_startup_run("run-startup"));
        assert!(!process.matches_startup_run("other-run"));
        assert!(process.retain_fence_before_launch("other-run").is_err());
        process.finish().unwrap();
        assert!(process.closure_evidence().unwrap().confirmed());
        assert!(LoginProcess::reserve_codex_owned(profile(&temp)).is_err());
        process.release_after_run_terminal().unwrap();
        assert!(!process.matches_startup_run("run-startup"));
        LoginProcess::reserve_codex_owned(profile(&temp)).unwrap().finish().unwrap();
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
    fn gone_identity_waits_for_reap_without_releasing_fence() {
        struct Gone;
        impl ProcTable for Gone {
            fn list(&self) -> Result<Vec<ProcEntry>, String> {
                Ok(Vec::new())
            }
            fn probe(&self, _: ProcIdentity) -> ProbeResult {
                ProbeResult::Gone
            }
            fn signal(&self, _: u32, _: TreeSignal) -> Result<(), String> {
                panic!("Gone identity must not be signalled");
            }
        }
        for expire in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let mut process = LoginProcess::reserve(&binding).unwrap();
            process
                .launch(|| {
                    Command::new("/bin/sleep")
                        .arg("30")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                })
                .unwrap();
            // Deterministically model the gap between identity disappearance
            // and an owned Child becoming reapable. This is not an OS claim.
            assert!(!process.poll_finish_with(&Gone).unwrap());
            assert!(process.closure_evidence().is_none());
            assert!(LoginProcess::reserve(&binding).is_err());
            if expire {
                process.cleanup_started = Some(Instant::now() - Duration::from_secs(9));
                assert!(process.poll_finish_with(&Gone).is_err());
                assert!(!process.closure_evidence().unwrap().confirmed());
            }
            let child = process.child.as_mut().unwrap();
            child.kill().unwrap();
            child.wait().unwrap();
            if expire {
                assert!(process.finish_with(&Gone).is_err());
                assert!(LoginProcess::reserve(&binding).is_err());
            } else {
                assert!(process.poll_finish_with(&Gone).unwrap());
                assert!(process.closure_evidence().unwrap().confirmed());
                assert!(LoginProcess::reserve(&binding).is_ok());
            }
        }
    }

    #[test]
    #[ignore = "bounded natural-exit stress probe; toy processes only"]
    fn natural_eof_cleanup_repro() {
        for attempt in 0..200 {
            let temp = tempfile::tempdir().unwrap();
            let profile = profile(&temp);
            let mut process = LoginProcess::reserve(&profile).unwrap();
            process.launch(cat).unwrap();
            assert!(process.finish().is_ok(), "EOF cleanup attempt {attempt}");
        }
    }

    #[test]
    #[ignore = "requires local TCP bind; toy process only, no provider/account"]
    fn model_proxy_listener_is_owned_until_confirmed_process_cleanup() {
        use std::io::{Read, Write};
        for uncertain in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let profile = profile(&temp);
            let mut process = LoginProcess::reserve(&profile).unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            // Remain alive independently of stdin EOF so this probe isolates
            // listener ownership from natural-exit timing.
            let mut command = Command::new("/bin/sleep");
            command
                .arg("30")
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            process
                .launch_with_model_proxy(
                    command,
                    crate::provider_model_proxy::broker::Broker::new(listener, Instant::now())
                        .unwrap(),
                    |_| Ok(()),
                )
                .unwrap();
            assert!(std::net::TcpListener::bind(address).is_err());
            assert!(!process.matches_runtime_scope(
                temp.path(),
                crate::provider_runtime_sandbox::WorkspaceAccess::ReadOnly
            ));
            let mut peer = std::net::TcpStream::connect(address).unwrap();
            peer.write_all(b"C").unwrap(); // Incomplete header: must never dial.
            peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            while process.model_proxy.as_ref().unwrap().connection_count() == 0 {
                assert!(Instant::now() < deadline);
                process.tick().unwrap();
                std::thread::sleep(Duration::from_millis(2));
            }
            if uncertain {
                process.inject_uncertain_cleanup_once();
                assert!(process.finish().is_err());
                assert!(std::net::TcpListener::bind(address).is_err());
                assert!(process.model_proxy.is_some());
                assert_eq!(process.model_proxy.as_ref().unwrap().connection_count(), 0);
                // Only this owned toy root exists. Explicitly reap it before
                // taking the test's retained socket; production has no accessor.
                let child = process.child.as_mut().unwrap();
                child.kill().unwrap();
                child.wait().unwrap();
                process.model_proxy.take();
            } else {
                process.finish().unwrap();
                assert!(process.model_proxy.is_none());
                assert!(process.closure_evidence().unwrap().confirmed());
            }
            let mut byte = [0];
            match peer.read(&mut byte) {
                Ok(count) => assert_eq!(count, 0),
                Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset),
            }
            assert!(std::net::TcpListener::bind(address).is_ok());
        }
    }

    #[test]
    #[ignore = "local broker lifecycle only; no upstream or provider"]
    fn expired_broker_fails_transport_without_claiming_process_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let profile = profile(&temp);
        let mut process = LoginProcess::reserve(&profile).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let broker = crate::provider_model_proxy::broker::Broker::new(
            listener,
            Instant::now() - Duration::from_secs(600),
        )
        .unwrap();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        process
            .launch_with_model_proxy(command, broker, |_| Ok(()))
            .unwrap();
        assert!(process.tick().is_err());
        assert!(process.tick().is_err());
        assert!(process.closure_evidence().is_none());
        assert!(LoginProcess::reserve(&profile).is_err());
        process.finish().unwrap();
        assert!(process.closure_evidence().unwrap().confirmed());
        assert!(LoginProcess::reserve(&profile).is_ok());
    }

    #[test]
    #[ignore = "local broker/barrier failure only; no provider/upstream"]
    fn model_proxy_barrier_failure_stops_service_before_cleanup() {
        use std::io::{Read, Write};
        let temp = tempfile::tempdir().unwrap();
        let profile = profile(&temp);
        let mut process = LoginProcess::reserve(&profile).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut broker =
            crate::provider_model_proxy::broker::Broker::new(listener, Instant::now()).unwrap();
        let mut peer = std::net::TcpStream::connect(address).unwrap();
        peer.write_all(b"C").unwrap(); // Pending admission; cannot start an upstream job.
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while broker.connection_count() == 0 {
            assert!(Instant::now() < deadline);
            broker.tick(Instant::now(), false).unwrap();
        }
        let command = crate::provider_login_gate::command(
            std::path::Path::new("/bin/sleep"),
            &[std::ffi::OsStr::new("30")],
            &profile.revalidate().unwrap(),
        )
        .unwrap();
        assert!(process
            .launch_with_model_proxy(command, broker, |child| {
                crate::provider_login_gate::release(child)?;
                Err(std::io::Error::other("uncertain release result"))
            })
            .is_err());
        assert_eq!(process.model_proxy.as_ref().unwrap().connection_count(), 0);
        assert!(process
            .model_proxy
            .as_mut()
            .unwrap()
            .tick(Instant::now(), false)
            .is_err());
        assert!(std::net::TcpListener::bind(address).is_err());
        let mut byte = [0];
        match peer.read(&mut byte) {
            Ok(count) => assert_eq!(count, 0),
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset),
        }
        assert!(process.closure_evidence().is_none());
        assert!(LoginProcess::reserve(&profile).is_err());
        process.finish().unwrap();
        assert!(std::net::TcpListener::bind(address).is_ok());
    }

    #[test]
    #[ignore = "requires local TCP bind; no provider/account"]
    fn uncertain_drop_quarantines_proxy_instead_of_releasing_its_port() {
        use std::os::fd::FromRawFd;
        let temp = tempfile::tempdir().unwrap();
        let profile = profile(&temp);
        let mut process = LoginProcess::reserve(&profile).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let fd = listener.as_raw_fd();
        // No process was started, so the test can safely reclaim its own fd
        // after injecting uncertainty. Production exposes no reclamation API.
        process.model_proxy = Some(
            crate::provider_model_proxy::broker::Broker::new(listener, Instant::now()).unwrap(),
        );
        process.inject_uncertain_cleanup_once();
        drop(process);
        assert!(std::net::TcpListener::bind(address).is_err());
        // SAFETY: Drop deliberately forgot this exact owned listener, and no
        // other object owns/closes the descriptor. No child was ever spawned.
        drop(unsafe { std::net::TcpListener::from_raw_fd(fd) });
        assert!(std::net::TcpListener::bind(address).is_ok());
    }

    #[test]
    fn incremental_cleanup_yields_and_retains_fence_until_confirmed() {
        struct Held(ProcIdentity);
        impl ProcTable for Held {
            fn list(&self) -> Result<Vec<ProcEntry>, String> {
                Ok(Vec::new())
            }
            fn probe(&self, identity: ProcIdentity) -> ProbeResult {
                assert_eq!(identity, self.0);
                ProbeResult::Alive(ProcEntry {
                    identity,
                    ppid: 1,
                    comm: "toy".into(),
                })
            }
            fn signal(&self, _: u32, _: TreeSignal) -> Result<(), String> {
                Ok(())
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process
            .launch(|| {
                Command::new("/bin/sleep")
                    .arg("30")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
            })
            .unwrap();
        process.retain_fence_until_terminal().unwrap();
        let root = process.root_for_test();
        for _ in 0..32 {
            assert!(!process.poll_finish_with(&Held(root)).unwrap());
            assert!(process.closure_evidence().is_none());
            assert!(process.release_after_run_terminal().is_err());
            assert!(LoginProcess::reserve(&binding).is_err());
            assert!(process.tick().is_err());
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while !process.poll_finish().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(process.closure_evidence().unwrap().confirmed());
        assert!(!std::fs::read(temp.path().join(PENDING))
            .unwrap()
            .ends_with(LOGIN_TERMINATED));
        assert!(
            LoginProcess::reserve(&binding).is_err(),
            "terminal transaction still owns fence"
        );
        process.release_after_run_terminal().unwrap();
        assert!(LoginProcess::reserve(&binding).is_ok());
        assert!(process.poll_finish().is_err());
    }

    #[test]
    fn owned_claude_profile_survives_request_and_rejects_directory_replacement() {
        use crate::provider_claude_profile::ClaudeProfileBinding;
        fn binding(temp: &tempfile::TempDir) -> ClaudeProfileBinding {
            let storage = crate::Storage::open(&temp.path().join("hello-agent.sqlite3")).unwrap();
            ClaudeProfileBinding::from_storage(&storage).unwrap()
        }
        for replace in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            // Neither the request-local Storage nor a borrowed binding is retained.
            let mut process = LoginProcess::reserve_claude_owned(binding(&temp)).unwrap();
            let home = process.claude_preflight_home().unwrap();
            assert!(process.codex_preflight_home().is_err());
            process.launch(cat).unwrap();
            process.retain_fence_until_terminal().unwrap();
            assert!(LoginProcess::reserve_claude_owned(binding(&temp)).is_err());
            if replace {
                std::fs::rename(&home, home.with_file_name("retained-claude-profile")).unwrap();
                let _replacement = binding(&temp);
                assert!(process.claude_preflight_home().is_err());
            }
            let cleanup = process.finish();
            if !replace {
                assert!(cleanup.is_ok());
                assert!(LoginProcess::reserve_claude_owned(binding(&temp)).is_err());
                process.release_after_run_terminal().unwrap();
                assert!(LoginProcess::reserve_claude_owned(binding(&temp)).is_ok());
            } else {
                assert!(cleanup.is_err());
                assert!(process.release_after_run_terminal().is_err());
                assert!(LoginProcess::reserve_claude_owned(binding(&temp)).is_err());
            }
        }
    }

    #[test]
    fn owned_profile_outlives_request_and_keeps_cleanup_and_identity_fences() {
        fn open_request(temp: &tempfile::TempDir) -> LoginProcess<'static> {
            // Storage and the local binding do not outlive this request.
            LoginProcess::reserve_codex_owned(profile(temp)).unwrap()
        }
        for case in ["finish", "drop", "uncertain", "replaced"] {
            let temp = tempfile::tempdir().unwrap();
            let mut process = open_request(&temp);
            let home = process.codex_preflight_home().unwrap();
            process.launch(cat).unwrap();
            let identity = process.root_for_test();
            // Another dispatcher request cannot reserve the same profile.
            let independent = profile(&temp);
            assert!(LoginProcess::reserve(&independent).is_err());
            if case == "replaced" {
                std::fs::rename(&home, home.with_file_name("retained-profile")).unwrap();
                // Same pathname now refers to a fresh, otherwise valid profile.
                let replacement = profile(&temp);
                assert!(replacement.revalidate().is_ok());
                assert!(process.codex_preflight_home().is_err());
            }
            if case == "uncertain" {
                process.inject_uncertain_cleanup_once();
            }
            if case != "drop" {
                assert_eq!(process.finish().is_ok(), case == "finish", "{case}");
            }
            drop(process);
            assert!(matches!(
                SystemProcTable.probe(identity),
                hello_agent_core::proctree::ProbeResult::Gone
            ));
            assert_eq!(
                temp.path().join(PENDING).exists(),
                case != "finish",
                "{case}"
            );
            assert_eq!(
                LoginProcess::reserve(&profile(&temp)).is_ok(),
                case == "finish",
                "{case}"
            );
        }
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
    fn explicit_recovery_requires_closed_owner_and_valid_termination_record() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch(cat).unwrap();
        let home = binding.revalidate().unwrap();
        // Inject an actual policy violation, not a permitted future cache name.
        let obstacle = home.join("AGENTS.md");
        std::fs::write(&obstacle, b"preserve").unwrap();
        assert!(process.finish().is_err());
        std::fs::remove_file(obstacle).unwrap();
        assert!(
            recover_confirmed_login(&binding).is_err(),
            "live owner still holds lock"
        );
        drop(process);
        recover_confirmed_login(&binding).unwrap();
        assert!(!temp.path().join(PENDING).exists());
        LoginProcess::reserve(&binding).unwrap().finish().unwrap();
    }

    #[test]
    fn explicit_recovery_rejects_legacy_malformed_and_unsafe_files() {
        use std::os::unix::fs::PermissionsExt;
        for case in [
            "legacy",
            "v1-marker",
            "partial",
            "malformed",
            "directory",
            "symlink",
            "hardlink",
            "writable",
            "oversized",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let path = temp.path().join(PENDING);
            let valid = format!(
                "hello-agent provider login pending v1\n{}",
                std::str::from_utf8(LOGIN_TERMINATED).unwrap()
            );
            let source = temp.path().join("source");
            std::fs::write(&source, valid.as_bytes()).unwrap();
            std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600)).unwrap();
            match case {
                "directory" => std::fs::create_dir(&path).unwrap(),
                "symlink" => std::os::unix::fs::symlink(&source, &path).unwrap(),
                "hardlink" => std::fs::hard_link(&source, &path).unwrap(),
                _ => {
                    let bytes = match case {
                        "legacy" => "hello-agent provider login pending v1\n42 123\n".to_owned(),
                        "v1-marker" => {
                            "hello-agent provider login pending v1\nlogin-processes-terminated-v1\n"
                                .to_owned()
                        }
                        "partial" => valid.trim_end().to_owned(),
                        "malformed" => valid.replace("v1\n", "v1\n00 1\n"),
                        "oversized" => "x".repeat(16385),
                        _ => valid.clone(),
                    };
                    std::fs::write(&path, bytes).unwrap();
                    std::fs::set_permissions(
                        &path,
                        std::fs::Permissions::from_mode(if case == "writable" {
                            0o666
                        } else {
                            0o600
                        }),
                    )
                    .unwrap();
                }
            }
            assert!(recover_confirmed_login(&binding).is_err(), "{case}");
            assert!(path.symlink_metadata().is_ok(), "{case}");
            assert_eq!(std::fs::read(&source).unwrap(), valid.as_bytes());
        }
    }

    #[test]
    fn failed_termination_record_preserves_the_fence() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch(cat).unwrap();
        // Keep the exact inode but inject a write failure at the final marker.
        process.reservation.file = process.reservation.directory.open(PENDING).unwrap();
        assert!(process.finish().is_err());
        assert!(process.reservation.failed);
        assert!(!process.closure_evidence().unwrap().confirmed());
        assert!(!std::fs::read(temp.path().join(PENDING))
            .unwrap()
            .ends_with(LOGIN_TERMINATED));
        assert!(LoginProcess::reserve(&binding).is_err());
    }

    #[test]
    fn termination_evidence_does_not_clear_a_profile_validation_failure() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let mut process = LoginProcess::reserve(&binding).unwrap();
        process.launch(cat).unwrap();
        let home = binding.revalidate().unwrap();
        std::fs::write(home.join("AGENTS.md"), b"preserve").unwrap();
        assert!(process.finish().is_err());
        let receipt = process.closure_evidence().unwrap();
        assert!(receipt.termination_confirmed());
        assert!(!receipt.confirmed());
        assert!(temp.path().join(PENDING).exists());
        assert!(std::fs::read(temp.path().join(PENDING))
            .unwrap()
            .ends_with(LOGIN_TERMINATED));
        // Repairing a profile is not retroactive cleanup authority.
        std::fs::remove_file(home.join("AGENTS.md")).unwrap();
        assert!(process.finish().is_err());
        assert!(LoginProcess::reserve(&binding).is_err());
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
        assert!(!process.closure_evidence().unwrap().termination_confirmed());
        assert!(!std::fs::read(temp.path().join(PENDING))
            .unwrap()
            .ends_with(LOGIN_TERMINATED));
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
