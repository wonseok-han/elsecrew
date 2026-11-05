//! Non-mutating CWD lease; product admission remains disabled.
//! A no-delete child keeps each parent nonempty, closing the attribute-only
//! name-surrogate mutation gap that deny-delete directory handles leave open.
//! This is not proof that every third-party/cloud reparse tag is immutable;
//! identity checks reject all tags, and process admission remains unconnected.
//! https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/fsctl-set-reparse-point
#![allow(dead_code)] // Connect together with executable authority, not by raw path.
use super::{protocol_path, windows_fs, ProjectFailure};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{ffi::OsStr, os::windows::fs::OpenOptionsExt as _, sync::Arc};

pub(super) struct DirectoryLease {
    path: String,
    directories: Vec<(Dir, windows_fs::Identity)>,
    names: Vec<String>,
    guard: File,
    guard_name: String,
    guard_id: windows_fs::Identity,
    guard_is_directory: bool,
}

/// Creator-thread resource ownership. Once a process exists, only the proven
/// family barrier can release the final pin. On unconfirmed failure retain pins
/// until sidecar exit; the durable launch fence must prohibit automatic retry.
pub(super) struct RetainedPin<T> {
    pin: Option<Arc<T>>,
    armed: bool,
}
impl<T> RetainedPin<T> {
    pub(super) fn new(pin: T) -> Self {
        Self {
            pin: Some(Arc::new(pin)),
            armed: false,
        }
    }
    pub(super) fn observer(&self) -> Self {
        Self {
            pin: self.pin.clone(),
            armed: false,
        }
    }
    pub(super) fn resource(&self) -> &T {
        self.pin.as_ref().unwrap()
    }
    pub(super) fn arm(&mut self) {
        self.armed = true;
    }
    pub(super) fn confirmed(&mut self) {
        self.armed = false;
    }
}
impl<T> Drop for RetainedPin<T> {
    fn drop(&mut self) {
        if self.armed {
            // Do not close pathname pins while an unconfirmed descendant may
            // still resolve its CWD. This bounded per-failed-launch retention is
            // intentionally not reported as successful cleanup.
            if let Some(pin) = self.pin.take() {
                std::mem::forget(pin);
            }
        }
    }
}

pub(super) type RetainedDirectory = RetainedPin<DirectoryLease>;
impl RetainedDirectory {
    pub(super) fn path(&self) -> &str {
        self.resource().path()
    }
    pub(super) fn validate(&self) -> Result<(), ProjectFailure> {
        self.resource().validate()
    }
}

