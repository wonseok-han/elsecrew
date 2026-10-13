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

pub(crate) struct InstallJournal<'a> {
    scope: &'a InstallScope,
    file: File,
    state: InstallLifecycle,
    sequence: u64,
    length: u64,
    failed: bool,
}

impl<'a> InstallJournal<'a> {
    pub(crate) fn begin(
        scope: &'a InstallScope,
        operation: uuid::Uuid,
        provider: &str,
    ) -> Result<Self, SidecarError> {
        if operation.is_nil() || !matches!(provider, "codex" | "claude") {
            return Err(failure());
        }
        let directory = scope.login_lease_directory()?;
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
            file,
            state: InstallLifecycle::default(),
            sequence: 0,
            length: header.len() as u64,
            failed: false,
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
        Ok(())
    }

    fn verify(&self) -> Result<(), SidecarError> {
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

/// Includes incomplete records as their last durable phase. Callers must still
/// treat any existing transaction as blocked until explicit recovery validates it.
pub(crate) fn inspect(
    scope: &InstallScope,
) -> Result<(uuid::Uuid, String, InstallPhase), SidecarError> {
    let directory = scope.login_lease_directory()?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let mut file = directory.open_with(NAME, &options).map_err(|_| failure())?;
    lock(&file, libc::LOCK_SH)?;
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
    for (index, line) in lines.enumerate() {
        let event = parse_record(line)?;
        if index as u64 >= MAX_EVENTS
            || event.as_object().map(|o| o.len()) != Some(2)
            || event["seq"].as_u64() != Some(index as u64 + 1)
        {
            return Err(failure());
        }
        apply(&mut state, event["event"].as_str().ok_or_else(failure)?)?;
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
    Ok((operation, provider.to_owned(), state.phase()))
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
