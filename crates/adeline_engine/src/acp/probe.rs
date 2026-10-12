//! Probes and logins: short-lived sessions that ask an agent what it offers or
//! log it in. Dropping their `Session` stops the agent.
use super::*;

#[derive(Debug)]
pub struct Session {
    _driver: Driver,
}

pub fn probe(
    mut config: ExecutionConfig,
) -> (Session, async_channel::Receiver<Result<Probed, ProbeError>>) {
    let folder = std::env::temp_dir().join(adeline_core::files::unique("adeline-probe"));
    let _ = std::fs::create_dir_all(&folder);
    config.directory.clone_from(&folder);
    config.mcp_servers.clear();
    let (events, received) = async_channel::unbounded();
    let driver = Driver::spawn("probe".into(), config, None, events);
    let _ = driver.send(Command::Probe);
    let (sender, results) = async_channel::bounded(1);
    tokio::spawn(async move {
        let mut probed = Probed::default();
        let result = loop {
            let Ok(event) = received.recv().await else {
                break Err(ProbeError {
                    message: "The probe stopped.".into(),
                    auth: Vec::new(),
                    version: false,
                });
            };
            match event.kind {
                EventKind::Agent {
                    name,
                    version,
                    features,
                } => {
                    probed.identity = name;
                    probed.version = version;
                    probed.features = features;
                }
                EventKind::Options(options) => probed.options = options,
                EventKind::Probed => break Ok(probed),
                EventKind::Error { message, kind } => {
                    break Err(ProbeError {
                        version: kind == FailureKind::Version,
                        auth: if kind == FailureKind::Authentication {
                            probed.features.auth.clone()
                        } else {
                            Vec::new()
                        },
                        message,
                    });
                }
                _ => {}
            }
        };
        let _ = sender.try_send(result);
        let _ = block_in_place(|| std::fs::remove_dir_all(&folder));
    });
    (Session { _driver: driver }, results)
}

pub fn login(
    mut config: ExecutionConfig,
    method: Option<String>,
) -> (
    Session,
    async_channel::Receiver<Result<(), String>>,
    async_channel::Receiver<String>,
) {
    config.directory = std::env::temp_dir();
    config.mcp_servers.clear();
    let (events, received) = async_channel::unbounded();
    let driver = Driver::spawn("login".into(), config, None, events);
    let _ = driver.send(match method {
        Some(method) => Command::Authenticate { method },
        None => Command::Logout,
    });
    let (sender, results) = async_channel::bounded(1);
    let (url_sender, urls) = async_channel::unbounded();
    tokio::spawn(async move {
        let result = loop {
            match received.recv().await.map(|event| event.kind) {
                Ok(EventKind::Auth(result)) => break result,
                Ok(EventKind::OpenUrl(url)) => {
                    let _ = url_sender.try_send(url);
                }
                Ok(EventKind::Error { message, .. }) => break Err(message),
                Ok(_) => {}
                Err(_) => break Err("The agent stopped.".into()),
            }
        };
        let _ = sender.try_send(result);
    });
    (Session { _driver: driver }, results, urls)
}
