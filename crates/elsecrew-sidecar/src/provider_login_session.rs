//! Login protocol + process ownership. Start is an explicit user login action.
//! The supplied backend launcher still must prove executable/environment isolation.
use crate::{
    provider_login_process::LoginProcess, provider_profile::CodexProfileBinding, SidecarError,
};
use elsecrew_core::provider::vendor_observation::codex_login::{
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
    CredentialsAbsent,
    Canceled,
}

pub(crate) struct LoginSession<'a> {
    process: LoginProcess<'a>,
    flow: LoginConnection,
    deadline: Instant,
    closed: bool,
    cleanup_required: bool,
    check_only: bool,
}

fn failure() -> SidecarError {
    SidecarError::new("providerLoginFailed", "login could not be verified")
}

impl<'a> LoginSession<'a> {
    #[cfg(test)]
    pub(crate) fn start_with_barrier(
        profile: &'a CodexProfileBinding,
        spawn: impl FnOnce() -> std::io::Result<Child>,
        release: impl FnOnce(&mut Child) -> std::io::Result<()>,
        cancel: &AtomicBool,
    ) -> Result<Self, SidecarError> {
        Self::begin_with_barrier(profile, spawn, release, cancel, false)
    }

    pub(crate) fn begin_with_barrier(
        profile: &'a CodexProfileBinding,
        spawn: impl FnOnce() -> std::io::Result<Child>,
        release: impl FnOnce(&mut Child) -> std::io::Result<()>,
        cancel: &AtomicBool,
        check_only: bool,
    ) -> Result<Self, SidecarError> {
        let home = profile.revalidate()?;
        let (mut flow, request) =
            LoginConnection::begin(home.to_str().ok_or_else(failure)?).map_err(|_| failure())?;
        flow.require_startup_isolation().map_err(|_| failure())?;
        let mut session = Self {
            process: LoginProcess::reserve(profile)?,
            flow,
            deadline: Instant::now() + RPC_TIMEOUT,
            closed: false,
            cleanup_required: false,
            check_only,
        };
        let preparation = session
            .process
            .launch_with_barrier(spawn, release)
            .and_then(|()| session.process.send(request, RPC_TIMEOUT, cancel));
        if let Err(error) = preparation {
            session.flow.invalidate();
            session.cleanup_failed_preparation()?;
            return Err(error);
        }
        Ok(session)
    }

