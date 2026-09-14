//! App-owned Codex authentication profile preparation. Never reads/copies credentials.
//! Only called with the sidecar's trusted app-data directory capability, not a project.
//! Login/runtime must still verify path-to-handle identity before spawning a vendor.

use crate::SidecarError;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{
    Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptions, OpenOptionsExt, PermissionsExt,
};
use std::io::{Read, Write};

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
    let parent = app_data.dir_metadata().map_err(|_| failure())?;
    if parent.uid() != unsafe { libc::geteuid() } || parent.permissions().mode() & 0o022 != 0 {
        return Err(failure());
    }
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    match app_data.create_dir_with(CODEX_PROFILE_DIRECTORY, &builder) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(failure()),
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
    // Login-only profile contract. Additional vendor runtime artifacts require a
    // separately reviewed policy before the runtime can use this directory.
    for entry in profile.entries().map_err(|_| failure())? {
        let entry = entry.map_err(|_| failure())?;
        if entry.file_name() != "config.toml" && entry.file_name() != "auth.json" {
            return Err(failure());
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
    match profile.open_with("config.toml", &create) {
        Ok(mut file) => {
            file.write_all(CONFIG).map_err(|_| failure())?;
            file.sync_all().map_err(|_| failure())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(failure()),
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
}
