//! Bounded nonblocking login stdio. The caller retains the child and its tree
//! tracker, and must terminate/persist uncertain cleanup on every failure.
//! This is not a spawn API or authorization to send arbitrary provider methods.
use crate::SidecarError;
use elsecrew_core::provider::vendor_observation::codex_login::LoginRequest;
use elsecrew_core::provider::oversized_line::{
    reduce_oversized_line, OversizedLinePolicy, PROVIDER_SESSION_LINE_RAW_MAX_BYTES,
    PROVIDER_SESSION_STREAM_MAX_BYTES,
};
use elsecrew_core::provider::{
    BoundedObservation, PROVIDER_OBSERVATION_LINE_MAX_BYTES, PROVIDER_OBSERVATION_TOTAL_MAX_BYTES,
};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How many connections in this process may hold a line over the observation
/// bound at once (PR #17 review I1). A connection waits, without reading more,
/// until one is free: its provider then blocks writing, and nothing is lost.
/// A holder always finishes or fails its line, so waiting never deadlocks.
/// A provider that stops mid-line does not keep a slot either: see
/// `LargeLineLimits` (PR #17 re-review).
///
/// Memory this bounds: each held line is at most 8 MiB raw plus the read in
/// progress (64 KiB), its parsed tree (string bytes up to the raw size, at most
/// `PROVIDER_SESSION_LINE_MAX_VALUES` values) and a 128 KiB result, so about
/// 24 MiB per slot at worst, 48 MiB for both. Every other connection holds at
/// most the 128 KiB bound plus one byte before it must wait.
pub(crate) const LARGE_LINE_SLOTS: usize = 2;
static LARGE_LINES_HELD: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static LARGE_LINES_PEAK: AtomicUsize = AtomicUsize::new(0);

/// One of the `LARGE_LINE_SLOTS`; returned when dropped.
struct LargeLineSlot(());

impl LargeLineSlot {
    fn try_take() -> Option<Self> {
        let mut held = LARGE_LINES_HELD.load(Ordering::Acquire);
        loop {
            if held >= LARGE_LINE_SLOTS {
                return None;
            }
            match LARGE_LINES_HELD.compare_exchange_weak(held, held + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    #[cfg(test)]
                    LARGE_LINES_PEAK.fetch_max(held + 1, Ordering::AcqRel);
                    return Some(Self(()));
                }
                Err(current) => held = current,
            }
        }
    }
}

impl Drop for LargeLineSlot {
    fn drop(&mut self) {
        LARGE_LINES_HELD.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Deadlines around the large-line slots (PR #17 re-review). Past any of
/// them only that connection fails (its line is refused, its slot and buffer
/// returned); every other run keeps going.
#[derive(Debug, Clone, Copy)]
struct LargeLineLimits {
    /// Longest a connection may hold a slot for one line.
    hold: Duration,
    /// Longest a held line may go without growing while the provider has
    /// nothing more to give (a read would block). A slow poll is not a stall:
    /// the clock is only judged when the pipe is empty.
    stall: Duration,
    /// Longest a connection may wait for a slot. Longer than `hold`, so a
    /// waiter outlasts at least one full hold of each slot.
    wait: Duration,
}

/// Longest a queued frame may go without any byte written while the
/// provider's input is full. Past it the connection fails (PR #19 review I1).
const SEND_STALL_MAX: Duration = Duration::from_secs(30);

const LARGE_LINE_LIMITS: LargeLineLimits = LargeLineLimits {
    hold: Duration::from_secs(30),
    stall: Duration::from_secs(10),
    wait: Duration::from_secs(90),
};

/// A slot held for the line being read, with its progress clock.
struct HeldLine {
    _slot: LargeLineSlot,
    since: Instant,
    progress: Instant,
    len: usize,
}

/// Tests that make lines over the observation bound share the process-wide
/// slots; they take this lock so their timing and counts do not mix.
#[cfg(test)]
pub(crate) static LARGE_LINE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) enum HookProbeRequest<'a> {
    Start(&'a std::path::Path),
    Inventory(&'a std::path::Path),
    OfflineTurn(&'a str),
}

fn failure() -> SidecarError {
    SidecarError::new(
        "providerLoginTransportFailed",
        "login transport failed; process cleanup must be confirmed",
    )
}

fn nonblocking(pipe: &impl AsRawFd) -> Result<(), SidecarError> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(failure());
    }
    Ok(())
}

/// No Debug: buffers may contain OAuth state or private account metadata.
pub(crate) struct LoginIo {
    input: ChildStdin,
    output: ChildStdout,
    pending: Vec<u8>,
    received: usize,
    sent: usize,
    code_sent: bool,
    failed: bool,
    outgoing: Option<(Vec<u8>, usize)>,
    /// The large-input budget an allow reply carries, kept until the reply is
    /// written, fails or the connection is dropped (PR #19 review I1).
    outgoing_hold: Option<elsecrew_core::provider::vendor_observation::claude_approval::LargeInputHold>,
    /// When the queued frame last moved; a provider that stops reading must
    /// not keep the run and its buffer forever.
    outgoing_progress: Instant,
    send_stall: Duration,
    /// Set once the connection carries an agent session (see `admit_session_lines`).
    session_lines: Option<OversizedLinePolicy>,
    /// How much of `pending` is already known to hold no newline, so a long
    /// line is scanned once rather than again on every read.
    scanned: usize,
    /// Held while `pending` is over the observation line bound.
    large_line: Option<HeldLine>,
    /// Since when this connection has waited for a slot.
    slot_wait: Option<Instant>,
    limits: LargeLineLimits,
}

/// Only requests already admitted by the corresponding protocol state machine.
pub(crate) enum NativeFrame {
    #[cfg(target_os = "macos")]
    ClaudeAllow(crate::provider_claude_session::PermissionAllow),
    #[cfg(target_os = "macos")]
    ClaudePrompt(crate::provider_claude_session::InitialPrompt),
    #[cfg(target_os = "macos")]
    ClaudeDenial(crate::provider_claude_session::PermissionDenial),
    ClaudeInitialize(
        elsecrew_core::provider::vendor_observation::claude_control::InitializeRequest,
    ),
    Preflight(
        elsecrew_core::provider::vendor_observation::codex_preflight::exchange::PreflightRequest,
    ),
    Turn(elsecrew_core::provider::vendor_observation::codex_turn::TurnRequest),
    Approval(elsecrew_core::provider::vendor_observation::codex_approval::ApprovalReply),
    /// The refusal the turn stream prepared for a request this client does not support.
    Refusal(elsecrew_core::provider::vendor_observation::codex_events::stream::RefusalReply),
    ConfinedAccept(crate::provider_preflight_session::ConfinedAcceptReply),
    #[cfg(all(test, target_os = "macos"))]
    PermissionProbe(crate::provider_permission_probe::ProbeFrame),
}

impl NativeFrame {
    fn bytes(&self) -> &[u8] {
        match self {
            #[cfg(target_os = "macos")]
            Self::ClaudeAllow(frame) => frame.as_str().as_bytes(),
            #[cfg(target_os = "macos")]
            Self::ClaudePrompt(frame) => frame.as_str().as_bytes(),
            #[cfg(target_os = "macos")]
            Self::ClaudeDenial(frame) => frame.as_str().as_bytes(),
            Self::ClaudeInitialize(frame) => frame.as_str().as_bytes(),
            Self::Preflight(frame) => frame.as_str().as_bytes(),
            Self::Turn(frame) => frame.as_str().as_bytes(),
            Self::Approval(frame) => frame.as_str().as_bytes(),
            Self::Refusal(frame) => frame.as_str().as_bytes(),
            Self::ConfinedAccept(frame) => frame.as_bytes(),
            #[cfg(all(test, target_os = "macos"))]
            Self::PermissionProbe(frame) => frame.as_bytes(),
        }
    }
}

/// CLI output is not JSONL: blank lines and clean EOF are valid. Never mix this
/// transport mode with send/poll frames on one connection.
pub(crate) enum OutputChunk {
    Pending,
    Bytes(Vec<u8>),
    Eof,
}

impl LoginIo {
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn poll_permission_probe_chunk(
        &mut self,
        cancel: &AtomicBool,
    ) -> Result<OutputChunk, SidecarError> {
        if self.failed || self.outgoing.is_some() || cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        for _ in 0..32 {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                if end == 0 || end > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
                    return Err(failure());
                }
                return Ok(OutputChunk::Bytes(self.pending.drain(..=end).collect()));
            }
            if self.pending.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
                return Err(failure());
            }
            let mut chunk = [0; 4096];
            match self.output.read(&mut chunk) {
                Ok(0) => {
                    return if self.pending.is_empty() {
                        Ok(OutputChunk::Eof)
                    } else {
                        Err(failure())
                    }
                }
                Ok(count) => {
                    self.received += count;
                    if self.received > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES {
                        self.failed = true;
                        return Err(failure());
                    }
                    self.pending.extend_from_slice(&chunk[..count]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    return Ok(OutputChunk::Pending);
                }
                Err(_) => return Err(failure()),
            }
        }
        Ok(OutputChunk::Pending)
    }

