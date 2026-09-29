//! Bounded opaque-byte transport for already admitted/connected sockets.
//! Does not resolve, dial, authenticate TLS, or authorize an upstream address.
//! The future broker owns admission, connection caps and the listener lease.
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

const BUFFER_SIZE: usize = 8192;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_LIFETIME: Duration = Duration::from_secs(600);
const ESTABLISHED: &[u8] = b"HTTP/1.1 200 Connection Established\r\n\r\n";

struct Direction {
    buffer: [u8; BUFFER_SIZE],
    start: usize,
    end: usize,
    eof: bool,
    shutdown: bool,
}

impl Default for Direction {
    fn default() -> Self {
        Self {
            buffer: [0; BUFFER_SIZE],
            start: 0,
            end: 0,
            eof: false,
            shutdown: false,
        }
    }
}

impl Direction {
    // At most one read and one write per tick, regardless of peer readiness.
    fn advance(
        &mut self,
        source: &mut impl Read,
        target: &mut impl Write,
        remaining: &mut u64,
    ) -> io::Result<()> {
        if self.start == self.end && !self.eof {
            match source.read(&mut self.buffer) {
                Ok(0) => self.eof = true,
                Ok(count) => {
                    *remaining = remaining
                        .checked_sub(count as u64)
                        .ok_or_else(|| io::Error::other("proxy byte budget exceeded"))?;
                    self.start = 0;
                    self.end = count;
                }
                Err(error) if retry_later(&error) => {}
                Err(error) => return Err(error),
            }
        }
        if self.start != self.end {
            match target.write(&self.buffer[self.start..self.end]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "proxy write closed",
                    ))
                }
                Ok(count) => self.start += count,
                Err(error) if retry_later(&error) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn drained(&self) -> bool {
        self.eof && self.start == self.end
    }

    fn clear(&mut self) {
        self.buffer.fill(0);
        self.start = 0;
        self.end = 0;
    }
}

fn retry_later(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

pub(super) struct Relay {
    downstream: TcpStream,
    upstream: TcpStream,
    to_upstream: Direction,
    to_downstream: Direction,
    started: Instant,
    remaining: u64,
    established: bool,
    closed: bool,
}

impl Relay {
    /// Backend-only: the caller must have authenticated CONNECT and connected
    /// a validated destination. No sockets or raw payloads escape this owner.
    pub(super) fn new(
        downstream: TcpStream,
        upstream: TcpStream,
        now: Instant,
    ) -> io::Result<Self> {
        downstream.set_nonblocking(true)?;
        upstream.set_nonblocking(true)?;
        let mut to_downstream = Direction::default();
        to_downstream.buffer[..ESTABLISHED.len()].copy_from_slice(ESTABLISHED);
        to_downstream.end = ESTABLISHED.len();
        Ok(Self {
            downstream,
            upstream,
            to_upstream: Direction::default(),
            to_downstream,
            started: now,
            remaining: MAX_BYTES,
            established: false,
            closed: false,
        })
    }

    /// false = pending, true = both EOFs drained. On any error both sockets
    /// close and future ticks fail; callers must drop/remove the relay.
    pub(super) fn advance(&mut self, now: Instant, canceled: bool) -> io::Result<bool> {
        if self.closed {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "proxy closed"));
        }
        let result = self.advance_open(now, canceled);
        if !matches!(result, Ok(false)) {
            self.close();
        }
        result
    }

    fn advance_open(&mut self, now: Instant, canceled: bool) -> io::Result<bool> {
        if canceled {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "proxy canceled"));
        }
        if now
            .checked_duration_since(self.started)
            .is_none_or(|elapsed| elapsed >= MAX_LIFETIME)
        {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "proxy lifetime exceeded",
            ));
        }
        transfer(
            &mut self.to_downstream,
            &mut self.to_upstream,
            &mut self.downstream,
            &mut self.upstream,
            &mut self.remaining,
            &mut self.established,
        )?;
        if self.to_upstream.drained() && !self.to_upstream.shutdown {
            self.upstream.shutdown(Shutdown::Write)?;
            self.to_upstream.shutdown = true;
        }
        if self.to_downstream.drained() && !self.to_downstream.shutdown {
            self.downstream.shutdown(Shutdown::Write)?;
            self.to_downstream.shutdown = true;
        }
        Ok(self.to_upstream.shutdown && self.to_downstream.shutdown)
    }

    fn close(&mut self) {
        let _ = self.downstream.shutdown(Shutdown::Both);
        let _ = self.upstream.shutdown(Shutdown::Both);
        self.to_upstream.clear();
        self.to_downstream.clear();
        self.closed = true;
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.close();
    }
}

