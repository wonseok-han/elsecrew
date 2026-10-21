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

/// Explicit recovery only. The verifier must prove the exact durable run is
/// terminal before unlink; disappearance of processes alone is insufficient.
pub(crate) fn recover_bound_run(
    profile: &dyn LoginProfile,
    verify_terminal: impl FnOnce(&RunFenceBinding, &BTreeSet<ProcIdentity>) -> Result<(), SidecarError>,
) -> Result<(), SidecarError> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
    use std::io::Read;
    let home = profile.revalidate()?;
    let directory = profile.login_lease_directory()?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let mut file = directory
        .open_with(profile.pending_name(), &options)
        .map_err(|_| failure())?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_file() || meta.len() > 16 * 1024 {
        return Err(failure());
    }
    lock_exclusive(&file)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 != meta.len() || !bytes.ends_with(b"\n") || bytes.contains(&b'\r') {
        return Err(failure());
    }
    let mut lines = std::str::from_utf8(&bytes).map_err(|_| failure())?.lines();
    if lines.next() != Some("hello-agent provider login pending v1") {
        return Err(failure());
    }
    let encoded = lines
        .next()
        .and_then(|line| line.strip_prefix("run-binding-v1 "))
        .ok_or_else(failure)?;
    let value: Value = serde_json::from_str(encoded).map_err(|_| failure())?;
    let binding = RunFenceBinding::from_value(&value)?;
    if serde_json::to_string(&binding.to_value()).map_err(|_| failure())? != encoded {
        return Err(failure());
    }
    let runtime = match profile.pending_name() {
        "codex-login.pending" => "codex",
        "claude-login.pending" => "claude-code",
        _ => return Err(failure()),
    };
    binding.validate(&binding.run_id, runtime)?;
    let dir_meta = directory.dir_metadata().map_err(|_| failure())?;
    if (meta.dev(), meta.ino(), dir_meta.dev(), dir_meta.ino())
        != (
            binding.device,
            binding.inode,
            binding.directory_device,
            binding.directory_inode,
        )
    {
        return Err(failure());
    }
    let mut recorded = BTreeSet::new();
    for line in lines {
        let (pid, start) = line.split_once(' ').ok_or_else(failure)?;
        let identity = ProcIdentity {
            pid: pid.parse().map_err(|_| failure())?,
            start_us: start.parse().map_err(|_| failure())?,
        };
        if identity.pid <= 1
            || identity.start_us <= 0
            || line != format!("{} {}", identity.pid, identity.start_us)
            || !recorded.insert(identity)
            || recorded.len() > MAX_TRACKED
        {
            return Err(failure());
        }
    }
    // Empty/unlaunched reservations need their live owner; do not infer closure.
    if recorded.is_empty() {
        return Err(failure());
    }
    let reservation = Reservation {
        name: profile.pending_name(),
        directory,
        file,
        recorded,
        failed: false,
    };
    reservation.verify()?;
    verify_terminal(&binding, &reservation.recorded)?;
    for identity in &reservation.recorded {
        if !matches!(
            SystemProcTable.probe(*identity),
            hello_agent_core::proctree::ProbeResult::Gone
        ) {
            return Err(failure());
        }
    }
    if profile.revalidate()? != home
        || !crate::provider_profile::same_directory(
            &reservation.directory,
            &profile.login_lease_directory()?,
        )?
    {
        return Err(failure());
    }
    reservation.release()
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
