//! A transcriber and its descendants enter an unnamed, non-inherited kill-on-close
//! job atomically at CreateProcessW. No suspended/spawn-then-assign gap or nightly
//! Rust APIs. stdin/stdout are explicit pipes; no recording is sent during spawn.
use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::ExitStatusExt,
    },
    path::Path,
    process::{Command, ExitStatus},
};
use windows::{
    Win32::{
        Foundation::{
            HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation, WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        System::{
            JobObjects::{
                CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Pipes::CreatePipe,
            Threading::{
                CREATE_NO_WINDOW, CreateProcessW, DeleteProcThreadAttributeList,
                EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE,
                InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
                PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
                UpdateProcThreadAttribute, WaitForSingleObject,
            },
        },
    },
    core::{BOOL, PCWSTR, PWSTR},
};

fn failure(operation: &'static str, error: windows::core::Error) -> io::Error {
    io::Error::other(format!("{operation} failed ({:#x})", error.code().0))
}
fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// Job is dropped first, before its process/pipes, even on an early error return.
pub(crate) struct Child {
    job: Option<OwnedHandle>,
    process: OwnedHandle,
    pub stdin: Option<File>,
    pub stdout: Option<File>,
}
impl Child {
    pub fn kill(&mut self) -> io::Result<()> {
        match &self.job {
            Some(job) => unsafe { TerminateJobObject(raw(job), 1) }
                .map_err(|e| failure("Terminate transcriber job", e)),
            None => Ok(()),
        }
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match unsafe { WaitForSingleObject(raw(&self.process), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => self.finished().map(Some),
            _ => Err(io::Error::other(
                "Could not inspect the transcriber process",
            )),
        }
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if unsafe { WaitForSingleObject(raw(&self.process), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::other(
                "Could not wait for the transcriber process",
            ));
        }
        self.finished()
    }
    fn finished(&mut self) -> io::Result<ExitStatus> {
        let status = self.status();
        // An exited transcriber cannot leave descendants holding pipes open
        // while the caller joins its stdin/stdout worker threads.
        self.job.take();
        status
    }
    fn status(&self) -> io::Result<ExitStatus> {
        let mut code = 0;
        unsafe { GetExitCodeProcess(raw(&self.process), &mut code) }
            .map_err(|e| failure("Read transcriber status", e))?;
        Ok(ExitStatus::from_raw(code))
    }
}

// PID and bounded teardown also support the explicit installed-window capture
// verifier. Enumeration/membership helpers stay test-only; no raw job handle is
// exposed, and ordinary transcription behavior is unchanged.
impl Child {
    pub(crate) fn id(&self) -> io::Result<u32> {
        let pid = unsafe { windows::Win32::System::Threading::GetProcessId(raw(&self.process)) };
        if pid == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(pid)
        }
    }
    pub(crate) fn is_running(&self) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(raw(&self.process), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(io::Error::other("Could not inspect owned fixture process")),
        }
    }
    #[cfg(test)]
    pub(crate) fn contains_pid(&self, pid: u32) -> io::Result<bool> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let job = self
            .job
            .as_ref()
            .ok_or_else(|| io::Error::other("Owned fixture job is closed"))?;
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|error| failure("Inspect fixture process", error))?;
        let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
        let mut belongs = BOOL::default();
        unsafe { IsProcessInJob(raw(&process), Some(raw(job)), &mut belongs) }
            .map_err(|error| failure("Check fixture job membership", error))?;
        Ok(belongs.as_bool())
    }
    #[cfg(test)]
    pub(crate) fn job_pids(&self) -> io::Result<Vec<u32>> {
        use windows::Win32::System::JobObjects::{
            JobObjectBasicProcessIdList, QueryInformationJobObject,
        };
        const MAX_PROCESSES: usize = 256;
        // Same header/ULONG_PTR array layout as JOBOBJECT_BASIC_PROCESS_ID_LIST,
        // with a fixed upper bound. Never return a truncated membership list.
        #[repr(C)]
        struct ProcessList {
            assigned: u32,
            count: u32,
            pids: [usize; MAX_PROCESSES],
        }
        let job = self
            .job
            .as_ref()
            .ok_or_else(|| io::Error::other("Owned fixture job is closed"))?;
        let mut list = ProcessList {
            assigned: 0,
            count: 0,
            pids: [0; MAX_PROCESSES],
        };
        unsafe {
            QueryInformationJobObject(
                Some(raw(job)),
                JobObjectBasicProcessIdList,
                (&mut list as *mut ProcessList).cast(),
                std::mem::size_of::<ProcessList>() as u32,
                None,
            )
        }
        .map_err(|error| failure("Read fixture job members", error))?;
        if list.count as usize > MAX_PROCESSES || list.assigned != list.count {
            return Err(io::Error::other(
                "Owned fixture job membership exceeded its bound",
            ));
        }
        let mut pids = list.pids[..list.count as usize]
            .iter()
            .map(|pid| {
                u32::try_from(*pid)
                    .ok()
                    .filter(|pid| *pid != 0)
                    .ok_or_else(|| io::Error::other("Invalid owned fixture process identifier"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        pids.sort_unstable();
        if pids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(io::Error::other(
                "Duplicate owned fixture process identifier",
            ));
        }
        Ok(pids)
    }
    pub(crate) fn stop_and_wait(&mut self, timeout: std::time::Duration) -> io::Result<()> {
        self.drain_job(timeout, true).map(|_| ())
    }
    pub(crate) fn wait_for_exit(&mut self, timeout: std::time::Duration) -> io::Result<ExitStatus> {
        self.drain_job(timeout, false)
    }
    fn drain_job(
        &mut self,
        timeout: std::time::Duration,
        terminate: bool,
    ) -> io::Result<ExitStatus> {
        use windows::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
            QueryInformationJobObject,
        };
        let deadline = std::time::Instant::now() + timeout.min(std::time::Duration::from_secs(10));
        let job = self
            .job
            .as_ref()
            .ok_or_else(|| io::Error::other("Owned fixture job is closed"))?;
        self.stdin.take();
        if terminate {
            unsafe { TerminateJobObject(raw(job), 1) }
                .map_err(|error| failure("Stop owned fixture job", error))?;
        }
        loop {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            unsafe {
                QueryInformationJobObject(
                    Some(raw(job)),
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&accounting) as u32,
                    None,
                )
            }
            .map_err(|error| failure("Read stopped fixture job", error))?;
            let root = unsafe { WaitForSingleObject(raw(&self.process), 0) };
            if root != WAIT_OBJECT_0 && root != WAIT_TIMEOUT {
                return Err(io::Error::other("Could not confirm owned fixture exit"));
            }
            if root == WAIT_OBJECT_0 && accounting.ActiveProcesses == 0 {
                let status = self.status()?;
                self.job.take();
                self.stdout.take();
                return Ok(status);
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                // Retain the job: Drop still kills only this exact contained
                // process tree. Never report successful profile cleanup yet.
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned fixture job did not stop in time",
                ));
            }
            std::thread::sleep(remaining.min(std::time::Duration::from_millis(20)));
        }
    }
}

