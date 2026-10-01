//! Stop-only ingress hint. Normal dispatch still validates and acknowledges requests.
use crate::{decode_jsonl, EnvelopeV1};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Default)]
pub(crate) struct ApplicationCancel(Arc<Mutex<Option<Slot>>>);

struct Slot {
    project: String,
    application: String,
    flag: Arc<AtomicBool>,
}

impl ApplicationCancel {
    pub(crate) fn begin(&self, project: &str, application: &str) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Slot {
            project: project.into(),
            application: application.into(),
            flag: Arc::new(AtomicBool::new(false)),
        });
    }

    pub(crate) fn flag(&self) -> Option<Arc<AtomicBool>> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.flag.clone())
    }

    pub(crate) fn stop(&self) {
        if let Some(flag) = self.flag() {
            flag.store(true, Ordering::Release);
        }
    }

    pub(crate) fn canceled(&self, project: &str, application: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|s| {
                s.project == project
                    && s.application == application
                    && s.flag.load(Ordering::Acquire)
            })
    }

    pub(crate) fn observe(&self, line: &[u8], instance: &str) {
        let Ok(EnvelopeV1::Request(request)) = decode_jsonl(line) else {
            return;
        };
        if request.sidecar_instance_id != instance {
            return;
        }
        let Some(params) = request.params.as_object() else {
            return;
        };
        if request.method == "app.prepareQuit" && params.is_empty() {
            self.stop();
        } else if request.method == "project.catalog"
            && params.len() == 3
            && params.get("action").and_then(|v| v.as_str()) == Some("cancelApplication")
        {
            let slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(slot) = slot.as_ref() {
                if params.get("projectId").and_then(|v| v.as_str()) == Some(slot.project.as_str())
                    && params.get("applicationId").and_then(|v| v.as_str())
                        == Some(slot.application.as_str())
                {
                    slot.flag.store(true, Ordering::Release);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ingress_requires_exact_instance_target_and_fields() {
        let cancel = ApplicationCancel::default();
        cancel.begin("project", "application");
        let valid = serde_json::json!({
            "protocolVersion": crate::PROTOCOL_VERSION,
            "kind":"request", "sidecarInstanceId":"instance", "requestId":"cancel-request",
            "method":"project.catalog", "params": {
                "action":"cancelApplication", "projectId":"project", "applicationId":"application"
            }
        });
        let mut wrong_instance = valid.clone();
        wrong_instance["sidecarInstanceId"] = serde_json::json!("other");
        let mut wrong_project = valid.clone();
        wrong_project["params"]["projectId"] = serde_json::json!("other");
        let mut wrong_application = valid.clone();
        wrong_application["params"]["applicationId"] = serde_json::json!("other");
        let mut extra = valid.clone();
        extra["params"]["extra"] = serde_json::json!(true);
        for rejected in [wrong_instance, wrong_project, wrong_application, extra] {
            cancel.observe(&serde_json::to_vec(&rejected).unwrap(), "instance");
            assert!(!cancel.canceled("project", "application"));
        }
        cancel.observe(&serde_json::to_vec(&valid).unwrap(), "instance");
        assert!(cancel.canceled("project", "application"));
    }

    #[test]
    fn old_flag_cannot_cancel_new_application() {
        let cancel = ApplicationCancel::default();
        cancel.begin("project", "first");
        let old = cancel.flag().unwrap();
        cancel.begin("project", "second");
        old.store(true, Ordering::Release);
        assert!(!cancel.canceled("project", "second"));
        cancel.stop();
        assert!(cancel.canceled("project", "second"));
        assert!(!cancel.canceled("project", "first"));
    }
}
