//! Local CONNECT admission, not an activated proxy or vendor network attestation.
//! Never forwards proxy credentials or logs request bytes. The broker validates
//! resolved addresses and caps connection resources. LoginProcess owns endpoint
//! shutdown; native vendor compatibility and product admission remain separate.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::time::{Duration, Instant};

pub(super) mod broker;
mod connect;
mod relay;

const MAX_HEADER: usize = 8 * 1024;
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);

// Deliberately no Debug/Serialize: this capability must not enter logs or IPC.
pub(super) struct ProxyCredential {
    password: String,
    authorization: String,
}

impl ProxyCredential {
    pub(super) fn new() -> Self {
        let password = uuid::Uuid::new_v4().simple().to_string();
        let authorization = format!(
            "Basic {}",
            STANDARD.encode(format!("hello-agent:{password}"))
        );
        Self {
            password,
            authorization,
        }
    }

    /// Only a trusted launcher may use this; descendants may inherit it.
    /// It is cross-run separation, not a boundary against the run's own tools.
    pub(super) fn loopback_url(&self, port: std::num::NonZeroU16) -> String {
        format!("http://hello-agent:{}@127.0.0.1:{port}", self.password)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Destination {
    ChatGpt,
    Authentication,
    OpenAiApi,
}

impl Destination {
    pub(super) fn authority(self) -> &'static str {
        match self {
            Self::ChatGpt => "chatgpt.com:443",
            Self::Authentication => "auth.openai.com:443",
            Self::OpenAiApi => "api.openai.com:443",
        }
    }

    fn parse(authority: &str) -> Option<Self> {
        [Self::ChatGpt, Self::Authentication, Self::OpenAiApi]
            .into_iter()
            .find(|destination| authority == destination.authority())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Pending,
    Accepted(Destination),
    Rejected,
}

pub(super) struct ConnectRequest {
    started: Instant,
    bytes: Vec<u8>,
    closed: bool,
}

impl ConnectRequest {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            started: now,
            bytes: Vec::new(),
            closed: false,
        }
    }

    /// Call with an empty slice on idle ticks too. No activity resets the budget.
    /// Accepted consumes the parser: tunnel bytes belong to a separate relay.
    pub(super) fn feed(
        &mut self,
        chunk: &[u8],
        now: Instant,
        credential: &ProxyCredential,
    ) -> Admission {
        if self.closed
            || now
                .checked_duration_since(self.started)
                .is_none_or(|elapsed| elapsed >= HEADER_TIMEOUT)
            || chunk.len() > MAX_HEADER.saturating_sub(self.bytes.len())
        {
            return self.reject();
        }
        self.bytes.extend_from_slice(chunk);
        let Some(end) = self.bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
            return if self.bytes.len() == MAX_HEADER {
                self.reject()
            } else {
                Admission::Pending
            };
        };
        // Do not accept a pipelined second request, body or early tunnel payload.
        // Clients must wait for the relay's 200 before beginning TLS.
        let target = (end + 4 == self.bytes.len())
            .then(|| parse(&self.bytes[..end], credential))
            .flatten();
        self.closed = true;
        self.bytes.fill(0);
        self.bytes.clear();
        target.map_or(Admission::Rejected, Admission::Accepted)
    }

    fn reject(&mut self) -> Admission {
        self.closed = true;
        self.bytes.fill(0);
        self.bytes.clear();
        Admission::Rejected
    }
}

