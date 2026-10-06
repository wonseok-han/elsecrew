//! Public-output/terminal bridge for an independently admitted Claude channel.
//! No launch, prompt, or tool grant authority is created here.
use super::*;
use crate::provider_claude_session::SessionChannel;
use crate::provider_claude_session::ControlInitialization;
use hello_agent_core::provider::vendor_observation::{
    claude_events::{Projection, ReportedOutcome},
    claude_session::Event,
    codex_events::stream::TurnOutcome,
};

pub(crate) struct ClaudeOutputState {
    run_id: String,
    connection: Uuid,
    initialized: bool,
    pending_answer: Option<String>,
    completion: Option<Result<TurnOutcome, SidecarError>>,
    terminal_saved: bool,
    finished: bool,
}

/// Owns the reservation, channel and cleanup together across dispatcher polls.
/// Construction and prompt queueing require a separately admitted launch.
pub(crate) struct OwnedClaudeRun {
    reservation: RunReservation,
    process: crate::provider_login_process::LoginProcess<'static>,
    channel: SessionChannel,
    output: ClaudeOutputState,
    cancel: std::sync::atomic::AtomicBool,
    approvals: Vec<ClaudePending>,
}

struct ClaudePending {
    id: String,
    request: hello_agent_core::provider::vendor_observation::claude_approval::Request,
    declined: bool,
}

/// The caller retains reservation/process slots until a fully prepared owner is
/// returned. Any error leaves those slots available for register_failed_start.
pub(crate) struct ClaudeStart {
    connection: Uuid,
    run_id: String,
    initialization: Option<ControlInitialization>,
    channel: Option<SessionChannel>,
    failed: bool,
    handed_off: bool,
}

impl ClaudeStart {
    pub(crate) fn start(
        reservation: &RunReservation,
        storage: &Storage,
        projects: &ProjectManager,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Self, SidecarError> {
        reservation.verify_claude_launch(storage, projects, process)?;
        let initialization = ControlInitialization::start(process, cancel)?;
        Ok(Self { connection:process.connection_id(), run_id:reservation.run_id.clone(),
            initialization:Some(initialization), channel:None, failed:false, handed_off:false })
    }

    /// One nonblocking initialization tick, then exactly one stored prompt queue.
    pub(crate) fn advance(
        &mut self,
        storage: &mut Storage,
        projects: &ProjectManager,
        reservation: &mut Option<RunReservation>,
        process: &mut Option<crate::provider_login_process::LoginProcess<'static>>,
        cancel: &std::sync::atomic::AtomicBool,
        admit: &mut impl FnMut(&mut crate::provider_login_process::LoginProcess<'_>) -> Result<(), SidecarError>,
    ) -> Result<Option<OwnedClaudeRun>, SidecarError> {
        let result = (|| {
            let failure = || SidecarError::new("agentInputChanged", "Claude initialization owner differs");
            if self.failed || self.handed_off || cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err(failure());
            }
            let pending = reservation.as_ref().ok_or_else(failure)?;
            let running = process.as_mut().ok_or_else(failure)?;
            if self.connection != running.connection_id() || self.run_id != pending.run_id { return Err(failure()); }
            let (workspace, access) = pending.verify_claude_launch(storage, projects, running)?;
            let initialization = self.initialization.as_mut().ok_or_else(failure)?;
            if !initialization.advance(running, cancel, &mut |process| pending.observe_process(storage, process))? {
                return Ok(None);
            }
            let expected = hello_agent_core::provider::vendor_observation::claude_init::ExpectedInit {
                cwd:&workspace,
                model:pending.run["selectedProfile"]["modelSelector"].as_str().ok_or_else(failure)?,
                permission_mode:"default",
                tools:crate::provider_claude_session::expected_tools(access),
            };
            self.channel = Some(self.initialization.take().ok_or_else(failure)?.into_session_channel(&expected)?);
            let mut owner = OwnedClaudeRun::take_prepared(storage, projects, reservation, process, &mut self.channel)?;
            // After this point every failure must restore ownership, not drop it.
            let queued = if cancel.load(std::sync::atomic::Ordering::Acquire) {
                Err(failure())
            } else {
                owner.queue_prompt(storage, projects, admit)
            };
            if let Err(error) = queued {
                let (pending, running) = owner.into_cleanup_parts();
                *reservation = Some(pending); *process = Some(running);
                return Err(error);
            }
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                let (pending, running) = owner.into_cleanup_parts();
                *reservation = Some(pending); *process = Some(running);
                return Err(failure());
            }
            self.handed_off = true;
            Ok(Some(owner))
        })();
        if result.is_err() { self.failed = true; }
        result
    }
}

