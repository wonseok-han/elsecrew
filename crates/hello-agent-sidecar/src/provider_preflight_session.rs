//! Internal discovery and text-turn candidates through the gated process owner.
use crate::{provider_login_process::LoginProcess, SidecarError};
use hello_agent_core::provider::vendor_observation::codex_preflight::exchange::{
    PreflightExchange, PreflightState,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(test)]
thread_local! {
    static BEFORE_RELEASE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

fn failure() -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        "provider settings could not be verified",
    )
}

/// Internal discovery entry point for a separately admitted executable pair.
/// Does not create a thread, send user input, or grant runtime execution authority.
pub(crate) struct ProviderInspection<'a> {
    pub(crate) result: Result<(), SidecarError>,
    pub(crate) process: LoginProcess<'a>,
}

/// Full candidate includes account/read and model/list. This is NOT the approved
/// config-only smoke and must not be invoked under that narrower authorization.
/// No product caller exists; configuration isolation/admission remains separate.
pub(crate) fn inspect_full_preflight_candidate<'a>(
    profile: &'a crate::provider_profile::CodexProfileBinding,
    executable: &crate::provider_executable::PinnedExecutable,
    helper: &crate::provider_executable::PinnedExecutable,
    expected: &hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    cancel: &AtomicBool,
) -> Result<ProviderInspection<'a>, SidecarError> {
    let home = profile.revalidate()?;
    let mut flow = PreflightExchange::new(expected, home.to_str().ok_or_else(failure)?)
        .map_err(|_| failure())?;
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    // Reject already stale pins before creating a durable process reservation.
    // The spawn/release checks below still catch subsequent changes.
    executable.revalidate()?;
    helper.revalidate()?;
    let mut process = LoginProcess::reserve(profile)?;
    let result = process
        .launch_with_barrier(
            || {
                let path = executable.revalidate().map_err(std::io::Error::other)?;
                helper.revalidate().map_err(std::io::Error::other)?;
                discovery_command(path, &home, expected)?.spawn()
            },
            |child| {
                #[cfg(test)]
                BEFORE_RELEASE.with(|hook| {
                    if let Some(hook) = hook.borrow_mut().take() {
                        hook();
                    }
                });
                executable.revalidate().map_err(std::io::Error::other)?;
                helper.revalidate().map_err(std::io::Error::other)?;
                let current = profile
                    .revalidate()
                    .map_err(|_| std::io::Error::other("profile changed"))?;
                if current != home || cancel.load(Ordering::Acquire) {
                    return Err(std::io::Error::other(
                        "preflight canceled or profile changed",
                    ));
                }
                crate::provider_login_gate::release(child)
            },
        )
        .and_then(|()| observe_and_close(&mut process, &mut flow, cancel));
    // Even a launch/cleanup error returns its owner for explicit cleanup retries.
    Ok(ProviderInspection { result, process })
}

/// Invocation-only overrides: never rewrite the user's app-owned login config.
fn discovery_command(
    executable: &std::path::Path,
    home: &std::path::Path,
    expected: &hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
) -> std::io::Result<std::process::Command> {
    PreflightExchange::new(
        expected,
        home.to_str()
            .ok_or_else(|| std::io::Error::other("invalid home"))?,
    )
    .map_err(|_| std::io::Error::other("invalid model profile"))?;
    let model = format!("model=\"{}\"", expected.model);
    let effort = format!("model_reasoning_effort=\"{}\"", expected.reasoning_effort);
    let args: Vec<&std::ffi::OsStr> = [
        "app-server",
        "--listen",
        "stdio://",
        "-c",
        &model,
        "-c",
        &effort,
        "-c",
        "model_provider=\"openai\"",
        "-c",
        "approval_policy=\"on-request\"",
        "-c",
        "sandbox_mode=\"read-only\"",
        "-c",
        "features.hooks=false",
    ]
    .into_iter()
    .map(std::ffi::OsStr::new)
    .collect();
    let mut command = crate::provider_login_gate::command(executable, &args, home)?;
    command
        .env("HOME", home)
        .env("CODEX_HOME", home)
        .env("TMPDIR", home.join("tmp"))
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    Ok(command)
}

