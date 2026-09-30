//! Bounded, canonical framing and per-item checks; never writes restored files.
use super::*;
use std::io::BufRead;

fn object(value: &Value, keys: &[&str]) -> Result<(), ProjectFailure> {
    if value
        .as_object()
        .is_none_or(|o| o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)))
    {
        return Err(invalid());
    }
    Ok(())
}

fn digest(value: &Value) -> Result<[u8; 32], ProjectFailure> {
    let values = value.as_array().ok_or_else(invalid)?;
    if values.len() != 32 {
        return Err(invalid());
    }
    let mut digest = [0; 32];
    for (out, value) in digest.iter_mut().zip(values) {
        *out = u8::try_from(value.as_u64().ok_or_else(invalid)?).map_err(|_| invalid())?;
    }
    Ok(digest)
}

fn line(input: &mut impl BufRead, budget: &mut u64) -> Result<Value, ProjectFailure> {
    let mut bytes = Vec::new();
    input
        .take(65537)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > 65536 || bytes.last() != Some(&b'\n') {
        return Err(invalid());
    }
    *budget = budget.checked_sub(bytes.len() as u64).ok_or_else(invalid)?;
    bytes.pop();
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    // Reject duplicate keys, extra whitespace and alternative numeric spellings.
    if serde_json::to_vec(&value).map_err(|_| invalid())? != bytes {
        return Err(invalid());
    }
    Ok(value)
}

