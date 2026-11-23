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
                CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
            },
            Pipes::CreatePipe,
            Threading::{
                CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW, CreateProcessW,
                DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
                INFINITE, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
                PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
                UpdateProcThreadAttribute, WaitForSingleObject,
            },
        },
    },
    core::{BOOL, PCWSTR, PWSTR},
};

#[derive(Debug)]
struct NativeFailure {
    operation: &'static str,
    hresult: i32,
}
impl std::fmt::Display for NativeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} failed ({:#x})", self.operation, self.hresult)
    }
}
impl std::error::Error for NativeFailure {}

/// Only fixed operation names and numeric native codes, never error messages or
/// executable/argument data. The installed capture probe uses this on failure.
pub(crate) fn native_failure(error: &io::Error) -> Option<(&'static str, i32)> {
    error
        .get_ref()?
        .downcast_ref::<NativeFailure>()
        .map(|failure| (failure.operation, failure.hresult))
}
fn child_policy_bits(flags: Option<u32>) -> [Option<bool>; 3] {
    [1, 2, 4].map(|bit| flags.map(|value| value & bit != 0))
}
fn is_extended_path(path: &Path) -> bool {
    path.as_os_str().encode_wide().take(4).eq([92, 92, 63, 92])
}
/// Inspect only this process and its current job. Query failures are evidence,
/// not defaults: each value is absent unless its native query succeeded. With a
/// nested chain, a NULL job query describes the caller's immediate job only.
pub(crate) fn parent_spawn_snapshot() -> serde_json::Value {
    use windows::Win32::System::{
        JobObjects::{
            JOBOBJECT_BASIC_UI_RESTRICTIONS, JobObjectBasicUIRestrictions,
            QueryInformationJobObject,
        },
        SystemServices::PROCESS_MITIGATION_CHILD_PROCESS_POLICY,
        Threading::{GetCurrentProcess, GetProcessMitigationPolicy, ProcessChildProcessPolicy},
    };
    let process = unsafe { GetCurrentProcess() };
    let mut in_job = BOOL::default();
    let membership = unsafe { IsProcessInJob(process, None, &mut in_job) };
    let mut ui = JOBOBJECT_BASIC_UI_RESTRICTIONS::default();
    let ui_result = unsafe {
        QueryInformationJobObject(
            None,
            JobObjectBasicUIRestrictions,
            (&mut ui as *mut JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
            std::mem::size_of_val(&ui) as u32,
            None,
        )
    };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let limits_result = unsafe {
        QueryInformationJobObject(
            None,
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
            None,
        )
    };
    let mut policy = PROCESS_MITIGATION_CHILD_PROCESS_POLICY::default();
    let mitigation = unsafe {
        GetProcessMitigationPolicy(
            process,
            ProcessChildProcessPolicy,
            (&mut policy as *mut PROCESS_MITIGATION_CHILD_PROCESS_POLICY).cast(),
            std::mem::size_of_val(&policy),
        )
    };
    let policy_flags = mitigation
        .as_ref()
        .ok()
        .map(|_| unsafe { policy.Anonymous.Flags });
    let executable_extended_path = std::env::current_exe()
        .ok()
        .map(|path| is_extended_path(&path));
    let [blocked, audited, secure_only] = child_policy_bits(policy_flags);
    let hresult = |result: &windows::core::Result<()>| {
        result.as_ref().err().map_or(0, |error| error.code().0)
    };
    serde_json::json!({
        "in_job":membership.as_ref().ok().map(|_|in_job.as_bool()),
        "in_job_hresult":hresult(&membership),
        "job_ui_flags":ui_result.as_ref().ok().map(|_|ui.UIRestrictionsClass.0),
        "job_ui_hresult":hresult(&ui_result),
        "job_limit_flags":limits_result.as_ref().ok().map(|_|limits.BasicLimitInformation.LimitFlags.0),
        "job_active_process_limit":limits_result.as_ref().ok().map(|_|limits.BasicLimitInformation.ActiveProcessLimit),
        "job_limits_hresult":hresult(&limits_result),
        "child_policy_flags":policy_flags,
        "child_creation_blocked":blocked,
        "child_creation_audited":audited,
        "child_secure_creation_only":secure_only,
        "child_policy_hresult":hresult(&mitigation),
        "executable_extended_path":executable_extended_path
    })
}
fn failure(operation: &'static str, error: windows::core::Error) -> io::Error {
    io::Error::other(NativeFailure {
        operation,
        hresult: error.code().0,
    })
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
    /// Inspect the original retained process without reaping it or closing its
    /// job. Test diagnostics must not change launcher/descendant ownership.
    #[cfg(test)]
    pub(crate) fn status_snapshot(&self) -> io::Result<Option<ExitStatus>> {
        if self.is_running()? {
            Ok(None)
        } else {
            self.status().map(Some)
        }
    }
    /// Classify only a member of this exact held job. The process handle used
    /// for membership is retained through the executable query, avoiding a
    /// PID-reuse gap. Callers must not log the returned owned-fixture path.
    #[cfg(test)]
    pub(crate) fn member_executable(&self, pid: u32) -> io::Result<std::path::PathBuf> {
        use std::os::windows::ffi::OsStringExt;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };
        let job = self
            .job
            .as_ref()
            .ok_or_else(|| io::Error::other("Owned fixture job is closed"))?;
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|error| failure("Inspect fixture member", error))?;
        let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
        let mut belongs = BOOL::default();
        unsafe { IsProcessInJob(raw(&process), Some(raw(job)), &mut belongs) }
            .map_err(|error| failure("Check fixture member", error))?;
        if !belongs.as_bool() {
            return Err(io::Error::other("Process is not an owned fixture member"));
        }
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                raw(&process),
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )
        }
        .map_err(|error| failure("Read fixture member executable", error))?;
        Ok(std::ffi::OsString::from_wide(&path[..length as usize]).into())
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
    fn new(count: u32) -> io::Result<Self> {
        let mut bytes = 0;
        // The first call reports the required allocation by design.
        let _ = unsafe { InitializeProcThreadAttributeList(None, count, None, &mut bytes) };
        if bytes == 0 || bytes > 65_536 {
            return Err(io::Error::other("Invalid process attribute allocation"));
        }
        let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
        let pointer = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        unsafe { InitializeProcThreadAttributeList(Some(pointer), count, None, &mut bytes) }
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
        .map_err(|e| {
            failure(
                if kind == PROC_THREAD_ATTRIBUTE_JOB_LIST {
                    "Set contained process job attribute"
                } else {
                    "Set contained process handle attribute"
                },
                e,
            )
        })
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
    spawn_with_stdio(command, StdioMode::Piped, false, None)
}