impl OwnedClaudeRun {
    pub(crate) fn approval_request(&mut self, action: &str, id: Option<&str>, identity: &Value) -> Result<Value, SidecarError> {
        let failure = || SidecarError::new("agentApprovalChanged", "Claude approval is no longer live");
        self.approvals.retain(|entry| self.channel.is_permission_pending(&entry.request.request_id));
        let mut inspection = None;
        let mut queued = None;
        if action != "list" {
            let id = id.ok_or_else(failure)?;
            let entry = self.approvals.iter_mut().find(|entry| entry.id == id && !entry.declined).ok_or_else(failure)?;
            match action {
                "decline" => { entry.declined = true; queued = Some(id); },
                "inspect" => inspection = Some(json!({"approvalId":id,"kind":"tool","command":null,
                    "cwd":null,"reason":entry.request.tool_name,"grantRoot":null,"host":null,"protocol":null,
                    "toolInput":entry.request.inspect_input()})),
                _ => return Err(failure()),
            }
        }
        let pending: Vec<_> = self.approvals.iter().filter(|entry| !entry.declined)
            .map(|entry| json!({"approvalId":entry.id,"kind":"tool"})).collect();
        let mut result = json!({"identity":identity,"pending":pending,"queued":queued});
        if let Some(inspection) = inspection { result["inspection"] = inspection; }
        Ok(result)
    }

    pub(crate) fn connection_id(&self) -> Uuid { self.process.connection_id() }

