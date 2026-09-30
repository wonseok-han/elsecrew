//! Index/HEAD semantic comparison only; raw working-tree equivalence is a separate step.
use super::{failure, open, stamp, MAX_ENTRIES, MAX_PATH_BYTES};
use crate::project::{
    parse_tree_record, trim_output, validate_relative_path, validated_oid, ProjectFailure,
    ProjectManager, SourceRepository,
};
use cap_std::fs::MetadataExt;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;

const OUTPUT_LIMIT: usize = 24 * 1024 * 1024;
const INDEX_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::project) struct GitIndexBaseline {
    entries: BTreeMap<String, GitEntry>,
    index_digest: [u8; 32],
    commit: String,
    tree: String,
}

#[derive(Debug, PartialEq, Eq)]
struct GitEntry {
    mode: String,
    oid: String,
}

// Not called by snapshot admission until raw content comparison and durable binding exist.
#[allow(dead_code)]
pub(in crate::project) fn capture_index_baseline(
    manager: &ProjectManager,
    source: &SourceRepository,
) -> Result<GitIndexBaseline, ProjectFailure> {
    validated_oid(&source.base_commit, &source.object_format)?;
    validated_oid(&source.base_tree, &source.object_format)?;
    let before = index_image(source)?;
    let shared = manager.git_at(&source.git_dir, ["rev-parse", "--shared-index-path"])?;
    if !shared.stdout.is_empty() {
        return Err(ProjectFailure::dirty(
            "split index needs a separate shared-index baseline",
        ));
    }
    let index = manager.git_limited_bytes_at(
        &source.git_dir,
        [
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "ls-files",
            "-v",
            "--stage",
            "--sparse",
            "--full-name",
            "-z",
        ],
        &[0],
        OUTPUT_LIMIT,
    )?;
    let tree = manager.git_limited_bytes_at(
        &source.git_dir,
        [
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.allow=never",
            "ls-tree",
            "-rz",
            "--full-tree",
            source.base_tree.as_str(),
        ],
        &[0],
        OUTPUT_LIMIT,
    )?;
    let entries = parse(&index, &source.object_format, true)?;
    if entries != parse(&tree, &source.object_format, false)? {
        return Err(ProjectFailure::dirty(
            "source index differs from the committed tree",
        ));
    }
    // Mode/OID/path alone hides intent-to-add on a committed empty blob. Cached-only
    // plumbing checks that flag without reading working-tree files or invoking converters.
    let difference = manager.git_allow_status_at(
        &source.git_dir,
        [
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.allow=never",
            "diff-index",
            "--cached",
            "--quiet",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--ignore-submodules=none",
            "--ita-invisible-in-index",
            source.base_tree.as_str(),
            "--",
        ],
        &[0, 1],
    )?;
    if !difference.stdout.is_empty() {
        return Err(ProjectFailure::manifest(
            "cached index probe returned unexpected output",
        ));
    }
    if difference.status.code() == Some(1) {
        return Err(ProjectFailure::dirty(
            "source index has pending changes or intent-to-add entries",
        ));
    }
    for (revision, expected) in [
        ("HEAD^{commit}", &source.base_commit),
        ("HEAD^{tree}", &source.base_tree),
    ] {
        let output = manager.git_at(&source.git_dir, ["rev-parse", "--verify", revision])?;
        if trim_output(&output)? != *expected {
            return Err(ProjectFailure::dirty(
                "source HEAD changed during baseline capture",
            ));
        }
    }
    if before != index_image(source)? {
        return Err(ProjectFailure::dirty(
            "source index changed during baseline capture",
        ));
    }
    Ok(GitIndexBaseline {
        entries,
        index_digest: before.1,
        commit: source.base_commit.clone(),
        tree: source.base_tree.clone(),
    })
}