struct Attributes {
    _storage: Vec<usize>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}
impl Attributes {
    fn new() -> io::Result<Self> {
        let mut bytes = 0;
        // The first call reports the required allocation by design.
        let _ = unsafe { InitializeProcThreadAttributeList(None, 2, None, &mut bytes) };
        if bytes == 0 || bytes > 65_536 {
            return Err(io::Error::other("Invalid process attribute allocation"));
        }
        let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
        let pointer = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        unsafe { InitializeProcThreadAttributeList(Some(pointer), 2, None, &mut bytes) }
            .map_err(|e| failure("Initialize process attributes", e))?;
        Ok(Self {
            _storage: storage,
            pointer,
        })
    }
    fn handles(&self, kind: u32, values: &[HANDLE]) -> io::Result<()> {
        // Caller keeps both handle arrays alive until this attribute list is dropped.
        unsafe {
            UpdateProcThreadAttribute(
                self.pointer,
                0,
                kind as usize,
                Some(values.as_ptr().cast()),
                std::mem::size_of_val(values),
                None,
                None,
            )
        }
        .map_err(|e| failure("Set contained process attributes", e))
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.pointer) }
    }
}

fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    unsafe { CreatePipe(&mut read, &mut write, None, 0) }
        .map_err(|e| failure("Create transcriber pipe", e))?;
    // CreatePipe succeeded and transferred two unique handles to us.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read.0),
            OwnedHandle::from_raw_handle(write.0),
        )
    })
}
fn inherit(handle: &OwnedHandle, enabled: bool) -> io::Result<()> {
    unsafe {
        SetHandleInformation(
            raw(handle),
            HANDLE_FLAG_INHERIT.0,
            if enabled {
                HANDLE_FLAG_INHERIT
            } else {
                HANDLE_FLAGS(0)
            },
        )
    }
    .map_err(|e| failure("Configure transcriber handle inheritance", e))
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let value: Vec<_> = value.encode_wide().take(32_767).collect();
    if value.len() >= 32_767 || value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid transcriber argument",
        ));
    }
    Ok(value)
}
fn command_line(command: &Command) -> io::Result<Vec<u16>> {
    let mut line = Vec::new();
    for argument in std::iter::once(command.get_program()).chain(command.get_args()) {
        if !line.is_empty() {
            line.push(32);
        }
        line.push(34);
        let mut slashes = 0;
        for unit in wide(argument)? {
            if unit == 92 {
                slashes += 1;
                continue;
            }
            line.extend(std::iter::repeat_n(
                92,
                if unit == 34 { slashes * 2 + 1 } else { slashes },
            ));
            line.push(unit);
            slashes = 0;
        }
        line.extend(std::iter::repeat_n(92, slashes * 2));
        line.push(34);
        if line.len() >= 32_767 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Transcriber command line is too long",
            ));
        }
    }
    line.push(0);
    Ok(line)
}

