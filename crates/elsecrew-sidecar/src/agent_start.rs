//! A run's start in stages, so the engine's one loop keeps answering while
//! runs start (ADE stabilization A). Before, every finished check was turned
//! into a launched run inside one poll: 13–18 s per start (most of it reading
//! the provider's 100+ MB executable again and again and checking its
//! publisher signature), and four starts back to back held every other
//! request for over a minute.
//!
//! Stages of one start:
//! 1. `Discovering` — the provider check runs on its own worker (unchanged).
//! 2. `Building` — the check is taken on the loop and a worker pins the
//!    signed executable(s) and checks the publisher signature (the slow part).
//!    Pure: no process is started, nothing is reserved or written, no lock is
//!    taken.
//! 3. `Ready` — on the loop, in one step exactly as before: reserve the run,
//!    take its lock, launch, record. The command's arguments and environment
//!    are composed there from the files as they are then (PR #25 review I2);
//!    the pins are used only for the very executable paths the check named.
//!    Right before, `ready_check` refuses a start that was stopped (its team
//!    canceled, the app quitting) and checks the provider again, at most
//!    twice, when the check got too old while the start waited (review I3).
//!
//! Concurrency boundary (docs/architecture.md "엔진 루프와 실행 시작"):
//! storage writes, provider locks (fences), process ownership and run state
//! change only on the loop thread, in the same order and within the same
//! single step as before. A durable run never waits across loop passes in a
//! half-made state (no new cancel or recovery window). Workers return owned,
//! inert values; the loop decides.
//!
//! Each pass of the loop does at most one loop step of all starts together
//! (taking a check, or reserving and launching one), oldest start first, so a
//! request waits for at most one such step. With four starts that used to be
//! one 60+ s hold, now four holds of a few seconds with requests between.
use super::*;

pub(crate) enum StartStage {
    /// The provider check is still running on its worker.
    Discovering,
    /// The check was taken; a worker builds the launch command.
    Building(StartEvidence, thread::JoinHandle<Result<PreparedLaunch, SidecarError>>),
    /// The command is built; the loop reserves and launches on a later step.
    Ready(StartEvidence, PreparedLaunch),
    /// Test-only: a loop step of a given cost, to measure the loop without a
    /// provider.
    #[cfg(test)]
    TestLoopStep(Duration, Option<Result<chat::G5Action, SidecarError>>),
    /// Test-only: a worker step that takes this long, then fails the start.
    #[cfg(test)]
    TestWorker(thread::JoinHandle<()>),
}

pub(crate) enum StartEvidence {
    Claude(agent_runtime::ScopedClaudeEvidence),
    Codex(agent_runtime::ScopedEvidence),
}

pub(crate) enum PreparedLaunch {
    Claude(agent_runtime::preparation::PreparedClaudeLaunch),
    Codex(agent_runtime::preparation::PreparedCodexLaunch),
}

/// How old a provider check may be when the start reserves on it: its
/// validity is 30 s and the reserve-and-launch step itself needs a few.
pub(crate) const START_CHECK_MAX_AGE: Duration = Duration::from_secs(25);
/// How many times one start checks the provider again because its check got
/// too old while it waited.
pub(crate) const START_RECHECKS: u8 = 2;

pub(crate) enum ReadyCheck {
    Proceed,
    CheckAgain,
    Refuse(SidecarError),
}

impl StartEvidence {
    fn age(&self) -> Duration {
        match self {
            Self::Claude(observed) => observed.evidence.age(),
            Self::Codex(observed) => observed.evidence.observation.age(),
        }
    }
}

/// Whether a start's turn used the loop for real work (counts against the
/// one step per pass).
enum Turn {
    Waiting,
    Worked,
}

impl Sidecar {
    /// Moves every start along; answers finished ones. At most one loop step
    /// in all (see the module comment).
    pub(crate) fn advance_agent_starts(&mut self, responses: &mut Vec<ResponseV1>, events: &mut Vec<chat::G5Event>) {
        let mut order: Vec<(u64, String)> = self.pending_agent_starts.iter().map(|(id, start)| (start.arrival, id.clone())).collect();
        order.sort();
        let mut worked = false;
        for (_, id) in order {
            let mut pending = self.pending_agent_starts.remove(&id).expect("listed start");
            if pending.result.is_none() && matches!(self.advance_start(&id, &mut pending, worked), Turn::Worked) {
                worked = true;
            }
            if pending.result.is_none() {
                self.pending_agent_starts.insert(id, pending);
                continue;
            }
            if self.agent_runtime.retain_rejected_start(&mut pending.reservation, &mut pending.process).is_err() {
                // Keep the original result and both owners; never rerun launch.
                self.pending_agent_starts.insert(id, pending);
                continue;
            }
            responses.push(match pending.result.take().unwrap() {
                Ok(action) => { events.extend(action.events); success_response(&pending.request, action.result) },
                Err(error) => error_response(&pending.request, error.kind(), "requestBlocked", error.detail()),
            });
        }
    }