fn index_image(source: &SourceRepository) -> Result<([i128; 9], [u8; 32]), ProjectFailure> {
    let mut file = open(&source.git_dir, "index", false)?;
    let metadata = file.metadata().map_err(|_| failure())?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > INDEX_LIMIT {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(INDEX_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    if bytes.len() as u64 != metadata.len()
        || stamp(&metadata) != stamp(&file.metadata().map_err(|_| failure())?)
        || stamp(&metadata)
            != stamp(
                &source
                    .git_dir
                    .symlink_metadata("index")
                    .map_err(|_| failure())?,
            )
    {
        return Err(failure());
    }
    Ok((stamp(&metadata), Sha256::digest(&bytes).into()))
}

fn parse(
    bytes: &[u8],
    format: &str,
    index: bool,
) -> Result<BTreeMap<String, GitEntry>, ProjectFailure> {
    if bytes.len() > OUTPUT_LIMIT || (!bytes.is_empty() && bytes.last() != Some(&0)) {
        return Err(failure());
    }
    let mut result = BTreeMap::new();
    let mut path_bytes = 0usize;
    if bytes.is_empty() {
        return Ok(result);
    }
    for record in bytes[..bytes.len() - 1].split(|b| *b == 0) {
        let (mode, oid, path) = if index {
            let separator = record
                .iter()
                .position(|b| *b == b'\t')
                .ok_or_else(failure)?;
            let metadata = std::str::from_utf8(&record[..separator]).map_err(|_| failure())?;
            let mut fields = metadata.split(' ');
            if fields.next() != Some("H") {
                return Err(ProjectFailure::dirty(
                    "skip-worktree, assume-unchanged, or non-regular index status is unsupported",
                ));
            }
            let mode = fields.next().ok_or_else(failure)?;
            let oid = fields.next().ok_or_else(failure)?;
            if fields.next() != Some("0") || fields.next().is_some() {
                return Err(ProjectFailure::dirty("unmerged or invalid index entry"));
            }
            (
                mode.to_owned(),
                validated_oid(oid, format)?,
                std::str::from_utf8(&record[separator + 1..])
                    .map_err(|_| failure())?
                    .to_owned(),
            )
        } else {
            let entry = parse_tree_record(record, format)?;
            (entry.mode, entry.oid, entry.path)
        };
        // Gitlinks need their own baseline policy; do not silently treat them as files.
        if !matches!(mode.as_str(), "100644" | "100755" | "120000") {
            return Err(ProjectFailure::dirty(
                "source baseline does not yet support gitlinks or sparse directory entries",
            ));
        }
        validate_relative_path(&path, false)?;
        if path.len() > 4096
            || path
                .split('/')
                .any(|part| part.eq_ignore_ascii_case(".git"))
        {
            return Err(failure());
        }
        path_bytes = path_bytes.checked_add(path.len()).ok_or_else(failure)?;
        if path_bytes > MAX_PATH_BYTES
            || result.len() >= MAX_ENTRIES
            || result.insert(path, GitEntry { mode, oid }).is_some()
        {
            return Err(failure());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_index_records_validate_stage_paths_modes_and_framing() {
        let oid = "1".repeat(40);
        let index = format!("H 100644 {oid} 0\tspace name\0");
        let tree = format!("100644 blob {oid}\tspace name\0");
        assert_eq!(
            parse(index.as_bytes(), "sha1", true).unwrap(),
            parse(tree.as_bytes(), "sha1", false).unwrap()
        );
        for invalid in [
            format!("H 100644 {oid} 1\tfile\0"),
            format!("H 160000 {oid} 0\tsub\0"),
            format!("H 100644 {oid} 0\t../outside\0"),
            format!("H 100644 {oid} 0\t.git/config\0"),
            format!("S 100644 {oid} 0\tfile\0"),
            format!("h 100644 {oid} 0\tfile\0"),
            format!("{index}{index}"),
            format!("H 100644 {oid} 0\tfile"),
        ] {
            assert!(parse(invalid.as_bytes(), "sha1", true).is_err());
        }
        assert!(parse(b"\0", "sha1", true).is_err());
        assert!(parse(
            format!("H 100644 {} 0\tfile\0", "2".repeat(64)).as_bytes(),
            "sha256",
            true
        )
        .is_ok());
    }
}
