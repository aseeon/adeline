//! Remote machines through the system `ssh`.
//!
//! This finds the remote platform and its Adeline, installs this client's version
//! there, and runs `adeline bridge`, which joins the SSH session to the remote
//! engine's local pipe or socket. `ssh` runs with the client's `askpass` address, so its prompts reach the UI. The
//! SSH session starts in the home folder on every platform, so remote paths are
//! relative to it.
use crate::{
    ipc,
    machines::split_port,
    protocol::{ClientMessage, Command, EngineMessage, PROTOCOL, Status},
};
use anyhow::Context as _;
use std::{
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWriteExt as _, BufReader},
    process::Child,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES: &str = "https://github.com/aseeon/adeline/releases/download";
const SUPPORTED: [&str; 4] = [
    "windows-x86_64",
    "macos-arm64",
    "linux-x86_64",
    "linux-aarch64",
];
const MARKER: &str = "ADELINE-PROBE";
const INSTALLED: &str = "ADELINE-INSTALLED";
// A remote engine stops its agents before it answers; give slow ones time.
const REMOTE_SHUTDOWN: Duration = Duration::from_secs(45);
// How long `ssh` gets to exit once its session is done.
const SSH_EXIT: Duration = Duration::from_secs(5);
// `ssh` exits with 255 for its own errors, any other code is the remote command's.
const SSH_FAILED: i32 = 255;
// Error output is kept to its last part; the end says what went wrong.
const ERRORS_CAP: usize = 16_384;
const ERRORS_KEPT: usize = 8_192;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    // Retried with growing delays.
    Network(String),
    // A rejected password or key. Not retried until the user asks.
    SignIn(String),
    // Retrying wouldn't help: a refused host key, the wrong engine, no build.
    Fatal(String),
    Unsupported(String),
    // The remote engine speaks a newer protocol than this client.
    LocalUpdate(String),
    // An older protocol runs there. Replacing it stops its agents.
    Upgrade { version: String, active: usize },
}

