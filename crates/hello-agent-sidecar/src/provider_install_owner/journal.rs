//! Bounded append-only installation transaction evidence. Reopening is inspection,
//! not authority to resume an installer, signal a PID, or release its process fence.
use super::{failure, InstallScope};
use crate::{provider_profile::LoginProfile, SidecarError};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{File, MetadataExt, OpenOptions, OpenOptionsExt};
use hello_agent_core::install_lifecycle::{InstallLifecycle, InstallPhase};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;

const NAME: &str = "provider-install-journal-v1.jsonl";
const MAX_BYTES: u64 = 16 * 1024;
const MAX_EVENTS: u64 = 16;
const ARCHIVE_PREFIX: &str = "provider-install-complete-";
const MAX_ARCHIVES: usize = 128;

/// Any active name blocks new work, including malformed files and links.
/// Inspection never resumes a process, signals a PID or removes evidence.
pub(super) fn has_active(storage: &crate::Storage) -> Result<bool, SidecarError> {
    let directory = crate::provider_profile::open_absolute_directory(
        storage.path().parent().ok_or_else(failure)?,
    )?;
    for name in [NAME, super::PENDING, super::EXECUTION_PENDING] {
        match directory.symlink_metadata(name) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure()),
        }
    }
    Ok(false)
}

pub(crate) struct InstallJournal<'a> {
    pub(super) scope: &'a InstallScope,
    pub(super) operation: uuid::Uuid,
    pub(super) provider: String,
    file: File,
    state: InstallLifecycle,
    sequence: u64,
    length: u64,
    failed: bool,
    contents: Vec<u8>,
}

impl<'a> InstallJournal<'a> {
    pub(crate) fn begin(
        scope: &'a InstallScope,
        operation: uuid::Uuid,
        provider: &str,
    ) -> Result<Self, SidecarError> {
        if scope.execution || operation.is_nil() || !matches!(provider, "codex" | "claude") {
            return Err(failure());
        }
        let directory = scope.login_lease_directory()?;
        require_no_process_fences(&directory)?;
        archive_budget(&directory)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .follow(FollowSymlinks::No);
        let mut file = directory.open_with(NAME, &options).map_err(|_| failure())?;
        lock(&file, libc::LOCK_EX)?;
        let header = format!(
            "{}\n",
            json!({"version":1,"operation":operation.to_string(),"provider":provider})
        );
        file.write_all(header.as_bytes()).map_err(|_| failure())?;
        file.sync_all().map_err(|_| failure())?;
        directory
            .into_std_file()
            .sync_all()
            .map_err(|_| failure())?;
        let journal = Self {
            scope,
            operation,
            provider: provider.to_owned(),
            file,
            state: InstallLifecycle::default(),
            sequence: 0,
            length: header.len() as u64,
            failed: false,
            contents: header.into_bytes(),
        };
        journal.verify()?;
        Ok(journal)
    }

    pub(crate) fn phase(&self) -> InstallPhase {
        if self.failed {
            InstallPhase::RecoveryRequired
        } else {
            self.state.phase()
        }
    }

    /// Explicit recovery entry point. Reopening never invents completion or
    /// releases process fences: only an already-terminal journal can archive.
    pub(crate) fn recover_terminal(
        scope: &'a InstallScope,
        expected: uuid::Uuid,
    ) -> Result<(), SidecarError> {
        let journal = reopen(scope, true)?;
        if journal.operation != expected {
            return Err(failure());
        }
        journal.archive_terminal()
    }

    /// Events are trusted worker observations, never renderer input. The process
    /// owner remains responsible for concrete closure/verification evidence.
    pub(crate) fn record(&mut self, event: &str) -> Result<(), SidecarError> {
        if self.failed {
            return Err(failure());
        }
        let mut next = self.state.clone();
        apply(&mut next, event)?;
        if next == self.state {
            // A repeated cancellation or recovery observation adds no evidence.
            return Ok(());
        }
        let bytes = format!("{}\n", json!({"seq":self.sequence + 1,"event":event}));
        let result = (|| {
            self.verify()?;
            if self.sequence >= MAX_EVENTS || self.length + bytes.len() as u64 > MAX_BYTES {
                return Err(failure());
            }
            self.file
                .write_all(bytes.as_bytes())
                .map_err(|_| failure())?;
            self.file.sync_all().map_err(|_| failure())?;
            self.length += bytes.len() as u64;
            self.verify()
        })();
        if result.is_err() {
            self.failed = true;
            return result;
        }
        // Publish state only after durable append. A failed append is sticky.
        self.sequence += 1;
        self.state = next;
        self.contents.extend_from_slice(bytes.as_bytes());
        Ok(())
    }

