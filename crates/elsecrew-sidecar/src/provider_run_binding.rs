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
    /// The hold file of this run (schema 2, ADE multi-project P5): each run
    /// has its own, so runs of one provider go on side by side. Schema 1
    /// receipts name the provider's one shared hold (`<provider>-login.pending`).
    lease: Option<String>,
}

/// The hold file name of one run of a provider (`codex` / `claude`): a
/// lowercase UUID without dashes keeps it unique and plain.
pub(crate) fn is_run_lease_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix("-run-"))
        .and_then(|rest| rest.strip_suffix(".pending"))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

impl RunFenceBinding {
    /// Whether the fence file found now is the one this receipt was written
    /// for. Inode numbers are the durable part of the identity. A device
    /// number is assigned when a volume is mounted and can differ after a
    /// restart of the computer, so it is only checked for consistency: the
    /// file and its directory share a device exactly when they did at bind
    /// time. A copied or recreated file gets other inode numbers and fails.
    #[cfg(target_os = "macos")]
    pub(crate) fn matches_file(
        &self,
        device: u64,
        inode: u64,
        directory_device: u64,
        directory_inode: u64,
    ) -> bool {
        self.inode == inode
            && self.directory_inode == directory_inode
            && (self.device == self.directory_device) == (device == directory_device)
    }

    pub(crate) fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The hold file this receipt names.
    pub(crate) fn lease_name(&self) -> String {
        match &self.lease {
            Some(lease) => lease.clone(),
            None if self.provider == "claude-code" => "claude-login.pending".into(),
            None => "codex-login.pending".into(),
        }
    }

    pub(crate) fn from_value(value: &Value) -> Result<Self, SidecarError> {
        const V1: [&str; 8] = ["schemaVersion", "runId", "nonce", "provider", "device", "inode", "directoryDevice", "directoryInode"];
        let leased = value.get("schemaVersion").and_then(Value::as_u64) == Some(2);
        let mut keys = V1.to_vec();
        if leased {
            keys.push("lease");
        }
        let object = crate::require_exact_params(value, &keys).map_err(|_| failure())?;
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
            lease: if leased { Some(object["lease"].as_str().ok_or_else(failure)?.into()) } else { None },
        })
    }

    pub(crate) fn to_value(&self) -> Value {
        let mut value = json!({"schemaVersion":self.schema_version,"runId":self.run_id,"nonce":self.nonce,
            "provider":self.provider,"device":self.device,"inode":self.inode,
            "directoryDevice":self.directory_device,"directoryInode":self.directory_inode});
        if let Some(lease) = &self.lease {
            value["lease"] = json!(lease);
        }
        value
    }

    pub(crate) fn validate(&self, run_id: &str, runtime: &str) -> Result<(), SidecarError> {
        let lease_ok = match (self.schema_version, &self.lease) {
            (1, None) => true,
            (2, Some(lease)) => is_run_lease_name(lease, if runtime == "claude-code" { "claude" } else { "codex" }),
            _ => false,
        };
        if !lease_ok
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

    #[cfg(target_os = "macos")]
    #[test]
    fn a_renumbered_device_still_names_the_same_file_but_other_inodes_never_do() {
        let receipt = RunFenceBinding::from_value(&json!({
            "schemaVersion": 1, "runId": "run-bound", "provider": "codex",
            "nonce": "12345678-1234-4234-8234-123456789abc",
            "device": 16777230, "inode": 2, "directoryDevice": 16777230, "directoryInode": 3,
        }))
        .unwrap();
        assert!(receipt.matches_file(16777230, 2, 16777230, 3));
        // A restart of the computer can mount the same volume under another number.
        assert!(receipt.matches_file(16777231, 2, 16777231, 3));
        // The file and its directory can never sit on different devices.
        assert!(!receipt.matches_file(16777231, 2, 16777230, 3));
        // A copied or recreated file or directory has another inode.
        assert!(!receipt.matches_file(16777230, 4, 16777230, 3));
        assert!(!receipt.matches_file(16777230, 2, 16777230, 5));
        assert!(!receipt.matches_file(16777231, 4, 16777231, 3));
    }

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
        // Schema 2 names the run's own hold file (ADE multi-project P5).
        let mut leased = value.clone();
        leased["schemaVersion"] = json!(2);
        assert!(RunFenceBinding::from_value(&leased).is_err(), "schema 2 needs its hold's name");
        leased["lease"] = json!("codex-run-0123456789abcdef0123456789abcdef.pending");
        let leased_receipt = RunFenceBinding::from_value(&leased).unwrap();
        leased_receipt.validate("run-bound", "codex").unwrap();
        assert_eq!(leased_receipt.lease_name(), "codex-run-0123456789abcdef0123456789abcdef.pending");
        assert_eq!(receipt.lease_name(), "codex-login.pending");
        for lease in ["claude-run-0123456789abcdef0123456789abcdef.pending", "codex-run-../x.pending", "codex-login.pending", "codex-run-0123.pending"] {
            leased["lease"] = json!(lease);
            assert!(RunFenceBinding::from_value(&leased).unwrap().validate("run-bound", "codex").is_err(), "{lease}");
        }
        for (field, replacement) in [
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