/// The caller retains ownership on error so uncertain cleanup can be retried.
/// Ok means observations matched AND cleanup was confirmed, never launch readiness.
/// The 30-second budget bounds protocol I/O; observed cleanup has its own deadline.
pub(crate) fn observe_and_close(
    process: &mut LoginProcess<'_>,
    flow: &mut PreflightExchange,
    cancel: &AtomicBool,
) -> Result<(), SidecarError> {
    observe_with_budget(process, flow, cancel, Duration::from_secs(30), None)
}

/// Diagnostic only. Caller must verify managed cwd and integration isolation before
/// launch; creating a thread can initialize integrations even without a model turn.
pub(crate) fn observe_thread_and_close(
    process: &mut LoginProcess<'_>,
    flow: &mut PreflightExchange,
    cancel: &AtomicBool,
    cwd: &std::path::Path,
) -> Result<(), SidecarError> {
    observe_with_budget(process, flow, cancel, Duration::from_secs(30), Some(cwd))
}

fn observe_with_budget(
    process: &mut LoginProcess<'_>,
    flow: &mut PreflightExchange,
    cancel: &AtomicBool,
    budget: Duration,
    thread_cwd: Option<&std::path::Path>,
) -> Result<(), SidecarError> {
    let started = Instant::now();
    // A rejected claim never acquires cleanup authority over another session.
    if !flow.is_fresh() || process.begin_preflight().is_err() {
        flow.invalidate();
        flow.close();
        return Err(failure());
    }
    let result = check_claimed(
        process,
        flow,
        cancel,
        budget,
        started,
        thread_cwd,
        &mut || Ok(()),
    );
    // Even a protocol error or cancellation must run the existing observed cleanup.
    let cleanup = process.finish();
    if result.is_err() || cleanup.is_err() || cancel.load(Ordering::Acquire) {
        flow.invalidate();
    }
    flow.close();
    cleanup?;
    result?;
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    Ok(())
}

fn check_claimed(
    process: &mut LoginProcess<'_>,
    flow: &mut PreflightExchange,
    cancel: &AtomicBool,
    budget: Duration,
    started: Instant,
    thread_cwd: Option<&std::path::Path>,
    verify_workspace: &mut impl FnMut() -> Result<(), SidecarError>,
) -> Result<(), SidecarError> {
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        let frames = flow.next_frames().map_err(|_| failure())?;
        if frames.is_empty() {
            return Err(failure());
        }
        for frame in frames {
            let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
            process.send_preflight(frame, remaining, cancel)?;
        }
        let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
        let reply = process.receive_preflight(remaining, cancel)?;
        if started.elapsed() >= budget {
            return Err(failure());
        }
        if flow.accept(&reply).map_err(|_| failure())? == PreflightState::ObservationsMatched {
            if let Some(cwd) = thread_cwd {
                verify_workspace()?;
                let request = flow.start_read_only_thread(cwd).map_err(|_| failure())?;
                let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
                process.send_preflight(request, remaining, cancel)?;
                // Exactly one reply and one thread/started, in either order.
                for _ in 0..2 {
                    let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
                    let response = process.receive_preflight(remaining, cancel)?;
                    if started.elapsed() >= budget {
                        return Err(failure());
                    }
                    flow.accept(&response).map_err(|_| failure())?;
                }
                if flow.state() != PreflightState::ThreadMatched {
                    return Err(failure());
                }
            }
            return Ok(());
        }
    }
}

/// A durable chat/snapshot binding, not a run reservation or OS sandbox grant.
/// Revalidated at each input boundary; callers must still own the run lock and
/// enforce launch-time isolation. No arbitrary cwd can enter the text candidate.
pub(crate) struct TextWorkspace<'a> {
    projects: &'a crate::project::ProjectManager,
    storage: &'a crate::Storage,
    address: crate::project::WorkspaceAddress,
    cwd: std::path::PathBuf,
}

