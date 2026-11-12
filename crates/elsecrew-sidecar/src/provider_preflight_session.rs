//! Internal discovery and text-turn candidates through the gated process owner.
use crate::{provider_login_process::LoginProcess, SidecarError};
use elsecrew_core::provider::vendor_observation::codex_preflight::exchange::{
    PreflightExchange, PreflightState,
};
use elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedPermission;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod incremental;
#[allow(dead_code)] // Product dispatcher activation follows complete discovery projection.
pub(crate) mod discovery_worker;
pub(crate) mod vendor_launch;
pub(crate) mod named_permissions;
#[allow(unused_imports)] // Candidate until named config/thread admission is connected.
pub(crate) use named_permissions::named_discovery_arguments;
pub(crate) use incremental::ConfinedAcceptReply;
pub(crate) use incremental::MatchedThreadReceipt;

#[cfg(test)]
thread_local! {
    pub(crate) static BEFORE_RELEASE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

fn failure() -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        "provider settings could not be verified",
    )
}

fn stage_failure(stage: &'static str) -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        format!("Codex preflight failed at {stage}"),
    )
}

fn remaining_preflight_budget(budget: Duration, elapsed: Duration, stage: &'static str) -> Result<Duration, SidecarError> {
    budget.checked_sub(elapsed).filter(|remaining| !remaining.is_zero()).ok_or_else(|| {
        SidecarError::new("providerPreflightFailed", format!("Codex preflight deadline reached at {stage}"))
    })
}

/// Internal discovery entry point for a separately admitted executable pair.
/// Does not create a thread, send user input, or grant runtime execution authority.
pub(crate) struct ProviderInspection<'a, T = ()> {
    pub(crate) result: Result<T, SidecarError>,
    pub(crate) display_usage: Option<serde_json::Value>,
    pub(crate) process: LoginProcess<'a>,
    pub(crate) display_models: Option<Vec<elsecrew_core::provider::vendor_observation::codex_preflight::CatalogModel>>,
}

/// Fixed, model-free version probe through the same tracked process owner.
/// Call on a discovery worker, not on the dispatch loop. A failed result still
/// returns its owner for explicit cleanup; no account/config query is sent.
#[allow(dead_code)] // Scoped discovery worker will combine version and profile evidence.
pub(crate) fn inspect_codex_version(
    profile: crate::provider_profile::CodexProfileBinding,
    executable: &crate::provider_executable::PinnedExecutable,
    cancel: &AtomicBool,
) -> Result<ProviderInspection<'static, String>, SidecarError> {
    use crate::provider_login_io::OutputChunk;
    use elsecrew_core::provider::{BoundedObservation, ProviderId, parse_cli_version};
    let home = profile.revalidate()?;
    if cancel.load(Ordering::Acquire) { return Err(failure()); }
    let args = codex_version_arguments(&home).map_err(|_| failure())?;
    let references: Vec<_> = args.iter().map(std::ffi::OsString::as_os_str).collect();
    let mut command = crate::provider_login_gate::command(
        executable.revalidate()?, &references, &home,
    ).map_err(|_| failure())?;
    discovery_environment(&mut command, &home);
    let mut process = LoginProcess::reserve_codex_owned(profile)?;
    let result = (|| {
        process.launch_with_barrier(|| command.spawn(), |child| {
            executable.revalidate().map_err(std::io::Error::other)?;
            if cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::other("version inspection canceled"));
            }
            crate::provider_login_gate::release(child)
        })?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut eof = false;
        loop {
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(stage_failure("version probe canceled or timed out"));
            }
            if !eof {
                match process.poll_output(cancel)? {
                    OutputChunk::Bytes(bytes) => {
                        if output.len() + bytes.len() > 1024 { return Err(stage_failure("version output budget")); }
                        output.extend(bytes);
                    }
                    OutputChunk::Eof => eof = true,
                    OutputChunk::Pending => {}
                }
            } else {
                process.tick()?;
            }
            if eof {
                if let Some(status) = process.exit_status()? {
                    if !status.success() { return Err(stage_failure("version process exit")); }
                    return parse_cli_version(ProviderId::Codex, &BoundedObservation::new(output)
                        .map_err(|_| stage_failure("version output observation"))?).map_err(|_| stage_failure("version output format"));
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let cleanup = process.finish();
    let result = result.and_then(|version| {
        cleanup?;
        executable.revalidate()?;
        if process.closed_codex_home()? != home || cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        Ok(version)
    });
    Ok(ProviderInspection { result, process, display_models: None, display_usage: None })
}

fn codex_version_arguments(home: &std::path::Path) -> std::io::Result<Vec<std::ffi::OsString>> {
    Ok(vec!["-c".into(), crate::provider_profile::codex_isolation_argument(home, home)?,
        "-c".into(), "features.hooks=false".into(), "-c".into(), "features.apps=false".into(),
        "-c".into(), "features.plugins=false".into(), "--version".into()])
}

/// A bounded, selected-profile observation, not a global provider capability.
/// Constructed only after the named exchange and confirmed cleanup succeed.
/// It contains no account response, credentials, or arbitrary vendor payload.
pub(crate) struct NamedProfileObservation {
    model: String,
    effort: String,
    permission: ExpectedPermission,
    workspace: std::path::PathBuf,
    home: std::path::PathBuf,
    observed_at: std::time::Instant,
}

impl NamedProfileObservation {
    pub(crate) fn selected_model(
        &self,
    ) -> Result<elsecrew_protocol::agent_adapter_v1::AgentModelCapabilityV1, SidecarError> {
        if self.observed_at.elapsed() > Duration::from_secs(30) {
            return Err(failure());
        }
        Ok(
            elsecrew_protocol::agent_adapter_v1::AgentModelCapabilityV1 {
                model_selector: self.model.clone(),
                reasoning_efforts: vec![self.effort.clone()],
                permissions: vec![match self.permission {
                    ExpectedPermission::ReadOnly => "readOnly",
                    ExpectedPermission::WorkspaceWrite => "workspaceWrite",
                }
                .into()],
            },
        )
    }

    /// Discovery is short-lived and bound to the exact inputs. A fresh start
    /// still repeats the same-connection preflight before sending user input.
    pub(crate) fn matches(
        &self,
        expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
        permission: ExpectedPermission,
        workspace: &std::path::Path,
        home: &std::path::Path,
    ) -> bool {
        self.model == expected.model
            && self.effort == expected.reasoning_effort
            && self.permission == permission
            && self.workspace == workspace
            && self.home == home
            && self.observed_at.elapsed() <= Duration::from_secs(30)
    }
}

#[cfg(test)]
mod named_observation_tests {
    use super::*;

    #[test]
    fn selected_observation_cannot_move_to_another_profile_or_workspace() {
        use elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
        let expected = ExpectedProfile { model: "test-model", reasoning_effort: "medium" };
        let workspace = std::path::Path::new("/managed/snapshot");
        let home = std::path::Path::new("/managed/profile");
        let mut observed = NamedProfileObservation {
            model: expected.model.into(), effort: expected.reasoning_effort.into(),
            permission: ExpectedPermission::ReadOnly, workspace: workspace.into(),
            home: home.into(), observed_at: Instant::now(),
        };
        assert!(observed.matches(&expected, ExpectedPermission::ReadOnly, workspace, home));
        assert!(!observed.matches(&expected, ExpectedPermission::WorkspaceWrite, workspace, home));
        assert!(!observed.matches(&expected, ExpectedPermission::ReadOnly, std::path::Path::new("/other"), home));
        assert!(!observed.matches(&expected, ExpectedPermission::ReadOnly, workspace, std::path::Path::new("/other")));
        let other = ExpectedProfile { model: "another-model", reasoning_effort: "medium" };
        assert!(!observed.matches(&other, ExpectedPermission::ReadOnly, workspace, home));
        let other = ExpectedProfile { model: "test-model", reasoning_effort: "high" };
        assert!(!observed.matches(&other, ExpectedPermission::ReadOnly, workspace, home));
        observed.observed_at = Instant::now() - Duration::from_secs(31);
        assert!(!observed.matches(&expected, ExpectedPermission::ReadOnly, workspace, home));
    }
}

/// Inspect the same named policy used by the native launcher. The workspace
/// must already be a verified managed snapshot. This queries account/models,
/// but never creates a thread or sends a goal; it is not a capability grant.
#[allow(dead_code)] // Product discovery integration remains gated.
pub(crate) fn inspect_named_preflight_candidate(
    profile: crate::provider_profile::CodexProfileBinding,
    executable: &crate::provider_executable::PinnedExecutable,
    helper: &crate::provider_executable::PinnedExecutable,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    permission: ExpectedPermission,
    workspace: &std::path::Path,
    cancel: &AtomicBool,
) -> Result<ProviderInspection<'static, NamedProfileObservation>, SidecarError> {
    inspect_named_preflight_display_candidate(profile, executable, helper, expected, permission, workspace, cancel, false)
}

