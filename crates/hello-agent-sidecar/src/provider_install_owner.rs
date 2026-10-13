//! Installation-only binding for the existing durable process owner.
//! Not wired to public IPC yet. No provider config or credentials live here.
use crate::{
    provider_login_process::LoginProcess,
    provider_profile::{open_absolute_directory, same_directory, LoginProfile},
    SidecarError, Storage,
};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::PathBuf;

const STAGING: &str = "provider-install-staging-v1";
const PENDING: &str = "provider-install.pending";

fn failure() -> SidecarError {
    SidecarError::new(
        "providerInstallRecoveryRequired",
        "installation workspace requires inspection",
    )
}

pub(crate) struct InstallScope {
    app_path: PathBuf,
    app: Dir,
    staging: Dir,
}

// Directory identity does not authenticate existing staging contents. A worker
// must create fresh artifacts with exclusive/no-follow opens and validate them
// before execution; constructing this scope grants no artifact execution rights.

impl InstallScope {
    /// Trusted storage only: renderer inputs cannot choose either directory.
    /// This creates a workspace, not a process reservation or consent token.
    pub(crate) fn from_storage(storage: &Storage) -> Result<Self, SidecarError> {
        let database = storage.path();
        if database.file_name()
            != Some(std::ffi::OsStr::new(
                hello_agent_protocol::lifecycle_v1::SQLITE_FILE_V1,
            ))
        {
            return Err(failure());
        }
        let app_path = database.parent().ok_or_else(failure)?.to_owned();
        let app = open_absolute_directory(&app_path)?;
        let meta = app.dir_metadata().map_err(|_| failure())?;
        if meta.uid() != unsafe { libc::geteuid() } || meta.permissions().mode() & 0o022 != 0 {
            return Err(failure());
        }
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        match app.create_dir_with(STAGING, &builder) {
            Ok(()) => {
                app.try_clone()
                    .map_err(|_| failure())?
                    .into_std_file()
                    .sync_all()
                    .map_err(|_| failure())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(failure()),
        }
        let staging = open_absolute_directory(&app_path.join(STAGING))?;
        let scope = Self {
            app_path,
            app,
            staging,
        };
        scope.revalidate()?;
        Ok(scope)
    }

    /// Reserve before any downloader/installer spawn. The fence survives Drop
    /// until the owning installation transaction has durably reached terminal.
    /// A previous fence (even empty/malformed) blocks a second reservation.
    pub(crate) fn reserve(&self, operation: uuid::Uuid) -> Result<LoginProcess<'_>, SidecarError> {
        if operation.is_nil() {
            return Err(failure());
        }
        let mut process = LoginProcess::reserve(self)?;
        process.retain_fence_before_launch(&operation.to_string())?;
        Ok(process)
    }
}

impl LoginProfile for InstallScope {
    fn revalidate(&self) -> Result<PathBuf, SidecarError> {
        let app = open_absolute_directory(&self.app_path)?;
        let staging = open_absolute_directory(&self.app_path.join(STAGING))?;
        let parent = app.dir_metadata().map_err(|_| failure())?;
        let meta = staging.dir_metadata().map_err(|_| failure())?;
        if !same_directory(&app, &self.app)?
            || !same_directory(&staging, &self.staging)?
            || parent.uid() != unsafe { libc::geteuid() }
            || parent.permissions().mode() & 0o022 != 0
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.permissions().mode() & 0o777 != 0o700
        {
            return Err(failure());
        }
        Ok(self.app_path.join(STAGING))
    }
    fn login_lease_directory(&self) -> Result<Dir, SidecarError> {
        self.revalidate()?;
        self.app.try_clone().map_err(|_| failure())
    }
    fn pending_name(&self) -> &'static str {
        PENDING
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_install_reservation_blocks_reopen_without_touching_auth() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let scope = InstallScope::from_storage(&storage).unwrap();
        let owner = scope.reserve(uuid::Uuid::new_v4()).unwrap();
        assert!(scope.reserve(uuid::Uuid::new_v4()).is_err());
        drop(owner);
        assert!(root.join(PENDING).is_file());
        let reopened = InstallScope::from_storage(&storage).unwrap();
        assert!(reopened.reserve(uuid::Uuid::new_v4()).is_err());
        assert!(!root.join("codex-profile-v1").exists());
        assert!(!root.join("claude-profile-v1").exists());
    }

    #[test]
    fn staging_link_or_replacement_is_rejected_and_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(STAGING)).unwrap();
        assert!(InstallScope::from_storage(&storage).is_err());
        assert!(root.join(STAGING).is_symlink());
        std::fs::remove_file(root.join(STAGING)).unwrap();
        let scope = InstallScope::from_storage(&storage).unwrap();
        std::fs::rename(root.join(STAGING), root.join("retained-staging")).unwrap();
        std::fs::create_dir(root.join(STAGING)).unwrap();
        assert!(scope.revalidate().is_err());
        assert!(scope.reserve(uuid::Uuid::new_v4()).is_err());
    }
}
