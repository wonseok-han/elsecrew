//! Fixed-destination DNS/connect jobs. Blocking OS resolution never runs on the
//! sidecar loop. A stuck resolver retains its global permit (at most eight);
//! dropping a job does not falsely claim that the resolver thread was killed.
use super::Destination;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

const MAX_JOBS: usize = 8;
const MAX_ADDRESSES: usize = 16;
const DEADLINE: Duration = Duration::from_secs(10);
static JOBS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();

struct Permit(Arc<AtomicUsize>);
impl Permit {
    fn acquire(count: Arc<AtomicUsize>) -> io::Result<Self> {
        count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_JOBS).then_some(n + 1)
            })
            .map_err(|_| io::Error::other("proxy connection workers busy"))?;
        Ok(Self(count))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) struct Job {
    receiver: mpsc::Receiver<io::Result<TcpStream>>,
    canceled: Arc<AtomicBool>,
    started: Instant,
}

impl Job {
    #[cfg(test)]
    pub(super) fn local_for_test(now: Instant, address: SocketAddr) -> io::Result<Self> {
        assert!(address.ip().is_loopback());
        Self::spawn(now, Arc::new(AtomicUsize::new(0)), move |_| {
            TcpStream::connect(address)
        })
    }

    pub(super) fn start(destination: Destination, now: Instant) -> io::Result<Self> {
        Self::spawn(
            now,
            JOBS.get_or_init(|| Arc::new(AtomicUsize::new(0))).clone(),
            move |cancel| dial(destination, now, cancel),
        )
    }

    fn spawn(
        now: Instant,
        count: Arc<AtomicUsize>,
        task: impl FnOnce(&AtomicBool) -> io::Result<TcpStream> + Send + 'static,
    ) -> io::Result<Self> {
        let permit = Permit::acquire(count)?;
        let canceled = Arc::new(AtomicBool::new(false));
        let worker_cancel = canceled.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("model-proxy-connect".into())
            .spawn(move || {
                let _permit = permit;
                if worker_cancel.load(Ordering::SeqCst) {
                    return;
                }
                let result = task(&worker_cancel);
                if !worker_cancel.load(Ordering::SeqCst) {
                    let _ = sender.send(result);
                }
            })?;
        Ok(Self {
            receiver,
            canceled,
            started: now,
        })
    }

    pub(super) fn poll(&mut self, now: Instant) -> io::Result<Option<TcpStream>> {
        if self.canceled.load(Ordering::SeqCst)
            || now
                .checked_duration_since(self.started)
                .is_none_or(|elapsed| elapsed >= DEADLINE)
        {
            self.canceled.store(true, Ordering::SeqCst);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "proxy connect expired",
            ));
        }
        match self.receiver.try_recv() {
            Ok(result) => {
                self.canceled.store(true, Ordering::SeqCst);
                result.map(Some)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(io::Error::other("proxy connect failed")),
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::SeqCst);
    }
}

fn dial(destination: Destination, started: Instant, cancel: &AtomicBool) -> io::Result<TcpStream> {
    // Resolve once, validate the whole bounded result, then use only numeric
    // SocketAddr values. No second hostname lookup while connecting.
    let addresses: Vec<_> = destination
        .authority()
        .to_socket_addrs()?
        .take(MAX_ADDRESSES + 1)
        .collect();
    validate(&addresses)?;
    for address in addresses {
        let remaining = DEADLINE
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero());
        if cancel.load(Ordering::SeqCst) || remaining.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "proxy connect expired",
            ));
        }
        if let Ok(stream) =
            TcpStream::connect_timeout(&address, remaining.unwrap().min(Duration::from_secs(2)))
        {
            if cancel.load(Ordering::SeqCst) || started.elapsed() >= DEADLINE {
                break;
            }
            if stream.peer_addr()? != address {
                return Err(io::Error::other("proxy peer mismatch"));
            }
            return Ok(stream);
        }
    }
    Err(io::Error::other("proxy destination unavailable"))
}

