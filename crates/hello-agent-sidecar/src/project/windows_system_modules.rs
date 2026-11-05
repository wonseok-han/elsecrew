//! Native system-directory module class. This does not authorize arbitrary
//! external DLLs or attest a compromised Windows installation.
use super::{windows_executable::ExecutableLease, windows_fs, ProjectFailure};
use cap_std::fs::Dir;
use std::{ffi::OsStr, os::windows::io::AsRawHandle, path::Path, sync::Mutex};

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
}

pub(super) struct SystemModules {
    directory: Dir,
    guard: ExecutableLease,
    images: Mutex<Vec<ExecutableLease>>,
}
impl SystemModules {
    pub(super) fn acquire() -> Result<Self, ProjectFailure> {
        let mut buffer = vec![0u16; 32_768];
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 || length as usize >= buffer.len() {
            return Err(ProjectFailure::recovery(
                "Windows system directory is unavailable",
            ));
        }
        let path = String::from_utf16(&buffer[..length as usize])
            .map_err(|_| ProjectFailure::manifest("Windows system directory is not Unicode"))?;
        let directory = windows_fs::open_directory(Path::new(&path))?;
        // Keep the directory and ancestry nonempty, not just a path prefix.
        let guard = ExecutableLease::acquire(&directory, OsStr::new("kernel32.dll"))?;
        Ok(Self {
            directory,
            guard,
            images: Mutex::new(Vec::new()),
        })
    }
    pub(super) fn validate(&self) -> Result<(), ProjectFailure> {
        self.guard.validate()?;
        let images = self
            .images
            .lock()
            .map_err(|_| ProjectFailure::recovery("Windows module inventory is poisoned"))?;
        for image in images.iter() {
            image.validate()?;
        }
        Ok(())
    }
    pub(super) fn retain_loaded(&self, actual: &impl AsRawHandle) -> Result<(), ProjectFailure> {
        self.guard.validate()?;
        let path = windows_fs::local_guid_path(actual)?;
        let (parent, leaf) = path
            .rsplit_once('\\')
            .ok_or_else(|| ProjectFailure::manifest("loaded module has no parent"))?;
        let expected = windows_fs::local_guid_path(&self.directory)?;
        if !parent.eq_ignore_ascii_case(expected.trim_end_matches('\\'))
            || !leaf.to_ascii_lowercase().ends_with(".dll")
        {
            return Err(ProjectFailure::recovery(
                "loaded DLL is outside the inspected installation and Windows system directory",
            ));
        }
        let mut images = self
            .images
            .lock()
            .map_err(|_| ProjectFailure::recovery("Windows module inventory is poisoned"))?;
        for image in images.iter() {
            if image.matches_file(actual)? {
                return image.validate();
            }
        }
        if images.len() >= 256 {
            return Err(ProjectFailure::manifest(
                "Windows system module inventory exceeds limit",
            ));
        }
        let image = ExecutableLease::acquire(&self.directory, OsStr::new(leaf))?;
        image.verify_loaded_image(actual)?;
        // Retained by the same armed family owner as the runtime. Never release
        // on UNLOAD_DLL: another family member may still have the image loaded.
        images.push(image);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_class_rejects_same_named_file_outside_system_directory() {
        let modules = SystemModules::acquire().unwrap();
        let kernel = modules.directory.open("kernel32.dll").unwrap();
        modules.retain_loaded(&kernel).unwrap();
        modules.validate().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("kernel32.dll");
        std::fs::write(&path, b"not a Windows module").unwrap();
        let fake = std::fs::File::open(path).unwrap();
        assert!(modules.retain_loaded(&fake).is_err());
    }
}