    /// Archive only a terminal transaction whose process reservations have both
    /// been released. Preserve a durable copy before removing the active name.
    /// Any partial archive remains evidence; never overwrite an older file.
    pub(crate) fn archive_terminal(self) -> Result<(), SidecarError> {
        if self.failed
            || !matches!(
                self.phase(),
                InstallPhase::Installed | InstallPhase::Canceled | InstallPhase::Failed
            )
        {
            return Err(failure());
        }
        self.verify()?;
        let directory = self.scope.login_lease_directory()?;
        require_no_process_fences(&directory)?;
        let mut read_options = OpenOptions::new();
        read_options
            .read(true)
            .follow(FollowSymlinks::No)
            .custom_flags(libc::O_NONBLOCK);
        let mut current = directory
            .open_with(NAME, &read_options)
            .map_err(|_| failure())?;
        let current_meta = current.metadata().map_err(|_| failure())?;
        let held_meta = self.file.metadata().map_err(|_| failure())?;
        if !valid_metadata(&current_meta)
            || current_meta.ino() != held_meta.ino()
            || current_meta.dev() != held_meta.dev()
        {
            return Err(failure());
        }
        let mut current_bytes = Vec::new();
        (&mut current)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut current_bytes)
            .map_err(|_| failure())?;
        if current_bytes != self.contents {
            return Err(failure());
        }
        self.verify()?;
        let name = format!("{ARCHIVE_PREFIX}{}.jsonl", self.operation);
        // A crash may leave the fully written archive and the active journal.
        // Accept only an exact existing copy; never overwrite partial evidence.
        let archive = match directory.open_with(&name, &read_options) {
            Ok(mut existing) => {
                lock(&existing, libc::LOCK_EX)?;
                if !valid_metadata(&existing.metadata().map_err(|_| failure())?) {
                    return Err(failure());
                }
                let mut bytes = Vec::new();
                (&mut existing)
                    .take(MAX_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| failure())?;
                if bytes != self.contents {
                    return Err(failure());
                }
                existing
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                archive_budget(&directory)?;
                let mut options = OpenOptions::new();
                options
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .follow(FollowSymlinks::No);
                let mut created = directory
                    .open_with(&name, &options)
                    .map_err(|_| failure())?;
                lock(&created, libc::LOCK_EX)?;
                created.write_all(&self.contents).map_err(|_| failure())?;
                created
            }
            Err(_) => return Err(failure()),
        };
        archive.sync_all().map_err(|_| failure())?;
        let held = archive.metadata().map_err(|_| failure())?;
        let named = directory.symlink_metadata(&name).map_err(|_| failure())?;
        if !valid_metadata(&named)
            || named.len() != self.length
            || named.ino() != held.ino()
            || named.dev() != held.dev()
        {
            return Err(failure());
        }
        directory
            .try_clone()
            .map_err(|_| failure())?
            .into_std_file()
            .sync_all()
            .map_err(|_| failure())?;
        self.verify()?;
        require_no_process_fences(&directory)?;
        directory.remove_file(NAME).map_err(|_| failure())?;
        directory.into_std_file().sync_all().map_err(|_| failure())
    }

    pub(super) fn verify(&self) -> Result<(), SidecarError> {
        let directory = self.scope.login_lease_directory()?;
        let current = directory.symlink_metadata(NAME).map_err(|_| failure())?;
        let held = self.file.metadata().map_err(|_| failure())?;
        if !valid_metadata(&current)
            || current.dev() != held.dev()
            || current.ino() != held.ino()
            || current.len() != self.length
        {
            return Err(failure());
        }
        Ok(())
    }
}

fn require_no_process_fences(directory: &cap_std::fs::Dir) -> Result<(), SidecarError> {
    for name in [super::PENDING, super::EXECUTION_PENDING] {
        match directory.symlink_metadata(name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(failure()),
        }
    }
    Ok(())
}

fn archive_budget(directory: &cap_std::fs::Dir) -> Result<(), SidecarError> {
    let mut archives = 0;
    for (index, entry) in directory.entries().map_err(|_| failure())?.enumerate() {
        if index >= 4096 {
            return Err(failure());
        }
        let name = entry.map_err(|_| failure())?.file_name();
        if name.to_string_lossy().starts_with(ARCHIVE_PREFIX) {
            archives += 1;
            if archives >= MAX_ARCHIVES {
                return Err(failure());
            }
        }
    }
    Ok(())
}

/// Includes incomplete records as their last durable phase. Callers must still
/// treat any existing transaction as blocked until explicit recovery validates it.
pub(crate) fn inspect(
    scope: &InstallScope,
) -> Result<(uuid::Uuid, String, InstallPhase), SidecarError> {
    let journal = reopen(scope, false)?;
    Ok((journal.operation, journal.provider.clone(), journal.phase()))
}

