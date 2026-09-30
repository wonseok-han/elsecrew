//! Raw committed-blob comparison, without checkout/filter/textconv transformations.
use super::super::{capture_once, failure, stamp, Entry, SourceInventory, MAX_BYTES};
use super::{capture_index_baseline, GitIndexBaseline};
use crate::project::{
    open_absolute_directory_capability, validated_oid, ProjectFailure, ProjectManager,
    SourceRepository, StreamingGitChild,
};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::Stdio;
pub(in crate::project) mod receipt;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::project) struct SourceBaseline {
    source_root: String,
    source_identity: String,
    object_format: String,
    raw: SourceInventory,
    index: GitIndexBaseline,
}

// Used by the internal application snapshot candidate; no product writer admission.
pub(in crate::project) fn capture_clean_baseline(
    manager: &ProjectManager,
    source: &SourceRepository,
) -> Result<SourceBaseline, ProjectFailure> {
    let index = capture_index_baseline(manager, source)?;
    let raw = capture_once(&source._root)?;
    verify_root_path(source, &raw)?;
    verify_blobs(manager, source, &index, &raw)?;
    if raw != capture_once(&source._root)? || index != capture_index_baseline(manager, source)? {
        return Err(ProjectFailure::dirty(
            "source changed while recording its baseline",
        ));
    }
    verify_root_path(source, &raw)?;
    Ok(SourceBaseline {
        source_root: crate::project::canonical_protocol_path_without_existing(
            &source.canonical_root,
        )?,
        source_identity: source.source_identity.clone(),
        object_format: source.object_format.clone(),
        raw,
        index,
    })
}

fn verify_root_path(
    source: &SourceRepository,
    raw: &SourceInventory,
) -> Result<(), ProjectFailure> {
    let root = open_absolute_directory_capability(&source.canonical_root)?;
    if stamp(&root.dir_metadata().map_err(|_| failure())?) != raw.root_stamp {
        return Err(ProjectFailure::dirty(
            "source root path no longer names the captured directory",
        ));
    }
    Ok(())
}

fn verify_blobs(
    manager: &ProjectManager,
    source: &SourceRepository,
    index: &GitIndexBaseline,
    raw: &SourceInventory,
) -> Result<(), ProjectFailure> {
    let mut command = manager.git_command_at(&source.git_dir)?;
    command
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.allow=never",
            "cat-file",
            "--batch",
        ])
        .env("GIT_NO_LAZY_FETCH", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = StreamingGitChild::spawn(&mut command).map_err(|_| failure())?;
    let mut input = child.stdin.take().ok_or_else(failure)?;
    let mut output = BufReader::new(child.stdout.take().ok_or_else(failure)?);
    let mut remaining = MAX_BYTES;
    for (path, entry) in &index.entries {
        let actual = raw
            .entries
            .get(path)
            .ok_or_else(|| ProjectFailure::dirty("tracked source file is missing"))?;
        let mode_matches = match entry.mode.as_str() {
            "120000" => actual.kind == 3,
            "100644" => actual.kind == 1 && actual.mode & 0o100 == 0,
            "100755" => actual.kind == 1 && actual.mode & 0o100 != 0,
            _ => false,
        };
        if !mode_matches {
            return Err(ProjectFailure::dirty(
                "tracked source file kind or executable bit differs",
            ));
        }
        writeln!(input, "{}", entry.oid).map_err(|_| failure())?;
        input.flush().map_err(|_| failure())?;
        compare_blob(
            &mut output,
            &entry.oid,
            &source.object_format,
            actual,
            &mut remaining,
        )?;
    }
    drop(input);
    // No unsolicited data is accepted after the final batch response.
    let mut extra = [0u8; 1];
    if output.read(&mut extra).map_err(|_| failure())? != 0 {
        return Err(failure());
    }
    let (status, _) = child.finish().map_err(|_| failure())?;
    if !status.success() {
        return Err(ProjectFailure::dirty("committed blob stream failed"));
    }
    Ok(())
}

