//! App-owned Codex login only; never starts a thread/turn or reads legacy credentials.
use crate::{
    provider_executable::PinnedExecutable,
    provider_login_gate,
    provider_login_session::{LoginProgress, LoginSession},
    provider_profile::CodexProfileBinding,
    SidecarError, Storage,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Default)]
pub(crate) struct LoginManager {
    claude: bool,
    worker: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    view: Arc<Mutex<Value>>,
    attempt: Option<String>,
    code: Arc<Mutex<Option<String>>>,
}

impl LoginManager {
    pub(crate) fn claude() -> Self {
        let mut manager = Self::default();
        manager.claude = true;
        manager
    }
    pub(crate) fn request(
        &mut self,
        storage: &Storage,
        params: &Value,
    ) -> Result<Value, SidecarError> {
        self.reap_finished();
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let id = params.get("attemptId").and_then(Value::as_str);
        let provider = params.get("provider");
        if provider.is_some_and(|value| {
            value.as_str() != Some(if self.claude { "claude" } else { "codex" })
        }) {
            return Err(invalid());
        }
        if params.as_object().is_none_or(|p| {
            p.len()
                != (if action == "status" {
                    1
                } else if action == "submitCode" {
                    3
                } else {
                    2
                }) + usize::from(provider.is_some())
        }) {
            return Err(invalid());
        }
        if action != "status"
            && id.is_none_or(|id| {
                id.is_empty()
                    || id.len() > 80
                    || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
        {
            return Err(invalid());
        }
        match action {
            "status" => {}
            "submitCode" => {
                let code = params
                    .get("code")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?;
                if !self.claude
                    || self.attempt.as_deref() != id
                    || !valid_code(code)
                    || self.cancel.load(Ordering::Acquire)
                    || self.worker.as_ref().is_none_or(|w| w.is_finished())
                {
                    return Err(invalid());
                }
                let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
                if view["state"] != "waiting" {
                    return Err(invalid());
                }
                *self.code.lock().unwrap_or_else(|e| e.into_inner()) = Some(code.to_owned());
                view["state"] = "verifying".into();
                view["authUrl"] = Value::Null;
            }
            "cancel" => {
                if self.attempt.as_deref() == id {
                    self.cancel.store(true, Ordering::Release);
                    self.code.lock().unwrap_or_else(|e| e.into_inner()).take();
                    let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
                    if view.is_object() {
                        view["authUrl"] = Value::Null;
                    }
                }
            }
            "start" | "check" => {
                if self.attempt.as_deref() == id {
                    return Ok(self.status());
                }
                if self.worker.as_ref().is_some_and(|w| !w.is_finished()) {
                    return Err(invalid());
                }
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
                self.attempt = id.map(str::to_owned);
                self.cancel = Arc::new(AtomicBool::new(false));
                self.code = Arc::new(Mutex::new(None));
                self.view = Arc::new(Mutex::new(
                    json!({"state":"starting","attemptId":id,"authUrl":null,"error":null}),
                ));
                let profile = match if self.claude {
                    crate::provider_claude_profile::ClaudeProfileBinding::from_storage(storage)
                        .map(Profile::Claude)
                } else {
                    CodexProfileBinding::from_storage(storage).map(Profile::Codex)
                } {
                    Ok(profile) => profile,
                    Err(error) => {
                        *self.view.lock().unwrap_or_else(|e| e.into_inner()) = json!({"state":"failed","attemptId":id,"authUrl":null,"error":error.kind()});
                        return Ok(self.status());
                    }
                };
                let view = self.view.clone();
                let cancel = self.cancel.clone();
                let code = self.code.clone();
                let attempt = self.attempt.clone().unwrap();
                let check_only = action == "check";
                self.worker = Some(thread::spawn(move || {
                    let publish = |state: &str, url: Option<String>, error: Option<&str>| {
                        let mut view = view.lock().unwrap_or_else(|e| e.into_inner());
                        if state == "waiting" && cancel.load(Ordering::Acquire) {
                            return;
                        }
                        *view =
                            json!({"state":state,"attemptId":attempt,"authUrl":url,"error":error});
                    };
                    let result = match &profile {
                        Profile::Codex(profile) => run(profile, &cancel, &publish, check_only),
                        Profile::Claude(profile) => crate::provider_claude_login::run(
                            profile, &cancel, &publish, check_only, &code,
                        ),
                    };
                    code.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Err(error) = result {
                        if cancel.load(Ordering::Acquire)
                            && error.kind() != "providerLoginRecoveryRequired"
                        {
                            publish("canceled", None, None);
                        } else {
                            publish("failed", None, Some(error.kind()));
                        }
                    }
                }));
            }
            _ => return Err(invalid()),
        }
        Ok(self.status())
    }

    fn status(&self) -> Value {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        if view.is_null() {
            return json!({"state":"idle","attemptId":null,"authUrl":null,"error":null});
        }
        let result = view.clone();
        view["authUrl"] = Value::Null; // one browser handoff; never persist or log OAuth URL
        result
    }

    pub(crate) fn prepare_quit(&mut self) -> bool {
        self.cancel.store(true, Ordering::Release);
        self.code.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.reap_finished();
        self.worker.as_ref().is_none_or(|w| w.is_finished())
            && self
                .view
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get("error")
                .and_then(Value::as_str)
                != Some("providerLoginRecoveryRequired")
    }

    fn reap_finished(&mut self) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
            && self.worker.take().unwrap().join().is_err()
        {
            *self.view.lock().unwrap_or_else(|e| e.into_inner()) = json!({"state":"failed","attemptId":self.attempt,"authUrl":null,"error":"providerLoginRecoveryRequired"});
        }
    }
}
pub(super) fn valid_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 4096
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-#.=~".contains(&b))
}
impl Drop for LoginManager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn invalid() -> SidecarError {
    SidecarError::new("invalidParams", "invalid login request")
}
#[cfg(test)]
fn unavailable() -> SidecarError {
    SidecarError::new(
        "providerUnavailable",
        "supported Codex executable is unavailable",
    )
}
enum Profile {
    Codex(CodexProfileBinding),
    Claude(crate::provider_claude_profile::ClaudeProfileBinding),
}
pub(super) fn digest(value: &str) -> [u8; 32] {
    let mut bytes = [0; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).expect("fixed digest");
    }
    bytes
}

