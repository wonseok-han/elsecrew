//! A run's start in stages, so the engine's one loop keeps answering while
//! runs start (ADE stabilization A). Before, every finished check was turned
//! into a launched run inside one poll: 13–18 s per start (most of it reading
//! the provider's 100+ MB executable again and again and checking its
//! publisher signature), and four starts back to back held every other
//! request for over a minute.
//!
//! Stages of one start:
//! 1. `Discovering` — the provider check runs on its own worker (unchanged).
//! 2. `Building` — the check is taken on the loop, the launch command's inputs
//!    are read from durable state, and a worker builds the command: it pins
//!    the signed executable(s), checks the publisher signature and composes
//!    the arguments. Pure: no process is started, nothing is reserved or
//!    written, no lock is taken.
//! 3. `Ready` — on the loop, in one step exactly as before: reserve the run,
//!    take its lock, launch, record. The launch re-reads its inputs and uses
//!    the built command only when they are equal; otherwise it is refused.
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
                    pending.result = Some(self.reserve_and_launch(id, pending, evidence, prepared));
                    return Turn::Worked;
                }
                #[cfg(test)]
                StartStage::TestLoopStep(cost, result) => {
                    if worked {
                        pending.stage = StartStage::TestLoopStep(cost, result);
                        return Turn::Waiting;
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

    /// Takes a finished provider check and starts building the launch command
    /// on a worker. `None` while the check still runs.
    fn take_start_check(&mut self, id: &str, pending: &PendingAgentStart) -> Option<Result<StartStage, SidecarError>> {
        if pending.runtime == "claude-code" {
            let observed = self.agent_runtime.take_claude_discovery(&self.storage, &self.projects, id)?;
            Some(observed.and_then(|observed| {
                let inputs = latency_log::step("start.claude.inputs", || self.agent_runtime.claude_start_launch_inputs(&self.storage, &self.projects, &pending.input, &observed))?;
                let worker = thread::Builder::new().name("start-claude-build".into())
                    .spawn(move || latency_log::step("start.claude.build", || agent_runtime::preparation::PreparedClaudeLaunch::build(inputs).map(PreparedLaunch::Claude)))
                    .map_err(|_| SidecarError::new("agentLaunchRejected", "launch preparation unavailable"))?;
                Ok(StartStage::Building(StartEvidence::Claude(observed), worker))
            }))
        } else {
            let observed = self.agent_runtime.take_scoped_discovery(&self.storage, &self.projects, id)?;
            Some(observed.and_then(|observed| {
                let inputs = latency_log::step("start.codex.inputs", || self.agent_runtime.codex_start_launch_inputs(&self.storage, &self.projects, &pending.input, &observed))?;
                let worker = thread::Builder::new().name("start-codex-build".into())
                    .spawn(move || latency_log::step("start.codex.build", || agent_runtime::preparation::PreparedCodexLaunch::build(inputs).map(PreparedLaunch::Codex)))
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
