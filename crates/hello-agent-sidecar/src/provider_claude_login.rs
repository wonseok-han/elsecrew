//! Publisher-authenticated, model-free Claude auth in the app credential namespace.
use crate::{
    provider_claude_profile::ClaudeProfileBinding, provider_executable::PinnedExecutable,
    provider_login_gate, provider_login_io::OutputChunk, provider_login_process::LoginProcess,
    provider_profile::LoginProfile, SidecarError,
};
use hello_agent_core::provider::{
    vendor_observation::{AuthObservation, Vendor},
    BoundedObservation,
};
use std::os::unix::process::CommandExt;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn failure() -> SidecarError {
    SidecarError::new("providerLoginFailed", "Claude login could not be verified")
}

pub(crate) fn diagnose() -> Result<(), SidecarError> {
    installed_executable().map(|_| ())
}

/// Shared native discovery for login and future runtime owners. Does not execute it.
pub(crate) fn installed_executable() -> Result<PinnedExecutable, SidecarError> {
    installed_executable_at_home(&PathBuf::from(
        std::env::var_os("HOME").ok_or_else(failure)?,
    ))
}

/// Use the install transaction's retained home, not a fresh environment lookup.
pub(crate) fn installed_executable_at_home(
    home: &std::path::Path,
) -> Result<PinnedExecutable, SidecarError> {
    let versions = home.join(".local/share/claude/versions");
    select_executable(&versions, &home.join(".local/bin/claude"))
}

pub(crate) fn run(
    profile: &ClaudeProfileBinding,
    cancel: &AtomicBool,
    publish: &impl Fn(&str, Option<String>, Option<&str>),
    check_only: bool,
    code: &std::sync::Mutex<Option<String>>,
) -> Result<(), SidecarError> {
    // Resolve only the native installer's bounded link target, then authenticate
    // and execute the concrete binary, never the mutable launcher itself.
    let executable = installed_executable()?;
    if !check_only {
        let (_, exit) = command_with_code(profile, &executable, cancel, false, publish, code)?;
        if cancel.load(Ordering::Acquire) {
            publish("canceled", None, None);
            return Ok(());
        }
        if exit != 0 {
            return Err(failure());
        }
    }
    if cancel.load(Ordering::Acquire) {
        publish("canceled", None, None);
        return Ok(());
    }
    crate::provider_claude_refresh::refresh_if_expired(profile)?;
    let (bytes, exit) = command(profile, &executable, cancel, true, publish)?;
    if cancel.load(Ordering::Acquire) {
        publish("canceled", None, None);
        return Ok(());
    }
    let connected = account(&bytes, exit, &profile.revalidate()?)?;
    if !check_only && !connected {
        return Err(failure());
    }
    publish(if connected { "connected" } else { "idle" }, None, None);
    Ok(())
}

fn select_executable(
    versions: &std::path::Path,
    launcher: &std::path::Path,
) -> Result<PinnedExecutable, SidecarError> {
    if std::fs::symlink_metadata(versions)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Err(SidecarError::new(
            "providerExecutableMissing",
            "Claude installation directory is unavailable",
        ));
    }
    let parent =
        crate::provider_profile::open_absolute_directory(launcher.parent().ok_or_else(failure)?)?;
    let target = parent
        .read_link_contents(launcher.file_name().ok_or_else(failure)?)
        .map_err(|_| failure())?;
    let selected = selected_version_path(versions, &target)?;
    PinnedExecutable::open_claude_signed(&selected)
}

fn selected_version_path(
    versions: &std::path::Path,
    target: &std::path::Path,
) -> Result<PathBuf, SidecarError> {
    let relative = target
        .strip_prefix(versions)
        .or_else(|_| target.strip_prefix("../share/claude/versions"))
        .map_err(|_| failure())?;
    let name = relative.to_str().ok_or_else(failure)?;
    let parts: Vec<_> = name.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|p| {
            p.is_empty()
                || p.len() > 9
                || !p.bytes().all(|b| b.is_ascii_digit())
                || (p.len() > 1 && p.starts_with('0'))
        })
    {
        return Err(failure());
    }
    Ok(versions.join(name))
}

