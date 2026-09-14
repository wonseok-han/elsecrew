//! App-owned Codex authentication profile preparation. Never reads/copies credentials.
//! Only called with the sidecar's trusted app-data directory capability, not a project.
//! Login/runtime must still verify path-to-handle identity before spawning a vendor.

use crate::SidecarError;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{
    Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptions, OpenOptionsExt, PermissionsExt,
};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub(crate) const CODEX_PROFILE_DIRECTORY: &str = "codex-profile-v1";
const CONFIG: &[u8] = b"# Managed by Hello, Agent. Credentials stay in this profile.\ncli_auth_credentials_store = \"file\"\n";

fn failure() -> SidecarError {
    SidecarError::new(
        "providerProfileUnavailable",
        "app-owned provider profile could not be verified",
    )
}

/// Returns a retained directory capability, not a path or launch authorization.
/// Existing unexpected content is preserved and refused, never repaired in place.
pub(crate) fn prepare_codex_profile(app_data: &Dir) -> Result<Dir, SidecarError> {
    profile_directory(app_data, true)
}

fn profile_directory(app_data: &Dir, create_missing: bool) -> Result<Dir, SidecarError> {
    let parent = app_data.dir_metadata().map_err(|_| failure())?;
    if parent.uid() != unsafe { libc::geteuid() } || parent.permissions().mode() & 0o022 != 0 {
        return Err(failure());
    }
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    if create_missing {
        match app_data.create_dir_with(CODEX_PROFILE_DIRECTORY, &builder) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(failure()),
        }
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .maybe_dir(true)
        .custom_flags(libc::O_NONBLOCK);
    let handle = app_data
        .open_with(CODEX_PROFILE_DIRECTORY, &options)
        .map_err(|_| failure())?;
    let metadata = handle.metadata().map_err(|_| failure())?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(failure());
    }
    let profile = Dir::from_std_file(handle.into_std());
    // Login-only profile contract: only the pinned vendor's observed runtime
    // artifact names below are admitted; arbitrary existing profiles are rejected.
    for entry in profile.entries().map_err(|_| failure())? {
        let entry = entry.map_err(|_| failure())?;
        if entry.file_name() != "config.toml" && entry.file_name() != "auth.json" {
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(failure)?;
            let directory = matches!(name, "skills" | "tmp" | ".tmp");
            let database = [
                "goals_1.sqlite",
                "logs_2.sqlite",
                "memories_1.sqlite",
                "queue_1.sqlite",
                "state_5.sqlite",
            ]
            .iter()
            .any(|base| {
                name == *base || name == format!("{base}-shm") || name == format!("{base}-wal")
            });
            if !directory && !database && name != "installation_id" {
                return Err(failure());
            }
            let meta = profile.symlink_metadata(name).map_err(|_| failure())?;
            if meta.uid() != unsafe { libc::geteuid() }
                || meta.permissions().mode() & 0o022 != 0
                || (directory && !meta.is_dir())
                || (!directory && (!meta.is_file() || meta.nlink() != 1))
            {
                return Err(failure());
            }
        }
        if entry.file_name() == "auth.json" {
            let metadata = profile
                .symlink_metadata("auth.json")
                .map_err(|_| failure())?;
            if !metadata.is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o777 != 0o600
            {
                return Err(failure());
            }
        }
    }
    let mut create = OpenOptions::new();
    create
        .write(true)
        .create_new(true)
        .mode(0o600)
        .follow(FollowSymlinks::No);
    if create_missing {
        match profile.open_with("config.toml", &create) {
            Ok(mut file) => {
                file.write_all(CONFIG).map_err(|_| failure())?;
                file.sync_all().map_err(|_| failure())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(failure()),
        }
    }
    let mut read = OpenOptions::new();
    read.read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let mut file = profile
        .open_with("config.toml", &read)
        .map_err(|_| failure())?;
    let metadata = file.metadata().map_err(|_| failure())?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() != CONFIG.len() as u64
    {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(CONFIG.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes != CONFIG {
        return Err(failure());
    }
    Ok(profile)
}

/// Retains the app-data and login directory identities for the owning sidecar.
/// Construction accepts an opened Storage, never a renderer-supplied path.
/// This is a point-in-time check, not protection from hostile same-UID mutations
/// after validation. The process owner still owns lifetime and launch isolation.
pub(crate) struct CodexProfileBinding {
    app_data_path: PathBuf,
    app_data: Dir,
    profile: Dir,
}

impl CodexProfileBinding {
    pub(super) fn login_lease_directory(&self) -> Result<Dir, SidecarError> {
        self.revalidate()?;
        self.app_data.try_clone().map_err(|_| failure())
    }

    pub(crate) fn from_storage(storage: &crate::Storage) -> Result<Self, SidecarError> {
        let database_path = storage.path();
        if database_path.file_name()
            != Some(std::ffi::OsStr::new(
                hello_agent_protocol::lifecycle_v1::SQLITE_FILE_V1,
            ))
        {
            return Err(failure());
        }
        let path = database_path.parent().ok_or_else(failure)?;
        let app_data = open_absolute_directory(path)?;
        let profile = prepare_codex_profile(&app_data)?;
        let binding = Self {
            app_data_path: path.to_path_buf(),
            app_data,
            profile,
        };
        binding.revalidate()?;
        Ok(binding)
    }

    /// Reopens without following links and never recreates a missing directory or
    /// config. No vendor or authentication file contents are read here.
    pub(crate) fn revalidate(&self) -> Result<PathBuf, SidecarError> {
        let current = open_absolute_directory(&self.app_data_path)?;
        if !same_directory(&current, &self.app_data)? {
            return Err(failure());
        }
        let profile = profile_directory(&current, false)?;
        if !same_directory(&profile, &self.profile)? {
            return Err(failure());
        }
        Ok(self.app_data_path.join(CODEX_PROFILE_DIRECTORY))
    }
}

fn same_directory(left: &Dir, right: &Dir) -> Result<bool, SidecarError> {
    let left = left.dir_metadata().map_err(|_| failure())?;
    let right = right.dir_metadata().map_err(|_| failure())?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

pub(super) fn open_absolute_directory(path: &Path) -> Result<Dir, SidecarError> {
    if !path.is_absolute() || path == Path::new("/") {
        return Err(failure());
    }
    let mut current =
        Dir::open_ambient_dir("/", cap_std::ambient_authority()).map_err(|_| failure())?;
    for component in path.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => return Err(failure()),
        };
        let mut options = OpenOptions::new();
        options
            .read(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No)
            .custom_flags(libc::O_NONBLOCK);
        let handle = current.open_with(name, &options).map_err(|_| failure())?;
        if !handle.metadata().map_err(|_| failure())?.is_dir() {
            return Err(failure());
        }
        current = Dir::from_std_file(handle.into_std());
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    fn root(temp: &tempfile::TempDir) -> Dir {
        Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap()
    }

    #[test]
    fn fresh_and_reopened_profile_preserve_credential_bytes_without_reading_them() {
        let temp = tempfile::tempdir().unwrap();
        let profile = prepare_codex_profile(&root(&temp)).unwrap();
        assert_eq!(profile.read("config.toml").unwrap(), CONFIG);
        profile
            .write("auth.json", b"synthetic credential sentinel")
            .unwrap();
        profile
            .set_permissions("auth.json", cap_std::fs::Permissions::from_mode(0o600))
            .unwrap();
        let reopened = prepare_codex_profile(&root(&temp)).unwrap();
        assert_eq!(
            reopened.read("auth.json").unwrap(),
            b"synthetic credential sentinel"
        );
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn rejects_changed_config_and_broad_permissions_without_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let profile = prepare_codex_profile(&root(&temp)).unwrap();
        profile.write("config.toml", b"user content").unwrap();
        assert!(prepare_codex_profile(&root(&temp)).is_err());
        assert_eq!(profile.read("config.toml").unwrap(), b"user content");
        let temp = tempfile::tempdir().unwrap();
        prepare_codex_profile(&root(&temp)).unwrap();
        let path = temp.path().join(CODEX_PROFILE_DIRECTORY);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(prepare_codex_profile(&root(&temp)).is_err());
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn rejects_profile_and_config_links_without_touching_targets() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), temp.path().join(CODEX_PROFILE_DIRECTORY)).unwrap();
        assert!(prepare_codex_profile(&root(&temp)).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        let temp = tempfile::tempdir().unwrap();
        prepare_codex_profile(&root(&temp)).unwrap();
        let config = temp
            .path()
            .join(CODEX_PROFILE_DIRECTORY)
            .join("config.toml");
        std::fs::remove_file(&config).unwrap();
        let target = outside.path().join("sentinel");
        std::fs::write(&target, CONFIG).unwrap();
        symlink(&target, &config).unwrap();
        assert!(prepare_codex_profile(&root(&temp)).is_err());
        assert_eq!(std::fs::read(target).unwrap(), CONFIG);
    }

    #[test]
    fn observed_login_runtime_names_require_safe_types_and_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let profile = prepare_codex_profile(&root(&temp)).unwrap();
        for name in ["skills", "tmp", ".tmp"] {
            profile.create_dir(name).unwrap();
        }
        for name in [
            "installation_id",
            "state_5.sqlite",
            "state_5.sqlite-wal",
            "state_5.sqlite-shm",
        ] {
            profile.write(name, b"synthetic runtime artifact").unwrap();
        }
        prepare_codex_profile(&root(&temp)).unwrap();
        let database = temp
            .path()
            .join(CODEX_PROFILE_DIRECTORY)
            .join("state_5.sqlite");
        std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(prepare_codex_profile(&root(&temp)).is_err());
        std::fs::remove_file(&database).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("sentinel");
        std::fs::write(&target, b"preserve").unwrap();
        symlink(&target, &database).unwrap();
        assert!(prepare_codex_profile(&root(&temp)).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"preserve");
    }

    #[test]
    fn rejects_unknown_profile_artifacts_without_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let profile = prepare_codex_profile(&root(&temp)).unwrap();
        profile
            .write("AGENTS.md", b"unexpected instructions")
            .unwrap();
        assert!(prepare_codex_profile(&root(&temp)).is_err());
        assert_eq!(
            profile.read("AGENTS.md").unwrap(),
            b"unexpected instructions"
        );
        assert_eq!(profile.read("config.toml").unwrap(), CONFIG);
    }

    #[test]
    fn binding_detects_directory_replacement_and_never_repairs_missing_config() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().canonicalize().unwrap();
        let database = canonical.join(hello_agent_protocol::lifecycle_v1::SQLITE_FILE_V1);
        let storage = crate::Storage::open(&database).unwrap();
        let binding = CodexProfileBinding::from_storage(&storage).unwrap();
        drop(storage);
        let path = binding.revalidate().unwrap();
        std::fs::remove_file(path.join("config.toml")).unwrap();
        assert!(binding.revalidate().is_err());
        assert!(!path.join("config.toml").exists());
        std::fs::rename(&path, canonical.join("retained-profile")).unwrap();
        let _replacement = prepare_codex_profile(&root(&temp)).unwrap();
        assert!(binding.revalidate().is_err());
    }

    #[test]
    fn binding_rejects_symlinked_ancestors_and_replaced_app_data() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        std::fs::create_dir(base.join("data")).unwrap();
        let database = base
            .join("data")
            .join(hello_agent_protocol::lifecycle_v1::SQLITE_FILE_V1);
        let storage = crate::Storage::open(&database).unwrap();
        let binding = CodexProfileBinding::from_storage(&storage).unwrap();
        drop(storage);
        std::fs::rename(base.join("data"), base.join("original")).unwrap();
        std::fs::create_dir(base.join("data")).unwrap();
        assert!(binding.revalidate().is_err());
        assert_eq!(std::fs::read_dir(base.join("data")).unwrap().count(), 0);
        symlink(base.join("original"), base.join("alias")).unwrap();
        assert!(open_absolute_directory(&base.join("alias")).is_err());
        let storage = crate::Storage::open(&base.join("wrong.sqlite")).unwrap();
        assert!(CodexProfileBinding::from_storage(&storage).is_err());
    }
}
