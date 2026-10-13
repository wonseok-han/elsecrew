//! Download stage only. No script execution, path publication, or fence release.
use super::{failure, journal::InstallJournal};
use crate::{
    provider_login_io::OutputChunk, provider_login_process::LoginProcess,
    provider_profile::LoginProfile, SidecarError,
};
use hello_agent_core::install_lifecycle::InstallPhase;
use std::{
    ffi::OsStr,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_SCRIPT_BYTES: usize = 128 * 1024;
const DEADLINE: Duration = Duration::from_secs(65);

/// The retained owner prevents another process reservation. The coordinator must
/// implement a durable step handoff before launching an installer; dropping this
/// value does not release the fence. Bytes are not executable admission evidence.
pub(crate) struct DownloadedScript<'a> {
    bytes: Vec<u8>,
    owner: LoginProcess<'a>,
    operation: uuid::Uuid,
    provider: String,
    scope: &'a super::InstallScope,
}

/// Keeps the confirmed download owner alive alongside the fresh installer owner.
/// No installer is launched by reservation. The coordinator must keep both until
/// a durable terminal result; neither owner is transferable to another journal.
pub(crate) struct InstallerReservation<'a> {
    pub(crate) bytes: Vec<u8>,
    pub(crate) downloader: LoginProcess<'a>,
    pub(crate) installer: LoginProcess<'a>,
}

impl<'a> DownloadedScript<'a> {
    pub(crate) fn reserve_installer(
        self,
        journal: &InstallJournal<'a>,
        execution: &'a super::InstallScope,
        cancel: &AtomicBool,
    ) -> Result<InstallerReservation<'a>, SidecarError> {
        journal.verify()?;
        if cancel.load(Ordering::Acquire)
            || journal.phase() != InstallPhase::Installing
            || journal.operation != self.operation
            || journal.provider != self.provider
            || !std::ptr::eq(journal.scope, self.scope)
            || !execution.is_execution_of(self.scope)?
            || !self.owner.closure_evidence().is_some_and(|e| e.confirmed())
        {
            return Err(failure());
        }
        let installer = execution.reserve(self.operation)?;
        journal.verify()?;
        Ok(InstallerReservation {
            bytes: self.bytes,
            downloader: self.owner,
            installer,
        })
    }
}

fn arguments(provider: &str) -> Result<Vec<&'static OsStr>, SidecarError> {
    let url = match provider {
        "codex" => "https://chatgpt.com/codex/install.sh",
        "claude" => "https://claude.ai/install.sh",
        _ => return Err(failure()),
    };
    // -q must be first: never read a user's curlrc. Restrict redirects to HTTPS,
    // bound their count, disable inherited proxy settings via the gate's env_clear.
    Ok([
        "-q",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-redirs",
        "5",
        "--connect-timeout",
        "15",
        "--max-time",
        "60",
        "--max-filesize",
        "131072",
        url,
    ]
    .into_iter()
    .map(OsStr::new)
    .collect())
}

/// Caller has consumed consent for the exact journal plan. Not exposed over IPC.
/// All failure paths retain durable evidence; none retry or execute partial data.
pub(crate) fn fetch<'a>(
    journal: &mut InstallJournal<'a>,
    cancel: &AtomicBool,
) -> Result<DownloadedScript<'a>, SidecarError> {
    if journal.scope.execution || journal.phase() != InstallPhase::Downloading {
        return Err(failure());
    }
    journal.verify()?;
    if cancel.load(Ordering::Acquire) {
        journal.record("cancelRequested")?;
        return Err(download_error("installation download canceled"));
    }
    let args = arguments(&journal.provider)?;
    let scope = journal.scope;
    let cwd = scope.revalidate()?;
    let mut owner = scope.reserve(journal.operation)?;
    let result = (|| {
        // Revalidate immediately before the gate is released. Neither a shell
        // command string nor a destination path comes from downloaded content.
        owner.launch_with_barrier(
            || crate::provider_login_gate::spawn(Path::new("/usr/bin/curl"), &args, &cwd),
            |child| {
                if cancel.load(Ordering::Acquire)
                    || journal.verify().is_err()
                    || scope.revalidate().is_err()
                {
                    return Err(std::io::Error::other(
                        "installation download admission changed",
                    ));
                }
                crate::provider_login_gate::release(child)
            },
        )?;
        collect(&mut owner, cancel)
    })();
    // Even EOF and exit 0 do not prove descendants are gone. Keep the reservation
    // after explicit cleanup; the installer transaction is still unfinished.
    if owner.finish().is_err() || !owner.closure_evidence().is_some_and(|e| e.confirmed()) {
        let _ = journal.record("recoveryRequired");
        return Err(failure());
    }
    match result {
        Ok(Some(bytes)) if !cancel.load(Ordering::Acquire) => {
            journal.record("downloadVerified")?;
            Ok(DownloadedScript {
                bytes,
                owner,
                operation: journal.operation,
                provider: journal.provider.clone(),
                scope,
            })
        }
        result => {
            // A late cancellation cannot erase a preceding download failure.
            if result.is_err() {
                journal.record("downloadFailed")?;
            }
            if result.is_ok() || cancel.load(Ordering::Acquire) {
                journal.record("cancelRequested")?;
            }
            // Terminal archival/release belongs to the coordinator, not fetch.
            Err(download_error("installation download did not complete"))
        }
    }
}

