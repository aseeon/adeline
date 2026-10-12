//! The `SSH_ASKPASS` helper. `ssh` runs this executable with `ADELINE_ASKPASS`
//! set, and `askpass_main` hands the prompt to the client that started `ssh`.
//! The client listens on an owner-only named pipe (Windows) or a 0600 Unix
//! socket, so only the same user can ask or read answers. One JSON line goes
//! each way: the `Ask`, then the answer, `null` when the user cancels.
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncWriteExt as _, BufReader};

pub struct Prompt {
    pub machine: String,
    pub session: u64,
    pub text: String,
    pub reply: async_channel::Sender<Option<String>>,
}

impl Prompt {
    pub fn host_key(&self) -> bool {
        self.text.contains("(yes/no")
    }
}

#[derive(Serialize, Deserialize)]
struct Ask {
    asker: String,
    prompt: String,
}

pub fn serve_askpass(prompts: async_channel::Sender<Prompt>) -> Result<String, String> {
    // Runs inside the client runtime. Returns the address `ssh` children get.
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

pub fn askpass_main(address: &str, prompt: &str) -> i32 {
    // `ssh` sends the answer printed here. Exits 1 when the user cancels.
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
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
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
