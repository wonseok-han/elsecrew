//! The build-then-preview write holds outlive an engine restart (ADE
//! stabilization A; before, PR #22's holds lived only in the engine's memory).
//!
//! Main runs the build and the engine keeps the hold that keeps team runs and
//! saves out of that workspace (work_scope.rs). Main names each hold itself
//! (`pbh_…`), so a reserve whose answer was lost can still be released. Every
//! change is written here with the identity of the app that asked (the
//! engine's parent: process number and start time) before it is answered.
//!
//! A new engine restores, before it answers any request (PR #25 review B2,
//! fail closed):
//! - holds of this same app: held, as before the restart;
//! - holds an earlier app run left (`stale`): they still block, because a
//!   build of that run may have outlived it;
//! - a file it cannot read or trust, or no identity of its own app
//!   (`unknown`): every task counts as busy.
//!
//! Main then says which holds it still has and whether every process an
//! earlier app run's build left is gone (`preview.buildHolds`): its own list
//! replaces this app's holds, and `stale`/`unknown` go only once nothing of an
//! earlier build can still write. Until then a refusal says why
//! (`BLOCKED_DETAIL`), and main asks again while leftovers remain.
//!
//! A change that cannot be written is not made: a reserve answers busy, a
//! release or sync answers an error and main tries again.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FILE: &str = "preview-build-holds-v1.json";
const MAX_HOLDS: usize = 64;

/// What a refusal caused by a hold this engine could not confirm says.
pub(crate) const BLOCKED_DETAIL: &str = "a build-then-preview job from before the engine restarted may still be writing; it is released once the app confirms its processes have ended";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Holds {
    /// This app's holds: hold id → task.
    pub(crate) mine: BTreeMap<String, String>,
    /// Holds an earlier app run left (still blocking).
    pub(crate) stale: BTreeMap<String, String>,
    /// The record could not be read or trusted: every task is busy.
    pub(crate) unknown: bool,
}

impl Holds {
    /// Something not confirmed by this app still blocks.
    pub(crate) fn unconfirmed(&self) -> bool {
        self.unknown || !self.stale.is_empty()
    }

    pub(crate) fn chats(&self) -> impl Iterator<Item = &String> {
        self.mine.values().chain(self.stale.values())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.mine.is_empty() && self.stale.is_empty() && !self.unknown
    }
}

