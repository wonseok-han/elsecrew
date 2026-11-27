//! The build-then-preview write holds outlive an engine restart (ADE
//! stabilization A; before, PR #22's holds lived only in the engine's memory).
//!
//! Main runs the build and the engine keeps the hold that keeps team runs and
//! saves out of that workspace (work_scope.rs). When the engine stops and main
//! starts it again, the build may still be running. So every reserve and
//! release is written here, with the identity of the app that asked (the
//! engine's parent: process number and start time). A new engine restores the
//! holds before it answers any request when its parent is that same app; any
//! other file is from an earlier app run, whose builds main itself settles at
//! its start (`recoverLeftovers`), and is removed. Main sends again a release
//! that could not reach the engine (`PreviewBuildManagerV1.engineReady`).
//!
//! A hold that cannot be written is refused (a reserve answers busy); a
//! release that cannot be written still frees the hold in memory, and the
//! file keeps it until the app's next start (fail closed).
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FILE: &str = "preview-build-holds-v1.json";

pub(crate) struct HoldFile {
    path: PathBuf,
    owner: Option<elsecrew_core::proctree::ProcIdentity>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Saved {
    schema_version: u32,
    owner_pid: u32,
    owner_start_us: i64,
    holds: BTreeMap<String, String>,
}

/// The app that runs this engine.
fn parent() -> Option<elsecrew_core::proctree::ProcIdentity> {
    #[cfg(unix)]
    {
        let pid = u32::try_from(unsafe { libc::getppid() }).ok().filter(|pid| *pid > 1)?;
        elsecrew_core::proctree::identity_of(pid).ok()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn valid_hold(id: &str) -> bool {
    id.strip_prefix("pbh_").is_some_and(|rest| rest.len() == 32 && rest.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn valid_chat(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

impl HoldFile {
    pub(crate) fn in_directory(directory: &Path) -> Self {
        Self::with_owner(directory, parent())
    }

    pub(crate) fn with_owner(directory: &Path, owner: Option<elsecrew_core::proctree::ProcIdentity>) -> Self {
        Self { path: directory.join(FILE), owner }
    }

    /// The holds this same app left with an earlier engine; any other file
    /// is removed. Anything unreadable restores nothing and is removed: main
    /// then cannot have a build this engine does not know about, because a
    /// build only starts after a hold was written (see `save`).
    pub(crate) fn restore(&self) -> BTreeMap<String, String> {
        let restored = (|| {
            let owner = self.owner?;
            let bytes = std::fs::read(&self.path).ok()?;
            let saved: Saved = serde_json::from_slice(&bytes).ok()?;
            if saved.schema_version != 1 || saved.owner_pid != owner.pid || saved.owner_start_us != owner.start_us
                || saved.holds.len() > 64
                || !saved.holds.iter().all(|(hold, chat)| valid_hold(hold) && valid_chat(chat))
            {
                return None;
            }
            Some(saved.holds)
        })();
        match restored {
            Some(holds) if !holds.is_empty() => holds,
            _ => {
                let _ = std::fs::remove_file(&self.path);
                BTreeMap::new()
            }
        }
    }

    /// Writes the current holds (whole file, then renamed into place).
    pub(crate) fn save(&self, holds: &BTreeMap<String, String>) -> std::io::Result<()> {
        if holds.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        let owner = self.owner.ok_or_else(|| std::io::Error::other("no owner identity"))?;
        let saved = Saved { schema_version: 1, owner_pid: owner.pid, owner_start_us: owner.start_us, holds: holds.clone() };
        let bytes = serde_json::to_vec(&saved).map_err(std::io::Error::other)?;
        let temporary = self.path.with_extension(format!("json.{}.tmp", std::process::id()));
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            let written = file.write_all(&bytes).and_then(|()| file.sync_all());
            if let Err(error) = written {
                drop(file);
                let _ = std::fs::remove_file(&temporary);
                return Err(error);
            }
        }
        std::fs::rename(&temporary, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elsecrew_core::proctree::ProcIdentity;

    fn hold(n: u8) -> String {
        format!("pbh_{}", format!("{n:02x}").repeat(16))
    }

    /// ADE stabilization A: holds written by one engine come back in the next
    /// engine of the same app, and are dropped for any other app run.
    #[test]
    fn holds_return_to_the_same_app_only() {
        let directory = tempfile::tempdir().unwrap();
        let app = ProcIdentity { pid: 4242, start_us: 1_000_000 };
        let file = HoldFile::with_owner(directory.path(), Some(app));
        let holds = BTreeMap::from([(hold(1), "chat_a".to_string()), (hold(2), "chat_b".to_string())]);
        file.save(&holds).unwrap();
        assert_eq!(HoldFile::with_owner(directory.path(), Some(app)).restore(), holds, "same app: restored");
        // Another app run (a new process, or the same number started later).
        for other in [ProcIdentity { pid: 4243, start_us: 1_000_000 }, ProcIdentity { pid: 4242, start_us: 2_000_000 }] {
            file.save(&holds).unwrap();
            assert!(HoldFile::with_owner(directory.path(), Some(other)).restore().is_empty());
            assert!(!directory.path().join(FILE).exists(), "a stale file is removed");
        }
        // Released down to nothing: no file.
        file.save(&holds).unwrap();
        file.save(&BTreeMap::new()).unwrap();
        assert!(!directory.path().join(FILE).exists());
        assert!(file.restore().is_empty());
        // A damaged or foreign file restores nothing and goes.
        for damaged in [&b"{not json"[..], br#"{"schemaVersion":1,"ownerPid":4242,"ownerStartUs":1000000,"holds":{"x":"chat_a"}}"#,
            br#"{"schemaVersion":2,"ownerPid":4242,"ownerStartUs":1000000,"holds":{}}"#] {
            std::fs::write(directory.path().join(FILE), damaged).unwrap();
            assert!(file.restore().is_empty());
            assert!(!directory.path().join(FILE).exists());
        }
        // Without an owner identity nothing is written or restored.
        let orphan = HoldFile::with_owner(directory.path(), None);
        assert!(orphan.save(&holds).is_err());
        assert!(orphan.restore().is_empty());
        // The engine's real parent has an identity on this platform.
        #[cfg(unix)]
        assert!(parent().is_some());
    }
}