pub(crate) fn inspect_named_preflight_display_candidate(
    profile: crate::provider_profile::CodexProfileBinding,
    executable: &crate::provider_executable::PinnedExecutable,
    helper: &crate::provider_executable::PinnedExecutable,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    permission: ExpectedPermission,
    workspace: &std::path::Path,
    cancel: &AtomicBool,
    include_usage: bool,
) -> Result<ProviderInspection<'static, NamedProfileObservation>, SidecarError> {
    let home = profile.revalidate()?;
    let mut flow = PreflightExchange::with_named_permission(
        expected,
        home.to_str().ok_or_else(failure)?,
        workspace,
        permission,
    )
    .map_err(|_| failure())?;
    flow.require_startup_isolation(workspace).map_err(|_| failure())?;
    if include_usage { flow.enable_usage_display().map_err(|_| failure())?; }
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    let command = vendor_launch::CodexCommand::new(
        executable.revalidate()?,
        helper.revalidate()?,
        &home,
        workspace,
        expected,
        permission,
    )?;
    let mut process = LoginProcess::reserve_codex_owned(profile)?;
    let launched = command.launch(&mut process, |child, _| {
        executable.revalidate().map_err(std::io::Error::other)?;
        helper.revalidate().map_err(std::io::Error::other)?;
        // launch_with_identity_barrier revalidates its owned profile before
        // this callback. No borrowed profile survives outside the request.
        if cancel.load(Ordering::Acquire) {
            return Err(std::io::Error::other(
                "preflight canceled or profile changed",
            ));
        }
        crate::provider_login_gate::release(child)
    });
    let result = launched.and_then(|()| {
        if !process.matches_named_scope(workspace, permission) {
            // A rejected scope still belongs to this inspection and must close.
            let _ = process.finish();
            return Err(stage_failure("named launch scope"));
        }
        observe_and_close(&mut process, &mut flow, cancel)?;
        // Do not retain an observation if executable/profile identity changed
        // while shutting down, even though the protocol itself matched.
        executable.revalidate()?;
        helper.revalidate()?;
        if process.closed_codex_home()? != home || cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        Ok(NamedProfileObservation {
            model: expected.model.into(),
            effort: expected.reasoning_effort.into(),
            permission,
            workspace: workspace.into(),
            home: home.clone(),
            observed_at: Instant::now(),
        })
    });
    // Display choices are not execution evidence. Even a rejected selection
    // can offer alternatives, but only after this exact owner closed and its
    // executable/profile binding survived cleanup. Never mask the original error.
    let display_models = if process.closure_evidence().is_some_and(|evidence| evidence.confirmed())
        && !cancel.load(Ordering::Acquire)
        && executable.revalidate().is_ok()
        && helper.revalidate().is_ok()
        && process.closed_codex_home().is_ok_and(|closed| closed == home)
    {
        flow.display_models().map(<[_]>::to_vec)
    } else {
        None
    };
    // Even launch/cleanup errors return the original owner for explicit cleanup.
    let display_usage = display_models.as_ref().and_then(|_| flow.display_usage());
    Ok(ProviderInspection { result, process, display_models, display_usage })
}

/// Full candidate includes account/read, model/list and configRequirements/read. This is NOT the approved
/// config-only smoke and must not be invoked under that narrower authorization.
/// No product caller exists; configuration isolation/admission remains separate.
pub(crate) fn inspect_full_preflight_candidate<'a>(
    profile: &'a crate::provider_profile::CodexProfileBinding,
    executable: &crate::provider_executable::PinnedExecutable,
    helper: &crate::provider_executable::PinnedExecutable,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    cancel: &AtomicBool,
) -> Result<ProviderInspection<'a>, SidecarError> {
    inspect_preflight_candidate(profile, executable, helper, expected, cancel, None)
}

/// Model-free inspection through the OS-isolated, owned proxy. This requires
/// account/model inspection authorization, not merely config-only approval.
/// ReadOnly limits workspace writes, not vendor auth/cache updates in the profile.
#[allow(dead_code)] // Diagnostic candidate; no product admission.
pub(crate) fn inspect_proxied_preflight_candidate<'a>(
    profile: &'a crate::provider_profile::CodexProfileBinding,
    executable: &crate::provider_executable::PinnedExecutable,
    helper: &crate::provider_executable::PinnedExecutable,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    cancel: &AtomicBool,
    workspace: &std::path::Path,
) -> Result<ProviderInspection<'a>, SidecarError> {
    inspect_preflight_candidate(
        profile,
        executable,
        helper,
        expected,
        cancel,
        Some(workspace),
    )
}

fn inspect_preflight_candidate<'a>(
    profile: &'a crate::provider_profile::CodexProfileBinding,
    executable: &crate::provider_executable::PinnedExecutable,
    helper: &crate::provider_executable::PinnedExecutable,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    cancel: &AtomicBool,
    workspace: Option<&std::path::Path>,
) -> Result<ProviderInspection<'a>, SidecarError> {
    let home = profile.revalidate()?;
    let mut flow = PreflightExchange::new(expected, home.to_str().ok_or_else(failure)?)
        .map_err(|_| failure())?;
    flow.require_startup_isolation(workspace.unwrap_or(&home)).map_err(|_| failure())?;
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    // Reject already stale pins before creating a durable process reservation.
    // The spawn/release checks below still catch subsequent changes.
    executable.revalidate()?;
    helper.revalidate()?;
    let mut process = LoginProcess::reserve(profile)?;
    let release = |child: &mut std::process::Child| {
        #[cfg(test)]
        BEFORE_RELEASE.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        executable.revalidate().map_err(std::io::Error::other)?;
        helper.revalidate().map_err(std::io::Error::other)?;
        let current = profile.revalidate().map_err(std::io::Error::other)?;
        if current != home || cancel.load(Ordering::Acquire) {
            return Err(std::io::Error::other(
                "preflight canceled or profile changed",
            ));
        }
        crate::provider_login_gate::release(child)
    };
    let launch = if let Some(workspace) = workspace {
        let command = (|| {
            let mut args = discovery_arguments(&home, expected, ExpectedPermission::ReadOnly)?;
            args.extend(["-c".into(), crate::provider_profile::codex_isolation_argument(&home, workspace)?]);
            crate::provider_runtime_sandbox::command_with_model_proxy(
                executable.revalidate().map_err(std::io::Error::other)?,
                helper.revalidate().map_err(std::io::Error::other)?,
                &args,
                &home,
                workspace,
                crate::provider_runtime_sandbox::WorkspaceAccess::ReadOnly,
                std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?,
            )
        })();
        command
            .map_err(|_| failure())
            .and_then(|command| command.launch(&mut process, release))
    } else {
        process.launch_with_barrier(
            || {
                let path = executable.revalidate().map_err(std::io::Error::other)?;
                helper.revalidate().map_err(std::io::Error::other)?;
                discovery_command(path, &home, expected)?.spawn()
            },
            release,
        )
    };
    let result = launch.and_then(|()| observe_and_close(&mut process, &mut flow, cancel));
    // Even a launch/cleanup error returns its owner for explicit cleanup retries.
    Ok(ProviderInspection { result, process, display_models: None, display_usage: None })
}

/// Invocation-only overrides: never rewrite the user's app-owned login config.
pub(crate) fn discovery_command(
    executable: &std::path::Path,
    home: &std::path::Path,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
) -> std::io::Result<std::process::Command> {
    let mut args = discovery_arguments(home, expected, ExpectedPermission::ReadOnly)?;
    args.extend(["-c".into(), crate::provider_profile::codex_isolation_argument(home, home)?]);
    let references: Vec<_> = args.iter().map(std::ffi::OsString::as_os_str).collect();
    let mut command = crate::provider_login_gate::command(executable, &references, home)?;
    discovery_environment(&mut command, home);
    Ok(command)
}