/// Only the transcriber's fixed piped-stdin/stdout/null-stderr command is used.
/// Environment/cwd overrides are rejected rather than silently ignored. The
/// exact absolute .exe and argument builder still come from validated settings.
pub(crate) fn spawn(command: &Command) -> io::Result<Child> {
    let program = Path::new(command.get_program());
    if !program.is_absolute()
        || !program
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        || command.get_envs().next().is_some()
        || command.get_current_dir().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid contained transcriber command",
        ));
    }
    let mut executable = wide(command.get_program())?;
    executable.push(0);
    let mut line = command_line(command)?;
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|e| failure("Create transcriber job", e))?;
    let job = unsafe { OwnedHandle::from_raw_handle(job.0) };
    inherit(&job, false)?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    }
    .map_err(|e| failure("Configure transcriber job", e))?;
    let (input, writer) = pipe()?;
    let (reader, output) = pipe()?;
    let errors: OwnedHandle = File::options().write(true).open("NUL")?.into();
    for handle in [&input, &output, &errors] {
        inherit(handle, true)?;
    }
    let jobs = [raw(&job)];
    let handles = [raw(&input), raw(&output), raw(&errors)];
    let attributes = Attributes::new()?;
    attributes.handles(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
    attributes.handles(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = raw(&input);
    startup.StartupInfo.hStdOutput = raw(&output);
    startup.StartupInfo.hStdError = raw(&errors);
    startup.lpAttributeList = attributes.pointer;
    let mut process = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(executable.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            None,
            PCWSTR::null(),
            &startup.StartupInfo,
            &mut process,
        )
    }
    .map_err(|e| failure("Start contained transcriber", e))?;
    let process_handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess.0) };
    let _thread = unsafe { OwnedHandle::from_raw_handle(process.hThread.0) };
    let mut child = Child {
        job: Some(job),
        process: process_handle,
        stdin: Some(writer.into()),
        stdout: Some(reader.into()),
    };
    let mut contained = BOOL::default();
    if unsafe {
        IsProcessInJob(
            raw(&child.process),
            Some(raw(child.job.as_ref().unwrap())),
            &mut contained,
        )
    }
    .is_err()
        || !contained.as_bool()
    {
        let _ = unsafe { TerminateProcess(raw(&child.process), 1) };
        let _ = child.wait();
        return Err(io::Error::other(
            "The transcriber did not enter its required job",
        ));
    }
    // CreateProcessW assigned the job before the initial thread could execute.
    // No PCM has entered writer; these unused child pipe copies now close here.
    Ok(child)
}

#[cfg(test)]
#[path = "windows_child_job_tests.rs"]
mod tests;