pub(crate) struct HoldFile {
    path: PathBuf,
    owner: Option<elsecrew_core::proctree::ProcIdentity>,
    #[cfg(test)]
    pub(crate) fail_writes: std::cell::Cell<bool>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Saved {
    schema_version: u32,
    owner_pid: u32,
    owner_start_us: i64,
    holds: BTreeMap<String, String>,
    stale: BTreeMap<String, String>,
    unknown: bool,
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

pub(crate) fn valid_hold(id: &str) -> bool {
    id.strip_prefix("pbh_").is_some_and(|rest| rest.len() == 32 && rest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
}

pub(crate) fn valid_chat(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn valid_map(map: &BTreeMap<String, String>) -> bool {
    map.len() <= MAX_HOLDS && map.iter().all(|(hold, chat)| valid_hold(hold) && valid_chat(chat))
}

impl HoldFile {
    pub(crate) fn in_directory(directory: &Path) -> Self {
        Self::with_owner(directory, parent())
    }

    pub(crate) fn with_owner(directory: &Path, owner: Option<elsecrew_core::proctree::ProcIdentity>) -> Self {
        Self {
            path: directory.join(FILE),
            owner,
            #[cfg(test)]
            fail_writes: std::cell::Cell::new(false),
        }
    }

    /// What blocks when this engine starts (see the module comment). The
    /// file is never removed here: only a confirmed change rewrites it.
    pub(crate) fn restore(&self) -> Holds {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Holds::default(),
            Err(_) => return Holds { unknown: true, ..Holds::default() },
        };
        let Some(saved) = serde_json::from_slice::<Saved>(&bytes).ok()
            .filter(|saved| saved.schema_version == 2 && valid_map(&saved.holds) && valid_map(&saved.stale))
        else {
            return Holds { unknown: true, ..Holds::default() };
        };
        let same = self.owner.is_some_and(|owner| owner.pid == saved.owner_pid && owner.start_us == saved.owner_start_us);
        if same {
            return Holds { mine: saved.holds, stale: saved.stale, unknown: saved.unknown };
        }
        let mut stale = saved.stale;
        stale.extend(saved.holds);
        // Without its own app's identity this engine cannot tell whose they are.
        Holds { mine: BTreeMap::new(), stale, unknown: saved.unknown || self.owner.is_none() }
    }

    /// Writes `holds` (whole file, then renamed into place).
    pub(crate) fn save(&self, holds: &Holds) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fail_writes.get() {
            return Err(std::io::Error::other("injected write failure"));
        }
        if holds.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        let owner = self.owner.ok_or_else(|| std::io::Error::other("no owner identity"))?;
        let saved = Saved {
            schema_version: 2, owner_pid: owner.pid, owner_start_us: owner.start_us,
            holds: holds.mine.clone(), stale: holds.stale.clone(), unknown: holds.unknown,
        };
        let bytes = serde_json::to_vec(&saved).map_err(std::io::Error::other)?;
        let temporary = self.path.with_extension(format!("json.{}.tmp", std::process::id()));
        {
            use std::io::Write;
            let _ = std::fs::remove_file(&temporary);
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

    /// ADE stabilization A, PR #25 review B2: holds written by one engine come
    /// back in the next engine of the same app; another app run's holds come
    /// back as still blocking (`stale`); a record that cannot be read or
    /// trusted blocks every task (`unknown`). Nothing is dropped on restore.
    #[test]
    fn holds_come_back_and_what_cannot_be_confirmed_keeps_blocking() {
        let directory = tempfile::tempdir().unwrap();
        let app = ProcIdentity { pid: 4242, start_us: 1_000_000 };
        let file = HoldFile::with_owner(directory.path(), Some(app));
        let holds = Holds { mine: BTreeMap::from([(hold(1), "chat_a".to_string()), (hold(2), "chat_b".to_string())]), ..Holds::default() };
        file.save(&holds).unwrap();
        assert_eq!(HoldFile::with_owner(directory.path(), Some(app)).restore(), holds, "same app: restored");
        // Another app run (a new process, or the same number started later):
        // its builds may have outlived it, so the holds still block.
        for other in [ProcIdentity { pid: 4243, start_us: 1_000_000 }, ProcIdentity { pid: 4242, start_us: 2_000_000 }] {
            let restored = HoldFile::with_owner(directory.path(), Some(other)).restore();
            assert_eq!(restored.stale, holds.mine);
            assert!(restored.mine.is_empty() && !restored.unknown && restored.unconfirmed());
            assert!(directory.path().join(FILE).exists(), "kept until confirmed");
        }
        // No identity of its own app: cannot tell whose they are.
        let unknown = HoldFile::with_owner(directory.path(), None).restore();
        assert!(unknown.unknown && unknown.stale == holds.mine);
        // A damaged, foreign or unreadable record blocks everything.
        for damaged in [&b"{not json"[..], br#"{"schemaVersion":2,"ownerPid":4242,"ownerStartUs":1000000,"holds":{"x":"chat_a"},"stale":{},"unknown":false}"#,
            br#"{"schemaVersion":1,"ownerPid":4242,"ownerStartUs":1000000,"holds":{}}"#] {
            std::fs::write(directory.path().join(FILE), damaged).unwrap();
            let restored = file.restore();
            assert!(restored.unknown && restored.mine.is_empty(), "{}", String::from_utf8_lossy(damaged));
            assert!(directory.path().join(FILE).exists());
        }
        std::fs::remove_file(directory.path().join(FILE)).unwrap();
        std::fs::create_dir(directory.path().join(FILE)).unwrap();
        assert!(file.restore().unknown, "a record that cannot be read");
        std::fs::remove_dir(directory.path().join(FILE)).unwrap();
        // Released down to nothing: no file, nothing blocks.
        file.save(&holds).unwrap();
        file.save(&Holds::default()).unwrap();
        assert!(!directory.path().join(FILE).exists());
        assert_eq!(file.restore(), Holds::default());
        // Without an owner identity nothing can be written.
        assert!(HoldFile::with_owner(directory.path(), None).save(&holds).is_err());
        #[cfg(unix)]
        assert!(parent().is_some(), "the engine's real parent has an identity");
    }
}
