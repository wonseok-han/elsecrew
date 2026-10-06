//! Owned, cancellable model-free discovery. No storage borrow crosses threads.
use super::*;
use crate::{provider_executable::PinnedExecutable, provider_profile::CodexProfileBinding};
use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
use std::{path::PathBuf, sync::Arc, thread::JoinHandle};

pub(crate) struct Evidence {
    pub(crate) version: String,
    pub(crate) observation: NamedProfileObservation,
    pub(crate) executable: PinnedExecutable,
    pub(crate) helper: PinnedExecutable,
}

pub(crate) struct Completion {
    pub(crate) result: Result<Evidence, SidecarError>,
    /// Must be transferred to runtime cleanup on failure, never to a new run.
    pub(crate) cleanup: Option<LoginProcess<'static>>,
}

impl Completion {
    fn failed(error: SidecarError, cleanup: Option<LoginProcess<'static>>) -> Self {
        Self {
            result: Err(error),
            cleanup,
        }
    }
}

pub(crate) struct DiscoveryWorker {
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<Completion>>,
}

impl DiscoveryWorker {
    pub(crate) fn spawn(
        profile: CodexProfileBinding,
        executable: PinnedExecutable,
        helper: PinnedExecutable,
        model: String,
        effort: String,
        permission: ExpectedPermission,
        workspace: PathBuf,
    ) -> Result<Self, SidecarError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let handle = std::thread::Builder::new()
            .name("codex-discovery".into())
            .spawn(move || {
                let version_profile = match profile.try_clone() {
                    Ok(profile) => profile,
                    Err(error) => return Completion::failed(error, None),
                };
                let version = match inspect_codex_version(version_profile, &executable, &signal) {
                    Ok(inspection) => match inspection.result {
                        Ok(version) => version, // Successful inspection already closed its owner.
                        Err(error) => return Completion::failed(error, Some(inspection.process)),
                    },
                    Err(error) => return Completion::failed(error, None),
                };
                if signal.load(Ordering::Acquire) {
                    return Completion::failed(failure(), None);
                }
                let expected = ExpectedProfile {
                    model: &model,
                    reasoning_effort: &effort,
                };
                match inspect_named_preflight_candidate(
                    profile,
                    &executable,
                    &helper,
                    &expected,
                    permission,
                    &workspace,
                    &signal,
                ) {
                    Ok(inspection) => match inspection.result {
                        Ok(observation) => Completion {
                            result: Ok(Evidence {
                                version,
                                observation,
                                executable,
                                helper,
                            }),
                            cleanup: None,
                        },
                        Err(error) => Completion::failed(error, Some(inspection.process)),
                    },
                    Err(error) => Completion::failed(error, None),
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

    /// Never joins a running worker on the normal dispatch path.
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
            .unwrap_or_else(|_| Completion::failed(failure(), None));
        if self.cancel.load(Ordering::Acquire) && completion.result.is_ok() {
            completion.result = Err(failure());
        }
        Some(completion)
    }
}

impl Drop for DiscoveryWorker {
    fn drop(&mut self) {
        self.cancel();
        // Abnormal fallback only. Normal shutdown cancels and polls, then hands
        // any uncertain owner to runtime cleanup. Never detach a live worker.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