    #[cfg(test)]
    fn start(
        profile: &'a CodexProfileBinding,
        spawn: impl FnOnce() -> std::io::Result<Child>,
    ) -> Result<Self, SidecarError> {
        Self::start_with_barrier(profile, spawn, |_| Ok(()), &AtomicBool::new(false))
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
                    let request = if self.check_only {
                        self.flow.check_account()
                    } else {
                        self.flow.start_login()
                    }
                    .map_err(|_| failure())?;
                    self.process.send(request, RPC_TIMEOUT, cancel)?;
                }
                self.deadline = Instant::now() + RPC_TIMEOUT;
                Ok(LoginProgress::Pending)
            }
            LoginAction::Browser(browser) => {
                self.deadline = Instant::now() + BROWSER_TIMEOUT;
                Ok(LoginProgress::Browser(browser))
            }
            LoginAction::AccountUpdated | LoginAction::ConfigurationPending => Ok(LoginProgress::Pending),
            LoginAction::CredentialsAbsent => {
                self.close_process()?;
                Ok(if cancel.load(Ordering::Acquire) {
                    LoginProgress::Canceled
                } else {
                    LoginProgress::CredentialsAbsent
                })
            }
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

    fn cleanup_failed_preparation(&mut self) -> Result<(), SidecarError> {
        if let Err(error) = self.close_process() {
            // Retain the child through delayed OS observations, but preserve
            // the original uncertainty even after a later successful reap.
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(100));
                if self.retry_cleanup().is_ok() {
                    break;
                }
            }
            return Err(error);
        }
        Ok(())
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
                .join("elsecrew.sqlite3"),
        )
        .unwrap();
        CodexProfileBinding::from_storage(&storage).unwrap()
    }

    fn configuration(home: &str) -> String {
        use elsecrew_core::provider::vendor_observation::codex_profile_config::ProfileConfig;
        let projects = ProfileConfig::parse(b"cli_auth_credentials_store='file'\n").unwrap()
            .isolated_projects(&[std::path::Path::new(home)]).unwrap();
        let flags = serde_json::json!({"projects":projects,"features":{"hooks":false,"apps":false,"plugins":false}});
        serde_json::json!({"id":60,"result":{"config":flags,"layers":[
            {"name":{"type":"sessionFlags"},"config":flags},
            {"name":{"type":"user","file":format!("{home}/config.toml")},"config":{"cli_auth_credentials_store":"file"}},
            {"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}
        ]}}).to_string()
    }

    fn peer(home: &str, account_kind: &str) -> std::io::Result<Child> {
        let init = serde_json::json!({"id":1,"result":{"userAgent":"test","codexHome":home,"platformFamily":"unix","platformOs":"macos"}}).to_string();
        let account = serde_json::json!({"id":3,"result":{"account":{"type":account_kind,"email":"PRIVATE"},"requiresOpenaiAuth":true}}).to_string();
        // Fixed toy peer: require exact client methods/IDs, never invoke a vendor.
        let script = r#"
IFS= read -r line; case "$line" in *'"method":"initialize"'*) ;; *) exit 2;; esac
printf '%s\n' "$1"
IFS= read -r line; test "$line" = '{"method":"initialized","params":{}}' || exit 3
IFS= read -r line; case "$line" in *'"method":"config/read"'*) ;; *) exit 6;; esac
printf '%s\n' "$3"
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
            .args(["-c", script, "login-peer", &init, &account, &configuration(home)])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    }

    #[test]
    fn restoration_queries_account_without_browser_and_releases_fence_before_result() {
        for present in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let binding = profile(&temp);
            let home = binding.revalidate().unwrap();
            let init = serde_json::json!({"id":1,"result":{"userAgent":"test","codexHome":home,"platformFamily":"unix","platformOs":"macos"}}).to_string();
            let account = if present {
                serde_json::json!({"type":"chatgpt","email":"synthetic"})
            } else {
                serde_json::Value::Null
            };
            let reply =
                serde_json::json!({"id":3,"result":{"account":account,"requiresOpenaiAuth":true}})
                    .to_string();
            let script = "IFS= read -r line; printf '%s\\n' \"$1\"; IFS= read -r line; IFS= read -r line; case \"$line\" in *'\"method\":\"config/read\"'*) ;; *) exit 6;; esac; printf '%s\\n' \"$3\"; IFS= read -r line; test \"$line\" = '{\"id\":3,\"method\":\"account/read\",\"params\":{\"refreshToken\":false}}' || exit 2; printf '%s\\n' \"$2\"; IFS= read -r hold; exec /bin/sleep 30";
            let mut session = LoginSession::begin_with_barrier(
                &binding,
                || {
                    Command::new("/bin/sh")
                        .args(["-c", script, "peer", &init, &reply, &configuration(home.to_str().unwrap())])
                        .env_clear()
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                },
                |_| Ok(()),
                &AtomicBool::new(false),
                true,
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                assert!(Instant::now() < deadline);
                match session.advance(&AtomicBool::new(false)).unwrap() {
                    LoginProgress::Pending => std::thread::sleep(Duration::from_millis(10)),
                    LoginProgress::CredentialsObserved => {
                        assert!(present);
                        break;
                    }
                    LoginProgress::CredentialsAbsent => {
                        assert!(!present);
                        break;
                    }
                    _ => panic!("restoration must not start browser login"),
                }
            }
            assert!(!temp.path().join("codex-login.pending").exists());
        }
    }

    #[test]
    fn cancel_during_initialization_explicitly_reaps_and_allows_new_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let cancel = AtomicBool::new(false);
        let result = LoginSession::start_with_barrier(
            &binding,
            || {
                Command::new("/bin/cat")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
            },
            |_| {
                cancel.store(true, Ordering::Release);
                Ok(())
            },
            &cancel,
        );
        assert!(result.is_err());
        assert!(!temp.path().join("codex-login.pending").exists());
        LoginProcess::reserve(&binding).unwrap().finish().unwrap();
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
                LoginProgress::Canceled | LoginProgress::CredentialsAbsent => panic!(),
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
    fn failed_preparation_cleanup_retains_child_until_reaped_without_clearing_fence() {
        use elsecrew_core::proctree::{ProbeResult, ProcTable, SystemProcTable};
        let temp = tempfile::tempdir().unwrap();
        let binding = profile(&temp);
        let home = binding.revalidate().unwrap();
        let (flow, _) = LoginConnection::begin(home.to_str().unwrap()).unwrap();
        let mut session = LoginSession {
            process: LoginProcess::reserve(&binding).unwrap(),
            flow,
            deadline: Instant::now() + RPC_TIMEOUT,
            closed: false,
            cleanup_required: false,
            check_only: false,
        };
        session
            .process
            .launch_with_barrier(
                || {
                    Command::new("/bin/cat")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                },
                |_| Ok(()),
            )
            .unwrap();
        let root = session.process.root_for_test();
        session.process.inject_uncertain_cleanup_once();
        assert!(session.cleanup_failed_preparation().is_err());
        assert!(matches!(SystemProcTable.probe(root), ProbeResult::Gone));
        assert!(temp.path().join("codex-login.pending").exists());
    }

    #[test]
    fn retained_failed_session_retries_cleanup_on_next_tick() {
        use elsecrew_core::proctree::{ProbeResult, ProcTable, SystemProcTable};
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
