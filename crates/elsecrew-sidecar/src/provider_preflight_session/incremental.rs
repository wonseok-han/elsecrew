//! Resumable protocol state, used by the synchronous compatibility driver.
//! No process/ledger ownership or cleanup authority: the driver retains all three.
use super::*;
use crate::provider_login_io::NativeFrame;
use elsecrew_core::provider::vendor_observation::{
    codex_approval::PendingApproval,
    codex_events::stream::{Observed, TurnOutcome, TurnStream},
    codex_turn::TurnStart,
};
use std::collections::VecDeque;

// Bounds only connection/thread preparation. Once the turn is queued there is
// no wall-clock bound (2026-11-17 user decision): a coding turn may run for
// hours and may wait on the user. Cancel, quit cleanup and the output budgets
// remain the bounds.
const PREFLIGHT_BUDGET: Duration = Duration::from_secs(30);

/// Constructible only in this transport, after the owner boundary callbacks.
/// No Clone/Debug/Deserialize and no public constructor.
pub(crate) struct ConfinedAcceptReply(String);
impl ConfinedAcceptReply {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Issued only after this connection's configuration/account/catalog and
/// correlated thread response have matched. Not filesystem isolation proof.
pub(crate) struct MatchedThreadReceipt {
    connection: uuid::Uuid,
    cwd: std::path::PathBuf,
    thread_digest: String,
}

impl MatchedThreadReceipt {
    pub(crate) fn matches(&self, process: &LoginProcess<'_>, cwd: &std::path::Path) -> bool {
        self.connection == process.connection_id() && self.cwd == cwd
    }

    pub(crate) fn redacted(&self) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "connectionId": self.connection.to_string(),
            "threadDigest": self.thread_digest,
        })
    }
}

enum Stage {
    Preflight {
        flow: PreflightExchange,
        waiting: bool,
    },
    Starting(TurnStart),
    Streaming(TurnStream),
}

pub(super) struct TextSession {
    // Taking the stage before any fallible work makes errors and completion sticky.
    stage: Option<Stage>,
    cwd: std::path::PathBuf,
    text: String,
    pending: Vec<PendingApproval>,
    started: Instant,
    turn_started: Option<Instant>,
    outgoing: VecDeque<NativeFrame>,
    transmitting: bool,
    requires_approval_boundary: bool,
    // Fixed labels only: never retain provider frames, paths, or account data.
    diagnostic_stage: &'static str,
    output_budget: elsecrew_core::provider::ProviderEventBudget,
}

