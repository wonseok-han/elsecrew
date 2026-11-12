//! Bind an explicitly selected image to the debugger's actual image handle.
//! This is file identity, NOT publisher/installation/DLL trust or admission.
#![allow(dead_code)]
use super::{
    windows_directory_lease::{DirectoryLease, RetainedPin},
    windows_fs, ProjectFailure,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{ffi::OsStr, os::windows::io::AsRawHandle};

pub(super) struct ExecutableLease {
    parent: DirectoryLease,
    image: File,
    identity: windows_fs::Identity,
    path: String,
    companions: Vec<ExecutableLease>,
    system_modules: Option<Box<super::windows_system_modules::SystemModules>>,
}
pub(super) type RetainedExecutable = RetainedPin<ExecutableLease>;
impl ExecutableLease {
    /// Caller supplies selected parent authority, never PATH or a repository
    /// executable name. Trust origin must be established separately before use.
    pub(super) fn acquire(parent: &Dir, name: &OsStr) -> Result<Self, ProjectFailure> {
        let pinned_parent = DirectoryLease::acquire(parent, name)?;
        let name = name
            .to_str()
            .ok_or_else(|| ProjectFailure::manifest("image name is not Unicode"))?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .share_mode(1)
            .custom_flags(windows_fs::OPEN_REPARSE_POINT);
        let image = parent.open_with(name, &options).map_err(|error| {
            ProjectFailure::recovery(format!("could not retain selected image: {error}"))
        })?;
        let identity = windows_fs::identity(&image, false)?;
        let path = format!(
            "{}{}{}",
            pinned_parent.path(),
            if pinned_parent.path().ends_with('\\') {
                ""
            } else {
                "\\"
            },
            name
        );
        let lease = Self {
            parent: pinned_parent,
            image,
            identity,
            path,
            companions: Vec::new(),
            system_modules: None,
        };
        lease.validate()?;
        Ok(lease)
    }
    /// OS chain/signature validity only. Does not identify an approved Git publisher.
    pub(super) fn verify_authenticode(
        &self,
    ) -> Result<super::windows_authenticode::VerifiedPublisher, ProjectFailure> {
        self.validate()?;
        let publisher = super::windows_authenticode::verify(&self.image, &self.path)?;
        self.validate()?;
        Ok(publisher)
    }
    /// Attach inspected files to the same process-family retention owner.
    /// No trust is conferred by attachment. A flat bounded set avoids unbounded
    /// validation recursion and releases only with the existing family barrier.
    pub(super) fn retain_companions(
        mut self,
        companions: Vec<Self>,
    ) -> Result<Self, ProjectFailure> {
        if !self.companions.is_empty()
            || companions.len() > 257
            || companions
                .iter()
                .any(|image| !image.companions.is_empty() || image.system_modules.is_some())
        {
            return Err(ProjectFailure::manifest(
                "invalid executable companion inventory",
            ));
        }
        self.companions = companions;
        self.validate()?;
        Ok(self)
    }
    pub(super) fn inspect_dependency_loads(mut self) -> Result<Self, ProjectFailure> {
        if self.system_modules.is_some() {
            return Err(ProjectFailure::manifest(
                "DLL observation is already configured",
            ));
        }
        self.system_modules = Some(Box::new(
            super::windows_system_modules::SystemModules::acquire()?,
        ));
        self.validate()?;
        Ok(self)
    }
    pub(super) fn matches_file(&self, actual: &impl AsRawHandle) -> Result<bool, ProjectFailure> {
        Ok(windows_fs::identity(actual, false)? == self.identity)
    }
    pub(super) fn observes_dependencies(&self) -> bool {
        self.system_modules.is_some()
    }
    pub(super) fn verify_loaded_dependency(
        &self,
        actual: &impl AsRawHandle,
    ) -> Result<(), ProjectFailure> {
        self.validate()?;
        // The debug event retains this exact loaded-file handle for the call.
        // Observe it once, rather than issuing the same identity query for
        // every entry in the bounded companion inventory. All lease validation
        // and the system-module admission path remain unchanged.
        let actual_identity = windows_fs::identity(actual, false)?;
        if actual_identity == self.identity {
            return Ok(());
        }
        for image in &self.companions {
            if actual_identity == image.identity {
                return Ok(());
            }
        }
        self.system_modules
            .as_ref()
            .ok_or_else(|| ProjectFailure::recovery("DLL observation is not configured"))?
            .retain_loaded(actual)
    }
    pub(super) fn verify_loaded_descendant(
        &self,
        actual: &impl AsRawHandle,
    ) -> Result<(), ProjectFailure> {
        self.validate()?;
        if self.matches_file(actual)? {
            return Ok(());
        }
        self.system_modules
            .as_ref()
            .ok_or_else(|| ProjectFailure::recovery("descendant observation is not configured"))?
            .verify_console_host(actual)
    }
    pub(super) fn path(&self) -> &str {
        &self.path
    }
    /// CreateProcess spelling only. Authority stays with the retained GUID
    /// parent/file handles and the pre-execution debug-event identity check.
    pub(super) fn launch_path(&self) -> Result<String, ProjectFailure> {
        self.validate()?;
        local_image_launch_path(&self.image)
    }
    pub(super) fn validate(&self) -> Result<(), ProjectFailure> {
        self.parent.validate()?;
        if windows_fs::identity(&self.image, false)? != self.identity {
            return Err(ProjectFailure::symlink(
                "selected executable identity changed",
            ));
        }
        for image in &self.companions {
            image.validate()?;
        }
        if let Some(system) = &self.system_modules {
            system.validate()?;
        }
        Ok(())
    }
    pub(super) fn verify_loaded_image<T: AsRawHandle>(
        &self,
        actual: &T,
    ) -> Result<(), ProjectFailure> {
        self.validate()?;
        if windows_fs::identity(actual, false)? != self.identity {
            return Err(ProjectFailure::symlink(
                "loaded image differs from selected executable",
            ));
        }
        Ok(())
    }
}

// Windows accepts some installed images via a DOS final name but rejects their
// volume-GUID application spelling. Never accept a caller-provided substitute.
fn local_image_launch_path(image: &File) -> Result<String, ProjectFailure> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFinalPathNameByHandleW(
            file: *mut std::ffi::c_void,
            path: *mut u16,
            capacity: u32,
            flags: u32,
        ) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            image.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    } as usize;
    if count == 0 || count >= buffer.len() {
        return Err(ProjectFailure::recovery(
            "held image launch path unavailable",
        ));
    }
    let path = String::from_utf16(&buffer[..count])
        .map_err(|_| ProjectFailure::manifest("image launch path is not Unicode"))?;
    let bytes = path.as_bytes();
    if !path.starts_with(r"\\?\")
        || bytes.len() < 7
        || !bytes[4].is_ascii_alphabetic()
        || bytes[5] != b':'
        || bytes[6] != b'\\'
    {
        return Err(ProjectFailure::manifest(
            "image launch path is not a local drive path",
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    #[test]
    fn launch_and_search_spelling_preserve_guid_identity() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("git.exe"), b"selected image").unwrap();
        let directory = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
        let image = ExecutableLease::acquire(&directory, OsStr::new("git.exe")).unwrap();
        let cwd = DirectoryLease::acquire(&directory, OsStr::new("git.exe")).unwrap();
        assert!(image.path().starts_with(r"\\?\Volume{"));
        assert!(cwd.path().starts_with(r"\\?\Volume{"));
        assert_ne!(image.path(), image.launch_path().unwrap());
        let actual = std::fs::File::open(image.launch_path().unwrap()).unwrap();
        image.verify_loaded_image(&actual).unwrap();
        let environment = super::super::windows_git_environment::for_runtime(&image, &cwd).unwrap();
        assert_eq!(
            environment["PATH"],
            image
                .launch_path()
                .unwrap()
                .strip_prefix(r"\\?\")
                .unwrap()
                .rsplit_once('\\')
                .unwrap()
                .0
        );
        assert_eq!(environment["GIT_EXEC_PATH"], environment["PATH"]);
        let searched = std::fs::File::open(format!("{}\\git.exe", environment["PATH"])).unwrap();
        image.verify_loaded_image(&searched).unwrap();
        image.validate().unwrap();
    }
    #[test]
    fn process_identity_accepts_runtime_hardlink_but_not_other_companion() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("git.exe"), b"runtime").unwrap();
        std::fs::hard_link(
            temp.path().join("git.exe"),
            temp.path().join("git-helper.exe"),
        )
        .unwrap();
        std::fs::write(temp.path().join("other.exe"), b"other").unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let runtime = ExecutableLease::acquire(&dir, OsStr::new("git.exe")).unwrap();
        let other = ExecutableLease::acquire(&dir, OsStr::new("other.exe")).unwrap();
        let runtime = runtime.retain_companions(vec![other]).unwrap();
        let hardlink = std::fs::File::open(temp.path().join("git-helper.exe")).unwrap();
        runtime.verify_loaded_image(&hardlink).unwrap();
        let other = std::fs::File::open(temp.path().join("other.exe")).unwrap();
        assert!(runtime.verify_loaded_image(&other).is_err());
    }

    #[test]
    fn observed_dependencies_require_inventory_or_system_identity() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["git.exe", "known.dll", "added.dll"] {
            std::fs::write(temp.path().join(name), name.as_bytes()).unwrap();
        }
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let root = ExecutableLease::acquire(&dir, OsStr::new("git.exe")).unwrap();
        let known = ExecutableLease::acquire(&dir, OsStr::new("known.dll")).unwrap();
        let root = root
            .retain_companions(vec![known])
            .unwrap()
            .inspect_dependency_loads()
            .unwrap();
        let actual = std::fs::File::open(temp.path().join("known.dll")).unwrap();
        root.verify_loaded_dependency(&actual).unwrap();
        let added = std::fs::File::open(temp.path().join("added.dll")).unwrap();
        assert!(root.verify_loaded_dependency(&added).is_err());
    }

    #[test]
    fn companion_write_lock_follows_retained_runtime_lifetime() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("git.exe"), b"root").unwrap();
        std::fs::write(temp.path().join("dependency.dll"), b"dependency").unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let root = ExecutableLease::acquire(&dir, OsStr::new("git.exe")).unwrap();
        let dependency = ExecutableLease::acquire(&dir, OsStr::new("dependency.dll")).unwrap();
        let root = root.retain_companions(vec![dependency]).unwrap();
        let retained = RetainedExecutable::new(root);
        let observer = retained.observer();
        drop(retained);
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("dependency.dll"))
            .is_err());
        observer.resource().validate().unwrap();
        drop(observer);
        std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("dependency.dll"))
            .unwrap();
    }

    #[test]
    fn image_binding_rejects_different_file_and_conflicting_write_access() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("selected.exe"), b"selected").unwrap();
        std::fs::write(temp.path().join("different.exe"), b"different").unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("selected.exe"))
            .unwrap();
        assert!(ExecutableLease::acquire(&dir, OsStr::new("selected.exe")).is_err());
        drop(writer);
        let lease = ExecutableLease::acquire(&dir, OsStr::new("selected.exe")).unwrap();
        let same = std::fs::File::open(temp.path().join("selected.exe")).unwrap();
        let other = std::fs::File::open(temp.path().join("different.exe")).unwrap();
        lease.verify_loaded_image(&same).unwrap();
        assert!(lease.verify_loaded_image(&other).is_err());
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("selected.exe"))
            .is_err());
        assert!(std::fs::rename(
            temp.path().join("selected.exe"),
            temp.path().join("moved.exe")
        )
        .is_err());
        drop(lease);
        std::fs::OpenOptions::new()
            .write(true)
            .open(temp.path().join("selected.exe"))
            .unwrap();
    }
}
