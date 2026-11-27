//! Backend-owned, expiring, single-use installation consent. No executable,
//! environment, URL or destination is accepted from the renderer.
use super::failure;
use crate::{
    provider_profile::{open_absolute_directory, same_directory},
    SidecarError,
};
use cap_std::fs::{Dir, MetadataExt, PermissionsExt};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(180);

pub(super) fn directories(provider: &str) -> Result<&'static [&'static str], SidecarError> {
    match provider {
        "codex" => Ok(&[".local/bin", ".codex/packages/standalone"]),
        "claude" => Ok(&[
            ".local/bin",
            ".claude/downloads",
            ".local/share/claude",
            ".local/state/claude",
            ".cache/claude",
        ]),
        _ => Err(failure()),
    }
}

#[derive(Default)]
pub(crate) struct ConsentStore {
    pending: Option<Prepared>,
}

struct Prepared {
    id: uuid::Uuid,
    created: Instant,
    authorization: AuthorizedInstall,
}

/// Not Clone or deserializable. Only consuming a pending plan constructs this.
pub(crate) struct AuthorizedInstall {
    operation: uuid::Uuid,
    provider: String,
    home_path: PathBuf,
    home: Dir,
    existing: Vec<(PathBuf, Dir)>,
    missing: Vec<PathBuf>,
    expires_at: Instant,
}

impl ConsentStore {
    pub(crate) fn prepare(&mut self, provider: &str) -> Result<Value, SidecarError> {
        self.pending = None;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(failure)?;
        self.prepare_at_home(provider, &home)
    }

    pub(super) fn prepare_at_home(
        &mut self,
        provider: &str,
        home_path: &Path,
    ) -> Result<Value, SidecarError> {
        self.pending = None;
        let roots = directories(provider)?;
        // Display paths must serialize losslessly; ':' would introduce extra
        // PATH entries when the fixed installer environment is constructed.
        if home_path.to_str().is_none_or(|path| {
            path.len() > 4096 || path.contains(':') || path.chars().any(char::is_control)
        }) {
            return Err(failure());
        }
        let home = checked(home_path)?;
        let mut existing = Vec::new();
        let mut missing = Vec::new();
        for relative in roots {
            let mut current = home_path.to_owned();
            let mut parent = home.try_clone().map_err(|_| failure())?;
            for component in relative.split('/') {
                current.push(component);
                match parent.symlink_metadata(component) {
                    Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
                        parent = checked(&current)?;
                        existing
                            .push((current.clone(), parent.try_clone().map_err(|_| failure())?));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        missing.push(current.clone());
                        break;
                    }
                    _ => return Err(failure()),
                }
            }
        }
        let authorization = AuthorizedInstall {
            operation: uuid::Uuid::new_v4(),
            provider: provider.to_owned(),
            home_path: home_path.to_owned(),
            home,
            existing,
            missing,
            expires_at: Instant::now() + TTL,
        };
        authorization.revalidate_plan()?;
        let id = uuid::Uuid::new_v4();
        let view = json!({
            "planId":id.to_string(), "provider":provider,
            "action":"installOrUpdate", "expiresInSeconds":TTL.as_secs(),
            "writeDirectories":roots.iter().map(|relative| home_path.join(relative)).collect::<Vec<_>>(),
            "replacesExistingCommands":true,
            "shellSettingsWritable":false,
            "startsLogin":false,
        });
        self.pending = Some(Prepared {
            id,
            created: Instant::now(),
            authorization,
        });
        Ok(view)
    }

    /// An acceptance naming the pending plan consumes it, including
    /// wrong-provider, expired, replaced-path and duplicate submissions. A
    /// new plan needs new UI consent.
    ///
    /// An acceptance naming another plan (an older one, answered in a box
    /// opened before the plan was prepared again) is refused and leaves the
    /// pending plan untouched: it was never shown in that box, so this answer
    /// cannot be its consent, and refusing it must not discard it either
    /// (PR #14 re-review). `Err(None)` marks that case for the caller.
    pub(crate) fn accept(
        &mut self,
        id: &str,
        provider: &str,
    ) -> Result<AuthorizedInstall, Option<SidecarError>> {
        if self
            .pending
            .as_ref()
            .is_none_or(|prepared| id != prepared.id.to_string())
        {
            return Err(None);
        }
        let prepared = self.pending.take().ok_or(None)?;
        if provider != prepared.authorization.provider || prepared.created.elapsed() >= TTL {
            return Err(Some(failure()));
        }
        prepared.authorization.revalidate_plan().map_err(Some)?;
        Ok(prepared.authorization)
    }

    pub(crate) fn dismiss(&mut self) {
        self.pending = None;
    }

    #[cfg(test)]
    pub(super) fn pending_id_for_test(&self) -> Option<String> {
        self.pending.as_ref().map(|prepared| prepared.id.to_string())
    }
}