fn transfer(
    to_downstream: &mut Direction,
    to_upstream: &mut Direction,
    downstream: &mut (impl Read + Write),
    upstream: &mut (impl Read + Write),
    remaining: &mut u64,
    established: &mut bool,
) -> io::Result<()> {
    to_downstream.advance(upstream, downstream, remaining)?;
    // This is a one-shot barrier for the seeded CONNECT response, not for
    // subsequent server payload. Never read client bytes before it drains.
    if !*established {
        if to_downstream.start != to_downstream.end {
            return Ok(());
        }
        *established = true;
    }
    to_upstream.advance(downstream, upstream, remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct SlowWriter {
        output: Vec<u8>,
        block: bool,
    }
    impl Write for SlowWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.block = !self.block;
            if self.block {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let count = bytes.len().min(3);
            self.output.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("no blocking flush");
        }
    }

    #[test]
    fn partial_writes_and_backpressure_preserve_bytes_and_eof() {
        let payload: Vec<u8> = (0..BUFFER_SIZE * 2 + 9).map(|n| (n % 251) as u8).collect();
        let mut input = VecDeque::from(payload.clone());
        let mut output = SlowWriter {
            output: Vec::new(),
            block: false,
        };
        let mut direction = Direction::default();
        let mut budget = MAX_BYTES;
        for _ in 0..payload.len() * 2 {
            direction
                .advance(&mut input, &mut output, &mut budget)
                .unwrap();
            if direction.drained() {
                break;
            }
        }
        assert!(direction.drained());
        assert_eq!(output.output, payload);
        assert_eq!(budget, MAX_BYTES - payload.len() as u64);
    }

    #[test]
    fn over_budget_payload_is_never_written() {
        let mut direction = Direction::default();
        let mut input = &b"secret payload"[..];
        let mut output = Vec::new();
        assert!(direction.advance(&mut input, &mut output, &mut 3).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn connect_response_must_drain_before_client_payload_is_read() {
        struct Peer {
            input: VecDeque<u8>,
            output: SlowWriter,
        }
        impl Read for Peer {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.input.read(bytes)
            }
        }
        impl Write for Peer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.output.write(bytes)
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("no flush");
            }
        }
        let payload = b"early client TLS bytes";
        let mut downstream = Peer {
            input: VecDeque::from(payload.to_vec()),
            output: SlowWriter {
                output: Vec::new(),
                block: false,
            },
        };
        let mut upstream = Peer {
            input: VecDeque::new(),
            output: SlowWriter {
                output: Vec::new(),
                block: false,
            },
        };
        let mut to_downstream = Direction::default();
        to_downstream.buffer[..ESTABLISHED.len()].copy_from_slice(ESTABLISHED);
        to_downstream.end = ESTABLISHED.len();
        let mut to_upstream = Direction::default();
        let mut remaining = MAX_BYTES;
        let mut established = false;
        for _ in 0..ESTABLISHED.len() * 2 {
            transfer(
                &mut to_downstream,
                &mut to_upstream,
                &mut downstream,
                &mut upstream,
                &mut remaining,
                &mut established,
            )
            .unwrap();
            if established {
                break;
            }
            assert_eq!(downstream.input.len(), payload.len());
            assert!(upstream.output.output.is_empty());
            assert_eq!(remaining, MAX_BYTES);
        }
        assert!(established);
        assert_eq!(downstream.output.output, ESTABLISHED);
        for _ in 0..payload.len() * 2 {
            transfer(
                &mut to_downstream,
                &mut to_upstream,
                &mut downstream,
                &mut upstream,
                &mut remaining,
                &mut established,
            )
            .unwrap();
        }
        assert_eq!(upstream.output.output, payload);
    }

    fn pair() -> (TcpStream, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (client, listener.accept().unwrap().0)
    }

    #[test]
    #[ignore = "loopback-only relay probe; no vendor/upstream network"]
    fn sockets_transfer_both_directions_and_preserve_half_close() {
        let (mut client, downstream) = pair();
        let (upstream, mut server) = pair();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let now = Instant::now();
        let mut relay = Relay::new(downstream, upstream, now).unwrap();
        relay.advance(now, false).unwrap();
        let mut header = vec![0; ESTABLISHED.len()];
        client.read_exact(&mut header).unwrap();
        assert_eq!(header, ESTABLISHED);
        client.write_all(b"client bytes").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        for _ in 0..20 {
            relay.advance(Instant::now(), false).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut request = Vec::new();
        server.read_to_end(&mut request).unwrap();
        assert_eq!(request, b"client bytes");
        // Upstream can still answer after the client's write half is closed.
        server.write_all(b"server bytes").unwrap();
        server.shutdown(Shutdown::Write).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !relay.advance(Instant::now(), false).unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert_eq!(response, b"server bytes");
        assert!(relay.advance(Instant::now(), false).is_err());
    }

    #[test]
    #[ignore = "loopback-only cancellation/deadline/drop probe"]
    fn terminal_paths_close_both_peers_and_cannot_resume() {
        for case in ["cancel", "deadline", "budget", "drop"] {
            let (mut client, downstream) = pair();
            let (upstream, mut server) = pair();
            for peer in [&client, &server] {
                peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            }
            let now = Instant::now();
            let mut relay = Relay::new(downstream, upstream, now).unwrap();
            if case == "budget" {
                relay.remaining = 0;
                client.write_all(b"x").unwrap();
            }
            if case != "drop" {
                let at = if case == "deadline" {
                    now + MAX_LIFETIME
                } else {
                    now
                };
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    if relay.advance(at, case == "cancel").is_err() {
                        break;
                    }
                    assert!(Instant::now() < deadline);
                }
                assert!(relay.advance(now, false).is_err());
                assert!(relay.to_downstream.buffer.iter().all(|byte| *byte == 0));
                assert!(relay.to_upstream.buffer.iter().all(|byte| *byte == 0));
            }
            drop(relay);
            for peer in [&mut client, &mut server] {
                let mut discarded = Vec::new();
                // RST is also terminal when closing a peer with unread input.
                if let Err(error) = peer.read_to_end(&mut discarded) {
                    assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
                }
            }
        }
    }
}