fn image(value: &Value) -> Result<(), ProjectFailure> {
    if value.is_null() {
        return Ok(());
    }
    object(value, &["kind", "executable", "size", "digest"])?;
    let kind = value["kind"].as_u64().ok_or_else(invalid)?;
    let size = value["size"].as_u64().ok_or_else(invalid)?;
    let executable = value["executable"].as_bool().ok_or_else(invalid)?;
    let hash = digest(&value["digest"])?;
    if !matches!(kind, 1..=3)
        || size > MAX_BYTES
        || (kind == 2 && (size != 0 || hash != [0; 32]))
        || (kind == 3 && (size == 0 || size > 4096))
        || (kind != 1 && executable)
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn validate(input: &mut impl BufRead) -> Result<Value, ProjectFailure> {
    let mut metadata_budget = 32 * 1024 * 1024;
    let mut content_budget = MAX_BYTES;
    let mut path_budget = 8 * 1024 * 1024usize;
    let header = line(input, &mut metadata_budget)?;
    object(
        &header,
        &["version", "reviewId", "binding", "changesDigest", "count"],
    )?;
    let id = header["reviewId"].as_str().ok_or_else(invalid)?;
    let count = header["count"].as_u64().ok_or_else(invalid)?;
    if header["version"] != serde_json::json!(2)
        || !header["binding"].is_object()
        || count > 100000
        || id.len() != 39
        || !id.starts_with("review_")
        || !id[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let expected = digest(&header["changesDigest"])?;
    let mut changes = ReviewHash {
        hash: Sha256::new(),
        remaining: MAX_REVIEW_BYTES,
    };
    serde_json::to_writer(&mut changes, &(1u32, &header["binding"])).map_err(|_| invalid())?;
    let mut previous = String::new();
    for _ in 0..count {
        let item = line(input, &mut metadata_budget)?;
        object(&item, &["path", "before", "after"])?;
        let path = item["path"].as_str().ok_or_else(invalid)?;
        if path <= previous.as_str()
            || path.len() > 4096
            || path.contains('\0')
            || path.split('/').count() > 129
            || path.split('/').any(|part| {
                part.is_empty() || part == "." || part == ".." || part.eq_ignore_ascii_case(".git")
            })
        {
            return Err(invalid());
        }
        path_budget = path_budget.checked_sub(path.len()).ok_or_else(invalid)?;
        previous = path.to_owned();
        let before = &item["before"];
        let after = &item["after"];
        image(after)?;
        let before_image = if before.is_null() {
            Value::Null
        } else {
            object(before, &["kind", "mode", "size", "digest", "identity"])?;
            let kind = before["kind"].as_u64().ok_or_else(invalid)?;
            let mode = u32::try_from(before["mode"].as_u64().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            let identity = before["identity"].as_array().ok_or_else(invalid)?;
            if identity.len() != 3
                || identity.iter().any(|v| v.as_u64().is_none())
                || identity[2] != serde_json::json!(1)
                || !matches!(kind, 1 | 3)
                || mode & !0o177777 != 0
                || mode & 0o170000 != if kind == 1 { 0o100000 } else { 0o120000 }
            {
                return Err(invalid());
            }
            let normalized = serde_json::json!({"kind":kind,"executable":kind == 1 && mode & 0o100 != 0,"size":before["size"],"digest":before["digest"]});
            image(&normalized)?;
            let size = before["size"].as_u64().ok_or_else(invalid)?;
            content_budget = content_budget.checked_sub(size).ok_or_else(invalid)?;
            let mut hash = Sha256::new();
            let mut remaining = size;
            let mut buffer = [0u8; 65536];
            while remaining != 0 {
                let n = remaining.min(buffer.len() as u64) as usize;
                input.read_exact(&mut buffer[..n]).map_err(|_| invalid())?;
                if kind == 3 && buffer[..n].contains(&0) {
                    return Err(invalid());
                }
                hash.update(&buffer[..n]);
                remaining -= n as u64;
            }
            if <[u8; 32]>::from(hash.finalize()) != digest(&before["digest"])? {
                return Err(invalid());
            }
            normalized
        };
        if before_image == *after
            || (!before_image.is_null()
                && !after.is_null()
                && before_image["kind"] != after["kind"])
        {
            return Err(invalid());
        }
        serde_json::to_writer(&mut changes, &(path, before_image, after)).map_err(|_| invalid())?;
    }
    if !input.fill_buf().map_err(|_| invalid())?.is_empty()
        || <[u8; 32]>::from(changes.hash.finalize()) != expected
    {
        return Err(invalid());
    }
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (Value, Vec<u8>) {
        (
            json!({"path":"a","before":{"kind":1,"mode":0o100644,"size":3,"digest":<[u8;32]>::from(Sha256::digest(b"a\nb")),"identity":[1,2,1]},"after":null}),
            b"a\nb".to_vec(),
        )
    }

    fn archive(items: &[(Value, Vec<u8>)]) -> Vec<u8> {
        let mut hash = ReviewHash {
            hash: Sha256::new(),
            remaining: MAX_REVIEW_BYTES,
        };
        let binding = json!({"projectId":"project_test"});
        serde_json::to_writer(&mut hash, &(1u32, &binding)).unwrap();
        for (item, _) in items {
            let b = &item["before"];
            let normalized = if b.is_null() {
                Value::Null
            } else {
                json!({"kind":b["kind"],"executable":b["kind"] == 1 && b["mode"].as_u64().unwrap_or(0) & 0o100 != 0,"size":b["size"],"digest":b["digest"]})
            };
            serde_json::to_writer(&mut hash, &(&item["path"], normalized, &item["after"])).unwrap();
        }
        let header = json!({"version":2,"reviewId":"review_0123456789abcdef0123456789abcdef","binding":binding,"count":items.len(),"changesDigest":<[u8;32]>::from(hash.hash.finalize())});
        let mut bytes = serde_json::to_vec(&header).unwrap();
        bytes.push(b'\n');
        for (item, payload) in items {
            bytes.extend(serde_json::to_vec(item).unwrap());
            bytes.push(b'\n');
            bytes.extend(payload);
        }
        bytes
    }

    fn valid(bytes: &[u8]) -> bool {
        validate(&mut std::io::Cursor::new(bytes)).is_ok()
    }

    #[test]
    fn backup_decoder_rejects_truncation_trailing_data_and_invalid_frames() {
        let original = fixture();
        let good = archive(std::slice::from_ref(&original));
        assert!(valid(&good));
        assert!(valid(&archive(&[])));
        for cut in 0..good.len() {
            assert!(!valid(&good[..cut]), "cut {cut}");
        }
        let mut extra = good.clone();
        extra.push(0);
        assert!(!valid(&extra));
        let mut corrupt = good.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(!valid(&corrupt));
        for path in [
            "../a",
            "/a",
            "a//b",
            "a/./b",
            ".git/config",
            "a/../b",
            "a\0b",
        ] {
            let mut bad = original.clone();
            bad.0["path"] = json!(path);
            assert!(!valid(&archive(&[bad])));
        }
        for (field, value) in [
            ("kind", json!(2)),
            ("mode", json!(0o120644)),
            ("size", json!(MAX_BYTES + 1)),
            ("digest", json!([0])),
            ("identity", json!([1, 2, 2])),
        ] {
            let mut bad = original.clone();
            bad.0["before"][field] = value;
            assert!(!valid(&archive(&[bad])));
        }
        assert!(!valid(&archive(&[original.clone(), original.clone()])));
        let mut bad = original.clone();
        bad.0["extra"] = json!(1);
        assert!(!valid(&archive(&[bad])));
        let mut duplicate = good.clone();
        let start = duplicate
            .windows(8)
            .position(|p| p == b"\"count\":")
            .unwrap();
        duplicate.splice(start..start, b"\"count\":1,".iter().copied());
        assert!(!valid(&duplicate));
        assert!(!valid(&vec![b'x'; 65537]));
    }
}
