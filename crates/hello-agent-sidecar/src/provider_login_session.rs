//! Login protocol + process ownership. Start is an explicit user login action.
//! The supplied backend launcher still must prove executable/environment isolation.
use crate::{
    provider_login_process::LoginProcess, provider_profile::CodexProfileBinding, SidecarError,
};
use hello_agent_core::provider::vendor_observation::codex_login::{
    connection::{ConnectionState, LoginAction, LoginConnection},
    BrowserLogin,
};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const BROWSER_TIMEOUT: Duration = Duration::from_secs(300);

/// No Debug: Browser contains an OAuth URL. No account identifiers leave the session.
pub(crate) enum LoginProgress {
    Pending,
    Browser(BrowserLogin),
    CredentialsObserved,
    Canceled,
}

pub(crate) struct LoginSession<'a> {
    process: LoginProcess<'a>,
    flow: LoginConnection,
    deadline: Instant,
    closed: bool,
    cleanup_required: bool,
}

fn failure() -> SidecarError {
    SidecarError::new("providerLoginFailed", "login could not be verified")
}

impl<'a> LoginSession<'a> {
    pub(crate) fn start_with_barrier(
        profile: &'a CodexProfileBinding,
        spawn: impl FnOnce() -> std::io::Result<Child>,
        release: impl FnOnce(&mut Child) -> std::io::Result<()>,
    ) -> Result<Self, SidecarError> {
        let home = profile.revalidate()?;
        let (flow, request) =
            LoginConnection::begin(home.to_str().ok_or_else(failure)?).map_err(|_| failure())?;
        let mut process = LoginProcess::reserve(profile)?;
        process.launch_with_barrier(spawn, release)?;
        process.send(request, RPC_TIMEOUT, &AtomicBool::new(false))?;
        Ok(Self {
            process,
            flow,
            deadline: Instant::now() + RPC_TIMEOUT,
            closed: false,
            cleanup_required: false,
        })
    }

    #[cfg(test)]
    fn start(
        profile: &'a CodexProfileBinding,
        spawn: impl FnOnce() -> std::io::Result<Child>,
    ) -> Result<Self, SidecarError> {
        Self::start_with_barrier(profile, spawn, |_| Ok(()))
    }

    /// Owner calls at most 100ms apart, including while the browser is open.
    /// Pending means no complete frame yet, not successful authentication.
    pub(crate) fn advance(&mut self, cancel: &AtomicBool) -> Result<LoginProgress, SidecarError> {
        if self.closed {
            self.retry_cleanup()?;
            return Err(failure());
        }
        if cancel.load(Ordering::Acquire) {
            return self.cancel();
        }
        let result = self.advance_inner(cancel);
        if result.is_err() {
            self.flow.invalidate();
            if !self.closed {
                // Prefer the recovery error when cleanup is uncertain.
                self.close_process()?;
            }
        }
        result
    }

    fn advance_inner(&mut self, cancel: &AtomicBool) -> Result<LoginProgress, SidecarError> {
        if Instant::now() >= self.deadline {
            return Err(failure());
        }
        let Some(frame) = self.process.poll(cancel)? else {
            return Ok(LoginProgress::Pending);
        };
        if Instant::now() >= self.deadline {
            return Err(failure());
        }
        match self.flow.accept(&frame).map_err(|_| failure())? {
            LoginAction::Send(request) => {
                self.process.send(request, RPC_TIMEOUT, cancel)?;
                if self.flow.state() == ConnectionState::ReadyToLogin {
                    let request = self.flow.start_login().map_err(|_| failure())?;
                    self.process.send(request, RPC_TIMEOUT, cancel)?;
                }
                self.deadline = Instant::now() + RPC_TIMEOUT;
                Ok(LoginProgress::Pending)
            }
            LoginAction::Browser(browser) => {
                self.deadline = Instant::now() + BROWSER_TIMEOUT;
                Ok(LoginProgress::Browser(browser))
            }
            LoginAction::AccountUpdated => Ok(LoginProgress::Pending),
            LoginAction::CredentialsObserved => {
                self.close_process()?;
                // A concurrent cancel never publishes a successful login outcome.
                if cancel.load(Ordering::Acquire) {
                    self.flow.cancel().map_err(|_| failure())?;
                    return Ok(LoginProgress::Canceled);
                }
                Ok(LoginProgress::CredentialsObserved)
            }
        }
    }

