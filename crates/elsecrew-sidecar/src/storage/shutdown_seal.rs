//! A clean shutdown's seal (user decision 2026-11-20): the first start after
//! anything unusual checks the whole ledger, but opening the app again after
//! it closed normally must not take that long every time.
//!
//! At a clean exit the engine empties the write-ahead log and hashes the
//! database file while it still holds the exclusive lock. The app signs that
//! hash with a key only it can read (Keychain-protected) and hands it back at
//! the next start. The engine then trusts the previous full check only if the
//! file is byte for byte what it sealed: no log left, the exclusive lock held,
//! the same hash. Anything else (no seal, another build, a crash, a pending
//! catch-up, one changed byte) is the full check as before. The seal is used
//! once: the app removes it before starting the engine.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read as _;

/// What was hashed, and the file's identity at that moment. `digest` is the
/// seal token `<sha256 of the file>.<engine context>`: the app hashes the
/// file itself before signing, so a look-alike line could only ever name
/// the file as it really is.
pub(crate) struct SealedFile {
    pub(crate) digest: String,
    identity: FileIdentity,
}

#[derive(PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    ino: u64,
}

fn identity(path: &Path) -> Option<FileIdentity> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return None;
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt as _;
    Some(FileIdentity {
        len: meta.len(),
        modified: meta.modified().ok(),
        #[cfg(unix)]
        ino: meta.ino(),
    })
}

/// The engine build and storage format are part of the seal: a seal made by
/// another engine (another version, or a rebuilt development engine with the
/// same version: its executable's build time differs, review M27) never
/// matches, so its rules are checked in full.
fn context() -> String {
    let built = std::env::current_exe()
        .ok()
        .and_then(|exe| fs::metadata(exe).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs());
    format!("{}+s{}+b{built}", env!("CARGO_PKG_VERSION"), STORAGE_LIFETIME_SCHEMA_VERSION_V13)
}

fn wal_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

/// No write-ahead log content: the database file alone is the whole state.
pub(crate) fn wal_is_empty(path: &Path) -> bool {
    match fs::symlink_metadata(wal_path(path)) {
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        Ok(meta) => meta.is_file() && !meta.file_type().is_symlink() && meta.len() == 0,
    }
}

/// `<64 lowercase hex>.<context>`, the context short and plain.
pub(crate) fn valid_seal(text: &str) -> bool {
    let Some((hash, context)) = text.split_once('.') else { return false };
    hash.len() == 64
        && hash.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        && (1..=64).contains(&context.len())
        && context.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'z' | b'.' | b'+' | b'-'))
}

fn digest_file(path: &Path) -> Option<SealedFile> {
    let before = identity(path)?;
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut read = 0u64;
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        read += count as u64;
        hasher.update(&buffer[..count]);
    }
    // The file did not change while it was read.
    if read != before.len || identity(path)? != before {
        return None;
    }
    let hash: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Some(SealedFile { digest: format!("{hash}.{}", context()), identity: before })
}

impl SealedFile {
    /// After the connection closed: closing wrote nothing more and left no
    /// log, so the hash is still the whole state.
    pub(crate) fn still_holds(&self, path: &Path) -> bool {
        wal_is_empty(path) && identity(path).is_some_and(|now| now == self.identity)
    }
}

impl Storage {
    /// Only a ledger with nothing half done is sealed. Derived records still
    /// waiting (`pending`) are normal while the app is used; the running
    /// engine's scheduler catches them up, as after any write. A catch-up
    /// stopped midway or failed, or a compaction under way, is left to the
    /// next start's full check, which is what resolves those.
    fn settled_for_seal(&self) -> bool {
        let unsettled_catchup: Option<i64> = self
            .connection
            .query_row("SELECT count(*) FROM lifetime_materialization_job WHERE state<>'pending'", [], |row| row.get(0))
            .ok();
        let running_compaction: Option<i64> = self
            .connection
            .query_row("SELECT count(*) FROM maintenance_jobs WHERE state IN ('pending','running')", [], |row| row.get(0))
            .ok();
        // As the full check forbids at startup (review M26).
        let delete_authority: Option<i64> = self
            .connection
            .query_row("SELECT count(*) FROM ledger_compaction_delete_authority", [], |row| row.get(0))
            .ok();
        unsettled_catchup == Some(0)
            && running_compaction == Some(0)
            && delete_authority == Some(0)
            && self.connection.is_autocommit()
    }

    /// At a clean exit, before the connection closes: empty the log and hash
    /// the file under the exclusive lock this connection holds.
    pub(crate) fn shutdown_seal(&self) -> Option<SealedFile> {
        let path = self.path.as_path();
        if !self.writes_admitted || !self.settled_for_seal() {
            return None;
        }
        let busy: u32 = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| row.get(0))
            .ok()?;
        if busy != 0 || !wal_is_empty(path) {
            return None;
        }
        digest_file(path)
    }

    /// At the next start, on the newly opened connection: with the exclusive
    /// lock held, the file is exactly what was sealed.
    pub(crate) fn matches_shutdown_seal(&self, expected: &str) -> bool {
        let path = self.path.as_path();
        if !valid_seal(expected) || !self.settled_for_seal() {
            return false;
        }
        if self.connection.execute_batch("BEGIN EXCLUSIVE; COMMIT;").is_err() {
            return false;
        }
        // Checked before opening too, but a log appearing in between would be
        // read by SQLite while only the file is hashed (review I8).
        if !wal_is_empty(path) {
            return false;
        }
        digest_file(path).is_some_and(|file| file.digest == expected)
    }

    /// A consistent copy of the database for the background full check: the
    /// log emptied and the file copied while this connection holds the
    /// exclusive lock (APFS clones it at once).
    pub(crate) fn copy_for_background_check(&self, destination: &Path) -> bool {
        if !self.writes_admitted || !self.connection.is_autocommit() {
            return false;
        }
        let busy: Option<u32> = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| row.get(0))
            .ok();
        busy == Some(0) && wal_is_empty(&self.path) && fs::copy(&self.path, destination).is_ok()
    }

    /// Whether this start trusted a clean shutdown's seal.
    pub(crate) fn opened_from_seal(&self) -> bool {
        self.opened_from_seal
    }

    pub(crate) fn database_path(&self) -> &Path {
        &self.path
    }
}