pub(crate) fn account(bytes: &[u8], exit: i32, home: &std::path::Path) -> Result<bool, SidecarError> {
    let observation = BoundedObservation::new(bytes.to_vec()).map_err(|_| failure())?;
    let auth = Vendor::Claude21269
        .observe_auth(&observation, exit)
        .map_err(|_| failure())?;
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| failure())?;
    if value["configDirectory"].as_str() != home.to_str()
        || value["apiProvider"] != "firstParty"
        || (auth == AuthObservation::CredentialsPresent && value["authMethod"] != "claude.ai")
    {
        return Err(failure());
    }
    Ok(auth == AuthObservation::CredentialsPresent)
}

fn browser_url(bytes: &[u8]) -> Result<Option<String>, SidecarError> {
    // Only complete whitespace-delimited URLs, so partial pipe reads cannot open
    // a truncated OAuth request. Raw output and URLs are never persisted.
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) if e.error_len().is_none() => return Ok(None),
        Err(_) => return Err(failure()),
    };
    for part in text.split_inclusive(char::is_whitespace) {
        if !part.ends_with(char::is_whitespace) {
            continue;
        }
        let url = part.trim();
        if url.starts_with("https://claude.ai/oauth/authorize?")
            || url.starts_with("https://claude.com/cai/oauth/authorize?")
        {
            if url.len() > 16384 || url.chars().any(char::is_control) || url.contains('\\') {
                return Err(failure());
            }
            return Ok(Some(url.to_owned()));
        }
    }
    Ok(None)
}

fn command(
    profile: &ClaudeProfileBinding,
    executable: &PinnedExecutable,
    cancel: &AtomicBool,
    check: bool,
    publish: &impl Fn(&str, Option<String>, Option<&str>),
) -> Result<(Vec<u8>, i32), SidecarError> {
    command_with_code(
        profile,
        executable,
        cancel,
        check,
        publish,
        &std::sync::Mutex::new(None),
    )
}

