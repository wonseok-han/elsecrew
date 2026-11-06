//! File binding for an explicit Git preference. No launch authority is issued:
//! publisher/install provenance, dependent images and updates remain separate.
#![allow(dead_code)]
use super::{
    git_selection, open_real_file_at, windows_executable::ExecutableLease, windows_fs,
    ProjectFailure, ProjectManager,
};
use std::{ffi::OsStr, path::Path};

pub(super) struct SelectedGitCandidate {
    _image: ExecutableLease,
    identity: GitTrustIdentity,
    _selected_path: String,
    _publisher: super::windows_authenticode::VerifiedPublisher,
    _runtime: ExecutableLease,
    _dependencies: DependencyInventory,
}

impl SelectedGitCandidate {
    /// This consumes file bindings only, not an installation admission grant.
    /// The caller still needs publisher/dependency policy before any spawn.
    pub(super) fn into_retained_runtime(self) -> Result<ExecutableLease, ProjectFailure> {
        self._image.validate()?;
        self._dependencies.validate()?;
        let mut companions: Vec<ExecutableLease> = self
            ._dependencies
            .images
            .into_iter()
            .map(|(_, image)| image)
            .collect();
        companions.push(self._image);
        self._runtime
            .retain_companions(companions)?
            .inspect_dependency_loads()
    }
}

impl ProjectManager {
    pub(super) fn selected_git_candidate(
        &self,
    ) -> Result<Option<SelectedGitCandidate>, ProjectFailure> {
        self.verify_managed_paths()?;
        match self.data_dir.symlink_metadata(git_selection::FILE) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ProjectFailure::recovery("Git preference is unavailable")),
            Ok(_) => {}
        }
        let file = open_real_file_at(&self.data_dir, git_selection::FILE)?;
        let identity = windows_fs::identity(&file, false)?;
        let path = git_selection::read(&file)?;
        self.verify_managed_paths()?;
        let current = open_real_file_at(&self.data_dir, git_selection::FILE)?;
        if windows_fs::identity(&current, false)? != identity {
            return Err(ProjectFailure::recovery(
                "Git selection changed during inspection",
            ));
        }
        let Some(path) = path else {
            return Ok(None);
        };
        let parent = Path::new(&path)
            .parent()
            .ok_or_else(|| ProjectFailure::manifest("selected Git has no parent"))?;
        let directory = windows_fs::open_directory(parent)?;
        let name = Path::new(&path).file_name().unwrap_or(OsStr::new(""));
        let image = ExecutableLease::acquire(&directory, name)?;
        let publisher = image.verify_authenticode()?;
        let root_path = super::git_installation_layout::installation_root(&path)?;
        let root = windows_fs::open_directory(Path::new(&root_path))?;
        let identity = GitTrustIdentity {
            installation_path: root_path,
            volume_path: windows_fs::local_guid_path(&root)?,
            directory_id: windows_fs::identity(&root, true)?.recovery_token(),
            common_name: publisher.common_name.clone(),
            organization: publisher.organization.clone(),
        };
        let mut runtime = None;
        for candidate in super::git_installation_layout::runtime_candidates(&path)? {
            // Only absence skips a layout. Access errors/reparse points are not
            // permission to fall through to a different executable.
            match std::fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(ProjectFailure::recovery("Git runtime is unavailable")),
                Ok(_) => {}
            }
            if runtime.is_some() {
                return Err(ProjectFailure::recovery(
                    "multiple Git runtimes found; select the intended runtime git.exe",
                ));
            }
            let parent = Path::new(&candidate)
                .parent()
                .ok_or_else(|| ProjectFailure::manifest("Git runtime has no parent"))?;
            let directory = windows_fs::open_directory(parent)?;
            let actual = ExecutableLease::acquire(&directory, OsStr::new("git.exe"))?;
            let signer = actual.verify_authenticode()?;
            if signer.common_name != publisher.common_name
                || signer.organization != publisher.organization
            {
                return Err(ProjectFailure::recovery(
                    "selected Git and runtime have different verified publishers",
                ));
            }
            let dependencies = DependencyInventory::acquire(directory)?;
            actual.validate()?;
            runtime = Some((actual, dependencies));
        }
        let (runtime, dependencies) = runtime.ok_or_else(|| {
            ProjectFailure::recovery("Git runtime not found in selected installation")
        })?;
        image.validate()?;
        dependencies.validate()?;
        Ok(Some(SelectedGitCandidate {
            _image: image,
            identity,
            _selected_path: path,
            _publisher: publisher,
            _runtime: runtime,
            _dependencies: dependencies,
        }))
    }
}