    pub(crate) fn cancel(&mut self) -> Result<LoginProgress, SidecarError> {
        if self.closed {
            self.retry_cleanup()?;
            return Err(failure());
        }
        let request = self.flow.cancel().map_err(|_| failure());
        self.closed = true;
        if let Ok(Some(request)) = request {
            // Best effort; only independently confirmed process cleanup releases the fence.
            let _ = self
                .process
                .send(request, Duration::from_secs(1), &AtomicBool::new(false));
        }
        self.close_process()?;
        Ok(LoginProgress::Canceled)
    }

    fn close_process(&mut self) -> Result<(), SidecarError> {
        self.closed = true;
        let result = self.process.finish();
        self.cleanup_required = result.is_err();
        result
    }

    /// Also called by subsequent advance/cancel after an uncertain close. The
    /// first uncertainty stays fenced even if later OS cleanup succeeds.
    pub(crate) fn retry_cleanup(&mut self) -> Result<(), SidecarError> {
        if self.cleanup_required {
            self.close_process()
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn profile(temp: &tempfile::TempDir) -> CodexProfileBinding {
        let storage = crate::Storage::open(
            &temp
                .path()
                .canonicalize()
                .unwrap()
                .join("hello-agent.sqlite3"),
        )
        .unwrap();
        CodexProfileBinding::from_storage(&storage).unwrap()
    }

    fn peer(home: &str, account_kind: &str) -> std::io::Result<Child> {
        let init = serde_json::json!({"id":1,"result":{"userAgent":"test","codexHome":home,"platformFamily":"unix","platformOs":"macos"}}).to_string();
        let account = serde_json::json!({"id":3,"result":{"account":{"type":account_kind,"email":"PRIVATE"},"requiresOpenaiAuth":true}}).to_string();
        // Fixed toy peer: require exact client methods/IDs, never invoke a vendor.
        let script = r#"
IFS= read -r line; case "$line" in *'"method":"initialize"'*) ;; *) exit 2;; esac
printf '%s\n' "$1"
IFS= read -r line; test "$line" = '{"method":"initialized","params":{}}' || exit 3
IFS= read -r line; case "$line" in *'"method":"account/login/start"'*) ;; *) exit 4;; esac
printf '%s\n' '{"id":2,"result":{"type":"chatgpt","loginId":"attempt","authUrl":"https://auth.openai.com/authorize?state=SECRET"}}'
printf '%s\n' '{"method":"account/updated","params":{"authMode":"chatgpt","planType":"plus"}}'
printf '%s\n' '{"method":"account/login/completed","emittedAtMs":1,"params":{"loginId":"attempt","success":true,"error":null,"onboardingEntrypoint":null}}'
IFS= read -r line; test "$line" = '{"id":3,"method":"account/read","params":{"refreshToken":false}}' || exit 5
printf '%s\n' "$2"
IFS= read -r hold
exec /bin/sleep 30
"#;
        Command::new("/bin/sh")
            .args(["-c", script, "login-peer", &init, &account])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    }

    #[test]
    fn real_pipe_session_initializes_logs_in_rechecks_and_closes_before_success() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let home = binding.revalidate().unwrap();
        let mut session =
            LoginSession::start(&binding, || peer(home.to_str().unwrap(), "chatgpt")).unwrap();
        let limit = Instant::now() + Duration::from_secs(3);
        let mut browser_seen = false;
        loop {
            assert!(Instant::now() < limit);
            match session.advance(&AtomicBool::new(false)).unwrap() {
                LoginProgress::Browser(browser) => {
                    browser_seen = true;
                    assert_eq!(browser.login_id, "attempt");
                }
                LoginProgress::CredentialsObserved => break,
                LoginProgress::Pending => std::thread::sleep(Duration::from_millis(5)),
                LoginProgress::Canceled => panic!(),
            }
        }
        assert!(browser_seen);
        assert!(session.closed);
        assert!(!temp.path().join("codex-login.pending").exists());
        assert!(session.advance(&AtomicBool::new(false)).is_err());
    }

    #[test]
    fn browser_success_cannot_replace_account_verification() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let home = binding.revalidate().unwrap();
        let mut session =
            LoginSession::start(&binding, || peer(home.to_str().unwrap(), "apiKey")).unwrap();
        let limit = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(Instant::now() < limit);
            match session.advance(&AtomicBool::new(false)) {
                Err(error) => {
                    assert!(!format!("{error:?}").contains("PRIVATE"));
                    break;
                }
                Ok(LoginProgress::CredentialsObserved) => panic!("wrong account accepted"),
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        assert_eq!(session.flow.state(), ConnectionState::Failed);
    }

    #[test]
    fn idle_poll_cancel_and_deadline_remain_distinct() {
        for cancel in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let mut session = LoginSession::start(&binding, || {
                Command::new("/bin/sh")
                    .args(["-c", "read line; read hold"])
                    .env_clear()
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
            })
            .unwrap();
            assert!(matches!(
                session.advance(&AtomicBool::new(false)).unwrap(),
                LoginProgress::Pending
            ));
            if cancel {
                assert!(matches!(
                    session.advance(&AtomicBool::new(true)).unwrap(),
                    LoginProgress::Canceled
                ));
            } else {
                session.deadline = Instant::now();
                assert!(session.advance(&AtomicBool::new(false)).is_err());
            }
            assert!(session.closed);
            assert!(!temp.path().join("codex-login.pending").exists());
        }
    }

    #[test]
    fn cleanup_failure_and_browser_cancel_never_publish_credentials() {
        for cancel in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let home = binding.revalidate().unwrap();
            let mut session =
                LoginSession::start(&binding, || peer(home.to_str().unwrap(), "chatgpt")).unwrap();
            let limit = Instant::now() + Duration::from_secs(3);
            loop {
                assert!(Instant::now() < limit);
                if matches!(
                    session.advance(&AtomicBool::new(false)).unwrap(),
                    LoginProgress::Browser(_)
                ) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if cancel {
                assert!(matches!(session.cancel().unwrap(), LoginProgress::Canceled));
                assert!(session.advance(&AtomicBool::new(false)).is_err());
            } else {
                std::fs::rename(
                    temp.path().join("codex-login.pending"),
                    temp.path().join("retained-pending"),
                )
                .unwrap();
                std::fs::write(temp.path().join("codex-login.pending"), b"foreign marker").unwrap();
                loop {
                    assert!(Instant::now() < limit);
                    match session.advance(&AtomicBool::new(false)) {
                        Err(_) => break,
                        Ok(LoginProgress::CredentialsObserved) => {
                            panic!("unconfirmed cleanup accepted")
                        }
                        _ => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
                assert_eq!(session.flow.state(), ConnectionState::Failed);
                assert_eq!(
                    std::fs::read(temp.path().join("codex-login.pending")).unwrap(),
                    b"foreign marker"
                );
            }
        }
    }

    #[test]
    fn retained_failed_session_retries_cleanup_on_next_tick() {
        use hello_agent_core::proctree::{ProbeResult, ProcTable, SystemProcTable};
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let home = binding.revalidate().unwrap();
        let mut session =
            LoginSession::start(&binding, || peer(home.to_str().unwrap(), "chatgpt")).unwrap();
        let root = session.process.root_for_test();
        session.process.inject_uncertain_cleanup_once();
        let limit = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(Instant::now() < limit);
            match session.advance(&AtomicBool::new(false)) {
                Err(_) => break,
                Ok(LoginProgress::CredentialsObserved) => panic!("cleanup unconfirmed"),
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        assert!(session.closed && session.cleanup_required);
        assert_eq!(session.flow.state(), ConnectionState::Failed);
        assert!(matches!(SystemProcTable.probe(root), ProbeResult::Alive(_)));
        assert!(session.advance(&AtomicBool::new(false)).is_err());
        assert!(matches!(SystemProcTable.probe(root), ProbeResult::Gone));
        assert!(temp.path().join("codex-login.pending").exists());
        assert!(LoginProcess::reserve(&binding).is_err());
    }
}