impl TextSession {
    pub(super) fn diagnostic_stage(&self) -> &'static str {
        self.diagnostic_stage
    }

    pub(super) fn pending_approvals(&self) -> Vec<&PendingApproval> {
        let Some(Stage::Streaming(stream)) = &self.stage else {
            return Vec::new();
        };
        self.pending
            .iter()
            .filter(|request| stream.awaits_approval_decision(&request.request_id))
            .collect()
    }

    pub(super) fn new(
        flow: PreflightExchange,
        cwd: &std::path::Path,
        text: &str,
        output_budget: elsecrew_core::provider::ProviderEventBudget,
    ) -> Result<Self, SidecarError> {
        TurnStart::validate_text(text).map_err(|_| failure())?;
        PreflightExchange::validate_thread_cwd(cwd).map_err(|_| failure())?;
        if !flow.is_fresh() {
            return Err(failure());
        }
        Ok(Self {
            stage: Some(Stage::Preflight {
                flow,
                waiting: false,
            }),
            cwd: cwd.into(),
            text: text.into(),
            pending: Vec::new(),
            started: Instant::now(),
            turn_started: None,
            outgoing: VecDeque::new(),
            transmitting: false,
            requires_approval_boundary: false,
            diagnostic_stage: "preflight initialization",
            output_budget,
        })
    }

    fn ensure_open(&self, cancel: &AtomicBool) -> Result<(), SidecarError> {
        self.ensure_open_at(Instant::now(), cancel)
    }

    fn ensure_open_at(&self, now: Instant, cancel: &AtomicBool) -> Result<(), SidecarError> {
        if cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        if self.turn_started.is_some() {
            return Ok(());
        }
        PREFLIGHT_BUDGET
            .checked_sub(now.saturating_duration_since(self.started))
            .filter(|remaining| !remaining.is_zero())
            .map(|_| ())
            .ok_or_else(|| SidecarError::new(
                "providerPreflightFailed",
                format!("Codex preflight deadline reached at {}", self.diagnostic_stage),
            ))
    }

    fn begin_turn(&mut self, now: Instant, cancel: &AtomicBool) -> Result<(), SidecarError> {
        self.ensure_open_at(now, cancel)?;
        if self.turn_started.is_some() {
            return Err(failure());
        }
        self.turn_started = Some(now);
        Ok(())
    }

    /// Consumes at most one received frame per call; never sleeps for a response.
    /// Sends use one nonblocking write per call. Observations and cleanup still
    /// have their own latency; the caller must explicitly finish its owner.
    pub(super) fn advance(
        &mut self,
        process: &mut LoginProcess<'_>,
        cancel: &AtomicBool,
        on_action: &mut impl FnMut(&mut LoginProcess<'_>, TextAction) -> Result<(), SidecarError>,
    ) -> Result<Option<TurnOutcome>, SidecarError> {
        let stage = self.stage.take().ok_or_else(failure)?;
        self.diagnostic_stage = match &stage {
            Stage::Preflight { flow, .. } => flow.diagnostic_stage(),
            Stage::Starting(_) => "turn start",
            Stage::Streaming(_) => "turn stream",
        };
        self.ensure_open(cancel)?;
        if self.transmitting || !self.outgoing.is_empty() {
            if self.requires_approval_boundary {
                on_action(process, TextAction::VerifyApprovalBoundary)?;
            }
            // Recheck authority after yielding, before every possible partial write.
            on_action(process, TextAction::VerifyWorkspace)?;
            on_action(process, TextAction::ObserveProcess)?;
            self.ensure_open(cancel)?;
            if !self.transmitting {
                process.queue_native(self.outgoing.pop_front().ok_or_else(failure)?)?;
                self.transmitting = true;
            }
            if process.flush_native_observed(cancel, &mut |process| {
                on_action(process, TextAction::ObserveProcess)
            })? {
                self.transmitting = false;
            }
            self.ensure_open(cancel)?;
            self.stage = Some(stage);
            return Ok(None);
        }
        let next = match stage {
            Stage::Preflight {
                mut flow,
                mut waiting,
            } => {
                match flow.state() {
                    PreflightState::Checking if !waiting => {
                        let frames = flow.next_frames().map_err(|_| failure())?;
                        if frames.is_empty() {
                            return Err(failure());
                        }
                        self.outgoing
                            .extend(frames.into_iter().map(NativeFrame::Preflight));
                        waiting = true;
                    }
                    PreflightState::ObservationsMatched => {
                        on_action(process, TextAction::VerifyWorkspace)?;
                        on_action(process, TextAction::ObserveProcess)?;
                        let request = flow.start_thread(&self.cwd).map_err(|_| failure())?;
                        self.outgoing.push_back(NativeFrame::Preflight(request));
                        waiting = true;
                    }
                    PreflightState::ThreadMatched => {
                        on_action(process, TextAction::VerifyWorkspace)?;
                        on_action(process, TextAction::ObserveProcess)?;
                        use sha2::{Digest, Sha256};
                        let receipt = MatchedThreadReceipt {
                            connection: process.connection_id(),
                            cwd: self.cwd.clone(),
                            thread_digest: format!("sha256:{:x}", Sha256::digest(
                                flow.matched_thread_id().ok_or_else(failure)?.as_bytes()
                            )),
                        };
                        // Persistence failure is sticky and occurs before any
                        // frame containing the user's input is even queued.
                        on_action(process, TextAction::ThreadMatched(&receipt))?;
                        self.ensure_open(cancel)?;
                        let mut turn = flow.into_text_turn(&self.text)
                            .and_then(|turn| turn.with_output_budget(self.output_budget))
                            .map_err(|_| failure())?;
                        // The thread is matched: a notification over the line bound
                        // (a large command output or image) is shortened (2026-11-26 D2).
                        process.admit_session_lines(
                            elsecrew_core::provider::oversized_line::OversizedLinePolicy::CodexAppServer,
                        )?;
                        self.outgoing.push_back(NativeFrame::Turn(
                            turn.take_frame().map_err(|_| failure())?,
                        ));
                        self.ensure_open(cancel)?;
                        self.begin_turn(Instant::now(), cancel)?;
                        self.stage = Some(Stage::Starting(turn));
                        return Ok(None);
                    }
                    PreflightState::Checking | PreflightState::ThreadStarting => {
                        if let Some(frame) = process
                            .poll_preflight_observed(cancel, &mut |process| {
                                on_action(process, TextAction::ObserveProcess)
                            })?
                        {
                            on_action(process, TextAction::ObserveProcess)?;
                            flow.accept(&frame).map_err(|_| {
                                self.diagnostic_stage = flow.diagnostic_stage();
                                failure()
                            })?;
                            waiting = flow.awaiting_response();
                        }
                    }
                    _ => return Err(failure()),
                }
                Stage::Preflight { flow, waiting }
            }
            Stage::Starting(mut turn) => {
                if let Some(frame) = process.poll_preflight_observed(cancel, &mut |process| {
                    on_action(process, TextAction::ObserveProcess)
                })? {
                    on_action(process, TextAction::ObserveProcess)?;
                    match turn.accept(&frame).map_err(|_| {
                        self.diagnostic_stage = turn.diagnostic_stage();
                        failure()
                    })? {
                        Some(stream) => Stage::Streaming(stream),
                        None => Stage::Starting(turn),
                    }
                } else {
                    Stage::Starting(turn)
                }
            }
            Stage::Streaming(mut stream) => {
                let frame = process.poll_preflight_observed(cancel, &mut |process| {
                    on_action(process, TextAction::ObserveProcess)
                })?;
                self.pending
                    .retain(|request| stream.awaits_approval_decision(&request.request_id));
                if let Some(frame) = frame {
                    on_action(process, TextAction::ObserveProcess)?;
                    let observed = stream.observe(&frame).map_err(|_| {
                        self.diagnostic_stage = stream.diagnostic_stage();
                        failure()
                    })?;
                    self.ensure_open(cancel)?;
                    match observed {
                        Observed::Public(value) => on_action(process, TextAction::Public(value))?,
                        Observed::ItemStarted | Observed::Private => {}
                        Observed::Finished(_) => {
                            return stream.finish().map(Some).map_err(|_| failure())
                        }
                        Observed::Tool(tool) => on_action(process, TextAction::Tool(tool))?,
                        Observed::Approval(request) => self.pending.push(*request),
                        // Answered at once with the prepared refusal; the turn goes on.
                        Observed::Refused(reply) => self.outgoing.push_back(NativeFrame::Refusal(reply)),
                        Observed::Started | Observed::RequiresHandling => {
                            self.diagnostic_stage = stream.diagnostic_stage();
                            return Err(failure());
                        }
                    }
                } else {
                    for request in &self.pending {
                        let mut decision = None;
                        on_action(
                            process,
                            TextAction::PollApproval {
                                request,
                                decision: &mut decision,
                            },
                        )?;
                        if let Some(decision) = decision {
                            if matches!(decision, elsecrew_core::provider::vendor_observation::codex_approval::Decision::AcceptOnce) {
                                self.requires_approval_boundary = true;
                                on_action(process, TextAction::VerifyApprovalBoundary)?;
                            }
                            on_action(process, TextAction::VerifyWorkspace)?;
                            on_action(process, TextAction::ObserveProcess)?;
                            self.ensure_open(cancel)?;
                            let frame = if matches!(decision, elsecrew_core::provider::vendor_observation::codex_approval::Decision::AcceptOnce) {
                                stream.reserve_approval_response(&request.request_id).map_err(|_| failure())?;
                                use elsecrew_core::provider::vendor_observation::codex_approval::RequestId;
                                let id = match &request.request_id {
                                    RequestId::Number(id) => serde_json::json!(id),
                                    RequestId::Text(id) => serde_json::json!(id),
                                };
                                NativeFrame::ConfinedAccept(ConfinedAcceptReply(format!("{}\n", serde_json::json!({"id":id,"result":{"decision":"accept"}}))))
                            } else {
                                NativeFrame::Approval(stream.prepare_approval_reply(&request.request_id, decision).map_err(|_| failure())?)
                            };
                            self.outgoing.push_back(frame);
                            break;
                        }
                    }
                }
                Stage::Streaming(stream)
            }
        };
        self.ensure_open(cancel)?;
        self.stage = Some(next);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_profile::CodexProfileBinding;
    use elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
    use std::process::{Command, Stdio};

    #[test]
    fn turn_has_no_wall_clock_bound_and_cannot_be_restarted() {
        let make = || {
            let flow = PreflightExchange::new(
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                "/profile",
            )
            .unwrap();
            TextSession::new(flow, std::path::Path::new("/workspace"), "goal", elsecrew_core::provider::ProviderEventBudget::LegacyV1).unwrap()
        };
        let cancel = AtomicBool::new(false);
        let mut expired = make();
        assert!(expired
            .begin_turn(expired.started + PREFLIGHT_BUDGET, &cancel)
            .is_err());
        assert!(expired.turn_started.is_none());
        let mut session = make();
        let turn = session.started + Duration::from_secs(29);
        session.begin_turn(turn, &cancel).unwrap();
        // Crossing the 30s preparation bound is valid during the actual turn,
        // and a turn may run for hours (tools, user approval waits).
        session
            .ensure_open_at(turn + Duration::from_secs(31), &cancel)
            .unwrap();
        session
            .ensure_open_at(turn + Duration::from_secs(24 * 60 * 60), &cancel)
            .unwrap();
        assert!(session
            .begin_turn(turn + Duration::from_secs(32), &cancel)
            .is_err());
        assert_eq!(session.turn_started, Some(turn));
        cancel.store(true, Ordering::Release);
        assert!(session
            .ensure_open_at(turn + Duration::from_secs(1), &cancel)
            .is_err());
        assert_eq!(session.turn_started, Some(turn));
    }

    #[test]
    fn stalled_peer_yields_and_failed_session_cannot_resume_or_release_owner() {
        for case in ["cancel", "deadline", "handler"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = crate::Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let flow = PreflightExchange::new(
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                home.to_str().unwrap(),
            )
            .unwrap();
            let mut process = LoginProcess::reserve(&profile).unwrap();
            process
                .launch(|| {
                    Command::new("/bin/sh")
                        .args(["-c", "IFS= read -r first; IFS= read -r hold"])
                        .env_clear()
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                })
                .unwrap();
            let mut session = TextSession::new(flow, &root, "goal", elsecrew_core::provider::ProviderEventBudget::LegacyV1).unwrap();
            let cancel = AtomicBool::new(false);
            process.begin_preflight().unwrap();
            let mut observe = |_: &mut LoginProcess<'_>, action: TextAction| {
                assert!(matches!(
                    action,
                    TextAction::ObserveProcess | TextAction::VerifyWorkspace
                ));
                Ok(())
            };
            // Send initialization, then return repeatedly with no peer response.
            // The caller regains control and can service unrelated IPC or cancel.
            assert!(session
                .advance(&mut process, &cancel, &mut observe)
                .unwrap()
                .is_none());
            for _ in 0..20 {
                assert!(session
                    .advance(&mut process, &cancel, &mut observe)
                    .unwrap()
                    .is_none());
            }
            assert!(process.exit_status().unwrap().is_none());
            assert!(LoginProcess::reserve(&profile).is_err());
            match case {
                "cancel" => cancel.store(true, Ordering::Release),
                "deadline" => session.started = Instant::now() - Duration::from_secs(31),
                _ => {
                    // Force the next observation checkpoint without changing the transport.
                    std::thread::sleep(Duration::from_millis(110));
                }
            }
            assert!(session
                .advance(&mut process, &cancel, &mut |_, _| Err(failure()))
                .is_err());
            assert_eq!(session.diagnostic_stage(), "initialize");
            cancel.store(false, Ordering::Release);
            session.started = Instant::now();
            session.turn_started = None;
            assert!(session
                .advance(&mut process, &cancel, &mut observe)
                .is_err());
            assert_eq!(session.diagnostic_stage(), "initialize");
            assert!(LoginProcess::reserve(&profile).is_err());
            process.finish().unwrap();
            assert!(LoginProcess::reserve(&profile).is_ok());
        }
    }
}