impl<'a> TextWorkspace<'a> {
    pub(crate) fn new(
        projects: &'a crate::project::ProjectManager,
        storage: &'a crate::Storage,
        address: &crate::project::WorkspaceAddress,
    ) -> Result<Self, SidecarError> {
        let manifest = projects
            .verify_workspace(storage, address)
            .map_err(|_| failure())?;
        let cwd = std::path::PathBuf::from(manifest["worktreePath"].as_str().ok_or_else(failure)?);
        PreflightExchange::validate_thread_cwd(&cwd).map_err(|_| failure())?;
        Ok(Self {
            projects,
            storage,
            address: address.clone(),
            cwd,
        })
    }

    pub(crate) fn revalidate(&self) -> Result<(), SidecarError> {
        let manifest = self
            .projects
            .verify_workspace(self.storage, &self.address)
            .map_err(|_| failure())?;
        if manifest["worktreePath"].as_str() != self.cwd.to_str() {
            return Err(failure());
        }
        Ok(())
    }
}

/// Internal candidate, no product caller. Caller must persist run ownership and
/// enforce isolation before use. Public callbacks are provisional until the
/// returned outcome AND observed cleanup succeed; they are not terminal events.
pub(crate) fn run_text_and_close(
    process: &mut LoginProcess<'_>,
    flow: PreflightExchange,
    cancel: &AtomicBool,
    workspace: &TextWorkspace<'_>,
    text: &str,
    on_public: &mut impl FnMut(
        hello_agent_core::provider::vendor_observation::codex_events::Projection,
    ) -> Result<(), SidecarError>,
) -> Result<
    hello_agent_core::provider::vendor_observation::codex_events::stream::TurnOutcome,
    SidecarError,
> {
    run_text_checked(
        process,
        flow,
        cancel,
        &workspace.cwd,
        text,
        on_public,
        &mut || workspace.revalidate(),
    )
}

fn run_text_checked(
    process: &mut LoginProcess<'_>,
    mut flow: PreflightExchange,
    cancel: &AtomicBool,
    cwd: &std::path::Path,
    text: &str,
    on_public: &mut impl FnMut(
        hello_agent_core::provider::vendor_observation::codex_events::Projection,
    ) -> Result<(), SidecarError>,
    verify_workspace: &mut impl FnMut() -> Result<(), SidecarError>,
) -> Result<
    hello_agent_core::provider::vendor_observation::codex_events::stream::TurnOutcome,
    SidecarError,