    pub(crate) fn registration_identity(&self, storage: &Storage, projects: &ProjectManager) -> Result<RunIdentity, SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "Claude registration binding differs");
        if self.output.finished || self.output.connection != self.process.connection_id()
            || self.output.run_id != self.reservation.run_id
            || !self.process.matches_startup_run(&self.reservation.run_id) { return Err(failure()); }
        self.reservation.validate_text_input(storage, projects).map_err(|_| failure())?;
        let run = &self.reservation.run;
        let field = |name| run.get(name).and_then(Value::as_str).map(str::to_owned).ok_or_else(failure);
        Ok(RunIdentity {
            chat_id:field("chatId")?, task_id:field("taskId")?, project_id:field("projectId")?,
            worktree_id:field("worktreeId")?, workspace_identity:field("workspaceIdentity")?, run_id:field("runId")?,
        })
    }

    pub(crate) fn take_prepared(
        storage: &Storage,
        projects: &ProjectManager,
        reservation: &mut Option<RunReservation>,
        process: &mut Option<crate::provider_login_process::LoginProcess<'static>>,
        channel: &mut Option<SessionChannel>,
    ) -> Result<Self, SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "Claude owner is unavailable");
        let output = reservation
            .as_ref()
            .ok_or_else(failure)?
            .begin_claude_output(
                storage,
                projects,
                process.as_ref().ok_or_else(failure)?,
                channel.as_ref().ok_or_else(failure)?,
            )?;
        Ok(Self {
            reservation: reservation.take().unwrap(),
            process: process.take().unwrap(),
            channel: channel.take().unwrap(),
            output,
            cancel: std::sync::atomic::AtomicBool::new(false),
            approvals: Vec::new(),
        })
    }

    pub(crate) fn queue_prompt(
        &mut self,
        storage: &mut Storage,
        projects: &ProjectManager,
        admit: &mut impl FnMut(
            &mut crate::provider_login_process::LoginProcess<'_>,
        ) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "Claude prompt binding differs");
        self.reservation
            .validate_text_input(storage, projects)
            .map_err(|_| failure())?;
        self.reservation
            .observe_process(storage, &mut self.process)?;
        let address = address_from_chat(&self.reservation.run).map_err(|_| failure())?;
        let workspace =
            crate::provider_preflight_session::TextWorkspace::new(projects, storage, &address)?;
        let text = std::str::from_utf8(&self.reservation.prompt).map_err(|_| failure())?;
        let result = self.channel.queue_initial_prompt(
            &mut self.process,
            &workspace,
            text,
            &self.cancel,
            admit,
        );
        if result.is_err() {
            self.cancel
                .store(true, std::sync::atomic::Ordering::Release);
        }
        result
    }

    pub(crate) fn request_cancel(&self, identity: &RunIdentity) -> Result<(), SidecarError> {
        if self.output.finished || !identity_value(identity)
            .as_object()
            .unwrap()
            .iter()
            .all(|(key, value)| self.reservation.run.get(key) == Some(value))
        {
            return Err(SidecarError::new(
                "agentInputChanged",
                "Claude cancel identity differs",
            ));
        }
        self.cancel
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    pub(crate) fn advance(
        &mut self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        handle: &mut impl FnMut(
            &mut crate::provider_login_process::LoginProcess<'_>,
            &mut SessionChannel,
            Event,
        ) -> Result<(), SidecarError>,
    ) -> Result<bool, SidecarError> {
        self.approvals.retain(|entry| self.channel.is_permission_pending(&entry.request.request_id));
        if self.channel.can_queue_reply() {
            if let Some(index) = self.approvals.iter().position(|entry| entry.declined) {
                let entry = self.approvals.remove(index);
                if let Err(error) = self.channel.deny(&mut self.process, &entry.request.request_id, false) {
                    self.output.completion = Some(Err(error));
                }
            }
        }
        let approvals = &mut self.approvals;
        self.reservation.advance_claude_output(
            storage,
            projects,
            &mut self.process,
            &mut self.channel,
            &self.cancel,
            &mut self.output,
            &mut |process, channel, event| {
                use hello_agent_core::provider::vendor_observation::claude_approval::Observation;
                match event {
                    Event::Permission(Observation::Requested(request) | Observation::ToolCheck(request)) => {
                        if approvals.len() >= 128 || approvals.iter().any(|entry| entry.request.request_id == request.request_id) {
                            return Err(SidecarError::new("agentApprovalChanged", "Claude approval queue is invalid"));
                        }
                        approvals.push(ClaudePending {id:format!("approval_{}", Uuid::new_v4().simple()),request,declined:false});
                        Ok(())
                    },
                    Event::Permission(Observation::Cancelled) => { approvals.retain(|entry| channel.is_permission_pending(&entry.request.request_id)); Ok(()) },
                    Event::Output(Projection::RequiresHandling) => Ok(()), // Private blocks confer no grant.
                    event => handle(process, channel, event),
                }
            },
        )
    }

    pub(crate) fn into_cleanup_parts(
        self,
    ) -> (
        RunReservation,
        crate::provider_login_process::LoginProcess<'static>,
    ) {
        (self.reservation, self.process)
    }
}

impl RunReservation {
    /// Recheck invocation identity and durable inputs during initialization.
    /// The returned cwd/access are not an execution-confinement grant.
    fn verify_claude_launch(
        &self, storage: &Storage, projects: &ProjectManager,
        process: &crate::provider_login_process::LoginProcess<'_>,
    ) -> Result<(String, crate::provider_claude_session::ToolAccess), SidecarError> {
        use crate::provider_claude_session::ToolAccess;
        let failure = || SidecarError::new("agentInputChanged", "Claude launch binding differs");
        if self.run["selectedProfile"]["runtime"] != "claude-code" || !process.matches_startup_run(&self.run_id) { return Err(failure()); }
        self.validate_text_input(storage, projects).map_err(|_| failure())?;
        let access = match self.run["selectedProfile"]["permission"].as_str() {
            Some("readOnly") => ToolAccess::ReadOnly,
            Some("workspaceWrite") => ToolAccess::Edit,
            _ => return Err(failure()),
        };
        let manifest = projects.verify_workspace_for_run(storage, &address_from_chat(&self.run).map_err(|_| failure())?).map_err(|_| failure())?;
        let workspace = manifest["worktreePath"].as_str().ok_or_else(failure)?;
        if !process.matches_claude_scope(Path::new(workspace),
            self.run["selectedProfile"]["modelSelector"].as_str().ok_or_else(failure)?,
            self.run["selectedProfile"]["reasoningEffort"].as_str().ok_or_else(failure)?, access) { return Err(failure()); }
        Ok((workspace.into(),access))
    }

