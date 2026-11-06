//! Detect stream layouts the current before-image/staged writer cannot preserve.
//! This is a read-only admission check, not a named-stream copy implementation.
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_stream_info
use super::ProjectFailure;
use std::{ffi::c_void, os::windows::io::AsRawHandle};

#[link(name = "kernel32")]
extern "system" {
    fn SetLastError(code: u32);
    fn GetFileInformationByHandleEx(
        handle: *mut c_void,
        class: i32,
        info: *mut c_void,
        bytes: u32,
    ) -> i32;
}
fn unsupported() -> ProjectFailure {
    ProjectFailure::dirty(
        "Windows file stream layout cannot be preserved by this application writer",
    )
}

// FILE_STREAM_INFO has a 24-byte fixed prefix and a counted UTF-16 name.
// No untrusted offset traversal is needed: the supported layout has exactly one
// unnamed data stream. Anything larger or containing another stream is refused.
fn default_stream(bytes: &[u8], expected_length: u64) -> bool {
    let Some(header) = bytes.get(..24) else {
        return false;
    };
    let next = u32::from_le_bytes(header[..4].try_into().unwrap());
    let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let size = i64::from_le_bytes(header[8..16].try_into().unwrap());
    let allocated = i64::from_le_bytes(header[16..24].try_into().unwrap());
    let expected: Vec<u8> = "::$DATA"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    next == 0
        && length == expected.len()
        && size >= 0
        && size as u64 == expected_length
        && allocated >= 0
        && bytes.get(24..24 + length) == Some(expected.as_slice())
}

pub(super) fn require_supported<T: AsRawHandle>(
    held: &T,
    directory: bool,
    length: u64,
) -> Result<(), ProjectFailure> {
    // Fixed allocation is large enough for the sole supported unnamed stream;
    // MORE_DATA is not interpreted as proof that trailing streams are absent.
    // u64 storage provides FILE_STREAM_INFO's required 8-byte alignment.
    let mut aligned = [0u64; 16];
    let ok = unsafe {
        SetLastError(0);
        GetFileInformationByHandleEx(
            held.as_raw_handle(),
            7,
            aligned.as_mut_ptr().cast(),
            std::mem::size_of_val(&aligned) as u32,
        )
    };
    let error = std::io::Error::last_os_error().raw_os_error();
    // FileStreamInfo documents ERROR_HANDLE_EOF when no streams are returned,
    // including its success/no-stream case. Clear last-error before the call so
    // an unrelated earlier EOF cannot suppress an actual returned stream list.
    if error == Some(38) && (directory || length == 0) {
        return Ok(());
    }
    if ok == 0 {
        return Err(unsupported());
    }
    // SAFETY: initialized fixed buffer, read only as bytes for its exact size.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            aligned.as_ptr().cast::<u8>(),
            std::mem::size_of_val(&aligned),
        )
    };
    if !directory && default_stream(bytes, length) {
        Ok(())
    } else {
        Err(unsupported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stream(name: &str, next: u32, size: i64) -> Vec<u8> {
        let name: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut result = Vec::new();
        result.extend(next.to_le_bytes());
        result.extend((name.len() as u32).to_le_bytes());
        result.extend(size.to_le_bytes());
        result.extend(4096i64.to_le_bytes());
        result.extend(name);
        result
    }
    #[test]
    fn default_stream_parser_never_ignores_additional_or_malformed_records() {
        assert!(default_stream(&stream("::$DATA", 0, 7), 7));
        assert!(!default_stream(&stream("::$DATA", 40, 7), 7));
        assert!(!default_stream(&stream(":Zone.Identifier:$DATA", 0, 7), 7));
        assert!(!default_stream(&stream("::$DATA", 0, -1), 7));
        assert!(!default_stream(&stream("::$DATA", 0, 7), 8));
        assert!(!default_stream(&stream("::$DATA", 0, 7)[..25], 7));
    }
    #[test]
    fn named_stream_is_rejected_without_modifying_any_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source.txt");
        std::fs::write(&path, b"source").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        require_supported(&file, false, 6).unwrap();
        let alternate = temp.path().join("source.txt:extra");
        std::fs::write(&alternate, b"preserve me").unwrap();
        assert!(require_supported(&file, false, 6).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"source");
        assert_eq!(std::fs::read(&alternate).unwrap(), b"preserve me");
    }
}
