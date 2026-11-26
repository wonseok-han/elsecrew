//! The full storage check while the app rests (user decision 2026-11-20).
//!
//! A start from a clean shutdown's seal skips the full ledger check, so what
//! the app wrote since the last full check has not been looked at. Instead of
//! a slow start every so often, the engine checks in the background: while no
//! agent work runs it takes a copy of the database (under its exclusive lock,
//! with the write-ahead log emptied: a consistent state; APFS clones it in an
//! instant), and a low-priority thread runs the very same full admission
//! check on that copy. The copy's own catch-up writes stay in the copy.
//!
//! Success moves "the last full check" to the copy's time, which the seal
//! carries to the next start. A failure may be a damaged ledger or something
//! passing (a full disk, say; review M41): until a later check passes, this
//! session does not seal, so the next start checks in full and shows the
//! usual storage failure if there is one. A failure is tried once more soon;
//! a second one in a row ends the checks for this session.
use crate::storage::Storage;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Where copies live while they are checked (inside the app data).
pub(crate) const CHECK_DIR: &str = "background-check-v1";
/// First look a while after the start; then again after this long.
const FIRST_AFTER: Duration = Duration::from_secs(60);
const AGAIN_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
/// A copy that could not be made (busy log, say) is tried again soon.
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

struct Running {
    handle: JoinHandle<bool>,
    copied_at_ms: u64,
}

pub(crate) struct BackgroundCheck {
    next_at: Instant,
    running: Option<Running>,
    cancel: Arc<AtomicBool>,
    /// When the state last proven by a full check was taken (ms since epoch).
    last_success_ms: Option<u64>,
    /// Full checks that failed in a row since the last success: while any,
    /// this session does not seal.
    failures: u8,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

fn remove_copy(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }
}

/// Copies left by an earlier session that ended mid-check.
pub(crate) fn clean_leftovers(data_dir: &Path) {
    let _ = std::fs::remove_dir_all(data_dir.join(CHECK_DIR));
}

impl BackgroundCheck {
    /// `from_seal`: this start skipped the full check, so the first look comes
    /// soon; a start that checked in full has nothing new to look at for now.
    pub(crate) fn new(from_seal: bool) -> Self {
        Self {
            next_at: Instant::now() + if from_seal { FIRST_AFTER } else { AGAIN_AFTER },
            running: None,
            cancel: Arc::new(AtomicBool::new(false)),
            last_success_ms: None,
            failures: 0,
        }
    }

    pub(crate) fn last_success_ms(&self) -> Option<u64> {
        self.last_success_ms
    }

    pub(crate) fn failed(&self) -> bool {
        self.failures > 0
    }

    #[cfg(test)]
    fn due_now(&mut self) {
        self.next_at = Instant::now();
    }

    /// The app is quitting: let a running check go (its thread ends with the
    /// process); nothing is recorded from it.
    pub(crate) fn stop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.running = None;
    }

    /// One maintenance tick. `idle`: no agent work runs or starts.
    pub(crate) fn tick(&mut self, storage: &Storage, idle: bool) {
        if self.failures >= 2 {
            return;
        }
        if let Some(running) = &self.running {
            if !running.handle.is_finished() {
                return;
            }
            let running = self.running.take().unwrap();
            match running.handle.join() {
                Ok(true) => {
                    // The state as of the copy passed in full, all of it.
                    self.last_success_ms = Some(running.copied_at_ms);
                    self.failures = 0;
                    self.next_at = Instant::now() + AGAIN_AFTER;
                }
                Ok(false) | Err(_) => {
                    eprintln!("storage background check: the full check failed; this session will not seal unless a later one passes");
                    self.failures += 1;
                    self.next_at = Instant::now() + RETRY_AFTER;
                }
            }
            return;
        }
        if !idle || Instant::now() < self.next_at {
            return;
        }
        let Some(data_dir) = storage.database_path().parent().map(Path::to_path_buf) else { return };
        let directory = data_dir.join(CHECK_DIR);
        let copy = directory.join("ledger.sqlite3");
        let made = std::fs::create_dir_all(&directory).is_ok() && {
            remove_copy(&copy);
            storage.copy_for_background_check(&copy)
        };
        if !made {
            remove_copy(&copy);
            self.next_at = Instant::now() + RETRY_AFTER;
            return;
        }
        let copied_at_ms = now_ms();
        let cancel = Arc::clone(&self.cancel);
        let handle = std::thread::spawn(move || {
            lower_priority();
            let passed = Storage::open_for_durable_admission(&copy, &cancel).is_ok();
            remove_copy(&copy);
            // A stopped check proves nothing either way.
            passed || cancel.load(Ordering::Acquire)
        });
        self.running = Some(Running { handle, copied_at_ms });
    }
}

