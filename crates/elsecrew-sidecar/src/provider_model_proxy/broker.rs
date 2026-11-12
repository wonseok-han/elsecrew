//! Backend-only broker candidate. Product launch/lease binding remains separate.
use super::{connect::Job, relay::Relay, Admission, ConnectRequest, Destination, ProxyCredential};
use std::io::{self, Read};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const MAX_CONNECTIONS: usize = 8;
const MAX_LIFETIME: Duration = Duration::from_secs(600);
const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

enum Entry {
    Header(TcpStream, ConnectRequest),
    Connecting(TcpStream, Job),
    Relaying(Box<Relay>),
}

pub(crate) struct Broker {
    listener: Option<TcpListener>,
    credential: ProxyCredential,
    entries: Vec<Entry>,
    started: Instant,
    remaining: u64,
    stopped: bool,
}

impl Broker {
    pub(crate) fn new(listener: TcpListener, now: Instant) -> io::Result<Self> {
        if !matches!(listener.local_addr()?, SocketAddr::V4(addr) if addr.ip() == &Ipv4Addr::LOCALHOST && addr.port() != 0)
        {
            return Err(io::Error::other(
                "proxy requires exact IPv4 loopback listener",
            ));
        }
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener: Some(listener),
            credential: ProxyCredential::new(),
            entries: Vec::new(),
            started: now,
            remaining: MAX_TOTAL_BYTES,
            stopped: false,
        })
    }

    pub(crate) fn listener(&self) -> &TcpListener {
        self.listener
            .as_ref()
            .expect("listener owned until consuming handoff")
    }

    pub(crate) fn proxy_url(&self) -> io::Result<String> {
        Ok(self.credential.loopback_url(
            std::num::NonZeroU16::new(self.listener().local_addr()?.port())
                .ok_or_else(|| io::Error::other("proxy port missing"))?,
        ))
    }

    /// Trusted owner must tick while a process is active and stop at termination.
    /// Each tick does bounded accepts/reads; DNS and connect run in capped jobs.
    pub(crate) fn tick(&mut self, now: Instant, canceled: bool) -> io::Result<()> {
        self.tick_with(now, canceled, Job::start)
    }

    fn tick_with(
        &mut self,
        now: Instant,
        canceled: bool,
        mut start: impl FnMut(Destination, Instant) -> io::Result<Job>,
    ) -> io::Result<()> {
        if self.stopped
            || canceled
            || self.remaining == 0
            || now
                .checked_duration_since(self.started)
                .is_none_or(|elapsed| elapsed >= MAX_LIFETIME)
        {
            self.stop();
            return Err(io::Error::other("proxy stopped"));
        }
        for _ in 0..4 {
            match self.listener().accept() {
                Ok((stream, peer)) => {
                    if self.entries.len() < MAX_CONNECTIONS
                        && peer.ip() == Ipv4Addr::LOCALHOST
                        && stream.set_nonblocking(true).is_ok()
                    {
                        self.entries
                            .push(Entry::Header(stream, ConnectRequest::new(now)));
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    break
                }
                Err(error) => {
                    self.stop();
                    return Err(error);
                }
            }
        }
        let entries = std::mem::take(&mut self.entries);
        for entry in entries {
            if let Ok(Some(entry)) = advance(
                entry,
                now,
                &self.credential,
                &mut self.remaining,
                &mut start,
            ) {
                self.entries.push(entry);
            }
            // Rejected/failed sockets and jobs drop here; never retain a failed
            // parser or resurrect it on another request. Errors contain no payload.
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        self.entries.clear();
        self.stopped = true;
        // Keep the bound listener until owner drop so a stopped service's port
        // cannot be rebound while a process cleanup lease still owns this broker.
    }

    /// Quarantine only the bound descriptor, not relay buffers/credentials/jobs.
    /// The consuming owner must retain it when process termination is uncertain.
    pub(crate) fn into_stopped_listener(mut self) -> TcpListener {
        self.stop();
        self.listener
            .take()
            .expect("listener owned until consuming handoff")
    }

    #[cfg(test)]
    pub(crate) fn connection_count(&self) -> usize {
        self.entries.len()
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn advance(
    entry: Entry,
    now: Instant,
    credential: &ProxyCredential,
    remaining: &mut u64,
    start: &mut impl FnMut(Destination, Instant) -> io::Result<Job>,
) -> io::Result<Option<Entry>> {
    Ok(match entry {
        Entry::Header(mut stream, mut parser) => {
            let mut bytes = [0; 1024];
            let count = match stream.read(&mut bytes) {
                Ok(0) => return Ok(None),
                Ok(count) => count,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    0
                }
                Err(error) => return Err(error),
            };
            match parser.feed(&bytes[..count], now, credential) {
                Admission::Pending => Some(Entry::Header(stream, parser)),
                Admission::Rejected => None,
                Admission::Accepted(destination) => {
                    Some(Entry::Connecting(stream, start(destination, now)?))
                }
            }
        }
        Entry::Connecting(stream, mut job) => match job.poll(now)? {
            Some(upstream) => Some(Entry::Relaying(Box::new(Relay::new(
                stream, upstream, now,
            )?))),
            None => Some(Entry::Connecting(stream, job)),
        },
        Entry::Relaying(mut relay) => {
            if relay.advance_with_budget(now, false, remaining)? {
                None
            } else {
                Some(Entry::Relaying(relay))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    #[ignore = "local broker sockets only; no DNS/provider/account"]
    fn admission_capacity_timeout_and_stop_do_not_dial() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let now = Instant::now();
        let mut broker = Broker::new(listener, now).unwrap();
        let mut peers = Vec::new();
        for _ in 0..MAX_CONNECTIONS + 1 {
            peers.push(TcpStream::connect(address).unwrap());
            broker
                .tick_with(now, false, |_, _| {
                    panic!("partial/empty request cannot dial")
                })
                .unwrap();
        }
        assert_eq!(broker.entries.len(), MAX_CONNECTIONS);
        broker
            .tick_with(now + Duration::from_secs(5), false, |_, _| {
                panic!("expired request cannot dial")
            })
            .unwrap();
        assert!(broker.entries.is_empty());
        let mut wrong = TcpStream::connect(address).unwrap();
        wrong.write_all(b"CONNECT chatgpt.com:443 HTTP/1.1\r\nHost: chatgpt.com:443\r\nProxy-Authorization: Basic wrong\r\n\r\n").unwrap();
        broker
            .tick_with(now, false, |_, _| panic!("wrong auth cannot dial"))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !broker.entries.is_empty() {
            assert!(Instant::now() < deadline);
            broker
                .tick_with(now, false, |_, _| panic!("wrong auth cannot dial"))
                .unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(broker.entries.is_empty());
        assert!(broker.tick(now, true).is_err());
        assert!(broker.tick(now, false).is_err());
        assert!(TcpListener::bind(address).is_err());
        drop(broker);
        assert!(TcpListener::bind(address).is_ok());
    }

    #[test]
    #[ignore = "local authenticated broker relay only; no external DNS/provider"]
    fn authenticated_request_reaches_only_its_injected_local_peer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        upstream.set_nonblocking(true).unwrap();
        let mut broker = Broker::new(listener, Instant::now()).unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        client.set_nonblocking(true).unwrap();
        let header = format!("CONNECT chatgpt.com:443 HTTP/1.1\r\nHost: chatgpt.com:443\r\nProxy-Authorization: {}\r\n\r\n", broker.credential.authorization);
        client.write_all(header.as_bytes()).unwrap();
        let mut peer = None;
        let mut response = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !response.ends_with(b"\r\n\r\n") {
            broker
                .tick_with(Instant::now(), false, |target, now| {
                    assert_eq!(target, Destination::ChatGpt);
                    Job::local_for_test(now, upstream.local_addr().unwrap())
                })
                .unwrap();
            if peer.is_none() {
                if let Ok((stream, _)) = upstream.accept() {
                    peer = Some(stream);
                }
            }
            let mut bytes = [0; 128];
            match client.read(&mut bytes) {
                Ok(count) => {
                    assert_ne!(count, 0);
                    response.extend_from_slice(&bytes[..count]);
                }
                Err(error) => assert_eq!(error.kind(), io::ErrorKind::WouldBlock),
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(response, b"HTTP/1.1 200 Connection Established\r\n\r\n");
        let mut peer = peer.unwrap();
        peer.set_nonblocking(true).unwrap();
        client.write_all(b"only payload, no credential").unwrap();
        let mut received = Vec::new();
        while received.len() < 27 {
            broker.tick(Instant::now(), false).unwrap();
            let mut bytes = [0; 64];
            match peer.read(&mut bytes) {
                Ok(count) => {
                    assert_ne!(count, 0);
                    received.extend_from_slice(&bytes[..count]);
                }
                Err(error) => assert_eq!(error.kind(), io::ErrorKind::WouldBlock),
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(received, b"only payload, no credential");
        assert_eq!(broker.remaining, MAX_TOTAL_BYTES - received.len() as u64);
        broker.stop();
        assert!(broker.entries.is_empty());
    }
}
