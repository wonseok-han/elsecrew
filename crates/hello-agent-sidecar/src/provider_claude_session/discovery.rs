//! Model-free selected-profile catalog observation, never account entitlement.
//! Call on a worker; every failed inspection returns its original cleanup owner.
use super::*;
use crate::{
    provider_claude_profile::ClaudeProfileBinding, provider_executable::PinnedExecutable,
    provider_preflight_session::ProviderInspection, provider_profile::LoginProfile,
};
use std::path::{Path, PathBuf};
use std::{sync::Arc, thread::JoinHandle};

mod status;

pub(crate) struct DiscoveryRequest {
    pub(crate) profile: ClaudeProfileBinding,
    pub(crate) selector: String,
    pub(crate) effort: Option<String>,
    pub(crate) access: ToolAccess,
    pub(crate) workspace: PathBuf,
    pub(crate) source: PathBuf,
}

pub(crate) struct DiscoveryEvidence {
    pub(crate) observation: ProfileObservation,
    pub(crate) executable: PinnedExecutable,
    pub(crate) version: String,
    /// Credential presence, not a successful model request or entitlement.
    pub(crate) credentials_present: bool,
    observed_from: Instant,
}

impl DiscoveryEvidence {
    pub(crate) fn is_fresh(&self) -> bool {
        self.observed_from.elapsed() <= Duration::from_secs(30)
    }
}

pub(crate) struct Completion {
    pub(crate) result: Result<DiscoveryEvidence, SidecarError>,
    pub(crate) cleanup: Option<LoginProcess<'static>>,
}

pub(crate) struct DiscoveryWorker {
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<Completion>>,
}

impl DiscoveryWorker {
    pub(crate) fn spawn(
        request: DiscoveryRequest,
        mut admit: impl FnMut(&Path, &Path, &str, Option<&str>, ToolAccess) -> Result<(), SidecarError>
            + Send
            + 'static,
    ) -> Result<Self, SidecarError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let handle = std::thread::Builder::new()
            .name("claude-discovery".into())
            .spawn(move || {
                // Signature checks and initialization must not block the dispatcher.
                let executable = match crate::provider_claude_login::installed_executable() {
                    Ok(executable) => executable,
                    Err(error) => {
                        return Completion {
                            result: Err(error),
                            cleanup: None,
                        }
                    }
                };
                let observed_from = Instant::now();
                let mut cleanup = None;
                let metadata = (|| {
                    let mut inspect = |query| {
                        let inspection = status::inspect(
                            request.profile.try_clone()?,
                            &executable,
                            &signal,
                            query,
                        )?;
                        match inspection.result {
                            Ok(observation) => Ok(observation),
                            Err(error) => {
                                cleanup = Some(inspection.process);
                                Err(error)
                            }
                        }
                    };
                    let status::Observation::Version(version) = inspect(status::Query::Version)?
                    else {
                        return Err(failure());
                    };
                    let status::Observation::CredentialsPresent(credentials_present) =
                        inspect(status::Query::Auth)?
                    else {
                        return Err(failure());
                    };
                    Ok((version, credentials_present))
                })();
                let (version, credentials_present) = match metadata {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        return Completion {
                            result: Err(error),
                            cleanup,
                        }
                    }
                };
                match inspect_candidate(
                    request.profile,
                    &executable,
                    &request.selector,
                    request.effort.as_deref(),
                    request.access,
                    &request.workspace,
                    &request.source,
                    &signal,
                    &mut admit,
                ) {
                    Ok(inspection) => match inspection.result {
                        Ok(observation) => Completion {
                            result: Ok(DiscoveryEvidence {
                                observation,
                                executable,
                                version,
                                credentials_present,
                                observed_from,
                            }),
                            cleanup: None,
                        },
                        Err(error) => Completion {
                            result: Err(error),
                            cleanup: Some(inspection.process),
                        },
                    },
                    Err(error) => Completion {
                        result: Err(error),
                        cleanup: None,
                    },
                }
            })
            .map_err(|_| failure())?;
        Ok(Self {
            cancel,
            handle: Some(handle),
        })
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub(crate) fn poll(&mut self) -> Option<Completion> {
        if !self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.is_finished())
        {
            return None;
        }
        let mut completion = self
            .handle
            .take()
            .unwrap()
            .join()
            .unwrap_or_else(|_| Completion {
                result: Err(failure()),
                cleanup: None,
            });
        if self.cancel.load(Ordering::Acquire) && completion.result.is_ok() {
            completion.result = Err(failure());
        }
        Some(completion)
    }
}

impl Drop for DiscoveryWorker {
    fn drop(&mut self) {
        self.cancel();
        // Abnormal fallback only; normal shutdown cancels, polls and transfers
        // the retained process into runtime cleanup rather than detaching it.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub(crate) struct ProfileObservation {
    selector: String,
    resolved_model: String,
    effort: Option<String>,
    access: ToolAccess,
    workspace: PathBuf,
    source: PathBuf,
    home: PathBuf,
    observed_at: Instant,
}

impl ProfileObservation {
    pub(crate) fn selected_model(
        &self,
    ) -> Result<hello_agent_protocol::agent_adapter_v1::AgentModelCapabilityV1, SidecarError> {
        self.resolved_model()?;
        Ok(
            hello_agent_protocol::agent_adapter_v1::AgentModelCapabilityV1 {
                model_selector: self.selector.clone(),
                reasoning_efforts: self.effort.iter().cloned().collect(),
                permissions: vec![match self.access {
                    ToolAccess::ReadOnly => "readOnly",
                    ToolAccess::Edit => "workspaceWrite",
                }
                .into()],
            },
        )
    }

