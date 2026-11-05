//! Bounded, cancelable parent endpoints for Windows Git stdio.
//! Not admitted to the product until the Windows transport/CWD gate is complete.
#![allow(dead_code)]
use std::{
    ffi::c_void,
    io::{self, Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const CHUNK: usize = 64 * 1024;
const TICK: Duration = Duration::from_millis(5);
const DRAIN: Duration = Duration::from_secs(2);
const IO_PENDING: i32 = 997;
const IO_INCOMPLETE: i32 = 996;

#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: [u32; 2],
    event: *mut c_void,
}
#[link(name = "kernel32")]
extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        open: u32,
        mode: u32,
        instances: u32,
        output: u32,
        input: u32,
        timeout: u32,
        security: *const c_void,
    ) -> *mut c_void;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn GetNamedPipeClientProcessId(pipe: *mut c_void, pid: *mut u32) -> i32;
    fn CreateEventW(
        security: *const c_void,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut Overlapped) -> i32;
    fn ReadFile(
        file: *mut c_void,
        buffer: *mut u8,
        size: u32,
        read: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn WriteFile(
        file: *mut c_void,
        buffer: *const u8,
        size: u32,
        written: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn GetOverlappedResult(
        file: *mut c_void,
        overlapped: *mut Overlapped,
        count: *mut u32,
        wait: i32,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
    fn CancelIoEx(file: *mut c_void, overlapped: *mut Overlapped) -> i32;
}
fn owned(raw: *mut c_void) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw as isize == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}

// The kernel never borrows a caller's stack or slice. This block and its handle
// stay owned until completion, including unwind and cancellation error paths.
struct Block {
    overlapped: Overlapped,
    buffer: [u8; CHUNK],
    _event: OwnedHandle,
}
struct Operation {
    handle: Option<OwnedHandle>,
    block: Option<Box<Block>>,
    pending: bool,
}
impl Operation {
    fn new(handle: &OwnedHandle) -> io::Result<Self> {
        let event = owned(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) })?;
        let overlapped = Overlapped {
            event: event.as_raw_handle(),
            ..Default::default()
        };
        Ok(Self {
            handle: Some(handle.try_clone()?),
            block: Some(Box::new(Block {
                overlapped,
                buffer: [0; CHUNK],
                _event: event,
            })),
            pending: false,
        })
    }
    fn poll(&mut self) -> Option<io::Result<usize>> {
        let mut count = 0;
        let result = unsafe {
            GetOverlappedResult(
                self.handle.as_ref().unwrap().as_raw_handle(),
                &mut self.block.as_mut().unwrap().overlapped,
                &mut count,
                0,
            )
        };
        if result != 0 {
            self.pending = false;
            Some(Ok(count as usize))
        } else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(IO_INCOMPLETE) {
                None
            } else {
                self.pending = false;
                Some(Err(error))
            }
        }
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if !self.pending {
            return;
        }
        unsafe {
            CancelIoEx(
                self.handle.as_ref().unwrap().as_raw_handle(),
                &mut self.block.as_mut().unwrap().overlapped,
            );
        }
        // CancelIoEx is a request, not completion. Even ERROR_NOT_FOUND must be
        // followed by completion observation before releasing kernel buffers.
        let deadline = Instant::now() + DRAIN;
        while self.poll().is_none() {
            if Instant::now() >= deadline {
                // No UAF and no unbounded join if a driver fails to complete.
                // The caller has already marked its endpoint unusable.
                std::mem::forget(self.block.take());
                std::mem::forget(self.handle.take());
                return;
            }
            std::thread::sleep(TICK);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Direction {
    Read,
    Write,
}
pub(super) struct BudgetPipe {
    handle: OwnedHandle,
    direction: Direction,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    external_cancel: Option<Arc<AtomicBool>>,
    broken: bool,
}
impl BudgetPipe {
    pub(super) fn pair(
        direction: Direction,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
        external_cancel: Option<Arc<AtomicBool>>,
    ) -> io::Result<(Self, OwnedHandle)> {
        let name: Vec<_> = format!(r"\\.\pipe\hello-agent-{}", uuid::Uuid::new_v4())
            .encode_utf16()
            .chain([0])
            .collect();
        // One unpredictable local instance; both original handles are non-inheritable.
        let handle = owned(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                0x40080000 | if direction == Direction::Read { 1 } else { 2 },
                8,
                1,
                CHUNK as u32,
                CHUNK as u32,
                0,
                std::ptr::null(),
            )
        })?;
        let child = owned(unsafe {
            CreateFileW(
                name.as_ptr(),
                if direction == Direction::Read {
                    0x40000000
                } else {
                    0x80000000
                },
                0,
                std::ptr::null(),
                3,
                0,
                std::ptr::null_mut(),
            )
        })?;
        let mut operation = Operation::new(&handle)?;
        operation.pending = true;
        let connected = unsafe {
            ConnectNamedPipe(
                operation.handle.as_ref().unwrap().as_raw_handle(),
                &mut operation.block.as_mut().unwrap().overlapped,
            )
        };
        if connected == 0 {
            let error = io::Error::last_os_error();
            // Our client has already connected. No asynchronous connect should
            // remain; unexpected pending is safely canceled by Operation::drop.
            operation.pending = error.raw_os_error() == Some(IO_PENDING);
            if error.raw_os_error() != Some(535) {
                return Err(error);
            }
        } else {
            operation.pending = false;
        }
        let mut client = 0;
        if unsafe { GetNamedPipeClientProcessId(handle.as_raw_handle(), &mut client) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if client != std::process::id() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "pipe client identity changed",
            ));
        }
        Ok((
            Self {
                handle,
                direction,
                deadline,
                cancelled,
                external_cancel,
                broken: false,
            },
            child,
        ))
    }
    fn check(&self) -> io::Result<()> {
        if self.broken {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "pipe operation did not complete",
            ));
        }
        if self.cancelled.load(Ordering::Acquire)
            || self
                .external_cancel
                .as_ref()
                .is_some_and(|v| v.load(Ordering::Acquire))
        {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Git owner stopped",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Git pipe deadline exceeded",
            ));
        }
        Ok(())
    }
    fn transfer(&mut self, buffer: &mut [u8], writing: bool) -> io::Result<usize> {
        self.check()?;
        if buffer.is_empty() {
            return Ok(0);
        }
        let count = buffer.len().min(CHUNK);
        let mut operation = Operation::new(&self.handle)?;
        let operation_handle = operation.handle.as_ref().unwrap().as_raw_handle();
        let block = operation.block.as_mut().unwrap();
        if writing {
            block.buffer[..count].copy_from_slice(&buffer[..count]);
        }
        operation.pending = true;
        let issued = unsafe {
            if writing {
                WriteFile(
                    operation_handle,
                    block.buffer.as_ptr(),
                    count as u32,
                    std::ptr::null_mut(),
                    &mut block.overlapped,
                )
            } else {
                ReadFile(
                    operation_handle,
                    block.buffer.as_mut_ptr(),
                    count as u32,
                    std::ptr::null_mut(),
                    &mut block.overlapped,
                )
            }
        };
        if issued == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(IO_PENDING) {
                operation.pending = false;
                if !writing && error.raw_os_error() == Some(109) {
                    return Ok(0);
                }
                return Err(error);
            }
        }
        loop {
            if let Err(error) = self.check() {
                self.broken = true;
                return Err(error); // Operation drains cancellation before freeing buffers.
            }
            if let Some(result) = operation.poll() {
                let size = match result {
                    Err(error) if !writing && error.raw_os_error() == Some(109) => 0,
                    value => value?,
                };
                if size > count {
                    self.broken = true;
                    return Err(io::Error::other("invalid pipe transfer length"));
                }
                if !writing {
                    buffer[..size]
                        .copy_from_slice(&operation.block.as_ref().unwrap().buffer[..size]);
                }
                return Ok(size);
            }
            let event = operation.block.as_ref().unwrap()._event.as_raw_handle();
            if unsafe { WaitForSingleObject(event, TICK.as_millis() as u32) } == u32::MAX {
                self.broken = true;
                return Err(io::Error::last_os_error());
            }
        }
    }
}
impl Read for BudgetPipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.direction != Direction::Read {
            return Err(io::Error::other("pipe is not readable"));
        }
        self.transfer(buffer, false)
    }
}
impl Write for BudgetPipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.direction != Direction::Write {
            return Err(io::Error::other("pipe is not writable"));
        }
        // Only one bounded chunk is copied, never the whole caller allocation.
        self.transfer(&mut buffer[..buffer.len().min(CHUNK)].to_vec(), true)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.check()
    } // No unbounded FlushFileBuffers.
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pair(direction: Direction, budget: Duration) -> (BudgetPipe, std::fs::File) {
        let (pipe, child) = BudgetPipe::pair(
            direction,
            Instant::now() + budget,
            Arc::new(AtomicBool::new(false)),
            None,
        )
        .unwrap();
        (pipe, std::fs::File::from(child))
    }
    #[test]
    fn bytes_and_eof_round_trip() {
        let (mut pipe, mut child) = pair(Direction::Read, Duration::from_secs(5));
        child.write_all(b"pipe bytes").unwrap();
        drop(child);
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"pipe bytes");
        let (mut pipe, mut child) = pair(Direction::Write, Duration::from_secs(5));
        pipe.write_all(b"input bytes").unwrap();
        drop(pipe);
        let mut bytes = Vec::new();
        child.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"input bytes");
    }
    #[test]
    fn stalled_read_and_write_obey_shared_deadline() {
        for direction in [Direction::Read, Direction::Write] {
            let (mut pipe, _held_child) = pair(direction, Duration::from_millis(50));
            // Exclude pipe creation/scheduler time from the deliberate stalled I/O budget.
            pipe.deadline = Instant::now() + Duration::from_millis(50);
            let start = Instant::now();
            let error = if direction == Direction::Read {
                pipe.read_exact(&mut [0]).unwrap_err()
            } else {
                pipe.write_all(&vec![1; CHUNK * 4]).unwrap_err()
            };
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
            assert!(start.elapsed() < Duration::from_secs(3));
            assert_eq!(pipe.check().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        }
    }
    #[test]
    fn cancellation_does_not_trigger_read_exact_interrupted_retry_loop() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (mut pipe, _held_child) = BudgetPipe::pair(
            Direction::Read,
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            Some(cancel.clone()),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cancel.store(true, Ordering::Release);
        });
        let start = Instant::now();
        assert_eq!(
            pipe.read_exact(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert!(start.elapsed() < Duration::from_secs(3));
        worker.join().unwrap();
    }
}