    /// One start's turn. `worked`: another start already used this pass's
    /// loop step; then only worker results are collected.
    fn advance_start(&mut self, id: &str, pending: &mut PendingAgentStart, worked: bool) -> Turn {
        loop {
            match std::mem::replace(&mut pending.stage, StartStage::Discovering) {
                StartStage::Discovering => {
                    if worked { return Turn::Waiting; }
                    match self.take_start_check(id, pending) {
                        None => return Turn::Waiting,
                        Some(Ok(stage)) => { pending.stage = stage; return Turn::Worked; }
                        Some(Err(error)) => { pending.result = Some(Err(error)); return Turn::Worked; }
                    }
                }
                StartStage::Building(evidence, worker) => {
                    if !worker.is_finished() {
                        pending.stage = StartStage::Building(evidence, worker);
                        return Turn::Waiting;
                    }
                    match worker.join() {
                        Ok(Ok(prepared)) => pending.stage = StartStage::Ready(evidence, prepared),
                        Ok(Err(error)) => { pending.result = Some(Err(error)); return Turn::Waiting; }
                        Err(_) => {
                            pending.result = Some(Err(SidecarError::new("agentLaunchRejected", "launch preparation stopped")));
                            return Turn::Waiting;
                        }
                    }
                }
                StartStage::Ready(evidence, prepared) => {
                    if worked {
                        pending.stage = StartStage::Ready(evidence, prepared);
                        return Turn::Waiting;
                    }
                    match self.ready_check(id, pending.rediscoveries, evidence.age()) {
                        ReadyCheck::Proceed => pending.result = Some(self.reserve_and_launch(id, pending, evidence, prepared)),
                        ReadyCheck::Refuse(error) => pending.result = Some(Err(error)),
                        ReadyCheck::CheckAgain => {
                            // Too old to reserve on (PR #25 review I3): check the
                            // provider again rather than fail; a stale check is
                            // never used.
                            drop((evidence, prepared));
                            pending.rediscoveries += 1;
                            match self.agent_runtime.begin_scoped_start(&self.storage, &self.projects, &pending.input, id) {
                                Ok(_) => pending.stage = StartStage::Discovering,
                                Err(error) => pending.result = Some(Err(error)),
                            }
                        }
                    }
                    return Turn::Worked;
                }
                #[cfg(test)]
                StartStage::TestLoopStep(cost, result) => {
                    if worked {
                        pending.stage = StartStage::TestLoopStep(cost, result);
                        return Turn::Waiting;
                    }
                    if let Err(error) = self.start_gate(id) {
                        pending.result = Some(Err(error));
                        return Turn::Worked;
                    }
                    thread::sleep(cost);
                    pending.result = result.or_else(|| Some(Err(SidecarError::new("agentLaunchRejected", "test start"))));
                    return Turn::Worked;
                }
                #[cfg(test)]
                StartStage::TestWorker(worker) => {
                    if !worker.is_finished() {
                        pending.stage = StartStage::TestWorker(worker);
                        return Turn::Waiting;
                    }
                    let _ = worker.join();
                    pending.result = Some(Err(SidecarError::new("agentLaunchRejected", "test start")));
                    return Turn::Waiting;
                }
            }
        }
    }

    /// Whether a start may still go on (PR #25 review, cancel during
    /// preparation): not while the app prepares to quit, and not a team
    /// step whose team was stopped meanwhile.
    pub(crate) fn start_gate(&self, id: &str) -> Result<(), SidecarError> {
        if self.prepared_to_quit {
            return Err(SidecarError::new("agentLaunchRejected", "the app is quitting; the run was not started"));
        }
        if let Some((chat, kind)) = self.workflow_requests.get(id) {
            if kind == "start" && !self.workflow_chats.contains(chat) {
                return Err(SidecarError::new("agentLaunchRejected", "the team task was stopped before its run started"));
            }
        }
        Ok(())
    }

    /// Right before reserving: go on, refuse, or check the provider again
    /// because the check got too old while the start waited (PR #25 review
    /// I3; at most `START_RECHECKS` times, never for a stopped start).
    pub(crate) fn ready_check(&self, id: &str, rechecked: u8, age: Duration) -> ReadyCheck {
        if let Err(error) = self.start_gate(id) {
            return ReadyCheck::Refuse(error);
        }
        if age <= START_CHECK_MAX_AGE {
            return ReadyCheck::Proceed;
        }
        if rechecked < START_RECHECKS {
            ReadyCheck::CheckAgain
        } else {
            ReadyCheck::Refuse(SidecarError::new("agentInputChanged", "the provider check stayed too old while the start waited"))
        }
    }