/// Explicit diagnostic consumers only. Ordinary transcriber calls install no
/// observer and perform no extra PID queries or logging.
#[derive(serde::Serialize)]
pub(crate) struct SpawnObservation {
    pub stage: &'static str,
    pub hresult: i32,
    pub parent_pid: u32,
    pub child_pid: Option<u32>,
    pub process_handle_pid: Option<u32>,
    pub contained: Option<bool>,
    pub creation_flags: u32,
}
type Observer<'a> = Option<&'a dyn Fn(SpawnObservation)>;
#[derive(Clone, Copy)]
struct ObservationContext {
    parent_pid: u32,
    creation_flags: u32,
}
fn observe(
    observer: Observer<'_>,
    stage: &'static str,
    hresult: i32,
    child_pid: Option<u32>,
    process_handle_pid: Option<u32>,
    contained: Option<bool>,
    context: ObservationContext,
) {
    if let Some(observer) = observer {
        observer(SpawnObservation {
            stage,
            hresult,
            parent_pid: context.parent_pid,
            child_pid,
            process_handle_pid,
            contained,
            creation_flags: context.creation_flags,
        });
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum StdioMode {
    Piped,
    None,
}
impl StdioMode {
    fn attributes(self) -> u32 {
        if self == Self::Piped { 2 } else { 1 }
    }
    fn inherits(self) -> bool {
        self == Self::Piped
    }
}
struct Pipes {
    input: OwnedHandle,
    writer: OwnedHandle,
    reader: OwnedHandle,
    output: OwnedHandle,
    errors: OwnedHandle,
}
impl Pipes {
    fn new() -> io::Result<Self> {
        let (input, writer) = pipe()?;
        let (reader, output) = pipe()?;
        let errors: OwnedHandle = File::options().write(true).open("NUL")?.into();
        for handle in [&input, &output, &errors] {
            inherit(handle, true)?;
        }
        Ok(Self {
            input,
            writer,
            reader,
            output,
            errors,
        })
    }
}
fn version_control_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.arg("--version");
    command
}
fn capture_fixture_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.arg("--windows-capture-owned-fixture");
    command
}
/// A failure-only diagnostic control, never a capture fallback. There is no
/// caller-supplied executable, argument, environment, or working directory.
pub(crate) fn spawn_self_version_control(observer: Observer<'_>) -> io::Result<Child> {
    spawn_with_stdio(
        &version_control_command(&std::env::current_exe()?),
        StdioMode::None,
        false,
        observer,
    )
}
pub(crate) fn spawn_self_capture_fixture(observer: Observer<'_>) -> io::Result<Child> {
    spawn_with_stdio(
        &capture_fixture_command(&std::env::current_exe()?),
        StdioMode::Piped,
        false,
        observer,
    )
}

