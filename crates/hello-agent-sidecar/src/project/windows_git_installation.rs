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
    _selected_path: String,
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
        image.verify_authenticode()?;
        Ok(Some(SelectedGitCandidate {
            _image: image,
            _selected_path: path,
        }))
    }
}
