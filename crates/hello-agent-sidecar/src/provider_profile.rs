//! App-owned Codex authentication profile preparation. Never reads/copies credentials.
//! Only called with the sidecar's trusted app-data directory capability, not a project.
//! Login/runtime must still verify path-to-handle identity before spawning a vendor.

use crate::SidecarError;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{
    Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptions, OpenOptionsExt, PermissionsExt,
};
use hello_agent_core::provider::vendor_observation::codex_profile_config::{
    ProfileConfig, MAX_CONFIG_BYTES,
};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(target_os = "macos")]
mod stale_helper;
#[cfg(target_os = "macos")]
mod runtime_files;
#[cfg(target_os = "macos")]
mod claude_runtime_files;
#[cfg(target_os = "macos")]
pub(crate) use claude_runtime_files::verify_claude_runtime_files;
#[cfg(target_os = "macos")]
pub(crate) use runtime_files::verify_codex_runtime_files;

pub(crate) const CODEX_PROFILE_DIRECTORY: &str = "codex-profile-v1";
pub(crate) trait LoginProfile: Sync {
    fn revalidate(&self) -> Result<PathBuf, SidecarError>;
    fn login_lease_directory(&self) -> Result<Dir, SidecarError>;
    fn pending_name(&self) -> &'static str;
}
const CONFIG: &[u8] = b"# Managed by Hello, Agent. Credentials stay in this profile.\ncli_auth_credentials_store = \"file\"\n";

fn failure() -> SidecarError {
    SidecarError::new(
        "providerProfileUnavailable",
        "app-owned provider profile could not be verified",
    )
}