pub struct Link {
    pub reader: ipc::Reader,
    pub writer: ipc::Writer,
    pub status: Status,
    pub destination: String,
    // Dropping the `ssh` process ends the session.
    pub child: Child,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("status", &self.status)
            .field("destination", &self.destination)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct Askpass {
    pub address: String,
    pub machine: String,
}

// A password typed in one `ssh` run of a connect answers the same prompt in the next one.
static SESSION: AtomicU64 = AtomicU64::new(0);

pub async fn connect(
    remote: &crate::machines::Machine,
    hello: &ClientMessage,
    upgrade: bool,
    askpass: &Askpass,
) -> Result<Link, Failure> {
    // The first destination that reaches the machine's engine wins. `upgrade` is
    // the user's consent to restart an older engine.
    let mut failures = Vec::new();
    let (mut sign_in, mut network) = (false, false);
    for destination in &remote.destinations {
        match attempt(remote, destination, hello, upgrade, askpass).await {
            Ok(link) => return Ok(link),
            Err(Failure::Network(error)) => {
                network = true;
                failures.push(format!("{destination}: {error}"));
            }
            Err(Failure::SignIn(error)) => {
                sign_in = true;
                failures.push(format!("{destination}: {error}"));
            }
            Err(Failure::Fatal(error)) => failures.push(format!("{destination}: {error}")),
            Err(other) => return Err(other),
        }
    }
    let message = failures.join("\n");
    Err(if sign_in {
        Failure::SignIn(message)
    } else if network {
        Failure::Network(message)
    } else {
        Failure::Fatal(message)
    })
}

async fn attempt(
    remote: &crate::machines::Machine,
    destination: &str,
    hello: &ClientMessage,
    upgrade: bool,
    askpass: &Askpass,
) -> Result<Link, Failure> {
    let mut probe = probe(destination, askpass).await?;
    if !SUPPORTED.contains(&probe.platform.as_str()) {
        return Err(Failure::Unsupported(format!(
            "No Adeline build for {}",
            probe.platform
        )));
    }
    let wrong = |found: &str| {
        Failure::Fatal(format!(
            "{destination} reaches a different Adeline engine ({}) than the one saved for {}.",
            short_id(found),
            remote.name
        ))
    };
    // A host whose engine never ran can't be the saved one either.
    if let Some(saved) = &remote.engine
        && probe.id != *saved
    {
        return Err(wrong(&probe.id));
    }
    // Only the engine's protocol decides compatibility; any Adeline version
    // that speaks this client's protocol is used as it is.
    if probe.version.trim().is_empty() {
        install(destination, &probe.platform, askpass).await?;
        probe.version = VERSION.into();
    }
    let windows = probe.platform.starts_with("windows");
    for round in 0..2 {
        let (child, reader, writer, status) = bridge(destination, windows, hello, askpass).await?;
        if let Some(saved) = &remote.engine
            && status.engine_id != *saved
        {
            return Err(wrong(&status.engine_id));
        }
        if status.protocol == PROTOCOL {
            return Ok(Link {
                reader,
                writer,
                status,
                destination: destination.to_owned(),
                child,
            });
        }
        if status.protocol > PROTOCOL {
            return Err(Failure::LocalUpdate(status.version));
        }
        if !upgrade || round > 0 {
            return Err(Failure::Upgrade {
                version: status.version,
                active: status.conversations.len(),
            });
        }
        // An installed build newer than this one may already speak its protocol.
        let installed = probe.version.trim();
        if installed != VERSION && !crate::harness::newer(installed, VERSION) {
            install(destination, &probe.platform, askpass).await?;
        }
        shut_down(child, reader, writer).await;
    }
    Err(Failure::Network(
        "The older engine didn't make way for this version.".into(),
    ))
}

async fn shut_down(mut child: Child, mut reader: ipc::Reader, mut writer: ipc::Writer) {
    let request = ClientMessage::Request {
        id: u64::MAX,
        command: Command::Shutdown,
    };
    if ipc::send(&mut writer, &request).await.is_ok() {
        let _ = tokio::time::timeout(REMOTE_SHUTDOWN, async {
            while let Ok(Some(_)) = reader.next_line().await {}
        })
        .await;
    }
    drop(writer);
    let _ = tokio::time::timeout(SSH_EXIT, child.wait()).await;
}

fn short_id(id: &str) -> &str {
    if id.is_empty() {
        "a new engine"
    } else {
        id.get(..8).unwrap_or(id)
    }
}

fn ssh(destination: &str, remote_command: &str, askpass: &Askpass) -> tokio::process::Command {
    // A test hook: another command line to run instead, such as `python fake_ssh.py`.
    let custom = std::env::var("ADELINE_SSH").unwrap_or_default();
    let mut words = custom.split_whitespace();
    let mut command = tokio::process::Command::new(words.next().unwrap_or("ssh"));
    command.args(words);
    let (host, port) = split_port(destination);
    if let Some(port) = port {
        command.arg("-p").arg(port);
    }
    command.args([
        "-T",
        "-o",
        "ServerAliveInterval=10",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ConnectTimeout=15",
        "--",
        host,
        remote_command,
    ]);
    // Prompts go to Adeline's dialog: `ssh` runs this executable as its askpass.
    if let Ok(exe) = std::env::current_exe() {
        command
            .env("SSH_ASKPASS", exe)
            .env("SSH_ASKPASS_REQUIRE", "force");
    }
    if std::env::var_os("DISPLAY").is_none() {
        // Older `ssh` uses askpass only with a display set.
        command.env("DISPLAY", ":0");
    }
    let session = SESSION.fetch_add(1, Ordering::Relaxed);
    command
        .env("ADELINE_ASKPASS", &askpass.address)
        .env(
            "ADELINE_ASKPASS_FOR",
            format!("{}\t{session}", askpass.machine),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    command
}

type Errors = (Arc<Mutex<String>>, tokio::task::JoinHandle<()>);

fn drain(stderr: Option<tokio::process::ChildStderr>) -> Errors {
    // Reading as it comes means a full pipe never stalls the child.
    let text = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&text);
    let reader = tokio::spawn(async move {
        let Some(mut stderr) = stderr else {
            return;
        };
        let mut buffer = [0; 4096];
        while let Ok(read) = stderr.read(&mut buffer).await {
            if read == 0 {
                break;
            }
            if let Ok(mut text) = sink.lock() {
                text.push_str(&String::from_utf8_lossy(&buffer[..read]));
                if text.len() > ERRORS_CAP {
                    let cut = text.len() - ERRORS_KEPT;
                    let cut = (cut..text.len())
                        .find(|&i| text.is_char_boundary(i))
                        .unwrap_or(0);
                    text.drain(..cut);
                }
            }
        }
    });
    (text, reader)
}

async fn finish((text, reader): Errors) -> String {
    // Waits for the last output, but not for a process that holds the pipe open.
    let _ = tokio::time::timeout(Duration::from_millis(500), reader).await;
    text.lock()
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

fn classify(stderr: &str) -> Failure {
    let text = if stderr.is_empty() {
        "The SSH connection closed.".to_owned()
    } else {
        stderr.to_owned()
    };
    if text.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
        || text.contains("Host key verification failed")
    {
        Failure::Fatal(text)
    } else if text.contains("Permission denied")
        || text.contains("Too many authentication failures")
        || text.contains("Authentication failed")
    {
        Failure::SignIn(text)
    } else {
        Failure::Network(text)
    }
}

fn spawn_error(error: &std::io::Error) -> Failure {
    Failure::Fatal(if error.kind() == std::io::ErrorKind::NotFound {
        "Adeline needs the ssh command. Install OpenSSH and try again.".into()
    } else {
        format!("Cannot run ssh: {error}")
    })
}

async fn run(
    destination: &str,
    remote_command: &str,
    input: &[u8],
    askpass: &Askpass,
) -> Result<(String, String, bool), Failure> {
    let mut child = ssh(destination, remote_command, askpass)
        .spawn()
        .map_err(|e| spawn_error(&e))?;
    let errors = drain(child.stderr.take());
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = input.to_vec();
    let feed = tokio::spawn(async move {
        let _ = stdin.write_all(&input).await;
        let _ = stdin.shutdown().await;
    });
    let mut output = Vec::new();
    let _ = stdout.read_to_end(&mut output).await;
    let _ = feed.await;
    let status = child
        .wait()
        .await
        .map_err(|e| Failure::Network(e.to_string()))?;
    let stderr = finish(errors).await;
    if status.code() == Some(SSH_FAILED) {
        return Err(classify(&stderr));
    }
    Ok((
        String::from_utf8_lossy(&output).into_owned(),
        stderr,
        status.success(),
    ))
}

fn powershell(script: &str) -> String {
    // Base64 survives the quoting of cmd and PowerShell, whichever the SSH shell is.
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        crate::conversation::base64(&utf16)
    )
}

fn unix_probe() -> String {
    format!(
        "sh -c 'echo {MARKER}; uname -sm; cat .config/adeline/engine/id 2>/dev/null; echo; cat .adeline/bin/version 2>/dev/null; echo'"
    )
}

fn windows_probe() -> String {
    powershell(&format!(
        r#"$h = $env:USERPROFILE
'{MARKER}'
'Windows ' + $env:PROCESSOR_ARCHITECTURE
('' + (Get-Content -Raw -LiteralPath "$h\.config\adeline\engine\id" -ErrorAction SilentlyContinue)).Trim()
('' + (Get-Content -Raw -LiteralPath "$h\.adeline\bin\version" -ErrorAction SilentlyContinue)).Trim()"#
    ))
}

fn unix_install() -> String {
    format!(
        "sh -c 'mkdir -p .adeline/bin && cat > .adeline/bin/adeline.part && chmod 755 .adeline/bin/adeline.part && mv -f .adeline/bin/adeline.part .adeline/bin/adeline && printf %s {VERSION} > .adeline/bin/version && echo {INSTALLED}'"
    )
}

fn windows_install() -> String {
    // A running exe can be renamed but not replaced.
    powershell(&format!(
        r#"$ErrorActionPreference = 'Stop'
$d = "$env:USERPROFILE\.adeline\bin"
New-Item -ItemType Directory -Force -Path $d | Out-Null
$part = "$d\adeline.part"
$out = [IO.File]::Create($part)
[Console]::OpenStandardInput().CopyTo($out)
$out.Close()
$exe = "$d\adeline.exe"
Remove-Item "$d\adeline.old.*" -ErrorAction SilentlyContinue
if (Test-Path -LiteralPath $exe) {{ Move-Item -LiteralPath $exe -Destination ("$d\adeline.old." + [guid]::NewGuid() + ".exe") }}
Move-Item -LiteralPath $part -Destination $exe
[IO.File]::WriteAllText("$d\version", '{VERSION}')
'{INSTALLED}'"#
    ))
}

// A login shell, so agents find the user's usual PATH.
const UNIX_BRIDGE: &str = r#"sh -c 'exec "${SHELL:-sh}" -lc "exec .adeline/bin/adeline bridge"'"#;
const WINDOWS_BRIDGE: &str = r".\.adeline\bin\adeline.exe bridge";

struct Probe {
    platform: String,
    id: String,
    version: String,
}

fn parse_probe(output: &str) -> Option<Probe> {
    let mut lines = output
        .lines()
        .skip_while(|line| line.trim() != MARKER)
        .skip(1)
        .map(str::trim);
    let platform = platform(lines.next()?)?;
    Some(Probe {
        platform,
        id: lines.next().unwrap_or_default().to_owned(),
        version: lines.next().unwrap_or_default().to_owned(),
    })
}

async fn probe(destination: &str, askpass: &Askpass) -> Result<Probe, Failure> {
    let (output, stderr, _) = run(destination, &unix_probe(), &[], askpass).await?;
    if let Some(probe) = parse_probe(&output) {
        return Ok(probe);
    }
    // No POSIX shell: a Windows host.
    let (output, windows_stderr, _) = run(destination, &windows_probe(), &[], askpass).await?;
    parse_probe(&output).ok_or_else(|| {
        let detail = [stderr, windows_stderr]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        Failure::Fatal(format!(
            "Couldn't tell which system the remote machine runs.{}",
            if detail.is_empty() {
                String::new()
            } else {
                format!("\n{detail}")
            }
        ))
    })
}

fn platform(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    let (system, machine) = (words.next()?, words.next_back()?);
    let os = if system.starts_with("Linux") {
        "linux"
    } else if system.starts_with("Darwin") {
        "macos"
    } else if ["Windows", "MINGW", "MSYS", "CYGWIN"]
        .iter()
        .any(|prefix| system.starts_with(prefix))
    {
        "windows"
    } else {
        return Some(format!(
            "{}-{}",
            system.to_lowercase(),
            machine.to_lowercase()
        ));
    };
    let arch = match machine.to_lowercase().as_str() {
        "x86_64" | "amd64" | "x64" => "x86_64".to_owned(),
        "arm64" | "aarch64" if os == "linux" => "aarch64".to_owned(),
        "arm64" | "aarch64" => "arm64".to_owned(),
        other => other.to_owned(),
    };
    Some(format!("{os}-{arch}"))
}

fn local_platform() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "aarch64" if os != "linux" => "arm64",
        arch => arch,
    };
    format!("{os}-{arch}")
}

