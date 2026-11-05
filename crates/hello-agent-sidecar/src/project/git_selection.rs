//! The persisted preference is a candidate, never an executable trust grant.
use super::{protocol_path, ProjectFailure};
use serde::Deserialize;
use std::io::Read;

#[cfg(windows)]
pub(super) const FILE: &str = "git-selection-v1.json";
const MAX_BYTES: u64 = 65_536;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Selection {
    schema_version: u32,
    path: Option<String>,
}

pub(super) fn read(reader: impl Read) -> Result<Option<String>, ProjectFailure> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ProjectFailure::recovery("could not read selected Git preference"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ProjectFailure::manifest(
            "Git preference exceeds size limit",
        ));
    }
    let fields: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ProjectFailure::manifest("invalid Git preference"))?;
    if fields.as_object().map(|v| v.len()) != Some(2) {
        return Err(ProjectFailure::manifest("Git preference fields differ"));
    }
    let selected: Selection = serde_json::from_slice(&bytes)
        .map_err(|_| ProjectFailure::manifest("invalid Git preference fields"))?;
    if selected.schema_version != 1 {
        return Err(ProjectFailure::manifest(
            "unsupported Git preference version",
        ));
    }
    selected
        .path
        .map(|path| {
            if path.encode_utf16().count() > 32_767 {
                return Err(ProjectFailure::manifest("selected Git path is too long"));
            }
            let normalized =
                protocol_path::from_native(&path, true).map_err(ProjectFailure::manifest)?;
            if !normalized
                .rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("git.exe"))
            {
                return Err(ProjectFailure::manifest("selected Git must name git.exe"));
            }
            Ok(normalized)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::read;

    #[test]
    fn accepts_explicit_local_selection_and_forgetting() {
        assert_eq!(
            read(br#"{"schemaVersion":1,"path":null}"#.as_slice()).unwrap(),
            None
        );
        assert_eq!(
            read(br#"{"schemaVersion":1,"path":"C:/Program Files/Git/cmd/git.exe"}"#.as_slice())
                .unwrap(),
            Some("C:/Program Files/Git/cmd/git.exe".into())
        );
    }

    #[test]
    fn rejects_ambiguous_paths_and_malformed_records() {
        for record in [
            r#"{"schemaVersion":1}"#,
            r#"{"schemaVersion":2,"path":null}"#,
            r#"{"schemaVersion":1,"path":null,"extra":true}"#,
            r#"{"schemaVersion":1,"path":null,"path":"C:/git.exe"}"#,
            r#"{"schemaVersion":1,"path":"git.exe"}"#,
            r#"{"schemaVersion":1,"path":"//server/share/git.exe"}"#,
            r#"{"schemaVersion":1,"path":"C:/git.exe:stream"}"#,
            r#"{"schemaVersion":1,"path":"C:/folder/../git.exe"}"#,
            r#"{"schemaVersion":1,"path":"C:/git.cmd"}"#,
        ] {
            assert!(read(record.as_bytes()).is_err(), "{record}");
        }
        assert!(read(vec![b' '; 65_537].as_slice()).is_err());
    }
}