// Exact signed CLI/host pairs; never learn trust from an installed candidate.
// 0.154.0-alpha.6.2 has identical generated login types to 0.153.4 (20 files).
const CODEX_LOGIN_PAIRS: &[(&str, &str)] = &[
    (
        "3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a",
        "4c39893666a166addd217df78f158c5975f69b598404a9f86ff0df0f3da43f6d",
    ),
    (
        "a1d2f191e70023ed7afd619bc70530f26067a085926e03bae50cf5c0f8298bcf",
        "35ce5aa1789c994e1cd2dbe134e7156fe4aebdd4d463bc5330e165c2cf925095",
    ),
    (
        "87a08119b8effa519f0ecb552dc98043f58a8200bf2ec5da60f76890c33e9c3a",
        "038b9c6b60baacbfacba1fe81cf603c22b73697810e013d4efda040f1a7294e7",
    ),
];

fn select_pair(
    roots: &[&Path],
    pairs: &[(&str, &str)],
) -> Result<(PinnedExecutable, PinnedExecutable), SidecarError> {
    let mut present = false;
    for root in roots {
        // 0.158 packages a shell wrapper in bin/. Bypass it: pin and execute
        // the signed native binary inside CodexCLI.app directly.
        for (cli_relative, host_relative) in [
            (
                "codex-cli/CodexCLI.app/Contents/MacOS/codex",
                "codex-cli/bin/codex-code-mode-host",
            ),
            ("codex", "codex-code-mode-host"),
        ] {
            let path = root.join(cli_relative);
            present |= std::fs::symlink_metadata(&path).is_ok();
            for (cli_hash, host_hash) in pairs {
                if let (Ok(cli), Ok(host)) = (
                    PinnedExecutable::open(&path, digest(cli_hash)),
                    PinnedExecutable::open(&root.join(host_relative), digest(host_hash)),
                ) {
                    return Ok((cli, host));
                }
            }
        }
    }
    Err(SidecarError::new(
        if present {
            "providerExecutableChanged"
        } else {
            "providerExecutableMissing"
        },
        "Supported provider executable pair is unavailable",
    ))
}

