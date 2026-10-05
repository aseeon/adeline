//! Remote machines through the system `ssh`: finding the remote platform and
//! its Adeline, installing this client's version there, and `adeline bridge`,
//! which joins the SSH session to the remote engine's local pipe or socket.
//! Prompts from `ssh` come back to the UI through `askpass`.
use crate::{
    ipc,
    protocol::{ClientMessage, Command, EngineMessage, PROTOCOL, Status},
};
use serde::{Deserialize, Serialize};
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
/// The platforms each release ships.
const SUPPORTED: [&str; 3] = ["windows-x86_64", "macos-arm64", "linux-x86_64"];
const MARKER: &str = "ADELINE-PROBE";
const INSTALLED: &str = "ADELINE-INSTALLED";

/// A saved remote machine, as the connection needs it.
#[derive(Clone)]
pub struct Remote {
    pub name: String,
    pub destinations: Vec<String>,
    /// The engine this machine was first connected to.
    pub engine: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// A timeout or network problem. Retried with growing delays.
    Network(String),
    /// A rejected password or key. Not retried until the user asks.
    SignIn(String),
    /// Retrying wouldn't help: a refused host key, the wrong engine, no build.
    Fatal(String),
    Unsupported(String),
    /// The remote runs a newer Adeline than this client.
    LocalUpdate(String),
    /// An older engine runs there; replacing it stops its agents.
    Upgrade {
        version: String,
        active: usize,
    },
}

/// A connected remote engine, after its welcome.
pub struct Link {
    pub reader: ipc::Reader,
    pub writer: ipc::Writer,
    pub status: Status,
    pub destination: String,
    /// The `ssh` process; dropping it ends the session.
    pub child: Child,
}

/// Where each `ssh` process sends its prompts, and for which machine.
#[derive(Clone)]
pub struct Askpass {
    pub address: String,
    pub machine: String,
}

/// Each `ssh` run is a session; a password typed in one session of a connect
/// answers the same prompt in the next one.
static SESSION: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Connecting

/// Tries each destination in order and returns the first that reaches the
/// machine's engine. `upgrade` is the user's consent to restart an older engine.
pub async fn connect(
    remote: &Remote,
    hello: &ClientMessage,
    upgrade: bool,
    askpass: &Askpass,
) -> Result<Link, Failure> {
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
    remote: &Remote,
    destination: &str,
    hello: &ClientMessage,
    upgrade: bool,
    askpass: &Askpass,
) -> Result<Link, Failure> {
    let probe = probe(destination, askpass).await?;
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
    match compare(&probe.version) {
        std::cmp::Ordering::Greater => return Err(Failure::LocalUpdate(probe.version)),
        std::cmp::Ordering::Less => install(destination, &probe.platform, askpass).await?,
        std::cmp::Ordering::Equal => {}
    }
    let windows = probe.platform.starts_with("windows");
    for round in 0..2 {
        let (child, reader, writer, status) = bridge(destination, windows, hello, askpass).await?;
        if let Some(saved) = &remote.engine
            && status.engine_id != *saved
        {
            return Err(wrong(&status.engine_id));
        }
        if status.protocol == PROTOCOL && status.version == VERSION {
            return Ok(Link {
                reader,
                writer,
                status,
                destination: destination.to_owned(),
                child,
            });
        }
        if compare(&status.version) == std::cmp::Ordering::Greater {
            return Err(Failure::LocalUpdate(status.version));
        }
        if !upgrade || round > 0 {
            return Err(Failure::Upgrade {
                version: status.version,
                active: status.conversations.len(),
            });
        }
        shut_down(child, reader, writer).await;
    }
    Err(Failure::Network(
        "The older engine didn't make way for this version.".into(),
    ))
}

/// Asks an older engine to stop its agents and exit, and waits until it has.
async fn shut_down(mut child: Child, mut reader: ipc::Reader, mut writer: ipc::Writer) {
    let request = ClientMessage::Request {
        id: u64::MAX,
        command: Command::Shutdown,
    };
    if ipc::send(&mut writer, &request).await.is_ok() {
        let _ = tokio::time::timeout(Duration::from_secs(45), async {
            while let Ok(Some(_)) = reader.next_line().await {}
        })
        .await;
    }
    drop(writer);
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
}

fn short_id(id: &str) -> &str {
    if id.is_empty() {
        "a new engine"
    } else {
        &id[..id.len().min(8)]
    }
}

// ---------------------------------------------------------------------------
// Running ssh

/// `user@host:port` splits into the destination and its port; anything else,
/// such as a `Host` alias, goes to `ssh` unchanged.
fn split_port(destination: &str) -> (&str, Option<&str>) {
    match destination.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty()
                && !host.contains(':')
                && !port.is_empty()
                && port.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (host, Some(port))
        }
        _ => (destination, None),
    }
}