    /// Prepare the vendor parent from durable inputs. The trusted caller must
    /// separately admit managed settings and tool isolation before the gate opens.
    pub(crate) fn launch_claude_candidate(
        &self,
        storage: &mut Storage,
        projects: &ProjectManager,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
        executable: &crate::provider_executable::PinnedExecutable,
        cancel: &std::sync::atomic::AtomicBool,
        admit: &mut impl FnMut(&Path, &Path, &str, &str, crate::provider_claude_session::ToolAccess) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        use crate::provider_claude_session::{launch::ClaudeCommand, ToolAccess};
        use std::sync::atomic::Ordering;
        let failure = || SidecarError::new("agentLaunchRejected", "Claude launch inputs differ");
        if self.run["selectedProfile"]["runtime"] != "claude-code"
            || !process.matches_startup_run(&self.run_id) || cancel.load(Ordering::Acquire)
        { return Err(failure()); }
        let selected = &self.run["selectedProfile"];
        let model = selected["modelSelector"].as_str().ok_or_else(failure)?;
        let effort = selected["reasoningEffort"].as_str().ok_or_else(failure)?;
        let access = match selected["permission"].as_str() {
            Some("readOnly") => ToolAccess::ReadOnly,
            Some("workspaceWrite") => ToolAccess::Edit,
            _ => return Err(failure()),
        };
        let address = address_from_chat(&self.run).map_err(|_| failure())?;
        projects.verify_runtime_file_isolation(storage, &address).map_err(|_| failure())?;
        self.validate_text_input(storage, projects).map_err(|_| failure())?;
        let manifest = projects.verify_workspace_for_run(storage, &address).map_err(|_| failure())?;
        let workspace = Path::new(manifest["worktreePath"].as_str().ok_or_else(failure)?);
        let home = process.claude_preflight_home()?;
        let command = ClaudeCommand::new(executable.revalidate()?, &home, workspace, model, effort, access)?;
        admit(workspace, &home, model, effort, access)?;
        if cancel.load(Ordering::Acquire) { return Err(failure()); }
        command.launch(process, |child, identities| {
            // Recheck both policy and durable input at the last pre-vendor boundary.
            admit(workspace, &home, model, effort, access).map_err(std::io::Error::other)?;
            executable.revalidate().map_err(std::io::Error::other)?;
            if cancel.load(Ordering::Acquire) { return Err(std::io::Error::other("Claude launch canceled")); }
            self.record_pre_release(storage, projects, identities)
                .map_err(|_| std::io::Error::other("Claude run identity persistence failed"))?;
            crate::provider_login_gate::release(child)
        })?;
        self.record_released_or_invalidate(storage, process)
    }

