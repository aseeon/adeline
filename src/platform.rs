//! OS glue for the engine: process containment, detached and piped spawning,
//! console attachment and owner-only IPC pipes. All unsafe code lives here.
#![cfg_attr(
    windows,
    expect(
        unsafe_code,
        reason = "Win32 calls for jobs, consoles and pipe security"
    )
)]
use std::{
    io,
    process::{Child, ChildStdin, Command, Stdio},
};
use tokio::io::AsyncRead;

pub type Reader = Box<dyn AsyncRead + Send + Unpin>;

/// Ties every process this one starts to its lifetime, even when it is killed.
#[cfg(windows)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Win32 struct sizes fit in u32"
)]
pub fn contain_children() -> Result<(), String> {
    use windows_sys::Win32::System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        Threading::GetCurrentProcess,
    };
    let error = |step| format!("{step} failed: {}", io::Error::last_os_error());
    // SAFETY: null attributes and name create an anonymous job with default security.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(error("Creating a job object"));
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `limits` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of the size passed.
    let set = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    // SAFETY: `job` is a valid job handle and the pseudo-handle names this process.
    if set == 0 || unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) } == 0 {
        return Err(error("Joining the job object"));
    }
    // The handle leaks on purpose: the OS closes it when this process dies,
    // and closing the last handle kills every process left in the job.
    Ok(())
}

/// Ties every process this one starts to its lifetime, even when it is killed.
#[cfg(not(windows))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "matches the Windows version, which can fail"
)]
pub fn contain_children() -> Result<(), String> {
    Ok(())
}

/// Starts a background process that outlives the caller, outside its console and job.
#[expect(
    clippy::disallowed_methods,
    reason = "Callers run this off the UI thread"
)]
pub fn spawn_detached(command: &mut Command) -> io::Result<Child> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
        };
        use windows_sys::Win32::{
            Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation},
            System::Console::{
                GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            },
        };
        // Windows passes every inheritable handle to the child, so our own
        // stdio (often a caller's capture pipe) would stay open in the engine
        // and the caller would never see end of output.
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: GetStdHandle takes no pointers; the handle it returns is
            // only passed back to SetHandleInformation, which ignores bad handles.
            unsafe {
                let handle = GetStdHandle(which);
                if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        match command
            .creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
        {
            // The caller's job forbids breakaway; stay in it rather than fail.
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                command.creation_flags(flags).spawn()
            }
            result => result,
        }
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0).spawn()
    }
}

/// Lets a `windows_subsystem = "windows"` exe print to the terminal that started it.
pub fn attach_parent_console() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        // SAFETY: AttachConsole takes no pointers; failure (no parent console) is fine.
        unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

