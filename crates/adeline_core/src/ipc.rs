//! The local, same-user transport between the engine and its clients: a named pipe
//! on Windows, a Unix socket elsewhere.
//!
//! Messages are JSON lines. `connect` fails with `NotFound` when no engine runs.
//! `engine/id` is the engine's lasting identity: remote clients read it over SSH to
//! tell engines apart.
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

#[cfg(windows)]
fn pipe_name() -> Result<String> {
    // One pipe per configuration folder, which is one per OS user.
    let folder = crate::config::directory()?;
    let hash = folder
        .to_string_lossy()
        .to_lowercase()
        .bytes()
        .fold(FNV_OFFSET, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        });
    Ok(format!(r"\\.\pipe\adeline-engine-{hash:016x}"))
}

#[cfg(unix)]
fn socket_path() -> Result<PathBuf> {
    Ok(engine_dir()?.join("engine.sock"))
}

fn split<S: AsyncRead + AsyncWrite + Send + 'static>(stream: S) -> (Reader, Writer) {
    let (read, write) = tokio::io::split(stream);
    let read: Box<dyn AsyncRead + Send + Unpin> = Box::new(read);
    (BufReader::new(read).lines(), Box::new(write))
}

pub async fn connect() -> io::Result<(Reader, Writer)> {
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        const PIPE_BUSY: i32 = 231;
        let name = pipe_name().map_err(io::Error::other)?;
        let mut attempts = 0;
        loop {
            match ClientOptions::new().open(&name) {
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
        let path = socket_path().map_err(io::Error::other)?;
        match tokio::net::UnixStream::connect(&path).await {
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
        #[cfg(windows)]
        {
            let name = pipe_name()?;
            let next = crate::platform::owner_only_pipe(&name, true)
                .context("Cannot create the engine pipe")?;
            Ok(Self { name, next })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let path = socket_path()?;
            let dir = engine_dir()?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| dir.display().to_string())?;
            // A socket left by a crashed engine; the lock proves it's stale.
            let _ = std::fs::remove_file(&path);
            let listener =
                tokio::net::UnixListener::bind(&path).context("Cannot create the engine socket")?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| path.display().to_string())?;
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
