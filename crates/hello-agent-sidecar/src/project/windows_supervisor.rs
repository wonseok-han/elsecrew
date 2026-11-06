//! Creator-thread debug pump. A process is retained before ContinueDebugEvent,
//! and its duplicate must signal after its exit event has been continued.
//! https://learn.microsoft.com/en-us/windows/win32/debug/debugging-events
//! Job accounting is a completeness cross-check, never an exit barrier itself.
use super::native::{accounting, Launch};
use std::{
    ffi::c_void,
    io,
    os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[repr(C)]
#[derive(Clone, Copy)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut c_void,
    address: *mut c_void,
    parameters: u32,
    information: [usize; 15],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ExceptionInfo {
    record: ExceptionRecord,
    first_chance: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CreateInfo {
    file: *mut c_void,
    process: *mut c_void,
    thread: *mut c_void,
    image: *mut c_void,
    debug_offset: u32,
    debug_size: u32,
    thread_local_base: *mut c_void,
    start: *mut c_void,
    image_name: *mut c_void,
    unicode: u16,
}
#[repr(C)]
union DebugInfo {
    exception: ExceptionInfo,
    create: CreateInfo,
    file: *mut c_void,
}
#[repr(C)]
struct DebugEvent {
    code: u32,
    pid: u32,
    tid: u32,
    data: DebugInfo,
}
#[link(name = "kernel32")]
extern "system" {
    fn WaitForDebugEvent(event: *mut DebugEvent, timeout: u32) -> i32;
    fn ContinueDebugEvent(pid: u32, tid: u32, status: u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetProcessId(process: *mut c_void) -> u32;
    fn DuplicateHandle(
        source_process: *mut c_void,
        source: *mut c_void,
        target_process: *mut c_void,
        target: *mut *mut c_void,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
    fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
    fn TerminateJobObject(job: *mut c_void, code: u32) -> i32;
}
const CONTINUE: u32 = 0x00010002;
const NOT_HANDLED: u32 = 0x80010001;
const MAX_MEMBERS: usize = 4096;

#[derive(Default)]
enum State {
    #[default]
    Running,
    Complete(u32),
    Failed(String),
}
pub(super) struct Supervisor {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Supervisor {
    /// Both CreateProcess(DEBUG_PROCESS) and all debug calls run on this thread.
    pub(super) fn spawn<F>(create: F) -> io::Result<(Launch, Self)>
    where
        F: FnOnce() -> io::Result<Launch> + Send + 'static,
    {
        let state = Arc::new(Mutex::new(State::Running));
        let stop = Arc::new(AtomicBool::new(false));
        let state_worker = Arc::clone(&state);
        let stop_worker = Arc::clone(&stop);
        let (send, receive) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("windows-family-owner".into())
            .spawn(move || {
                let result = (|| -> io::Result<u32> {
                    let mut launch = create()?;
                    let published = Launch {
                        job: launch.job.try_clone()?,
                        process: launch.process.try_clone()?,
                        thread: launch.thread.try_clone()?,
                        directory: launch.directory.as_ref().map(|pin| pin.observer()),
                        executable: launch.executable.as_ref().map(|pin| pin.observer()),
                    };
                    if send.send(published).is_err() {
                        unsafe {
                            TerminateJobObject(launch.job.as_raw_handle(), 1);
                        }
                        return Err(io::Error::other("family creation receiver closed"));
                    }
                    let result = pump(&launch, &stop_worker).map_err(|error| {
                        if launch.directory.is_some() || launch.executable.is_some() {
                            io::Error::other(format!(
                                "{error}; launch pins retained until sidecar exit"
                            ))
                        } else {
                            error
                        }
                    });
                    if result.is_ok() {
                        if let Some(pin) = &mut launch.directory {
                            pin.confirmed();
                        }
                    }
                    if result.is_ok() {
                        if let Some(pin) = &mut launch.executable {
                            pin.confirmed();
                        }
                    }
                    if result.is_err() {
                        // Request only. Failure never changes into completion proof.
                        unsafe {
                            TerminateJobObject(launch.job.as_raw_handle(), 1);
                        }
                    }
                    result
                })();
                if let Ok(mut state) = state_worker.lock() {
                    *state = match result {
                        Ok(code) => State::Complete(code),
                        Err(error) => State::Failed(error.to_string()),
                    };
                }
                // Debugger-thread exit/Job close request cleanup on every error path.
            })?;
        let owner = Self {
            state,
            stop,
            worker: Some(worker),
        };
        match receive.recv_timeout(Duration::from_secs(10)) {
            Ok(launch) => Ok((launch, owner)),
            Err(_) => {
                owner.check()?;
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows family creation unconfirmed",
                ))
            }
        }
    }
    pub(super) fn check(&self) -> io::Result<()> {
        self.completed().map(|_| ())
    }
    pub(super) fn completed(&self) -> io::Result<Option<u32>> {
        let state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("family state poisoned"))?;
        match &*state {
            State::Complete(code) => Ok(Some(*code)),
            State::Failed(message) => Err(io::Error::other(message.clone())),
            State::Running if self.worker.as_ref().is_some_and(|w| w.is_finished()) => {
                Err(io::Error::other("family supervisor exited without proof"))
            }
            State::Running => Ok(None),
        }
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            if worker.is_finished() {
                let _ = worker.join();
            }
            // Otherwise detach: the worker owns all handles, requests termination
            // and has a two-second drain budget. Drop makes no cleanup guarantee.
        }
    }
}
struct Member {
    pid: u32,
    process: OwnedHandle,
    exit_continued: bool,
    initial_breakpoint: bool,
}
fn duplicate(raw: *mut c_void) -> io::Result<OwnedHandle> {
    let current = unsafe { GetCurrentProcess() };
    let mut copy = std::ptr::null_mut();
    if unsafe { DuplicateHandle(current, raw, current, &mut copy, 0, 0, 2) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(copy) })
}
fn close_image(raw: *mut c_void) {
    if !raw.is_null() && raw as isize != -1 {
        drop(unsafe { OwnedHandle::from_raw_handle(raw) });
    }
}
fn signaled(handle: &OwnedHandle) -> io::Result<bool> {
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
        0 => Ok(true),
        258 => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}
