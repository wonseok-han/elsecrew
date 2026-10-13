//! Internal macOS installer execution. Public consent/activation is not wired.
//! Upstream scripts are trusted installation code, not model-generated commands.
use super::{download::InstallerReservation, failure, journal::InstallJournal};
use crate::{
    provider_login_io::OutputChunk,
    provider_profile::{open_absolute_directory, same_directory, LoginProfile},
    SidecarError,
};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, MetadataExt, PermissionsExt};
use hello_agent_core::install_lifecycle::InstallPhase;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(600);
const OUTPUT_LIMIT: usize = 1024 * 1024;
const POLICY: &str = r#"(version 1)
(allow default)
(deny file-write*)
(allow file-write-data (literal "/dev/null"))
(allow file-write* (subpath (param "TEMP")))
"#;

/// Held capabilities prevent silently switching the destination during launch.
/// Only fixed installation directories are writable; shell profiles, credentials,
/// provider settings and user projects are not in the sandbox write allowlist.
pub(super) struct Target {
    home_path: PathBuf,
    home: Dir,
    temporary: PathBuf,
    directories: Vec<(PathBuf, Dir)>,
}

impl Target {
    fn prepare(journal: &InstallJournal<'_>) -> Result<Self, SidecarError> {
        journal.verify()?;
        let home_path = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(failure)?;
        let home = checked_directory(&home_path)?;
        let mut directories = Vec::new();
        let roots: &[&str] = match journal.provider.as_str() {
            "codex" => &[".local/bin", ".codex/packages/standalone"],
            "claude" => &[
                ".local/bin",
                ".claude/downloads",
                ".local/share/claude",
                ".local/state/claude",
                ".cache/claude",
            ],
            _ => return Err(failure()),
        };
        for relative in roots {
            directories.push(prepare_directory(&home_path, relative)?);
        }
        let staging = journal.scope.revalidate()?;
        let (temporary, temporary_dir) = prepare_directory(&staging, "installer-tmp")?;
        directories.push((temporary.clone(), temporary_dir));
        let target = Self {
            home_path,
            home,
            temporary,
            directories,
        };
        target.revalidate()?;
        Ok(target)
    }

    fn revalidate(&self) -> Result<(), SidecarError> {
        if !same_directory(&self.home, &checked_directory(&self.home_path)?)? {
            return Err(failure());
        }
        for (path, held) in &self.directories {
            if !same_directory(held, &checked_directory(path)?)? {
                return Err(failure());
            }
        }
        Ok(())
    }

    fn verify_installation(&self, provider: &str) -> Result<(), SidecarError> {
        self.revalidate()?;
        match provider {
            "codex" => {
                let (cli, helper) =
                    crate::provider_login_manager::select_standalone_codex_pair(&self.home_path)?;
                cli.revalidate()?;
                helper.revalidate()?;
            }
            "claude" => {
                crate::provider_claude_login::installed_executable_at_home(&self.home_path)?
                    .revalidate()?;
            }
            _ => return Err(failure()),
        }
        self.revalidate()
    }

    fn command(&self, provider: &str, bytes: &[u8]) -> Result<Command, SidecarError> {
        self.revalidate()?;
        let script = std::str::from_utf8(bytes).map_err(|_| failure())?;
        if bytes.len() > 128 * 1024 || !bytes.starts_with(b"#!") || bytes.contains(&0) {
            return Err(failure());
        }
        let shell = match provider {
            "codex" => "/bin/sh",
            "claude" => "/bin/bash",
            _ => return Err(failure()),
        };
        let mut policy = POLICY.to_owned();
        let mut parameters = vec![OsString::from("-D"), parameter("TEMP", &self.temporary)];
        // The final directory is the temporary root, already in POLICY.
        for (index, (path, _)) in self.directories[..self.directories.len() - 1]
            .iter()
            .enumerate()
        {
            let key = format!("INSTALL{index}");
            policy.push_str(&format!(
                "(allow file-write* (subpath (param \"{key}\")))\n"
            ));
            parameters.extend([OsString::from("-D"), parameter(&key, path)]);
        }
        let mut args = vec![OsString::from("-p"), OsString::from(policy)];
        args.extend(parameters);
        args.extend([
            OsString::from(shell),
            OsString::from("-c"),
            // Stream the official script from memory as one argument, avoiding
            // publishing/reopening an executable script at a mutable file path.
            OsString::from(format!("exec 2>&1\n{script}")),
            OsString::from("hello-agent-official-installer"),
        ]);
        let references: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
        let mut command = crate::provider_login_gate::command(
            Path::new("/usr/bin/sandbox-exec"),
            &references,
            &self.temporary,
        )
        .map_err(|_| failure())?;
        let mut path = OsString::from("/usr/bin:/bin:/usr/sbin:/sbin:");
        path.push(self.home_path.join(".local/bin"));
        command
            .env("HOME", &self.home_path)
            .env("PATH", path)
            .env("TMPDIR", &self.temporary)
            .env("CURL_HOME", &self.temporary)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("CODEX_NON_INTERACTIVE", "true");
        Ok(command)
    }
}