// Inventory is deliberately not called a trusted dependency closure: dynamic
// imports, system resolution and the installation trust policy remain required.
struct DependencyInventory {
    directory: cap_std::fs::Dir,
    images: Vec<(String, ExecutableLease)>,
}
impl DependencyInventory {
    fn names(directory: &cap_std::fs::Dir) -> Result<Vec<String>, ProjectFailure> {
        let mut names = Vec::new();
        for (index, entry) in directory
            .entries()
            .map_err(|_| ProjectFailure::recovery("Git dependency directory is unavailable"))?
            .enumerate()
        {
            if index >= 4096 {
                return Err(ProjectFailure::manifest(
                    "Git installation directory exceeds inspection limit",
                ));
            }
            let entry = entry
                .map_err(|_| ProjectFailure::recovery("Git dependency entry is unavailable"))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| ProjectFailure::manifest("Git dependency name is not Unicode"))?;
            if name.to_ascii_lowercase().ends_with(".dll") {
                if names.len() >= 256 || name.chars().any(char::is_control) {
                    return Err(ProjectFailure::manifest(
                        "Git dependency inventory exceeds supported limits",
                    ));
                }
                names.push(name.to_owned());
            }
        }
        names.sort_by_key(|name| name.to_ascii_lowercase());
        if names
            .windows(2)
            .any(|pair| pair[0].eq_ignore_ascii_case(&pair[1]))
        {
            return Err(ProjectFailure::manifest(
                "Git dependency names are ambiguous",
            ));
        }
        Ok(names)
    }
    fn acquire(directory: cap_std::fs::Dir) -> Result<Self, ProjectFailure> {
        let images = Self::names(&directory)?
            .into_iter()
            .map(|name| {
                let image = ExecutableLease::acquire(&directory, OsStr::new(&name))?;
                Ok((name, image))
            })
            .collect::<Result<Vec<_>, ProjectFailure>>()?;
        let inventory = Self { directory, images };
        inventory.validate()?;
        Ok(inventory)
    }
    fn validate(&self) -> Result<(), ProjectFailure> {
        let expected: Vec<String> = self.images.iter().map(|(name, _)| name.clone()).collect();
        if Self::names(&self.directory)? != expected {
            return Err(ProjectFailure::recovery(
                "Git dependencies changed during inspection",
            ));
        }
        for (_, image) in &self.images {
            image.validate()?;
        }
        Ok(())
    }
}

// Root identity and verified publisher survive normal file updates. No release
// version, executable digest or signing certificate serial is pinned here.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GitTrustIdentity {
    installation_path: String,
    volume_path: String,
    directory_id: String,
    common_name: String,
    organization: Option<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustRecord {
    schema_version: u32,
    identity: Option<GitTrustIdentity>,
}
const TRUST_FILE: &str = "git-installation-trust-v1.json";
impl ProjectManager {
    fn read_git_trust(&self) -> Result<Option<GitTrustIdentity>, ProjectFailure> {
        use std::io::Read;
        self.verify_managed_paths()?;
        match self.data_dir.symlink_metadata(TRUST_FILE) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ProjectFailure::recovery("Git trust record is unavailable")),
            Ok(_) => {}
        }
        let file = open_real_file_at(&self.data_dir, TRUST_FILE)?;
        let mut bytes = Vec::new();
        file.take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| ProjectFailure::recovery("could not read Git trust record"))?;
        if bytes.len() > 65_536 {
            return Err(ProjectFailure::manifest("Git trust record exceeds limit"));
        }
        let record: TrustRecord = serde_json::from_slice(&bytes)
            .map_err(|_| ProjectFailure::manifest("invalid Git trust record"))?;
        if record.schema_version != 1 {
            return Err(ProjectFailure::manifest(
                "unsupported Git trust record version",
            ));
        }
        Ok(record.identity)
    }
    pub(crate) fn preview_git_trust(&self) -> Result<serde_json::Value, ProjectFailure> {
        let candidate = self
            .selected_git_candidate()?
            .ok_or_else(|| ProjectFailure::recovery("select a Git installation first"))?;
        let trusted = self.read_git_trust()?.as_ref() == Some(&candidate.identity);
        Ok(
            serde_json::json!({"identity": candidate.identity, "trusted": trusted, "executionEnabled": false}),
        )
    }
    pub(crate) fn confirm_git_trust(
        &self,
        expected: GitTrustIdentity,
        confirmed: bool,
    ) -> Result<serde_json::Value, ProjectFailure> {
        if !confirmed {
            return Err(ProjectFailure::manifest(
                "Git installation trust requires explicit confirmation",
            ));
        }
        // Reinspect now, never persist renderer-supplied identity as authority.
        let candidate = self
            .selected_git_candidate()?
            .ok_or_else(|| ProjectFailure::recovery("Git selection is unavailable"))?;
        if candidate.identity != expected {
            return Err(ProjectFailure::dirty(
                "Git installation or verified publisher changed; inspect again",
            ));
        }
        self.verify_managed_paths()?;
        super::write_json_atomic(
            &self.data_dir,
            TRUST_FILE,
            &serde_json::json!(TrustRecord {
                schema_version: 1,
                identity: Some(candidate.identity.clone())
            }),
        )?;
        Ok(
            serde_json::json!({"identity": candidate.identity, "trusted": true, "executionEnabled": false}),
        )
    }
    pub(crate) fn revoke_git_trust(&self) -> Result<(), ProjectFailure> {
        self.verify_managed_paths()?;
        super::write_json_atomic(
            &self.data_dir,
            TRUST_FILE,
            &serde_json::json!(TrustRecord {
                schema_version: 1,
                identity: None
            }),
        )
    }
    pub(super) fn admitted_git_runtime(&self) -> Result<ExecutableLease, ProjectFailure> {
        // Every launch repeats image/installation/identity inspection. A version
        // update with the same root/publisher can pass without a hash whitelist.
        let candidate = self
            .selected_git_candidate()?
            .ok_or_else(|| ProjectFailure::recovery("select and trust a Git installation first"))?;
        if self.read_git_trust()?.as_ref() != Some(&candidate.identity) {
            return Err(ProjectFailure::recovery(
                "Git installation trust needs confirmation",
            ));
        }
        candidate.into_retained_runtime()
    }
}