impl AuthorizedInstall {
    pub(crate) fn operation(&self) -> uuid::Uuid {
        self.operation
    }
    pub(crate) fn provider(&self) -> &str {
        &self.provider
    }

    pub(super) fn revalidate_plan(&self) -> Result<(), SidecarError> {
        if Instant::now() >= self.expires_at {
            return Err(failure());
        }
        if !same_directory(&self.home, &checked(&self.home_path)?)? {
            return Err(failure());
        }
        for (path, held) in &self.existing {
            if !same_directory(held, &checked(path)?)? {
                return Err(failure());
            }
        }
        for path in &self.missing {
            match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(failure()),
            }
        }
        Ok(())
    }

    /// Last plan validation before creating installation directories. Subsequent
    /// checks belong to Target, which retains those newly created capabilities.
    pub(super) fn bind_home(&self) -> Result<(PathBuf, Dir), SidecarError> {
        self.revalidate_plan()?;
        Ok((
            self.home_path.clone(),
            self.home.try_clone().map_err(|_| failure())?,
        ))
    }
}

fn checked(path: &Path) -> Result<Dir, SidecarError> {
    let directory = open_absolute_directory(path)?;
    let meta = directory.dir_metadata().map_err(|_| failure())?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.permissions().mode() & 0o022 != 0 {
        return Err(failure());
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_is_single_use_provider_bound_expiring_and_side_effect_free() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mut store = ConsentStore::default();
        let first = store.prepare_at_home("codex", &root).unwrap();
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        let id = first["planId"].as_str().unwrap();
        assert!(store.accept(id, "claude").is_err());
        assert!(store.accept(id, "codex").is_err());
        let next = store.prepare_at_home("codex", &root).unwrap();
        store.pending.as_mut().unwrap().created = Instant::now() - TTL;
        assert!(store
            .accept(next["planId"].as_str().unwrap(), "codex")
            .is_err());
        let final_plan = store.prepare_at_home("claude", &root).unwrap();
        let id = final_plan["planId"].as_str().unwrap();
        let authorization = store.accept(id, "claude").unwrap();
        assert_eq!(authorization.provider(), "claude");
        assert!(store.accept(id, "claude").is_err());
        assert!(authorization.revalidate_plan().is_ok());
        std::fs::create_dir(root.join(".local")).unwrap();
        assert!(authorization.revalidate_plan().is_err());
    }

    #[test]
    fn an_answer_for_an_older_plan_leaves_the_new_plan_waiting() {
        // PR #14 re-review: a box opened for the first plan is answered after
        // the plan was prepared again. That answer is refused, and the plan
        // now awaiting consent is neither used nor discarded.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mut store = ConsentStore::default();
        let older = store.prepare_at_home("codex", &root).unwrap();
        let newer = store.prepare_at_home("codex", &root).unwrap();
        let older_id = older["planId"].as_str().unwrap();
        let newer_id = newer["planId"].as_str().unwrap();
        assert_ne!(older_id, newer_id);
        for provider in ["codex", "claude"] {
            assert!(matches!(store.accept(older_id, provider), Err(None)));
            assert!(matches!(store.accept("not-a-plan", provider), Err(None)));
        }
        assert_eq!(store.pending.as_ref().unwrap().id.to_string(), newer_id);
        // The pending plan's own answer still behaves as before: a wrong
        // provider consumes it.
        assert!(matches!(store.accept(newer_id, "claude"), Err(Some(_))));
        assert!(store.pending.is_none());
        assert!(matches!(store.accept(newer_id, "codex"), Err(None)));
        let last = store.prepare_at_home("codex", &root).unwrap();
        assert!(matches!(store.accept(older_id, "codex"), Err(None)));
        assert_eq!(
            store
                .accept(last["planId"].as_str().unwrap(), "codex")
                .ok()
                .unwrap()
                .provider(),
            "codex"
        );
    }

    #[test]
    fn replaced_or_linked_destinations_require_new_consent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir(root.join(".local")).unwrap();
        let mut store = ConsentStore::default();
        let plan = store.prepare_at_home("codex", &root).unwrap();
        std::fs::rename(root.join(".local"), root.join("retained-local")).unwrap();
        std::fs::create_dir(root.join(".local")).unwrap();
        assert!(store
            .accept(plan["planId"].as_str().unwrap(), "codex")
            .is_err());
        std::os::unix::fs::symlink(root.join("retained-local"), root.join(".local/bin")).unwrap();
        assert!(store.prepare_at_home("codex", &root).is_err());
    }
}
