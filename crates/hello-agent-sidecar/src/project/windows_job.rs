//! Windows process-family ownership. Not yet connected to Git pipes/admission.
//! Job membership is atomic with creation, before the primary thread can run.
//! https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
#![allow(dead_code)] // Gated foundation until Windows Git transport is connected.

fn quoted(argument: &str) -> String {
    let mut output = String::from("\"");
    let mut slashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            slashes += 1;
            continue;
        }
        output.extend(std::iter::repeat_n(
            '\\',
            if character == '"' {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        output.push(character);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n('\\', slashes * 2));
    output.push('"');
    output
}

#[cfg(windows)]
#[path = "windows_supervisor.rs"]
mod supervisor;

#[cfg(windows)]
pub(super) mod native {
    use super::{quoted, supervisor::Supervisor};
    use crate::project::windows_directory_lease::{DirectoryLease, RetainedDirectory};
    use std::{
        alloc::{alloc_zeroed, dealloc, Layout},
        collections::BTreeMap,
        ffi::c_void,
        io,
        os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
        ptr::NonNull,
        time::{Duration, Instant},
    };

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        minimum: usize,
        maximum: usize,
        active: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Limits {
        basic: BasicLimits,
        io: [u64; 6],
        memory: [usize; 4],
    }
    #[repr(C)]
    #[derive(Default)]
    struct Accounting {
        times: [i64; 4],
        faults: u32,
        total: u32,
        active: u32,
        terminated: u32,
    }
    #[repr(C)]
    struct Startup {
        size: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        position: [u32; 7],
        flags: u32,
        show: u16,
        reserved_size: u16,
        reserved_bytes: *mut u8,
        stdin: *mut c_void,
        stdout: *mut c_void,
        stderr: *mut c_void,
    }
    #[repr(C)]
    struct StartupEx {
        startup: Startup,
        attributes: *mut c_void,
    }
    #[repr(C)]
    struct ProcessInfo {
        process: *mut c_void,
        thread: *mut c_void,
        pid: u32,
        tid: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn DuplicateHandle(
            source_process: *mut c_void,
            source: *mut c_void,
            target_process: *mut c_void,
            target: *mut *mut c_void,
            access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
        fn CreateJobObjectW(security: *const c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(
            job: *mut c_void,
            class: i32,
            data: *const c_void,
            size: u32,
        ) -> i32;
        fn QueryInformationJobObject(
            job: *mut c_void,
            class: i32,
            data: *mut c_void,
            size: u32,
            returned: *mut u32,
        ) -> i32;
        fn InitializeProcThreadAttributeList(
            list: *mut c_void,
            count: u32,
            flags: u32,
            size: *mut usize,
        ) -> i32;
        fn UpdateProcThreadAttribute(
            list: *mut c_void,
            flags: u32,
            attribute: usize,
            value: *const c_void,
            size: usize,
            previous: *mut c_void,
            returned: *mut usize,
        ) -> i32;
        fn DeleteProcThreadAttributeList(list: *mut c_void);
        fn CreateProcessW(
            application: *const u16,
            command: *mut u16,
            process_security: *const c_void,
            thread_security: *const c_void,
            inherit: i32,
            flags: u32,
            environment: *const c_void,
            directory: *const u16,
            startup: *const StartupEx,
            info: *mut ProcessInfo,
        ) -> i32;
        fn ResumeThread(thread: *mut c_void) -> u32;
        fn TerminateJobObject(job: *mut c_void, code: u32) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
        fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
    }

    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message)
    }
    fn wide(value: &str) -> io::Result<Vec<u16>> {
        if value.contains('\0') {
            return Err(invalid("native process text contains NUL"));
        }
        let mut result: Vec<_> = value.encode_utf16().collect();
        if result.len() >= 32767 {
            return Err(invalid("native process text exceeds bounds"));
        }
        result.push(0);
        Ok(result)
    }

    struct Attributes {
        pointer: NonNull<c_void>,
        layout: Layout,
        initialized: bool,
    }
    impl Attributes {
        fn new(count: u32) -> io::Result<Self> {
            let mut size = 0;
            // SAFETY: the first call only queries the required allocation size.
            let first = unsafe {
                InitializeProcThreadAttributeList(std::ptr::null_mut(), count, 0, &mut size)
            };
            if first != 0
                || io::Error::last_os_error().raw_os_error() != Some(122)
                || size == 0
                || size > 65536
            {
                return Err(invalid("invalid process attribute buffer size"));
            }
            let layout = Layout::from_size_align(size, 16)
                .map_err(|_| invalid("invalid attribute alignment"))?;
            let pointer = NonNull::new(unsafe { alloc_zeroed(layout) }.cast())
                .ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
            let mut result = Self {
                pointer,
                layout,
                initialized: false,
            };
            // SAFETY: the allocation has the requested size and native alignment.
            if unsafe { InitializeProcThreadAttributeList(pointer.as_ptr(), count, 0, &mut size) }
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            result.initialized = true;
            Ok(result)
        }
    }
    impl Drop for Attributes {
        fn drop(&mut self) {
            unsafe {
                if self.initialized {
                    DeleteProcThreadAttributeList(self.pointer.as_ptr());
                }
                dealloc(self.pointer.as_ptr().cast(), self.layout);
            }
        }
    }

    pub(crate) struct Family {
        job: OwnedHandle,
        process: OwnedHandle,
        thread: OwnedHandle,
        started: bool,
        supervisor: Supervisor,
        directory: Option<RetainedDirectory>,
    }

    pub(super) struct Launch {
        pub(super) job: OwnedHandle,
        pub(super) process: OwnedHandle,
        pub(super) thread: OwnedHandle,
        pub(super) directory: Option<RetainedDirectory>,
    }

    pub(super) fn accounting(job: &OwnedHandle) -> io::Result<(u32, u32)> {
        let mut value = Accounting::default();
        let mut returned = 0;
        if unsafe {
            QueryInformationJobObject(
                job.as_raw_handle(),
                1,
                (&mut value as *mut Accounting).cast(),
                std::mem::size_of::<Accounting>() as u32,
                &mut returned,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if returned as usize != std::mem::size_of::<Accounting>() {
            return Err(io::Error::other("incomplete job accounting"));
        }
        Ok((value.total, value.active))
    }

    impl Family {
        /// Caller must persist its launch fence before creating this owner.
        /// This supplies no Git CWD authority or redirected pipe transport yet.
        pub(crate) fn suspended(
            executable: &str,
            arguments: &[String],
            environment: &BTreeMap<String, String>,
            directory: &str,
        ) -> io::Result<Self> {
            Self::suspended_with_stdio(executable, arguments, environment, directory, None)
        }

        /// Only these three duplicated handles are inherited by this child.
        /// The sidecar spawn lock excludes its other Windows launch paths while
        /// the temporary inheritable duplicates exist.
        /// Pipe deadlines/cancellation and CWD authority belong to the caller.
        pub(crate) fn suspended_with_stdio(
            executable: &str,
            arguments: &[String],
            environment: &BTreeMap<String, String>,
            directory: &str,
            stdio: Option<[BorrowedHandle<'_>; 3]>,
        ) -> io::Result<Self> {
            Self::suspended_inner(executable, arguments, environment, directory, stdio, None)
        }

        /// CWD binding only. Executable authority/product admission is separate.
        pub(crate) fn suspended_in_lease(
            executable: &str,
            arguments: &[String],
            environment: &BTreeMap<String, String>,
            directory: DirectoryLease,
            stdio: Option<[BorrowedHandle<'_>; 3]>,
        ) -> io::Result<Self> {
            let path = directory.path().to_owned();
            Self::suspended_inner(
                executable,
                arguments,
                environment,
                &path,
                stdio,
                Some(RetainedDirectory::new(directory)),
            )
        }

        fn suspended_inner(
            executable: &str,
            arguments: &[String],
            environment: &BTreeMap<String, String>,
            directory: &str,
            stdio: Option<[BorrowedHandle<'_>; 3]>,
            retained: Option<RetainedDirectory>,
        ) -> io::Result<Self> {
            // Borrowed stdio may not cross the creator thread's lifetime.
            // These clones stay non-inheritable; only Launch::create duplicates
            // them as inheritable while holding the common spawn mutex.
            let handles = stdio
                .map(|handles| {
                    handles
                        .into_iter()
                        .map(|h| h.try_clone_to_owned())
                        .collect::<io::Result<Vec<_>>>()
                })
                .transpose()?;
            let executable = executable.to_owned();
            let arguments = arguments.to_owned();
            let environment = environment.to_owned();
            let directory = directory.to_owned();
            let (launch, supervisor) = Supervisor::spawn(move || {
                let stdio = handles
                    .as_ref()
                    .map(|h| [h[0].as_handle(), h[1].as_handle(), h[2].as_handle()]);
                Launch::create(
                    &executable,
                    &arguments,
                    &environment,
                    &directory,
                    stdio,
                    retained,
                )
            })?;
            Ok(Self {
                job: launch.job,
                process: launch.process,
                thread: launch.thread,
                started: false,
                supervisor,
                directory: launch.directory,
            })
        }

        pub(crate) fn start(&mut self) -> io::Result<()> {
            if self.started {
                return Err(invalid("primary thread has already been resumed"));
            }
            self.supervisor.check()?;
            if let Some(directory) = &self.directory {
                directory
                    .validate()
                    .map_err(|error| io::Error::other(error.detail))?;
            }
            self.started = true;
            if unsafe { ResumeThread(self.thread.as_raw_handle()) } != 1 {
                return Err(io::Error::other(
                    "primary thread did not have exactly one suspension",
                ));
            }
            Ok(())
        }

        fn active(&self) -> io::Result<u32> {
            accounting(&self.job).map(|(_, active)| active)
        }

        pub(crate) fn completed(&self) -> io::Result<Option<u32>> {
            self.supervisor.completed()
        }

        /// Only the supervisor's retained-handle barrier can return true.
        pub(crate) fn terminate(&self, budget: Duration) -> io::Result<bool> {
            if self.completed()?.is_some() {
                return Ok(true);
            }
            if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let deadline = Instant::now()
                .checked_add(budget)
                .ok_or_else(|| invalid("termination budget overflow"))?;
            loop {
                if self.completed()?.is_some() {
                    return Ok(true);
                }
                if Instant::now() >= deadline {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Launch {
        fn create(
            executable: &str,
            arguments: &[String],
            environment: &BTreeMap<String, String>,
            directory: &str,
            stdio: Option<[BorrowedHandle<'_>; 3]>,
            mut retained: Option<RetainedDirectory>,
        ) -> io::Result<Self> {
            crate::project::protocol_path::from_native(executable, true).map_err(invalid)?;
            if let Some(pin) = &retained {
                if pin.path() != directory {
                    return Err(invalid("CWD lease path mismatch"));
                }
                pin.validate()
                    .map_err(|error| io::Error::other(error.detail))?;
            } else {
                // Raw GUID paths cannot enter the diagnostic string path.
                crate::project::protocol_path::from_native(directory, true).map_err(invalid)?;
            }
            if arguments.len() > 512 {
                return Err(invalid("too many native process arguments"));
            }
            let executable_wide = wide(executable)?;
            let directory_wide = wide(directory)?;
            let mut command = wide(
                &std::iter::once(executable)
                    .chain(arguments.iter().map(String::as_str))
                    .map(quoted)
                    .collect::<Vec<_>>()
                    .join(" "),
            )?;
            let mut sorted = BTreeMap::new();
            for (key, value) in environment {
                if key.is_empty()
                    || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    || value.contains('\0')
                    || sorted.insert(key.to_ascii_uppercase(), value).is_some()
                {
                    return Err(invalid("invalid or case-duplicate environment key"));
                }
            }
            let mut env = Vec::new();
            for (key, value) in sorted {
                env.extend(wide(&format!("{key}={value}"))?);
            }
            env.push(0);
            if env.len() == 1 {
                env.push(0);
            }
            if env.len() > 32767 {
                return Err(invalid("native environment exceeds bounds"));
            }
            // Null security attributes make the unnamed owner handle non-inheritable.
            let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw_job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
            let mut limits = Limits::default();
            limits.basic.flags = 0x2000; // KILL_ON_JOB_CLOSE; no breakaway flags.
            if unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    9,
                    (&limits as *const Limits).cast(),
                    std::mem::size_of::<Limits>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // Shared with all admitted Windows std::process launch paths.
            // Declared before duplicates so it is released after they close.
            let _spawn_guard = crate::process_spawn::lock()?;
            // Duplicates are temporary; the caller's original inheritance flags
            // are never modified. All failure paths close partial duplicates.
            let mut inherited = Vec::new();
            if let Some(handles) = stdio {
                for handle in handles {
                    let mut duplicate = std::ptr::null_mut();
                    let process = unsafe { GetCurrentProcess() };
                    if unsafe {
                        DuplicateHandle(
                            process,
                            handle.as_raw_handle(),
                            process,
                            &mut duplicate,
                            0,
                            1,
                            2,
                        ) // DUPLICATE_SAME_ACCESS
                    } == 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    inherited.push(unsafe { OwnedHandle::from_raw_handle(duplicate) });
                }
            }
            let handles: Vec<_> = inherited.iter().map(AsRawHandle::as_raw_handle).collect();
            let jobs = [job.as_raw_handle()];
            let attributes = Attributes::new(if handles.is_empty() { 1 } else { 2 })?;
            if !handles.is_empty()
                && unsafe {
                    UpdateProcThreadAttribute(
                        attributes.pointer.as_ptr(),
                        0,
                        0x00020002,
                        handles.as_ptr().cast(),
                        std::mem::size_of_val(handles.as_slice()),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // The handle array stays alive until after the attribute list drops.
            if unsafe {
                UpdateProcThreadAttribute(
                    attributes.pointer.as_ptr(),
                    0,
                    0x0002000d,
                    jobs.as_ptr().cast(),
                    std::mem::size_of_val(&jobs),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: these C records contain only integer and pointer fields.
            let mut startup: StartupEx = unsafe { std::mem::zeroed() };
            startup.startup.size = std::mem::size_of::<StartupEx>() as u32;
            // STARTF_USESTDHANDLES requires valid explicit inherited handles.
            // Without redirection, leave this flag clear and inheritance disabled.
            if !handles.is_empty() {
                startup.startup.flags = 0x100;
                startup.startup.stdin = handles[0];
                startup.startup.stdout = handles[1];
                startup.startup.stderr = handles[2];
            }
            startup.attributes = attributes.pointer.as_ptr();
            let mut info: ProcessInfo = unsafe { std::mem::zeroed() };
            // EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT |
            // CREATE_SUSPENDED | CREATE_NO_WINDOW. Membership is atomic with creation.
            if let Some(pin) = &retained {
                pin.validate()
                    .map_err(|error| io::Error::other(error.detail))?;
            }
            let ok = unsafe {
                CreateProcessW(
                    executable_wide.as_ptr(),
                    command.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    i32::from(!handles.is_empty()),
                    0x08080405, // DEBUG_PROCESS: creator thread pumps every descendant event.
                    env.as_ptr().cast(),
                    directory_wide.as_ptr(),
                    &startup,
                    &mut info,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            if let Some(pin) = &mut retained {
                pin.arm();
            }
            // Successful creation transfers both handles to the creator thread.
            // All debug events remain on that thread through final completion.
            Ok(Self {
                job,
                process: unsafe { OwnedHandle::from_raw_handle(info.process) },
                thread: unsafe { OwnedHandle::from_raw_handle(info.thread) },
                directory: retained,
            })
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::process_spawn::SpawnGuarded as _;
        const FIXTURE: &str = "project::windows_job::native::tests::family_fixture";
        #[test]
        fn family_fixture() {
            let mode = std::env::var("HELLO_AGENT_JOB_FIXTURE").unwrap_or_default();
            if mode == "parent" {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", FIXTURE])
                    .env_clear()
                    .env("SystemRoot", std::env::var("SystemRoot").unwrap())
                    .env("HELLO_AGENT_JOB_FIXTURE", "child")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn_guarded()
                    .unwrap();
                std::fs::write("fixture-child.pid", child.id().to_string()).unwrap();
            } else if mode == "short-children" {
                let mut child_ids = Vec::new();
                for _ in 0..16 {
                    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", FIXTURE])
                        .env_clear()
                        .env("SystemRoot", std::env::var("SystemRoot").unwrap())
                        .env("HELLO_AGENT_JOB_FIXTURE", "output")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn_guarded()
                        .unwrap();
                    child_ids.push(child.id().to_string());
                    assert!(child.wait().unwrap().success());
                }
                std::fs::write("completed-children.pids", child_ids.join("\n")).unwrap();
            } else if mode == "nested-debugger" {
                use std::os::windows::process::CommandExt;
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", FIXTURE])
                    .env_clear()
                    .env("SystemRoot", std::env::var("SystemRoot").unwrap())
                    .env("HELLO_AGENT_JOB_FIXTURE", "child")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .creation_flags(0x3) // Break the inherited debug chain.
                    .spawn_guarded()
                    .unwrap();
                std::fs::write("nested-child.pid", child.id().to_string()).unwrap();
                // This debugger thread's exit terminates its own debug child.
                // The outer supervisor must still reject the missing inventory.
            } else if mode == "retention-unwind" {
                use cap_std::{ambient_authority, fs::Dir};
                use std::{ffi::OsStr, io::Write};
                let dir =
                    Dir::open_ambient_dir(std::env::current_dir().unwrap(), ambient_authority())
                        .unwrap();
                let mut retained = RetainedDirectory::new(
                    DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap(),
                );
                retained.arm();
                let failure = std::panic::catch_unwind(move || {
                    let _pin = retained;
                    panic!("injected unconfirmed owner failure");
                });
                assert!(failure.is_err());
                assert_eq!(
                    std::fs::remove_file("guard").unwrap_err().raw_os_error(),
                    Some(32)
                );
                std::io::stdout().write_all(b"HELLO_RETAINED").unwrap();
                // The retained pin is released by process exit, not unwinding.
            } else if mode == "cwd" {
                use std::io::Write;
                assert_eq!(std::fs::read("cwd-marker").unwrap(), b"owned-directory");
                std::io::stdout().write_all(b"HELLO_CWD_OK").unwrap();
            } else if mode == "output" {
                use std::io::Write;
                std::io::stdout().write_all(b"HELLO_JOB_STDOUT").unwrap();
                std::io::stderr().write_all(b"HELLO_JOB_STDERR").unwrap();
            } else if mode == "child" {
                std::thread::sleep(Duration::from_secs(30));
            }
        }
        fn fixture_owner(directory: &str) -> Family {
            fixture_mode(directory, "parent")
        }
        fn fixture_mode(directory: &str, mode: &str) -> Family {
            use std::os::windows::io::AsHandle;
            // The Rust test harness expects valid standard handles even when
            // no output is captured. Give the fixture explicit NUL endpoints.
            let null = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("NUL")
                .unwrap();
            let environment = BTreeMap::from([
                ("SystemRoot".into(), std::env::var("SystemRoot").unwrap()),
                ("HELLO_AGENT_JOB_FIXTURE".into(), mode.into()),
            ]);
            Family::suspended_with_stdio(
                std::env::current_exe().unwrap().to_str().unwrap(),
                &["--exact".into(), FIXTURE.into()],
                &environment,
                directory,
                Some([null.as_handle(), null.as_handle(), null.as_handle()]),
            )
            .unwrap()
        }
        #[test]
        fn rejected_process_creation_releases_unarmed_directory_pins() {
            use cap_std::{ambient_authority, fs::Dir};
            use std::ffi::OsStr;
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("guard"), b"pin").unwrap();
            let dir = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
            let lease = DirectoryLease::acquire(&dir, OsStr::new("guard")).unwrap();
            let missing = root.path().canonicalize().unwrap().join("missing.exe");
            assert!(Family::suspended_in_lease(
                missing.to_str().unwrap(),
                &[],
                &BTreeMap::new(),
                lease,
                None
            )
            .is_err());
            std::fs::remove_file(root.path().join("guard")).unwrap();
        }

        #[test]
        fn explicit_standard_handles_receive_output() {
            use std::os::windows::io::AsHandle;
            let directory = tempfile::tempdir().unwrap();
            let input = std::fs::File::open("NUL").unwrap();
            let stdout = tempfile::NamedTempFile::new().unwrap();
            let stderr = tempfile::NamedTempFile::new().unwrap();
            let environment = BTreeMap::from([
                ("SystemRoot".into(), std::env::var("SystemRoot").unwrap()),
                ("HELLO_AGENT_JOB_FIXTURE".into(), "output".into()),
            ]);
            let mut owner = Family::suspended_with_stdio(
                std::env::current_exe().unwrap().to_str().unwrap(),
                &["--exact".into(), FIXTURE.into(), "--nocapture".into()],
                &environment,
                directory.path().canonicalize().unwrap().to_str().unwrap(),
                Some([
                    input.as_handle(),
                    stdout.as_file().as_handle(),
                    stderr.as_file().as_handle(),
                ]),
            )
            .unwrap();
            owner.start().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(code) = owner.completed().unwrap() {
                    assert_eq!(code, 0);
                    break;
                }
                assert!(Instant::now() < deadline, "output fixture did not exit");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(std::fs::read_to_string(stdout.path())
                .unwrap()
                .contains("HELLO_JOB_STDOUT"));
            assert!(std::fs::read_to_string(stderr.path())
                .unwrap()
                .contains("HELLO_JOB_STDERR"));
        }

        #[test]
        fn short_lived_descendants_remain_in_completion_inventory() {
            let directory = tempfile::tempdir().unwrap();
            let mut owner = fixture_mode(
                directory.path().canonicalize().unwrap().to_str().unwrap(),
                "short-children",
            );
            owner.start().unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if let Some(code) = owner.completed().unwrap() {
                    assert_eq!(code, 0);
                    let (total, active) = accounting(&owner.job).unwrap();
                    // Windows may add another owned process. The supervisor
                    // must retain every Job member, not only our 16 children.
                    assert!(total >= 17, "missing fixture family members: {total}");
                    let pids =
                        std::fs::read_to_string(directory.path().join("completed-children.pids"))
                            .unwrap();
                    let pids: Vec<u32> = pids.lines().map(|p| p.parse().unwrap()).collect();
                    assert_eq!(pids.len(), 16);
                    assert!(pids.iter().all(|pid| *pid != 0));
                    assert_eq!(active, 0);
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "short-lived family did not complete"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        #[test]
        fn nested_debugger_cannot_hide_untracked_job_members() {
            let directory = tempfile::tempdir().unwrap();
            let mut owner = fixture_mode(
                directory.path().canonicalize().unwrap().to_str().unwrap(),
                "nested-debugger",
            );
            owner.start().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match owner.completed() {
                    Err(error) => {
                        assert!(
                            error
                                .to_string()
                                .contains("incomplete Windows family inventory"),
                            "{error}"
                        );
                        assert!(directory.path().join("nested-child.pid").exists());
                        break;
                    }
                    Ok(None) => {}
                    Ok(Some(code)) => panic!("untracked child authorized completion: {code}"),
                }
                assert!(
                    Instant::now() < deadline,
                    "untracked child was not rejected"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        #[test]
        #[cfg(target_pointer_width = "64")]
        fn native_record_layout_matches_windows_64_bit_abi() {
            assert_eq!(std::mem::size_of::<BasicLimits>(), 64);
            assert_eq!(std::mem::size_of::<Limits>(), 144);
            assert_eq!(std::mem::size_of::<Accounting>(), 48);
            assert_eq!(std::mem::size_of::<Startup>(), 104);
            assert_eq!(std::mem::size_of::<StartupEx>(), 112);
            assert_eq!(std::mem::size_of::<ProcessInfo>(), 24);
        }

        #[test]
        fn suspended_process_is_owned_before_first_instruction() {
            let directory = tempfile::tempdir().unwrap();
            let owner = Family::suspended(
                std::env::current_exe().unwrap().to_str().unwrap(),
                &["--exact".into(), FIXTURE.into()],
                &BTreeMap::from([("SystemRoot".into(), std::env::var("SystemRoot").unwrap())]),
                directory.path().canonicalize().unwrap().to_str().unwrap(),
            )
            .unwrap();
            assert_eq!(owner.active().unwrap(), 1);
            assert_eq!(owner.completed().unwrap(), None);
            assert!(owner.terminate(Duration::from_secs(5)).unwrap());
        }
        #[test]
        fn parent_exit_does_not_hide_a_live_descendant() {
            let directory = tempfile::tempdir().unwrap();
            let mut owner =
                fixture_owner(directory.path().canonicalize().unwrap().to_str().unwrap());
            owner.start().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if unsafe { WaitForSingleObject(owner.process.as_raw_handle(), 0) } == 0
                    && owner.active().unwrap() > 0
                {
                    break;
                }
                assert_eq!(
                    owner.completed().unwrap(),
                    None,
                    "fixture exited without a retained child (parent exit code)"
                );
                assert!(
                    Instant::now() < deadline,
                    "fixture parent/child state was not observed: parent signal {}, active {}",
                    unsafe { WaitForSingleObject(owner.process.as_raw_handle(), 0) },
                    owner.active().unwrap()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let mut parent_code = 0;
            assert_ne!(
                unsafe { GetExitCodeProcess(owner.process.as_raw_handle(), &mut parent_code) },
                0
            );
            assert_eq!(parent_code, 0);
            // The job may include console-host processes as well. Bind to the
            // actual spawned fixture PID instead of assuming exactly one member.
            #[link(name = "kernel32")]
            extern "system" {
                fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
            }
            let pid: u32 = std::fs::read_to_string(directory.path().join("fixture-child.pid"))
                .unwrap()
                .parse()
                .unwrap();
            let raw = unsafe { OpenProcess(0x00100000, 0, pid) }; // SYNCHRONIZE only
            assert!(!raw.is_null(), "{}", io::Error::last_os_error());
            let child = unsafe { OwnedHandle::from_raw_handle(raw) };
            assert_eq!(
                unsafe { WaitForSingleObject(child.as_raw_handle(), 0) },
                258
            );
            assert_eq!(owner.completed().unwrap(), None);
            assert!(owner.start().is_err());
            assert!(owner.terminate(Duration::from_secs(5)).unwrap());
            assert!(owner.completed().unwrap().is_some());
            assert_eq!(unsafe { WaitForSingleObject(child.as_raw_handle(), 0) }, 0);
        }
    }

    // Dropping the unnamed job closes its only inheritable-free handle and
    // requests kernel cleanup. Only the supervisor barrier proves completion;
    // Drop itself is never evidence that descendants have terminated.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_escape_quotes_and_trailing_backslashes() {
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("a b"), "\"a b\"");
        assert_eq!(quoted("a\"b"), "\"a\\\"b\"");
        assert_eq!(quoted("C:\\folder\\"), "\"C:\\folder\\\\\"");
    }
}