fn parameter(name: &str, path: &Path) -> OsString {
    let mut value = OsString::from(format!("{name}="));
    value.push(path);
    value
}

fn checked_directory(path: &Path) -> Result<Dir, SidecarError> {
    let directory = open_absolute_directory(path)?;
    let meta = directory.dir_metadata().map_err(|_| failure())?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.permissions().mode() & 0o022 != 0 {
        return Err(failure());
    }
    Ok(directory)
}

fn prepare_directory(root: &Path, relative: &str) -> Result<(PathBuf, Dir), SidecarError> {
    let mut path = root.to_owned();
    let mut parent = checked_directory(root)?;
    // All components come from fixed backend literals above, never UI input.
    for component in relative.split('/') {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err(failure());
        }
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        match parent.create_dir_with(component, &builder) {
            Ok(()) => parent
                .try_clone()
                .map_err(|_| failure())?
                .into_std_file()
                .sync_all()
                .map_err(|_| failure())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(failure()),
        }
        path.push(component);
        parent = checked_directory(&path)?;
    }
    Ok((path, parent))
}

impl InstallerReservation<'_> {
    /// Leaves both owners retained. Success reaches Verifying, never Installed.
    /// No retry, authentication call, model task, or original-project access.
    pub(crate) fn run(
        &mut self,
        journal: &mut InstallJournal<'_>,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        journal.verify()?;
        if self.begun
            || journal.phase() != InstallPhase::Installing
            || journal.operation != self.operation
            || journal.provider != self.provider
            || !std::ptr::eq(journal.scope, self.scope)
            || !self
                .downloader
                .closure_evidence()
                .is_some_and(|e| e.confirmed())
        {
            return Err(failure());
        }
        self.begun = true;
        if cancel.load(Ordering::Acquire) {
            journal.record("cancelRequested")?;
            return Ok(());
        }
        let mut held_target = None;
        let result: Result<Option<bool>, SidecarError> = (|| {
            held_target = Some(Target::prepare(journal)?);
            let target = held_target.as_ref().ok_or_else(failure)?;
            let mut command = target.command(&self.provider, &self.bytes)?;
            self.installer.launch_with_barrier(
                || command.spawn(),
                |child| {
                    if cancel.load(Ordering::Acquire)
                        || journal.verify().is_err()
                        || target.revalidate().is_err()
                    {
                        return Err(std::io::Error::other("installer destination changed"));
                    }
                    crate::provider_login_gate::release(child)
                },
            )?;
            self.collect(cancel)
        })();
        if self.installer.finish().is_err()
            || !self
                .installer
                .closure_evidence()
                .is_some_and(|e| e.confirmed())
        {
            let _ = journal.record("recoveryRequired");
            return Err(failure());
        }
        if held_target
            .as_ref()
            .is_some_and(|target| target.revalidate().is_err())
        {
            let _ = journal.record("recoveryRequired");
            return Err(failure());
        }
        self.target = held_target;
        match result {
            Ok(Some(true)) if !cancel.load(Ordering::Acquire) => journal.record("installerExited"),
            Ok(None) | Ok(Some(true)) => journal.record("cancelRequested"),
            Ok(Some(false)) | Err(_) => {
                journal.record("installerFailed")?;
                if cancel.load(Ordering::Acquire) {
                    journal.record("cancelRequested")?;
                }
                Err(SidecarError::new(
                    "providerInstallFailed",
                    "official installer did not complete",
                ))
            }
        }
    }

    /// Signature/path checks are installation evidence only, not login or model
    /// readiness. Both subprocesses stay reserved throughout this verification.
    pub(crate) fn verify_result(
        &mut self,
        journal: &mut InstallJournal<'_>,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        self.check_transaction(journal)?;
        if journal.phase() != InstallPhase::Verifying || !self.begun {
            return Err(failure());
        }
        if cancel.load(Ordering::Acquire) {
            return journal.record("cancelRequested");
        }
        let result = self
            .target
            .as_ref()
            .ok_or_else(failure)?
            .verify_installation(&self.provider);
        // Persist a failed check before observing a late cancellation.
        journal.record(if result.is_ok() {
            "verified"
        } else {
            "verificationFailed"
        })?;
        if cancel.load(Ordering::Acquire) {
            journal.record("cancelRequested")?;
        }
        result
    }

    fn check_transaction(&self, journal: &InstallJournal<'_>) -> Result<(), SidecarError> {
        journal.verify()?;
        if journal.operation != self.operation
            || journal.provider != self.provider
            || !std::ptr::eq(journal.scope, self.scope)
            || !self
                .downloader
                .closure_evidence()
                .is_some_and(|e| e.confirmed())
            || !self
                .installer
                .closure_evidence()
                .is_some_and(|e| e.confirmed())
        {
            return Err(failure());
        }
        Ok(())
    }

    /// Durable terminal evidence must precede either fence release. Keep the
    /// journal in place for terminal archival; it still blocks a new transaction.
    pub(crate) fn finalize(
        &mut self,
        journal: &mut InstallJournal<'_>,
    ) -> Result<(), SidecarError> {
        if journal.phase() != InstallPhase::Stopping {
            return Err(failure());
        }
        // Includes cancellation before the installer was spawned.
        if self.installer.closure_evidence().is_none() && self.installer.finish().is_err() {
            let _ = journal.record("recoveryRequired");
            return Err(failure());
        }
        self.check_transaction(journal)?;
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.revalidate().is_err())
        {
            journal.record("recoveryRequired")?;
            return Err(failure());
        }
        journal.record("cleanupConfirmed")?;
        self.installer.release_after_run_terminal()?;
        self.downloader.release_after_run_terminal()?;
        Ok(())
    }

    fn collect(&mut self, cancel: &AtomicBool) -> Result<Option<bool>, SidecarError> {
        let started = Instant::now();
        let transport_cancel = AtomicBool::new(false);
        let mut count = 0usize;
        let mut eof = false;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Ok(None);
            }
            if started.elapsed() >= DEADLINE {
                return Err(failure());
            }
            if !eof {
                match self.installer.poll_output(&transport_cancel)? {
                    OutputChunk::Bytes(bytes) => {
                        count += bytes.len();
                        if count > OUTPUT_LIMIT {
                            return Err(failure());
                        }
                        continue;
                    }
                    OutputChunk::Eof => eof = true,
                    OutputChunk::Pending => {}
                }
            }
            if eof {
                if let Some(status) = self.installer.exit_status()? {
                    return Ok(Some(status.success()));
                }
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(root: &Path) -> Target {
        let home = checked_directory(root).unwrap();
        let bin = prepare_directory(root, ".local/bin").unwrap();
        let package = prepare_directory(root, ".codex/packages/standalone").unwrap();
        let temp = prepare_directory(root, "private-tmp").unwrap();
        Target {
            home_path: root.to_owned(),
            home,
            temporary: temp.0.clone(),
            directories: vec![bin, package, temp],
        }
    }

    #[test]
    fn failed_target_verification_is_durable_before_fences_are_released() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let scope = super::super::InstallScope::from_storage(&storage).unwrap();
        let execution = scope.execution_scope().unwrap();
        let operation = uuid::Uuid::new_v4();
        let mut journal = InstallJournal::begin(&scope, operation, "codex").unwrap();
        for event in ["consented", "downloadVerified", "installerExited"] {
            journal.record(event).unwrap();
        }
        // Synthetic process-free receipts exercise transaction ordering only.
        let mut downloader = scope.reserve(operation).unwrap();
        downloader.finish().unwrap();
        let mut installer = execution.reserve(operation).unwrap();
        installer.finish().unwrap();
        let home = root.join("empty-home");
        std::fs::create_dir(&home).unwrap();
        let mut reservation = InstallerReservation {
            bytes: Vec::new(),
            downloader,
            installer,
            operation,
            provider: "codex".into(),
            begun: true,
            scope: &scope,
            target: Some(target(&home)),
        };
        assert!(reservation.finalize(&mut journal).is_err());
        assert!(reservation
            .verify_result(&mut journal, &AtomicBool::new(false))
            .is_err());
        assert_eq!(journal.phase(), InstallPhase::Stopping);
        assert!(root.join(super::super::PENDING).exists());
        assert!(root.join(super::super::EXECUTION_PENDING).exists());
        reservation.finalize(&mut journal).unwrap();
        assert_eq!(journal.phase(), InstallPhase::Failed);
        assert!(!root.join(super::super::PENDING).exists());
        assert!(!root.join(super::super::EXECUTION_PENDING).exists());
        assert!(reservation.finalize(&mut journal).is_err());
        drop(journal);
        assert_eq!(
            super::super::journal::inspect(&scope).unwrap().2,
            InstallPhase::Failed
        );
        assert!(InstallJournal::begin(&scope, uuid::Uuid::new_v4(), "codex").is_err());
    }

    #[test]
    fn installation_verifier_never_accepts_an_unrelated_or_missing_installation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let target = target(&root);
        assert!(target.verify_installation("codex").is_err());
        assert!(target.verify_installation("claude").is_err());
        // Even a legacy executable pair is not the requested standalone install.
        std::fs::write(root.join(".local/bin/codex"), b"legacy").unwrap();
        std::fs::write(root.join(".local/bin/codex-code-mode-host"), b"legacy").unwrap();
        assert!(target.verify_installation("codex").is_err());
        assert!(target.verify_installation("other").is_err());
    }

    #[test]
    fn target_creation_does_not_modify_settings_or_follow_links() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join(".zshrc"), b"keep this setting").unwrap();
        let target = target(&root);
        assert_eq!(
            std::fs::read(root.join(".zshrc")).unwrap(),
            b"keep this setting"
        );
        assert!(target.revalidate().is_ok());
        std::os::unix::fs::symlink(root.join(".local"), root.join("redirect")).unwrap();
        assert!(prepare_directory(&root, "redirect/new").is_err());
        assert!(!root.join(".local/new").exists());
        std::fs::rename(root.join(".local/bin"), root.join("retained-bin")).unwrap();
        std::fs::create_dir(root.join(".local/bin")).unwrap();
        assert!(target.revalidate().is_err());
    }

    #[test]
    fn command_uses_parameterized_paths_and_only_explicit_environment() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home_path = root.join("home \" (allow default)");
        std::fs::create_dir(&home_path).unwrap();
        let target = target(&home_path);
        let command = target.command("codex", b"#!/bin/sh\nexit 0\n").unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert!(args.contains(&OsStr::new("/usr/bin/sandbox-exec")));
        let policy_index = args.iter().position(|a| *a == "-p").unwrap() + 1;
        let policy = args[policy_index].to_str().unwrap();
        assert!(policy.contains("(deny file-write*)"));
        assert!(!policy.contains(home_path.to_str().unwrap()));
        assert!(args.contains(&parameter("INSTALL0", &home_path.join(".local/bin")).as_os_str()));
        let environment: Vec<_> = command.get_envs().collect();
        assert!(environment.contains(&(
            OsStr::new("CODEX_NON_INTERACTIVE"),
            Some(OsStr::new("true"))
        )));
        assert!(environment.contains(&(OsStr::new("HOME"), Some(home_path.as_os_str()))));
        assert!(!environment
            .iter()
            .any(|(key, _)| matches!(key.to_str(), Some("BASH_ENV" | "ENV" | "CODEX_HOME"))));
        assert!(target.command("other", b"#!/bin/sh\n").is_err());
        assert!(target.command("claude", b"<html>error</html>").is_err());
    }
}