fn pump(launch: &Launch, stop: &AtomicBool) -> io::Result<u32> {
    let root_pid = unsafe { GetProcessId(launch.process.as_raw_handle()) };
    if root_pid == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut members: Vec<Member> = Vec::new();
    let mut shutdown = None;
    loop {
        if stop.load(Ordering::Acquire) && shutdown.is_none() {
            if unsafe { TerminateJobObject(launch.job.as_raw_handle(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            shutdown = Some(Instant::now() + Duration::from_secs(2));
        }
        if shutdown.is_some_and(|end| Instant::now() >= end) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "family shutdown unconfirmed",
            ));
        }
        let mut event: DebugEvent = unsafe { std::mem::zeroed() };
        if unsafe { WaitForDebugEvent(&mut event, 10) } != 0 {
            // Rejected events must NOT resume user code. The worker requests
            // Job termination and exits its debugger thread on any rejection.
            let result = if members.is_empty() && (event.code != 3 || event.pid != root_pid) {
                // Still close image handles delivered by an invalid event.
                if event.code == 3 {
                    close_image(unsafe { event.data.create.file });
                }
                if event.code == 6 {
                    close_image(unsafe { event.data.file });
                }
                Err(io::Error::other(
                    "first debug event does not identify owned root",
                ))
            } else {
                let image = if members.is_empty() || matches!(event.code, 3 | 6) {
                    launch.executable.as_ref().map(|pin| pin.resource())
                } else {
                    None
                };
                handle_event(&event, &launch.job, &mut members, image)
            };
            let status = result?;
            if unsafe { ContinueDebugEvent(event.pid, event.tid, status) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if event.code == 5 {
                let member = members
                    .iter_mut()
                    .find(|m| m.pid == event.pid && !m.exit_continued)
                    .ok_or_else(|| io::Error::other("exit without retained process"))?;
                member.exit_continued = true;
            }
        } else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(121) {
                return Err(error);
            } // ERROR_SEM_TIMEOUT
        }
        if members.is_empty() || !signaled(&launch.process)? {
            continue;
        }
        let mut all_signaled = true;
        for member in &members {
            if !member.exit_continued || !signaled(&member.process)? {
                all_signaled = false;
                break;
            }
        }
        if !all_signaled {
            continue;
        }
        // DEBUG_PROCESS inheritance can be interrupted by a nested debugger.
        // Total lifetime Job membership must match our entire retained inventory.
        // Never treat active=0 alone as proof, or ignore untracked console members.
        let (total, active) = accounting(&launch.job)?;
        if total as usize != members.len() {
            return Err(io::Error::other("incomplete Windows family inventory"));
        }
        if active != 0 {
            continue;
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(launch.process.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(code);
    }
}
fn handle_event(
    event: &DebugEvent,
    job: &OwnedHandle,
    members: &mut Vec<Member>,
    image: Option<&crate::project::windows_executable::ExecutableLease>,
) -> io::Result<u32> {
    match event.code {
        3 => {
            let created = unsafe { event.data.create };
            // A Git invocation permits re-execution of its inspected runtime
            // (including hard links to that same file), not arbitrary helpers,
            // shells or repository executables. Non-Git fixture owners retain
            // their existing root-only image contract.
            let image = image.filter(|image| members.is_empty() || image.observes_dependencies());
            let verified = if let Some(image) = image {
                if created.file.is_null() || created.file as isize == -1 {
                    Err(io::Error::other("process image handle is unavailable"))
                } else {
                    // This event precedes user-mode execution. Compare the actual
                    // image object, never the optional process-supplied filename.
                    let actual = unsafe { BorrowedHandle::borrow_raw(created.file) };
                    image.verify_loaded_image(&actual).map_err(|error| {
                        // Opt-in installed-Git CI needs to distinguish the root
                        // from a rejected child. Diagnostics never admit it or
                        // continue the event after a failed identity check.
                        #[cfg(test)]
                        eprintln!(
                            "Windows image rejection: first={}, selected={:?}, actual={:?}, identity={:?}",
                            members.is_empty(),
                            image.path(),
                            crate::project::windows_fs::local_guid_path(&actual),
                            crate::project::windows_fs::identity(&actual, false),
                        );
                        io::Error::other(format!(
                            "{} process image: {}",
                            if members.is_empty() { "initial" } else { "descendant" },
                            error.detail
                        ))
                    })
                }
            } else {
                Ok(())
            };
            close_image(created.file);
            verified?;
            if members.len() >= MAX_MEMBERS
                || members
                    .iter()
                    .any(|m| m.pid == event.pid && !m.exit_continued)
            {
                return Err(io::Error::other(
                    "invalid or oversized Windows family inventory",
                ));
            }
            let process = duplicate(created.process)?;
            let mut belongs = 0;
            if unsafe { IsProcessInJob(process.as_raw_handle(), job.as_raw_handle(), &mut belongs) }
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            if belongs == 0 {
                return Err(io::Error::other("debug process outside owned job"));
            }
            members.push(Member {
                pid: event.pid,
                process,
                exit_continued: false,
                initial_breakpoint: true,
            });
        }
        6 => {
            let file = unsafe { event.data.file };
            let verified = if let Some(image) = image.filter(|image| image.observes_dependencies())
            {
                if file.is_null() || file as isize == -1 {
                    Err(io::Error::other("loaded DLL handle is unavailable"))
                } else {
                    let actual = unsafe { BorrowedHandle::borrow_raw(file) };
                    image
                        .verify_loaded_dependency(&actual)
                        .map_err(|error| io::Error::other(error.detail))
                }
            } else {
                Ok(())
            };
            close_image(file);
            verified?;
        }
        1 => {
            let exception = unsafe { event.data.exception };
            let member = members
                .iter_mut()
                .find(|m| m.pid == event.pid && !m.exit_continued)
                .ok_or_else(|| io::Error::other("exception without retained process"))?;
            if exception.record.code == 0x80000003
                && exception.first_chance == 1
                && member.initial_breakpoint
            {
                member.initial_breakpoint = false;
            } else {
                // Preserve application exception handling; do not swallow crashes.
                return Ok(NOT_HANDLED);
            }
        }
        2 | 4 | 5 | 7 | 8 => {}
        _ => return Err(io::Error::other("unrecognized Windows debug event")),
    }
    Ok(CONTINUE)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn debug_records_match_windows_64_bit_abi() {
        assert_eq!(std::mem::size_of::<ExceptionRecord>(), 152);
        assert_eq!(std::mem::size_of::<ExceptionInfo>(), 160);
        assert_eq!(std::mem::size_of::<CreateInfo>(), 72);
        assert_eq!(std::mem::size_of::<DebugEvent>(), 176);
        assert_eq!(std::mem::offset_of!(DebugEvent, data), 16);
    }
}
