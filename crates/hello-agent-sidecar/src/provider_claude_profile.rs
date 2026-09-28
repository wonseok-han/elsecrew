//! Dedicated Claude config/Keychain namespace. Never read credential contents.
use crate::{
    provider_profile::{open_absolute_directory, same_directory, LoginProfile},
    SidecarError, Storage,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptions, OpenOptionsExt};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

const DIRECTORY: &str = "claude-profile-v1";
const SETTINGS: &[u8] = b"{\"forceLoginMethod\":\"claudeai\"}\n";
fn failure() -> SidecarError {
    SidecarError::new(
        "providerProfileUnavailable",
        "Claude profile could not be verified",
    )
}

pub(crate) struct ClaudeProfileBinding {
    path: PathBuf,
    parent: Dir,
    profile: Dir,
}
impl ClaudeProfileBinding {
    pub(crate) fn from_storage(storage: &Storage) -> Result<Self, SidecarError> {
        let database = storage.path();
        if database.file_name() != Some(hello_agent_protocol::lifecycle_v1::SQLITE_FILE_V1.as_ref())
        {
            return Err(failure());
        }
        let path = database.parent().ok_or_else(failure)?.to_path_buf();
        let parent = open_absolute_directory(&path)?;
        let profile = open(&parent, true)?;
        Ok(Self {
            path,
            parent,
            profile,
        })
    }
}
impl LoginProfile for ClaudeProfileBinding {
    fn revalidate(&self) -> Result<PathBuf, SidecarError> {
        let parent = open_absolute_directory(&self.path)?;
        if !same_directory(&parent, &self.parent)?
            || !same_directory(&open(&parent, false)?, &self.profile)?
        {
            return Err(failure());
        }
        Ok(self.path.join(DIRECTORY))
    }
    fn login_lease_directory(&self) -> Result<Dir, SidecarError> {
        self.revalidate()?;
        self.parent.try_clone().map_err(|_| failure())
    }
    fn pending_name(&self) -> &'static str {
        "claude-login.pending"
    }
}

fn open(parent: &Dir, create: bool) -> Result<Dir, SidecarError> {
    let meta = parent.dir_metadata().map_err(|_| failure())?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return Err(failure());
    }
    if create {
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        match parent.create_dir_with(DIRECTORY, &builder) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(failure()),
        }
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let file = parent
        .open_with(DIRECTORY, &options)
        .map_err(|_| failure())?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o777 != 0o700 {
        return Err(failure());
    }
    let dir = Dir::from_std_file(file.into_std());
    if create {
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        match dir.create_dir_with("tmp", &builder) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(failure()),
        }
    }
    for entry in dir.entries().map_err(|_| failure())? {
        let entry = entry.map_err(|_| failure())?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(failure)?;
        let directory = matches!(name, "backups" | "debug" | "cache" | "tmp");
        if !directory && !matches!(name, "settings.json" | ".claude.json" | ".credentials.json") {
            return Err(failure());
        }
        let meta = dir.symlink_metadata(name).map_err(|_| failure())?;
        if meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o022 != 0
            || (directory && !meta.is_dir())
            || (!directory && (!meta.is_file() || meta.nlink() != 1))
            || (name == ".credentials.json" && meta.mode() & 0o777 != 0o600)
        {
            return Err(failure());
        }
    }
    if create {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .follow(FollowSymlinks::No);
        match dir.open_with("settings.json", &options) {
            Ok(mut file) => {
                file.write_all(SETTINGS).map_err(|_| failure())?;
                file.sync_all().map_err(|_| failure())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(failure()),
        }
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(libc::O_NONBLOCK);
    let file = dir
        .open_with("settings.json", &options)
        .map_err(|_| failure())?;
    let meta = file.metadata().map_err(|_| failure())?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o777 != 0o600
    {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    file.take(SETTINGS.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes != SETTINGS {
        return Err(failure());
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn profiles_and_fences_are_separate_and_foreign_content_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let codex = crate::provider_profile::CodexProfileBinding::from_storage(&storage).unwrap();
        let claude = ClaudeProfileBinding::from_storage(&storage).unwrap();
        assert_ne!(codex.revalidate().unwrap(), claude.revalidate().unwrap());
        let mut a = crate::provider_login_process::LoginProcess::reserve(&codex).unwrap();
        let mut b = crate::provider_login_process::LoginProcess::reserve(&claude).unwrap();
        assert!(root.join("codex-login.pending").exists());
        assert!(root.join("claude-login.pending").exists());
        a.finish().unwrap();
        assert!(root.join("claude-login.pending").exists());
        b.finish().unwrap();
        let credentials = root.join(DIRECTORY).join(".credentials.json");
        std::fs::write(&credentials, b"synthetic sentinel").unwrap();
        std::fs::set_permissions(&credentials, std::fs::Permissions::from_mode(0o600)).unwrap();
        claude.revalidate().unwrap();
        assert_eq!(std::fs::read(&credentials).unwrap(), b"synthetic sentinel");
        std::fs::write(
            root.join(DIRECTORY).join("settings.json"),
            b"foreign settings",
        )
        .unwrap();
        assert!(claude.revalidate().is_err());
        assert_eq!(
            std::fs::read(root.join(DIRECTORY).join("settings.json")).unwrap(),
            b"foreign settings"
        );
        assert!(codex.revalidate().is_ok());
    }
    #[test]
    fn profile_symlink_is_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.join(DIRECTORY)).unwrap();
        assert!(ClaudeProfileBinding::from_storage(&storage).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