fn reopen(scope: &InstallScope, exclusive: bool) -> Result<InstallJournal<'_>, SidecarError> {
    let directory = scope.login_lease_directory()?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let mut file = directory.open_with(NAME, &options).map_err(|_| failure())?;
    lock(
        &file,
        if exclusive {
            libc::LOCK_EX
        } else {
            libc::LOCK_SH
        },
    )?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !valid_metadata(&meta) {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 != meta.len() || !bytes.ends_with(b"\n") || bytes.contains(&b'\r') {
        return Err(failure());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| failure())?;
    let mut lines = text.lines();
    let header = parse_record(lines.next().ok_or_else(failure)?)?;
    if header.as_object().map(|o| o.len()) != Some(3) || header["version"] != 1 {
        return Err(failure());
    }
    let operation_text = header["operation"].as_str().ok_or_else(failure)?;
    let operation = uuid::Uuid::parse_str(operation_text).map_err(|_| failure())?;
    let provider = header["provider"].as_str().ok_or_else(failure)?;
    if operation.is_nil()
        || operation.to_string() != operation_text
        || !matches!(provider, "codex" | "claude")
    {
        return Err(failure());
    }
    let mut state = InstallLifecycle::default();
    let mut sequence = 0;
    for (index, line) in lines.enumerate() {
        let event = parse_record(line)?;
        if index as u64 >= MAX_EVENTS
            || event.as_object().map(|o| o.len()) != Some(2)
            || event["seq"].as_u64() != Some(index as u64 + 1)
        {
            return Err(failure());
        }
        apply(&mut state, event["event"].as_str().ok_or_else(failure)?)?;
        sequence += 1;
    }
    let current = directory.symlink_metadata(NAME).map_err(|_| failure())?;
    if !valid_metadata(&current)
        || current.dev() != meta.dev()
        || current.ino() != meta.ino()
        || current.len() != meta.len()
    {
        return Err(failure());
    }
    scope.revalidate()?;
    let journal = InstallJournal {
        scope,
        operation,
        provider: provider.to_owned(),
        file,
        state,
        sequence,
        length: bytes.len() as u64,
        failed: false,
        contents: bytes,
    };
    journal.verify()?;
    Ok(journal)
}

fn valid_metadata(meta: &cap_std::fs::Metadata) -> bool {
    meta.is_file()
        && meta.nlink() == 1
        && meta.uid() == unsafe { libc::geteuid() }
        && meta.mode() & 0o777 == 0o600
        && meta.len() <= MAX_BYTES
}

fn parse_record(line: &str) -> Result<Value, SidecarError> {
    let value: Value = serde_json::from_str(line).map_err(|_| failure())?;
    // We own the writer. Canonical round-trip rejects duplicate keys and any
    // alternate encoding instead of silently normalizing recovery evidence.
    if value.to_string() != line {
        return Err(failure());
    }
    Ok(value)
}

fn lock(file: &File, operation: i32) -> Result<(), SidecarError> {
    if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } != 0 {
        Err(failure())
    } else {
        Ok(())
    }
}

