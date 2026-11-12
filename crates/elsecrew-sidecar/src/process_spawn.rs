//! Serialize Windows inheritable-handle windows across sidecar launch paths.
use std::{
    io,
    process::{Child, Command},
};

#[cfg(windows)]
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(windows)]
pub(crate) fn lock() -> io::Result<std::sync::MutexGuard<'static, ()>> {
    SPAWN
        .lock()
        .map_err(|_| io::Error::other("Windows process creation lock poisoned"))
}

pub(crate) trait SpawnGuarded {
    fn spawn_guarded(&mut self) -> io::Result<Child>;
}
impl SpawnGuarded for Command {
    fn spawn_guarded(&mut self) -> io::Result<Child> {
        #[cfg(windows)]
        let _guard = lock()?;
        self.spawn()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn concurrent_launch_cannot_enter_handle_inheritance_window() {
        let held = lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _guard = lock().unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(30))
            .is_err());
        drop(held);
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
    }
}