fn collect(
    owner: &mut LoginProcess<'_>,
    cancel: &AtomicBool,
) -> Result<Option<Vec<u8>>, SidecarError> {
    let started = Instant::now();
    let mut bytes = Vec::new();
    let mut eof = false;
    // Cancellation is handled between bounded reads. Passing it into LoginIo
    // would mark ordinary cancellation as a transport-evidence failure.
    let transport_cancel = AtomicBool::new(false);
    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        if started.elapsed() >= DEADLINE {
            return Err(download_error("installation download timed out"));
        }
        if !eof {
            match owner.poll_output(&transport_cancel)? {
                OutputChunk::Bytes(chunk) => {
                    append(&mut bytes, &chunk)?;
                    continue;
                }
                OutputChunk::Eof => eof = true,
                OutputChunk::Pending => {}
            }
        }
        if eof {
            if let Some(status) = owner.exit_status()? {
                if status.success() && valid_script(&bytes) {
                    return Ok(Some(bytes));
                }
                return Err(download_error(
                    "installation download returned invalid data",
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn append(bytes: &mut Vec<u8>, chunk: &[u8]) -> Result<(), SidecarError> {
    if chunk.len() > MAX_SCRIPT_BYTES.saturating_sub(bytes.len()) {
        return Err(download_error("installation download exceeded size limit"));
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

fn valid_script(bytes: &[u8]) -> bool {
    // Reject empty/error pages and binary data. This is a format check, not a
    // signature, checksum pin, or substitute for trusting the official endpoint.
    bytes.starts_with(b"#!") && !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

fn download_error(detail: &str) -> SidecarError {
    SidecarError::new("providerInstallDownloadFailed", detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_download_before_consent_or_after_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let scope = super::super::InstallScope::from_storage(&storage).unwrap();
        let mut journal = InstallJournal::begin(&scope, uuid::Uuid::new_v4(), "claude").unwrap();
        assert!(fetch(&mut journal, &AtomicBool::new(false)).is_err());
        assert_eq!(journal.phase(), InstallPhase::AwaitingConsent);
        assert!(!root.join(super::super::PENDING).exists());
        journal.record("consented").unwrap();
        assert!(fetch(&mut journal, &AtomicBool::new(true)).is_err());
        assert_eq!(journal.phase(), InstallPhase::Stopping);
        assert!(!root.join(super::super::PENDING).exists());
    }

    #[test]
    fn download_targets_and_transport_policy_are_fixed() {
        for (provider, url) in [
            ("codex", "https://chatgpt.com/codex/install.sh"),
            ("claude", "https://claude.ai/install.sh"),
        ] {
            let args = arguments(provider).unwrap();
            assert_eq!(args.first(), Some(&OsStr::new("-q")));
            assert_eq!(args.last(), Some(&OsStr::new(url)));
            assert!(args
                .windows(2)
                .any(|w| w == [OsStr::new("--proto-redir"), OsStr::new("=https")]));
            assert!(!args.contains(&OsStr::new("--output")));
        }
        assert!(arguments("https://example.com/install.sh").is_err());
    }

    #[test]
    fn oversized_or_non_script_output_is_never_published() {
        let mut bytes = vec![b'a'; MAX_SCRIPT_BYTES - 1];
        append(&mut bytes, b"b").unwrap();
        assert!(append(&mut bytes, b"c").is_err());
        assert_eq!(bytes.len(), MAX_SCRIPT_BYTES);
        assert!(valid_script(b"#!/bin/sh\nexit 0\n"));
        for value in [
            b"".as_slice(),
            b"<html>error</html>",
            b"#!/bin/sh\0",
            b"#!\xff",
        ] {
            assert!(!valid_script(value));
        }
    }
}