> {
    use hello_agent_core::provider::vendor_observation::codex_events::stream::Observed;
    hello_agent_core::provider::vendor_observation::codex_turn::TurnStart::validate_text(text)
        .map_err(|_| failure())?;
    PreflightExchange::validate_thread_cwd(cwd).map_err(|_| failure())?;
    verify_workspace()?;
    let started = Instant::now();
    let budget = Duration::from_secs(30);
    if !flow.is_fresh() || process.begin_preflight().is_err() {
        return Err(failure());
    }
    let result = (|| {
        check_claimed(
            process,
            &mut flow,
            cancel,
            budget,
            started,
            Some(cwd),
            verify_workspace,
        )?;
        verify_workspace()?;
        let mut turn = flow.into_text_turn(text).map_err(|_| failure())?;
        let remaining = || budget.checked_sub(started.elapsed()).ok_or_else(failure);
        process.send_turn(
            turn.take_frame().map_err(|_| failure())?,
            remaining()?,
            cancel,
        )?;
        let mut stream = None;
        for _ in 0..2 {
            let response = process.receive_preflight(remaining()?, cancel)?;
            if let Some(value) = turn.accept(&response).map_err(|_| failure())? {
                stream = Some(value);
            }
        }
        let mut stream = stream.ok_or_else(failure)?;
        loop {
            let frame = process.receive_preflight(remaining()?, cancel)?;
            let observed = stream.observe(&frame).map_err(|_| failure())?;
            if cancel.load(Ordering::Acquire) || started.elapsed() >= budget {
                return Err(failure());
            }
            match observed {
                Observed::Public(value) => on_public(value)?,
                Observed::ItemStarted | Observed::Private => {}
                Observed::Finished(_) => return stream.finish().map_err(|_| failure()),
                // Tools/private notifications need an explicit owner handler.
                Observed::Started | Observed::RequiresHandling => return Err(failure()),
            }
        }
    })();
    process.finish()?;
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_profile::CodexProfileBinding;
    use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
    use serde_json::json;
    use std::process::{Command, Stdio};

    #[test]
    fn invalid_turn_text_never_claims_or_initializes_the_live_owner() {
        for (text, cwd) in [
            ("", "/managed"),
            (" \n", "/managed"),
            ("bad\0text", "/managed"),
            (&"x".repeat(65537), "/managed"),
            ("goal", "relative"),
            ("goal", "/managed/../other"),
            ("goal", "/managed/\nother"),
            ("goal", "/managed"), // Well-formed input, rejected workspace admission.
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let marker = root.join("received-input");
            let flow = PreflightExchange::new(
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                home.to_str().unwrap(),
            )
            .unwrap();
            let mut process = LoginProcess::reserve(&profile).unwrap();
            process
                .launch(|| {
                    Command::new("/bin/sh")
                        .args([
                            "-c",
                            "IFS= read -r line && printf received > \"$1\"; IFS= read -r hold",
                            "invalid-input-peer",
                        ])
                        .arg(&marker)
                        .env_clear()
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                })
                .unwrap();
            let mut workspace_checks = 0;
            let result = run_text_checked(
                &mut process,
                flow,
                &AtomicBool::new(false),
                std::path::Path::new(cwd),
                text,
                &mut |_| panic!("unexpected output"),
                &mut || {
                    workspace_checks += 1;
                    Err(failure())
                },
            );
            assert!(result.is_err());
            assert_eq!(
                workspace_checks,
                usize::from(text == "goal" && cwd == "/managed")
            );
            assert!(process.exit_status().unwrap().is_none());
            assert!(LoginProcess::reserve(&profile).is_err());
            // Fresh claim proves the rejected call did not use or claim transport.
            process.begin_preflight().unwrap();
            process.finish().unwrap();
            assert!(!marker.exists());
            assert!(LoginProcess::reserve(&profile).is_ok());
        }
    }

    #[test]
    fn waiting_gate_blocks_changed_pair_and_late_cancel_without_losing_owner() {
        use crate::provider_executable::PinnedExecutable;
        use hello_agent_core::proctree::{ProbeResult, ProcTable, SystemProcTable};
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Arc;
        const SCRIPT: &str = "#!/bin/sh\nprintf ran > \"$HOME/vendor-ran\"\n";
        for case in ["cli", "helper", "cancel"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let cli_path = root.join("toy-cli");
            let helper_path = root.join("toy-helper");
            for path in [&cli_path, &helper_path] {
                std::fs::write(path, SCRIPT).unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let digest: [u8; 32] = Sha256::digest(SCRIPT.as_bytes()).into();
            let cli = PinnedExecutable::open(&cli_path, digest).unwrap();
            let helper = PinnedExecutable::open(&helper_path, digest).unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let trigger = cancel.clone();
            BEFORE_RELEASE.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || match case {
                    "cli" => std::fs::write(cli_path, "changed CLI").unwrap(),
                    "helper" => std::fs::write(helper_path, "changed helper").unwrap(),
                    _ => trigger.store(true, Ordering::Release),
                }))
            });
            let mut outcome = inspect_full_preflight_candidate(
                &profile,
                &cli,
                &helper,
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                &cancel,
            )
            .unwrap();
            assert!(outcome.result.is_err(), "{case}");
            BEFORE_RELEASE.with(|hook| assert!(hook.borrow().is_none(), "hook must execute"));
            let identity = outcome.process.root_for_test();
            assert!(
                matches!(SystemProcTable.probe(identity), ProbeResult::Alive(_)),
                "gated root retained: {case}"
            );
            assert!(
                LoginProcess::reserve(&profile).is_err(),
                "reservation retained: {case}"
            );
            assert!(!home.join("vendor-ran").exists());
            outcome.process.finish().unwrap();
            assert!(matches!(SystemProcTable.probe(identity), ProbeResult::Gone));
            assert!(
                !home.join("vendor-ran").exists(),
                "vendor must never execute: {case}"
            );
            drop(outcome);
            LoginProcess::reserve(&profile).unwrap().finish().unwrap();
        }
    }

    #[test]
    fn pinned_discovery_runs_end_to_end_and_rejects_stale_pins_before_reservation() {
        use crate::provider_executable::PinnedExecutable;
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        const PEER: &str = r#"#!/bin/sh
test "$1" = app-server || exit 2
test "$2" = --listen || exit 2
test "$3" = stdio:// || exit 2
test "$HOME" = "$CODEX_HOME" || exit 2
IFS= read -r request
printf '{"id":1,"result":{"userAgent":"toy","codexHome":"%s","platformFamily":"unix","platformOs":"macos"}}\n' "$HOME"
IFS= read -r request
IFS= read -r request
printf '{"id":2,"result":{"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only","features":{"hooks":false}},"layers":[{"name":{"type":"sessionFlags"},"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only","features":{"hooks":false}}},{"name":{"type":"user","file":"%s/config.toml"},"config":{"cli_auth_credentials_store":"file"}},{"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}]}}\n' "$HOME"
IFS= read -r request
printf '%s\n' '{"id":3,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}'
IFS= read -r request
printf '%s\n' '{"id":4,"result":{"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}}'
IFS= read -r hold
"#;
        for case in [
            "success",
            "canceled",
            "changed-cli",
            "changed-helper",
            "eof",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let original = std::fs::read(home.join("config.toml")).unwrap();
            let script = if case == "eof" {
                "#!/bin/sh\nexit 0\n"
            } else {
                PEER
            };
            let cli_path = root.join("toy-cli");
            let helper_path = root.join("toy-helper");
            for (path, source) in [(&cli_path, script), (&helper_path, "#!/bin/sh\nexit 0\n")] {
                std::fs::write(path, source).unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let cli = PinnedExecutable::open(&cli_path, Sha256::digest(script.as_bytes()).into())
                .unwrap();
            let helper =
                PinnedExecutable::open(&helper_path, Sha256::digest(b"#!/bin/sh\nexit 0\n").into())
                    .unwrap();
            if case == "changed-cli" {
                std::fs::write(&cli_path, "changed").unwrap();
            }
            if case == "changed-helper" {
                std::fs::write(&helper_path, "changed").unwrap();
            }
            let cancel = AtomicBool::new(case == "canceled");
            let outcome = inspect_full_preflight_candidate(
                &profile,
                &cli,
                &helper,
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                &cancel,
            );
            if case == "canceled" || case.starts_with("changed-") {
                assert!(outcome.is_err());
                LoginProcess::reserve(&profile).unwrap().finish().unwrap();
            } else {
                let outcome = outcome.unwrap();
                assert_eq!(outcome.result.is_ok(), case == "success", "{case}");
                drop(outcome);
                LoginProcess::reserve(&profile).unwrap().finish().unwrap();
            }
            assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), original);
            assert!(!home.join("auth.json").exists());
        }
    }

    #[test]
    fn discovery_uses_invocation_only_settings_and_rejects_injected_values() {
        let profile = ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
        };
        let command = discovery_command(
            std::path::Path::new("/verified/codex"),
            std::path::Path::new("/private/profile"),
            &profile,
        )
        .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            &args[4..],
            &[
                "app-server",
                "--listen",
                "stdio://",
                "-c",
                "model=\"test-model\"",
                "-c",
                "model_reasoning_effort=\"medium\"",
                "-c",
                "model_provider=\"openai\"",
                "-c",
                "approval_policy=\"on-request\"",
                "-c",
                "sandbox_mode=\"read-only\"",
                "-c",
                "features.hooks=false"
            ]
        );
        assert_eq!(
            command.get_current_dir(),
            Some(std::path::Path::new("/private/profile"))
        );
        let env: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_str().unwrap(), value.unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(env.len(), 4);
        assert_eq!(env["HOME"], "/private/profile");
        assert_eq!(env["CODEX_HOME"], "/private/profile");
        assert_eq!(env["TMPDIR"], "/private/profile/tmp");
        for model in ["", "x\"\nmalicious=true", "x y", "$(touch x)"] {
            assert!(discovery_command(
                std::path::Path::new("/verified/codex"),
                std::path::Path::new("/private/profile"),
                &ExpectedProfile {
                    model,
                    reasoning_effort: "medium"
                }
            )
            .is_err());
        }
    }

    #[test]
    fn real_pipes_match_cleanup_and_fail_closed_on_wrong_config_cancel_or_uncertainty() {
        for case in [
            "success",
            "config",
            "cancel",
            "uncertain",
            "timeout",
            "reused",
            "used-process",
            "polled-process",
            "claimed-process",
            "thread",
            "thread-before",
            "wrong-thread",
            "turn-success",
            "turn-wrong-id",
            "turn-eof",
            "turn-cancel",
            "turn-sink-error",
            "turn-uncertain",
            "turn-tool",
            "turn-workspace-before-thread",
            "turn-workspace-before-prompt",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let storage = crate::Storage::open(
                &temp
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("hello-agent.sqlite3"),
            )
            .unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let mut flow = PreflightExchange::new(
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                home.to_str().unwrap(),
            )
            .unwrap();
            let mut process = LoginProcess::reserve(&profile).unwrap();
            let replies = [
                json!({"id":1,"result":{"userAgent":"test","codexHome":home,"platformFamily":"unix","platformOs":"macos"}}),
                json!({"id":2,"result":{"config":{"model":if case == "config" {"wrong"} else {"test-model"},"model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only"}}}),
                json!({"id":3,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}),
                json!({"id":4,"result":{"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}}),
                json!({"id":5,"result":{"thread":{"id":"thread-test"},"model":"test-model","modelProvider":"openai","cwd":if case == "wrong-thread" {"/wrong"} else {"/managed/snapshot"},"instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"}}),
            ].map(|mut value| {
                if value["id"]==2 {
                    value["result"]["config"]["features"]=json!({"hooks":false});
                    value["result"]["layers"]=json!([
                        {"name":{"type":"sessionFlags"},"config":value["result"]["config"]},
                        {"name":{"type":"user","file":home.join("config.toml")},"config":{"cli_auth_credentials_store":"file"}},
                        {"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}
                    ]);
                }
                value.to_string()
            });
            let script = r#"
IFS= read -r line; case "$line" in *'"method":"initialize"'*) ;; *) exit 2;; esac
printf '%s\n' "$1"
IFS= read -r line; test "$line" = '{"method":"initialized","params":{}}' || exit 3
IFS= read -r line; case "$line" in *'"method":"config/read"'*) ;; *) exit 4;; esac
printf '%s\n' "$2"
IFS= read -r line; case "$line" in *'"method":"account/read"'*) ;; *) exit 5;; esac
printf '%s\n' "$3"
IFS= read -r line; case "$line" in *'"method":"model/list"'*) ;; *) exit 6;; esac
printf '%s\n' "$4"
IFS= read -r line || exit 0
case "$line" in *'"method":"thread/start"'*) ;; *) exit 7;; esac
printf received > "$7"
if test "$6" = thread-before; then
  printf '%s\n' '{"method":"thread/started","params":{"thread":{"id":"thread-test"}}}'
  printf '%s\n' "$5"