    pub(crate) fn begin_claude_output(
        &self,
        storage: &Storage,
        projects: &ProjectManager,
        process: &crate::provider_login_process::LoginProcess<'_>,
        channel: &SessionChannel,
    ) -> Result<ClaudeOutputState, SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "Claude output owner differs");
        let (workspace, access) = self.verify_claude_launch(storage, projects, process)?;
        if !channel.matches_role(
            self.run["selectedProfile"]["modelSelector"]
                .as_str()
                .ok_or_else(failure)?,
            access,
        ) {
            return Err(failure());
        }
        if !channel.matches_process(process, Path::new(&workspace)) {
            return Err(failure());
        }
        Ok(ClaudeOutputState {
            run_id: self.run_id.clone(),
            connection: process.connection_id(),
            initialized: false,
            pending_answer: None,
            completion: None,
            terminal_saved: false,
            finished: false,
        })
    }

    /// One transport/cleanup tick. Non-public tool/control events go only to the
    /// trusted handler; absent/failed handling must never imply authorization.
    pub(crate) fn advance_claude_output(
        &self,
        storage: &mut Storage,
        projects: &mut ProjectManager,
        process: &mut crate::provider_login_process::LoginProcess<'_>,
        channel: &mut SessionChannel,
        cancel: &std::sync::atomic::AtomicBool,
        state: &mut ClaudeOutputState,
        handle: &mut impl FnMut(
            &mut crate::provider_login_process::LoginProcess<'_>,
            &mut SessionChannel,
            Event,
        ) -> Result<(), SidecarError>,
    ) -> Result<bool, SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "Claude output state differs");
        if state.finished
            || state.run_id != self.run_id
            || state.connection != process.connection_id()
        {
            return Err(failure());
        }
        if state.completion.is_none() {
            let next = self
                .validate_text_input(storage, projects)
                .map_err(|_| failure())
                .and_then(|_| {
                    channel.advance(process, cancel, &mut |owner| {
                        self.observe_process(storage, owner)
                    })
                });
            let result = next.and_then(|event| {
                match event {
                    None => {}
                    Some(event @ Event::Initialized { .. }) => {
                        if state.initialized {
                            return Err(failure());
                        }
                        handle(process, channel, event)?;
                        state.initialized = true;
                    }
                    Some(Event::Output(Projection::Assistant {
                        text,
                        has_private_blocks,
                        ..
                    })) => {
                        if !state.initialized {
                            return Err(failure());
                        }
                        if let Some(previous) = state.pending_answer.replace(text) {
                            self.persist_text(storage, "status", previous)?;
                        }
                        if has_private_blocks {
                            handle(
                                process,
                                channel,
                                Event::Output(Projection::RequiresHandling),
                            )?;
                        }
                    }
                    Some(Event::Output(Projection::Result(result))) => {
                        if !state.initialized {
                            return Err(failure());
                        }
                        // Missing queue state is unknown, not zero. Result alone
                        // does not release the process fence or mark success.
                        state.completion = Some(
                            if result.outcome == ReportedOutcome::Success
                                && result.queued_turn_count == Some(0)
                                && !result.permission_denied
                            {
                                Ok(TurnOutcome::Completed)
                            } else {
                                Err(SidecarError::new(
                                    "agentExecutionFailed",
                                    "Claude did not confirm a clean result",
                                ))
                            },
                        );
                    }
                    Some(event) => handle(process, channel, event)?,
                }
                Ok(())
            });
            if let Err(error) = result {
                state.completion = Some(Err(error));
            }
        }
        if state.completion.is_none() {
            return Ok(false);
        }
        let cleanup = process.poll_finish();
        if !state.terminal_saved
            && self
                .persist_process_snapshot(storage, &process.tracked_for_cleanup())
                .is_err()
        {
            process.invalidate_cleanup_evidence();
            state.completion = Some(Err(SidecarError::new(
                "agentProcessObservationFailed",
                "Claude cleanup evidence requires recovery",
            )));
        }
        if matches!(cleanup, Ok(false)) {
            return Ok(false);
        }
        if let Err(error) = cleanup {
            state.completion = Some(Err(error));
        }
        if !state.terminal_saved {
            if process
                .closure_evidence()
                .is_some_and(|receipt| receipt.confirmed())
            {
                if let Some(answer) = state.pending_answer.take() {
                    // Persist once; a failure is not retried as a fresh event.
                    let kind = if state.completion.as_ref().is_some_and(Result::is_ok)
                        && !cancel.load(std::sync::atomic::Ordering::Acquire)
                    {
                        "final"
                    } else {
                        "status"
                    };
                    if let Err(error) = self.persist_text(storage, kind, answer) {
                        state.completion = Some(Err(error));
                    }
                }
            }
            self.persist_text_terminal(
                storage,
                projects,
                process,
                state.completion.as_ref().unwrap(),
                cancel.load(std::sync::atomic::Ordering::Acquire),
            )
            .map_err(|_| failure())?;
            state.terminal_saved = true;
        }
        if process
            .closure_evidence()
            .is_some_and(|receipt| receipt.confirmed())
            && process.retains_run_fence()
        {
            process.release_after_run_terminal()?;
        }
        state.finished = true;
        Ok(true)
    }
}
