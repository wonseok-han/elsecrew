//! Ownership of already launched Claude processes during initialization.
use super::*;
use crate::provider_login_process::LoginProcess;
use std::sync::atomic::{AtomicBool, Ordering};

type PromptAdmission =
    Box<dyn for<'a> FnMut(&mut LoginProcess<'a>) -> Result<(), SidecarError> + Send>;

pub(crate) struct PendingClaudeStart {
    identity: RunIdentity,
    reservation: Option<preparation::RunReservation>,
    process: Option<LoginProcess<'static>>,
    controller: preparation::ClaudeStart,
    cancel: AtomicBool,
    admit: PromptAdmission,
    tool_admit: Option<preparation::ClaudeToolAdmission>,
    failed: bool,
}

impl PendingClaudeStart {
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub(crate) fn request_cancel(&self, identity: &RunIdentity) -> Result<(), ProjectFailure> {
        if !same_identity(&self.identity, identity) {
            return Err(ProjectFailure::manifest(
                "Claude startup cancellation identity differs",
            ));
        }
        self.cancel();
        Ok(())
    }
}

impl AgentRuntime {
    /// Launch a reserved Claude run and keep initialization asynchronous. Both
    /// policy callbacks come from the trusted caller, never renderer payloads.
    /// Any rejected launch retains its exact process for the common cleanup path.
    #[allow(dead_code)] // Awaiting Claude discovery/policy admission integration.
    pub(crate) fn start_reserved_claude(
        &mut self,
        storage: &mut Storage,
        projects: &ProjectManager,
        reservation: &mut Option<preparation::RunReservation>,
        process: &mut Option<LoginProcess<'static>>,
        executable: &crate::provider_executable::PinnedExecutable,
        cancel: &AtomicBool,
        launch_admit: &mut impl FnMut(
            &std::path::Path,
            &std::path::Path,
            &str,
            &str,
            crate::provider_claude_session::ToolAccess,
        ) -> Result<(), SidecarError>,
        prompt_admit: PromptAdmission,
        tool_admit: preparation::ClaudeToolAdmission,
    ) -> Result<G5Action, SidecarError> {
        let failure = || SidecarError::new("agentLaunchRejected", "Claude start unavailable");
        let pending = reservation.as_ref().ok_or_else(failure)?;
        if self.preparing_to_quit
            || cancel.load(Ordering::Acquire)
            || self.has_active_run(&pending.run_id)
            || active_run_limit_reached(self.owned_run_ids().len())
        {
            return Err(failure());
        }
        let run = &pending.run;
        let accepted = json!({
            "schemaVersion": 1, "runId": run["runId"], "chatId": run["chatId"],
            "taskId": run["taskId"], "projectId": run["projectId"],
            "worktreeId": run["worktreeId"], "workspaceIdentity": run["workspaceIdentity"],
            "roleId": run["selectedProfile"]["roleId"],
            "capabilityFingerprint": run["capabilityFingerprint"], "state": "preparing",
        });
        validate_agent_adapter_payload_v1("runAccepted", &accepted).map_err(|_| failure())?;
        let result = (|| {
            let pending = reservation.as_ref().ok_or_else(failure)?;
            let owner = process.as_mut().ok_or_else(failure)?;
            if !owner.matches_startup_run(&pending.run_id) {
                owner.retain_fence_before_launch(&pending.run_id)?;
            }
            pending.launch_claude_candidate(
                storage,
                projects,
                owner,
                executable,
                cancel,
                launch_admit,
            )?;
            if cancel.load(Ordering::Acquire) {
                return Err(failure());
            }
            self.register_claude_start(storage, projects, reservation, process, prompt_admit, tool_admit)
        })();
        if let Err(error) = result {
            // If cleanup registration rejects, caller slots still own the process.
            self.retain_rejected_start(reservation, process)?;
            return Err(error);
        }
        Ok(G5Action {
            result: accepted,
            events: Vec::new(),
        })
    }

    /// The trusted launch caller supplies admission. Initialization is not a
    /// permission grant; rejection leaves both caller slots available to cleanup.
    #[allow(dead_code)] // Product launch admission/dispatcher is not activated.
    pub(crate) fn register_claude_start(
        &mut self,
        storage: &Storage,
        projects: &ProjectManager,
        reservation: &mut Option<preparation::RunReservation>,
        process: &mut Option<LoginProcess<'static>>,
        admit: PromptAdmission,
        tool_admit: preparation::ClaudeToolAdmission,
    ) -> Result<(), SidecarError> {
        let failure =
            || SidecarError::new("agentRegistrationRejected", "Claude startup unavailable");
        let pending = reservation.as_ref().ok_or_else(failure)?;
        if self.preparing_to_quit
            || active_run_limit_reached(self.owned_run_ids().len())
            || self.has_active_run(&pending.run_id)
        {
            return Err(failure());
        }
        let field = |name| {
            pending
                .run
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(failure)
        };
        let identity = RunIdentity {
            chat_id: field("chatId")?,
            task_id: field("taskId")?,
            project_id: field("projectId")?,
            worktree_id: field("worktreeId")?,
            workspace_identity: field("workspaceIdentity")?,
            run_id: field("runId")?,
        };
        if identity.run_id != pending.run_id {
            return Err(failure());
        }
        let cancel = AtomicBool::new(false);
        let controller = preparation::ClaudeStart::start(
            pending,
            storage,
            projects,
            process.as_mut().ok_or_else(failure)?,
            &cancel,
        )?;
        self.pending_claude_starts.insert(
            identity.run_id.clone(),
            PendingClaudeStart {
                tool_admit: Some(tool_admit),
                identity,
                reservation: reservation.take(),
                process: process.take(),
                controller,
                cancel,
                admit,
                failed: false,
            },
        );
        Ok(())
    }

    pub(crate) fn poll_claude_starts(
        &mut self,
        storage: &mut Storage,
        projects: &ProjectManager,
    ) -> Result<Vec<G5Event>, SidecarError> {
        let mut failure = None;
        let ids: Vec<_> = self.pending_claude_starts.keys().cloned().collect();
        for id in ids {
            // Temporarily remove only while transferring ownership between maps.
            let mut pending = self.pending_claude_starts.remove(&id).unwrap();
            if !pending.failed {
                let result = pending.controller.advance(
                    storage,
                    projects,
                    &mut pending.reservation,
                    &mut pending.process,
                    &pending.cancel,
                    &mut |process| (pending.admit)(process),
                );
                match result {
                    Ok(None) => {
                        self.pending_claude_starts.insert(id, pending);
                        continue;
                    }
                    Ok(Some(mut owner)) => {
                        if let Some(admission) = pending.tool_admit.take() {
                            owner.set_tool_admission(admission);
                        }
                        let mut slot = Some(owner);
                        match self.register_claude_candidate(storage, projects, &mut slot) {
                            Ok(()) => continue,
                            Err(error) => {
                                let (reservation, process) = slot.unwrap().into_cleanup_parts();
                                pending.reservation = Some(reservation);
                                pending.process = Some(process);
                                failure = Some(error);
                            }
                        }
                    }
                    Err(error) => failure = Some(error),
                }
                pending.failed = true;
            }
            // Failed initialization/prompt admission is never retried. Existing
            // cleanup owns termination, terminal persistence and fence release.
            if let Err(error) =
                self.register_failed_start(&mut pending.reservation, &mut pending.process)
            {
                failure = Some(error);
                self.pending_claude_starts.insert(id, pending);
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(Vec::new()),
        }
    }
}
