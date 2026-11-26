//! Durable identity only; this receipt does not authorize releasing a fence.
use super::*;
use serde_json::{json, Value};

pub(crate) use crate::provider_run_binding::RunFenceBinding;

/// Opens a hold bound to a run and reads what it recorded. Taking its lock
/// proves no live owner holds it; the lock lasts as long as the reservation.
fn read_bound(
    profile: &dyn LoginProfile,
    name: &str,
) -> Result<(RunFenceBinding, Reservation, std::path::PathBuf), SidecarError> {
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
        .open_with(name, &options)
        .map_err(|_| failure())?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_file() || meta.len() > super::HOLD_MAX_BYTES {
        return Err(failure());
    }
    lock_exclusive(&file)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(super::HOLD_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 != meta.len() || !bytes.ends_with(b"\n") || bytes.contains(&b'\r') {
        return Err(failure());
    }
    let mut lines = std::str::from_utf8(&bytes).map_err(|_| failure())?.lines();
    if lines.next() != Some("elsecrew provider login pending v1") {
        return Err(failure());
    }
    let encoded = lines
        .next()
        .and_then(|line| line.strip_prefix("run-binding-v1 "))
        .ok_or_else(failure)?;
    let value: Value = serde_json::from_str(encoded).map_err(|_| failure())?;
    let binding = RunFenceBinding::from_value(&value)?;
    if serde_json::to_string(&binding.to_value()).map_err(|_| failure())? != encoded {
        return Err(failure());
    }
    let runtime = match profile.pending_name() {
        "codex-login.pending" => "codex",
        "claude-login.pending" => "claude-code",
        _ => return Err(failure()),
    };
    binding.validate(binding.run_id(), runtime)?;
    // The receipt names the very file it was read from.
    if binding.lease_name() != name {
        return Err(failure());
    }
    let dir_meta = directory.dir_metadata().map_err(|_| failure())?;
    if !binding.matches_file(meta.dev(), meta.ino(), dir_meta.dev(), dir_meta.ino()) {
        return Err(failure());
    }
    let (recorded, sessions) = super::parse_hold_lines(lines)?;
    let reservation = Reservation {
        name: name.to_owned(),
        directory,
        file,
        recorded,
        sessions,
        failed: false,
    };
    reservation.verify()?;
    Ok((binding, reservation, home))
}

/// The process identities this exact run's hold recorded, for the owner that
/// settles the run after its engine is gone: the engine's own last cleanup
/// can add identities here that never reached the run ledger. Not permission
/// to release anything; an error only means there is no usable record.
pub(crate) fn bound_run_identities(
    profile: &dyn LoginProfile,
    expected: &RunFenceBinding,
) -> Result<BTreeSet<ProcIdentity>, SidecarError> {
    let (binding, reservation, _) = read_bound(profile, &expected.lease_name())?;
    if &binding != expected {
        return Err(failure());
    }
    Ok(reservation.recorded.clone())
}

/// Recovery by proof only, whether the user asks for it or the app retries it
/// (at start, after a run was settled). The verifier must prove the exact
/// durable run is terminal before unlink; disappearance of processes alone is
/// insufficient.
pub(crate) fn recover_bound_run(
    profile: &dyn LoginProfile,
    name: &str,
    verify_terminal: impl FnOnce(&RunFenceBinding, &BTreeSet<ProcIdentity>) -> Result<(), SidecarError>,
) -> Result<(), SidecarError> {
    let (binding, reservation, home) = read_bound(profile, name)?;
    // Nothing recorded: the engine died before it launched anything. The
    // provider only ever starts once its root is recorded here (the gate
    // waits for the release line, written after the record, and ends on its
    // own when the engine's pipe closes), and the lock is ours, so no owner
    // lives: nothing to settle (PR #20 review B2).
    if !reservation.recorded.is_empty() {
        verify_terminal(&binding, &reservation.recorded)?;
    }
    // The hold's lock is ours, so its owner is gone: a reused process ID
    // proves the recorded process ended.
    for identity in &reservation.recorded {
        if !matches!(
            elsecrew_core::proctree::SettledProcTable.probe(*identity),
            elsecrew_core::proctree::ProbeResult::Gone
        ) {
            return Err(failure());
        }
    }
    // The run's own session, where registered: every process in it ended,
    // seen or not (PR #20 re-review B2).
    if !reservation.sessions.is_empty() && !super::sessions_ended(&reservation.sessions, &reservation.recorded) {
        return Err(failure());
    }
    if profile.revalidate()? != home
        || !crate::provider_profile::same_directory(
            &reservation.directory,
            &profile.login_lease_directory()?,
        )?
    {
        return Err(failure());
    }
    reservation.release()
}

/// A run hold bound to no run: a model list, a capability or sign-in check,
/// or a run whose engine died before it wrote its binding (PR #20 review B2).
/// The proof from the moment it was published: the provider only ever
/// starts once its root process is recorded in the hold (the gate releases
/// it after the record; a gate whose engine died ends on its pipe's close),
/// so with the hold's lock ours (no owner lives) and every recorded process
/// gone, nothing of it runs. A termination marker may end it. Anything
/// malformed stays fenced. `Ok(false)`: the hold is bound to a run, and only
/// `recover_bound_run` may settle it.
pub(crate) fn recover_unbound_run(profile: &dyn LoginProfile, name: &str) -> Result<bool, SidecarError> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
    use std::io::Read;
    let home = profile.revalidate()?;
    let directory = profile.login_lease_directory()?;
    let prefix = super::hold_prefix(profile).ok_or_else(failure)?;
    if !crate::provider_run_binding::is_run_lease_name(name, prefix) {
        return Err(failure());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).custom_flags(libc::O_NONBLOCK);
    let mut file = directory.open_with(name, &options).map_err(|_| failure())?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_file() || meta.len() > super::HOLD_MAX_BYTES {
        return Err(failure());
    }
    lock_exclusive(&file)?;
    let mut bytes = Vec::new();
    (&mut file).take(super::HOLD_MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| failure())?;
    if bytes.len() as u64 != meta.len() || bytes.contains(&b'\r') {
        return Err(failure());
    }
    let marked = bytes.ends_with(LOGIN_TERMINATED);
    let body = bytes.strip_suffix(LOGIN_TERMINATED).unwrap_or(&bytes);
    let text = std::str::from_utf8(body).map_err(|_| failure())?;
    if !text.ends_with('\n') {
        return Err(failure());
    }
    let mut lines = text.lines();
    if lines.next() != Some("elsecrew provider login pending v1") {
        return Err(failure());
    }
    let lines: Vec<&str> = lines.collect();
    if lines.first().is_some_and(|line| line.starts_with("run-binding-v1 ")) {
        return Ok(false);
    }
    let (recorded, sessions) = super::parse_hold_lines(lines.into_iter())?;
    // Something ran: only the whole tree's end lets the hold go. The
    // provider's session was registered before it ran; without it, or the
    // owner's termination marker, nothing proves that (fail closed; PR #20
    // re-review B2).
    if !recorded.is_empty() && sessions.is_empty() && !marked {
        return Err(failure());
    }
    let reservation = Reservation { name: name.to_owned(), directory, file, recorded, sessions, failed: false };
    reservation.verify()?;
    // The lock is ours, so the owner is gone: a reused process ID proves the
    // recorded process ended.
    for identity in &reservation.recorded {
        if !matches!(elsecrew_core::proctree::SettledProcTable.probe(*identity), elsecrew_core::proctree::ProbeResult::Gone) {
            return Err(failure());
        }
    }
    // Every process of the provider's session ended, the ones the engine
    // never saw included.
    if !reservation.sessions.is_empty() && !super::sessions_ended(&reservation.sessions, &reservation.recorded) {
        return Err(failure());
    }
    if profile.revalidate()? != home
        || !crate::provider_profile::same_directory(&reservation.directory, &profile.login_lease_directory()?)?
    {
        return Err(failure());
    }
    reservation.release()?;
    Ok(true)
}