fn fixture_breakaway_eligible(native: Option<(&str, i32)>, flags: Option<u32>) -> bool {
    native == Some(("Start contained transcriber", 0x80070005_u32 as i32))
        && flags.is_some_and(|flags| flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK.0 != 0)
}
pub(crate) fn fixture_creation_flags(breakaway: bool) -> u32 {
    (CREATE_NO_WINDOW
        | EXTENDED_STARTUPINFO_PRESENT
        | if breakaway {
            CREATE_BREAKAWAY_FROM_JOB
        } else {
            Default::default()
        })
    .0
}
/// Retry only this fixed diagnostic helper after the exact initial native
/// access-denied failure and a fresh successful query permitting breakaway.
/// The parent's job is not ours to change. Our own JOB_LIST/KILL_ON_CLOSE job
/// remains mandatory and is assigned before the new initial thread can run.
pub(crate) fn retry_capture_fixture_breakaway(
    original: &io::Error,
    observer: Observer<'_>,
) -> Option<io::Result<Child>> {
    use windows::Win32::System::JobObjects::QueryInformationJobObject;
    if native_failure(original) != Some(("Start contained transcriber", 0x80070005_u32 as i32)) {
        return None;
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let queried = unsafe {
        QueryInformationJobObject(
            None,
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
            None,
        )
    };
    let flags = queried
        .ok()
        .map(|_| limits.BasicLimitInformation.LimitFlags.0);
    if !fixture_breakaway_eligible(native_failure(original), flags) {
        return None;
    }
    Some((|| {
        let command = capture_fixture_command(&std::env::current_exe()?);
        spawn_with_stdio(&command, StdioMode::Piped, true, observer)
    })())
}

fn spawn_with_stdio(
    command: &Command,
    mode: StdioMode,
    breakaway: bool,
    observer: Observer<'_>,
) -> io::Result<Child> {
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
    let pipes = if mode == StdioMode::Piped {
        Some(Pipes::new()?)
    } else {
        None
    };
    let jobs = [raw(&job)];
    let handles = pipes
        .as_ref()
        .map(|pipes| [raw(&pipes.input), raw(&pipes.output), raw(&pipes.errors)]);
    let attributes = Attributes::new(mode.attributes())?;
    attributes.handles(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
    if let Some(handles) = &handles {
        attributes.handles(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles)?;
    }
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    if let Some(pipes) = &pipes {
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = raw(&pipes.input);
        startup.StartupInfo.hStdOutput = raw(&pipes.output);
        startup.StartupInfo.hStdError = raw(&pipes.errors);
    }
    startup.lpAttributeList = attributes.pointer;
    let mut process = PROCESS_INFORMATION::default();
    let observation_context = ObservationContext {
        parent_pid: if observer.is_some() {
            unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }
        } else {
            0
        },
        creation_flags: fixture_creation_flags(breakaway),
    };
    observe(
        observer,
        "create-process-before",
        0,
        None,
        None,
        None,
        observation_context,
    );
    let created = unsafe {
        CreateProcessW(
            PCWSTR(executable.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            mode.inherits(),
            windows::Win32::System::Threading::PROCESS_CREATION_FLAGS(fixture_creation_flags(
                breakaway,
            )),
            None,
            PCWSTR::null(),
            &startup.StartupInfo,
            &mut process,
        )
    };
    // Preserve the call's actual status and raw PROCESS_INFORMATION PID before
    // any post-return native query, including GetProcessId. The observer writes
    // to an independent retained handle, not the process's standard table.
    let creation_hresult = created.as_ref().err().map_or(0, |error| error.code().0);
    observe(
        observer,
        "create-process-returned",
        creation_hresult,
        created.as_ref().ok().map(|_| process.dwProcessId),
        None,
        None,
        observation_context,
    );
    created.map_err(|e| failure("Start contained transcriber", e))?;
    // Query only the exact returned process handle for the explicit observer.
    let process_handle_pid = if observer.is_some() {
        let pid = unsafe { windows::Win32::System::Threading::GetProcessId(process.hProcess) };
        (pid != 0).then_some(pid)
    } else {
        None
    };
    observe(
        observer,
        "process-handle-queried",
        creation_hresult,
        Some(process.dwProcessId),
        process_handle_pid,
        None,
        observation_context,
    );
    // Attribute values reference the job/pipe handle arrays. Delete the list
    // while all values and their handles still live, before transferring pipes.
    drop(attributes);
    observe(
        observer,
        "attributes-dropped",
        0,
        Some(process.dwProcessId),
        process_handle_pid,
        None,
        observation_context,
    );
    let process_handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess.0) };
    let _thread = unsafe { OwnedHandle::from_raw_handle(process.hThread.0) };
    let (stdin, stdout) = match pipes {
        Some(pipes) => (Some(pipes.writer.into()), Some(pipes.reader.into())),
        None => (None, None),
    };
    let child = Child {
        job: Some(job),
        process: process_handle,
        stdin,
        stdout,
    };
    let mut contained = BOOL::default();
    observe(
        observer,
        "membership-before",
        0,
        Some(process.dwProcessId),
        process_handle_pid,
        None,
        observation_context,
    );
    let membership = unsafe {
        IsProcessInJob(
            raw(&child.process),
            Some(raw(child.job.as_ref().unwrap())),
            &mut contained,
        )
    };
    observe(
        observer,
        "membership-returned",
        membership.as_ref().err().map_or(0, |error| error.code().0),
        Some(process.dwProcessId),
        process_handle_pid,
        membership.as_ref().ok().map(|_| contained.as_bool()),
        observation_context,
    );
    if membership.is_err() || !contained.as_bool() {
        observe(
            observer,
            "membership-cleanup-before",
            0,
            Some(process.dwProcessId),
            process_handle_pid,
            None,
            observation_context,
        );
        let _ = unsafe { TerminateProcess(raw(&child.process), 1) };
        // Retain the exact process/job handles through a bounded termination
        // wait. Returning drops the kill-on-close job even if no ACK arrives;
        // never replace the original membership failure with cleanup details.
        let _ = unsafe { WaitForSingleObject(raw(&child.process), 1000) };
        observe(
            observer,
            "membership-cleanup-waited",
            0,
            Some(process.dwProcessId),
            process_handle_pid,
            None,
            observation_context,
        );
        return Err(match membership {
            Err(error) => failure("Check contained process job", error),
            Ok(()) => io::Error::other("The transcriber did not enter its required job"),
        });
    }
    // CreateProcessW assigned the job before the initial thread could execute.
    // No PCM has entered writer; these unused child pipe copies now close here.
    observe(
        observer,
        "spawn-complete",
        0,
        Some(process.dwProcessId),
        process_handle_pid,
        Some(true),
        observation_context,
    );
    Ok(child)
}

