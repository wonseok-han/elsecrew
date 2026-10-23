//! Durable-output admission only. Never deletes records or limits recovery writes.
use super::SidecarError;
use std::path::Path;

const DATABASE_BUDGET: u64 = 1024 * 1024 * 1024;
const FREE_RESERVE: u64 = 256 * 1024 * 1024;
// Head, row and indexes can dirty more pages than the encoded event size.
const APPEND_HEADROOM: u64 = 8 * 1024 * 1024;

fn failure() -> SidecarError {
    SidecarError::new("agentOutputStorageFull", "durable output storage budget unavailable")
}

fn within_budget(bytes: u64, available: u64, before_write: bool) -> Result<(), SidecarError> {
    let headroom = if before_write { APPEND_HEADROOM } else { 0 };
    if bytes.checked_add(headroom).is_none_or(|value| value > DATABASE_BUDGET)
        || available < FREE_RESERVE + headroom
    {
        return Err(failure());
    }
    Ok(())
}

/// Includes the whole database, WAL and SHM, not just logical output text.
/// Free space is observational, not an OS reservation: concurrent disk users can
/// still cause ENOSPC. The existing persistent run fence must survive that case.
#[cfg(unix)]
pub(super) fn check(connection: &rusqlite::Connection, path: &Path, before_write: bool) -> Result<(), SidecarError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let open = |path: &Path| std::fs::OpenOptions::new().read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK).open(path);
    let database = open(path).map_err(|_| failure())?;
    let metadata = database.metadata().map_err(|_| failure())?;
    if !metadata.is_file() { return Err(failure()); }
    let pages: u64 = connection.query_row("PRAGMA main.page_count", [], |row| row.get(0)).map_err(|_| failure())?;
    let page_size: u64 = connection.query_row("PRAGMA main.page_size", [], |row| row.get(0)).map_err(|_| failure())?;
    let logical = pages.checked_mul(page_size).ok_or_else(failure)?;
    // Count uncheckpointed growth too; main-file length alone can be stale in WAL mode.
    let mut bytes = metadata.len().max(logical);
    for suffix in ["-wal", "-shm"] {
        let mut filename = path.as_os_str().to_os_string();
        filename.push(suffix);
        match open(Path::new(&filename)) {
            Ok(file) => {
                let metadata = file.metadata().map_err(|_| failure())?;
                if !metadata.is_file() { return Err(failure()); }
                bytes = bytes.checked_add(metadata.len()).ok_or_else(failure)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure()),
        }
    }
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: valid owned descriptor and writable statvfs storage.
    if unsafe { libc::fstatvfs(database.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
        return Err(failure());
    }
    // SAFETY: successful fstatvfs initialized the structure.
    let stats = unsafe { stats.assume_init() };
    let available = (stats.f_bavail as u64).checked_mul(stats.f_frsize as u64).ok_or_else(failure)?;
    within_budget(bytes, available, before_write)
}

#[cfg(not(unix))]
pub(super) fn check(_connection: &rusqlite::Connection, _path: &Path, _before_write: bool) -> Result<(), SidecarError> {
    // Actual Windows execution remains disabled until its storage probe is verified.
    Err(failure())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_budget_keeps_recovery_reserve_and_checks_overflow() {
        assert!(within_budget(DATABASE_BUDGET - APPEND_HEADROOM, FREE_RESERVE + APPEND_HEADROOM, true).is_ok());
        assert!(within_budget(DATABASE_BUDGET - APPEND_HEADROOM + 1, u64::MAX, true).is_err());
        assert!(within_budget(0, FREE_RESERVE + APPEND_HEADROOM - 1, true).is_err());
        assert!(within_budget(u64::MAX, u64::MAX, true).is_err());
        assert!(within_budget(DATABASE_BUDGET, FREE_RESERVE, false).is_ok());
        assert!(within_budget(DATABASE_BUDGET + 1, u64::MAX, false).is_err());
    }
}