fn validate(addresses: &[SocketAddr]) -> io::Result<()> {
    if addresses.is_empty()
        || addresses.len() > MAX_ADDRESSES
        || addresses.iter().any(|address| {
            address.port() != 443
                || !public_address(address.ip())
                || matches!(address, SocketAddr::V6(v6) if v6.scope_id() != 0 || v6.flowinfo() != 0)
        })
    {
        return Err(io::Error::other("proxy destination rejected"));
    }
    Ok(())
}

// Conservative service policy, not a general-purpose is_global implementation.
// IANA special registries checked 2026-09-29. Exclude special-purpose blocks,
// including globally reachable exceptions which are not vendor service ranges.
// https://www.iana.org/assignments/iana-ipv4-special-registry
// https://www.iana.org/assignments/iana-ipv6-special-registry
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let n = u32::from(ip);
            ![
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc01fc400, 24),
                (0xc034c100, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc0af3000, 24),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 3),
            ]
            .iter()
            .any(|&(base, bits)| n >> (32 - bits) == base >> (32 - bits))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
                && s[0] != 0x2002
                && !(s[0] == 0x2620 && s[1] == 0x004f && s[2] == 0x8000)
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reserved_mixed_and_unbounded_dns_answers_are_rejected() {
        for host in [
            "0.1.2.3",
            "10.1.2.3",
            "100.64.0.1",
            "100.127.255.255",
            "127.1.2.3",
            "169.254.1.2",
            "172.31.1.2",
            "192.0.0.9",
            "192.0.2.1",
            "192.31.196.1",
            "192.52.193.1",
            "192.88.99.2",
            "192.168.1.2",
            "192.175.48.1",
            "198.19.1.2",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:8.8.8.8",
            "64:ff9b::7f00:1",
            "2001::1",
            "2001:1ff::1",
            "2001:db8::1",
            "2002:7f00:1::",
            "2620:4f:8000::1",
            "3fff:fff::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
        ] {
            let address = SocketAddr::new(host.parse().unwrap(), 443);
            assert!(validate(&[address]).is_err(), "{host}");
            assert!(
                validate(&["8.8.8.8:443".parse().unwrap(), address]).is_err(),
                "mixed {host}"
            );
        }
        for host in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "2001:4860::1",
            "2606:4700::1",
        ] {
            assert!(validate(&[SocketAddr::new(host.parse().unwrap(), 443)]).is_ok());
        }
        assert!(validate(&[]).is_err());
        for (flow, scope) in [(1, 0), (0, 1)] {
            let address = SocketAddr::V6(std::net::SocketAddrV6::new(
                "2606:4700::1".parse().unwrap(),
                443,
                flow,
                scope,
            ));
            assert!(validate(&[address]).is_err());
        }
        assert!(validate(&["8.8.8.8:80".parse().unwrap()]).is_err());
        assert!(validate(&vec!["8.8.8.8:443".parse().unwrap(); 17]).is_err());
    }

    #[test]
    fn canceled_stuck_worker_keeps_its_permit_until_it_really_exits() {
        let count = Arc::new(AtomicUsize::new(0));
        let (release, blocked) = mpsc::channel();
        let (entered, ready) = mpsc::channel();
        let now = Instant::now();
        let mut job = Job::spawn(now, count.clone(), move |_| {
            entered.send(()).unwrap();
            blocked.recv().unwrap();
            Err(io::Error::other("toy"))
        })
        .unwrap();
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(job.poll(now + DEADLINE).is_err());
        drop(job);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let permits: Vec<_> = (1..MAX_JOBS)
            .map(|_| Permit::acquire(count.clone()).unwrap())
            .collect();
        assert!(Permit::acquire(count.clone()).is_err());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while count.load(Ordering::SeqCst) == MAX_JOBS {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        drop(permits);
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
}