fn failure(error: std::io::Error) -> ProjectFailure {
    ProjectFailure::recovery(format!("could not retain Windows directory lease: {error}"))
}
fn normal_component(value: &str) -> Result<(), ProjectFailure> {
    if value.is_empty() || value.contains(['/', '\\']) {
        return Err(ProjectFailure::symlink(
            "lease requires a direct normal child",
        ));
    }
    // Reuse the protocol's Windows alias/ADS/device-name rejection without
    // treating this synthetic lexical path as filesystem authority.
    protocol_path::from_native(&format!("C:/{value}"), true).map_err(ProjectFailure::manifest)?;
    if matches!(value, "." | "..") {
        return Err(ProjectFailure::symlink(
            "lease child cannot traverse parents",
        ));
    }
    Ok(())
}
fn pin_child(parent: &Dir, name: &str) -> Result<File, ProjectFailure> {
    normal_component(name)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .maybe_dir(true)
        .share_mode(3) // FILE_SHARE_READ | FILE_SHARE_WRITE; NEVER DELETE.
        .custom_flags(windows_fs::OPEN_REPARSE_POINT);
    parent.open_with(name, &options).map_err(failure)
}
impl DirectoryLease {
    /// `guard_name` is an existing direct child, e.g. `.git`. This function never
    /// creates a guard or writes to the source. Empty/unavailable directories
    /// remain unsupported until the caller supplies a managed preparation step.
    pub(super) fn acquire(authority: &Dir, guard_name: &OsStr) -> Result<Self, ProjectFailure> {
        let expected = windows_fs::identity(authority, true)?;
        let path = windows_fs::local_guid_path(authority)?;
        let guard_name = guard_name
            .to_str()
            .ok_or_else(|| ProjectFailure::manifest("lease child is not Unicode"))?;
        normal_component(guard_name)?;
        let boundary = path
            .find("}\\")
            .ok_or_else(|| ProjectFailure::manifest("lease has no local GUID root"))?
            + 2;
        let root = &path[..boundary];
        let tail = &path[boundary..];
        let names: Vec<String> = if tail.is_empty() {
            Vec::new()
        } else {
            tail.split('\\').map(str::to_owned).collect()
        };
        if names.len() > 256 {
            return Err(ProjectFailure::manifest(
                "Windows lease path depth exceeds bounds",
            ));
        }
        for name in &names {
            normal_component(name)?;
        }
        // Only the GUID volume root is opened ambiently. Every child lookup is
        // relative to the previously held directory, never the user's old path.
        let root_file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x02000000 | windows_fs::OPEN_REPARSE_POINT)
            .open(root)
            .map_err(failure)?;
        let root_dir = Dir::from_std_file(root_file);
        windows_fs::require_local_volume(&root_dir)?;
        let root_id = windows_fs::identity(&root_dir, true)?;
        let mut directories = vec![(root_dir, root_id)];
        for name in &names {
            let file = pin_child(&directories.last().unwrap().0, name)?;
            let id = windows_fs::identity(&file, true)?;
            // Transfer the exact pin; do not reopen with different share flags.
            directories.push((Dir::from_std_file(file.into_std()), id));
        }
        let final_dir = &directories.last().unwrap().0;
        if windows_fs::identity(final_dir, true)? != expected {
            return Err(ProjectFailure::symlink(
                "lease path differs from held directory authority",
            ));
        }
        let guard = pin_child(final_dir, guard_name)?;
        let guard_is_directory = guard.metadata().map_err(failure)?.is_dir();
        let guard_id = windows_fs::identity(&guard, guard_is_directory)?;
        let lease = Self {
            path,
            directories,
            names,
            guard,
            guard_name: guard_name.to_owned(),
            guard_id,
            guard_is_directory,
        };
        // All nonempty pins now exist. Check every earlier open for a reparse
        // mutation during construction, then rebind every parent/child edge.
        lease.validate()?;
        Ok(lease)
    }
    /// Caller must retain Self through supervisor-confirmed process-family exit.
    pub(super) fn validate(&self) -> Result<(), ProjectFailure> {
        for (directory, id) in &self.directories {
            if windows_fs::identity(directory, true)? != *id {
                return Err(ProjectFailure::symlink("leased directory changed identity"));
            }
        }
        for (index, name) in self.names.iter().enumerate() {
            let child = pin_child(&self.directories[index].0, name)?;
            if windows_fs::identity(&child, true)? != self.directories[index + 1].1 {
                return Err(ProjectFailure::symlink(
                    "leased parent/child binding changed",
                ));
            }
        }
        let final_dir = &self.directories.last().unwrap().0;
        let named_guard = pin_child(final_dir, &self.guard_name)?;
        if windows_fs::identity(&self.guard, self.guard_is_directory)? != self.guard_id
            || windows_fs::identity(&named_guard, self.guard_is_directory)? != self.guard_id
            || windows_fs::local_guid_path(final_dir)? != self.path
        {
            return Err(ProjectFailure::symlink("leased guard or GUID path changed"));
        }
        Ok(())
    }
    pub(super) fn path(&self) -> &str {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::c_void, os::windows::io::AsRawHandle, path::Path};
    fn authority(path: &Path) -> Dir {
        Dir::from_std_file(
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(7)
                .custom_flags(0x02000000 | windows_fs::OPEN_REPARSE_POINT)
                .open(path)
                .unwrap(),
        )
    }
    fn junction_on_existing(path: &Path, target: &Path) -> std::io::Result<()> {
        #[link(name = "kernel32")]
        extern "system" {
            fn DeviceIoControl(
                handle: *mut c_void,
                code: u32,
                input: *const c_void,
                input_size: u32,
                output: *mut c_void,
                output_size: u32,
                returned: *mut u32,
                overlapped: *mut c_void,
            ) -> i32;
        }
        let canonical = target.canonicalize()?;
        let target = canonical.to_str().unwrap().strip_prefix(r"\\?\").unwrap();
        let substitute: Vec<u16> = format!(r"\??\{target}").encode_utf16().collect();
        let length = u16::try_from(substitute.len() * 2).unwrap();
        let mut data = Vec::new();
        data.extend_from_slice(&0xa0000003u32.to_le_bytes());
        data.extend_from_slice(&(8u16 + length + 4).to_le_bytes());
        for value in [0u16, 0, length, length + 2, 0] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        for value in substitute.into_iter().chain([0, 0]) {
            data.extend_from_slice(&value.to_le_bytes());
        }
        // Attribute-only access deliberately exercises the previous gap.
        let file = std::fs::OpenOptions::new()
            .access_mode(0x100)
            .share_mode(7)
            .custom_flags(0x02000000 | windows_fs::OPEN_REPARSE_POINT)
            .open(path)?;
        let mut returned = 0;
        if unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                0x000900a4,
                data.as_ptr().cast(),
                data.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[test]
    fn parent_delete_child_permission_cannot_bypass_guard_pin() {
        use std::os::windows::{
            ffi::OsStrExt,
            io::{FromRawHandle, OwnedHandle},
        };
        #[link(name = "advapi32")]
        extern "system" {
            fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
            fn GetTokenInformation(
                token: *mut c_void,
                class: i32,
                data: *mut c_void,
                size: u32,
                needed: *mut u32,
            ) -> i32;
            fn ConvertSidToStringSidW(sid: *const c_void, text: *mut *mut u16) -> i32;
            fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text: *const u16,
                revision: u32,
                descriptor: *mut *mut c_void,
                size: *mut u32,
            ) -> i32;
            fn SetFileSecurityW(
                path: *const u16,
                information: u32,
                descriptor: *const c_void,
            ) -> i32;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn LocalFree(memory: *mut c_void) -> *mut c_void;
        }
        struct Local(*mut c_void);
        impl Drop for Local {
            fn drop(&mut self) {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
        let mut raw = std::ptr::null_mut();
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut raw) },
            0
        );
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut size = 0;
        assert_eq!(
            unsafe {
                GetTokenInformation(token.as_raw_handle(), 1, std::ptr::null_mut(), 0, &mut size)
            },
            0
        );
        assert!((8..65536).contains(&size));
        let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    1,
                    data.as_mut_ptr().cast(),
                    size,
                    &mut size,
                )
            },
            0
        );
        // TOKEN_USER begins with SID_AND_ATTRIBUTES, whose first field is PSID.
        let sid = unsafe { *(data.as_ptr().cast::<*const c_void>()) };
        let mut raw_text = std::ptr::null_mut();
        assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut raw_text) }, 0);
        let _text_allocation = Local(raw_text.cast());
        let mut length = 0;
        while unsafe { *raw_text.add(length) } != 0 {
            length += 1;
            assert!(length < 256);
        }
        let sid =
            String::from_utf16(unsafe { std::slice::from_raw_parts(raw_text, length) }).unwrap();
        let set_acl = |path: &Path, sddl: &str| {
            let text: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
            let mut raw = std::ptr::null_mut();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        text.as_ptr(),
                        1,
                        &mut raw,
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            let descriptor = Local(raw);
            let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
            assert_ne!(
                unsafe { SetFileSecurityW(path.as_ptr(), 0x80000004, descriptor.0) },
                0,
                "{}",
                std::io::Error::last_os_error()
            ); // protected DACL only
        };
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let guard = parent.join("guard");
        let control = parent.join("control");
        std::fs::write(&guard, b"guard").unwrap();
        std::fs::write(&control, b"control").unwrap();
        let child_acl = format!("D:P(D;;SD;;;{sid})(A;;FA;;;{sid})");
        set_acl(&guard, &child_acl);
        set_acl(&control, &child_acl);
        // Child DELETE denied, parent FILE_DELETE_CHILD absent: deletion fails.
        set_acl(&parent, &format!("D:P(A;;0x001f01bf;;;{sid})"));
        let denied = std::fs::remove_file(&control);
        // Restore parent full access before assertions so temp cleanup remains
        // possible if the negative control unexpectedly fails.
        set_acl(&parent, &format!("D:P(A;;FA;;;{sid})"));
        assert_eq!(denied.unwrap_err().raw_os_error(), Some(5));
        // Same child ACL, parent now grants FILE_DELETE_CHILD: deletion works.
        std::fs::remove_file(&control).unwrap();
        let dir = authority(&parent);
        let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        assert_eq!(
            std::fs::remove_file(&guard).unwrap_err().raw_os_error(),
            Some(32)
        );
        lease.validate().unwrap();
        drop(lease);
        std::fs::remove_file(&guard).unwrap();
    }

    #[test]
    fn missing_guard_and_preexisting_delete_access_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let dir = authority(temp.path());
        assert!(DirectoryLease::acquire(&dir, OsStr::new("guard")).is_err());
        std::fs::write(temp.path().join("guard"), b"pin").unwrap();
        let delete = std::fs::OpenOptions::new()
            .access_mode(0x10000)
            .share_mode(7)
            .open(temp.path().join("guard"))
            .unwrap();
        assert!(DirectoryLease::acquire(&dir, OsStr::new("guard")).is_err());
        drop(delete);
        DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
    }
    #[test]
    fn pins_block_delete_rename_and_attribute_only_reparse() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("one");
        let cwd = parent.join("two");
        let outside = temp.path().join("outside");
        let control = temp.path().join("empty-control");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(&control).unwrap();
        std::fs::write(cwd.join("guard"), b"unchanged").unwrap();
        // Prove that this filesystem/test token can perform the attempted attack.
        junction_on_existing(&control, &outside).unwrap();
        let dir = authority(&cwd);
        let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
        assert_eq!(
            std::fs::remove_file(cwd.join("guard"))
                .unwrap_err()
                .raw_os_error(),
            Some(32)
        );
        assert_eq!(
            std::fs::rename(&parent, temp.path().join("moved"))
                .unwrap_err()
                .raw_os_error(),
            Some(32)
        );
        for component in [&parent, &cwd] {
            assert_eq!(
                junction_on_existing(component, &outside)
                    .unwrap_err()
                    .raw_os_error(),
                Some(145)
            );
        }
        lease.validate().unwrap();
        assert!(lease.path().starts_with(r"\\?\Volume{"));
        assert_eq!(std::fs::read(cwd.join("guard")).unwrap(), b"unchanged");
        drop(lease);
        std::fs::remove_file(cwd.join("guard")).unwrap();
        std::fs::rename(&parent, temp.path().join("moved")).unwrap();
    }
}