fn command_with_code(
    profile: &ClaudeProfileBinding,
    executable: &PinnedExecutable,
    cancel: &AtomicBool,
    check: bool,
    publish: &impl Fn(&str, Option<String>, Option<&str>),
    code: &std::sync::Mutex<Option<String>>,
) -> Result<(Vec<u8>, i32), SidecarError> {
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    let home = profile.revalidate()?;
    // macOS resolves the default Keychain through the OS user's HOME. Keep
    // credentials/settings isolated with CLAUDE_CONFIG_DIR, not a fake OS home.
    let user_home = PathBuf::from(std::env::var_os("HOME").ok_or_else(failure)?);
    if !user_home.is_absolute() || user_home == home {
        return Err(failure());
    }
    let mut process = LoginProcess::reserve(profile)?;
    let result = (|| {
        process.launch_with_barrier(
            || {
                let path = executable
                    .revalidate()
                    .map_err(|_| std::io::Error::other("executable changed"))?;
                let args = if check {
                    ["auth", "status", "--json"]
                } else {
                    ["auth", "login", "--claudeai"]
                };
                let mut cmd =
                    provider_login_gate::command(path, &args.map(std::ffi::OsStr::new), &home)?;
                cmd.env("HOME", &user_home)
                    .env("CLAUDE_CONFIG_DIR", &home)
                    .env("TMPDIR", home.join("tmp"))
                    .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                    .env("BROWSER", "/usr/bin/false")
                    .env("DISABLE_AUTOUPDATER", "1")
                    .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
                if !check {
                    // Claude's auth command writes the browser prompt to stderr.
                    // Capture it in the same bounded, non-persisted pipe. Only the
                    // async-signal-safe descriptor syscall runs between fork/exec.
                    unsafe {
                        cmd.pre_exec(|| {
                            if libc::dup2(libc::STDOUT_FILENO, libc::STDERR_FILENO) < 0 {
                                Err(std::io::Error::last_os_error())
                            } else {
                                Ok(())
                            }
                        });
                    }
                }
                cmd.spawn()
            },
            |child| {
                executable
                    .revalidate()
                    .map_err(|_| std::io::Error::other("executable changed"))?;
                if cancel.load(Ordering::Acquire) {
                    return Err(std::io::Error::other("canceled"));
                }
                provider_login_gate::release(child)
            },
        )?;
        let mut deadline = Instant::now() + Duration::from_secs(if check { 30 } else { 300 });
        let mut bytes = Vec::new();
        let mut handed_off = false;
        let mut eof = false;
        let mut submitted = false;
        loop {
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(failure());
            }
            if !check && handed_off && !submitted {
                let input = code.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(input) = input {
                    process.submit_claude_code(&input, cancel)?;
                    submitted = true;
                    deadline = Instant::now() + Duration::from_secs(30);
                }
            }
            if !eof {
                match process.poll_output(cancel)? {
                    OutputChunk::Pending => (),
                    OutputChunk::Eof => eof = true,
                    OutputChunk::Bytes(chunk) => {
                        if bytes.len() + chunk.len() > 128 * 1024 {
                            return Err(failure());
                        }
                        bytes.extend_from_slice(&chunk);
                        if !check && !handed_off {
                            if let Some(url) = browser_url(&bytes)? {
                                if cancel.load(Ordering::Acquire) {
                                    return Err(failure());
                                }
                                publish("waiting", Some(url), None);
                                handed_off = true;
                            }
                        }
                    }
                }
            }
            if eof {
                if let Some(exit) = process.exit_status()? {
                    return Ok((bytes, exit.code().ok_or_else(failure)?));
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    // Hold the owner through all failure/cancel paths; only cleanup can release
    // its own provider fence. A later observation cannot erase uncertainty.
    if let Err(error) = process.finish() {
        super::provider_login_manager::retry_cleanup_ticks(|| process.finish());
        return Err(error);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn installed_version_names_never_authorize_unsigned_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert_eq!(
            select_executable(&root.join("missing"), &root.join("launcher"))
                .err()
                .unwrap()
                .kind(),
            "providerExecutableMissing"
        );
        assert!(select_executable(&root, &root.join("launcher")).is_err());
        for version in ["2.1.275", "2.1.270", "99.0.0"] {
            std::fs::write(root.join(version), b"untrusted executable").unwrap();
            std::fs::set_permissions(root.join(version), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        std::os::unix::fs::symlink(root.join("99.0.0"), root.join("launcher")).unwrap();
        assert_eq!(
            select_executable(&root, &root.join("launcher"))
                .err()
                .unwrap()
                .kind(),
            "providerExecutableChanged"
        );
    }

    #[test]
    fn native_launcher_target_is_confined_to_one_version() {
        let root = std::path::Path::new("/home/test/.local/share/claude/versions");
        for target in [
            root.join("2.1.285"),
            PathBuf::from("../share/claude/versions/99.12.345"),
        ] {
            assert!(selected_version_path(root, &target).is_ok());
        }
        for target in [
            "/outside/2.1.285",
            "../share/claude/versions/../2.1.285",
            "../share/claude/versions/2.1.285/nested",
            "../share/claude/versions/01.2.3",
            "../share/claude/versions/2.1.x",
            "../share/claude/versions/2.1.285-beta",
        ] {
            assert!(selected_version_path(root, std::path::Path::new(target)).is_err());
        }
    }

    #[test]
    #[ignore = "read-only installed Claude signature verification; does not execute provider"]
    fn installed_claude_launcher_selects_authenticated_current_version() {
        diagnose().unwrap();
        assert!(
            PinnedExecutable::open_claude_signed(std::path::Path::new("/usr/bin/true")).is_err()
        );
    }
    #[test]
    fn os_home_and_app_credential_namespace_are_separate() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let profile = ClaudeProfileBinding::from_storage(&storage).unwrap();
        let path = root.join("fake-claude-env");
        let script = b"#!/bin/sh\n[ \"$HOME\" != \"$CLAUDE_CONFIG_DIR\" ] || exit 9\n[ \"$PWD\" = \"$CLAUDE_CONFIG_DIR\" ] || exit 10\n[ \"$TMPDIR\" = \"$CLAUDE_CONFIG_DIR/tmp\" ] || exit 11\nprintf '%s' \"$HOME\"\n";
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = PinnedExecutable::open(&path, Sha256::digest(script).into()).unwrap();
        let (bytes, exit) = command(
            &profile,
            &executable,
            &AtomicBool::new(false),
            true,
            &|_, _, _| panic!("no browser"),
        )
        .unwrap();
        assert_eq!(exit, 0);
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            std::env::var("HOME").unwrap()
        );
        assert!(!root.join("claude-login.pending").exists());
    }
    #[test]
    fn code_reaches_owned_stdin_without_echo_or_persistence_and_process_exits() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let profile = ClaudeProfileBinding::from_storage(&storage).unwrap();
        let path = root.join("fake-claude-code");
        let script = b"#!/bin/sh\nprintf 'Open: https://claude.com/cai/oauth/authorize?state=fake\\n' >&2\nIFS= read -r code\n[ \"$code\" = 'synthetic#state' ] || exit 3\nprintf 'Login complete\\n'\nexit 0\n";
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = PinnedExecutable::open(&path, Sha256::digest(script).into()).unwrap();
        let code = std::sync::Mutex::new(None);
        let (output, exit) = command_with_code(
            &profile,
            &executable,
            &AtomicBool::new(false),
            false,
            &|_, _, _| {
                *code.lock().unwrap() = Some("synthetic#state".into());
            },
            &code,
        )
        .unwrap();
        assert_eq!(exit, 0);
        assert!(code.lock().unwrap().is_none());
        assert!(!String::from_utf8(output)
            .unwrap()
            .contains("synthetic#state"));
        assert!(!root.join("claude-login.pending").exists());
    }
    #[test]
    fn fixed_commands_capture_stderr_and_cleanup_before_account_observation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let profile = ClaudeProfileBinding::from_storage(&storage).unwrap();
        let path = root.join("fake-claude");
        let script = b"#!/bin/sh\nif [ \"$2\" = status ]; then printf '{\"loggedIn\":false,\"authMethod\":\"none\",\"apiProvider\":\"firstParty\",\"configDirectory\":\"%s\"}\\n' \"$CLAUDE_CONFIG_DIR\"; exit 1; fi\nprintf 'Open: https://claude.com/cai/oauth/authorize?state=synthetic\\n' >&2\nIFS= read -r hold\nexec /bin/sleep 30\n";
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = PinnedExecutable::open(&path, Sha256::digest(script).into()).unwrap();
        let cancel = AtomicBool::new(false);
        let (bytes, exit) = command(&profile, &executable, &cancel, true, &|_, _, _| {
            panic!("check cannot open browser")
        })
        .unwrap();
        assert!(!root.join("claude-login.pending").exists());
        assert!(!account(&bytes, exit, &profile.revalidate().unwrap()).unwrap());
        let result = command(&profile, &executable, &cancel, false, &|state, url, _| {
            assert_eq!(state, "waiting");
            assert!(url
                .unwrap()
                .starts_with("https://claude.com/cai/oauth/authorize?"));
            cancel.store(true, Ordering::Release);
        });
        assert!(result.is_err());
        assert!(!root.join("claude-login.pending").exists());
    }
    #[test]
    fn url_must_be_complete_and_official_and_auth_must_match_namespace() {
        assert!(browser_url(b"https://claude.ai/oauth/authorize?state=x")
            .unwrap()
            .is_none());
        assert_eq!(
            browser_url(b"Open:\nhttps://claude.ai/oauth/authorize?state=x\n")
                .unwrap()
                .unwrap(),
            "https://claude.ai/oauth/authorize?state=x"
        );
        assert!(
            browser_url(b"https://claude.ai.evil.test/oauth/authorize?a=b\n")
                .unwrap()
                .is_none()
        );
        let home = std::path::Path::new("/app/claude");
        for (logged, exit, expected) in [(true, 0, true), (false, 1, false)] {
            let mut value = serde_json::json!({"loggedIn":logged,"authMethod":"claude.ai","apiProvider":"firstParty","configDirectory":"/app/claude"});
            assert_eq!(
                account(value.to_string().as_bytes(), exit, home).unwrap(),
                expected
            );
            value["configDirectory"] = "/other".into();
            assert!(account(value.to_string().as_bytes(), exit, home).is_err());
        }
    }
}