else
  printf '%s\n' "$5"
  printf '%s\n' '{"method":"thread/started","params":{"thread":{"id":"thread-test"}}}'
fi
case "$6" in turn-*)
IFS= read -r line; case "$line" in *'"method":"turn/start"'*) ;; *) exit 8;; esac
printf received > "$8"
test "$6" != turn-eof || exit 0
if test "$6" = turn-wrong-id; then
printf '%s\n' '{"id":99,"result":{"turn":{"id":"t1","status":"inProgress","items":[],"error":null}}}'
else
printf '%s\n' '{"id":6,"result":{"turn":{"id":"t1","status":"inProgress","items":[],"error":null}}}'
fi
printf '%s\n' '{"method":"turn/started","params":{"threadId":"thread-test","turn":{"id":"t1","status":"inProgress","items":[],"error":null}}}'
printf '%s\n' '{"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"userMessage","id":"u1","clientId":null,"content":[{"type":"text","text":"PRIVATE","text_elements":[]}]}}}'
printf '%s\n' '{"method":"item/completed","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"userMessage","id":"u1","clientId":null,"content":[{"type":"text","text":"PRIVATE","text_elements":[]}]}}}'
printf '%s\n' '{"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"reasoning","id":"r1","summary":[],"content":[]}}}'
printf '%s\n' '{"method":"item/reasoning/textDelta","params":{"threadId":"thread-test","turnId":"t1","itemId":"r1","delta":"PRIVATE","contentIndex":0}}'
printf '%s\n' '{"method":"item/completed","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"reasoning","id":"r1","summary":[],"content":["PRIVATE"]}}}'
if test "$6" = turn-tool; then
printf '%s\n' '{"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"commandExecution","id":"c1","command":"PRIVATE"}}}'
fi
printf '%s\n' '{"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"agentMessage","id":"a1","text":""}}}'
printf '%s\n' '{"method":"item/agentMessage/delta","params":{"threadId":"thread-test","turnId":"t1","itemId":"a1","delta":"hello"}}'
printf '%s\n' '{"method":"item/completed","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"agentMessage","id":"a1","text":"hello"}}}'
printf '%s\n' '{"method":"turn/completed","params":{"threadId":"thread-test","turn":{"id":"t1","status":"completed","items":[],"error":null}}}'
;; esac
IFS= read -r hold
"#;
            process
                .launch(|| {
                    Command::new("/bin/sh")
                        .args(["-c", script, "preflight-peer"])
                        .args(&replies)
                        .arg(case)
                        .arg(temp.path().join("thread-received"))
                        .arg(temp.path().join("prompt-received"))
                        .env_clear()
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                })
                .unwrap();
            if case == "uncertain" || case == "turn-uncertain" {
                process.inject_uncertain_cleanup_once();
            }
            let cancel = AtomicBool::new(case == "cancel");
            if case.starts_with("turn-") {
                use hello_agent_core::provider::vendor_observation::codex_events::stream::TurnOutcome;
                use hello_agent_core::provider::vendor_observation::codex_events::Projection;
                let mut text = String::new();
                let mut checks = 0;
                let result = run_text_checked(
                    &mut process,
                    flow,
                    &cancel,
                    std::path::Path::new("/managed/snapshot"),
                    "test goal",
                    &mut |value| {
                        match value {
                            Projection::Delta { text: value, .. } => {
                                assert_eq!(value, "hello");
                                text.push_str(&value);
                            }
                            Projection::Completed { text: value, .. } => {
                                assert_eq!(value, "hello");
                                text = value;
                            }
                            Projection::RequiresHandling => panic!("private projection"),
                        }
                        if case == "turn-cancel" {
                            cancel.store(true, Ordering::Release);
                        }
                        if case == "turn-sink-error" {
                            return Err(failure());
                        }
                        Ok(())
                    },
                    &mut || {
                        checks += 1;
                        if (case == "turn-workspace-before-thread" && checks == 2)
                            || (case == "turn-workspace-before-prompt" && checks == 3)
                        {
                            return Err(failure());
                        }
                        Ok(())
                    },
                );
                if case == "turn-success" {
                    assert_eq!(checks, 3);
                    assert_eq!(result.unwrap(), TurnOutcome::Completed);
                    assert_eq!(text, "hello");
                } else {
                    assert!(result.is_err(), "{case}");
                }
                if case.starts_with("turn-workspace-") {
                    assert!(!temp.path().join("prompt-received").exists());
                    assert_eq!(
                        temp.path().join("thread-received").exists(),
                        case == "turn-workspace-before-prompt"
                    );
                    assert!(text.is_empty());
                }
                if case == "turn-uncertain" {
                    let _ = process.finish();
                }
                drop(process);
                assert_eq!(
                    LoginProcess::reserve(&profile).is_ok(),
                    case != "turn-uncertain",
                    "{case}"
                );
                continue;
            }
            if case == "polled-process" {
                assert!(process.poll(&cancel).unwrap().is_none());
            }
            if case == "used-process" {
                let (_, request) = hello_agent_core::provider::vendor_observation::codex_login::connection::LoginConnection::begin(home.to_str().unwrap()).unwrap();
                process
                    .send(request, Duration::from_secs(1), &cancel)
                    .unwrap();
                // The prior initialize reply remains unread in this transport.
            }
            if case == "claimed-process" {
                process.begin_preflight().unwrap();
                assert!(process.receive(Duration::ZERO, &cancel).is_err());
                assert!(process.poll(&cancel).is_err());
                assert!(process.poll_output(&cancel).is_err());
                let (_, request) = hello_agent_core::provider::vendor_observation::codex_login::connection::LoginConnection::begin(home.to_str().unwrap()).unwrap();
                assert!(process
                    .send(request, Duration::from_secs(1), &cancel)
                    .is_err());
            }
            if case == "reused" {
                flow.next_request().unwrap();
            }
            let budget = if case == "timeout" {
                Duration::ZERO
            } else {
                Duration::from_secs(3)
            };
            let thread_cwd = if matches!(case, "thread" | "thread-before" | "wrong-thread") {
                Some(std::path::Path::new("/managed/snapshot"))
            } else {
                None
            };
            let result = observe_with_budget(&mut process, &mut flow, &cancel, budget, thread_cwd);
            assert_eq!(
                result.is_ok(),
                matches!(case, "success" | "thread" | "thread-before"),
                "{case}"
            );
            assert_eq!(
                flow.state(),
                if matches!(case, "success" | "thread" | "thread-before") {
                    PreflightState::Closed
                } else {
                    PreflightState::Failed
                }
            );
            assert!(flow.next_request().is_err());
            assert!(flow
                .start_read_only_thread(std::path::Path::new("/managed/snapshot"))
                .is_err());
            assert_eq!(flow.matched_thread_id(), None);
            if matches!(
                case,
                "reused" | "used-process" | "polled-process" | "claimed-process"
            ) {
                // Rejection must leave the original owner and its live child untouched.
                assert!(process.exit_status().unwrap().is_none(), "{case}");
                assert!(LoginProcess::reserve(&profile).is_err(), "{case}");
                process.finish().unwrap();
            }
            if case == "uncertain" {
                assert_eq!(result.unwrap_err().kind(), "providerLoginRecoveryRequired");
                // Retain and reap the process, but do not reinterpret the original result.
                let _ = process.finish();
            }
            drop(process);
            let reservation = LoginProcess::reserve(&profile);
            assert_eq!(reservation.is_ok(), case != "uncertain", "{case}");
        }
    }
}