#[cfg(test)]
#[path = "windows_child_job_tests.rs"]
mod tests;

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn native_failure_preserves_fixed_operation_and_exact_hresult_without_parsing_text() {
        let code = windows::core::HRESULT(0x80070005_u32 as i32);
        let error = failure(
            "Start contained transcriber",
            windows::core::Error::from_hresult(code),
        );
        assert_eq!(
            native_failure(&error),
            Some(("Start contained transcriber", code.0))
        );
        assert_eq!(
            error.to_string(),
            "Start contained transcriber failed (0x80070005)"
        );
        assert_eq!(
            native_failure(&io::Error::other("untrusted/private/message")),
            None
        );
        assert_eq!(native_failure(&io::Error::from_raw_os_error(5)), None);
    }
    #[test]
    fn failure_control_has_fixed_argv_and_no_stdio_inheritance_policy() {
        let path = Path::new(r"C:\Owned directory\PickleRune.exe");
        let command = version_control_command(path);
        assert_eq!(command.get_program(), path.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("--version")]
        );
        assert!(command.get_envs().next().is_none());
        assert!(command.get_current_dir().is_none());
        assert_eq!(StdioMode::None.attributes(), 1); // JOB_LIST only
        assert!(!StdioMode::None.inherits());
        assert_eq!(StdioMode::Piped.attributes(), 2); // JOB_LIST + HANDLE_LIST
        assert!(StdioMode::Piped.inherits());
    }
    #[test]
    fn policy_unknown_is_not_false_and_extended_path_is_only_a_boolean() {
        assert_eq!(child_policy_bits(None), [None, None, None]);
        assert_eq!(child_policy_bits(Some(0)), [Some(false); 3]);
        assert_eq!(
            child_policy_bits(Some(5)),
            [Some(true), Some(false), Some(true)]
        );
        assert_eq!(
            child_policy_bits(Some(2)),
            [Some(false), Some(true), Some(false)]
        );
        assert!(is_extended_path(Path::new(r"\\?\C:\Owned\PickleRune.exe")));
        assert!(!is_extended_path(Path::new(r"C:\Owned\PickleRune.exe")));
    }
    #[test]
    fn fixture_breakaway_requires_exact_native_failure_and_explicit_parent_permission() {
        let denied = Some(("Start contained transcriber", 0x80070005_u32 as i32));
        let allowed = Some(JOB_OBJECT_LIMIT_BREAKAWAY_OK.0);
        assert!(fixture_breakaway_eligible(denied, allowed));
        assert!(!fixture_breakaway_eligible(denied, None));
        assert!(!fixture_breakaway_eligible(denied, Some(0)));
        assert!(!fixture_breakaway_eligible(denied, Some(0x1000))); // silent-only isn't explicit permission
        assert!(!fixture_breakaway_eligible(None, allowed));
        assert!(!fixture_breakaway_eligible(
            Some(("Set contained process job attribute", 0x80070005_u32 as i32)),
            allowed
        ));
        assert!(!fixture_breakaway_eligible(
            Some(("Start contained transcriber", 0x80070057_u32 as i32)),
            allowed
        ));
    }
    #[test]
    fn fixture_retry_changes_only_breakaway_flag_and_keeps_fixed_helper_argv() {
        let ordinary = fixture_creation_flags(false);
        let retry = fixture_creation_flags(true);
        assert_eq!(retry ^ ordinary, CREATE_BREAKAWAY_FROM_JOB.0);
        assert_eq!(
            ordinary,
            (CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT).0
        );
        assert_eq!(
            retry & windows::Win32::System::Threading::CREATE_SUSPENDED.0,
            0
        );
        let path = Path::new(r"C:\Owned directory\PickleRune.exe");
        let command = capture_fixture_command(path);
        assert_eq!(command.get_program(), path.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("--windows-capture-owned-fixture")]
        );
        assert!(command.get_envs().next().is_none());
        assert!(command.get_current_dir().is_none());
        assert_eq!(StdioMode::Piped.attributes(), 2);
        assert!(StdioMode::Piped.inherits());
    }
}
