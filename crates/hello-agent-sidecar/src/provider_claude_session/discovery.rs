//! Model-free selected-profile catalog observation, never account entitlement.
//! Call on a worker; every failed inspection returns its original cleanup owner.
use super::*;
use crate::{
    provider_claude_profile::ClaudeProfileBinding, provider_executable::PinnedExecutable,
    provider_preflight_session::ProviderInspection, provider_profile::LoginProfile,
};
use std::path::{Path, PathBuf};
use std::{sync::Arc, thread::JoinHandle};

pub(crate) struct DiscoveryRequest {
    pub(crate) profile: ClaudeProfileBinding,
    pub(crate) selector: String,
    pub(crate) effort: String,
    pub(crate) access: ToolAccess,
    pub(crate) workspace: PathBuf,
    pub(crate) source: PathBuf,
}

pub(crate) struct DiscoveryEvidence {
    pub(crate) observation: ProfileObservation,
    pub(crate) executable: PinnedExecutable,
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
        mut admit: impl FnMut(&Path, &Path, &str, &str, ToolAccess) -> Result<(), SidecarError>
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
                match inspect_candidate(
                    request.profile,
                    &executable,
                    &request.selector,
                    &request.effort,
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
    effort: String,
    access: ToolAccess,
    workspace: PathBuf,
    source: PathBuf,
    home: PathBuf,
    observed_at: Instant,
}

impl ProfileObservation {
    pub(crate) fn matches(
        &self,
        selector: &str,
        effort: &str,
        access: ToolAccess,
        workspace: &Path,
        source: &Path,
        home: &Path,
    ) -> bool {
        self.selector == selector
            && self.effort == effort
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
    effort: &str,
    access: ToolAccess,
    workspace: &Path,
    source: &Path,
    cancel: &AtomicBool,
    admit: &mut impl FnMut(&Path, &Path, &str, &str, ToolAccess) -> Result<(), SidecarError>,
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
            .map_err(|_| failure())?
            .to_owned();
        if !process.matches_claude_scope(workspace, selector, effort, access) {
            return Err(failure());
        }
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
            effort: effort.into(),
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
            effort: "high".into(),
            access: ToolAccess::Edit,
            workspace: "/snapshot".into(),
            source: "/source".into(),
            home: "/profile".into(),
            observed_at: Instant::now(),
        };
        let matches = |observation: &ProfileObservation, source: &str| {
            observation.matches(
                "opus",
                "high",
                ToolAccess::Edit,
                Path::new("/snapshot"),
                Path::new(source),
                Path::new("/profile"),
            )
        };
        assert!(matches(&observation, "/source"));
        assert!(!matches(&observation, "/other"));
        assert!(!observation.matches(
            "opus",
            "high",
            ToolAccess::ReadOnly,
            Path::new("/snapshot"),
            Path::new("/source"),
            Path::new("/profile")
        ));
        assert!(!observation.matches(
            "claude-model",
            "high",
            ToolAccess::Edit,
            Path::new("/snapshot"),
            Path::new("/source"),
            Path::new("/profile")
        ));
        observation.observed_at = Instant::now() - Duration::from_secs(31);
        assert!(!matches(&observation, "/source"));
        assert!(observation.resolved_model().is_err());
    }
}
