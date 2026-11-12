//! Wire conversion only; this does not attest filesystem identity or authorize IO.
use elsecrew_protocol::project_v1::AbsolutePathScanV1;

pub(super) fn from_native(input: &str, windows: bool) -> Result<String, &'static str> {
    if input.chars().any(char::is_control) {
        return Err("project path contains control characters");
    }
    let value = if windows {
        // fs::canonicalize may return a verbatim drive path. Stripping that
        // prefix is safe only for components with unchanged Win32 semantics.
        // https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file
        let drive = if let Some(verbatim) = input.strip_prefix(r"\\?\") {
            if verbatim.contains('/') {
                return Err("verbatim project path contains a non-native separator");
            }
            verbatim
        } else {
            input
        };
        let bytes = drive.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || !matches!(bytes[2], b'\\' | b'/')
        {
            return Err("project path requires an absolute local drive path");
        }
        let mut normalized = drive.replace('\\', "/");
        normalized.replace_range(0..1, &drive[0..1].to_ascii_uppercase());
        for component in normalized[3..].split('/') {
            if component.ends_with(['.', ' '])
                || component
                    .chars()
                    .any(|c| matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
                || reserved_device(component)
            {
                return Err("project path has an ambiguous Windows component");
            }
        }
        normalized
    } else {
        // A backslash is a real Unix filename character, never a separator.
        // Reject unrepresentable names rather than redirecting to another path.
        if !input.starts_with('/') {
            return Err("project path is not absolute");
        }
        input.to_owned()
    };
    let mut scan = AbsolutePathScanV1::default();
    scan.push(value.as_bytes())
        .map_err(|_| "project path is not canonical")?;
    scan.finish("project path")
        .map_err(|_| "project path is not canonical")?;
    Ok(value)
}

fn reserved_device(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_windows_drive_paths_preserve_components() {
        for (input, expected) in [
            (r"C:\Projects\hello", "C:/Projects/hello"),
            (r"\\?\d:\한글 폴더\hello", "D:/한글 폴더/hello"),
            ("d:/Projects/hello", "D:/Projects/hello"),
            (r"C:\work\COM10.txt", "C:/work/COM10.txt"),
        ] {
            assert_eq!(from_native(input, true).unwrap(), expected);
        }
    }

    #[test]
    fn windows_aliases_devices_and_unsupported_namespaces_are_rejected() {
        for input in [
            r"C:relative",
            r"\rooted",
            r"\\server\share\project",
            r"\\?\UNC\server\share\project",
            r"\\.\C:\project",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\project",
            r"\\?\C:\work\trailing.",
            r"\\?\C:\work\trailing ",
            r"\\?\C:\work/other",
            r"C:\work\..\other",
            r"C:\work\.\other",
            r"C:\work\\other",
            r"C:\work\file:stream",
            r"C:\work\NUL.txt",
            r"C:\work\com¹.txt",
            r"C:\work\LPT9",
            r"C:\work\CONIN$",
            r"C:\work\CON .txt",
            r"C:\work\wild*",
            "C:/",
        ] {
            assert!(from_native(input, true).is_err(), "{input}");
        }
    }

    #[test]
    fn unix_names_are_not_reinterpreted_as_windows_paths() {
        assert_eq!(
            from_native("/tmp/한글 folder", false).unwrap(),
            "/tmp/한글 folder"
        );
        assert!(from_native(r"/tmp/real\name", false).is_err());
        assert!(from_native("C:/relative-on-unix", false).is_err());
        assert!(from_native("/tmp/../other", false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn existing_backslash_directory_cannot_be_rebound_to_nested_directory() {
        let root = tempfile::tempdir().unwrap();
        let literal = root.path().join(r"real\name");
        let nested = root.path().join("real/name");
        std::fs::create_dir(&literal).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(literal.join("marker"), b"literal").unwrap();
        std::fs::write(nested.join("marker"), b"nested").unwrap();
        assert!(super::super::canonical_protocol_path(&literal).is_err());
        assert!(super::super::canonical_protocol_path(&nested).is_ok());
        assert_eq!(std::fs::read(literal.join("marker")).unwrap(), b"literal");
        assert_eq!(std::fs::read(nested.join("marker")).unwrap(), b"nested");
    }
}
