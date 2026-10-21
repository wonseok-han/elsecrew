//! Durable identity only; this receipt does not authorize releasing a fence.
use super::*;
use serde_json::{json, Value};

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

impl LoginProcess<'_> {
    /// Called once before launch, after run retention. The caller must commit
    /// the returned receipt before spawning. A failed write stays fenced.
    pub(crate) fn bind_run_fence(&mut self, run_id: &str) -> Result<RunFenceBinding, SidecarError> {
        if !self.matches_startup_run(run_id) || self.attempted || self.finished || self.failed {
            return Err(failure());
        }
        let result = (|| {
            self.profile.as_ref().revalidate()?;
            self.reservation.verify()?;
            let file = self.reservation.file.metadata().map_err(|_| failure())?;
            let directory = self
                .reservation
                .directory
                .dir_metadata()
                .map_err(|_| failure())?;
            // No earlier process identity or run binding may precede this line.
            if file.len() != b"hello-agent provider login pending v1\n".len() as u64 {
                return Err(failure());
            }
            let provider = match self.reservation.name {
                "codex-login.pending" => "codex",
                "claude-login.pending" => "claude-code",
                _ => return Err(failure()),
            };
            let receipt = RunFenceBinding {
                schema_version: 1,
                run_id: run_id.into(),
                nonce: self.connection_id.to_string(),
                provider: provider.into(),
                device: file.dev(),
                inode: file.ino(),
                directory_device: directory.dev(),
                directory_inode: directory.ino(),
            };
            receipt.validate(run_id, provider)?;
            let encoded = serde_json::to_string(&receipt.to_value()).map_err(|_| failure())?;
            writeln!(self.reservation.file, "run-binding-v1 {encoded}").map_err(|_| failure())?;
            self.reservation.file.sync_all().map_err(|_| failure())?;
            self.reservation.verify()?;
            Ok(receipt)
        })();
        if result.is_err() {
            self.failed = true;
            self.reservation.failed = true;
        }
        result
    }
}