    pub(crate) fn queue_native(&mut self, mut frame: NativeFrame) -> Result<(), SidecarError> {
        let hold = match &mut frame {
            #[cfg(target_os = "macos")]
            NativeFrame::ClaudeAllow(reply) => reply.take_hold(),
            _ => None,
        };
        self.queue_bytes(frame.bytes())?;
        self.outgoing_hold = hold;
        Ok(())
    }

    /// An agent session (not a login or preflight exchange) may send lines over
    /// the observation line bound when the agent reads a large image or file
    /// (2026-11-26 D2). From here on, such a line is read up to
    /// `PROVIDER_SESSION_LINE_RAW_MAX_BYTES` and shortened by `policy` before it
    /// is observed; frames that must be read exactly still fail when oversized.
    /// The connection totals rise to the session budget. One-way: never reset.
    pub(crate) fn admit_session_lines(&mut self, policy: OversizedLinePolicy) -> Result<(), SidecarError> {
        if self.failed || self.session_lines.is_some_and(|current| current != policy) {
            return Err(failure());
        }
        self.session_lines = Some(policy);
        Ok(())
    }

    /// Forget buffered output, returning its memory and any large-line slot.
    fn drop_pending(&mut self) {
        self.pending = Vec::new();
        self.scanned = 0;
        self.large_line = None;
        self.slot_wait = None;
    }

    #[cfg(test)]
    fn set_large_line_limits(&mut self, hold: Duration, stall: Duration, wait: Duration) {
        self.limits = LargeLineLimits { hold, stall, wait };
    }

    /// One frame this side may send. A session may echo a large tool input
    /// whole in an allow reply (2026-11-26 big Write); the frame is built by
    /// the permission parser, never taken from the provider as is.
    fn frame_limit(&self) -> usize {
        if self.session_lines.is_some() {
            elsecrew_core::provider::vendor_observation::claude_approval::LARGE_TOOL_REPLY_MAX_BYTES
        } else {
            PROVIDER_OBSERVATION_LINE_MAX_BYTES
        }
    }

    fn stream_limit(&self) -> usize {
        if self.session_lines.is_some() { PROVIDER_SESSION_STREAM_MAX_BYTES } else { PROVIDER_OBSERVATION_TOTAL_MAX_BYTES }
    }

    fn queue_bytes(&mut self, bytes: &[u8]) -> Result<(), SidecarError> {
        if self.failed
            || self.outgoing.is_some()
            || bytes.len() < 2
            || bytes.len() > self.frame_limit()
            || bytes.last() != Some(&b'\n')
            || bytes[..bytes.len() - 1].contains(&b'\n')
            || self.sent + bytes.len() > self.stream_limit()
        {
            self.failed = true;
            self.outgoing = None;
            return Err(failure());
        }
        self.outgoing = Some((bytes.to_vec(), 0));
        self.outgoing_progress = Instant::now();
        Ok(())
    }