/// Creates an inbound and outbound IPC pipe instance only this user (and SYSTEM)
/// can open. The default named-pipe DACL lets Everyone read.
#[cfg(windows)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Win32 struct sizes fit in u32"
)]
pub fn owner_only_pipe(
    name: &str,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            SECURITY_ATTRIBUTES,
        },
    };
    // The user's SID, not OW: an elevated process's objects are owned by Administrators.
    let sddl = format!("D:P(A;;GA;;;{})(A;;GA;;;SY)", current_user_sid()?);
    let sddl: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated and `descriptor` is a valid out pointer.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` and its descriptor stay alive for the whole call.
    let server = unsafe {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, (&raw mut attributes).cast())
    };
    // SAFETY: the descriptor came from LocalAlloc and nothing uses it any more.
    unsafe { LocalFree(descriptor) };
    server
}

#[cfg(windows)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Win32 struct sizes fit in u32"
)]
fn current_user_sid() -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    // SAFETY: the pseudo-handle names this process and `token` is a valid out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64s keep TOKEN_USER aligned; 512 bytes fit it plus the largest SID.
    let mut buffer = [0u64; 64];
    let mut length = 0;
    // SAFETY: `buffer` is writable for the size passed and `length` is a valid out pointer.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size_of_val(&buffer) as u32,
            &raw mut length,
        )
    };
    let error = io::Error::last_os_error();
    // SAFETY: `token` is an open handle this function owns.
    unsafe { CloseHandle(token) };
    if read == 0 {
        return Err(error);
    }
    // SAFETY: GetTokenInformation filled `buffer` with an aligned TOKEN_USER.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = std::ptr::null_mut();
    // SAFETY: the SID points into `buffer`, which is alive; `text` is a valid out pointer.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ConvertSidToStringSidW returned a NUL-terminated string we free once read.
    let sid = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        sid
    };
    Ok(sid)
}

/// Spawns `command` with piped stdin and stdout, plus stderr when asked. The
/// readers are driven by the reactor (IOCP or epoll), so no thread waits on them.
#[expect(
    clippy::disallowed_methods,
    reason = "Callers run this off the UI thread"
)]
pub fn spawn_piped(
    command: &mut Command,
    stderr: bool,
) -> io::Result<(Child, ChildStdin, Reader, Option<Reader>)> {
    command.stdin(Stdio::piped());
    #[cfg(windows)]
    let (mut child, stdout, stderr) = {
        // Anonymous pipes would need a blocking-pool thread parked on every read.
        let (stdout, client) = overlapped_pipe()?;
        command.stdout(client);
        let stderr = if stderr {
            let (reader, client) = overlapped_pipe()?;
            command.stderr(client);
            Some(reader)
        } else {
            command.stderr(Stdio::null());
            None
        };
        let child = command.spawn();
        // `command` holds the child's pipe ends; drop them so EOF arrives on exit.
        command.stdout(Stdio::null()).stderr(Stdio::null());
        (child?, stdout, stderr)
    };
    #[cfg(not(windows))]
    let (mut child, stdout, stderr) = {
        command.stdout(Stdio::piped()).stderr(if stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = command.spawn()?;
        let reader = |fd: std::os::fd::OwnedFd| -> io::Result<Reader> {
            Ok(Box::new(tokio::net::unix::pipe::Receiver::from_owned_fd(
                fd,
            )?))
        };
        let stdout = reader(child.stdout.take().expect("piped stdout").into())?;
        let stderr = child
            .stderr
            .take()
            .map(|fd| reader(fd.into()))
            .transpose()?;
        (child, stdout, stderr)
    };
    let stdin = child.stdin.take().expect("piped stdin");
    Ok((child, stdin, stdout, stderr))
}

/// An overlapped pipe: the async read end and the synchronous write end for a child.
#[cfg(windows)]
fn overlapped_pipe() -> io::Result<(Reader, std::fs::File)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        r"\\.\pipe\adeline-io-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let server = tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .max_instances(1)
        .access_inbound(true)
        .access_outbound(false)
        .reject_remote_clients(true)
        .create(&name)?;
    let client = std::fs::OpenOptions::new().write(true).open(&name)?;
    // mio starts reading only after `connect`. The client is already in, so
    // the first poll finishes it.
    {
        let connect = std::pin::pin!(server.connect());
        let waker = std::task::Waker::noop();
        match connect.poll(&mut std::task::Context::from_waker(waker)) {
            std::task::Poll::Ready(result) => result?,
            std::task::Poll::Pending => return Err(io::Error::other("Pipe did not connect")),
        }
    }
    Ok((Box::new(server), client))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tokio::io::AsyncReadExt as _;

    #[tokio::test(flavor = "multi_thread")]
    async fn piped_child_echoes_stdin_and_reports_stderr() {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd.exe");
            std::os::windows::process::CommandExt::raw_arg(
                &mut command,
                "/C more & echo oops 1>&2",
            );
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "read line; echo \"$line\"; echo oops >&2"]);
            command
        };
        let (mut child, mut stdin, mut stdout, stderr) = spawn_piped(&mut command, true).unwrap();
        writeln!(stdin, "hello").unwrap();
        drop(stdin);
        let (mut out, mut err) = (String::new(), String::new());
        stdout.read_to_string(&mut out).await.unwrap();
        stderr.unwrap().read_to_string(&mut err).await.unwrap();
        assert_eq!(out.trim(), "hello");
        assert_eq!(err.trim(), "oops");
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn detached_process_runs() {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd.exe");
            command.args(["/C", "exit 0"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = Command::new("true");
        assert!(
            spawn_detached(&mut command)
                .unwrap()
                .wait()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn children_can_be_contained() {
        contain_children().unwrap();
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn owner_only_pipe_accepts_this_user() {
        use tokio::io::AsyncWriteExt as _;
        let name = format!(r"\\.\pipe\adeline-test-{}", std::process::id());
        let mut server = owner_only_pipe(&name, true).unwrap();
        let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .unwrap();
        server.connect().await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut buffer = [0; 4];
        server.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"ping");
    }
}
