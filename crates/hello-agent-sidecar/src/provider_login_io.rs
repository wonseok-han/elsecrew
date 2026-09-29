//! Bounded nonblocking login stdio. The caller retains the child and its tree
//! tracker, and must terminate/persist uncertain cleanup on every failure.
//! This is not a spawn API or authorization to send arbitrary provider methods.
use crate::SidecarError;
use hello_agent_core::provider::vendor_observation::codex_login::LoginRequest;
use hello_agent_core::provider::{
    BoundedObservation, PROVIDER_OBSERVATION_LINE_MAX_BYTES, PROVIDER_OBSERVATION_TOTAL_MAX_BYTES,
};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
}

/// Only requests already admitted by the corresponding protocol state machine.
pub(crate) enum NativeFrame {
    Preflight(
        hello_agent_core::provider::vendor_observation::codex_preflight::exchange::PreflightRequest,
    ),
    Turn(hello_agent_core::provider::vendor_observation::codex_turn::TurnRequest),
    Approval(hello_agent_core::provider::vendor_observation::codex_approval::ApprovalReply),
    ConfinedAccept(crate::provider_preflight_session::ConfinedAcceptReply),
}

impl NativeFrame {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Preflight(frame) => frame.as_str().as_bytes(),
            Self::Turn(frame) => frame.as_str().as_bytes(),
            Self::Approval(frame) => frame.as_str().as_bytes(),
            Self::ConfinedAccept(frame) => frame.as_bytes(),
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
    pub(crate) fn queue_native(&mut self, frame: NativeFrame) -> Result<(), SidecarError> {
        self.queue_bytes(frame.bytes())
    }

    fn queue_bytes(&mut self, bytes: &[u8]) -> Result<(), SidecarError> {
        if self.failed
            || self.outgoing.is_some()
            || bytes.len() < 2
            || bytes.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES
            || bytes.last() != Some(&b'\n')
            || bytes[..bytes.len() - 1].contains(&b'\n')
            || self.sent + bytes.len() > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES
        {
            self.failed = true;
            self.outgoing = None;
            return Err(failure());
        }
        self.outgoing = Some((bytes.to_vec(), 0));
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
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return Err(failure()),
            }
            let complete = *written == bytes.len();
            if complete {
                self.outgoing = None;
            }
            Ok(complete)
        })();
        if result.is_err() {
            self.failed = true;
            self.outgoing = None;
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
        frame: hello_agent_core::provider::vendor_observation::codex_preflight::exchange::PreflightRequest,
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
            self.pending.clear();
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
            self.pending.clear();
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
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                if end == 0 || end > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
                    return Err(failure());
                }
                let bytes: Vec<u8> = self.pending.drain(..=end).collect();
                return BoundedObservation::new(bytes)
                    .map(Some)
                    .map_err(|_| failure());
            }
            if self.pending.len() > PROVIDER_OBSERVATION_LINE_MAX_BYTES {
                return Err(failure());
            }
            let mut chunk = [0u8; 4096];
            match self.output.read(&mut chunk) {
                Ok(0) => return Err(failure()), // including partial-frame EOF
                Ok(count) => {
                    self.received += count;
                    if self.received > PROVIDER_OBSERVATION_TOTAL_MAX_BYTES {
                        return Err(failure());
                    }
                    self.pending.extend_from_slice(&chunk[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(None);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(failure()),
            }
        }
        Ok(None)
    }
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
    use hello_agent_core::provider::vendor_observation::codex_login::LoginExchange;
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
        use hello_agent_core::provider::vendor_observation::{AuthObservation, Vendor};
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