    /// One nonblocking write syscall. False retains the exact unwritten suffix.
    /// The session owner applies its overall deadline between these calls.
    pub(crate) fn flush_native(
        &mut self,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<bool, SidecarError> {
        let result = (|| {
            if self.failed || cancel.load(Ordering::Acquire) {
                return Err(failure());
            }
            observe()?;
            if cancel.load(Ordering::Acquire) {
                return Err(failure());
            }
            let (bytes, written) = self.outgoing.as_mut().ok_or_else(failure)?;
            match self.input.write(&bytes[*written..]) {
                Ok(0) => return Err(failure()),
                Ok(count) => {
                    *written += count;
                    self.sent += count;
                    self.outgoing_progress = Instant::now();
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    // The provider stopped reading: give up on this reply
                    // rather than hold the run and its buffer (review I1).
                    if self.outgoing_progress.elapsed() > self.send_stall {
                        eprintln!("provider session reply refused: the provider stopped reading");
                        return Err(failure());
                    }
                }
                Err(_) => return Err(failure()),
            }
            let complete = *written == bytes.len();
            if complete {
                self.outgoing = None;
                self.outgoing_hold = None;
            }
            Ok(complete)
        })();
        if result.is_err() {
            self.failed = true;
            self.outgoing = None;
            self.outgoing_hold = None;
        }
        result
    }

    pub(crate) fn poll_chunk(&mut self, cancel: &AtomicBool) -> Result<OutputChunk, SidecarError> {
        if self.failed
            || self.outgoing.is_some()
            || !self.pending.is_empty()
            || (self.sent != 0 && !self.code_sent)
            || cancel.load(Ordering::Acquire)
        {
            return Err(failure());
        }
        let mut chunk = [0; 4096];
        match self.output.read(&mut chunk) {
            Ok(0) => Ok(OutputChunk::Eof),
            Ok(count) => {
                self.received += count;
                if self.received > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES {
                    self.failed = true;
                    return Err(failure());
                }
                Ok(OutputChunk::Bytes(chunk[..count].to_vec()))
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                Ok(OutputChunk::Pending)
            }
            Err(_) => {
                self.failed = true;
                Err(failure())
            }
        }
    }
    /// Must attach only to a child already held by the process/lifecycle owner.
    /// Caller must drain or redirect stderr separately. On failure the child and
    /// both pipe handles remain with the caller (possibly set to nonblocking).
    pub(crate) fn attach(child: &mut Child) -> Result<Self, SidecarError> {
        nonblocking(child.stdin.as_ref().ok_or_else(failure)?)?;
        nonblocking(child.stdout.as_ref().ok_or_else(failure)?)?;
        Ok(Self {
            input: child.stdin.take().ok_or_else(failure)?,
            output: child.stdout.take().ok_or_else(failure)?,
            pending: Vec::new(),
            received: 0,
            sent: 0,
            code_sent: false,
            failed: false,
            outgoing: None,
            outgoing_hold: None,
            outgoing_progress: Instant::now(),
            send_stall: SEND_STALL_MAX,
            session_lines: None,
            scanned: 0,
            large_line: None,
            slot_wait: None,
            limits: LARGE_LINE_LIMITS,
        })
    }

    pub(crate) fn submit_claude_code(
        &mut self,
        code: &str,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if self.sent != 0
            || self.code_sent
            || code.is_empty()
            || code.len() > 4096
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-#.=~".contains(&b))
        {
            return Err(failure());
        }
        let mut bytes = code.as_bytes().to_vec();
        bytes.push(b'\n');
        let result = self.send_inner(&bytes, Duration::from_secs(1), cancel, observe);
        bytes.fill(0);
        self.code_sent = true;
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn send(
        &mut self,
        frame: LoginRequest,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<(), SidecarError> {
        self.send_observed(frame, timeout, cancel, &mut || Ok(()))
    }

    pub(crate) fn send_observed(
        &mut self,
        frame: LoginRequest,
        timeout: Duration,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        let result = self.send_inner(frame.as_str().as_bytes(), timeout, cancel, observe);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub(crate) fn send_preflight(
        &mut self,
        frame: elsecrew_core::provider::vendor_observation::codex_preflight::exchange::PreflightRequest,
        timeout: Duration,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        let result = self.send_inner(frame.as_str().as_bytes(), timeout, cancel, observe);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Native opt-in test only. Offline turn needs additional approval and the
    /// caller's outer OS network-denying sandbox; never a real login profile.
    #[cfg(test)]
    pub(crate) fn send_hook_probe(
        &mut self,
        request: HookProbeRequest<'_>,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        let request = match request {
            HookProbeRequest::Inventory(cwd) if cwd.is_absolute() =>
                serde_json::json!({"id":100,"method":"hooks/list","params":{"cwds":[cwd]}}),
            HookProbeRequest::Start(cwd) if cwd.is_absolute() => serde_json::json!({
                "id":99, "method":"thread/start",
                "params":{"cwd":cwd,"model":"test-model","approvalPolicy":"never",
                    "sandbox":"read-only","ephemeral":true,
                    "config":{"bypass_hook_trust":true}}
            }),
            HookProbeRequest::OfflineTurn(id) if !id.is_empty() && id.len() <= 128
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && std::env::var("ELSECREW_APPROVED_OFFLINE_HOOK_TURN").as_deref() == Ok("1") =>
                serde_json::json!({"id":101,"method":"turn/start","params":{
                    "threadId":id,"input":[{"type":"text","text":"Do not use tools. Reply OK."}]
                }}),
            _ => return Err(failure()),
        };
        let frame = format!("{request}\n");
        let result = self.send_inner(frame.as_bytes(), Duration::from_secs(15), cancel, observe);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn send_inner(
        &mut self,
        frame: &[u8],
        timeout: Duration,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if self.failed
            || self.outgoing.is_some()
            || frame.len() < 2
            || frame.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES
            || frame.last() != Some(&b'\n')
            || frame[..frame.len() - 1].contains(&b'\n')
            || self.sent + frame.len() > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES
        {
            return Err(failure());
        }
        let start = Instant::now();
        let mut written = 0;
        while written < frame.len() {
            check_wait(start, timeout, cancel)?;
            observe()?;
            check_wait(start, timeout, cancel)?;
            match self.input.write(&frame[written..]) {
                Ok(0) => return Err(failure()),
                Ok(count) => {
                    written += count;
                    self.sent += count;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(failure()),
            }
        }
        Ok(())
    }

    pub(crate) fn receive(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<BoundedObservation, SidecarError> {
        self.receive_observed(timeout, cancel, &mut || Ok(()))
    }

    pub(crate) fn receive_observed(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<BoundedObservation, SidecarError> {
        let result = self.receive_inner(timeout, cancel, observe);
        if result.is_err() {
            self.failed = true;
            self.drop_pending();
        }
        result
    }

    fn receive_inner(
        &mut self,
        timeout: Duration,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<BoundedObservation, SidecarError> {
        let start = Instant::now();
        loop {
            check_wait(start, timeout, cancel)?;
            let frame = self.poll_inner(cancel, observe)?;
            check_wait(start, timeout, cancel)?;
            if let Some(frame) = frame {
                return Ok(frame);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// A bounded, nonblocking poll: no bytes yet is not EOF or a timeout.
    pub(crate) fn poll_observed(
        &mut self,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<Option<BoundedObservation>, SidecarError> {
        let result = self.poll_inner(cancel, observe);
        if result.is_err() {
            self.failed = true;
            self.drop_pending();
        }
        result
    }

    fn poll_inner(
        &mut self,
        cancel: &AtomicBool,
        observe: &mut impl FnMut() -> Result<(), SidecarError>,
    ) -> Result<Option<BoundedObservation>, SidecarError> {
        if self.failed || self.outgoing.is_some() {
            return Err(failure());
        }
        for _ in 0..32 {
            if cancel.load(Ordering::Acquire) {
                return Err(failure());
            }
            observe()?;
            if cancel.load(Ordering::Acquire) {
                return Err(failure());
            }
            let line_limit = if self.session_lines.is_some() {
                PROVIDER_SESSION_LINE_RAW_MAX_BYTES
            } else {
                PROVIDER_OBSERVATION_LINE_MAX_BYTES
            };
            let found = self.pending[self.scanned..].iter().position(|byte| *byte == b'\n');
            if let Some(offset) = found {
                let end = self.scanned + offset;
                self.scanned = 0;
                if end == 0 || end > line_limit {
                    return Err(refuse_line(self.session_lines.is_some(), "over the session line bound"));
                }
                // Take the line without copying it; only what follows is moved.
                let rest = self.pending.split_off(end + 1);
                let bytes = std::mem::replace(&mut self.pending, rest);
                if self.pending.len() <= PROVIDER_OBSERVATION_LINE_MAX_BYTES {
                    self.large_line = None;
                }
                if end > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
                    let policy = self.session_lines.ok_or_else(failure)?;
                    return reduce_oversized_line(&bytes, policy)
                        .map(Some)
                        .map_err(|refusal| refuse_line(true, refusal.as_str()));
                }
                return BoundedObservation::new(bytes)
                    .map(Some)
                    .map_err(|_| failure());
            }
            self.scanned = self.pending.len();
            if self.pending.len() > line_limit {
                return Err(refuse_line(self.session_lines.is_some(), "over the session line bound"));
            }
            // Past the observation bound a line needs one of the process-wide
            // slots; without one, read nothing more until a slot is free. The
            // bound is on the line's body: `bound` bytes and no newline yet
            // may still be a normal line, so the slot is needed only once a
            // byte past the bound arrived without its newline (PR #24 review I1).
            if self.session_lines.is_some()
                && self.pending.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES
                && self.large_line.is_none()
            {
                match LargeLineSlot::try_take() {
                    Some(slot) => {
                        let now = Instant::now();
                        self.slot_wait = None;
                        self.large_line = Some(HeldLine { _slot: slot, since: now, progress: now, len: self.pending.len() });
                    }
                    None => {
                        let since = *self.slot_wait.get_or_insert_with(Instant::now);
                        if since.elapsed() > self.limits.wait {
                            return Err(refuse_line(true, "waited too long for a large-line slot"));
                        }
                        return Ok(None);
                    }
                }
            }
            // A session reads larger pieces so a multi-megabyte line arrives in
            // a few polls; other exchanges keep the small read.
            let mut chunk = [0u8; 64 * 1024];
            let mut size = if self.session_lines.is_some() { chunk.len() } else { 4096 };
            // Without a slot a read stops one byte past the observation bound
            // (enough to see the newline of a line exactly at the bound): the
            // check above must see any longer line. A short read (a busy
            // machine) once left 120 KiB here, and the next 64 KiB read
            // finished a 150 KiB line that was then parsed without a slot
            // (flaky `a_session_waiting_too_long_for_a_slot_fails_alone`).
            if self.session_lines.is_some() && self.large_line.is_none() && self.pending.len() <= PROVIDER_OBSERVATION_LINE_MAX_BYTES {
                size = size.min(PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1 - self.pending.len());
            }
            match self.output.read(&mut chunk[..size]) {
                Ok(0) => return Err(failure()), // including partial-frame EOF
                Ok(count) => {
                    self.received += count;
                    if self.received > self.stream_limit() {
                        return Err(failure());
                    }
                    self.pending.extend_from_slice(&chunk[..count]);
                    if let Some(held) = self.large_line.as_mut() {
                        if self.pending.len() > held.len {
                            held.len = self.pending.len();
                            held.progress = Instant::now();
                        }
                        if held.since.elapsed() > self.limits.hold {
                            return Err(refuse_line(true, "a large line took too long"));
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Some(held) = &self.large_line {
                        if held.since.elapsed() > self.limits.hold {
                            return Err(refuse_line(true, "a large line took too long"));
                        }
                        if held.progress.elapsed() > self.limits.stall {
                            return Err(refuse_line(true, "a large line stopped arriving"));
                        }
                    }
                    return Ok(None);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(failure()),
            }
        }
        Ok(None)
    }
}

/// A session line that could not be observed. The reason is a fixed name,
/// never the provider's text (PR #17 review M2).
fn refuse_line(session: bool, reason: &'static str) -> SidecarError {
    if session {
        eprintln!("provider session line refused: {reason}");
    }
    failure()
}

fn check_wait(start: Instant, timeout: Duration, cancel: &AtomicBool) -> Result<(), SidecarError> {
    if timeout.is_zero()
        || timeout > Duration::from_secs(30)
        || start.elapsed() >= timeout
        || cancel.load(Ordering::Acquire)
    {
        return Err(failure());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use elsecrew_core::provider::vendor_observation::codex_login::LoginExchange;
    use std::process::{Command, Stdio};
    struct Fixture(Child);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn fixture(script: Option<&str>) -> Fixture {
        let mut command = Command::new(if script.is_some() {
            "/bin/sh"
        } else {
            "/bin/cat"
        });
        if let Some(script) = script {
            command.args(["-c", script]);
        }
        Fixture(
            command
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    #[test]
    fn queued_write_yields_under_backpressure_and_cancel_is_sticky() {
        let mut child = fixture(Some("exec /bin/sleep 5"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        let mut bytes = vec![b'x'; PROVIDER_OBSERVATION_LINE_MAX_BYTES - 1];
        bytes.push(b'\n');
        io.queue_bytes(&bytes).unwrap();
        let cancel = AtomicBool::new(false);
        for _ in 0..128 {
            assert!(!io.flush_native(&cancel, &mut || Ok(())).unwrap());
        }
        let written = io.outgoing.as_ref().unwrap().1;
        assert!(written > 0 && written < bytes.len());
        assert_eq!(io.sent, written);
        cancel.store(true, Ordering::Release);
        assert!(io.flush_native(&cancel, &mut || Ok(())).is_err());
        cancel.store(false, Ordering::Release);
        assert!(io.outgoing.is_none());
        assert!(io.queue_bytes(b"{}\n").is_err());
        assert!(io.flush_native(&cancel, &mut || Ok(())).is_err());
    }

    #[test]
    fn queued_partial_writes_preserve_exact_frame_and_reject_overlap() {
        let mut child = fixture(Some(
            "IFS= read -r line; printf '%s\\n' \"$line\"; IFS= read -r hold",
        ));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        let bytes = format!("{{\"text\":\"{}\"}}\n", "x".repeat(96 * 1024));
        let cancel = AtomicBool::new(false);
        io.queue_bytes(bytes.as_bytes()).unwrap();
        let start = Instant::now();
        while !io.flush_native(&cancel, &mut || Ok(())).unwrap() {
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(io.sent, bytes.len());
        // Inspect this synthetic peer's bytes directly, without exposing raw
        // provider observations in the production API.
        let mut echoed = Vec::new();
        while echoed.len() < bytes.len() {
            let mut chunk = [0; 4096];
            match io.output.read(&mut chunk) {
                Ok(0) => panic!("early EOF"),
                Ok(count) => echoed.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(start.elapsed() < Duration::from_secs(3));
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("{error}"),
            }
        }
        assert_eq!(echoed, bytes.as_bytes());
        io.queue_bytes(b"{}\n").unwrap();
        assert!(io.queue_bytes(b"{}\n").is_err());
        assert!(io.outgoing.is_none());
        assert!(io.flush_native(&cancel, &mut || Ok(())).is_err());
    }

    #[test]
    fn real_pipe_roundtrip_and_buffered_frames() {
        use elsecrew_core::provider::vendor_observation::{AuthObservation, Vendor};
        let mut child = fixture(Some("read request; printf '%s\n%s\n' '{\"account\":null,\"requiresOpenaiAuth\":true}' '{\"account\":{\"type\":\"chatgpt\"},\"requiresOpenaiAuth\":true}'"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        let cancel = AtomicBool::new(false);
        let frames = [
            b"{\"account\":null,\"requiresOpenaiAuth\":true}\n".as_slice(),
            b"{\"account\":{\"type\":\"chatgpt\"},\"requiresOpenaiAuth\":true}\n",
        ];
        let (_, request) = LoginExchange::begin(1).unwrap();
        io.send(request, Duration::from_secs(1), &cancel).unwrap();
        for expected in [
            AuthObservation::NotAuthenticated,
            AuthObservation::CredentialsPresent,
        ] {
            let response = io.receive(Duration::from_secs(1), &cancel).unwrap();
            assert_eq!(
                Vendor::Codex01491.observe_auth(&response, 0).unwrap(),
                expected
            );
        }
        assert_eq!(
            io.received,
            frames.iter().map(|frame| frame.len()).sum::<usize>()
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn permission_probe_preserves_coalesced_lines_and_rejects_invalid_frames() {
        let mut child = fixture(None);
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        let cancel = AtomicBool::new(false);
        io.pending.extend_from_slice(b"{\"id\":1}\n{\"id\":2}\n");
        for expected in [b"{\"id\":1}\n", b"{\"id\":2}\n"] {
            match io.poll_permission_probe_chunk(&cancel).unwrap() {
                OutputChunk::Bytes(bytes) => assert_eq!(bytes, expected),
                _ => panic!("buffered line was lost"),
            }
        }
        assert!(io.pending.is_empty());
        io.pending.push(b'\n');
        assert!(io.poll_permission_probe_chunk(&cancel).is_err());
        io.pending = vec![b'x'; PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1];
        assert!(io.poll_permission_probe_chunk(&cancel).is_err());
        io.pending.clear();
        assert!(io
            .poll_permission_probe_chunk(&AtomicBool::new(true))
            .is_err());
    }
    #[test]
    fn timeout_cancel_partial_eof_and_oversize_poison_connection() {
        for script in [None, Some("printf partial"), Some("printf '\\n'")] {
            let mut child = fixture(script);
            let mut io = LoginIo::attach(&mut child.0).unwrap();
            let cancel = AtomicBool::new(false);
            assert!(io.receive(Duration::from_millis(30), &cancel).is_err());
            let (_, request) = LoginExchange::begin(1).unwrap();
            assert!(io.send(request, Duration::from_secs(1), &cancel).is_err());
        }
        let mut child = fixture(None);
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        assert!(io
            .receive(Duration::from_secs(1), &AtomicBool::new(true))
            .is_err());
        let mut child = fixture(None);
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        assert!(io
            .send_inner(
                &vec![b'x'; PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1],
                Duration::from_secs(1),
                &AtomicBool::new(false),
                &mut || Ok(())
            )
            .is_err());
    }

    /// D2 (2026-11-26): ten image tool results of ~148 KB each, as Claude
    /// prints them after reading a 55 KB PNG, then an ordinary line.
    fn large_image_lines_script(lines: usize) -> String {
        format!(
            "i=0; while [ $i -lt {lines} ]; do \
               printf '{{\"type\":\"user\",\"uuid\":\"u%s\",\"message\":{{\"content\":[{{\"type\":\"image\",\"source\":{{\"data\":\"' $i; \
               head -c 151552 /dev/zero | tr '\\0' A; printf '\"}}}}]}}}}\\n'; i=$((i+1)); done; \
             printf '{{\"type\":\"user\",\"uuid\":\"last\"}}\\n'; read hold"
        )
    }

    #[test]
    fn a_session_reads_many_large_image_lines_and_keeps_going() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = AtomicBool::new(false);
        let mut child = fixture(Some(&large_image_lines_script(10)));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let mut projection = elsecrew_core::provider::vendor_observation::claude_events::SessionProjection::new("s").unwrap();
        for index in 0..10 {
            let frame = io.receive(Duration::from_secs(5), &cancel).unwrap();
            assert!(frame.byte_len() < 2048, "line {index} was shortened to {} bytes", frame.byte_len());
            // Still a well-formed frame the session parser accepts and ignores.
            assert!(projection.project(&frame).is_ok(), "line {index}");
        }
        let last = io.receive(Duration::from_secs(5), &cancel).unwrap();
        assert_eq!(last.byte_len(), b"{\"type\":\"user\",\"uuid\":\"last\"}\n".len());
        assert!(io.received > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES, "past the old 1 MiB connection total");
        assert!(!io.failed);
    }

    /// Review I1: several sessions send large lines at the same time. At most
    /// `LARGE_LINE_SLOTS` connections in the process hold one; the others read
    /// nothing past the observation bound plus one byte until a slot is free,
    /// and every session still gets every line.
    #[test]
    fn concurrent_large_lines_share_the_process_wide_slots() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = AtomicBool::new(false);
        let mut children = Vec::new();
        let mut ios = Vec::new();
        for _ in 0..4 {
            let mut child = fixture(Some(&large_image_lines_script(3)));
            let mut io = LoginIo::attach(&mut child.0).unwrap();
            io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
            children.push(child);
            ios.push(io);
        }
        let mut frames = [0usize; 4];
        let deadline = Instant::now() + Duration::from_secs(20);
        while frames.iter().any(|count| *count < 4) {
            assert!(Instant::now() < deadline, "sessions stalled: {frames:?}");
            for (index, io) in ios.iter_mut().enumerate() {
                if frames[index] == 4 { continue; }
                if io.poll_observed(&cancel, &mut || Ok(())).unwrap().is_some() {
                    frames[index] += 1;
                }
                if io.large_line.is_none() {
                    assert!(io.pending.len() <= PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1,
                        "a connection without a slot buffered {} bytes", io.pending.len());
                }
            }
            assert!(LARGE_LINES_HELD.load(Ordering::Acquire) <= LARGE_LINE_SLOTS);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(LARGE_LINES_PEAK.load(Ordering::Acquire) <= LARGE_LINE_SLOTS);
        assert!(ios.iter().all(|io| io.large_line.is_none() && !io.failed));
    }

    /// A line refused while it holds a slot (too many small values) returns
    /// the slot and the buffer's memory.
    #[test]
    fn a_refused_large_line_returns_its_slot() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = AtomicBool::new(false);
        let script = "printf '{\"type\":\"user\",\"items\":['; i=0; while [ $i -lt 4000 ]; do \
            printf '1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,'; i=$((i+1)); done; printf '1]}\\n'; read hold";
        let mut child = fixture(Some(script));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        assert!(io.receive(Duration::from_secs(10), &cancel).is_err());
        assert!(io.large_line.is_none());
        assert_eq!(io.pending.capacity(), 0);
    }

    /// A provider that writes past the observation bound and then stops,
    /// keeping its pipe open (it never sends the newline).
    const STUCK_LINE: &str = "printf '{\"type\":\"user\",\"pad\":\"'; head -c 200000 /dev/zero | tr '\\0' A; read hold";

    fn stuck_holders(stall: Duration) -> (Vec<Fixture>, Vec<LoginIo>) {
        let cancel = AtomicBool::new(false);
        let mut children = Vec::new();
        let mut ios = Vec::new();
        for _ in 0..LARGE_LINE_SLOTS {
            let mut child = fixture(Some(STUCK_LINE));
            let mut io = LoginIo::attach(&mut child.0).unwrap();
            io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
            io.set_large_line_limits(Duration::from_secs(30), stall, Duration::from_secs(60));
            let deadline = Instant::now() + Duration::from_secs(5);
            while io.large_line.is_none() {
                assert!(Instant::now() < deadline, "the stuck provider never reached a slot");
                assert!(io.poll_observed(&cancel, &mut || Ok(())).unwrap().is_none());
                std::thread::sleep(Duration::from_millis(1));
            }
            children.push(child);
            ios.push(io);
        }
        assert_eq!(LARGE_LINES_HELD.load(Ordering::Acquire), LARGE_LINE_SLOTS);
        (children, ios)
    }

    /// PR #17 re-review: two unfinished lines hold both slots and stop. They
    /// are refused once they stop growing, their slots come back, and a third
    /// run with large lines proceeds within the deadline.
    #[test]
    fn stuck_large_lines_release_their_slots_and_another_run_proceeds() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = AtomicBool::new(false);
        let (_stuck_children, mut stuck) = stuck_holders(Duration::from_millis(300));
        let mut child = fixture(Some(&large_image_lines_script(3)));
        let mut third = LoginIo::attach(&mut child.0).unwrap();
        third.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        third.set_large_line_limits(Duration::from_secs(30), Duration::from_secs(10), Duration::from_secs(5));
        let started = Instant::now();
        let mut frames = 0;
        let mut refused = [false; LARGE_LINE_SLOTS];
        while frames < 4 {
            assert!(started.elapsed() < Duration::from_secs(5), "the third run did not proceed ({frames} frames)");
            for (index, io) in stuck.iter_mut().enumerate() {
                if !refused[index] && io.poll_observed(&cancel, &mut || Ok(())).is_err() {
                    refused[index] = true;
                    assert!(io.large_line.is_none(), "a refused line returns its slot");
                }
            }
            if third.poll_observed(&cancel, &mut || Ok(())).unwrap().is_some() {
                frames += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(refused, [true; LARGE_LINE_SLOTS], "both stuck lines were refused");
        assert!(!third.failed && third.large_line.is_none());
        assert_eq!(LARGE_LINES_HELD.load(Ordering::Acquire), 0);
    }

    /// PR #17 re-review: a connection that cannot get a slot in time fails on
    /// its own; the holders and their runs are untouched.
    #[test]
    fn a_session_waiting_too_long_for_a_slot_fails_alone() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = AtomicBool::new(false);
        let (_stuck_children, mut stuck) = stuck_holders(Duration::from_secs(30));
        let mut child = fixture(Some(&large_image_lines_script(1)));
        let mut waiter = LoginIo::attach(&mut child.0).unwrap();
        waiter.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        waiter.set_large_line_limits(Duration::from_secs(30), Duration::from_secs(10), Duration::from_millis(300));
        let started = Instant::now();
        let failed = loop {
            assert!(started.elapsed() < Duration::from_secs(5), "the waiter never gave up");
            match waiter.poll_observed(&cancel, &mut || Ok(())) {
                Ok(None) => {
                    assert!(waiter.pending.len() <= PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(Some(_)) => panic!("the waiter got a line without a slot"),
                Err(_) => break started.elapsed(),
            }
        };
        assert!(failed >= Duration::from_millis(300));
        assert!(waiter.failed && waiter.pending.capacity() == 0 && waiter.slot_wait.is_none());
        for io in &mut stuck {
            assert!(io.poll_observed(&cancel, &mut || Ok(())).unwrap().is_none(), "the holders keep going");
            assert!(io.large_line.is_some() && !io.failed);
        }
        drop(stuck);
        assert_eq!(LARGE_LINES_HELD.load(Ordering::Acquire), 0);
    }

    /// Polls a session with both slots taken until its line arrives (Some),
    /// or it waits for a slot past `settled` (None), or it fails (Err). While
    /// it waits it never holds more than the bound plus one byte.
    fn poll_without_slots(io: &mut LoginIo, settled: Duration) -> Result<Option<BoundedObservation>, (Duration, Duration)> {
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let mut waiting_since = None;
        loop {
            assert!(started.elapsed() < Duration::from_secs(20), "the session neither ended nor waited");
            match io.poll_observed(&cancel, &mut || Ok(())) {
                Ok(Some(line)) => return Ok(Some(line)),
                Ok(None) => {
                    assert!(io.large_line.is_none(), "a slot was taken while both were held");
                    assert!(io.pending.len() <= PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1, "read {} bytes without a slot", io.pending.len());
                    if io.slot_wait.is_some() {
                        let since = *waiting_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= settled { return Ok(None); }
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => return Err((started.elapsed(), waiting_since.map_or(Duration::ZERO, |since| since.elapsed()))),
            }
        }
    }

    /// PR #24 review I1: the bound is on a line's body. With both slots
    /// taken, a body of exactly 128 KiB (and one byte less) arrives as a
    /// normal line; one byte more waits for a slot.
    #[test]
    fn a_line_at_the_bound_needs_no_slot_and_one_byte_more_does() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let held: Vec<_> = (0..LARGE_LINE_SLOTS).map(|_| LargeLineSlot::try_take().unwrap()).collect();
        for body in [PROVIDER_OBSERVATION_LINE_MAX_BYTES - 1, PROVIDER_OBSERVATION_LINE_MAX_BYTES, PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1] {
            let script = format!("head -c {body} /dev/zero | tr '\\0' A; printf '\\n'; read hold");
            let mut child = fixture(Some(&script));
            let mut io = LoginIo::attach(&mut child.0).unwrap();
            io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
            io.set_large_line_limits(Duration::from_secs(30), Duration::from_secs(10), Duration::from_secs(30));
            match poll_without_slots(&mut io, Duration::from_millis(300)) {
                Ok(Some(line)) => {
                    assert!(body <= PROVIDER_OBSERVATION_LINE_MAX_BYTES, "{body}: a line over the bound came without a slot");
                    assert_eq!(line.byte_len(), body + 1, "{body}");
                }
                Ok(None) => {
                    assert_eq!(body, PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1, "{body}: a line within the bound waited for a slot");
                    assert_eq!(io.pending.len(), PROVIDER_OBSERVATION_LINE_MAX_BYTES + 1);
                }
                Err(_) => panic!("{body}: the session failed"),
            }
        }
        drop(held);
        assert_eq!(LARGE_LINES_HELD.load(Ordering::Acquire), 0);
    }

    /// ADE stabilization B: 120 KiB of a line arrive first, then the rest at
    /// once. A read that crossed the bound used to finish the line there and
    /// hand it over without a slot; now the read stops past the bound, the
    /// slot check sees the line, and with no slot free the line waits its
    /// full wait and then fails alone. The provider writes the rest only
    /// once the test drained the first part (no fixed delays, PR #24 M1).
    #[test]
    fn a_line_that_crosses_the_bound_in_one_read_still_needs_a_slot() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let held: Vec<_> = (0..LARGE_LINE_SLOTS).map(|_| LargeLineSlot::try_take().unwrap()).collect();
        let cancel = AtomicBool::new(false);
        let temp = tempfile::tempdir().unwrap();
        let go = temp.path().join("go");
        let script = format!(
            "printf '{{\"type\":\"user\",\"pad\":\"'; head -c 122880 /dev/zero | tr '\\0' A; \
             while [ ! -e '{}' ]; do /bin/sleep 0.01; done; \
             head -c 30000 /dev/zero | tr '\\0' A; printf '\"}}\\n'; read hold",
            go.display()
        );
        let mut child = fixture(Some(&script));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let wait = Duration::from_millis(300);
        io.set_large_line_limits(Duration::from_secs(30), Duration::from_secs(10), wait);
        let first = "{\"type\":\"user\",\"pad\":\"".len() + 122880;
        let started = Instant::now();
        while io.pending.len() < first {
            assert!(started.elapsed() < Duration::from_secs(20), "the first part never arrived");
            assert!(io.poll_observed(&cancel, &mut || Ok(())).unwrap().is_none());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(io.pending.len(), first);
        std::fs::write(&go, b"").unwrap();
        match poll_without_slots(&mut io, Duration::from_secs(60)) {
            Ok(_) => panic!("a line over the bound was handed over without a slot"),
            // It failed because its slot wait ran out, not for another reason.
            Err((_, waited)) => assert!(waited >= wait, "failed after waiting {waited:?}"),
        }
        assert!(io.failed && io.large_line.is_none());
        drop(held);
        assert_eq!(LARGE_LINES_HELD.load(Ordering::Acquire), 0);
    }

    /// 2026-11-26 big Write: a session may send an allow reply over the
    /// 128 KiB line bound (the echoed input); a login or preflight exchange may not.
    #[test]
    fn only_a_session_sends_frames_over_the_line_bound() {
        let mut frame = vec![b'x'; PROVIDER_OBSERVATION_LINE_MAX_BYTES + 10];
        frame.push(b'\n');
        let mut child = fixture(None);
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        assert!(io.queue_bytes(&frame).is_err());
        let mut child = fixture(Some("exec /bin/cat >/dev/null"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        io.queue_bytes(&frame).unwrap();
        let cancel = AtomicBool::new(false);
        while !io.flush_native(&cancel, &mut || Ok(())).unwrap() {}
        let mut over = vec![b'x'; elsecrew_core::provider::vendor_observation::claude_approval::LARGE_TOOL_REPLY_MAX_BYTES + 1];
        over.push(b'\n');
        let mut child = fixture(None);
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        assert!(io.queue_bytes(&over).is_err(), "still bounded");
    }

    /// A large-input budget as an allow reply carries it: a 200 KB Write
    /// request allowed through the public session parser.
    fn reply_hold() -> elsecrew_core::provider::vendor_observation::claude_approval::LargeInputHold {
        use elsecrew_core::provider::vendor_observation::{claude_approval::Observation, claude_init::ExpectedInit, claude_session::{Event, Session}};
        let mut session = Session::new(&ExpectedInit { cwd: "/snapshot", model: "model", permission_mode: "default", tools: &["Write"] });
        let init = b"{\"type\":\"system\",\"subtype\":\"init\",\"uuid\":\"init\",\"session_id\":\"s\",\"cwd\":\"/snapshot\",\"model\":\"model\",\"permissionMode\":\"default\",\"tools\":[\"Write\"],\"mcp_servers\":[],\"skills\":[],\"plugins\":[]}\n";
        session.observe(&BoundedObservation::new(init.to_vec()).unwrap()).unwrap();
        let frame = format!("{{\"type\":\"control_request\",\"request_id\":\"w\",\"request\":{{\"subtype\":\"can_use_tool\",\"tool_name\":\"Write\",\"input\":{{\"file_path\":\"a.txt\",\"content\":\"{}\"}},\"tool_use_id\":\"t\"}}}}\n", "y".repeat(200_000));
        let observed = reduce_oversized_line(frame.as_bytes(), OversizedLinePolicy::ClaudeStreamJson).unwrap();
        let Event::Permission(Observation::Requested(request)) = session.observe(&observed).unwrap() else { panic!("expected a request") };
        let mut reply = session.allow_once(&request).unwrap();
        drop(request);
        reply.take_hold().expect("a large reply carries its hold")
    }

    /// PR #19 review I1: an allow reply keeps its large-input budget while it
    /// is written; the budget returns when the write completes, when the
    /// provider stops reading past the send deadline (the connection fails),
    /// and when the connection is dropped.
    #[test]
    fn an_allow_reply_keeps_its_budget_until_written_and_a_stalled_write_fails() {
        use elsecrew_core::provider::vendor_observation::claude_approval::large_tool_inputs_held;
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = AtomicBool::new(false);
        let base = large_tool_inputs_held();
        let mut frame = vec![b'z'; 300_000];
        frame.push(b'\n');
        // Written whole: returned.
        let mut child = fixture(Some("exec /bin/cat >/dev/null"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        io.queue_bytes(&frame).unwrap();
        io.outgoing_hold = Some(reply_hold());
        assert!(large_tool_inputs_held() > base);
        while !io.flush_native(&cancel, &mut || Ok(())).unwrap() {}
        assert_eq!(large_tool_inputs_held(), base, "written: returned");
        // The provider stops reading: past the send deadline the connection fails.
        let mut child = fixture(Some("exec /bin/sleep 30"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        io.send_stall = Duration::from_millis(300);
        io.queue_bytes(&frame).unwrap();
        io.outgoing_hold = Some(reply_hold());
        let started = Instant::now();
        let failed = loop {
            assert!(started.elapsed() < Duration::from_secs(5), "a stalled write never gave up");
            match io.flush_native(&cancel, &mut || Ok(())) {
                Ok(true) => panic!("a provider that reads nothing took the whole reply"),
                Ok(false) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => break started.elapsed(),
            }
        };
        assert!(failed >= Duration::from_millis(300));
        assert!(io.failed && io.outgoing.is_none());
        assert_eq!(large_tool_inputs_held(), base, "stalled: returned");
        // Dropped mid-write: returned.
        let mut child = fixture(Some("exec /bin/sleep 30"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        io.queue_bytes(&frame).unwrap();
        io.outgoing_hold = Some(reply_hold());
        let _ = io.flush_native(&cancel, &mut || Ok(()));
        drop(io);
        assert_eq!(large_tool_inputs_held(), base, "dropped: returned");
    }

    #[test]
    fn without_session_mode_a_large_image_line_still_fails() {
        let cancel = AtomicBool::new(false);
        let mut child = fixture(Some(&large_image_lines_script(1)));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        assert!(io.receive(Duration::from_secs(5), &cancel).is_err());
        assert!(io.failed);
    }

    #[test]
    fn a_session_still_fails_on_an_oversized_permission_request_or_runaway_line() {
        let _slots = LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = AtomicBool::new(false);
        let script = "printf '{\"type\":\"control_request\",\"request_id\":\"r\",\"request\":{\"input\":{\"content\":\"'; \
            head -c 200000 /dev/zero | tr '\\0' x; printf '\"}}}\\n'; read hold";
        let mut child = fixture(Some(script));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        assert!(io.receive(Duration::from_secs(5), &cancel).is_err(), "a permission request is never shortened");
        // An unterminated line past the session raw bound fails instead of growing.
        let mut io = LoginIo::attach(&mut fixture(Some("head -c 9000000 /dev/zero | tr '\\0' x; read hold")).0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        assert!(io.receive(Duration::from_secs(10), &cancel).is_err());
        assert!(io.pending.is_empty());
        // The mode cannot be switched to another vendor's policy.
        let mut child = fixture(None);
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        io.admit_session_lines(OversizedLinePolicy::ClaudeStreamJson).unwrap();
        assert!(io.admit_session_lines(OversizedLinePolicy::CodexAppServer).is_err());
    }

    #[test]
    fn blocked_stdin_and_unterminated_oversized_stdout_are_bounded() {
        let cancel = AtomicBool::new(false);
        let mut child = fixture(Some("exec /bin/sleep 10"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        let start = Instant::now();
        for id in 1..=100_000 {
            let (_, request) = LoginExchange::begin(id).unwrap();
            if io
                .send(request, Duration::from_millis(30), &cancel)
                .is_err()
            {
                break;
            }
        }
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(io.failed);
        let mut child = fixture(Some("printf '%0135170d' 0; read hold"));
        let mut io = LoginIo::attach(&mut child.0).unwrap();
        assert!(io.receive(Duration::from_secs(1), &cancel).is_err());
        assert!(io.received > PROVIDER_OBSERVATION_LINE_MAX_BYTES);
        assert!(io.pending.is_empty());
    }
}
