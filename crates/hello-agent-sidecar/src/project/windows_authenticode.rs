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
// Only the documented prefix is inspected. The provider owns all pointers
// until WTD_STATEACTION_CLOSE; no pointer escapes into the returned identity.
#[repr(C)]
struct ProviderCertificatePrefix {
    size: u32,
    certificate: *const c_void,
}
#[derive(Debug)]
#[allow(dead_code)] // Consumed by installation admission in the next implementation wave.
pub(super) struct VerifiedPublisher {
    pub(super) common_name: String,
    pub(super) organization: Option<String>,
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}
#[link(name = "crypt32")]
extern "system" {
    fn CertGetNameStringW(
        certificate: *const c_void,
        kind: u32,
        flags: u32,
        parameter: *const c_void,
        output: *mut u16,
        length: u32,
    ) -> u32;
}
fn signer_failure() -> ProjectFailure {
    ProjectFailure::recovery("verified image signer information is unavailable")
}
impl TrustState {
    fn publisher(&self) -> Result<VerifiedPublisher, ProjectFailure> {
        if self.0.state.is_null() {
            return Err(signer_failure());
        }
        // wintrust is already a static import of this module. Resolve helper
        // exports from that loaded module; never search a selected Git folder.
        let module_name: Vec<u16> = "wintrust.dll".encode_utf16().chain(Some(0)).collect();
        let module = unsafe { GetModuleHandleW(module_name.as_ptr()) };
        if module.is_null() {
            return Err(signer_failure());
        }
        let resolve = |name: &'static [u8]| -> Result<*mut c_void, ProjectFailure> {
            let address = unsafe { GetProcAddress(module, name.as_ptr()) };
            if address.is_null() {
                Err(signer_failure())
            } else {
                Ok(address)
            }
        };
        type Provider = unsafe extern "system" fn(*mut c_void) -> *mut c_void;
        type Signer = unsafe extern "system" fn(*mut c_void, u32, i32, u32) -> *mut c_void;
        type Certificate =
            unsafe extern "system" fn(*mut c_void, u32) -> *const ProviderCertificatePrefix;
        // Export signatures match wintrust.h, checked before invocation.
        let provider: Provider =
            unsafe { std::mem::transmute(resolve(b"WTHelperProvDataFromStateData\0")?) };
        let signer: Signer =
            unsafe { std::mem::transmute(resolve(b"WTHelperGetProvSignerFromChain\0")?) };
        let certificate: Certificate =
            unsafe { std::mem::transmute(resolve(b"WTHelperGetProvCertFromChain\0")?) };
        let data = unsafe { provider(self.0.state) };
        if data.is_null() {
            return Err(signer_failure());
        }
        // First primary signer; never use a timestamp countersigner as publisher.
        let signer = unsafe { signer(data, 0, 0, 0) };
        if signer.is_null() {
            return Err(signer_failure());
        }
        let cert = unsafe { certificate(signer, 0) };
        if cert.is_null() {
            return Err(signer_failure());
        }
        if unsafe { (*cert).size } < std::mem::size_of::<ProviderCertificatePrefix>() as u32 {
            return Err(signer_failure());
        }
        let cert = unsafe { (*cert).certificate };
        if cert.is_null() {
            return Err(signer_failure());
        }
        let common_name = certificate_attribute(cert, b"2.5.4.3\0")?.ok_or_else(signer_failure)?;
        let organization = certificate_attribute(cert, b"2.5.4.10\0")?;
        Ok(VerifiedPublisher {
            common_name,
            organization,
        })
    }
}
fn certificate_attribute(
    certificate: *const c_void,
    oid: &'static [u8],
) -> Result<Option<String>, ProjectFailure> {
    // CERT_NAME_ATTR_TYPE; flags=0 selects the subject, not issuer or display alias.
    let length =
        unsafe { CertGetNameStringW(certificate, 3, 0, oid.as_ptr().cast(), ptr::null_mut(), 0) };
    if length == 0 || length > 4096 {
        return Err(signer_failure());
    }
    let mut buffer = vec![0u16; length as usize];
    let written = unsafe {
        CertGetNameStringW(
            certificate,
            3,
            0,
            oid.as_ptr().cast(),
            buffer.as_mut_ptr(),
            length,
        )
    };
    if written != length || buffer.last() != Some(&0) {
        return Err(signer_failure());
    }
    buffer.pop();
    let value = String::from_utf16(&buffer).map_err(|_| signer_failure())?;
    if value.chars().any(char::is_control) {
        return Err(signer_failure());
    }
    Ok(if value.is_empty() { None } else { Some(value) })
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

pub(super) fn verify(
    file: &impl AsRawHandle,
    path: &str,
) -> Result<VerifiedPublisher, ProjectFailure> {
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
    state.publisher()
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::offset_of!(ProviderCertificatePrefix, certificate) == 8);
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
