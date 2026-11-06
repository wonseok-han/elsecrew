//! Construct a fresh environment for internal snapshot/diff Git only.
//! The caller still needs installation consent and product admission.
use super::{
    windows_directory_lease::DirectoryLease, windows_executable::ExecutableLease, ProjectFailure,
};
use std::collections::BTreeMap;

#[link(name = "kernel32")]
extern "system" {
    fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
}

pub(super) fn for_runtime(
    runtime: &ExecutableLease,
    cwd: &DirectoryLease,
) -> Result<BTreeMap<String, String>, ProjectFailure> {
    runtime.validate()?;
    cwd.validate()?;
    let runtime_directory = runtime
        .path()
        .rsplit_once('\\')
        .map(|(parent, _)| parent)
        .ok_or_else(|| ProjectFailure::manifest("Git runtime has no directory"))?;
    // A path-list separator must not introduce another executable search root.
    if runtime_directory.contains(';') {
        return Err(ProjectFailure::manifest(
            "Git installation path contains a PATH separator",
        ));
    }
    let mut buffer = vec![0u16; 32_768];
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err(ProjectFailure::recovery(
            "Windows directory is unavailable for Git environment",
        ));
    }
    let windows = String::from_utf16(&buffer[..length as usize])
        .map_err(|_| ProjectFailure::manifest("Windows directory is not Unicode"))?;
    if windows.chars().any(char::is_control) {
        return Err(ProjectFailure::manifest("invalid Windows directory"));
    }
    // Never copy std::env. In particular inherited GIT_CONFIG_PARAMETERS,
    // GIT_DIR/WORK_TREE, SSH/ASKPASS, HOME and PATH cannot redirect this owner.
    Ok([
        ("SystemRoot", windows.as_str()),
        ("WINDIR", windows.as_str()),
        ("PATH", runtime_directory),
        ("GIT_EXEC_PATH", runtime_directory),
        ("LC_ALL", "C"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", "NUL"),
        ("GIT_CONFIG_COUNT", "0"),
        ("GIT_OPTIONAL_LOCKS", "0"),
        ("GIT_NO_REPLACE_OBJECTS", "1"),
        ("GIT_NO_LAZY_FETCH", "1"),
        ("GIT_ATTR_NOSYSTEM", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_PAGER", ""),
        ("PAGER", ""),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect())
}