/// Background I/O and CPU priority for this thread only, so the check never
/// competes with what the user is doing.
fn lower_priority() {
    #[cfg(target_os = "macos")]
    unsafe {
        let _ = libc::setpriority(libc::PRIO_DARWIN_THREAD, 0, libc::PRIO_DARWIN_BG);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn wait(check: &mut BackgroundCheck, storage: &Storage) {
        for _ in 0..600 {
            if check.running.is_none() {
                return;
            }
            check.tick(storage, true);
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the background check did not finish");
    }

    /// User decision 2026-11-20: while the app rests, a copy of the ledger is
    /// checked in full; success is what the seal carries, the copy is gone.
    #[test]
    fn a_resting_app_checks_a_copy_in_full_and_leaves_no_copy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("elsecrew.sqlite3");
        let storage = Storage::open_for_durable_admission(&path, &AtomicBool::new(false)).unwrap();
        let mut check = BackgroundCheck::new(false);
        check.tick(&storage, true);
        assert!(check.running.is_none(), "a start that checked in full waits");

        let mut check = BackgroundCheck::new(true);
        check.tick(&storage, true);
        assert!(check.running.is_none(), "not right after the start");
        check.due_now();
        check.tick(&storage, false);
        assert!(check.running.is_none(), "not while agent work runs");
        let before = now_ms();
        check.tick(&storage, true);
        assert!(check.running.is_some());
        wait(&mut check, &storage);
        assert!(!check.failed());
        assert!(check.last_success_ms().is_some_and(|at| at >= before));
        assert!(!directory.path().join(CHECK_DIR).join("ledger.sqlite3").exists());
        // The real ledger is untouched and still opens as before.
        drop(storage);
        drop(Storage::open_for_durable_admission(&path, &AtomicBool::new(false)).unwrap());
    }

    #[test]
    fn a_failed_check_stops_sealing_until_a_later_one_passes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("elsecrew.sqlite3");
        let storage = Storage::open_for_durable_admission(&path, &AtomicBool::new(false)).unwrap();
        // A check that finds damage: the thread reports failure.
        let mut check = BackgroundCheck::new(true);
        let fail = |check: &mut BackgroundCheck| {
            check.running = Some(Running { handle: std::thread::spawn(|| false), copied_at_ms: now_ms() });
        };
        fail(&mut check);
        wait(&mut check, &storage);
        assert!(check.failed());
        assert_eq!(check.last_success_ms(), None);
        // Tried once more (it may have been a full disk): a pass clears it.
        check.due_now();
        check.tick(&storage, true);
        assert!(check.running.is_some(), "a second look after one failure");
        wait(&mut check, &storage);
        assert!(!check.failed() && check.last_success_ms().is_some());
        // Two failures in a row: no more checks, and no seal this session.
        fail(&mut check);
        wait(&mut check, &storage);
        fail(&mut check);
        wait(&mut check, &storage);
        assert!(check.failed());
        check.due_now();
        check.tick(&storage, true);
        assert!(check.running.is_none(), "no further checks after two failures");
    }
}