async fn install(destination: &str, platform: &str, askpass: &Askpass) -> Result<(), Failure> {
    let binary = binary(platform).await?;
    let script = if platform.starts_with("windows") {
        windows_install()
    } else {
        unix_install()
    };
    let (output, stderr, _) = run(destination, &script, &binary, askpass).await?;
    if output.contains(INSTALLED) {
        Ok(())
    } else {
        Err(Failure::Fatal(format!(
            "Installing Adeline {VERSION} failed.{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!("\n{stderr}")
            }
        )))
    }
}

async fn binary(platform: &str) -> Result<Vec<u8>, Failure> {
    let release = download(platform).await;
    match release {
        Ok(bytes) => Ok(bytes),
        Err(_) if platform == local_platform() => {
            let exe = std::env::current_exe().map_err(|e| Failure::Fatal(e.to_string()))?;
            std::fs::read(&exe).map_err(|e| Failure::Fatal(crate::files::error(&exe, e)))
        }
        Err(error) => Err(Failure::Fatal(format!(
            "No Adeline {VERSION} release for {platform} ({error:#})."
        ))),
    }
}

async fn download(platform: &str) -> anyhow::Result<Vec<u8>> {
    // Cached per user: a shared temp folder would let another user plant the binary.
    let cache = crate::config::directory()?.join("cache");
    let folder = cache.join(format!("headless-{VERSION}-{platform}"));
    let exe = folder.join(if platform.starts_with("windows") {
        "adeline.exe"
    } else {
        "adeline"
    });
    if let Ok(bytes) = tokio::fs::read(&exe).await {
        return Ok(bytes);
    }
    // Extracted elsewhere and renamed into place, so an interrupted download is never reused.
    let staging = cache.join(crate::files::unique("headless"));
    tokio::fs::create_dir_all(&staging)
        .await
        .with_context(|| staging.display().to_string())?;
    let fetched = fetch(platform, &staging).await;
    if fetched.is_err() || tokio::fs::rename(&staging, &folder).await.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    fetched?;
    tokio::fs::read(&exe)
        .await
        .with_context(|| exe.display().to_string())
}

