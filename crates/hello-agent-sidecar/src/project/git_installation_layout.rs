//! Known Git for Windows layouts, not installation provenance or execution authority.
use super::{protocol_path, ProjectFailure};

pub(super) fn runtime_candidates(selected: &str) -> Result<Vec<String>, ProjectFailure> {
    let path = protocol_path::from_native(selected, true).map_err(ProjectFailure::manifest)?;
    let lower = path.to_ascii_lowercase();
    for flavor in ["ucrt64", "mingw64", "clangarm64", "mingw32"] {
        if lower.ends_with(&format!("/{flavor}/bin/git.exe")) {
            return Ok(vec![path]);
        }
    }
    let root = ["/cmd/git.exe", "/bin/git.exe"]
        .into_iter()
        .find_map(|suffix| {
            lower
                .ends_with(suffix)
                .then(|| &path[..path.len() - suffix.len()])
        })
        .filter(|root| root.len() >= 3)
        .ok_or_else(|| {
            ProjectFailure::manifest("select git.exe from a Git for Windows installation")
        })?;
    Ok(["ucrt64", "mingw64", "clangarm64", "mingw32"]
        .into_iter()
        .map(|flavor| format!("{root}/{flavor}/bin/git.exe"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapper_resolution_stays_in_selected_installation() {
        let candidates = runtime_candidates("C:/My Git/cmd/git.exe").unwrap();
        assert_eq!(candidates.len(), 4);
        assert_eq!(candidates[0], "C:/My Git/ucrt64/bin/git.exe");
        assert!(candidates.iter().all(|path| path.starts_with("C:/My Git/")));
        assert_eq!(
            runtime_candidates("D:/Git/clangarm64/bin/git.exe").unwrap(),
            vec!["D:/Git/clangarm64/bin/git.exe"]
        );
        assert!(runtime_candidates("C:/random/git.exe").is_err());
        assert!(runtime_candidates("C:/Git/../cmd/git.exe").is_err());
    }
}