impl LoginProcess<'_> {
    /// Called once before launch, after run retention. The caller must commit
    /// the returned receipt before spawning. A failed write stays fenced.
    pub(crate) fn bind_run_fence(&mut self, run_id: &str) -> Result<RunFenceBinding, SidecarError> {
        if !self.matches_startup_run(run_id) || self.attempted || self.finished || self.failed {
            return Err(failure());
        }
        let result = (|| {
            self.profile.as_ref().revalidate()?;
            self.reservation.verify()?;
            let file = self.reservation.file.metadata().map_err(|_| failure())?;
            let directory = self
                .reservation
                .directory
                .dir_metadata()
                .map_err(|_| failure())?;
            // No earlier process identity or run binding may precede this line.
            if file.len() != b"elsecrew provider login pending v1\n".len() as u64 {
                return Err(failure());
            }
            let provider = match self.profile.as_ref().pending_name() {
                "codex-login.pending" => "codex",
                "claude-login.pending" => "claude-code",
                _ => return Err(failure()),
            };
            // A run's own hold (schema 2); the provider's shared one is schema 1.
            let mut value = json!({
                "schemaVersion": 1, "runId": run_id,
                "nonce": self.connection_id.to_string(), "provider": provider,
                "device": file.dev(), "inode": file.ino(),
                "directoryDevice": directory.dev(), "directoryInode": directory.ino(),
            });
            if self.reservation.name != self.profile.as_ref().pending_name() {
                value["schemaVersion"] = json!(2);
                value["lease"] = json!(self.reservation.name);
            }
            let receipt = RunFenceBinding::from_value(&value)?;
            receipt.validate(run_id, provider)?;
            let encoded = serde_json::to_string(&receipt.to_value()).map_err(|_| failure())?;
            writeln!(self.reservation.file, "run-binding-v1 {encoded}").map_err(|_| failure())?;
            self.reservation.file.sync_all().map_err(|_| failure())?;
            self.reservation.verify()?;
            Ok(receipt)
        })();
        if result.is_err() {
            self.failed = true;
            self.reservation.failed = true;
        }
        result
    }
}