    /// Takes a finished provider check and starts building the launch command
    /// on a worker. `None` while the check still runs.
    fn take_start_check(&mut self, id: &str, pending: &PendingAgentStart) -> Option<Result<StartStage, SidecarError>> {
        if pending.runtime == "claude-code" {
            let observed = self.agent_runtime.take_claude_discovery(&self.storage, &self.projects, id)?;
            // Stopped or quitting meanwhile: the check is dropped, nothing is
            // built or reserved (PR #25 review).
            Some(observed.and_then(|observed| {
                self.start_gate(id)?;
                let executable = observed.evidence.executable.revalidate()?.to_owned();
                let worker = thread::Builder::new().name("start-claude-build".into())
                    .spawn(move || latency_log::step("start.claude.build", || agent_runtime::preparation::PreparedClaudeLaunch::build(executable).map(PreparedLaunch::Claude)))
                    .map_err(|_| SidecarError::new("agentLaunchRejected", "launch preparation unavailable"))?;
                Ok(StartStage::Building(StartEvidence::Claude(observed), worker))
            }))
        } else {
            let observed = self.agent_runtime.take_scoped_discovery(&self.storage, &self.projects, id)?;
            Some(observed.and_then(|observed| {
                self.start_gate(id)?;
                let executable = observed.evidence.executable.revalidate()?.to_owned();
                let helper = observed.evidence.helper.revalidate()?.to_owned();
                let worker = thread::Builder::new().name("start-codex-build".into())
                    .spawn(move || latency_log::step("start.codex.build", || agent_runtime::preparation::PreparedCodexLaunch::build(executable, helper).map(PreparedLaunch::Codex)))
                    .map_err(|_| SidecarError::new("agentLaunchRejected", "launch preparation unavailable"))?;
                Ok(StartStage::Building(StartEvidence::Codex(observed), worker))
            }))
        }
    }

    /// The one loop step that makes a run: reserve, lock, launch, record —
    /// unchanged from before, now with the command built ahead.
    fn reserve_and_launch(&mut self, id: &str, pending: &mut PendingAgentStart, evidence: StartEvidence, prepared: PreparedLaunch)
        -> Result<chat::G5Action, SidecarError>
    {
        let mismatch = || SidecarError::new("agentLaunchRejected", "launch preparation differs");
        match (evidence, prepared) {
            (StartEvidence::Claude(observed), PreparedLaunch::Claude(prepared)) => {
                latency_log::note("start.claude.checkAge", observed.evidence.age());
                latency_log::step("start.claude.reserve", || self.agent_runtime.reserve_scoped_claude_start(&mut self.storage, &self.projects,
                    &pending.input, id, &observed, &mut pending.reservation, &mut pending.process))?;
                let invalid = || SidecarError::new("agentInputChanged", "Claude reservation unavailable");
                let admission = Arc::new(latency_log::step("start.claude.admission", || pending.reservation.as_ref().ok_or_else(invalid)?.claude_admission(
                    &self.storage, &self.projects, pending.process.as_ref().ok_or_else(invalid)?))?);
                let prompt_admission = admission.clone();
                let tool_admission = admission.clone();
                latency_log::step("start.claude.launch", || self.agent_runtime.start_reserved_claude(&mut self.storage, &self.projects,
                    &mut pending.reservation, &mut pending.process, &observed.evidence.executable,
                    &AtomicBool::new(self.prepared_to_quit),
                    &mut |workspace, home, model, effort, access| admission.launch(workspace, home, model, effort, access),
                    Box::new(move |process| prompt_admission.process(process)),
                    Box::new(move |process, request| tool_admission.tool(process, request)),
                    Some(prepared)))
            }
            (StartEvidence::Codex(observed), PreparedLaunch::Codex(prepared)) => {
                latency_log::note("start.codex.checkAge", observed.evidence.observation.age());
                latency_log::step("start.codex.reserve", || self.agent_runtime.reserve_scoped_start(
                    &mut self.storage, &self.projects, &pending.input, id,
                    &observed, &mut pending.reservation, &mut pending.process,
                ))?;
                latency_log::step("start.codex.launch", || self.agent_runtime.start_reserved_codex(
                    &mut self.storage, &self.projects, &mut pending.reservation,
                    &mut pending.process, &observed.evidence.observation,
                    &observed.evidence.executable, &observed.evidence.helper,
                    &AtomicBool::new(self.prepared_to_quit),
                    Some(prepared),
                ))
            }
            _ => Err(mismatch()),
        }
    }
}