    pub(crate) fn matches(
        &self,
        selector: &str,
        effort: Option<&str>,
        access: ToolAccess,
        workspace: &Path,
        source: &Path,
        home: &Path,
    ) -> bool {
        self.selector == selector
            && self.effort.as_deref() == effort
            && self.access == access
            && self.workspace == workspace
            && self.source == source
            && self.home == home
            && self.observed_at.elapsed() <= Duration::from_secs(30)
    }

    pub(crate) fn resolved_model(&self) -> Result<&str, SidecarError> {
        if self.observed_at.elapsed() > Duration::from_secs(30) {
            return Err(failure());
        }
        Ok(&self.resolved_model)
    }
}

/// Policy admission is independent of initialization. No user prompt is sent,
/// no run fence is fabricated, and no authenticated/canStart claim is returned.
pub(crate) fn inspect_candidate(
    profile: ClaudeProfileBinding,
    executable: &PinnedExecutable,
    selector: &str,
    effort: Option<&str>,
    access: ToolAccess,
    workspace: &Path,
    source: &Path,
    cancel: &AtomicBool,
    admit: &mut impl FnMut(&Path, &Path, &str, Option<&str>, ToolAccess) -> Result<(), SidecarError>,
) -> Result<ProviderInspection<'static, ProfileObservation>, SidecarError> {
    let home = profile.revalidate()?;
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    let command = launch::ClaudeCommand::new(
        executable.revalidate()?,
        &home,
        workspace,
        source,
        selector,
        effort,
        access,
    )?;
    admit(workspace, &home, selector, effort, access)?;
    let mut process = LoginProcess::reserve_claude_owned(profile)?;
    let result = (|| {
        command.launch(&mut process, |child, _| {
            executable.revalidate().map_err(std::io::Error::other)?;
            admit(workspace, &home, selector, effort, access).map_err(std::io::Error::other)?;
            if cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::other("Claude discovery canceled"));
            }
            crate::provider_login_gate::release(child)
        })?;
        let mut initialization = ControlInitialization::start(&mut process, cancel)?;
        loop {
            if initialization.advance(&mut process, cancel, &mut |owner| owner.tick())? {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Catalog plus current mode and hook registration only. system/init
        // target/tool matching still occurs on the actual execution connection.
        let resolved_model = initialization
            .exchange
            .profile_model(selector, effort)
            .map_err(|_| initialization_failure("model, effort, hooks or permission selection"))?
            .to_owned();
        process.verify_claude_scope(workspace, selector, effort, access)?;
        Ok(resolved_model)
    })();
    let cleanup = process.finish();
    let result = result.and_then(|resolved_model| {
        cleanup?;
        executable.revalidate()?;
        if process.closed_claude_home()? != home || cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        Ok(ProfileObservation {
            selector: selector.into(),
            resolved_model,
            effort: effort.map(str::to_owned),
            access,
            workspace: workspace.into(),
            source: source.into(),
            home,
            observed_at: Instant::now(),
        })
    });
    Ok(ProviderInspection { result, process })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_is_bound_to_source_snapshot_profile_and_expiry() {
        let mut observation = ProfileObservation {
            selector: "opus".into(),
            resolved_model: "claude-model".into(),
            effort: Some("high".into()),
            access: ToolAccess::Edit,
            workspace: "/snapshot".into(),
            source: "/source".into(),
            home: "/profile".into(),
            observed_at: Instant::now(),
        };
        let matches = |observation: &ProfileObservation, source: &str| {
            observation.matches(
                "opus",
                Some("high"),
                ToolAccess::Edit,
                Path::new("/snapshot"),
                Path::new(source),
                Path::new("/profile"),
            )
        };
        assert!(matches(&observation, "/source"));
        let model = observation.selected_model().unwrap();
        assert_eq!(model.model_selector, "opus");
        assert_eq!(model.reasoning_efforts, vec!["high"]);
        assert_eq!(model.permissions, vec!["workspaceWrite"]);
        assert!(!matches(&observation, "/other"));
        assert!(!observation.matches(
            "opus",
            Some("high"),
            ToolAccess::ReadOnly,
            Path::new("/snapshot"),
            Path::new("/source"),
            Path::new("/profile")
        ));
        assert!(!observation.matches(
            "claude-model",
            Some("high"),
            ToolAccess::Edit,
            Path::new("/snapshot"),
            Path::new("/source"),
            Path::new("/profile")
        ));
        observation.effort = None;
        assert!(observation
            .selected_model()
            .unwrap()
            .reasoning_efforts
            .is_empty());
        assert!(!matches(&observation, "/source"));
        assert!(observation.matches(
            "opus",
            None,
            ToolAccess::Edit,
            Path::new("/snapshot"),
            Path::new("/source"),
            Path::new("/profile")
        ));
        observation.observed_at = Instant::now() - Duration::from_secs(31);
        assert!(!matches(&observation, "/source"));
        assert!(observation.resolved_model().is_err());
        assert!(observation.selected_model().is_err());
    }
}