/// Execution-only admission. Ordinary login/status checks retain their existing
/// artifact contract. Scan metadata, including plugins and temporary descendants,
/// without reading credentials or following links. Native launch additionally
/// denies plugin cache reads/writes in provider_runtime_sandbox. This scanner
/// alone is not plugin-disable authority or a same-UID race barrier.
pub(crate) fn verify_runtime_files(home: &Path) -> Result<(), SidecarError> {
    let profile = open_absolute_directory(home)?;
    crate::project::runtime_isolation::verify(&profile).map_err(|error| {
        // Only fixed diagnostic categories cross this boundary, never paths,
        // filenames, OS error strings, or provider/credential contents.
        let detail = match error.detail.as_str() {
            "runtime directory listing failed" => "provider runtime directory listing failed",
            "runtime directory entry unavailable" => "provider runtime directory entry unavailable",
            "runtime entry metadata unavailable" => "provider runtime entry metadata unavailable",
            "runtime directory capability unavailable" => {
                "provider runtime directory capability unavailable"
            }
            "runtime symbolic link rejected" => "provider runtime symbolic link rejected",
            "runtime shared regular file rejected" => "provider runtime shared regular file rejected",
            "runtime special file rejected" => "provider runtime special file rejected",
            "runtime file scan exceeds depth budget" => "provider runtime scan exceeds depth budget",
            "runtime file scan exceeds entry budget" => "provider runtime scan exceeds entry budget",
            _ => "provider runtime file isolation scan rejected profile contents",
        };
        SidecarError::new("providerProfileUnavailable", detail)
    })
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
    // Vendor artifact names evolve. Preserve structural ownership checks and
    // exact app-managed config/credential checks, not a version-specific name list.
    for entry in profile.entries().map_err(|_| failure())? {
        let entry = entry.map_err(|_| failure())?;
        let file_name = entry.file_name();
        let folded = file_name.to_str().ok_or_else(failure)?.to_ascii_lowercase();
        let name = folded.as_str();
        if name != "config.toml" && name != "auth.json" {
            if matches!(name, "agents.md" | "claude.md" | "settings.local.json" | ".mcp.json") {
                return Err(failure());
            }
            let meta = profile.symlink_metadata(&file_name).map_err(|_| failure())?;
            let requires_directory = matches!(name, "skills" | "tmp" | ".tmp" | "plugins");
            let requires_file = matches!(name, "installation_id" | "models_cache.json")
                || name.ends_with(".sqlite") || name.ends_with(".sqlite-wal") || name.ends_with(".sqlite-shm");
            if meta.uid() != unsafe { libc::geteuid() }
                || meta.permissions().mode() & 0o022 != 0
                || !(meta.is_dir() || (meta.is_file() && meta.nlink() == 1))
                || (requires_directory && !meta.is_dir()) || (requires_file && !meta.is_file())
            {
                return Err(failure());
            }
        }
        if name == "auth.json" {
            let metadata = profile
                .symlink_metadata(&file_name)
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
    // Parsing is not launch authority. Every Codex launcher overrides stored
    // project trust per invocation; login/runtime additionally verify config
    // evidence before account observation or work. Preserve the source bytes.
    read_profile_config(&profile)?;
    Ok(profile)
}

fn read_profile_config(profile: &Dir) -> Result<ProfileConfig, SidecarError> {
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
        || metadata.len() > MAX_CONFIG_BYTES as u64
    {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    ProfileConfig::parse(&bytes).map_err(|_| failure())
}

/// Additional startup restriction, never profile or runtime admission.
/// All real app-server entry points use this without rewriting stored config.
pub(crate) fn codex_isolation_argument(home: &Path, cwd: &Path) -> std::io::Result<std::ffi::OsString> {
    let invalid = || std::io::Error::other("Codex startup isolation configuration unavailable");
    let directory = open_absolute_directory(home).map_err(|_| invalid())?;
    read_profile_config(&directory).map_err(|_| invalid())?
        .isolation_argument(&[home, cwd]).map(std::ffi::OsString::from).map_err(|_| invalid())
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
    pub(crate) fn try_clone(&self) -> Result<Self, SidecarError> {
        self.revalidate()?;
        let binding = Self {
            app_data_path: self.app_data_path.clone(),
            app_data: self.app_data.try_clone().map_err(|_| failure())?,
            profile: self.profile.try_clone().map_err(|_| failure())?,
        };
        binding.revalidate()?;
        Ok(binding)
    }

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

impl LoginProfile for CodexProfileBinding {
    fn revalidate(&self) -> Result<PathBuf, SidecarError> {
        self.revalidate()
    }
    fn login_lease_directory(&self) -> Result<Dir, SidecarError> {
        self.login_lease_directory()
    }
    fn pending_name(&self) -> &'static str {
        "codex-login.pending"
    }
}

pub(super) fn same_directory(left: &Dir, right: &Dir) -> Result<bool, SidecarError> {
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
    #[test]
    #[ignore = "requires explicit approval to recover an app-owned confirmed login fence"]
    fn installed_codex_confirmed_login_recovery() {
        assert_eq!(
            std::env::var("HELLO_AGENT_APPROVED_LOGIN_RECOVERY").as_deref(),
            Ok("1")
        );
        let path = std::path::PathBuf::from(
            std::env::var_os("HELLO_AGENT_INSPECTION_APP_DATA")
                .expect("explicit app data directory"),
        );
        assert!(path.ends_with("com.wonseokhan.helloagent/m5"));
        let app_data = super::open_absolute_directory(&path).unwrap();
        let profile = super::profile_directory(&app_data, false).unwrap();
        let binding = super::CodexProfileBinding {
            app_data_path: path,
            app_data,
            profile,
        };
        crate::provider_login_process::recover_confirmed_login(&binding).unwrap();
    }

    #[test]
    #[ignore = "requires approval for existing app-owned account and model inspection"]
    fn installed_codex_app_profile_model_free_inspection() {
        inspect_app_profile(false);
    }

    #[test]
    #[ignore = "requires approval for app-owned account inspection through local model proxy"]
    fn installed_codex_app_profile_proxied_model_free_inspection() {
        inspect_app_profile(true);
    }

    fn inspect_app_profile(proxied: bool) {
        use crate::provider_executable::PinnedExecutable;
        use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
        use std::sync::atomic::AtomicBool;
        assert_eq!(
            std::env::var("HELLO_AGENT_APPROVED_ACCOUNT_INSPECTION").as_deref(),
            Ok("1")
        );
        let path = std::path::PathBuf::from(
            std::env::var_os("HELLO_AGENT_INSPECTION_APP_DATA")
                .expect("explicit app data directory"),
        );
        assert!(path.ends_with("com.wonseokhan.helloagent/m5"));
        let app_data = super::open_absolute_directory(&path).unwrap();
        // Open an existing validated profile without opening/migrating the app DB.
        let profile = super::profile_directory(&app_data, false).unwrap();
        let binding = super::CodexProfileBinding {
            app_data_path: path,
            app_data,
            profile,
        };
        let home = binding.revalidate().unwrap();
        // This is the login/discovery contract, not runtime file admission.
        // Vendor-created temporary links still block actual code execution.
        let original_config = std::fs::read(home.join("config.toml")).unwrap();
        let cli = PinnedExecutable::open(
            std::path::Path::new("/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"),
            crate::provider_login_manager::digest("3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a"),
        ).unwrap();
        let helper = PinnedExecutable::open(
            std::path::Path::new(
                "/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex-code-mode-host",
            ),
            crate::provider_login_manager::digest(
                "4c39893666a166addd217df78f158c5975f69b598404a9f86ff0df0f3da43f6d",
            ),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().canonicalize().unwrap();
        let expected = ExpectedProfile {
            model: "gpt-6-astra",
            reasoning_effort: "medium",
        };
        let cancel = AtomicBool::new(false);
        let mut inspection = if proxied {
            crate::provider_preflight_session::inspect_proxied_preflight_candidate(
                &binding, &cli, &helper, &expected, &cancel, &workspace,
            )
        } else {
            crate::provider_preflight_session::inspect_full_preflight_candidate(
                &binding, &cli, &helper, &expected, &cancel,
            )
        }
        .unwrap();
        let cleanup = if inspection.process.closure_evidence().is_some() {
            Ok(())
        } else {
            inspection.process.finish()
        };
        let confirmed = inspection
            .process
            .closure_evidence()
            .is_some_and(|receipt| receipt.confirmed());
        let terminated = inspection
            .process
            .closure_evidence()
            .is_some_and(|receipt| receipt.termination_confirmed());
        eprintln!(
            "app_profile_inspection proxied={proxied} matched={} termination_confirmed={terminated} cleanup_confirmed={confirmed}",
            inspection.result.is_ok()
        );
        assert_eq!(
            std::fs::read(home.join("config.toml")).unwrap(),
            original_config
        );
        assert!(
            cleanup.is_ok() && confirmed,
            "inspection cleanup not confirmed"
        );
        assert!(
            inspection.result.is_ok(),
            "account/model configuration did not match"
        );
    }

    use super::*;
    use cap_std::ambient_authority;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    fn root(temp: &tempfile::TempDir) -> Dir {
        Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap()
    }

    #[test]
    fn runtime_admission_checks_nested_artifacts_without_changing_login_or_credentials() {
        for location in [
            "skills/.hidden/item",
            "tmp/nested/item",
            ".tmp/item",
            "plugins/cache/item",
        ] {
            let temp = tempfile::tempdir().unwrap();
            prepare_codex_profile(&root(&temp)).unwrap();
            let home = temp
                .path()
                .canonicalize()
                .unwrap()
                .join(CODEX_PROFILE_DIRECTORY);
            let outside = temp.path().join("original");
            std::fs::write(&outside, b"preserve").unwrap();
            let target = home.join(location);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(&outside, &target).unwrap();
            verify_runtime_files(&home).unwrap();
            std::fs::remove_file(&target).unwrap();
            std::fs::hard_link(&outside, &target).unwrap();
            // Login still accepts its known top-level artifact directories;
            // execution alone requires all descendants to be unshared.
            prepare_codex_profile(&root(&temp)).unwrap();
            assert!(verify_runtime_files(&home).is_err());
            std::fs::remove_file(&target).unwrap();
            symlink(&outside, &target).unwrap();
            assert!(verify_runtime_files(&home).is_err());
            std::fs::remove_file(&target).unwrap();
            use std::os::unix::ffi::OsStrExt;
            let fifo = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
            assert!(verify_runtime_files(&home).is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"preserve");
            assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), CONFIG);
        }
    }

    #[test]
    fn plugin_cache_requires_a_safe_directory_and_runtime_scans_its_contents() {
        for case in ["directory", "symlink", "file", "writable"] {
            let temp = tempfile::tempdir().unwrap();
            prepare_codex_profile(&root(&temp)).unwrap();
            let home = temp
                .path()
                .canonicalize()
                .unwrap()
                .join(CODEX_PROFILE_DIRECTORY);
            let plugins = home.join("plugins");
            match case {
                "symlink" => symlink(temp.path(), &plugins).unwrap(),
                "file" => std::fs::write(&plugins, b"not a cache directory").unwrap(),
                _ => {
                    std::fs::create_dir_all(plugins.join("cache/openai-curated-remote")).unwrap();
                    std::fs::create_dir(plugins.join(".remote-plugin-install-staging")).unwrap();
                    std::fs::set_permissions(
                        &plugins,
                        std::fs::Permissions::from_mode(if case == "writable" {
                            0o777
                        } else {
                            0o755
                        }),
                    )
                    .unwrap();
                }
            }
            assert_eq!(
                prepare_codex_profile(&root(&temp)).is_ok(),
                case == "directory",
                "{case}"
            );
            // Full runtime admission first uses the profile binding's metadata
            // checks; its recursive scan then rejects nested shared/link files.
            if case == "directory" {
                verify_runtime_files(&home).unwrap();
            }
            if case == "symlink" {
                assert!(verify_runtime_files(&home).is_err());
            }
            assert!(std::fs::symlink_metadata(&plugins).is_ok());
            assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), CONFIG);
        }
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
    fn equivalent_config_and_project_trust_are_preserved_with_invocation_isolation() {
        let temp = tempfile::tempdir().unwrap();
        let profile = prepare_codex_profile(&root(&temp)).unwrap();
        let equivalent = b"# changed formatting\ncli_auth_credentials_store='file'\n";
        profile.write("config.toml", equivalent).unwrap();
        prepare_codex_profile(&root(&temp)).unwrap();
        assert_eq!(profile.read("config.toml").unwrap(), equivalent);
        let trusted = b"cli_auth_credentials_store='file'\n[projects.'/snapshot']\ntrust_level='trusted'\n";
        profile.write("config.toml", trusted).unwrap();
        prepare_codex_profile(&root(&temp)).unwrap();
        let home = temp.path().join(CODEX_PROFILE_DIRECTORY);
        let argument = codex_isolation_argument(&home, &home).unwrap();
        assert!(argument.to_str().unwrap().contains("untrusted"));
        let parsed = read_profile_config(&profile).unwrap();
        let projects = parsed.isolated_projects(&[&home]).unwrap();
        assert_eq!(projects["/snapshot"]["trust_level"], "untrusted");
        assert_eq!(profile.read("config.toml").unwrap(), trusted);
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
    fn model_catalog_cache_requires_an_unshared_nonwritable_regular_file() {
        for case in ["regular", "symlink", "hardlink", "directory", "writable"] {
            let temp = tempfile::tempdir().unwrap();
            prepare_codex_profile(&root(&temp)).unwrap();
            let cache = temp
                .path()
                .join(CODEX_PROFILE_DIRECTORY)
                .join("models_cache.json");
            let source = temp.path().join("synthetic-model-cache");
            std::fs::write(&source, b"synthetic cache").unwrap();
            match case {
                "symlink" => symlink(&source, &cache).unwrap(),
                "hardlink" => std::fs::hard_link(&source, &cache).unwrap(),
                "directory" => std::fs::create_dir(&cache).unwrap(),
                _ => {
                    std::fs::copy(&source, &cache).unwrap();
                    std::fs::set_permissions(
                        &cache,
                        std::fs::Permissions::from_mode(if case == "writable" {
                            0o666
                        } else {
                            0o644
                        }),
                    )
                    .unwrap();
                }
            }
            assert_eq!(
                prepare_codex_profile(&root(&temp)).is_ok(),
                case == "regular",
                "{case}"
            );
            assert_eq!(std::fs::read(&source).unwrap(), b"synthetic cache");
        }
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
    fn accepts_new_vendor_names_but_rejects_instruction_files_without_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let profile = prepare_codex_profile(&root(&temp)).unwrap();
        profile.write("future-vendor-cache", b"synthetic cache").unwrap();
        profile.create_dir("future-vendor-directory").unwrap();
        prepare_codex_profile(&root(&temp)).unwrap();
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