fn apply(state: &mut InstallLifecycle, event: &str) -> Result<(), SidecarError> {
    let result = match event {
        "consented" => state.accept_consent(),
        "downloadVerified" => state.downloaded(true),
        "downloadFailed" => state.downloaded(false),
        "installerExited" => state.installer_exited(true),
        "installerFailed" => state.installer_exited(false),
        "verified" => state.verified(true),
        "verificationFailed" => state.verified(false),
        "cancelRequested" => state.cancel(),
        "cleanupConfirmed" => state.cleanup_finished(true),
        "cleanupUnconfirmed" => state.cleanup_finished(false),
        "recoveryRequired" => state.require_recovery(),
        _ => return Err(failure()),
    };
    result.map_err(|_| failure())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_terminal_recovery_accepts_only_exact_operation_and_matching_archive() {
        use std::os::unix::fs::PermissionsExt;
        for archive_exists in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            let scope = InstallScope::from_storage(&storage).unwrap();
            let operation = uuid::Uuid::new_v4();
            let mut journal = InstallJournal::begin(&scope, operation, "codex").unwrap();
            journal.record("consented").unwrap();
            journal.record("cancelRequested").unwrap();
            journal.record("cleanupConfirmed").unwrap();
            assert!(InstallJournal::recover_terminal(&scope, operation).is_err());
            drop(journal);
            let bytes = std::fs::read(root.join(NAME)).unwrap();
            let archive = root.join(format!("{ARCHIVE_PREFIX}{operation}.jsonl"));
            if archive_exists {
                std::fs::write(&archive, &bytes).unwrap();
                std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            assert!(InstallJournal::recover_terminal(&scope, uuid::Uuid::new_v4()).is_err());
            assert_eq!(std::fs::read(root.join(NAME)).unwrap(), bytes);
            InstallJournal::recover_terminal(&scope, operation).unwrap();
            assert!(!has_active(&storage).unwrap());
            assert_eq!(std::fs::read(&archive).unwrap(), bytes);
        }
    }

    #[test]
    fn explicit_recovery_preserves_interrupted_or_fenced_transactions() {
        for terminal in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            let scope = InstallScope::from_storage(&storage).unwrap();
            let operation = uuid::Uuid::new_v4();
            let mut journal = InstallJournal::begin(&scope, operation, "claude").unwrap();
            journal.record("consented").unwrap();
            if terminal {
                journal.record("cancelRequested").unwrap();
                journal.record("cleanupConfirmed").unwrap();
                let owner = scope.reserve(operation).unwrap();
                drop(owner);
            }
            drop(journal);
            let bytes = std::fs::read(root.join(NAME)).unwrap();
            assert!(InstallJournal::recover_terminal(&scope, operation).is_err());
            assert_eq!(std::fs::read(root.join(NAME)).unwrap(), bytes);
            assert!(has_active(&storage).unwrap());
        }
    }

    #[test]
    fn archive_preserves_active_evidence_when_a_fence_or_destination_exists() {
        for pending in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            let scope = InstallScope::from_storage(&storage).unwrap();
            let operation = uuid::Uuid::new_v4();
            let mut journal = InstallJournal::begin(&scope, operation, "claude").unwrap();
            journal.record("consented").unwrap();
            journal.record("cancelRequested").unwrap();
            journal.record("cleanupConfirmed").unwrap();
            let old = std::fs::read(root.join(NAME)).unwrap();
            let archive_name = format!("{ARCHIVE_PREFIX}{operation}.jsonl");
            let owner = if pending {
                Some(scope.reserve(operation).unwrap())
            } else {
                std::fs::write(root.join(&archive_name), b"preserve prior evidence").unwrap();
                None
            };
            assert!(journal.archive_terminal().is_err());
            assert_eq!(std::fs::read(root.join(NAME)).unwrap(), old);
            if !pending {
                assert_eq!(
                    std::fs::read(root.join(archive_name)).unwrap(),
                    b"preserve prior evidence"
                );
            }
            drop(owner);
        }
    }

    #[test]
    fn archive_does_not_replace_same_length_changed_evidence_with_memory_copy() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let scope = InstallScope::from_storage(&storage).unwrap();
        let mut journal = InstallJournal::begin(&scope, uuid::Uuid::new_v4(), "claude").unwrap();
        journal.record("cancelRequested").unwrap();
        let mut altered = std::fs::read(root.join(NAME)).unwrap();
        let offset = altered.windows(6).position(|w| w == b"claude").unwrap();
        altered[offset] = b'C';
        std::fs::write(root.join(NAME), &altered).unwrap();
        assert!(journal.archive_terminal().is_err());
        assert_eq!(std::fs::read(root.join(NAME)).unwrap(), altered);
    }

    #[test]
    fn replay_preserves_cancellation_during_final_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let scope = InstallScope::from_storage(&storage).unwrap();
        let id = uuid::Uuid::new_v4();
        let mut journal = InstallJournal::begin(&scope, id, "codex").unwrap();
        assert!(InstallJournal::begin(&scope, id, "codex").is_err());
        assert!(inspect(&scope).is_err()); // live writer owns the lock
        for event in [
            "consented",
            "downloadVerified",
            "installerExited",
            "verified",
            "cancelRequested",
            "cancelRequested",
            "cleanupConfirmed",
        ] {
            journal.record(event).unwrap();
        }
        assert_eq!(journal.phase(), InstallPhase::Canceled);
        drop(journal);
        assert_eq!(
            inspect(&scope).unwrap(),
            (id, "codex".into(), InstallPhase::Canceled)
        );
        assert!(root.join(NAME).is_file()); // inspection never deletes evidence
        assert!(InstallJournal::begin(&scope, uuid::Uuid::new_v4(), "claude").is_err());
    }

    #[test]
    fn incomplete_or_replaced_evidence_cannot_claim_success() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let scope = InstallScope::from_storage(&storage).unwrap();
        let mut journal = InstallJournal::begin(&scope, uuid::Uuid::new_v4(), "claude").unwrap();
        journal.record("consented").unwrap();
        assert!(journal.record("cleanupConfirmed").is_err());
        std::fs::rename(root.join(NAME), root.join("retained-journal")).unwrap();
        std::fs::write(root.join(NAME), b"incomplete").unwrap();
        assert!(journal.record("downloadVerified").is_err());
        assert_eq!(journal.phase(), InstallPhase::RecoveryRequired);
        drop(journal);
        assert!(inspect(&scope).is_err());
        assert_eq!(std::fs::read(root.join(NAME)).unwrap(), b"incomplete");
        assert!(parse_record(r#"{"seq":1,"seq":2}"#).is_err());
    }
}