async fn fetch(platform: &str, folder: &std::path::Path) -> anyhow::Result<()> {
    let zip = folder.join("adeline.zip");
    let url = format!("{RELEASES}/v{VERSION}/adeline-headless-{platform}.zip");
    local_tool(
        "curl",
        &[
            "-fsSL".as_ref(),
            "-o".as_ref(),
            zip.as_os_str(),
            url.as_ref(),
        ],
    )
    .await?;
    if cfg!(target_os = "linux") {
        local_tool(
            "unzip",
            &[
                "-o".as_ref(),
                "-q".as_ref(),
                zip.as_os_str(),
                "-d".as_ref(),
                folder.as_os_str(),
            ],
        )
        .await?;
    } else {
        local_tool(
            "tar",
            &[
                "-xf".as_ref(),
                zip.as_os_str(),
                "-C".as_ref(),
                folder.as_os_str(),
            ],
        )
        .await?;
    }
    let _ = tokio::fs::remove_file(&zip).await;
    Ok(())
}

async fn local_tool(name: &str, args: &[&std::ffi::OsStr]) -> anyhow::Result<()> {
    // Windows uses its own copies, not ones a toolchain put first on PATH.
    #[cfg(windows)]
    let program = std::env::var_os("SystemRoot").map_or_else(
        || std::path::PathBuf::from(format!("{name}.exe")),
        |root| {
            std::path::PathBuf::from(root)
                .join("System32")
                .join(format!("{name}.exe"))
        },
    );
    #[cfg(not(windows))]
    let program = name;
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    let output = command.output().await.context(name.to_owned())?;
    if !output.status.success() {
        anyhow::bail!("{name}: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

async fn bridge(
    destination: &str,
    windows: bool,
    hello: &ClientMessage,
    askpass: &Askpass,
) -> Result<(Child, ipc::Reader, ipc::Writer, Status), Failure> {
    let command = if windows { WINDOWS_BRIDGE } else { UNIX_BRIDGE };
    let mut child = ssh(destination, command, askpass)
        .spawn()
        .map_err(|e| spawn_error(&e))?;
    let errors = drain(child.stderr.take());
    let stdout: Box<dyn AsyncRead + Send + Unpin> =
        Box::new(child.stdout.take().expect("piped stdout"));
    let mut reader = BufReader::new(stdout).lines();
    let mut writer: ipc::Writer = Box::new(child.stdin.take().expect("piped stdin"));
    if ipc::send(&mut writer, hello).await.is_ok() {
        // Shell start-up files may print before the engine does.
        while let Ok(Some(line)) = reader.next_line().await {
            if let Ok(EngineMessage::Welcome { status }) = serde_json::from_str(&line) {
                return Ok((child, reader, writer, status));
            }
        }
    }
    let _ = tokio::time::timeout(SSH_EXIT, child.wait()).await;
    Err(classify(&finish(errors).await))
}

pub fn bridge_main() -> i32 {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return 1;
    };
    runtime.block_on(async {
        let (mut engine_lines, mut engine) = match ipc::connect_or_start(false).await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("{error:#}");
                return 1;
            }
        };
        let mut client_lines = BufReader::new(tokio::io::stdin()).lines();
        let mut stdout = tokio::io::stdout();
        let upstream = async {
            while let Ok(Some(line)) = client_lines.next_line().await {
                let line = line + "\n";
                if engine.write_all(line.as_bytes()).await.is_err() || engine.flush().await.is_err()
                {
                    break;
                }
            }
        };
        let downstream = async {
            while let Ok(Some(line)) = engine_lines.next_line().await {
                let line = line + "\n";
                if stdout.write_all(line.as_bytes()).await.is_err() || stdout.flush().await.is_err()
                {
                    break;
                }
            }
        };
        tokio::select! {
            () = upstream => {},
            () = downstream => {},
        }
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_name_release_platforms() {
        let probe = parse_probe("motd\nADELINE-PROBE\nLinux x86_64\nabc\n0.1.5\n").unwrap();
        assert_eq!(probe.platform, "linux-x86_64");
        assert_eq!(probe.id, "abc");
        assert_eq!(probe.version, "0.1.5");
        let probe = parse_probe("ADELINE-PROBE\nDarwin arm64\n\n\n").unwrap();
        assert_eq!(probe.platform, "macos-arm64");
        assert!(probe.id.is_empty() && probe.version.is_empty());
        assert_eq!(platform("Windows AMD64").unwrap(), "windows-x86_64");
        assert_eq!(
            platform("MINGW64_NT-10.0-26100 x86_64").unwrap(),
            "windows-x86_64"
        );
        assert_eq!(platform("Linux aarch64").unwrap(), "linux-aarch64");
        assert_eq!(platform("Darwin x86_64").unwrap(), "macos-x86_64");
        assert!(parse_probe("'sh' is not recognized").is_none());
    }

    #[test]
    fn ssh_failures_sort_by_whether_retrying_helps() {
        assert!(matches!(
            classify("me@host: Permission denied (publickey)."),
            Failure::SignIn(_)
        ));
        assert!(matches!(
            classify("@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@"),
            Failure::Fatal(_)
        ));
        assert!(matches!(
            classify("Connection timed out"),
            Failure::Network(_)
        ));
    }
}