/// Offline OS-isolated candidate, not a networking or product activation path.
pub(crate) fn offline_discovery_command(
    executable: &std::path::Path,
    helper: &std::path::Path,
    home: &std::path::Path,
    workspace: &std::path::Path,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    permission: ExpectedPermission,
) -> std::io::Result<std::process::Command> {
    let mut args = discovery_arguments(home, expected, permission)?;
    args.extend(["-c".into(), crate::provider_profile::codex_isolation_argument(home, workspace)?]);
    let mut command = crate::provider_runtime_sandbox::command(
        executable,
        helper,
        &args,
        home,
        workspace,
        match permission {
            ExpectedPermission::ReadOnly => {
                crate::provider_runtime_sandbox::WorkspaceAccess::ReadOnly
            }
            ExpectedPermission::WorkspaceWrite => {
                crate::provider_runtime_sandbox::WorkspaceAccess::Write
            }
        },
    )?;
    discovery_environment(&mut command, home);
    Ok(command)
}

pub(crate) fn discovery_arguments(
    home: &std::path::Path,
    expected: &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile<'_>,
    permission: ExpectedPermission,
) -> std::io::Result<Vec<std::ffi::OsString>> {
    PreflightExchange::with_permission(
        expected,
        home.to_str()
            .ok_or_else(|| std::io::Error::other("invalid home"))?,
        permission,
    )
    .map_err(|_| std::io::Error::other("invalid model profile"))?;
    let model = format!("model=\"{}\"", expected.model);
    let effort = format!("model_reasoning_effort=\"{}\"", expected.reasoning_effort);
    let sandbox = format!("sandbox_mode=\"{}\"", permission.sandbox_mode());
    let mut args: Vec<std::ffi::OsString> = [
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
        &sandbox,
        "-c",
        "features.hooks=false",
        "-c",
        "features.apps=false",
        "-c",
        "features.plugins=false",
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();
    if permission == ExpectedPermission::WorkspaceWrite {
        for setting in [
            "sandbox_workspace_write.writable_roots=[]",
            "sandbox_workspace_write.network_access=false",
            "sandbox_workspace_write.exclude_tmpdir_env_var=true",
            "sandbox_workspace_write.exclude_slash_tmp=true",
        ] {
            args.extend(["-c".into(), setting.into()]);
        }
    }
    Ok(args)
}

fn discovery_environment(command: &mut std::process::Command, home: &std::path::Path) {
    command
        .env("HOME", home)
        .env("CODEX_HOME", home)
        .env("TMPDIR", home.join("tmp"))
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
}

/// The caller retains ownership on error so uncertain cleanup can be retried.
/// Ok means the selected observation scope matched AND cleanup was confirmed,
/// never launch readiness. A configuration-only flow never queries account/models.
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
        &mut |_, _| Ok(()),
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
    on_action: &mut impl FnMut(&mut LoginProcess<'_>, TextAction) -> Result<(), SidecarError>,
) -> Result<(), SidecarError> {
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        let awaiting = flow.awaiting_response();
        let frames = if awaiting {
            Vec::new()
        } else {
            flow.next_frames().map_err(|_| failure())?
        };
        if frames.is_empty() && !awaiting {
            return Err(failure());
        }
        for frame in frames {
            #[cfg(test)]
            {
                let id = serde_json::from_str::<serde_json::Value>(frame.as_str())
                    .ok()
                    .and_then(|value| value.get("id").and_then(serde_json::Value::as_u64));
                eprintln!("preflight sending_rpc_id={id:?}");
            }
            on_action(process, TextAction::ObserveProcess)?;
            let remaining = remaining_preflight_budget(budget, started.elapsed(), flow.diagnostic_stage())?;
            process.send_preflight_observed(frame, remaining, cancel, &mut |process| {
                on_action(process, TextAction::ObserveProcess)
            })?;
        }
        let remaining = remaining_preflight_budget(budget, started.elapsed(), flow.diagnostic_stage())?;
        let reply = process.receive_preflight_observed(remaining, cancel, &mut |process| {
            on_action(process, TextAction::ObserveProcess)
        })?;
        on_action(process, TextAction::ObserveProcess)?;
        remaining_preflight_budget(budget, started.elapsed(), flow.diagnostic_stage())?;
        #[cfg(test)]
        let previous = flow.state();
        let stage = flow.diagnostic_stage();
        let accepted = flow.accept(&reply);
        #[cfg(test)]
        eprintln!("preflight stage={previous:?} accepted={}", accepted.is_ok());
        let state = accepted.map_err(|_| {
            let detail = flow.diagnostic_stage();
            stage_failure(if detail == "exchange state" { stage } else { detail })
        })?;
        if state == PreflightState::ConfigurationMatched {
            return if thread_cwd.is_none() {
                Ok(())
            } else {
                Err(failure())
            };
        }
        if state == PreflightState::ObservationsMatched {
            if let Some(cwd) = thread_cwd {
                on_action(process, TextAction::VerifyWorkspace)?;
                on_action(process, TextAction::ObserveProcess)?;
                let request = flow.start_thread(cwd).map_err(|_| failure())?;
                let remaining = remaining_preflight_budget(budget, started.elapsed(), flow.diagnostic_stage())?;
                process.send_preflight_observed(request, remaining, cancel, &mut |process| {
                    on_action(process, TextAction::ObserveProcess)
                })?;
                // Exactly one reply and one thread/started, in either order.
                for _ in 0..2 {
                    let remaining = remaining_preflight_budget(budget, started.elapsed(), flow.diagnostic_stage())?;
                    let response =
                        process.receive_preflight_observed(remaining, cancel, &mut |process| {
                            on_action(process, TextAction::ObserveProcess)
                        })?;
                    on_action(process, TextAction::ObserveProcess)?;
                    remaining_preflight_budget(budget, started.elapsed(), flow.diagnostic_stage())?;
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
    storage: &'a mut crate::Storage,
    address: crate::project::WorkspaceAddress,
    cwd: std::path::PathBuf,
}

impl<'a> TextWorkspace<'a> {
    pub(crate) fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    pub(crate) fn new(
        projects: &'a crate::project::ProjectManager,
        storage: &'a mut crate::Storage,
        address: &crate::project::WorkspaceAddress,
    ) -> Result<Self, SidecarError> {
        let manifest = projects
            .verify_workspace_for_run(storage, address)
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
            .verify_workspace_for_run(self.storage, &self.address)
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
    workspace: &mut TextWorkspace<'_>,
    text: &str,
    on_public: &mut impl FnMut(
        &mut crate::Storage,
        elsecrew_core::provider::vendor_observation::codex_events::Projection,
    ) -> Result<(), SidecarError>,
    on_process: &mut impl FnMut(&mut crate::Storage, &mut LoginProcess<'_>) -> Result<(), SidecarError>,
) -> Result<
    elsecrew_core::provider::vendor_observation::codex_events::stream::TurnOutcome,
    SidecarError,
> {
    run_text_with_handler_and_close(
        process,
        flow,
        cancel,
        workspace,
        text,
        &mut |storage, process, action| match action {
            TextAction::Public(value) => on_public(storage, value),
            TextAction::ObserveProcess => on_process(storage, process),
            // Compatibility transport has no durable run; the reserved-run
            // driver handles this action with its own persistence barrier.
            TextAction::ThreadMatched(_) => on_process(storage, process),
            TextAction::Tool(_)
            | TextAction::PollApproval { .. }
            | TextAction::VerifyWorkspace
            | TextAction::VerifyApprovalBoundary => Err(failure()),
        },
    )
}

/// Trusted owner handler only: must be nonblocking and enforce role/OS admission.
/// No product caller or implicit tool permission. VerifyWorkspace is handled here,
/// never delegated; other callbacks share the exact bound workspace's storage.
pub(crate) fn run_text_with_handler_and_close(
    process: &mut LoginProcess<'_>,
    flow: PreflightExchange,
    cancel: &AtomicBool,
    workspace: &mut TextWorkspace<'_>,
    text: &str,
    on_action: &mut impl FnMut(
        &mut crate::Storage,
        &mut LoginProcess<'_>,
        TextAction<'_>,
    ) -> Result<(), SidecarError>,
) -> Result<
    elsecrew_core::provider::vendor_observation::codex_events::stream::TurnOutcome,
    SidecarError,
> {
    let cwd = workspace.cwd.clone();
    run_text_checked(
        process,
        flow,
        cancel,
        &cwd,
        text,
        &mut |process, action| match action {
            TextAction::VerifyWorkspace => workspace.revalidate(),
            action => on_action(workspace.storage, process, action),
        },
    )
}

// A single sequential callback avoids two closures aliasing the same ledger.
/// Keeps the exact workspace and connection binding without borrowing Storage.
/// The run owner retains cleanup and must advance using its original process.
pub(crate) struct BoundTextSession {
    session: incremental::TextSession,
    address: crate::project::WorkspaceAddress,
    cwd: std::path::PathBuf,
    connection_id: uuid::Uuid,
}

impl BoundTextSession {
    pub(crate) fn pending_approvals(
        &self,
    ) -> Vec<&elsecrew_core::provider::vendor_observation::codex_approval::PendingApproval> {
        self.session.pending_approvals()
    }

    pub(crate) fn begin(
        process: &mut LoginProcess<'_>,
        flow: PreflightExchange,
        workspace: &TextWorkspace<'_>,
        text: &str,
        output_budget: elsecrew_core::provider::ProviderEventBudget,
    ) -> Result<Self, SidecarError> {
        let session = incremental::TextSession::new(flow, &workspace.cwd, text, output_budget)?;
        workspace.revalidate()?;
        process.begin_preflight()?;
        Ok(Self {
            session,
            address: workspace.address.clone(),
            cwd: workspace.cwd.clone(),
            connection_id: process.connection_id(),
        })
    }

    pub(crate) fn matches_process(&self, process: &LoginProcess<'_>) -> bool {
        self.connection_id == process.connection_id()
    }

    pub(crate) fn diagnostic_stage(&self) -> &'static str {
        self.session.diagnostic_stage()
    }

    pub(crate) fn advance(
        &mut self,
        process: &mut LoginProcess<'_>,
        cancel: &AtomicBool,
        storage: &mut crate::Storage,
        projects: &crate::project::ProjectManager,
        on_action: &mut impl FnMut(
            &mut crate::Storage,
            &mut LoginProcess<'_>,
            TextAction<'_>,
        ) -> Result<(), SidecarError>,
    ) -> Result<
        Option<elsecrew_core::provider::vendor_observation::codex_events::stream::TurnOutcome>,
        SidecarError,
    > {
        if !self.matches_process(process) {
            return Err(failure());
        }
        // These bindings came from begin's validated workspace, not caller paths.
        // Revalidate at the existing authority checkpoints rather than doing
        // redundant database/filesystem work on every empty pipe poll.
        let workspace = TextWorkspace {
            projects,
            storage,
            address: self.address.clone(),
            cwd: self.cwd.clone(),
        };
        self.session
            .advance(process, cancel, &mut |process, action| match action {
                TextAction::VerifyWorkspace => workspace.revalidate(),
                action => on_action(workspace.storage, process, action),
            })
    }
}

// This is private transport plumbing, not a renderer-controlled action API.
pub(crate) enum TextAction<'a> {
    VerifyWorkspace,
    /// No default approval. The reserved owner must verify its immutable role,
    /// workspace/fence, live process and sealed outer-sandbox launch provenance.
    VerifyApprovalBoundary,
    ObserveProcess,
    ThreadMatched(&'a MatchedThreadReceipt),
    Public(elsecrew_core::provider::vendor_observation::codex_events::Projection),
    Tool(elsecrew_core::provider::vendor_observation::codex_events::stream::ToolEvent),
    /// Must return promptly; None keeps waiting, never grants a default decision.
    PollApproval {
        request:
            &'a elsecrew_core::provider::vendor_observation::codex_approval::PendingApproval,
        decision: &'a mut Option<
            elsecrew_core::provider::vendor_observation::codex_approval::Decision,
        >,
    },
}

fn run_text_checked(
    process: &mut LoginProcess<'_>,
    flow: PreflightExchange,
    cancel: &AtomicBool,
    cwd: &std::path::Path,
    text: &str,
    on_action: &mut impl FnMut(&mut LoginProcess<'_>, TextAction) -> Result<(), SidecarError>,
) -> Result<
    elsecrew_core::provider::vendor_observation::codex_events::stream::TurnOutcome,
    SidecarError,
> {
    let mut session = incremental::TextSession::new(flow, cwd, text, elsecrew_core::provider::ProviderEventBudget::LegacyV1)?;
    on_action(process, TextAction::VerifyWorkspace)?;
    process.begin_preflight()?;
    let result = loop {
        match session.advance(process, cancel, on_action) {
            Ok(Some(outcome)) => break Ok(outcome),
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(error) => break Err(error),
        }
    };
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
    use elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
    use serde_json::json;
    use std::process::{Command, Stdio};

    #[test]
    fn preflight_deadline_preserves_fixed_stage_and_exact_boundary() {
        let budget = Duration::from_secs(30);
        assert_eq!(remaining_preflight_budget(budget, Duration::from_secs(29), "config/read").unwrap(), Duration::from_secs(1));
        for elapsed in [budget, budget + Duration::from_nanos(1)] {
            let error = remaining_preflight_budget(budget, elapsed, "config/read").unwrap_err();
            assert_eq!(error.kind, "providerPreflightFailed");
            assert_eq!(error.detail, "Codex preflight deadline reached at config/read");
        }
    }

    /// Explicit approval only: real installed CLI, but fresh profile, OS-denied
    /// networking and config-only RPC. Never select the user's login profile.
    #[test]
    #[ignore = "requires explicit approval for installed CLI offline configuration inspection"]
    fn installed_codex_offline_configuration_only() {
        inspect_installed_codex_offline(false);
    }

    #[test]
    #[ignore = "requires explicit approval for installed CLI temporary configuration inspection"]
    fn installed_codex_project_trust_isolation_control() {
        inspect_project_trust_isolation(false, false, false);
    }

    #[test]
    #[ignore = "requires explicit approval for installed CLI temporary configuration inspection"]
    fn installed_codex_login_configuration_only() {
        inspect_project_trust_isolation(false, true, false);
    }

    #[test]
    #[ignore = "requires explicit approval for installed CLI temporary configuration inspection"]
    fn installed_codex_version_isolated_profile() {
        inspect_project_trust_isolation(false, false, true);
    }

    #[test]
    #[ignore = "requires explicit approval for temporary offline first-turn hook isolation control"]
    fn installed_codex_project_hook_isolation_control() {
        assert_eq!(std::env::var("ELSECREW_APPROVED_HOOK_PROBE").as_deref(), Ok("1"));
        assert_eq!(std::env::var("ELSECREW_APPROVED_OFFLINE_HOOK_TURN").as_deref(), Ok("1"));
        inspect_project_trust_isolation(true, false, false);
    }

    fn inspect_project_trust_isolation(hook_probe: bool, login_probe: bool, version_probe: bool) {
        use crate::provider_profile::LoginProfile;
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;
        assert_eq!(std::env::var("ELSECREW_APPROVED_OFFLINE_CONFIG").as_deref(), Ok("1"));
        // Test-only owner: no product profile admission is bypassed. All paths
        // and configuration below are freshly created, without credentials.
        struct InspectionProfile {
            root: PathBuf,
            home: PathBuf,
            config: Vec<u8>,
        }
        impl LoginProfile for InspectionProfile {
            fn revalidate(&self) -> Result<PathBuf, SidecarError> {
                if std::fs::read(self.home.join("config.toml")).map_err(|_| failure())? != self.config
                    || self.home.join("auth.json").exists()
                {
                    return Err(failure());
                }
                Ok(self.home.clone())
            }
            fn login_lease_directory(&self) -> Result<cap_std::fs::Dir, SidecarError> {
                crate::provider_profile::open_absolute_directory(&self.root)
            }
            fn pending_name(&self) -> &'static str { "codex-login.pending" }
        }
        let (cli, helper) = crate::provider_login_manager::select_installed_codex_pair().unwrap();
        for isolated in [false, true] {
            if (login_probe || version_probe) && !isolated { continue; }
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let home = root.join("profile");
            let workspace = if version_probe { home.clone() } else { root.join("workspace") };
            std::fs::create_dir(&home).unwrap();
            if workspace != home { std::fs::create_dir(&workspace).unwrap(); }
            assert!(Command::new("/usr/bin/git")
                .args(["-c", "init.templateDir=", "init", "--quiet"])
                .current_dir(&workspace).env_clear().env("HOME", &home)
                .status().unwrap().success());
            std::fs::create_dir(workspace.join(".codex")).unwrap();
            let marker = home.join("hook-probe-marker");
            let mut project_config = "developer_instructions='ELSECREW_PROJECT_CONFIG_SENTINEL'\n".to_owned();
            if hook_probe || version_probe {
                let marker_path = marker.to_str().unwrap();
                assert!(!marker_path.contains('\''));
                let command = serde_json::to_string(&format!("/usr/bin/touch '{marker_path}'")).unwrap();
                project_config.push_str(&format!(
                    "[[hooks.SessionStart]]\n[[hooks.SessionStart.hooks]]\ntype='command'\ncommand={command}\ntimeout=3\n"
                ));
            }
            std::fs::write(workspace.join(".codex/config.toml"), &project_config).unwrap();
            let config = format!("cli_auth_credentials_store='file'\n[projects.{}]\ntrust_level='trusted'\n",
                serde_json::to_string(workspace.to_str().unwrap()).unwrap()).into_bytes();
            std::fs::write(home.join("config.toml"), &config).unwrap();
            std::fs::set_permissions(home.join("config.toml"), std::fs::Permissions::from_mode(0o600)).unwrap();
            let profile = InspectionProfile { root, home: home.clone(), config };
            let expected = ExpectedProfile { model: "test-model", reasoning_effort: "medium" };
            let mut args = if version_probe {
                codex_version_arguments(&home).unwrap()
            } else if login_probe {
                ["app-server", "--listen", "stdio://", "-c", "features.hooks=false", "-c", "features.apps=false", "-c", "features.plugins=false"].into_iter().map(std::ffi::OsString::from).collect()
            } else if isolated && !hook_probe {
                named_discovery_arguments(&home, &workspace, &expected, ExpectedPermission::ReadOnly).unwrap()
            } else {
                discovery_arguments(&home, &expected, ExpectedPermission::ReadOnly).unwrap()
            };
            if hook_probe {
                // The fixed test-only thread request carries hook trust bypass.
                // It is a request override, not an app-server startup setting.
                if !isolated {
                    let flag = args.iter_mut().find(|arg| *arg == "features.hooks=false").unwrap();
                    *flag = "features.hooks=true".into();
                }
            }
            if isolated && !version_probe {
                args.extend(["-c".into(), crate::provider_profile::codex_isolation_argument(&home, if login_probe { &home } else { &workspace }).unwrap()]);
            }
            let mut process = LoginProcess::reserve(&profile).unwrap();
            let cancel = AtomicBool::new(false);
            let launch = process.launch_with_barrier(
                || {
                    if version_probe {
                        let cli_path = cli.revalidate().map_err(std::io::Error::other)?;
                        let mut sandbox_args = vec![std::ffi::OsString::from("-p"), "(version 1)(allow default)(deny network*)".into(), cli_path.as_os_str().to_owned()];
                        sandbox_args.extend(args.iter().cloned());
                        let references: Vec<_> = sandbox_args.iter().map(std::ffi::OsString::as_os_str).collect();
                        let mut command = crate::provider_login_gate::command(std::path::Path::new("/usr/bin/sandbox-exec"), &references, &home)?;
                        discovery_environment(&mut command, &home);
                        return command.spawn();
                    }
                    let mut command = crate::provider_runtime_sandbox::command(
                        cli.revalidate().map_err(std::io::Error::other)?,
                        helper.revalidate().map_err(std::io::Error::other)?,
                        &args, &home, &workspace,
                        crate::provider_runtime_sandbox::WorkspaceAccess::ReadOnly,
                    )?;
                    discovery_environment(&mut command, &home);
                    // Exercise named-launch cwd semantics while retaining the
                    // probe's outer OS network/file restrictions.
                    command.current_dir(if login_probe { &home } else { &workspace });
                    command.spawn()
                },
                |child| {
                    cli.revalidate().map_err(std::io::Error::other)?;
                    helper.revalidate().map_err(std::io::Error::other)?;
                    profile.revalidate().map_err(std::io::Error::other)?;
                    crate::provider_login_gate::release(child)
                },
            );
            let observation = launch.and_then(|()| {
                if version_probe {
                    use crate::provider_login_io::OutputChunk;
                    let deadline = Instant::now() + Duration::from_secs(5);
                    let mut output = Vec::new();
                    loop {
                        if Instant::now() >= deadline { return Err(failure()); }
                        match process.poll_output(&cancel)? {
                            OutputChunk::Bytes(bytes) => { output.extend(bytes); if output.len() > 1024 { return Err(failure()); } }
                            OutputChunk::Eof => break,
                            OutputChunk::Pending => std::thread::sleep(Duration::from_millis(10)),
                        }
                    }
                    let observation = elsecrew_core::provider::BoundedObservation::new(output).map_err(|_| failure())?;
                    elsecrew_core::provider::parse_cli_version(elsecrew_core::provider::ProviderId::Codex, &observation).map_err(|_| failure())?;
                    if marker.exists() { return Err(failure()); }
                    loop {
                        if let Some(status) = process.exit_status()? {
                            if !status.success() { return Err(failure()); }
                            break;
                        }
                        if Instant::now() >= deadline { return Err(failure()); }
                        process.tick()?;
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    eprintln!("version_probe parsed=true hook_marker=false account_request_sent=false");
                    return Ok(());
                }
                if login_probe {
                    use elsecrew_core::provider::vendor_observation::codex_login::connection::{LoginConnection, LoginAction};
                    let (mut flow, init) = LoginConnection::begin(home.to_str().unwrap()).map_err(|_| failure())?;
                    flow.require_startup_isolation().map_err(|_| failure())?;
                    process.send(init, Duration::from_secs(15), &cancel)?;
                    let reply = process.receive(Duration::from_secs(15), &cancel)?;
                    let LoginAction::Send(initialized) = flow.accept(&reply).map_err(|_| failure())? else { return Err(failure()); };
                    process.send(initialized, Duration::from_secs(15), &cancel)?;
                    let configuration = flow.check_account().map_err(|_| failure())?;
                    if serde_json::from_str::<serde_json::Value>(configuration.as_str()).map_err(|_| failure())?["method"] != "config/read" { return Err(failure()); }
                    process.send(configuration, Duration::from_secs(15), &cancel)?;
                    let deadline = Instant::now() + Duration::from_secs(20);
                    loop {
                        let reply = process.receive(deadline.checked_duration_since(Instant::now()).ok_or_else(failure)?, &cancel)?;
                        match flow.accept(&reply).map_err(|_| failure())? {
                            LoginAction::ConfigurationPending => continue,
                            LoginAction::Send(account) => {
                                if serde_json::from_str::<serde_json::Value>(account.as_str()).map_err(|_| failure())?["method"] != "account/read" { return Err(failure()); }
                                // Stop BEFORE sending account/read: no account,
                                // OAuth, model or thread query in this probe.
                                eprintln!("login_config_probe configuration_matched=true account_request_sent=false");
                                return Ok(());
                            }
                            _ => return Err(failure()),
                        }
                    }
                }
                process.begin_preflight()?;
                // Named configuration explicitly scopes config/read to cwd.
                // This phase sends only initialize/config. The optional hook
                // probe below separately gates an ephemeral offline turn.
                let mut flow = PreflightExchange::with_named_permission(
                    &expected, home.to_str().unwrap(), &workspace, ExpectedPermission::ReadOnly,
                ).map_err(|_| failure())?;
                if isolated && !hook_probe { flow.require_startup_isolation(&workspace).map_err(|_| failure())?; }
                for frame in flow.next_frames().map_err(|_| failure())? {
                    process.send_preflight(frame, Duration::from_secs(15), &cancel)?;
                }
                let reply = process.receive_preflight(Duration::from_secs(15), &cancel)?;
                flow.accept(&reply).map_err(|_| failure())?;
                eprintln!("project_trust_control isolated={isolated} initialized=true");
                for frame in flow.next_frames().map_err(|_| failure())? {
                    process.send_preflight(frame, Duration::from_secs(15), &cancel)?;
                }
                let receive_config = |process: &mut LoginProcess<'_>, timeout| {
                    let value = process.receive_permission_probe(timeout, &cancel)?;
                    if value["method"] == "configWarning" {
                        for key in ["summary", "details"] {
                            if let Some(text) = value["params"][key].as_str().filter(|text| text.len() <= 2048) {
                                eprintln!("project_trust_control warning_{key}={}", json!(text.replace(profile.root.to_str().unwrap(), "<probe>")));
                            }
                        }
                    }
                    if let Some(layers) = value["result"]["layers"].as_array() {
                        for layer in layers.iter().filter(|layer| layer["name"]["type"] == "project").take(8) {
                            for (key, field) in [("folder", &layer["name"]["dotCodexFolder"]), ("disabledReason", &layer["disabledReason"])] {
                                if let Some(text) = field.as_str().filter(|text| text.len() <= 2048) {
                                    eprintln!("project_trust_control project_{key}={}", json!(text.replace(profile.root.to_str().unwrap(), "<probe>")));
                                }
                            }
                        }
                    }
                    elsecrew_core::provider::BoundedObservation::new(serde_json::to_vec(&value).map_err(|_| failure())?).map_err(|_| failure())
                };
                let mut reply = receive_config(&mut process, Duration::from_secs(15))?;
                // Count bounded warning frames for the diagnostic controls.
                // Only the isolated config-only case also exercises product
                // validation; hook controls retain their separate probe path.
                let mut warning_count = 0;
                let config_deadline = Instant::now() + Duration::from_secs(20);
                for _ in 0..8 {
                    if !elsecrew_core::provider::vendor_observation::codex_profile_config::is_probe_configuration_warning(&reply) {
                        break;
                    }
                    warning_count += 1;
                    if isolated && !hook_probe { flow.accept(&reply).map_err(|_| failure())?; }
                    let remaining = config_deadline.checked_duration_since(Instant::now()).ok_or_else(failure)?;
                    reply = receive_config(&mut process, remaining)?;
                }
                eprintln!("project_trust_control isolated={isolated} diagnostic_warning_count={warning_count}");
                if isolated && !hook_probe {
                    flow.accept(&reply).map_err(|_| failure())?;
                    eprintln!("project_trust_control isolated=true product_configuration_matched=true");
                }
                elsecrew_core::provider::vendor_observation::codex_profile_config::matches_project_config_probe(
                    &reply, &home, &workspace, isolated, hook_probe && !isolated,
                ).map_err(|reason| SidecarError::new("configProbeFailed", reason))?;
                eprintln!("project_trust_control isolated={isolated} matched=true");
                if hook_probe {
                    // This native opt-in probe has no credentials or user data.
                    // Inspect only bounded warning text, redact its disposable
                    // root, and never expose this diagnostic through product IPC.
                    let log_warning = |reply: &serde_json::Value| {
                        if !matches!(reply["method"].as_str(), Some("configWarning" | "warning")) {
                            return;
                        }
                        for key in ["summary", "details", "message"] {
                            if let Some(text) = reply["params"][key].as_str().filter(|text| text.len() <= 2048) {
                                let text = text.replace(profile.root.to_str().unwrap(), "<probe>");
                                eprintln!("hook_probe warning_{key}={}", json!(text));
                            }
                        }
                    };
                    eprintln!("hook_probe isolated={isolated} marker_before_thread={}", marker.exists());
                    use crate::provider_login_io::HookProbeRequest;
                    process.send_hook_probe(HookProbeRequest::Start(&workspace), &cancel)?;
                    let mut thread_id = None;
                    let deadline = Instant::now() + Duration::from_secs(20);
                    for _ in 0..64 {
                        let remaining = deadline.checked_duration_since(Instant::now())
                            .ok_or_else(|| SidecarError::new("hookProbeFailed", "thread response timed out"))?;
                        let value = process.receive_permission_probe(remaining, &cancel)?;
                        log_warning(&value);
                        let reply = elsecrew_core::provider::BoundedObservation::new(serde_json::to_vec(&value).map_err(|_| failure())?)
                            .map_err(|_| failure())?;
                        eprintln!("hook_probe event={}", elsecrew_core::provider::vendor_observation::codex_profile_config::hook_probe_notification(&reply));
                        if elsecrew_core::provider::vendor_observation::codex_profile_config::hook_probe_started(&reply)
                            .map_err(|reason| SidecarError::new("hookProbeFailed", reason))? {
                            thread_id = value["result"]["thread"]["id"].as_str().map(str::to_owned);
                            break;
                        }
                    }
                    let thread_id = thread_id.ok_or_else(|| SidecarError::new("hookProbeFailed", "thread response missing"))?;
                    eprintln!("hook_probe isolated={isolated} thread_response_received=true");
                    process.send_hook_probe(HookProbeRequest::Inventory(&workspace), &cancel)?;
                    let inventory_deadline = Instant::now() + Duration::from_secs(15);
                    let mut inventory = None;
                    for _ in 0..64 {
                        let remaining = inventory_deadline.checked_duration_since(Instant::now()).ok_or_else(failure)?;
                        let reply = process.receive_permission_probe(remaining, &cancel)?;
                        log_warning(&reply);
                        if reply["id"] == 100 {
                            if !reply["error"].is_null() { return Err(SidecarError::new("hookProbeFailed", "hook inventory rejected")); }
                            inventory = Some(reply["result"].clone());
                            break;
                        }
                        if reply.get("id").is_some() { return Err(failure()); }
                        let bounded = elsecrew_core::provider::BoundedObservation::new(serde_json::to_vec(&reply).map_err(|_| failure())?)
                            .map_err(|_| failure())?;
                        eprintln!("hook_probe inventory_event={}", elsecrew_core::provider::vendor_observation::codex_profile_config::hook_probe_notification(&bounded));
                    }
                    fn shape(value: &serde_json::Value, depth: usize) -> serde_json::Value {
                        if depth > 6 { return json!("depth-limit"); }
                        match value {
                            serde_json::Value::Object(map) => map.iter().map(|(key, value)| (key.clone(), shape(value, depth + 1))).collect(),
                            serde_json::Value::Array(items) => items.iter().take(8).map(|item| shape(item, depth + 1)).collect(),
                            serde_json::Value::String(text) => {
                                if ["SessionStart", "command", "trusted", "untrusted", "project", "user", "enabled", "disabled"].contains(&text.as_str()) { json!(text) }
                                else { json!({"textLength":text.len()}) }
                            }
                            value => value.clone(),
                        }
                    }
                    eprintln!("hook_probe isolated={isolated} inventory_shape={}", shape(&inventory.ok_or_else(failure)?, 0));
                    if std::env::var("ELSECREW_APPROVED_OFFLINE_HOOK_TURN").as_deref() == Ok("1") {
                        // Separate approval; no account, proxy or network permission.
                        process.send_hook_probe(HookProbeRequest::OfflineTurn(&thread_id), &cancel)?;
                        eprintln!("hook_probe isolated={isolated} offline_turn_sent=true");
                    }
                    // Keep the owner alive for the hook timeout plus a margin.
                    let settle = Instant::now() + Duration::from_secs(4);
                    let mut frames = 0;
                    while Instant::now() < settle {
                        if let Some(reply) = process.poll_preflight_observed(&cancel, &mut |_| Ok(()))? {
                            frames += 1;
                            if frames > 64 { return Err(failure()); }
                            eprintln!("hook_probe settled_event={}", elsecrew_core::provider::vendor_observation::codex_profile_config::hook_probe_notification(&reply));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                Ok(())
            });
            let cleanup = process.finish();
            let confirmed = process.closure_evidence().is_some_and(|evidence| evidence.confirmed());
            if cleanup.is_err() || !confirmed {
                let retained = temp.keep();
                panic!("inspection termination unconfirmed; retained {}", retained.display());
            }
            profile.revalidate().unwrap();
            assert_eq!(std::fs::read(workspace.join(".codex/config.toml")).unwrap(), project_config.as_bytes());
            if version_probe { assert!(!marker.exists(), "version probe hook must remain unexecuted after termination"); }
            assert!(observation.is_ok(), "project trust control failed: isolated={isolated}, fixed_error={:?}",
                observation.as_ref().err().map(|error| &error.detail));
            if hook_probe {
                assert_eq!(marker.exists(), !isolated, "hook execution control isolated={isolated}");
                eprintln!("hook_probe isolated={isolated} marker_present={} cleanup_confirmed=true", marker.exists());
            }
        }
    }

    /// Reproduce a vendor-update leftover in a disposable profile only. This
    /// probe does not authorize bypassing runtime admission for a real profile.
    #[test]
    #[ignore = "requires explicit approval for installed CLI offline configuration inspection"]
    fn installed_codex_offline_stale_helper_probe() {
        inspect_installed_codex_offline(true);
    }

    fn inspect_installed_codex_offline(stale_helper: bool) {
        assert_eq!(
            std::env::var("ELSECREW_APPROVED_OFFLINE_CONFIG").as_deref(),
            Ok("1"),
            "explicit config-only inspection approval is required"
        );
        let (cli, helper) = crate::provider_login_manager::select_installed_codex_pair().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("empty-workspace");
        std::fs::create_dir(&workspace).unwrap();
        let storage = crate::Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
        let profile = CodexProfileBinding::from_storage(&storage).unwrap();
        let home = profile.revalidate().unwrap();
        let original_config = std::fs::read(home.join("config.toml")).unwrap();
        assert!(!home.join("auth.json").exists());
        crate::provider_profile::verify_runtime_files(&home).unwrap();
        let stale_link = home.join("tmp/arg0/codex-arg0stale/codex-execve-wrapper");
        if stale_helper {
            std::fs::create_dir_all(stale_link.parent().unwrap()).unwrap();
            // Deliberately absent target inside the disposable test root. Never
            // link to user data or give the vendor an additional executable.
            std::os::unix::fs::symlink(root.join("removed-cli"), &stale_link).unwrap();
            assert!(crate::provider_profile::verify_runtime_files(&home).is_err());
        }
        // This is a configuration tuple only, not a model availability claim.
        let expected = ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
        };
        let mut flow =
            PreflightExchange::configuration_only(&expected, home.to_str().unwrap()).unwrap();
        flow.require_startup_isolation(&workspace).unwrap();
        let cancel = AtomicBool::new(false);
        let mut process = LoginProcess::reserve(&profile).unwrap();
        let launch = process.launch_with_barrier(
            || {
                let cli_path = cli.revalidate().map_err(std::io::Error::other)?;
                let helper_path = helper.revalidate().map_err(std::io::Error::other)?;
                offline_discovery_command(
                    cli_path,
                    helper_path,
                    &home,
                    &workspace,
                    &expected,
                    ExpectedPermission::ReadOnly,
                )?
                .spawn()
            },
            |child| {
                cli.revalidate().map_err(std::io::Error::other)?;
                helper.revalidate().map_err(std::io::Error::other)?;
                if profile.revalidate().map_err(std::io::Error::other)? != home {
                    return Err(std::io::Error::other("inspection profile changed"));
                }
                crate::provider_login_gate::release(child)
            },
        );
        // Name only the exchange stage, never print raw config/auth responses.
        // Use the same typed transport and validators as the shared driver.
        eprintln!(
            "offline_config owned_pids={:?}",
            process
                .tracked_for_cleanup()
                .iter()
                .map(|identity| identity.pid)
                .collect::<Vec<_>>()
        );
        let observation = launch.and_then(|()| {
            process.begin_preflight()?;
            let started = Instant::now();
            for stage in ["initialize", "configuration"] {
                let remaining = || {
                    Duration::from_secs(30)
                        .checked_sub(started.elapsed())
                        .ok_or_else(failure)
                };
                for frame in flow.next_frames().map_err(|_| failure())? {
                    process.send_preflight(frame, remaining()?, &cancel)?;
                }
                let reply = process.receive_preflight(remaining()?, &cancel)?;
                let accepted = flow.accept(&reply);
                eprintln!("offline_config stage={stage} accepted={}", accepted.is_ok());
                accepted.map_err(|_| failure())?;
            }
            if flow.state() != PreflightState::ConfigurationMatched
                || !flow.next_frames().map_err(|_| failure())?.is_empty()
            {
                return Err(failure());
            }
            Ok(())
        });
        // Retain cleanup ownership on every diagnostic failure. Never retry and
        // reinterpret an uncertain receipt as a confirmed result.
        let cleanup = if process.closure_evidence().is_some() {
            Ok(())
        } else {
            process.finish()
        };
        let confirmed = process
            .closure_evidence()
            .is_some_and(|evidence| evidence.confirmed());
        if observation.is_err() || cleanup.is_err() || !confirmed {
            flow.invalidate();
        }
        flow.close();
        if stale_helper {
            eprintln!(
                "offline_stale_helper retained={} runtime_files_accepted={}",
                std::fs::symlink_metadata(&stale_link).is_ok(),
                crate::provider_profile::verify_runtime_files(&home).is_ok(),
            );
        }
        eprintln!(
            "offline_config observation={} cleanup={} confirmed={} state={:?}",
            observation
                .as_ref()
                .err()
                .map_or("matched", |error| error.kind()),
            cleanup.as_ref().err().map_or("ok", |error| error.kind()),
            confirmed,
            flow.state()
        );
        if cleanup.is_err() || !confirmed {
            let retained = temp.keep();
            panic!("offline inspection termination unconfirmed; retained {}", retained.display());
        }
        assert_eq!(
            std::fs::read(home.join("config.toml")).unwrap(),
            original_config
        );
        assert!(!home.join("auth.json").exists());
        assert!(
            cleanup.is_ok() && confirmed,
            "offline inspection cleanup was not confirmed"
        );
        assert!(observation.is_ok(), "offline configuration did not match");
        assert_eq!(flow.state(), PreflightState::Closed);
        assert!(flow.next_request().is_err());
        drop(process);
        LoginProcess::reserve(&profile).unwrap().finish().unwrap();
    }

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
            let storage = crate::Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
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
                &mut |_, action| {
                    assert!(matches!(action, TextAction::VerifyWorkspace));
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
        use elsecrew_core::proctree::{ProbeResult, ProcTable, SystemProcTable};
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Arc;
        const SCRIPT: &str = "#!/bin/sh\nprintf ran > \"$HOME/vendor-ran\"\n";
        for case in ["cli", "helper", "cancel"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
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
printf '{"id":2,"result":{"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only","features":{"hooks":false,"apps":false,"plugins":false}},"layers":[{"name":{"type":"sessionFlags"},"config":{"model":"test-model","model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only","features":{"hooks":false,"apps":false,"plugins":false}}},{"name":{"type":"user","file":"%s/config.toml"},"config":{"cli_auth_credentials_store":"file"}},{"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}]}}\n' "$HOME"
IFS= read -r request
printf '%s\n' '{"id":3,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}'
IFS= read -r request
printf '%s\n' '{"id":4,"result":{"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}}'
IFS= read -r request
test "$request" = '{"id":50,"method":"configRequirements/read","params":{}}' || exit 3
printf '%s\n' '{"id":50,"result":{"requirements":null}}'
IFS= read -r hold
"#;
        for case in [
            "success",
            "canceled",
            "changed-cli",
            "changed-helper",
            "eof",
            "missing-isolation",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let original = std::fs::read(home.join("config.toml")).unwrap();
            let script = if case == "eof" {
                "#!/bin/sh\nexit 0\n".to_owned()
            } else if case == "missing-isolation" {
                PEER.to_owned()
            } else {
                let config = elsecrew_core::provider::vendor_observation::codex_profile_config::ProfileConfig::parse(&original).unwrap();
                let projects = config.isolated_projects(&[&home]).unwrap();
                PEER.replace("\"features\":", &format!("\"projects\":{projects},\"features\":"))
            };
            let cli_path = root.join("toy-cli");
            let helper_path = root.join("toy-helper");
            for (path, source) in [(&cli_path, script.as_str()), (&helper_path, "#!/bin/sh\nexit 0\n")] {
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
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        std::fs::write(home.join("config.toml"), b"cli_auth_credentials_store='file'").unwrap();
        std::fs::set_permissions(home.join("config.toml"), std::fs::Permissions::from_mode(0o600)).unwrap();
        let profile = ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
        };
        let command = discovery_command(
            std::path::Path::new("/verified/codex"),
            &home,
            &profile,
        )
        .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            &args[4..args.len() - 2],
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
                "features.hooks=false",
                "-c",
                "features.apps=false",
                "-c",
                "features.plugins=false"
            ]
        );
        assert_eq!(args[args.len() - 2], "-c");
        assert_eq!(args[args.len() - 1],
            crate::provider_profile::codex_isolation_argument(&home, &home).unwrap().to_str().unwrap());
        assert_eq!(
            command.get_current_dir(),
            Some(home.as_path())
        );
        let env: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_str().unwrap(), value.unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(env.len(), 4);
        assert_eq!(env["HOME"], home.to_str().unwrap());
        assert_eq!(env["CODEX_HOME"], home.to_str().unwrap());
        assert_eq!(env["TMPDIR"], home.join("tmp").to_str().unwrap());
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
            "requirements",
            "requirements-error",
            "config-only",
            "config-only-thread",
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
            "turn-approval-decline",
            "turn-approval-cleared",
            "turn-approval-cancel",
            "turn-approval-handler-error",
            "turn-approval-accept-blocked",
            "turn-approval-send-error",
            "turn-workspace-before-thread",
            "turn-workspace-before-prompt",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let storage = crate::Storage::open(
                &temp
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("elsecrew.sqlite3"),
            )
            .unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let constructor = if case.starts_with("config-only") {
                PreflightExchange::configuration_only
            } else {
                PreflightExchange::new
            };
            let mut flow = constructor(
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
                    value["result"]["config"]["features"]=json!({"hooks":false,"apps":false,"plugins":false});
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
IFS= read -r line || exit 0
printf received > "$7.account"
case "$line" in *'"method":"account/read"'*) ;; *) exit 5;; esac
printf '%s\n' '{"method":"account/updated","emittedAtMs":1,"params":{"authMode":"chatgpt","planType":"pro"}}'
printf '%s\n' "$3"
IFS= read -r line; case "$line" in *'"method":"model/list"'*) ;; *) exit 6;; esac
printf '%s\n' "$4"
IFS= read -r line; test "$line" = '{"id":50,"method":"configRequirements/read","params":{}}' || exit 6
if test "$6" = requirements; then
printf '%s\n' '{"id":50,"result":{"requirements":{"allowedSandboxModes":["readOnly"]}}}'
elif test "$6" = requirements-error; then
printf '%s\n' '{"id":50,"error":{"message":"PRIVATE_POLICY"}}'
else
printf '%s\n' '{"id":50,"result":{"requirements":null}}'
fi
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
case "$6" in turn-approval-*)
printf '%s\n' '{"method":"item/started","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"commandExecution","id":"c1","command":"echo test","cwd":"/managed/snapshot","status":"inProgress"}}}'
printf '%s\n' '{"id":40,"method":"item/commandExecution/requestApproval","params":{"threadId":"thread-test","turnId":"t1","itemId":"c1","startedAtMs":1,"kind":"command"}}'
if test "$6" = turn-approval-send-error; then exec 0<&-; /bin/sleep 0.4; exit 0; fi
if test "$6" != turn-approval-cleared; then
IFS= read -r decision || exit 9
printf '%s\n' "$decision" > "$9"
test "$decision" = '{"id":40,"result":{"decision":"decline"}}' || exit 10
fi
printf '%s\n' '{"method":"serverRequest/resolved","params":{"threadId":"thread-test","requestId":40}}'
printf '%s\n' '{"method":"item/completed","params":{"threadId":"thread-test","turnId":"t1","item":{"type":"commandExecution","id":"c1","command":"echo test","cwd":"/managed/snapshot","status":"declined"}}}'
;; esac
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
                        .arg(temp.path().join("approval-received"))
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
                use elsecrew_core::provider::vendor_observation::codex_events::stream::TurnOutcome;
                use elsecrew_core::provider::vendor_observation::codex_events::Projection;
                let mut text = String::new();
                let mut checks = 0;
                let mut approval_polls = 0;
                let mut tool_events = 0;
                let mut approval_observations = 0;
                let result = run_text_checked(
                    &mut process,
                    flow,
                    &cancel,
                    std::path::Path::new("/managed/snapshot"),
                    "test goal",
                    &mut |_, action| {
                        let value = match action {
                            TextAction::ThreadMatched(_) => return Ok(()),
                            TextAction::VerifyApprovalBoundary => return Err(failure()),
                            TextAction::ObserveProcess => {
                                if approval_polls > 0 {
                                    approval_observations += 1;
                                }
                                return Ok(());
                            }
                            TextAction::VerifyWorkspace => {
                                checks += 1;
                                // Six small preflight frames add six flush checks;
                                // the thread frame adds one before the prompt boundary.
                                if (case == "turn-workspace-before-thread" && checks == 8)
                                    || (case == "turn-workspace-before-prompt" && checks == 10)
                                {
                                    return Err(failure());
                                }
                                return Ok(());
                            }
                            TextAction::Public(value) => value,
                            TextAction::Tool(tool) => {
                                if !case.starts_with("turn-approval-") {
                                    return Err(failure());
                                }
                                assert_eq!(tool.item_id, "c1");
                                tool_events += 1;
                                return Ok(());
                            }
                            TextAction::PollApproval { request, decision } => {
                                if !case.starts_with("turn-approval-") {
                                    return Err(failure());
                                }
                                assert_eq!(request.item_id, "c1");
                                approval_polls += 1;
                                if case == "turn-approval-handler-error" {
                                    return Err(failure());
                                }
                                if case == "turn-approval-cancel" {
                                    cancel.store(true, Ordering::Release);
                                }
                                if approval_polls >= 55 {
                                    use elsecrew_core::provider::vendor_observation::codex_approval::Decision;
                                    *decision = Some(if case == "turn-approval-accept-blocked" {
                                        Decision::AcceptOnce
                                    } else {
                                        Decision::Decline
                                    });
                                }
                                return Ok(());
                            }
                        };
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
                );
                if matches!(
                    case,
                    "turn-success" | "turn-approval-decline" | "turn-approval-cleared"
                ) {
                    // Initial/boundary checks plus every queued write attempt.
                    // Backpressure may add further authority revalidations.
                    assert!(
                        checks
                            >= if case == "turn-approval-decline" {
                                12
                            } else {
                                10
                            }
                    );
                    assert_eq!(result.unwrap(), TurnOutcome::Completed);
                    assert_eq!(text, "hello");
                } else {
                    assert!(result.is_err(), "{case}");
                }
                if case.starts_with("turn-approval-") {
                    assert_eq!(
                        temp.path().join("approval-received").exists(),
                        case == "turn-approval-decline"
                    );
                    if case == "turn-approval-cleared" {
                        assert!(approval_polls < 55);
                    }
                    if case == "turn-approval-decline" {
                        assert_eq!(approval_polls, 55);
                        assert!(approval_observations > 0);
                    }
                    assert_eq!(
                        tool_events,
                        if matches!(case, "turn-approval-decline" | "turn-approval-cleared") {
                            2
                        } else {
                            1
                        }
                    );
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
                let (_, request) = elsecrew_core::provider::vendor_observation::codex_login::connection::LoginConnection::begin(home.to_str().unwrap()).unwrap();
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
                let (_, request) = elsecrew_core::provider::vendor_observation::codex_login::connection::LoginConnection::begin(home.to_str().unwrap()).unwrap();
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
            let thread_cwd = if matches!(
                case,
                "thread"
                    | "thread-before"
                    | "wrong-thread"
                    | "config-only-thread"
                    | "requirements"
                    | "requirements-error"
            ) {
                Some(std::path::Path::new("/managed/snapshot"))
            } else {
                None
            };
            let result = observe_with_budget(&mut process, &mut flow, &cancel, budget, thread_cwd);
            assert_eq!(
                result.is_ok(),
                matches!(case, "success" | "thread" | "thread-before" | "config-only"),
                "{case}"
            );
            assert_eq!(
                flow.state(),
                if matches!(case, "success" | "thread" | "thread-before" | "config-only") {
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
            if case.starts_with("requirements") {
                assert!(!temp.path().join("thread-received").exists());
                assert!(!temp.path().join("prompt-received").exists());
                assert!(process.closure_evidence().unwrap().confirmed());
            }
            if case.starts_with("config-only") {
                assert!(!temp.path().join("thread-received.account").exists());
                assert!(!temp.path().join("thread-received").exists());
                assert!(!temp.path().join("prompt-received").exists());
                assert!(process.closure_evidence().unwrap().confirmed());
            }
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