fn compare_blob<R: BufRead>(
    output: &mut R,
    oid: &str,
    format: &str,
    actual: &Entry,
    remaining: &mut u64,
) -> Result<(), ProjectFailure> {
    let mut header = Vec::new();
    output
        .by_ref()
        .take(257)
        .read_until(b'\n', &mut header)
        .map_err(|_| failure())?;
    if header.len() > 256 || header.last() != Some(&b'\n') {
        return Err(failure());
    }
    let text = std::str::from_utf8(&header[..header.len() - 1]).map_err(|_| failure())?;
    let mut fields = text.split(' ');
    if validated_oid(fields.next().ok_or_else(failure)?, format)? != oid
        || fields.next() != Some("blob")
    {
        return Err(failure());
    }
    let size_text = fields.next().ok_or_else(failure)?;
    if size_text.is_empty()
        || !size_text.bytes().all(|b| b.is_ascii_digit())
        || fields.next().is_some()
    {
        return Err(failure());
    }
    let size = size_text.parse::<u64>().map_err(|_| failure())?;
    if size != actual.size {
        return Err(ProjectFailure::dirty(
            "raw source length differs from committed blob",
        ));
    }
    *remaining = remaining.checked_sub(size).ok_or_else(failure)?;
    let mut left = size;
    let mut hash = Sha256::new();
    // Git object identity hashes the object header plus content, not the content alone.
    let mut git_sha1 = sha1::Sha1::new();
    let mut git_sha256 = Sha256::new();
    let object_header = format!("blob {size}\0");
    git_sha1.update(object_header.as_bytes());
    git_sha256.update(object_header.as_bytes());
    let mut buffer = vec![0; 64 * 1024];
    while left > 0 {
        let count = left.min(buffer.len() as u64) as usize;
        output
            .read_exact(&mut buffer[..count])
            .map_err(|_| failure())?;
        hash.update(&buffer[..count]);
        git_sha1.update(&buffer[..count]);
        git_sha256.update(&buffer[..count]);
        left -= count as u64;
    }
    let mut separator = [0u8; 1];
    output.read_exact(&mut separator).map_err(|_| failure())?;
    if separator != *b"\n" || <[u8; 32]>::from(hash.finalize()) != actual.digest {
        return Err(ProjectFailure::dirty(
            "raw source bytes differ from committed blob",
        ));
    }
    let computed_oid = match format {
        "sha1" => format!("{:x}", git_sha1.finalize()),
        "sha256" => format!("{:x}", git_sha256.finalize()),
        _ => return Err(failure()),
    };
    if computed_oid != oid {
        return Err(ProjectFailure::manifest(
            "committed blob content does not match its Git object id",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn source_blob_stream_rejects_mismatch_truncation_and_over_budget() {
        let oid = format!("{:x}", sha1::Sha1::digest(b"blob 4\0raw\0"));
        let actual = Entry {
            identity: (1, 2, 1),
            kind: 1,
            mode: 0o100644,
            size: 4,
            digest: Sha256::digest(b"raw\0").into(),
        };
        let valid = format!("{oid} blob 4\nraw\0\n").into_bytes();
        compare_blob(&mut Cursor::new(&valid), &oid, "sha1", &actual, &mut 4).unwrap();
        let forged_oid = "1".repeat(40);
        assert_eq!(
            compare_blob(
                &mut Cursor::new(format!("{forged_oid} blob 4\nraw\0\n")),
                &forged_oid,
                "sha1",
                &actual,
                &mut 4
            )
            .unwrap_err()
            .kind,
            "manifestMismatch"
        );
        let sha256_oid = format!("{:x}", Sha256::digest(b"blob 4\0raw\0"));
        compare_blob(
            &mut Cursor::new(format!("{sha256_oid} blob 4\nraw\0\n")),
            &sha256_oid,
            "sha256",
            &actual,
            &mut 4,
        )
        .unwrap();
        assert!(compare_blob(&mut Cursor::new(&valid), &oid, "sha1", &actual, &mut 3).is_err());
        for bytes in [
            format!("{oid} blob 5\n").into_bytes(),
            format!("{oid} missing\n").into_bytes(),
            format!("{oid} blob 4\nnope\n").into_bytes(),
            valid[..valid.len() - 1].to_vec(),
            vec![b'x'; 257],
        ] {
            assert!(compare_blob(&mut Cursor::new(bytes), &oid, "sha1", &actual, &mut 4).is_err());
        }
    }
}
