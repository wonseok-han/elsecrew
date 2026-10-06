//! Public-output/terminal bridge for an independently admitted Claude channel.
//! No launch, prompt, or tool grant authority is created here.
use super::*;
use crate::provider_claude_session::SessionChannel;
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
}

impl OwnedClaudeRun {
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
        self.reservation.advance_claude_output(
            storage,
            projects,
            &mut self.process,
            &mut self.channel,
            &self.cancel,
            &mut self.output,
            handle,
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
    pub(crate) fn begin_claude_output(
        &self,
        storage: &Storage,
        projects: &ProjectManager,
        process: &crate::provider_login_process::LoginProcess<'_>,
        channel: &SessionChannel,
    ) -> Result<ClaudeOutputState, SidecarError> {
        let failure = || SidecarError::new("agentInputChanged", "Claude output owner differs");
        if self.run["selectedProfile"]["runtime"] != "claude-code"
            || !process.matches_startup_run(&self.run_id)
        {
            return Err(failure());
        }
        process.claude_preflight_home()?;
        let access = match self.run["selectedProfile"]["permission"].as_str() {
            Some("readOnly") => crate::provider_claude_session::ToolAccess::ReadOnly,
            Some("workspaceWrite") => crate::provider_claude_session::ToolAccess::Edit,
            _ => return Err(failure()),
        };
        if !channel.matches_role(
            self.run["selectedProfile"]["modelSelector"]
                .as_str()
                .ok_or_else(failure)?,
            access,
        ) {
            return Err(failure());
        }
        self.validate_text_input(storage, projects)
            .map_err(|_| failure())?;
        let manifest = projects
            .verify_workspace_for_run(
                storage,
                &address_from_chat(&self.run).map_err(|_| failure())?,
            )
            .map_err(|_| failure())?;
        if !channel.matches_process(
            process,
            Path::new(manifest["worktreePath"].as_str().ok_or_else(failure)?),
        ) {
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
