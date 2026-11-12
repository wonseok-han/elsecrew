//! Portable persisted receipt validation; this data grants no process or fence authority.
use crate::SidecarError;
use serde_json::{json, Value};

fn failure() -> SidecarError {
    SidecarError::new(
        "providerLoginRecoveryRequired",
        "login process cleanup could not be confirmed",
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunFenceBinding {
    schema_version: u32,
    run_id: String,
    nonce: String,
    provider: String,
    device: u64,
    inode: u64,
    directory_device: u64,
    directory_inode: u64,
}

impl RunFenceBinding {
    #[cfg(target_os = "macos")]
    pub(crate) fn file_identity(&self) -> (u64, u64, u64, u64) {
        (
            self.device,
            self.inode,
            self.directory_device,
            self.directory_inode,
        )
    }

    pub(crate) fn run_id(&self) -> &str {
        &self.run_id
    }

    pub(crate) fn from_value(value: &Value) -> Result<Self, SidecarError> {
        let object = crate::require_exact_params(
            value,
            &[
                "schemaVersion",
                "runId",
                "nonce",
                "provider",
                "device",
                "inode",
                "directoryDevice",
                "directoryInode",
            ],
        )
        .map_err(|_| failure())?;
        Ok(Self {
            schema_version: u32::try_from(object["schemaVersion"].as_u64().ok_or_else(failure)?)
                .map_err(|_| failure())?,
            run_id: object["runId"].as_str().ok_or_else(failure)?.into(),
            nonce: object["nonce"].as_str().ok_or_else(failure)?.into(),
            provider: object["provider"].as_str().ok_or_else(failure)?.into(),
            device: object["device"].as_u64().ok_or_else(failure)?,
            inode: object["inode"].as_u64().ok_or_else(failure)?,
            directory_device: object["directoryDevice"].as_u64().ok_or_else(failure)?,
            directory_inode: object["directoryInode"].as_u64().ok_or_else(failure)?,
        })
    }

    pub(crate) fn to_value(&self) -> Value {
        json!({"schemaVersion":self.schema_version,"runId":self.run_id,"nonce":self.nonce,
            "provider":self.provider,"device":self.device,"inode":self.inode,
            "directoryDevice":self.directory_device,"directoryInode":self.directory_inode})
    }

    pub(crate) fn validate(&self, run_id: &str, runtime: &str) -> Result<(), SidecarError> {
        if self.schema_version != 1
            || self.run_id != run_id
            || run_id.is_empty()
            || run_id.len() > 128
            || !run_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || self.provider != runtime
            || !matches!(runtime, "codex" | "claude-code")
            || uuid::Uuid::parse_str(&self.nonce).map_or(true, |nonce| {
                nonce.is_nil() || nonce.to_string() != self.nonce
            })
            || self.inode == 0
            || self.directory_inode == 0
        {
            return Err(failure());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_receipt_validation_needs_no_native_process_owner() {
        let value = json!({
            "schemaVersion": 1, "runId": "run-bound", "provider": "codex",
            "nonce": "12345678-1234-4234-8234-123456789abc",
            "device": 1, "inode": 2, "directoryDevice": 1, "directoryInode": 3,
        });
        let receipt = RunFenceBinding::from_value(&value).unwrap();
        receipt.validate("run-bound", "codex").unwrap();
        assert_eq!(receipt.to_value(), value);
        assert!(receipt.validate("other-run", "codex").is_err());
        assert!(receipt.validate("run-bound", "claude-code").is_err());
        for (field, replacement) in [
            ("schemaVersion", json!(2)),
            ("runId", json!("")),
            ("provider", json!("unknown")),
            ("nonce", json!("00000000-0000-0000-0000-000000000000")),
            ("inode", json!(0)),
            ("directoryInode", json!(0)),
        ] {
            let mut changed = value.clone();
            changed[field] = replacement;
            assert!(
                RunFenceBinding::from_value(&changed)
                    .unwrap()
                    .validate("run-bound", "codex")
                    .is_err(),
                "{field}"
            );
        }
        for field in value.as_object().unwrap().keys() {
            let mut changed = value.clone();
            changed.as_object_mut().unwrap().remove(field);
            assert!(RunFenceBinding::from_value(&changed).is_err(), "{field}");
        }
        let mut changed = value;
        changed["authority"] = json!("release");
        assert!(RunFenceBinding::from_value(&changed).is_err());
    }
}
