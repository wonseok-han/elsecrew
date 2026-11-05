//! Authenticode validation of a held image, not Git publisher admission.
//! https://learn.microsoft.com/en-us/windows/win32/api/wintrust/nf-wintrust-winverifytrust
//! https://learn.microsoft.com/en-us/windows/win32/api/wintrust/ns-wintrust-wintrust_data
use super::ProjectFailure;
use std::{ffi::c_void, os::windows::io::AsRawHandle, ptr};

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}
#[repr(C)]
struct FileInfo {
    size: u32,
    path: *const u16,
    file: *mut c_void,
    subject: *mut Guid,
}
#[repr(C)]
struct TrustData {
    size: u32,
    policy_callback: *mut c_void,
    sip_client: *mut c_void,
    ui: u32,
    revocation: u32,
    choice: u32,
    file: *mut FileInfo,
    action: u32,
    state: *mut c_void,
    url: *mut u16,
    flags: u32,
    context: u32,
    signature_settings: *mut c_void,
}
#[link(name = "wintrust")]
extern "system" {
    fn WinVerifyTrust(window: *mut c_void, action: *mut Guid, data: *mut TrustData) -> i32;
}
fn action() -> Guid {
    // WINTRUST_ACTION_GENERIC_VERIFY_V2 (softpub.h).
    Guid {
        data1: 0x00aac56b,
        data2: 0xcd44,
        data3: 0x11d0,
        data4: [0x8c, 0xc2, 0x00, 0xc0, 0x4f, 0xc2, 0x95, 0xee],
    }
}
struct TrustState(TrustData);
impl Drop for TrustState {
    fn drop(&mut self) {
        self.0.action = 2; // WTD_STATEACTION_CLOSE, including failed verification.
        unsafe {
            WinVerifyTrust(-1isize as *mut c_void, &mut action(), &mut self.0);
        }
    }
}

pub(super) fn verify(file: &impl AsRawHandle, path: &str) -> Result<(), ProjectFailure> {
    if path.contains('\0') {
        return Err(ProjectFailure::manifest(
            "signature image path contains NUL",
        ));
    }
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut info = FileInfo {
        size: std::mem::size_of::<FileInfo>() as u32,
        path: wide.as_ptr(),
        file: file.as_raw_handle(),
        subject: ptr::null_mut(),
    };
    // State is dropped before info/wide and the caller-owned handle. Never
    // load/execute the candidate, display OS dialogs, or fetch URLs here.
    let mut state = TrustState(TrustData {
        size: std::mem::size_of::<TrustData>() as u32,
        policy_callback: ptr::null_mut(),
        sip_client: ptr::null_mut(),
        ui: 2,
        revocation: 1,
        choice: 1,
        file: &mut info,
        action: 1,
        state: ptr::null_mut(),
        url: ptr::null_mut(),
        // WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT.
        // Missing/stale revocation evidence fails; it is not silently bypassed.
        flags: 0x1000 | 0x80,
        context: 0,
        signature_settings: ptr::null_mut(),
    });
    let result = unsafe { WinVerifyTrust(-1isize as *mut c_void, &mut action(), &mut state.0) };
    if result != 0 {
        return Err(ProjectFailure::recovery(format!(
            "selected image signature could not be verified (Windows status {:08x})",
            result as u32
        )));
    }
    Ok(())
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::size_of::<FileInfo>() == 32);
    assert!(std::mem::size_of::<TrustData>() == 88);
    assert!(std::mem::offset_of!(TrustData, file) == 40);
    assert!(std::mem::offset_of!(TrustData, state) == 56);
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsigned_image_is_never_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("git.exe");
        std::fs::write(&path, b"MZ unsigned test fixture").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        assert!(verify(&file, path.to_str().unwrap()).is_err());
    }
}
