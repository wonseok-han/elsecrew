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
    worker: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    view: Arc<Mutex<Value>>,
    attempt: Option<String>,
}

impl LoginManager {
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
        if params
            .as_object()
            .is_none_or(|p| p.len() != if action == "status" { 1 } else { 2 })
        {
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
            "cancel" => {
                if self.attempt.as_deref() == id {
                    self.cancel.store(true, Ordering::Release);
                    let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
                    if view.is_object() {
                        view["authUrl"] = Value::Null;
                    }
                }
            }
            "start" => {
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
                self.view = Arc::new(Mutex::new(
                    json!({"state":"starting","attemptId":id,"authUrl":null,"error":null}),
                ));
                let profile = match CodexProfileBinding::from_storage(storage) {
                    Ok(profile) => profile,
                    Err(error) => {
                        *self.view.lock().unwrap_or_else(|e| e.into_inner()) = json!({"state":"failed","attemptId":id,"authUrl":null,"error":error.kind()});
                        return Ok(self.status());
                    }
                };
                let view = self.view.clone();
                let cancel = self.cancel.clone();
                let attempt = self.attempt.clone().unwrap();
                self.worker = Some(thread::spawn(move || {
                    let publish = |state: &str, url: Option<String>, error: Option<&str>| {
                        *view.lock().unwrap_or_else(|e| e.into_inner()) =
                            json!({"state":state,"attemptId":attempt,"authUrl":url,"error":error});
                    };
                    let result = run(&profile, &cancel, &publish);
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
fn unavailable() -> SidecarError {
    SidecarError::new(
        "providerUnavailable",
        "supported Codex executable is unavailable",
    )
}
fn digest(value: &str) -> [u8; 32] {
    let mut bytes = [0; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).expect("fixed digest");
    }
    bytes
}

fn run(
    profile: &CodexProfileBinding,
    cancel: &AtomicBool,
    publish: &impl Fn(&str, Option<String>, Option<&str>),
) -> Result<(), SidecarError> {
    let mut selected = None;
    for root in [
        "/Applications/ChatGPT.app/Contents/Resources",
        "/Applications/Codex.app/Contents/Resources",
    ] {
        let executable = PinnedExecutable::open(
            &Path::new(root).join("codex"),
            digest("87a08119b8effa519f0ecb552dc98043f58a8200bf2ec5da60f76890c33e9c3a"),
        );
        // The exact supported pair was independently signature-verified before activation.
        if let Ok(executable) = executable {
            selected = Some((root, executable));
            break;
        }
    }
    let (root, executable) = selected.ok_or_else(unavailable)?;
    let helper = PinnedExecutable::open(
        &Path::new(root).join("codex-code-mode-host"),
        digest("038b9c6b60baacbfacba1fe81cf603c22b73697810e013d4efda040f1a7294e7"),
    )?;
    let home = profile.revalidate()?;
    if cancel.load(Ordering::Acquire) {
        publish("canceled", None, None);
        return Ok(());
    }
    let mut session = LoginSession::start_with_barrier(
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
            LoginProgress::Canceled => {
                publish("canceled", None, None);
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn retry_cleanup_ticks(mut cleanup: impl FnMut() -> Result<(), SidecarError>) {
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
