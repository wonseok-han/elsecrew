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
