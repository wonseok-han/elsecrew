//! macOS installer execution behind destination-bound explicit consent.
//! Upstream scripts are trusted installation code, not model-generated commands.
use super::{
    consent::{self, AuthorizedInstall},
    download::InstallerReservation,
    failure,
    journal::InstallJournal,
};
use crate::{
    provider_login_io::OutputChunk,
    provider_profile::{open_absolute_directory, same_directory, LoginProfile},
    SidecarError,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
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
#[cfg(test)]
thread_local! {
    pub(super) static INSTALL_DIAGNOSTICS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
const POLICY: &str = r#"(version 1)
(deny default)
(deny file-link)
(allow file-read-metadata)
(allow file-read*
    (literal "/")
    (subpath "/System") (subpath "/usr/bin") (subpath "/usr/sbin")
    (subpath "/usr/lib") (subpath "/usr/share") (subpath "/bin") (subpath "/sbin")
    (literal "/dev/null") (literal "/dev/urandom")
    (literal "/private/etc/ssl/cert.pem") (literal "/private/etc/resolv.conf")
    (literal "/private/etc/ssl/openssl.cnf")
    (literal "/private/etc/hosts") (literal "/private/etc/localtime")
    (subpath (param "TEMP")))
(allow process-exec
    (subpath "/usr/bin") (subpath "/usr/sbin") (subpath "/bin") (subpath "/sbin"))
(allow process-fork)
(allow signal (target self))
(allow sysctl-read)
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))
; DNS and HTTPS only. No generic Mach, AppleEvent, local socket or listener grant.
; HTTPS here is a port restriction, not a destination-domain authentication rule.
(allow network-outbound (literal "/private/var/run/mDNSResponder") (remote tcp "*:443"))
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
    fn prepare(
        journal: &InstallJournal<'_>,
        authorization: &AuthorizedInstall,
    ) -> Result<Self, SidecarError> {
        journal.verify()?;
        if journal.operation != authorization.operation()
            || journal.provider != authorization.provider()
        {
            return Err(failure());
        }
        let (home_path, home) = authorization.bind_home()?;
        let mut directories = Vec::new();
        let roots = consent::directories(&journal.provider)?;
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
        let mut remaining = 20_000;
        for (_, held) in &self.directories {
            reject_shared_files(held, 0, &mut remaining)?;
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
                "(allow file-read* file-write* process-exec (subpath (param \"{key}\")))\n"
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
            .env("CURL_CA_BUNDLE", "/private/etc/ssl/cert.pem")
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

/// Inspect metadata only. Official launcher symlinks remain supported; reads
/// and writes through them are still bounded by the sandbox's resolved paths.
/// Pre-existing hard links and special files are refused, never repaired.
fn reject_shared_files(
    root: &Dir,
    depth: usize,
    remaining: &mut usize,
) -> Result<(), SidecarError> {
    if depth > 64 {
        return Err(failure());
    }
    for entry in root.entries().map_err(|_| failure())? {
        *remaining = remaining.checked_sub(1).ok_or_else(failure)?;
        let name = entry.map_err(|_| failure())?.file_name();
        let before = root.symlink_metadata(&name).map_err(|_| failure())?;
        if before.uid() != unsafe { libc::geteuid() } {
            return Err(failure());
        }
        if before.file_type().is_symlink() {
            if before.nlink() != 1 {
                return Err(failure());
            }
        } else if before.is_file() {
            if before.nlink() != 1 || before.mode() & 0o022 != 0 {
                return Err(failure());
            }
        } else if before.is_dir() {
            let mut options = cap_std::fs::OpenOptions::new();
            options
                .read(true)
                .follow(FollowSymlinks::No)
                .maybe_dir(true)
                .custom_flags(libc::O_NONBLOCK);
            let handle = root.open_with(&name, &options).map_err(|_| failure())?;
            let held = handle.metadata().map_err(|_| failure())?;
            if !held.is_dir()
                || held.dev() != before.dev()
                || held.ino() != before.ino()
                || held.mode() & 0o022 != 0
            {
                return Err(failure());
            }
            reject_shared_files(&Dir::from_std_file(handle.into_std()), depth + 1, remaining)?;
        } else {
            return Err(failure());
        }
        let after = root.symlink_metadata(&name).map_err(|_| failure())?;
        if after.dev() != before.dev()
            || after.ino() != before.ino()
            || after.nlink() != before.nlink()
            || after.mode() != before.mode()
            || after.uid() != before.uid()
        {
            return Err(failure());
        }
    }
    Ok(())
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
        authorization: &AuthorizedInstall,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        journal.verify()?;
        if self.begun
            || journal.phase() != InstallPhase::Installing
            || journal.operation != self.operation
            || journal.provider != self.provider
            || authorization.operation() != self.operation
            || authorization.provider() != self.provider
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
            held_target = Some(Target::prepare(journal, authorization)?);
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
        #[cfg(test)]
        INSTALL_DIAGNOSTICS.with(|enabled| {
            if enabled.get() {
                eprintln!("installer execution result: {result:?}");
            }
        });
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
                        #[cfg(test)]
                        INSTALL_DIAGNOSTICS.with(|enabled| {
                            if enabled.get() {
                                eprintln!("installer output: {}", String::from_utf8_lossy(&bytes));
                            }
                        });
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
        assert!(policy.contains("(deny default)"));
        assert!(!policy.contains("(allow default)"));
        assert!(policy.contains("(deny file-link)"));
        assert!(!policy.contains("(subpath (param \"HOME\"))"));
        assert!(!policy.contains("(allow mach-lookup)"));
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

    #[test]
    fn writable_installation_roots_reject_hardlinks_without_modifying_originals() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let target = target(&root);
        let original = root.join(".zshrc");
        std::fs::write(&original, b"preserve shell settings").unwrap();
        let linked = root.join(".local/bin/linked-command");
        std::fs::hard_link(&original, &linked).unwrap();
        assert!(target.command("codex", b"#!/bin/sh\nexit 0\n").is_err());
        assert_eq!(
            std::fs::read(&original).unwrap(),
            b"preserve shell settings"
        );
        assert_eq!(
            std::fs::metadata(&original).unwrap().len(),
            std::fs::metadata(&linked).unwrap().len()
        );
        let mut empty_budget = 0;
        assert!(reject_shared_files(&target.directories[0].1, 0, &mut empty_budget).is_err());
    }

    #[test]
    #[ignore = "requires external macOS sandbox; temporary fixtures and system tools only, no installer/network/account"]
    fn installer_default_deny_policy_protects_outside_files_through_children_and_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir(&home).unwrap();
        let target = target(&home);
        let outside = home.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let original = outside.join("original");
        std::fs::write(&original, b"original sentinel").unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let scope = super::super::InstallScope::from_storage(&storage).unwrap();
        let script = br##"#!/bin/sh
set -eu
printf allowed > "$HOME/.local/bin/probe"
printf allowed > "$HOME/.codex/packages/standalone/probe"
printf temporary > "$TMPDIR/probe"
if (printf changed > "$OUTSIDE/original") 2>/dev/null; then exit 31; fi
if /bin/sh -c 'printf changed > "$OUTSIDE/original"' 2>/dev/null; then exit 32; fi
if /bin/cat "$OUTSIDE/original" >/dev/null 2>&1; then exit 33; fi
if /bin/ln "$OUTSIDE/original" "$HOME/.local/bin/hard" 2>/dev/null; then exit 34; fi
/bin/ln -s "$OUTSIDE/original" "$HOME/.local/bin/symbolic"
if (printf changed > "$HOME/.local/bin/symbolic") 2>/dev/null; then exit 35; fi
if /bin/mv "$OUTSIDE/original" "$HOME/.local/bin/moved" 2>/dev/null; then exit 36; fi
/usr/bin/defaults write "$OUTSIDE/preferences" protected changed >/dev/null 2>&1 || true
printf installer-boundary-ok
"##;
        let mut command = target.command("codex", script).unwrap();
        command.env("OUTSIDE", &outside);
        let mut owner = scope.reserve(uuid::Uuid::new_v4()).unwrap();
        owner
            .launch_with_barrier(|| command.spawn(), crate::provider_login_gate::release)
            .unwrap();
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let mut output = Vec::new();
        let outcome = (|| -> Result<bool, SidecarError> {
            loop {
                if started.elapsed() > Duration::from_secs(15) {
                    return Err(failure());
                }
                match owner.poll_output(&cancel)? {
                    OutputChunk::Bytes(bytes) => {
                        output.extend(bytes);
                        if output.len() > 16 * 1024 {
                            return Err(failure());
                        }
                    }
                    OutputChunk::Eof => {
                        if let Some(status) = owner.exit_status()? {
                            return Ok(status.success());
                        }
                    }
                    OutputChunk::Pending => {}
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        })();
        owner.finish().unwrap();
        assert!(
            outcome.unwrap(),
            "sandbox toy command failed: {}",
            String::from_utf8_lossy(&output)
        );
        assert!(String::from_utf8_lossy(&output).contains("installer-boundary-ok"));
        assert_eq!(std::fs::read(&original).unwrap(), b"original sentinel");
        assert!(!outside.join("preferences.plist").exists());
        assert_eq!(
            std::fs::read(home.join(".local/bin/probe")).unwrap(),
            b"allowed"
        );
        assert_eq!(
            std::fs::read(home.join(".codex/packages/standalone/probe")).unwrap(),
            b"allowed"
        );
    }
}
