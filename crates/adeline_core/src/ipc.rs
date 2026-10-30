//! The local, same-user transport between the engine and its clients: a named pipe
//! on Windows, a Unix socket elsewhere.
//!
//! Messages are JSON lines. `connect` fails with `NotFound` when no engine runs.
//! `engine/id` is the engine's lasting identity: remote clients read it over SSH to
//! tell engines apart. A UI started with `--control` listens on its own pipe or
//! socket, named after its process ID (`control_address`).
use anyhow::{Context as _, Result, bail};
use serde::Serialize;
use std::{io, path::PathBuf, time::Duration};
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader, Lines,
};

pub type Reader = Lines<BufReader<Box<dyn AsyncRead + Send + Unpin>>>;
pub type Writer = Box<dyn AsyncWrite + Send + Unpin>;

pub fn engine_dir() -> Result<PathBuf> {
    Ok(crate::config::directory()?.join("engine"))
}

pub fn engine_id() -> Result<String> {
    let path = engine_dir()?.join("id");
    if let Ok(id) = std::fs::read_to_string(&path)
        && !id.trim().is_empty()
    {
        return Ok(id.trim().to_owned());
    }
    std::fs::create_dir_all(engine_dir()?).with_context(|| path.display().to_string())?;
    crate::files::seed(&path, crate::files::random_id().as_bytes())?;
    std::fs::read_to_string(&path)
        .map(|id| id.trim().to_owned())
        .with_context(|| path.display().to_string())
}

pub fn log_path() -> String {
    engine_dir().map_or_else(
        |error| format!("{error:#}"),
        |dir| dir.join("logs").display().to_string(),
    )
}

// 64-bit FNV-1a: stable across Rust versions, unlike `DefaultHasher`, so every
// Adeline version finds the same pipe.
#[cfg(windows)]
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
#[cfg(windows)]
const FNV_PRIME: u64 = 0x0100_0000_01b3;

// One pipe per configuration folder, which is one per OS user.
#[cfg(windows)]
fn folder_hash() -> Result<u64> {
    let folder = crate::config::directory()?;
    Ok(folder
        .to_string_lossy()
        .to_lowercase()
        .bytes()
        .fold(FNV_OFFSET, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        }))
}

fn engine_address() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        Ok(format!(r"\\.\pipe\adeline-engine-{:016x}", folder_hash()?).into())
    }
    #[cfg(unix)]
    {
        Ok(engine_dir()?.join("engine.sock"))
    }
}

fn control_prefix() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        Ok(format!(r"\\.\pipe\adeline-ui-{:016x}-", folder_hash()?).into())
    }
    #[cfg(unix)]
    {
        Ok(engine_dir()?.join("ui-"))
    }
}

/// Where the control UI with process ID `pid` listens.
pub fn control_address(pid: u32) -> Result<PathBuf> {
    let mut address = control_prefix()?.into_os_string();
    address.push(pid.to_string());
    Ok(address.into())
}

/// The process IDs of control UIs with an endpoint, including ones that died
/// and left a socket behind.
pub fn control_pids() -> Result<Vec<u32>> {
    let prefix = control_prefix()?;
    let (Some(dir), Some(start)) = (prefix.parent(), prefix.file_name()) else {
        return Ok(Vec::new());
    };
    let start = start.to_string_lossy();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut pids: Vec<u32> = entries
        .filter_map(|entry| {
            let name = entry.ok()?.file_name();
            name.to_string_lossy().strip_prefix(&*start)?.parse().ok()
        })
        .collect();
    pids.sort_unstable();
    Ok(pids)
}

fn split<S: AsyncRead + AsyncWrite + Send + 'static>(stream: S) -> (Reader, Writer) {
    let (read, write) = tokio::io::split(stream);
    let read: Box<dyn AsyncRead + Send + Unpin> = Box::new(read);
    (BufReader::new(read).lines(), Box::new(write))
}

pub async fn connect() -> io::Result<(Reader, Writer)> {
    connect_to(&engine_address().map_err(io::Error::other)?).await
}

pub async fn connect_to(address: &std::path::Path) -> io::Result<(Reader, Writer)> {
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        const PIPE_BUSY: i32 = 231;
        let mut attempts = 0;
        loop {
            match ClientOptions::new().open(address) {
                Ok(client) => return Ok(split(client)),
                Err(error) if error.raw_os_error() == Some(PIPE_BUSY) && attempts < 100 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
    #[cfg(unix)]
    {
        match tokio::net::UnixStream::connect(address).await {
            Ok(stream) => Ok(split(stream)),
            // A socket left by a killed engine refuses connections.
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                Err(io::Error::new(io::ErrorKind::NotFound, error))
            }
            Err(error) => Err(error),
        }
    }
}

pub async fn send(writer: &mut Writer, message: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_string(message).map_err(io::Error::other)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await
}

#[derive(Debug)]
pub struct Listener {
    #[cfg(windows)]
    name: String,
    #[cfg(windows)]
    next: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
}

impl Listener {
    pub fn bind() -> Result<Self> {
        // Callers hold the engine lock, so no other engine listens.
        Self::bind_at(&engine_address()?).context("Cannot listen for clients")
    }

    /// Listens as this process's control UI. Must run inside a tokio runtime.
    pub fn bind_control() -> Result<Self> {
        std::fs::create_dir_all(engine_dir()?)?;
        Self::bind_at(&control_address(std::process::id())?)
            .context("Cannot open the control endpoint")
    }

    fn bind_at(address: &std::path::Path) -> Result<Self> {
        #[cfg(windows)]
        {
            let name = address.to_string_lossy().into_owned();
            let next = crate::platform::owner_only_pipe(&name, true)
                .with_context(|| format!("Cannot create the pipe {name}"))?;
            Ok(Self { name, next })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let dir = engine_dir()?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| dir.display().to_string())?;
            // A socket left by a crashed process. The engine lock, or the
            // process ID in the name, proves it's stale.
            let _ = std::fs::remove_file(address);
            let listener = tokio::net::UnixListener::bind(address)
                .with_context(|| format!("Cannot create the socket {}", address.display()))?;
            std::fs::set_permissions(address, std::fs::Permissions::from_mode(0o600))
                .with_context(|| address.display().to_string())?;
            Ok(Self { listener })
        }
    }

    pub async fn accept(&mut self) -> io::Result<(Reader, Writer)> {
        #[cfg(windows)]
        {
            self.next.connect().await?;
            let next = crate::platform::owner_only_pipe(&self.name, false)?;
            Ok(split(std::mem::replace(&mut self.next, next)))
        }
        #[cfg(unix)]
        {
            let (stream, _) = self.listener.accept().await?;
            Ok(split(stream))
        }
    }
}

fn spawn_engine(daemon: bool) -> io::Result<()> {
    let mut command = std::process::Command::new(crate::platform::own_exe()?);
    command.arg("engine");
    if daemon {
        command.arg("--daemon");
    }
    crate::platform::spawn_detached(&mut command).map(drop)
}

const START_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn connect_or_start(daemon: bool) -> Result<(Reader, Writer)> {
    if let Ok(connection) = connect().await {
        return Ok(connection);
    }
    spawn_engine(daemon).context("Cannot start the conversation engine")?;
    // Losing a start race is fine: the winner accepts instead.
    // A cold start on a slow disk can take several seconds.
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(connection) = connect().await {
            return Ok(connection);
        }
    }
    bail!(
        "The conversation engine did not accept a connection within {} seconds.",
        START_TIMEOUT.as_secs()
    )
}