/// Checks a destination as typed: `user@host[:port]` or a `Host` alias.
pub fn check_destination(destination: &str) -> Result<(), String> {
    let (host, _) = split_port(destination);
    if host.is_empty()
        || host.starts_with('-')
        || host.contains(char::is_whitespace)
        || host.ends_with('@')
        || host.starts_with('@')
    {
        return Err(format!(
            "\"{destination}\" isn't a destination. Use user@host, user@host:port, or a Host alias from ~/.ssh/config."
        ));
    }
    Ok(())
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

/// Collects a child's error output as it comes, so a full pipe never stalls it.
fn drain(stderr: Option<tokio::process::ChildStderr>) -> Arc<Mutex<String>> {
    let text = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = stderr {
        let sink = text.clone();
        tokio::spawn(async move {
            let mut buffer = [0; 4096];
            while let Ok(read) = stderr.read(&mut buffer).await {
                if read == 0 {
                    break;
                }
                if let Ok(mut text) = sink.lock() {
                    text.push_str(&String::from_utf8_lossy(&buffer[..read]));
                    if text.len() > 16_384 {
                        let cut = text.len() - 8_192;
                        let cut = (cut..text.len())
                            .find(|&i| text.is_char_boundary(i))
                            .unwrap_or(0);
                        text.drain(..cut);
                    }
                }
            }
        });
    }
    text
}

fn taken(text: &Arc<Mutex<String>>) -> String {
    text.lock()
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

/// Sorts an `ssh` failure by whether trying again could help.
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

/// Runs one remote command to completion, feeding it `input`.
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
    // Let the last error output arrive.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let stderr = taken(&errors);
    if status.code() == Some(255) {
        return Err(classify(&stderr));
    }
    Ok((
        String::from_utf8_lossy(&output).into_owned(),
        stderr,
        status.success(),
    ))
}

// ---------------------------------------------------------------------------
// What runs on the remote machine

/// A PowerShell script that runs the same whether the remote's SSH shell is
/// cmd or PowerShell: base64 survives both shells' quoting.
fn powershell(script: &str) -> String {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64(&utf16)
    )
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The SSH session starts in the home folder on every platform, so remote
/// paths are relative to it.
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

/// Joins the session to the remote engine, through a login shell so agents
/// find the user's usual PATH.
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

/// `uname -sm` (or the Windows probe's line) as a release platform name.
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

/// Compares a remote version with this client's. A missing or unreadable
/// version is older.
fn compare(version: &str) -> std::cmp::Ordering {
    let parse = |text: &str| -> Vec<u64> {
        text.trim()
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    if version.trim().is_empty() {
        return std::cmp::Ordering::Less;
    }
    parse(version).cmp(&parse(VERSION))
}

// ---------------------------------------------------------------------------
// Installing

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

/// The client's version for `platform`: from its GitHub release, or this
/// very executable when there's no release and the platforms match.
async fn binary(platform: &str) -> Result<Vec<u8>, Failure> {
    let release = download(platform).await;
    match release {
        Ok(bytes) => Ok(bytes),
        Err(_) if platform == local_platform() => {
            let exe = std::env::current_exe().map_err(|e| Failure::Fatal(e.to_string()))?;
            std::fs::read(&exe).map_err(|e| Failure::Fatal(crate::files::error(&exe, e)))
        }
        Err(error) => Err(Failure::Fatal(format!(
            "No Adeline {VERSION} release for {platform} ({error})."
        ))),
    }
}

/// Downloads and unpacks a release build, keeping it for other machines.
async fn download(platform: &str) -> Result<Vec<u8>, String> {
    let folder = std::env::temp_dir().join(format!("adeline-{VERSION}-{platform}"));
    let exe = folder.join(if platform.starts_with("windows") {
        "adeline.exe"
    } else {
        "adeline"
    });
    if let Ok(bytes) = std::fs::read(&exe) {
        return Ok(bytes);
    }
    std::fs::create_dir_all(&folder).map_err(|e| crate::files::error(&folder, e))?;
    let zip = folder.join("adeline.zip");
    let url = format!("{RELEASES}/v{VERSION}/adeline-{platform}.zip");
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
    let _ = std::fs::remove_file(&zip);
    std::fs::read(&exe).map_err(|e| crate::files::error(&exe, e))
}

/// Runs `curl`, `tar` or `unzip` on this machine. Windows uses its own copies,
/// not ones a toolchain put first on PATH.
async fn local_tool(name: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
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
    let output = command.output().await.map_err(|e| format!("{name}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

// ---------------------------------------------------------------------------
// The bridge

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
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    Err(classify(&taken(&errors)))
}

/// `adeline bridge`: joins this process's stdin and stdout to the local
/// engine, starting it when it isn't running. A remote client runs it over SSH.
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
                eprintln!("{error}");
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
        // Either side closing ends the bridge.
        tokio::select! {
            () = upstream => {},
            () = downstream => {},
        }
        0
    })
}

// ---------------------------------------------------------------------------
// Askpass

/// A prompt from `ssh`, waiting for the user.
pub struct Prompt {
    pub machine: String,
    pub session: u64,
    pub text: String,
    pub reply: async_channel::Sender<Option<String>>,
}

impl Prompt {
    /// An unknown host key, answered yes or no rather than typed.
    pub fn host_key(&self) -> bool {
        self.text.contains("(yes/no")
    }
}

#[derive(Serialize, Deserialize)]
struct Ask {
    asker: String,
    prompt: String,
}

/// Listens for this process's askpass helpers. Call inside the client
/// runtime. Returns the address `ssh` children get.
pub fn serve_askpass(prompts: async_channel::Sender<Prompt>) -> Result<String, String> {
    #[cfg(windows)]
    {
        let address = format!(
            r"\\.\pipe\adeline-askpass-{}-{}",
            std::process::id(),
            crate::files::random_id()
        );
        let mut server = crate::platform::owner_only_pipe(&address, true)
            .map_err(|e| format!("Cannot listen for SSH prompts: {e}"))?;
        let name = address.clone();
        tokio::spawn(async move {
            while server.connect().await.is_ok() {
                let Ok(next) = crate::platform::owner_only_pipe(&name, false) else {
                    return;
                };
                let stream = std::mem::replace(&mut server, next);
                tokio::spawn(answer(stream, prompts.clone()));
            }
        });
        Ok(address)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let path = crate::config::directory()?.join(format!("askpass-{}.sock", std::process::id()));
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path)
            .map_err(|e| format!("Cannot listen for SSH prompts: {e}"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| crate::files::error(&path, e))?;
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(answer(stream, prompts.clone()));
            }
        });
        Ok(path.display().to_string())
    }
}