fn parse(bytes: &[u8], credential: &ProxyCredential) -> Option<Destination> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split("\r\n");
    let request = lines.next()?;
    let authority = request
        .strip_prefix("CONNECT ")?
        .strip_suffix(" HTTP/1.1")?;
    let destination = Destination::parse(authority)?;
    let mut host = false;
    let mut authorized = false;
    let mut user_agent = false;
    let mut connection = false;
    for line in lines {
        if !line.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
            return None;
        }
        let (name, raw) = line.split_once(':')?;
        let value = raw.trim_matches(' ');
        if name.eq_ignore_ascii_case("host") && !host && value == authority {
            host = true;
        } else if name.eq_ignore_ascii_case("proxy-authorization")
            && !authorized
            && value == credential.authorization
        {
            authorized = true;
        } else if name.eq_ignore_ascii_case("user-agent") && !user_agent && !value.is_empty() {
            user_agent = true;
        } else if name.eq_ignore_ascii_case("proxy-connection")
            && !connection
            && value.eq_ignore_ascii_case("keep-alive")
        {
            connection = true;
        } else {
            return None;
        }
    }
    (host && authorized).then_some(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "local loopback/curl handshake only; never connects upstream"]
    fn local_standard_client_connect_is_admitted_only_with_run_credential() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::process::{Command, Stdio};
        let credential = ProxyCredential::new();
        let foreign = ProxyCredential::new();
        for valid in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = std::num::NonZeroU16::new(listener.local_addr().unwrap().port()).unwrap();
            let url = if valid { &credential } else { &foreign }.loopback_url(port);
            let mut client = Command::new("/usr/bin/curl")
                .env_clear()
                .args([
                    "-q",
                    "--silent",
                    "--head",
                    "--proxy",
                    &url,
                    "--noproxy",
                    "",
                    "--connect-timeout",
                    "1",
                    "--max-time",
                    "2",
                    "https://chatgpt.com/",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("local accept failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut parser = ConnectRequest::new(Instant::now());
            let admission = loop {
                let mut buffer = [0u8; 1024];
                let count = stream.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                match parser.feed(&buffer[..count], Instant::now(), &credential) {
                    Admission::Pending => continue,
                    admission => break admission,
                }
            };
            // No DNS, upstream connection or TLS is performed by this probe.
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            drop(stream);
            assert!(!client.wait().unwrap().success());
            assert_eq!(
                admission,
                if valid {
                    Admission::Accepted(Destination::ChatGpt)
                } else {
                    Admission::Rejected
                }
            );
        }
    }
    fn request(credential: &ProxyCredential, authority: &str, extra: &str) -> Vec<u8> {
        format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Authorization: {}\r\n{extra}\r\n", credential.authorization).into_bytes()
    }

    #[test]
    fn admission_is_fragment_safe_single_use_and_run_authenticated() {
        let credential = ProxyCredential::new();
        let foreign = ProxyCredential::new();
        let now = Instant::now();
        for target in [
            Destination::ChatGpt,
            Destination::Authentication,
            Destination::OpenAiApi,
        ] {
            let frame = request(
                &credential,
                target.authority(),
                "User-Agent: local-probe\r\nProxy-Connection: Keep-Alive\r\n",
            );
            for split in 0..frame.len() {
                let mut parser = ConnectRequest::new(now);
                assert_eq!(
                    parser.feed(&frame[..split], now, &credential),
                    Admission::Pending
                );
                assert_eq!(
                    parser.feed(&frame[split..], now, &credential),
                    Admission::Accepted(target)
                );
                assert_eq!(parser.feed(&frame, now, &credential), Admission::Rejected);
            }
            assert_eq!(
                ConnectRequest::new(now).feed(&frame, now, &foreign),
                Admission::Rejected
            );
        }
        assert!(credential
            .loopback_url(std::num::NonZeroU16::new(1234).unwrap())
            .ends_with("@127.0.0.1:1234"));
    }

    #[test]
    fn rejects_ambiguous_destinations_headers_and_pipelining() {
        let credential = ProxyCredential::new();
        let now = Instant::now();
        for authority in [
            "chatgpt.com",
            "chatgpt.com:80",
            "chatgpt.com:0443",
            "CHATGPT.COM:443",
            "chatgpt.com.:443",
            "chatgpt.com.evil:443",
            "chatgpt.com@127.0.0.1:443",
            "127.0.0.1:443",
            "[::1]:443",
            "chatgpt.com:443/path",
            "chatgpt.com%00:443",
        ] {
            assert_eq!(
                ConnectRequest::new(now).feed(
                    &request(&credential, authority, ""),
                    now,
                    &credential
                ),
                Admission::Rejected
            );
        }
        for extra in [
            "Host: chatgpt.com:443\r\n",
            "host: chatgpt.com:443\r\n",
            "Content-Length: 0\r\n",
            "Transfer-Encoding: chunked\r\n",
            "Authorization: secret\r\n",
            " Proxy-Authorization: folded\r\n",
            "X-Test: x\nInjected: y\r\n",
            "User-Agent:\tvalue\r\n",
        ] {
            assert_eq!(
                ConnectRequest::new(now).feed(
                    &request(&credential, "chatgpt.com:443", extra),
                    now,
                    &credential
                ),
                Admission::Rejected
            );
        }
        let frame = request(&credential, "chatgpt.com:443", "");
        let text = String::from_utf8(frame.clone()).unwrap();
        for invalid in [
            text.replace("Host: chatgpt.com:443\r\n", ""),
            text.replace("Host: chatgpt.com:443", "Host: auth.openai.com:443"),
            text.replace(
                &format!("Proxy-Authorization: {}\r\n", credential.authorization),
                "",
            ),
            text.replace("Proxy-Authorization:", "Proxy-Authorization :"),
            text.replace(
                "\r\n\r\n",
                &format!(
                    "\r\nproxy-authorization: {}\r\n\r\n",
                    credential.authorization
                ),
            ),
        ] {
            assert_eq!(
                ConnectRequest::new(now).feed(invalid.as_bytes(), now, &credential),
                Admission::Rejected
            );
        }
        for bytes in [
            String::from_utf8(frame.clone())
                .unwrap()
                .replacen("CONNECT", "GET", 1)
                .into_bytes(),
            [frame.clone(), b"extra".to_vec()].concat(),
        ] {
            assert_eq!(
                ConnectRequest::new(now).feed(&bytes, now, &credential),
                Admission::Rejected
            );
        }
    }

    #[test]
    fn limits_are_total_and_failure_cannot_be_resumed() {
        let credential = ProxyCredential::new();
        let now = Instant::now();
        let mut parser = ConnectRequest::new(now);
        assert_eq!(
            parser.feed(b"C", now + Duration::from_secs(4), &credential),
            Admission::Pending
        );
        assert_eq!(
            parser.feed(b"", now + HEADER_TIMEOUT, &credential),
            Admission::Rejected
        );
        assert_eq!(
            parser.feed(
                &request(&credential, "chatgpt.com:443", ""),
                now,
                &credential
            ),
            Admission::Rejected
        );
        assert_eq!(
            ConnectRequest::new(now).feed(&vec![b'x'; MAX_HEADER], now, &credential),
            Admission::Rejected
        );
        assert_eq!(
            ConnectRequest::new(now).feed(&vec![b'x'; MAX_HEADER + 1], now, &credential),
            Admission::Rejected
        );
    }
}
