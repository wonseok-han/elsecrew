//! Request/lifecycle management, not publicly activated yet.
use super::{consent::ConsentStore, journal, worker, InstallScope};
use crate::{SidecarError, Storage};
use hello_agent_core::install_lifecycle::InstallPhase;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

pub(crate) struct InstallManager {
    consent: ConsentStore,
    worker: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    view: Arc<Mutex<Value>>,
    accepted: Option<(String, String)>,
    closing: bool,
}

impl Default for InstallManager {
    fn default() -> Self {
        Self {
            consent: ConsentStore::default(),
            worker: None,
            cancel: Arc::new(AtomicBool::new(false)),
            view: Arc::new(Mutex::new(view("idle", None, None, None, None))),
            accepted: None,
            closing: false,
        }
    }
}

impl InstallManager {
    pub(crate) fn request(
        &mut self,
        storage: &Storage,
        params: &Value,
    ) -> Result<Value, SidecarError> {
        self.reap();
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let fields: &[&str] = match action {
            "status" | "dismiss" => &["action"],
            "prepare" => &["action", "provider"],
            "start" => &["action", "provider", "planId"],
            "cancel" => &["action", "operationId"],
            _ => return Err(invalid()),
        };
        if params.as_object().is_none_or(|object| {
            object.len() != fields.len() || fields.iter().any(|key| !object.contains_key(*key))
        }) {
            return Err(invalid());
        }
        if self.closing && !matches!(action, "status" | "cancel") {
            return Err(invalid());
        }
        match action {
            "status" => self.detect_pending(storage)?,
            "prepare" => {
                self.require_idle(storage)?;
                let provider = provider(params)?;
                self.accepted = None;
                let plan = match self.consent.prepare(provider) {
                    Ok(plan) => plan,
                    Err(error) => {
                        self.set(view(
                            "failed",
                            None,
                            Some(provider),
                            None,
                            Some("providerInstallConsentExpired"),
                        ));
                        return Err(error);
                    }
                };
                self.cancel = Arc::new(AtomicBool::new(false));
                self.set(view(
                    "awaitingConsent",
                    None,
                    Some(provider),
                    Some(plan),
                    None,
                ));
            }
            "dismiss" => {
                if self.worker.is_some() {
                    return Err(invalid());
                }
                self.consent.dismiss();
                if self.snapshot()["state"] == "awaitingConsent" {
                    self.set(view("idle", None, None, None, None));
                }
            }
            "start" => {
                let provider = provider(params)?;
                let id = params["planId"].as_str().ok_or_else(invalid)?;
                // Re-delivery returns the prior operation, never another spawn.
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|(old, p)| old == id && p == provider)
                {
                    return Ok(self.status());
                }
                self.require_idle(storage)?;
                if self.snapshot()["state"] != "awaitingConsent" {
                    return Err(invalid());
                }
                let authorization = match self.consent.accept(id, provider) {
                    Ok(value) => value,
                    Err(error) => {
                        self.set(view(
                            "failed",
                            None,
                            Some(provider),
                            None,
                            Some("providerInstallConsentExpired"),
                        ));
                        return Err(error);
                    }
                };
                let scope = match InstallScope::from_storage(storage) {
                    Ok(scope) => scope,
                    Err(error) => {
                        self.set(view(
                            "recoveryRequired",
                            None,
                            Some(provider),
                            None,
                            Some("providerInstallRecoveryRequired"),
                        ));
                        return Err(error);
                    }
                };
                let operation = authorization.operation().to_string();
                self.accepted = Some((id.to_owned(), provider.to_owned()));
                self.cancel = Arc::new(AtomicBool::new(false));
                self.set(view(
                    "starting",
                    Some(&operation),
                    Some(provider),
                    None,
                    None,
                ));
                let progress = self.view.clone();
                let cancel = self.cancel.clone();
                let selected = provider.to_owned();
                match thread::Builder::new()
                    .name("provider-install".into())
                    .spawn(move || {
                        let publish = |phase| {
                            let error = match phase {
                                InstallPhase::Failed => Some("providerInstallFailed"),
                                InstallPhase::RecoveryRequired => {
                                    Some("providerInstallRecoveryRequired")
                                }
                                _ => None,
                            };
                            *progress.lock().unwrap_or_else(|e| e.into_inner()) = view(
                                phase_name(phase),
                                Some(&operation),
                                Some(&selected),
                                None,
                                error,
                            );
                        };
                        if let Err(error) =
                            worker::execute_consented(scope, authorization, &cancel, &publish)
                        {
                            if error.kind() == "providerInstallConsentExpired" {
                                *progress.lock().unwrap_or_else(|e| e.into_inner()) = view(
                                    "failed",
                                    Some(&operation),
                                    Some(&selected),
                                    None,
                                    Some(error.kind()),
                                );
                            } else {
                                publish(InstallPhase::RecoveryRequired);
                            }
                        }
                    }) {
                    Ok(handle) => self.worker = Some(handle),
                    Err(_) => self.fail("failed", "providerInstallStartFailed"),
                }
            }
            "cancel" => {
                let id = params["operationId"].as_str().ok_or_else(invalid)?;
                if !valid_id(id) || self.snapshot()["operationId"].as_str() != Some(id) {
                    return Err(invalid());
                }
                if self.worker.is_some() {
                    self.cancel.store(true, Ordering::Release);
                }
            }
            _ => return Err(invalid()),
        }
        Ok(self.status())
    }

    fn require_idle(&mut self, storage: &Storage) -> Result<(), SidecarError> {
        self.detect_pending(storage)?;
        if self.worker.is_some() || self.snapshot()["state"] == "recoveryRequired" {
            return Err(invalid());
        }
        Ok(())
    }
    fn detect_pending(&mut self, storage: &Storage) -> Result<(), SidecarError> {
        if self.worker.is_none() && journal::has_active(storage).unwrap_or(true) {
            self.consent.dismiss();
            self.set(view(
                "recoveryRequired",
                None,
                None,
                None,
                Some("providerInstallRecoveryRequired"),
            ));
        }
        Ok(())
    }
    fn snapshot(&self) -> Value {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn set(&self, value: Value) {
        *self.view.lock().unwrap_or_else(|e| e.into_inner()) = value;
    }
    fn fail(&self, state: &str, error: &str) {
        let mut value = self.snapshot();
        value["state"] = state.into();
        value["error"] = error.into();
        self.set(value);
    }
    fn status(&self) -> Value {
        let mut status = self.snapshot();
        let live = self
            .worker
            .as_ref()
            .is_some_and(|handle| !handle.is_finished());
        status["cancelRequested"] = (live && self.cancel.load(Ordering::Acquire)).into();
        // Do not let callback publication win the worker-exit race in the UI.
        if live
            && matches!(
                status["state"].as_str(),
                Some("installed" | "canceled" | "failed")
            )
        {
            status["state"] = "stopping".into();
        }
        status
    }
    fn reap(&mut self) {
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished)
            && self.worker.take().unwrap().join().is_err()
        {
            self.fail("recoveryRequired", "providerInstallRecoveryRequired");
        }
    }
    /// Nonblocking shutdown poll. The caller must wire this into app.prepareQuit.
    pub(crate) fn prepare_quit(&mut self, storage: &Storage) -> bool {
        self.closing = true;
        self.consent.dismiss();
        self.cancel.store(true, Ordering::Release);
        self.reap();
        let _ = self.detect_pending(storage);
        self.worker.is_none() && self.snapshot()["state"] != "recoveryRequired"
    }
}
impl Drop for InstallManager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn view(
    state: &str,
    operation: Option<&str>,
    provider: Option<&str>,
    plan: Option<Value>,
    error: Option<&str>,
) -> Value {
    json!({"state":state,"operationId":operation,"provider":provider,"plan":plan,"error":error,"cancelRequested":false})
}
fn provider(params: &Value) -> Result<&str, SidecarError> {
    match params["provider"].as_str() {
        Some(value @ ("codex" | "claude")) => Ok(value),
        _ => Err(invalid()),
    }
}
fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| !uuid.is_nil() && uuid.to_string() == id)
}
fn invalid() -> SidecarError {
    SidecarError::new("invalidParams", "invalid installation request")
}
fn phase_name(phase: InstallPhase) -> &'static str {
    match phase {
        InstallPhase::AwaitingConsent => "awaitingConsent",
        InstallPhase::Downloading => "downloading",
        InstallPhase::Installing => "installing",
        InstallPhase::Verifying => "verifying",
        InstallPhase::Stopping => "stopping",
        InstallPhase::Installed => "installed",
        InstallPhase::Canceled => "canceled",
        InstallPhase::Failed => "failed",
        InstallPhase::RecoveryRequired => "recoveryRequired",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_extra_fields_and_quits_without_starting_a_process() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(
            &temp
                .path()
                .canonicalize()
                .unwrap()
                .join("hello-agent.sqlite3"),
        )
        .unwrap();
        let mut manager = InstallManager::default();
        assert_eq!(
            manager
                .request(&storage, &json!({"action":"status"}))
                .unwrap()["state"],
            "idle"
        );
        assert!(manager
            .request(&storage, &json!({"action":"status","path":"/tmp"}))
            .is_err());
        assert!(manager
            .request(
                &storage,
                &json!({"action":"start","provider":"claude","planId":"unknown"})
            )
            .is_err());
        assert!(manager
            .request(
                &storage,
                &json!({"action":"cancel","operationId":uuid::Uuid::new_v4().to_string()})
            )
            .is_err());
        assert!(manager.prepare_quit(&storage));
        assert!(manager
            .request(&storage, &json!({"action":"prepare","provider":"codex"}))
            .is_err());
        assert!(manager.worker.is_none());
    }
    #[test]
    fn pending_evidence_blocks_without_deletion_or_resume() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        std::fs::write(root.join(super::super::PENDING), b"interrupted").unwrap();
        let mut manager = InstallManager::default();
        assert_eq!(
            manager
                .request(&storage, &json!({"action":"status"}))
                .unwrap()["state"],
            "recoveryRequired"
        );
        assert!(!manager.prepare_quit(&storage));
        assert_eq!(
            std::fs::read(root.join(super::super::PENDING)).unwrap(),
            b"interrupted"
        );
        assert!(manager.worker.is_none());
    }
}
