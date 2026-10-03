//! The local, same-user transport between the engine and its clients: a named
//! pipe on Windows, a Unix socket elsewhere. Messages are JSON lines.
use serde::Serialize;
use std::{io, path::PathBuf, time::Duration};
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader, Lines,
};

pub type Reader = Lines<BufReader<Box<dyn AsyncRead + Send + Unpin>>>;
pub type Writer = Box<dyn AsyncWrite + Send + Unpin>;

pub fn engine_dir() -> Result<PathBuf, String> {
    Ok(crate::config::directory()?.join("engine"))
}

pub fn log_path() -> String {
    engine_dir().map_or_else(|error| error, |dir| dir.join("logs").display().to_string())
}

/// One pipe per configuration folder, which is one per OS user.
#[cfg(windows)]
fn pipe_name() -> Result<String, String> {
    let folder = crate::config::directory()?;
    let hash = folder
        .to_string_lossy()
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    Ok(format!(r"\\.\pipe\adeline-engine-{hash:016x}"))
}

#[cfg(unix)]
fn socket_path() -> Result<PathBuf, String> {
    Ok(engine_dir()?.join("engine.sock"))
}

fn split<S: AsyncRead + AsyncWrite + Send + 'static>(stream: S) -> (Reader, Writer) {
    let (read, write) = tokio::io::split(stream);
    let read: Box<dyn AsyncRead + Send + Unpin> = Box::new(read);
    (BufReader::new(read).lines(), Box::new(write))
}

/// Connects to the running engine. `NotFound` means none is running.
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

pub struct Listener {
    #[cfg(windows)]
    name: String,
    #[cfg(windows)]
    next: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
}

impl Listener {
    /// Call only while holding the engine lock, so no other engine listens.
    pub fn bind() -> Result<Self, String> {
        #[cfg(windows)]
        {
            let name = pipe_name()?;
            let next = crate::platform::owner_only_pipe(&name, true)
                .map_err(|e| format!("Cannot create the engine pipe: {e}"))?;
            Ok(Self { name, next })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let path = socket_path()?;
            let dir = engine_dir()?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| crate::files::error(&dir, e))?;
            // A socket left by a crashed engine; the lock proves it's stale.
            let _ = std::fs::remove_file(&path);
            let listener = tokio::net::UnixListener::bind(&path)
                .map_err(|e| format!("Cannot create the engine socket: {e}"))?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| crate::files::error(&path, e))?;
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

/// Starts `adeline engine` in the background, detached from this process.
pub fn spawn_engine(daemon: bool) -> io::Result<u32> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command.arg("engine");
    if daemon {
        command.arg("--daemon");
    }
    crate::platform::spawn_detached(&mut command).map(|child| child.id())
}

/// Connects, starting an engine first when none is running.
pub async fn connect_or_start(daemon: bool) -> Result<(Reader, Writer), String> {
    if let Ok(connection) = connect().await {
        return Ok(connection);
    }
    spawn_engine(daemon).map_err(|e| format!("Cannot start the conversation engine: {e}"))?;
    // Losing a start race is fine: the winner accepts instead.
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(connection) = connect().await {
            return Ok(connection);
        }
    }
    Err("The conversation engine did not accept a connection within 10 seconds.".into())
}