async fn answer<S: AsyncRead + tokio::io::AsyncWrite + Send + 'static>(
    stream: S,
    prompts: async_channel::Sender<Prompt>,
) {
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let Ok(Some(line)) = lines.next_line().await else {
        return;
    };
    let Ok(ask) = serde_json::from_str::<Ask>(&line) else {
        return;
    };
    let (machine, session) = ask.asker.split_once('\t').unwrap_or((&ask.asker, "0"));
    let (reply, replies) = async_channel::bounded(1);
    let prompt = Prompt {
        machine: machine.to_owned(),
        session: session.parse().unwrap_or(0),
        text: ask.prompt,
        reply,
    };
    if prompts.send(prompt).await.is_err() {
        return;
    }
    let answer = replies.recv().await.ok().flatten();
    if let Ok(mut line) = serde_json::to_string(&answer) {
        line.push('\n');
        let _ = write.write_all(line.as_bytes()).await;
        let _ = write.flush().await;
    }
}

/// The askpass helper: `ssh` runs this executable with a prompt, and the
/// answer printed here is what it sends. Exits 1 when the user cancels.
pub fn askpass_main(address: &str, prompt: &str) -> i32 {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return 1;
    };
    let asker = std::env::var("ADELINE_ASKPASS_FOR").unwrap_or_default();
    let answer: Option<String> = runtime.block_on(async {
        #[cfg(windows)]
        let stream = {
            let mut attempts = 0;
            loop {
                match tokio::net::windows::named_pipe::ClientOptions::new().open(address) {
                    Ok(stream) => break stream,
                    Err(_) if attempts < 100 => {
                        attempts += 1;
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    Err(_) => return None,
                }
            }
        };
        #[cfg(unix)]
        let stream = tokio::net::UnixStream::connect(address).await.ok()?;
        let (read, mut write) = tokio::io::split(stream);
        let mut line = serde_json::to_string(&Ask {
            asker,
            prompt: prompt.to_owned(),
        })
        .ok()?;
        line.push('\n');
        write.write_all(line.as_bytes()).await.ok()?;
        write.flush().await.ok()?;
        let reply = BufReader::new(read).lines().next_line().await.ok()??;
        serde_json::from_str(&reply).ok()?
    });
    match answer {
        Some(answer) => {
            println!("{answer}");
            0
        }
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destinations_split_off_a_numeric_port() {
        assert_eq!(
            split_port("me@home.example.com:2222"),
            ("me@home.example.com", Some("2222"))
        );
        assert_eq!(split_port("desktop.lan"), ("desktop.lan", None));
        assert_eq!(split_port("alias"), ("alias", None));
        assert!(check_destination("me@host:22").is_ok());
        assert!(check_destination("desktop").is_ok());
        assert!(check_destination("-oProxyCommand=x").is_err());
        assert!(check_destination("me@").is_err());
        assert!(check_destination("two words").is_err());
    }

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
    fn versions_compare_numerically() {
        use std::cmp::Ordering::*;
        assert_eq!(compare(VERSION), Equal);
        assert_eq!(compare(""), Less);
        assert_eq!(compare("0.0.1"), Less);
        assert_eq!(compare("99.0.0"), Greater);
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64(b"abcdef"), "YWJjZGVm");
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
