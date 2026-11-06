//! Index/HEAD semantic comparison only; raw working-tree equivalence is a separate step.
#[cfg(unix)]
use super::{failure, open, stamp, MAX_ENTRIES, MAX_PATH_BYTES};
#[cfg(windows)]
use crate::project::windows_source_inventory::{failure, MAX_ENTRIES, MAX_PATH_BYTES};
use crate::project::{
    parse_tree_record, trim_output, validate_relative_path, validated_oid, ProjectFailure,
    ProjectManager, SourceRepository,
};
#[cfg(unix)]
use cap_std::fs::MetadataExt;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
#[cfg(unix)]
pub(in crate::project) mod raw_baseline;

const OUTPUT_LIMIT: usize = 24 * 1024 * 1024;
const INDEX_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::project) struct GitIndexBaseline {
    pub(in crate::project) entries: BTreeMap<String, GitEntry>,
    pub(in crate::project) index_digest: [u8; 32],
    pub(in crate::project) commit: String,
    pub(in crate::project) tree: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::project) struct GitEntry {
    pub(in crate::project) mode: String,
    pub(in crate::project) oid: String,
}

// Index-only component; snapshot admission must use the full baseline and durable binding.
#[allow(dead_code)]
pub(in crate::project) fn capture_index_baseline(
    manager: &ProjectManager,
    source: &SourceRepository,
) -> Result<GitIndexBaseline, ProjectFailure> {
    manager.check_application_preparation()?;
    validated_oid(&source.base_commit, &source.object_format)?;
    validated_oid(&source.base_tree, &source.object_format)?;
    let before = index_image(manager, source)?;
    let shared = manager.git_at(&source.git_dir, ["rev-parse", "--shared-index-path"])?;
    if !shared.stdout.is_empty() {
        return Err(ProjectFailure::dirty(
            "split index needs a separate shared-index baseline",
        ));
    }
    manager.check_application_preparation()?;
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
    manager.check_application_preparation()?;
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
    manager.check_application_preparation()?;
    let entries = parse(&index, &source.object_format, true)?;
    if entries != parse(&tree, &source.object_format, false)? {
        return Err(ProjectFailure::dirty(
            "source index differs from the committed tree",
        ));
    }
    // Mode/OID/path alone hides intent-to-add on a committed empty blob. Cached-only
    // plumbing checks that flag without reading working-tree files or invoking converters.
    manager.check_application_preparation()?;
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
        manager.check_application_preparation()?;
        let output = manager.git_at(&source.git_dir, ["rev-parse", "--verify", revision])?;
        if trim_output(&output)? != *expected {
            return Err(ProjectFailure::dirty(
                "source HEAD changed during baseline capture",
            ));
        }
    }
    if before != index_image(manager, source)? {
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

#[cfg(unix)]
fn index_image(
    manager: &ProjectManager,
    source: &SourceRepository,
) -> Result<([i128; 9], [u8; 32]), ProjectFailure> {
    let mut file = open(&source.git_dir, "index", false)?;
    let metadata = file.metadata().map_err(|_| failure())?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > INDEX_LIMIT {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    let mut input = (&mut file).take(INDEX_LIMIT + 1);
    let mut buffer = [0u8; 65536];
    loop {
        manager.check_application_preparation()?;
        let n = input.read(&mut buffer).map_err(|_| failure())?;
        manager.check_application_preparation()?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
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

#[cfg(windows)]
fn index_image(
    manager: &ProjectManager,
    source: &SourceRepository,
) -> Result<
    (
        crate::project::windows_source_inventory::Observation,
        [u8; 32],
    ),
    ProjectFailure,
> {
    use crate::project::{open_real_file_at, windows_source_inventory::file_observation};
    let mut file = open_real_file_at(&source.git_dir, "index")?;
    let before = file_observation(&file)?;
    if before.length > INDEX_LIMIT {
        return Err(failure());
    }
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        manager.check_application_preparation()?;
        let n = file.read(&mut buffer).map_err(|_| failure())?;
        manager.check_application_preparation()?;
        if n == 0 {
            break;
        }
        count = count.checked_add(n as u64).ok_or_else(failure)?;
        if count > INDEX_LIMIT {
            return Err(failure());
        }
        hash.update(&buffer[..n]);
    }
    if count != before.length
        || file_observation(&file)? != before
        || file_observation(&open_real_file_at(&source.git_dir, "index")?)? != before
    {
        return Err(failure());
    }
    Ok((before, hash.finalize().into()))
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

#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
pub(in crate::project) struct WindowsBaseline {
    pub(in crate::project) source_root: String,
    pub(in crate::project) source_identity: String,
    pub(in crate::project) object_format: String,
    pub(in crate::project) index: GitIndexBaseline,
    pub(in crate::project) raw: crate::project::windows_source_inventory::Inventory,
}

#[cfg(windows)]
impl WindowsBaseline {
    pub(in crate::project) fn inventory(
        &self,
    ) -> &crate::project::windows_source_inventory::Inventory {
        &self.raw
    }
    pub(in crate::project) fn tracked_paths(&self) -> impl Iterator<Item = &str> {
        self.index.entries.keys().map(String::as_str)
    }
}

#[cfg(windows)]
pub(in crate::project) fn capture_windows_clean_baseline(
    manager: &ProjectManager,
    source: &SourceRepository,
) -> Result<WindowsBaseline, ProjectFailure> {
    use crate::project::windows_source_inventory;
    use std::io::{BufRead, BufReader, Write};
    let check = || manager.check_application_preparation();
    let index = capture_index_baseline(manager, source)?;
    let raw = windows_source_inventory::capture(&source._root, &source.canonical_root, &check)?;
    let mut child = manager.spawn_git_stream(
        &source.git_dir,
        [
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.allow=never",
            "cat-file",
            "--batch",
        ],
        true,
    )?;
    let mut input = child.stdin.take().ok_or_else(failure)?;
    let mut output = BufReader::new(child.stdout.take().ok_or_else(failure)?);
    let mut budget = 4u64 * 1024 * 1024 * 1024;
    let mut buffer = vec![0u8; 65536];
    for (path, entry) in &index.entries {
        check()?;
        // Windows has no POSIX executable bit. Preserve the committed mode in
        // the index baseline; never interpret a symlink blob as an ordinary file.
        if !matches!(entry.mode.as_str(), "100644" | "100755") {
            return Err(failure());
        }
        let (size, expected) = raw.regular_content(path).ok_or_else(failure)?;
        budget = budget.checked_sub(size).ok_or_else(failure)?;
        writeln!(input, "{}", entry.oid).map_err(|_| failure())?;
        input.flush().map_err(|_| failure())?;
        let mut header = Vec::new();
        output
            .by_ref()
            .take(257)
            .read_until(b'\n', &mut header)
            .map_err(|_| failure())?;
        // Exact framing and bounded size: no filters, textconv or newline conversion.
        if header != format!("{} blob {size}\n", entry.oid).as_bytes() {
            return Err(failure());
        }
        let mut remaining = size;
        let mut hash = Sha256::new();
        while remaining != 0 {
            check()?;
            let count = remaining.min(buffer.len() as u64) as usize;
            output
                .read_exact(&mut buffer[..count])
                .map_err(|_| failure())?;
            check()?;
            hash.update(&buffer[..count]);
            remaining -= count as u64;
        }
        let mut newline = [0];
        output.read_exact(&mut newline).map_err(|_| failure())?;
        let digest: [u8; 32] = hash.finalize().into();
        if newline != [b'\n'] || digest != expected {
            return Err(failure());
        }
    }
    drop(input);
    if output.read(&mut [0u8; 1]).map_err(|_| failure())? != 0 {
        return Err(failure());
    }
    let (status, _) = child.finish().map_err(|_| failure())?;
    if !status.success() {
        return Err(failure());
    }
    if index != capture_index_baseline(manager, source)?
        || raw != windows_source_inventory::capture(&source._root, &source.canonical_root, &check)?
    {
        return Err(failure());
    }
    check()?;
    Ok(WindowsBaseline {
        source_root: crate::project::canonical_protocol_path_without_existing(
            &source.canonical_root,
        )?,
        source_identity: source.source_identity.clone(),
        object_format: source.object_format.clone(),
        index,
        raw,
    })
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