fn run(
    profile: &CodexProfileBinding,
    cancel: &AtomicBool,
    publish: &impl Fn(&str, Option<String>, Option<&str>),
    check_only: bool,
) -> Result<(), SidecarError> {
    let (executable, helper) = select_pair(
        &[
            Path::new("/Applications/ChatGPT.app/Contents/Resources"),
            Path::new("/Applications/Codex.app/Contents/Resources"),
        ],
        CODEX_LOGIN_PAIRS,
    )?;
    let home = profile.revalidate()?;
    if cancel.load(Ordering::Acquire) {
        publish("canceled", None, None);
        return Ok(());
    }
    let mut session = LoginSession::begin_with_barrier(
        profile,
        || {
            let path = executable
                .revalidate()
                .map_err(|_| std::io::Error::other("executable changed"))?;
            helper
                .revalidate()
                .map_err(|_| std::io::Error::other("helper changed"))?;
            let mut command = provider_login_gate::command(
                path,
                &[
                    "app-server".as_ref(),
                    "--listen".as_ref(),
                    "stdio://".as_ref(),
                ],
                &home,
            )?;
            command
                .env("HOME", &home)
                .env("CODEX_HOME", &home)
                .env("TMPDIR", home.join("tmp"))
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
            command.spawn()
        },
        |child| {
            executable
                .revalidate()
                .map_err(|_| std::io::Error::other("executable changed"))?;
            helper
                .revalidate()
                .map_err(|_| std::io::Error::other("helper changed"))?;
            if cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::other("login canceled before release"));
            }
            provider_login_gate::release(child)
        },
        cancel,
        check_only,
    )?;
    loop {
        let progress = session.advance(cancel).inspect_err(|error| {
            if error.kind() == "providerLoginRecoveryRequired" {
                // Keep the owner alive across delayed OS observations. Even a
                // later successful reap cannot erase the original evidence fence.
                retry_cleanup_ticks(|| session.retry_cleanup());
            }
        })?;
        match progress {
            LoginProgress::Pending => {}
            LoginProgress::Browser(browser) => publish("waiting", Some(browser.auth_url), None),
            LoginProgress::CredentialsObserved => {
                publish("connected", None, None);
                return Ok(());
            }
            LoginProgress::CredentialsAbsent => {
                publish("idle", None, None);
                return Ok(());
            }
            LoginProgress::Canceled => {
                publish("canceled", None, None);
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn retry_cleanup_ticks(mut cleanup: impl FnMut() -> Result<(), SidecarError>) {
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(100));
        if cleanup().is_ok() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn admission_requires_one_complete_pair_and_reports_drift_separately() {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let a = format!("{:x}", Sha256::digest(b"cli-a"));
        let b = format!("{:x}", Sha256::digest(b"host-a"));
        let c = format!("{:x}", Sha256::digest(b"cli-b"));
        let d = format!("{:x}", Sha256::digest(b"host-b"));
        let pairs = [(a.as_str(), b.as_str()), (c.as_str(), d.as_str())];
        assert_eq!(
            select_pair(&[&root], &pairs).err().unwrap().kind(),
            "providerExecutableMissing"
        );
        for (name, bytes) in [
            ("codex", b"cli-a".as_slice()),
            ("codex-code-mode-host", b"host-b".as_slice()),
        ] {
            std::fs::write(root.join(name), bytes).unwrap();
            std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        assert_eq!(
            select_pair(&[&root], &pairs).err().unwrap().kind(),
            "providerExecutableChanged"
        );
        std::fs::write(root.join("codex-code-mode-host"), b"host-a").unwrap();
        assert!(select_pair(&[&root], &pairs).is_ok());
        std::fs::write(root.join("codex"), b"cli-b").unwrap();
        std::fs::write(root.join("codex-code-mode-host"), b"host-b").unwrap();
        assert!(select_pair(&[&root], &pairs).is_ok());
        let native = root.join("codex-cli/CodexCLI.app/Contents/MacOS/codex");
        let host = root.join("codex-cli/bin/codex-code-mode-host");
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::create_dir_all(host.parent().unwrap()).unwrap();
        std::fs::rename(root.join("codex"), &native).unwrap();
        std::fs::rename(root.join("codex-code-mode-host"), &host).unwrap();
        std::fs::write(root.join("codex-cli/bin/codex"), b"untrusted shell wrapper").unwrap();
        let (cli, _) = select_pair(&[&root], &pairs).unwrap();
        assert_eq!(cli.revalidate().unwrap(), native);
        std::fs::write(&host, b"host-a").unwrap();
        assert!(select_pair(&[&root], &pairs).is_err());
    }

    #[test]
    fn code_submission_is_private_single_use_and_bound_to_live_claude_attempt() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let mut manager = LoginManager::claude();
        manager.attempt = Some("current".into());
        *manager.view.lock().unwrap() =
            json!({"state":"waiting","attemptId":"current","authUrl":null,"error":null});
        let cancel = manager.cancel.clone();
        manager.worker = Some(thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(5));
            }
        }));
        let mut request = json!({"action":"submitCode","provider":"claude","attemptId":"old","code":"synthetic#state"});
        assert!(manager.request(&storage, &request).is_err());
        request["attemptId"] = "current".into();
        let response = manager.request(&storage, &request).unwrap();
        assert_eq!(response["state"], "verifying");
        assert!(!response.to_string().contains("synthetic"));
        assert!(manager.request(&storage, &request).is_err());
        manager
            .request(&storage, &json!({"action":"cancel","attemptId":"current"}))
            .unwrap();
        assert!(manager.code.lock().unwrap().is_none());
        assert!(manager.request(&storage, &request).is_err());
        assert!(LoginManager::default().request(&storage, &request).is_err());
        for invalid in ["", "x\ny", "code secret", "https://host/code", "a\u{0}"] {
            assert!(!valid_code(invalid));
        }
    }

    #[test]
    fn cleanup_owner_is_retained_through_delayed_observations() {
        let mut probes = 0;
        retry_cleanup_ticks(|| {
            probes += 1;
            if probes < 3 {
                Err(unavailable())
            } else {
                Ok(())
            }
        });
        assert_eq!(probes, 3);
    }

    #[test]
    fn panicked_worker_never_authorizes_clean_quit() {
        let mut manager = LoginManager::default();
        manager.worker = Some(thread::spawn(|| panic!("injected worker failure")));
        while !manager.worker.as_ref().unwrap().is_finished() {
            thread::yield_now();
        }
        assert!(!manager.prepare_quit());
        assert_eq!(manager.status()["error"], "providerLoginRecoveryRequired");
    }

    #[test]
    fn cancel_never_hands_browser_url_out_and_unknown_cleanup_blocks_quit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let mut manager = LoginManager::default();
        manager.attempt = Some("test".into());
        *manager.view.lock().unwrap() = json!({"state":"waiting","attemptId":"test","authUrl":"https://auth.openai.com/authorize?state=private","error":null});
        let response = manager
            .request(&storage, &json!({"action":"cancel","attemptId":"test"}))
            .unwrap();
        assert!(response["authUrl"].is_null());
        assert!(manager.cancel.load(Ordering::Acquire));
        *manager.view.lock().unwrap() = json!({"state":"failed","attemptId":"test","authUrl":null,"error":"providerLoginRecoveryRequired"});
        assert!(!manager.prepare_quit());
    }

    #[test]
    fn invalid_requests_and_private_profile_conflicts_do_not_start_worker() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let mut manager = LoginManager::default();
        assert_eq!(
            manager
                .request(&storage, &json!({"action":"status"}))
                .unwrap()["state"],
            "idle"
        );
        assert!(manager
            .request(&storage, &json!({"action":"start","attemptId":"bad/path"}))
            .is_err());
        std::fs::create_dir(root.join("codex-profile-v1")).unwrap();
        std::fs::write(root.join("codex-profile-v1/foreign"), b"preserve").unwrap();
        assert_eq!(
            manager
                .request(&storage, &json!({"action":"start","attemptId":"test"}))
                .unwrap()["state"],
            "failed"
        );
        assert!(manager.worker.is_none());
        assert_eq!(
            std::fs::read(root.join("codex-profile-v1/foreign")).unwrap(),
            b"preserve"
        );
    }

    /// New temporary app-owned profile only. Opens no browser and performs no
    /// model call; requests an OAuth URL and then immediately cancels the login.
    #[test]
    #[ignore = "explicit model-free installed providers check smoke"]
    fn installed_providers_check_empty_profiles_without_login() {
        for mut manager in [LoginManager::default(), LoginManager::claude()] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            manager
                .request(&storage, &json!({"action":"check","attemptId":"check"}))
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(40);
            loop {
                let status = manager
                    .request(&storage, &json!({"action":"status"}))
                    .unwrap();
                assert!(status["authUrl"].is_null());
                assert_ne!(status["state"], "waiting");
                assert_ne!(status["state"], "failed", "safe error: {}", status["error"]);
                if status["state"] == "idle" {
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(50));
            }
            assert!(manager.prepare_quit());
            assert!(!root
                .join(if manager.claude {
                    "claude-login.pending"
                } else {
                    "codex-login.pending"
                })
                .exists());
        }
    }

    #[test]
    #[ignore = "explicit model-free installed Claude login URL cancellation smoke"]
    fn installed_claude_login_url_then_cancel() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let mut manager = LoginManager::claude();
        manager
            .request(&storage, &json!({"action":"start","attemptId":"smoke"}))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut observed = false;
        loop {
            let status = manager
                .request(&storage, &json!({"action":"status"}))
                .unwrap();
            if status["state"] == "waiting" {
                observed = status["authUrl"].as_str().is_some_and(|s| {
                    s.starts_with("https://claude.ai/oauth/authorize?")
                        || s.starts_with("https://claude.com/cai/oauth/authorize?")
                });
                break;
            }
            if status["state"] == "failed" || Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        manager
            .request(&storage, &json!({"action":"cancel","attemptId":"smoke"}))
            .unwrap();
        let cleanup_deadline = Instant::now() + Duration::from_secs(10);
        while !manager.prepare_quit() && Instant::now() < cleanup_deadline {
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            manager.prepare_quit(),
            "safe error: {}",
            manager.status()["error"]
        );
        assert!(
            observed,
            "OAuth URL not observed; safe error: {}",
            manager.status()["error"]
        );
        assert!(!root.join("claude-login.pending").exists());
    }

    #[test]
    #[ignore = "explicit model-free installed Codex login smoke"]
    fn installed_codex_login_url_then_cancel_without_browser() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let mut manager = LoginManager::default();
        manager
            .request(&storage, &json!({"action":"start","attemptId":"smoke"}))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let status = manager
                .request(&storage, &json!({"action":"status"}))
                .unwrap();
            assert!(Instant::now() < deadline, "login preparation deadline");
            assert_ne!(
                status["state"], "failed",
                "safe error code: {}",
                status["error"]
            );
            if status["state"] == "waiting" {
                assert!(status["authUrl"]
                    .as_str()
                    .is_some_and(|url| url.starts_with("https://")));
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        manager
            .request(&storage, &json!({"action":"cancel","attemptId":"smoke"}))
            .unwrap();
        while !manager.prepare_quit() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(manager.status()["state"], "canceled");
        assert!(!root.join("codex-login.pending").exists());
    }
}
