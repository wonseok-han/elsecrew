//! Display-only, project-independent model discovery. No user prompt or run reservation.
use super::*;
use crate::provider_profile::{open_absolute_directory, same_directory, LoginProfile};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt};
use hello_agent_core::provider::vendor_observation::codex_preflight::{
    CatalogModel, ExpectedPermission,
};

enum Worker {
    #[cfg(test)]
    CompletedFixture,
    Codex(crate::provider_preflight_session::discovery_worker::DiscoveryWorker),
    Claude(crate::provider_claude_session::discovery::DiscoveryWorker),
}
pub(super) struct PendingCatalog {
    worker: Worker,
    usage: Option<serde_json::Value>,
    cleanup_only: bool,
    completion: Option<(
        Result<Vec<CatalogModel>, SidecarError>,
        Option<crate::provider_login_process::LoginProcess<'static>>,
    )>,
    // Retain fresh empty input folders until the owned query has terminated.
    parent: Dir,
    root: Dir,
    name: String,
    path: PathBuf,
}
impl PendingCatalog {
    pub(super) fn cancel(&self) {
        match &self.worker {
            #[cfg(test)]
            Worker::CompletedFixture => (),
            Worker::Codex(worker) => worker.cancel(),
            Worker::Claude(worker) => worker.cancel(),
        }
    }
    fn clean_empty(&self) -> Result<(), SidecarError> {
        if !same_directory(&self.root, &open_absolute_directory(&self.path)?)? {
            return Err(failure());
        }
        for name in ["workspace", "source"] {
            if let Err(error) = self.root.remove_dir(name) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(failure());
                }
            }
        }
        self.parent.remove_dir(&self.name).map_err(|_| failure())
    }
}
fn failure() -> SidecarError {
    SidecarError::new("agentDiscoveryRejected", "model catalog unavailable")
}
impl AgentRuntime {
    pub(crate) fn begin_model_catalog(
        &mut self,
        storage: &Storage,
        request_id: &str,
        runtime: &str,
    ) -> Result<(), SidecarError> {
        if self.preparing_to_quit
            || !self.owned_run_ids().is_empty()
            || self.pending_catalogs.contains_key(request_id)
        {
            return Err(failure());
        }
        if !matches!(runtime, "codex" | "claude-code") {
            return Err(failure());
        }
        let base = storage.path().parent().ok_or_else(failure)?;
        let parent = open_absolute_directory(base)?;
        let name = format!("model-catalog-{}", Uuid::new_v4().simple());
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        parent
            .create_dir_with(&name, &builder)
            .map_err(|_| failure())?;
        let path = base.join(&name);
        let root = open_absolute_directory(&path)?;
        root.create_dir_with("workspace", &builder)
            .map_err(|_| failure())?;
        root.create_dir_with("source", &builder)
            .map_err(|_| failure())?;
        let workspace = path.join("workspace");
        let source = path.join("source");
        let worker = (|| -> Result<Worker, SidecarError> {
            Ok(if runtime == "codex" {
                // The catalog is retained even when this probe selector is unavailable.
                Worker::Codex(
                    crate::provider_preflight_session::discovery_worker::DiscoveryWorker::spawn_catalog(
                        crate::provider_profile::CodexProfileBinding::from_storage(storage)?,
                        "catalog-probe".into(),
                        "medium".into(),
                        ExpectedPermission::ReadOnly,
                        workspace,
                    )?,
                )
            } else {
                use crate::provider_claude_session::{
                    admission::Admission,
                    discovery::{DiscoveryRequest, DiscoveryWorker},
                    ToolAccess,
                };
                let profile =
                    crate::provider_claude_profile::ClaudeProfileBinding::from_storage(storage)?;
                let admission = Admission::new(
                    &workspace,
                    &profile.revalidate()?,
                    &source,
                    "opus",
                    None,
                    ToolAccess::ReadOnly,
                )?;
                Worker::Claude(DiscoveryWorker::spawn(
                    DiscoveryRequest {
                        profile,
                        selector: "opus".into(),
                        effort: None,
                        access: ToolAccess::ReadOnly,
                        workspace,
                        source,
                    },
                    move |workspace, home, model, effort, access| {
                        admission.launch(workspace, home, model, effort, access)
                    },
                )?)
            })
        })();
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                let _ = root.remove_dir("workspace");
                let _ = root.remove_dir("source");
                let _ = parent.remove_dir(&name);
                return Err(error);
            }
        };
        self.pending_catalogs.insert(
            request_id.into(),
            PendingCatalog {
                worker,
                usage: None,
                cleanup_only: false,
                completion: None,
                parent,
                root,
                name,
                path,
            },
        );
        Ok(())
    }
    pub(super) fn poll_catalog_cleanup(&mut self) -> Result<Vec<G5Event>, SidecarError> {
        let ids: Vec<_> = self
            .pending_catalogs
            .iter()
            .filter(|(_, pending)| pending.cleanup_only)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let pending = self.pending_catalogs.get_mut(&id).unwrap();
            let owner = pending
                .completion
                .as_mut()
                .and_then(|(_, owner)| owner.as_mut());
            let confirmed = match owner {
                None => Ok(true),
                Some(owner)
                    if owner
                        .closure_evidence()
                        .is_some_and(|receipt| receipt.confirmed()) =>
                {
                    Ok(true)
                }
                Some(owner) => owner.poll_finish().and_then(|finished| {
                    if !finished {
                        return Ok(false);
                    }
                    if owner
                        .closure_evidence()
                        .is_some_and(|receipt| receipt.confirmed())
                    {
                        Ok(true)
                    } else {
                        Err(SidecarError::new(
                            "agentRecoveryRequired",
                            "model catalog cleanup is unconfirmed",
                        ))
                    }
                }),
            };
            match confirmed {
                Ok(true) => {
                    if pending.clean_empty().is_ok() {
                        self.pending_catalogs.remove(&id);
                    }
                }
                // The request already reported recovery required. Keep the
                // service and its quit blocker alive until cleanup is proven.
                Ok(false) | Err(_) => {}
            }
        }
        Ok(Vec::new())
    }

    pub(crate) fn take_model_catalog(
        &mut self,
        request_id: &str,
    ) -> Option<Result<(Vec<CatalogModel>, Option<serde_json::Value>), SidecarError>> {
        let pending = self.pending_catalogs.get_mut(request_id)?;
        if pending.cleanup_only {
            return None;
        }
        if pending.completion.is_none() {
            let done = match &mut pending.worker {
                #[cfg(test)]
                Worker::CompletedFixture => return None,
                Worker::Codex(worker) => {
                    let done = worker.poll()?;
                    pending.usage = done.display_usage;
                    (
                        done.display_models
                            .ok_or_else(|| done.result.err().unwrap_or_else(failure)),
                        done.cleanup,
                    )
                }
                Worker::Claude(worker) => {
                    let done = worker.poll()?;
                    (
                        done.display_models
                            .ok_or_else(|| done.result.err().unwrap_or_else(failure)),
                        done.cleanup,
                    )
                }
            };
            pending.completion = Some(done);
        }
        let (result, mut cleanup) = pending.completion.take().unwrap();
        // Keep an uncertain process AND its input directories under this owner.
        // Never discard the folder association while shutdown is still pending.
        if let Some(owner) = cleanup.as_mut() {
            let confirmed = owner
                .closure_evidence()
                .is_some_and(|receipt| receipt.confirmed())
                || (matches!(owner.poll_finish(), Ok(true))
                    && owner
                        .closure_evidence()
                        .is_some_and(|receipt| receipt.confirmed()));
            if !confirmed {
                self.pending_catalogs
                    .get_mut(request_id)
                    .unwrap()
                    .completion = Some((result, cleanup));
                self.pending_catalogs
                    .get_mut(request_id)
                    .unwrap()
                    .cleanup_only = true;
                return Some(Err(SidecarError::new(
                    "agentRecoveryRequired",
                    "model catalog process termination is unconfirmed",
                )));
            }
        }
        drop(cleanup);
        let pending = self.pending_catalogs.remove(request_id).unwrap();
        if let Err(error) = pending.clean_empty() {
            return Some(Err(error));
        }
        if self.preparing_to_quit {
            return Some(Err(failure()));
        }
        Some(result.map(|models| (models, pending.usage)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_is_owned_until_consumed_and_empty_inputs_are_cleaned_after_completion() {
        for quitting in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let base = temp.path().canonicalize().unwrap();
            let parent = open_absolute_directory(&base).unwrap();
            parent.create_dir("query").unwrap();
            let path = base.join("query");
            let root = open_absolute_directory(&path).unwrap();
            root.create_dir("workspace").unwrap();
            root.create_dir("source").unwrap();
            let mut runtime = AgentRuntime::blocked();
            runtime.preparing_to_quit = quitting;
            runtime.pending_catalogs.insert(
                "query".into(),
                PendingCatalog {
                    worker: Worker::CompletedFixture,
                usage: None,
                    cleanup_only: false,
                    parent,
                    root,
                    path: path.clone(),
                    name: "query".into(),
                    completion: Some((
                        Ok(vec![CatalogModel {
                            model: "model".into(),
                            reasoning_efforts: vec![],
                            is_default: true,
                        }]),
                        None,
                    )),
                },
            );
            assert!(runtime.owned_run_ids().contains(&"query".into()));
            let completion = runtime
                .pending_catalogs
                .get_mut("query")
                .unwrap()
                .completion
                .take();
            assert!(runtime.take_model_catalog("query").is_none());
            assert!(path.join("workspace").is_dir());
            assert!(runtime.owned_run_ids().contains(&"query".into()));
            runtime
                .pending_catalogs
                .get_mut("query")
                .unwrap()
                .completion = completion;
            assert_eq!(
                runtime.take_model_catalog("query").unwrap().is_ok(),
                !quitting
            );
            assert!(runtime.owned_run_ids().is_empty());
            assert!(!path.exists());
            assert!(runtime.take_model_catalog("query").is_none());
        }
    }
    #[test]
    fn uncertain_catalog_cleanup_replies_with_error_and_retains_owner_and_directories() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&base.join("hello-agent.sqlite3")).unwrap();
        let binding = crate::provider_profile::CodexProfileBinding::from_storage(&storage).unwrap();
        let mut process =
            crate::provider_login_process::LoginProcess::reserve_codex_owned(binding).unwrap();
        process.inject_uncertain_cleanup_once();
        let parent = open_absolute_directory(&base).unwrap();
        parent.create_dir("query").unwrap();
        let path = base.join("query");
        let root = open_absolute_directory(&path).unwrap();
        root.create_dir("workspace").unwrap();
        root.create_dir("source").unwrap();
        let mut runtime = AgentRuntime::blocked();
        runtime.pending_catalogs.insert(
            "query".into(),
            PendingCatalog {
                worker: Worker::CompletedFixture,
                usage: None,
                cleanup_only: false,
                parent,
                root,
                name: "query".into(),
                path: path.clone(),
                completion: Some((Ok(vec![]), Some(process))),
            },
        );
        let error = runtime.take_model_catalog("query").unwrap().unwrap_err();
        assert_eq!(error.kind(), "agentRecoveryRequired");
        assert!(runtime.take_model_catalog("query").is_none()); // reply consumed, cleanup still independently owned
        assert!(runtime.poll_catalog_cleanup().is_ok());
        assert!(runtime.owned_run_ids().contains(&"query".into()));
        assert!(path.join("workspace").is_dir());
        assert!(runtime.pending_catalogs["query"].cleanup_only);
    }

    #[test]
    fn independently_polled_closed_catalog_cleanup_removes_its_exact_tree() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&base.join("hello-agent.sqlite3")).unwrap();
        let binding = crate::provider_profile::CodexProfileBinding::from_storage(&storage).unwrap();
        let process =
            crate::provider_login_process::LoginProcess::reserve_codex_owned(binding).unwrap();
        let parent = open_absolute_directory(&base).unwrap();
        parent.create_dir("query").unwrap();
        let path = base.join("query");
        let root = open_absolute_directory(&path).unwrap();
        root.create_dir("workspace").unwrap();
        root.create_dir("source").unwrap();
        let mut runtime = AgentRuntime::blocked();
        runtime.pending_catalogs.insert(
            "query".into(),
            PendingCatalog {
                worker: Worker::CompletedFixture,
                usage: None,
                cleanup_only: true,
                parent,
                root,
                name: "query".into(),
                path: path.clone(),
                completion: Some((Ok(vec![]), Some(process))),
            },
        );
        runtime.poll_catalog_cleanup().unwrap();
        assert!(runtime.owned_run_ids().is_empty());
        assert!(!path.exists());
    }
    #[test]
    fn catalog_cleanup_failure_is_nonfatal_through_sidecar_poll_and_replies_once() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let mut session =
            crate::SidecarSession::new("catalog-test".into(), AgentRuntime::blocked());
        session.start_admission(&base.join("hello-agent.sqlite3"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            session.poll_admission();
            if !matches!(session.admission, crate::AdmissionState::Verifying(_)) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let crate::AdmissionState::Ready { sidecar, .. } = &mut session.admission else {
            panic!("admission failed")
        };
        let binding =
            crate::provider_profile::CodexProfileBinding::from_storage(&sidecar.storage).unwrap();
        let mut process =
            crate::provider_login_process::LoginProcess::reserve_codex_owned(binding).unwrap();
        process.inject_uncertain_cleanup_once();
        let parent = open_absolute_directory(&base).unwrap();
        parent.create_dir("query").unwrap();
        let path = base.join("query");
        let root = open_absolute_directory(&path).unwrap();
        root.create_dir("workspace").unwrap();
        root.create_dir("source").unwrap();
        sidecar.agent_runtime.pending_catalogs.insert(
            "query".into(),
            PendingCatalog {
                worker: Worker::CompletedFixture,
                usage: None,
                cleanup_only: false,
                parent,
                root,
                name: "query".into(),
                path: path.clone(),
                completion: Some((Ok(vec![]), Some(process))),
            },
        );
        sidecar.pending_agent_capabilities.insert(
            "query".into(),
            hello_agent_protocol::RequestV1 {
                protocol_version: hello_agent_protocol::PROTOCOL_VERSION,
                kind: "request".into(),
                sidecar_instance_id: "catalog-test".into(),
                request_id: "query".into(),
                method: "agent.getModelChoices".into(),
                params: json!({"runtime":"codex","refresh":true}),
            },
        );
        let (responses, _) = sidecar.poll_runtime_events().unwrap();
        assert_eq!(responses.len(), 1);
        assert!(!responses[0].ok);
        assert_eq!(
            responses[0].error.as_ref().unwrap().code,
            "agentRecoveryRequired"
        );
        for _ in 0..3 {
            assert!(sidecar.poll_runtime_events().unwrap().0.is_empty());
        }
        assert!(sidecar
            .agent_runtime
            .owned_run_ids()
            .contains(&"query".into()));
        assert!(path.join("workspace").is_dir());
    }
}
