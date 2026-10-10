//! The conversation engine: the background process that owns agent processes,
//! conversation state and every engine-owned file. One runs per OS user.
//!
//! Every command and agent event goes through one task, in arrival order, so
//! commands from several clients can't interleave or deadlock.
use crate::{
    acp,
    agents::{self, AgentCatalog},
    config,
    conversation::{Attachment, AuthMethod, Category, PermissionKind, StopReason, TurnState},
    data::{Message, Thread, Workspace},
    harness, install, ipc,
    protocol::{
        ActiveConversation, ClientMessage, Command, Delta, EngineMessage, EngineSettings, Live,
        PROTOCOL, PendingPermission, Queued, Snapshot, Status,
    },
    storage::{self, ProjectStore},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

const IDLE_EXIT: Duration = Duration::from_secs(60);
/// Deltas kept for clients that reconnect after a short drop.
const RECENT_DELTAS: usize = 20_000;
const STOP_GRACE: Duration = Duration::from_secs(5);
/// No traffic for this long during a turn shows the agent as quiet.
const QUIET_AFTER_MS: u64 = 5_000;
const ALREADY_PROCESSING: &str =
    "This conversation is already processing (sent from another window)";
const PERMISSION_STOPPED: &str = "Stopped while waiting for permission: no Adeline window was open to answer it. Retry continues the turn.";

/// `adeline engine [status|start|stop] [--daemon]`. Returns the exit code.
pub fn main(args: &[String]) -> i32 {
    crate::platform::attach_parent_console();
    let daemon = args.iter().any(|arg| arg == "--daemon");
    match args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(String::as_str)
    {
        None => run(daemon),
        Some("status") => cli(cli_status),
        Some("start") => cli(move || cli_start(daemon)),
        Some("stop") => cli(cli_stop),
        Some(other) => {
            eprintln!("Unknown engine command {other}. Use status, start [--daemon] or stop.");
            2
        }
    }
}

fn cli<F: Future<Output = i32>>(run: impl FnOnce() -> F) -> i32 {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run()),
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

/// A command-line request: connects without counting as a client.
async fn request(command: Command) -> Result<(Value, ipc::Reader), String> {
    let (mut reader, mut writer) = ipc::connect().await.map_err(|e| e.to_string())?;
    let hello = ClientMessage::Hello {
        protocol: PROTOCOL,
        cli: true,
        resume: None,
    };
    let request = ClientMessage::Request { id: 1, command };
    for message in [hello, request] {
        ipc::send(&mut writer, &message)
            .await
            .map_err(|e| e.to_string())?;
    }
    while let Ok(Some(line)) = reader.next_line().await {
        if let Ok(EngineMessage::Reply { id: 1, result }) = serde_json::from_str(&line) {
            // Keep the writer open until the reader is done with it.
            drop(writer);
            return result.map(|value| (value, reader));
        }
    }
    Err("The conversation engine closed the connection.".into())
}

async fn cli_status() -> i32 {
    let Ok((value, _)) = request(Command::Status).await else {
        println!("Conversation engine: not running");
        return 1;
    };
    let Ok(status) = serde_json::from_value::<Status>(value) else {
        println!("Conversation engine: running (unreadable status)");
        return 0;
    };
    let uptime = status.uptime_secs;
    println!("Conversation engine: running");
    println!("PID: {}", status.pid);
    println!("Version: {}", status.version);
    println!("Build: {}", crate::build(status.headless));
    println!("Protocol: {}", status.protocol);
    println!("Daemon mode: {}", if status.daemon { "on" } else { "off" });
    println!(
        "Uptime: {}h {}m {}s",
        uptime / 3600,
        uptime / 60 % 60,
        uptime % 60
    );
    println!("Connected clients: {}", status.clients);
    println!("Active conversations: {}", status.conversations.len());
    for conversation in &status.conversations {
        println!(
            "  {} / {} - {}",
            conversation.project, conversation.title, conversation.state
        );
    }
    0
}

async fn cli_start(daemon: bool) -> i32 {
    if let Ok((value, _)) = request(Command::Status).await {
        let pid = value["pid"].as_u64().unwrap_or_default();
        println!("The conversation engine is already running (PID {pid}).");
        return 0;
    }
    match ipc::connect_or_start(daemon).await {
        Ok(_) => match request(Command::Status).await {
            Ok((value, _)) => {
                let pid = value["pid"].as_u64().unwrap_or_default();
                println!("Started the conversation engine (PID {pid}).");
                0
            }
            Err(error) => {
                println!("{error}");
                1
            }
        },
        Err(error) => {
            println!("{error} See {}.", ipc::log_path());
            1
        }
    }
}

async fn cli_stop() -> i32 {
    let Ok((value, mut reader)) = request(Command::Shutdown).await else {
        println!("The conversation engine is not running.");
        return 0;
    };
    let stopped: Vec<String> = serde_json::from_value(value["stopped"].clone()).unwrap_or_default();
    if stopped.is_empty() {
        println!("No agents were running.");
    } else {
        println!("Stopped {} agent(s):", stopped.len());
        for name in stopped {
            println!("  {name}");
        }
    }
    // The engine holds its lock until the process has exited.
    let exited = tokio::time::timeout(Duration::from_secs(30), async {
        while let Ok(Some(_)) = reader.next_line().await {}
        let lock = ipc::engine_dir().ok().and_then(|dir| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(dir.join("engine.lock"))
                .ok()
        });
        while lock.as_ref().is_some_and(|lock| lock.try_lock().is_err()) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    if exited.is_ok() {
        println!("Conversation engine stopped.");
        0
    } else {
        println!("The conversation engine is still exiting after 30 seconds.");
        1
    }
}

// ---------------------------------------------------------------------------
// Logging

static LOG: Mutex<Option<PathBuf>> = Mutex::new(None);
const LOG_CAP: u64 = 1024 * 1024;

/// Appends a line to `engine/logs/engine.log`, rotating at 1 MB through three files.
pub fn log(message: impl std::fmt::Display) {
    let Ok(guard) = LOG.lock() else {
        return;
    };
    let Some(dir) = guard.as_ref() else {
        return;
    };
    let path = dir.join("engine.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > LOG_CAP) {
        for index in (1..3).rev() {
            let _ = std::fs::rename(
                dir.join(format!("engine.{index}.log")),
                dir.join(format!("engine.{}.log", index + 1)),
            );
        }
        let _ = std::fs::rename(&path, dir.join("engine.1.log"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(
            file,
            "{} [{}] {message}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            std::process::id()
        );
    }
}

// ---------------------------------------------------------------------------
// Settings

fn settings_path() -> Result<PathBuf, String> {
    Ok(ipc::engine_dir()?.join("settings.yml"))
}

/// Reads `engine/settings.yml`. The first start copies the retry limit from
/// the UI's settings.
fn load_settings() -> EngineSettings {
    let defaults = EngineSettings {
        keep_running: false,
        retry_limit: 5,
        mcp_servers: Vec::new(),
    };
    let Ok(path) = settings_path() else {
        return defaults;
    };
    if let Ok(text) = std::fs::read_to_string(&path) {
        return match serde_yaml_ng::from_str(&text) {
            Ok(settings) => settings,
            Err(error) => {
                log(format!("{}: {error}", path.display()));
                defaults
            }
        };
    }
    let retry_limit = config::directory()
        .ok()
        .and_then(|root| std::fs::read_to_string(root.join("settings.yml")).ok())
        .and_then(|text| serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text).ok())
        .and_then(|yaml| yaml["modes"]["chats"]["retry_limit"].as_u64())
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(defaults.retry_limit);
    let settings = EngineSettings {
        retry_limit,
        ..defaults
    };
    if let Err(error) = config::seed_yaml(&path, &settings) {
        log(error);
    }
    settings
}

// ---------------------------------------------------------------------------
// The engine process

fn run(daemon: bool) -> i32 {
    let dir = match ipc::engine_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if let Err(error) = std::fs::create_dir_all(dir.join("logs")) {
        eprintln!("{}", crate::files::error(&dir, error));
        return 1;
    }
    let lock = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("engine.lock"))
    {
        Ok(lock) => lock,
        Err(error) => {
            eprintln!("{}", crate::files::error(&dir, error));
            return 1;
        }
    };
    // Another engine won a simultaneous start; its clients connect to it.
    if lock.try_lock().is_err() {
        return 0;
    }
    if let Ok(mut guard) = LOG.lock() {
        *guard = Some(dir.join("logs"));
    }
    log(format!(
        "Engine {} starting (protocol {PROTOCOL}{})",
        env!("CARGO_PKG_VERSION"),
        if daemon { ", --daemon" } else { "" }
    ));
    if let Err(error) = crate::platform::contain_children(&dir) {
        log(format!("Cannot tie agent processes to the engine: {error}"));
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(64)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log(&error);
            return 1;
        }
    };
    let code = runtime.block_on(serve(daemon));
    // A detection or probe still running mustn't hold up the exit.
    runtime.shutdown_timeout(Duration::from_secs(1));
    crate::platform::stop_children();
    log("Engine exited");
    drop(lock);
    code
}

#[expect(
    clippy::large_enum_variant,
    reason = "each input is moved once, straight to the engine"
)]
enum Input {
    Connected {
        client: u64,
        sender: mpsc::UnboundedSender<String>,
        writer: tokio::task::JoinHandle<()>,
    },
    Message {
        client: u64,
        message: ClientMessage,
    },
    Disconnected {
        client: u64,
    },
    Watch(Vec<PathBuf>),
    ProjectsLoaded {
        store: ProjectStore,
        started: Instant,
    },
    Harnesses(harness::Catalog),
    ProbeDone {
        client: u64,
        request: u64,
        probe: u64,
        result: Option<Result<acp::Probed, acp::ProbeError>>,
    },
    /// A line of a running install's output.
    Output {
        client: u64,
        request: u64,
        line: String,
    },
    /// An install, login or logout finished.
    Done {
        client: u64,
        request: u64,
        result: Result<Value, String>,
        /// Look for installed agents again.
        detect: bool,
    },
    /// A login asked to open a page.
    OpenUrl(String),
}

async fn serve(daemon: bool) -> i32 {
    let mut listener = match ipc::Listener::bind() {
        Ok(listener) => listener,
        Err(error) => {
            log(&error);
            return 1;
        }
    };
    let (input, mut inputs) = mpsc::unbounded_channel();
    let (events, agent_events) = async_channel::unbounded();
    let mut engine = Engine::new(daemon, input.clone(), events);
    let accept_input = input.clone();
    tokio::spawn(async move {
        let mut next = 0;
        loop {
            match listener.accept().await {
                Ok((reader, writer)) => {
                    next += 1;
                    connection(next, reader, writer, &accept_input);
                }
                Err(error) => {
                    log(format!("Accepting a client failed: {error}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    while !engine.exit {
        tokio::select! {
            Some(input) = inputs.recv() => engine.input(input),
            Ok(event) = agent_events.recv() => engine.driver_event(event),
            _ = tick.tick() => engine.tick(),
        }
        engine.refresh_status();
    }
    // Let writers flush the last replies and the goodbye.
    let writers: Vec<_> = engine
        .clients
        .drain()
        .map(|(_, client)| client.writer)
        .collect();
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        for writer in writers {
            let _ = writer.await;
        }
    })
    .await;
    0
}

fn connection(
    client: u64,
    mut reader: ipc::Reader,
    mut writer: ipc::Writer,
    input: &mpsc::UnboundedSender<Input>,
) {
    let (sender, mut outgoing) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        while let Some(line) = outgoing.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            // Batch whatever else is queued before flushing.
            while let Ok(line) = outgoing.try_recv() {
                if writer.write_all(line.as_bytes()).await.is_err() {
                    return;
                }
            }
            if writer.flush().await.is_err() {
                break;
            }
        }
    });
    let _ = input.send(Input::Connected {
        client,
        sender,
        writer,
    });
    let input = input.clone();
    tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            match serde_json::from_str(&line) {
                Ok(message) => {
                    let _ = input.send(Input::Message { client, message });
                }
                Err(error) => log(format!(
                    "Client {client} sent an unreadable message: {error}"
                )),
            }
        }
        let _ = input.send(Input::Disconnected { client });
    });
}

struct Client {
    sender: mpsc::UnboundedSender<String>,
    writer: tokio::task::JoinHandle<()>,
    /// A UI with a matching protocol; it gets the state and every change.
    subscribed: bool,
}

struct Slot {
    driver: acp::Driver,
    turn: u64,
}

struct StopAll {
    deadline: Instant,
    forced: bool,
    waiters: Vec<(u64, u64)>,
    stopped: Vec<String>,
    exit: bool,
}

struct Engine {
    store: Arc<Mutex<ProjectStore>>,
    projects: Vec<Workspace>,
    live: HashMap<String, Live>,
    drivers: HashMap<String, Slot>,
    agents: AgentCatalog,
    harnesses: harness::Catalog,
    icons: BTreeMap<String, String>,
    settings: EngineSettings,
    daemon_flag: bool,
    clients: HashMap<u64, Client>,
    input: mpsc::UnboundedSender<Input>,
    events: async_channel::Sender<acp::Event>,
    started: Instant,
    idle_since: Option<Instant>,
    stop_all: Option<StopAll>,
    deleting: HashMap<String, Vec<(u64, u64)>>,
    probes: HashMap<(u64, u64), acp::Session>,
    /// Logins and logouts in progress, by client and request.
    logins: HashMap<(u64, u64), acp::Session>,
    /// The conversation each client watches the ACP traffic of.
    watching: HashMap<u64, String>,
    /// Queued messages' files, by queued message ID.
    queued_files: HashMap<u64, Vec<Attachment>>,
    next_queued: u64,
    /// The files of each conversation's last prompt, for Retry.
    attachments: HashMap<String, Vec<Attachment>>,
    /// A Send now waiting for the agent's steering answer, per conversation.
    steering: HashMap<String, (String, Vec<Attachment>)>,
    /// Conversations whose queue goes out once their cancelled turn settles.
    send_after_stop: HashSet<String>,
    /// When each running conversation's agent last sent anything, in ms.
    last_traffic: HashMap<String, u64>,
    permission_stopped: HashSet<String>,
    /// Forks whose copied history went out as text but no turn has finished yet.
    text_copy_pending: HashSet<String>,
    /// Turns waiting for their conversation's closing agent to exit.
    queued: HashMap<String, String>,
    /// When the engine itself last wrote each conversation, for file watching.
    touched: HashMap<String, Instant>,
    watched: Vec<PathBuf>,
    watch_due: Option<Instant>,
    _watcher: Option<notify::RecommendedWatcher>,
    status: Status,
    exit: bool,
    identity: String,
    /// This process's delta stream: every delta sent so far is numbered, and
    /// the latest ones are kept for reconnecting clients.
    epoch: String,
    seq: u64,
    recent: std::collections::VecDeque<(u64, String)>,
}

/// Runs blocking file work without stalling the runtime's other tasks.
fn blocking<T>(work: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(work)
}

fn live_for(conversation: &storage::StoredConversation) -> Live {
    let thread = conversation.to_thread();
    // A fork's copied prompts belong to its source's turns.
    let copied = conversation
        .settings
        .forked_from
        .as_ref()
        .map_or(0, |origin| origin.message + 1);
    let last_user = thread
        .messages
        .iter()
        .rposition(|m| m.role == "user")
        .filter(|&i| i >= copied);
    let interrupted = matches!(
        conversation.settings.status.as_str(),
        "processing" | "blocked"
    );
    let last_error = conversation
        .events
        .iter()
        .chain(&conversation.unsaved_events)
        .rev()
        .take_while(|event| event.kind != "message" || event.data["role"] != "user")
        .find_map(|event| match event.kind.as_str() {
            "error" => event.data.get("message").and_then(Value::as_str),
            "lifecycle" => event.data.get("error").and_then(Value::as_str),
            _ => None,
        });
    let error = conversation
        .storage_error
        .clone()
        .map(|error| {
            format!(
                "History could not be saved: {error}. Restore storage access, then Retry storage."
            )
        })
        .or_else(|| {
            interrupted.then(|| {
                last_error.map_or_else(
                    || {
                        "The previous turn was interrupted. Retry continues from the saved session."
                            .to_owned()
                    },
                    str::to_owned,
                )
            })
        });
    Live {
        agent_id: conversation.settings.agent_id.clone(),
        storage_failed: conversation.storage_error.is_some(),
        execution: Some(conversation.settings.execution.clone()),
        options: conversation.settings.options.clone(),
        commands: conversation.settings.commands.clone(),
        features: conversation.settings.features.clone(),
        todo: conversation.settings.todo.clone(),
        last_read_through: thread
            .messages
            .iter()
            .rposition(|message| message.role == "assistant" && message.read),
        error,
        last_prompt: last_user
            .map(|i| thread.messages[i].text.clone())
            .unwrap_or_default(),
        worked: interrupted || last_user.is_some_and(|i| i + 1 < thread.messages.len()),
        ..Default::default()
    }
}

/// A folder's entries for a client browsing this machine; the home folder by default.
fn list_directory(path: Option<PathBuf>) -> Result<crate::protocol::Listing, String> {
    let path = match path.filter(|path| !path.as_os_str().is_empty()) {
        Some(path) => path,
        None => config::directory()?
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or("Could not find your home directory.")?,
    };
    let mut entries: Vec<_> = std::fs::read_dir(&path)
        .map_err(|e| crate::files::error(&path, e))?
        .flatten()
        .map(|entry| crate::protocol::Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            directory: entry.path().is_dir(),
        })
        .collect();
    entries.sort_by_key(|entry| (!entry.directory, entry.name.to_lowercase()));
    Ok(crate::protocol::Listing {
        parent: path.parent().map(Path::to_path_buf),
        path,
        entries,
    })
}

/// Messages as `role: text` paragraphs, for giving an agent saved history.
fn history_text(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Registry icons and agent avatars, which clients can't read from disk.
fn disk_icons() -> BTreeMap<String, String> {
    let mut icons = BTreeMap::new();
    let Ok(root) = config::directory() else {
        return icons;
    };
    let mut add = |folder: PathBuf, key: &dyn Fn(&str) -> Option<String>, file: &str| {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if file.is_empty() {
                entry.path()
            } else {
                entry.path().join(file)
            };
            if let (Some(key), Ok(svg)) = (key(&name), std::fs::read_to_string(path)) {
                icons.insert(key, svg);
            }
        }
    };
    add(
        root.join("cache").join("registry-icons"),
        &|name| {
            name.strip_suffix(".svg")
                .map(|id| format!("registry-icons/{id}.svg"))
        },
        "",
    );
    add(
        root.join("agents"),
        &|name| {
            crate::files::checked_id(name)
                .ok()
                .map(|()| format!("agent-avatars/{name}.svg"))
        },
        agents::AVATAR,
    );
    icons
}

impl Engine {
    fn new(
        daemon: bool,
        input: mpsc::UnboundedSender<Input>,
        events: async_channel::Sender<acp::Event>,
    ) -> Self {
        let store = blocking(ProjectStore::new);
        for error in &store.errors {
            log(error);
        }
        let projects = store.to_workspaces();
        let live = store
            .projects
            .iter()
            .flat_map(|project| &project.conversations)
            .map(|conversation| (conversation.id.clone(), live_for(conversation)))
            .collect();
        let mut engine = Self {
            store: Arc::new(Mutex::new(store)),
            projects,
            live,
            drivers: HashMap::new(),
            agents: blocking(|| AgentCatalog::new(false)),
            harnesses: harness::Catalog {
                harnesses: blocking(harness::load),
                ..Default::default()
            },
            icons: blocking(disk_icons),
            settings: blocking(load_settings),
            daemon_flag: daemon,
            clients: HashMap::new(),
            _watcher: Self::watch(input.clone()),
            input,
            events,
            started: Instant::now(),
            idle_since: None,
            stop_all: None,
            deleting: HashMap::new(),
            probes: HashMap::new(),
            logins: HashMap::new(),
            watching: HashMap::new(),
            queued_files: HashMap::new(),
            next_queued: 0,
            attachments: HashMap::new(),
            steering: HashMap::new(),
            send_after_stop: HashSet::new(),
            last_traffic: HashMap::new(),
            permission_stopped: HashSet::new(),
            text_copy_pending: HashSet::new(),
            queued: HashMap::new(),
            touched: HashMap::new(),
            watched: Vec::new(),
            watch_due: None,
            status: Status::default(),
            exit: false,
            identity: ipc::engine_id().unwrap_or_else(|error| {
                log(format!("Cannot keep an engine identity: {error}"));
                String::new()
            }),
            epoch: crate::files::random_id(),
            seq: 0,
            recent: std::collections::VecDeque::new(),
        };
        engine.detect_harnesses(false);
        engine.status = engine.compute_status();
        engine
    }

    fn daemon(&self) -> bool {
        self.daemon_flag || self.settings.keep_running
    }

    fn subscribers(&self) -> usize {
        self.clients.values().filter(|c| c.subscribed).count()
    }

    fn compute_status(&self) -> Status {
        let mut conversations: Vec<_> = self
            .projects
            .iter()
            .flat_map(|project| {
                project.threads.iter().filter_map(|thread| {
                    let live = self.live.get(&thread.id)?;
                    live.processing.then(|| ActiveConversation {
                        project: project.config.name.clone(),
                        title: thread.title.clone(),
                        state: if !live.permission.is_empty() {
                            "waiting for permission"
                        } else if live
                            .progress
                            .as_deref()
                            .is_some_and(|p| p.starts_with("Retry"))
                        {
                            "retrying"
                        } else {
                            "processing"
                        }
                        .into(),
                    })
                })
            })
            .collect();
        conversations.sort_by(|a, b| (&a.project, &a.title).cmp(&(&b.project, &b.title)));
        Status {
            protocol: PROTOCOL,
            version: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            daemon: self.daemon(),
            uptime_secs: self.started.elapsed().as_secs(),
            clients: self.subscribers(),
            conversations,
            log: ipc::log_path(),
            engine_id: self.identity.clone(),
            headless: !cfg!(feature = "gui"),
        }
    }

    /// Tells clients when the status changed, ignoring the clock.
    fn refresh_status(&mut self) {
        let status = self.compute_status();
        let same = Status {
            uptime_secs: self.status.uptime_secs,
            ..status.clone()
        } == self.status;
        if !same {
            self.status = status.clone();
            self.broadcast_message(&EngineMessage::Delta(Delta::EngineStatus(status)));
        }
    }

    fn send_to(&self, client: u64, message: &EngineMessage) {
        if let Some(client) = self.clients.get(&client)
            && let Ok(mut line) = serde_json::to_string(message)
        {
            line.push('\n');
            let _ = client.sender.send(line);
        }
    }

    fn broadcast_message(&mut self, message: &EngineMessage) {
        let Ok(mut line) = serde_json::to_string(message) else {
            return;
        };
        line.push('\n');
        for client in self.clients.values().filter(|c| c.subscribed) {
            let _ = client.sender.send(line.clone());
        }
        if matches!(message, EngineMessage::Delta(_)) {
            self.seq += 1;
            self.recent.push_back((self.seq, line));
            if self.recent.len() > RECENT_DELTAS {
                self.recent.pop_front();
            }
        }
    }

    /// Sends a reconnecting client the deltas after `resume`, if they are all
    /// still kept. Returns whether it did.
    fn resume(&self, client: u64, resume: &crate::protocol::Resume) -> bool {
        let first = self.recent.front().map_or(self.seq + 1, |(seq, _)| *seq);
        if resume.epoch != self.epoch || resume.seq > self.seq || resume.seq + 1 < first {
            return false;
        }
        self.send_to(client, &EngineMessage::Resumed);
        if let Some(entry) = self.clients.get(&client) {
            for (_, line) in self.recent.iter().filter(|(seq, _)| *seq > resume.seq) {
                let _ = entry.sender.send(line.clone());
            }
        }
        true
    }

    /// Applies a change to the engine's own state, then sends it to every client.
    fn broadcast(&mut self, delta: Delta) {
        crate::protocol::apply(&mut self.projects, &mut self.live, &delta);
        self.broadcast_message(&EngineMessage::Delta(delta));
    }

    fn send_live(&mut self, id: &str) {
        let running = self.drivers.contains_key(id);
        let live = self.live.entry(id.to_owned()).or_default();
        live.running = running;
        live.turn = if !live.permission.is_empty()
            || live.auth_required
            || live.replacement
            || live.storage_failed
        {
            TurnState::NeedsAction
        } else if live.processing {
            TurnState::Running
        } else {
            TurnState::Idle
        };
        let live = live.clone();
        self.broadcast_message(&EngineMessage::Delta(Delta::Live {
            id: id.to_owned(),
            live,
        }));
    }

    fn reply(&self, client: u64, request: u64, result: Result<Value, String>) {
        self.send_to(
            client,
            &EngineMessage::Reply {
                id: request,
                result,
            },
        );
    }

    fn snapshot(&self) -> Snapshot {
        let mut live = self.live.clone();
        for (id, state) in &mut live {
            state.running = self.drivers.contains_key(id);
        }
        Snapshot {
            projects: self.projects.clone(),
            live,
            agents: self.agents.entries.clone(),
            agent_errors: self.agents.errors.clone(),
            harnesses: self.harnesses.clone(),
            icons: self.icons.clone(),
            settings: self.settings.clone(),
            errors: self
                .store
                .lock()
                .map(|store| store.errors.clone())
                .unwrap_or_default(),
            status: self.compute_status(),
            epoch: self.epoch.clone(),
            seq: self.seq,
        }
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::Connected {
                client,
                sender,
                writer,
            } => {
                self.clients.insert(
                    client,
                    Client {
                        sender,
                        writer,
                        subscribed: false,
                    },
                );
            }
            Input::Message { client, message } => self.message(client, message),
            Input::Disconnected { client } => {
                self.probes.retain(|(owner, _), _| *owner != client);
                self.logins.retain(|(owner, _), _| *owner != client);
                self.watching.remove(&client);
                if self.clients.remove(&client).is_some_and(|c| c.subscribed) {
                    log(format!("Client {client} disconnected"));
                    self.clients_changed();
                }
            }
            Input::Watch(paths) => {
                self.watched.extend(paths);
                self.watch_due = Some(Instant::now() + Duration::from_millis(300));
            }
            Input::ProjectsLoaded { store, started } => self.reconcile(store, started),
            Input::Harnesses(catalog) => {
                self.harnesses = catalog;
                self.broadcast(Delta::Harnesses(self.harnesses.clone()));
                self.refresh_icons();
            }
            Input::Output {
                client,
                request,
                line,
            } => self.send_to(client, &EngineMessage::Output { request, line }),
            Input::Done {
                client,
                request,
                result,
                detect,
            } => {
                self.logins.remove(&(client, request));
                self.reply(client, request, result);
                if detect {
                    self.detect_harnesses(false);
                }
            }
            Input::OpenUrl(url) => {
                self.broadcast_message(&EngineMessage::Delta(Delta::OpenUrl { url }));
            }
            Input::ProbeDone {
                client,
                request,
                probe,
                result,
            } => {
                let result = if self.probes.remove(&(client, probe)).is_some() {
                    result
                        .ok_or_else(|| "The probe stopped.".to_owned())
                        .and_then(|result| serde_json::to_value(result).map_err(|e| e.to_string()))
                } else {
                    Err("Probe cancelled.".into())
                };
                self.reply(client, request, result);
            }
        }
    }

    fn message(&mut self, client: u64, message: ClientMessage) {
        match message {
            ClientMessage::Hello {
                protocol,
                cli,
                resume,
            } => {
                self.send_to(
                    client,
                    &EngineMessage::Welcome {
                        status: self.compute_status(),
                    },
                );
                if protocol == PROTOCOL && !cli {
                    if let Some(entry) = self.clients.get_mut(&client) {
                        entry.subscribed = true;
                    }
                    log(format!("Client {client} connected"));
                    if !resume.is_some_and(|resume| self.resume(client, &resume)) {
                        self.send_to(client, &EngineMessage::Snapshot(Box::new(self.snapshot())));
                    }
                    self.clients_changed();
                }
            }
            ClientMessage::Request { id, command } => {
                let subscribed = self.clients.get(&client).is_some_and(|c| c.subscribed);
                if !subscribed
                    && !matches!(
                        command,
                        Command::Status | Command::StopAll | Command::Shutdown
                    )
                {
                    self.reply(
                        client,
                        id,
                        Err("This client uses a different protocol version.".into()),
                    );
                    return;
                }
                if let Some(result) = self.command(client, id, command) {
                    self.reply(client, id, result);
                }
            }
        }
    }

    /// Runs a command. `None` means the reply comes later.
    fn command(
        &mut self,
        client: u64,
        request: u64,
        command: Command,
    ) -> Option<Result<Value, String>> {
        let conversation = |engine: &Self, id: &str| {
            engine
                .locate(id)
                .map(|_| ())
                .ok_or_else(|| "This conversation no longer exists.".to_owned())
        };
        Some(match command {
            Command::Send {
                project_id,
                conversation_id,
                agent_id,
                prompt,
                attachments,
                now,
            } => self
                .send(
                    &project_id,
                    conversation_id,
                    agent_id.as_deref(),
                    &prompt,
                    attachments,
                    now,
                )
                .map(Value::String),
            Command::TakeQueued { id, queued } => {
                conversation(self, &id).and_then(|()| self.take_queued(&id, queued))
            }
            Command::SendQueuedNow { id, queued } => conversation(self, &id).and_then(|()| {
                let (text, files) = self.take_queued_parts(&id, queued)?;
                self.send_now(&id, text, files);
                self.send_live(&id);
                Ok(Value::Null)
            }),
            Command::Stop { id } => conversation(self, &id).map(|()| {
                self.stop(&id);
                Value::Null
            }),
            Command::ForceStop { id } => conversation(self, &id).map(|()| {
                if let Some(slot) = self.drivers.get(&id) {
                    let _ = slot.driver.send(acp::Command::ForceStop);
                }
                Value::Null
            }),
            Command::Retry { id } => conversation(self, &id).and_then(|()| self.retry(&id)),
            Command::Restart { id } => conversation(self, &id).map(|()| {
                if let Some(slot) = self.drivers.get(&id) {
                    let _ = slot.driver.send(acp::Command::ForceStop);
                    let live = self.live.entry(id.clone()).or_default();
                    live.shutting_down = true;
                    live.progress = Some("Restarting the agent…".into());
                    live.quiet_since = None;
                    self.queued.insert(
                        id.clone(),
                        "Continue the interrupted turn from the saved session. Preserve completed work; do not repeat completed tool actions.".into(),
                    );
                    self.send_live(&id);
                }
                Value::Null
            }),
            Command::RetryStorage { id } => {
                conversation(self, &id).and_then(|()| self.retry_storage(&id))
            }
            Command::ReplaceSession { id } => {
                conversation(self, &id).and_then(|()| self.replace_session(&id))
            }
            Command::Fork { id, message } => {
                conversation(self, &id).and_then(|()| self.fork(&id, message))
            }
            Command::SetOption {
                id,
                category,
                option,
                value,
            } => conversation(self, &id)
                .and_then(|()| self.set_option(&id, category, &option, value)),
            Command::AnswerPermission {
                id,
                request_id,
                option_id,
            } => conversation(self, &id)
                .and_then(|()| self.answer_permission(&id, request_id, &option_id)),
            Command::SetStatus { id, status } => {
                conversation(self, &id).and_then(|()| self.set_status(&id, &status))
            }
            Command::MarkRead { id, through } => {
                conversation(self, &id).map(|()| self.mark_read(&id, through))
            }
            Command::SaveProject {
                original,
                name,
                directory,
            } => self.save_project(original.as_deref(), &name, &directory),
            Command::DeleteProject { id } => {
                if !self.projects.iter().any(|p| p.config.id == id) {
                    return Some(Err(format!("Project {id} no longer exists.")));
                }
                self.deleting
                    .entry(id.clone())
                    .or_default()
                    .push((client, request));
                let ids = self.project_conversations(&id);
                for conversation in ids {
                    self.shutdown_conversation(&conversation);
                    self.send_live(&conversation);
                }
                self.finish_deletions();
                return None;
            }
            Command::CancelDeleteProject { id } => {
                for (client, request) in self.deleting.remove(&id).unwrap_or_default() {
                    self.reply(client, request, Err("Deletion cancelled.".into()));
                }
                Ok(Value::Null)
            }
            Command::ForceProject { id } => {
                for conversation in self.project_conversations(&id) {
                    if let Some(slot) = self.drivers.get(&conversation) {
                        let _ = slot.driver.send(acp::Command::ForceStop);
                    }
                }
                Ok(Value::Null)
            }
            Command::SaveAgent {
                original,
                definition,
                expected,
                overwrite,
            } => {
                let saved = blocking(|| {
                    self.agents.save(
                        original.as_deref(),
                        definition,
                        expected.as_ref(),
                        overwrite,
                    )
                });
                self.agents_changed();
                saved.map(Value::String)
            }
            Command::DeleteAgent { id } => {
                let deleted = blocking(|| self.agents.delete(&id));
                self.agents_changed();
                deleted.map(|()| Value::Null)
            }
            Command::RefreshHarnesses { fetch } => {
                self.detect_harnesses(fetch);
                Ok(Value::Null)
            }
            Command::Probe {
                probe,
                harness,
                command,
                arguments,
                model,
            } => return self.probe(client, request, probe, &harness, &command, arguments, model),
            Command::CancelProbe { probe } => {
                self.probes.remove(&(client, probe));
                Ok(Value::Null)
            }
            Command::PlanInstall { target } => install::plan(&target, &self.harnesses)
                .and_then(|steps| serde_json::to_value(steps).map_err(|e| e.to_string())),
            Command::Install { target } => {
                let steps = match install::plan(&target, &self.harnesses) {
                    Ok(steps) => steps,
                    Err(error) => return Some(Err(error)),
                };
                let input = self.input.clone();
                tokio::spawn(async move {
                    let output_input = input.clone();
                    let output = Arc::new(move |line: String| {
                        let _ = output_input.send(Input::Output {
                            client,
                            request,
                            line,
                        });
                    });
                    let result = install::run(steps, output).await.map(|()| Value::Null);
                    let _ = input.send(Input::Done {
                        client,
                        request,
                        result,
                        detect: true,
                    });
                });
                return None;
            }
            Command::Login {
                harness,
                command,
                arguments,
                method,
            } => return self.login(client, request, &harness, &command, arguments, method),
            Command::WatchTraffic { id } => {
                let recorded = id
                    .as_ref()
                    .and_then(|id| self.store.lock().ok().map(|store| store.traffic(id, 2000)))
                    .unwrap_or_default();
                match id {
                    Some(id) => self.watching.insert(client, id),
                    None => self.watching.remove(&client),
                };
                Ok(Value::Array(recorded))
            }
            Command::SetSettings { settings } => self.set_settings(settings),
            Command::Status => {
                serde_json::to_value(self.compute_status()).map_err(|e| e.to_string())
            }
            Command::ListDirectory { path } => blocking(|| list_directory(path))
                .and_then(|listing| serde_json::to_value(listing).map_err(|e| e.to_string())),
            Command::StopAll => {
                self.begin_stop_all(Some((client, request)), false);
                return None;
            }
            Command::Shutdown => {
                log(format!("Client {client} asked the engine to stop"));
                self.begin_stop_all(Some((client, request)), true);
                return None;
            }
        })
    }

    fn clients_changed(&mut self) {
        if self.subscribers() > 0 {
            return;
        }
        // Nobody can answer a permission now.
        let waiting: Vec<_> = self
            .live
            .iter()
            .filter(|(_, live)| !live.permission.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        for id in waiting {
            self.stop_for_permission(&id);
        }
        if !self.daemon() {
            let idle: Vec<_> = self
                .drivers
                .keys()
                .filter(|id| self.live.get(*id).is_none_or(|live| !live.processing))
                .cloned()
                .collect();
            for id in idle {
                self.shutdown_conversation(&id);
            }
        }
    }

    fn tick(&mut self) {
        let failed = self
            .store
            .lock()
            .map(|mut store| store.flush_due())
            .unwrap_or_default();
        for (id, error) in failed {
            self.storage_failure(&id, &error);
            self.send_live(&id);
        }
        // A running turn with no traffic for a while reads as quiet (scope R32, R34).
        let now = crate::recency::now_ms();
        let quiet: Vec<String> = self
            .live
            .iter()
            .filter(|(id, live)| {
                live.processing
                    && live.quiet_since.is_none()
                    && self
                        .last_traffic
                        .get(*id)
                        .is_some_and(|at| now.saturating_sub(*at) >= QUIET_AFTER_MS)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in quiet {
            let since = self.last_traffic.get(&id).copied();
            self.live.entry(id.clone()).or_default().quiet_since = since;
            self.send_live(&id);
        }
        if self.watch_due.is_some_and(|due| Instant::now() >= due) {
            self.watch_due = None;
            self.reload_watched();
        }
        if let Some(stop) = &mut self.stop_all
            && Instant::now() >= stop.deadline
        {
            if stop.forced {
                // Agents that ignore even a kill can't hold the engine forever.
                log("Some agents did not stop after being killed");
                self.drivers.clear();
                self.finish_stop_all();
            } else {
                stop.forced = true;
                stop.deadline = Instant::now() + STOP_GRACE;
                log("Killing agents that did not stop within 5 seconds");
                for slot in self.drivers.values() {
                    let _ = slot.driver.send(acp::Command::ForceStop);
                }
            }
        }
        let idle = self.subscribers() == 0
            && !self.daemon()
            && self.stop_all.is_none()
            && !self.live.values().any(|live| live.processing);
        if !idle {
            self.idle_since = None;
        } else if self.idle_since.get_or_insert_with(Instant::now).elapsed() >= IDLE_EXIT {
            log("No clients or active conversations for 60 seconds; exiting");
            self.begin_stop_all(None, true);
        }
    }

    fn begin_stop_all(&mut self, waiter: Option<(u64, u64)>, exit: bool) {
        if let Some(stop) = &mut self.stop_all {
            stop.waiters.extend(waiter);
            stop.exit |= exit;
            return;
        }
        let ids: Vec<_> = self.drivers.keys().cloned().collect();
        let stopped = ids
            .iter()
            .filter_map(|id| {
                let project_id = self.project_of(id)?;
                let project = self.projects.iter().find(|p| p.config.id == project_id)?;
                let thread = project.threads.iter().find(|t| &t.id == id)?;
                let state = if self.live.get(id).is_some_and(|l| l.processing) {
                    "processing"
                } else {
                    "idle"
                };
                Some(format!(
                    "{} / {} ({state})",
                    project.config.name, thread.title
                ))
            })
            .collect();
        self.stop_all = Some(StopAll {
            deadline: Instant::now() + STOP_GRACE,
            forced: false,
            waiters: waiter.into_iter().collect(),
            stopped,
            exit,
        });
        for id in ids {
            self.shutdown_conversation(&id);
            self.send_live(&id);
        }
        self.finish_stop_all();
    }

    fn finish_stop_all(&mut self) {
        if !self.drivers.is_empty() {
            return;
        }
        let Some(stop) = self.stop_all.take() else {
            return;
        };
        for (client, request) in stop.waiters {
            self.reply(client, request, Ok(json!({"stopped": stop.stopped})));
        }
        if stop.exit {
            self.broadcast_message(&EngineMessage::Bye);
            self.exit = true;
        }
    }

    fn locate(&self, id: &str) -> Option<&Thread> {
        self.projects
            .iter()
            .flat_map(|p| &p.threads)
            .find(|t| t.id == id)
    }

    fn project_of(&self, id: &str) -> Option<String> {
        self.projects
            .iter()
            .find(|p| p.threads.iter().any(|t| t.id == id))
            .map(|p| p.config.id.clone())
    }

    fn project_conversations(&self, project_id: &str) -> Vec<String> {
        self.projects
            .iter()
            .filter(|p| p.config.id == project_id)
            .flat_map(|p| p.threads.iter().map(|t| t.id.clone()))
            .collect()
    }

    fn status_of(&self, id: &str) -> String {
        self.locate(id)
            .map(|t| t.status.clone())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Storage

    fn conversation_settings(&self, id: &str) -> Option<storage::ConversationSettings> {
        let store = self.store.lock().ok()?;
        store.conversation(id).map(|c| c.settings.clone())
    }

    fn storage_failure(&mut self, id: &str, error: &str) {
        let live = self.live.entry(id.to_owned()).or_default();
        live.storage_failed = true;
        live.recovering_storage = false;
        live.error = Some(format!(
            "History could not be saved: {error}. Restore storage access, then Retry storage."
        ));
        live.permission.clear();
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::Cancel);
        }
        if !matches!(self.status_of(id).as_str(), "completed" | "archived" | "") {
            self.broadcast(Delta::Status {
                id: id.to_owned(),
                status: "blocked".into(),
            });
        }
    }

    /// Queues a streamed event; it reaches disk within 250 ms (scope R41).
    fn queue_visible(&mut self, id: &str, kind: &str, data: Value) -> bool {
        self.touched.insert(id.to_owned(), Instant::now());
        let result = self
            .store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut store| store.queue_event(id, storage::TranscriptEvent::new(kind, data)));
        if let Err(error) = result {
            if !self.live.get(id).is_some_and(|live| live.storage_failed) {
                self.storage_failure(id, &error);
            }
            false
        } else {
            true
        }
    }

    /// Saves an event, and everything queued before it, before returning.
    fn record_visible(&mut self, id: &str, kind: &str, data: Value) -> bool {
        self.touched.insert(id.to_owned(), Instant::now());
        let store = self.store.clone();
        let result = blocking(|| {
            store
                .lock()
                .map_err(|e| e.to_string())
                .and_then(|mut store| {
                    store.record_event(id, &storage::TranscriptEvent::new(kind, data))
                })
        });
        if let Err(error) = result {
            self.storage_failure(id, &error);
            false
        } else {
            true
        }
    }

    fn save_conversation_settings(
        &mut self,
        id: &str,
        settings: storage::ConversationSettings,
    ) -> bool {
        let Some(project_id) = self.project_of(id) else {
            return false;
        };
        self.touched.insert(id.to_owned(), Instant::now());
        let store = self.store.clone();
        let result = blocking(|| {
            store
                .lock()
                .map_err(|e| e.to_string())
                .and_then(|mut store| store.update_conversation(&project_id, id, settings))
        });
        if let Err(error) = result {
            self.storage_failure(id, &error);
            false
        } else {
            true
        }
    }

    fn set_runtime_status(&mut self, id: &str, status: &str) -> bool {
        let Some(mut settings) = self.conversation_settings(id) else {
            return false;
        };
        status.clone_into(&mut settings.status);
        if !self.save_conversation_settings(id, settings) {
            return false;
        }
        self.broadcast(Delta::Status {
            id: id.to_owned(),
            status: status.to_owned(),
        });
        true
    }

    fn thread_from_store(&self, id: &str) -> Option<(String, Thread)> {
        let store = self.store.lock().ok()?;
        store.projects.iter().find_map(|p| {
            p.conversations
                .iter()
                .find(|c| c.id == id)
                .map(|c| (p.id.clone(), c.to_thread()))
        })
    }

    // -----------------------------------------------------------------------
    // Conversations

    fn ensure_driver(&mut self, id: &str) -> bool {
        if self.drivers.contains_key(id) {
            return true;
        }
        let Some(settings) = self.conversation_settings(id) else {
            return false;
        };
        let driver = acp::Driver::spawn(
            id.to_owned(),
            settings.execution,
            settings.session_id,
            self.events.clone(),
        );
        self.drivers.insert(id.to_owned(), Slot { driver, turn: 0 });
        self.live.entry(id.to_owned()).or_default().running = true;
        true
    }

    /// Why this conversation's agent cannot start: a harness that is no
    /// longer installed.
    fn cannot_start(&self, id: &str) -> Option<String> {
        let execution = self.live.get(id)?.execution.as_ref()?;
        let command = Path::new(&execution.command);
        (command.is_absolute() && !command.is_file()).then(|| {
            format!(
                "The {} harness is not installed.",
                self.harnesses
                    .label(&execution.harness, &execution.identity)
            )
        })
    }

    /// Why the agent can't take these files: images it can't receive, or
    /// files over the size limit (scope R23).
    fn refuse_attachments(&self, id: Option<&str>, attachments: &[Attachment]) -> Option<String> {
        for file in attachments {
            if file.size > crate::conversation::ATTACHMENT_LIMIT {
                return Some(format!(
                    "{} is {} MB. Files over 20 MB can't be attached.",
                    file.name,
                    file.size.div_ceil(1024 * 1024)
                ));
            }
        }
        let live = id.and_then(|id| self.live.get(id))?;
        if attachments.iter().any(Attachment::image) && live.features.known && !live.features.images
        {
            let name = live
                .execution
                .as_ref()
                .map_or("This agent", |execution| execution.name.as_str());
            return Some(format!("{name} can't receive images."));
        }
        None
    }

    fn send(
        &mut self,
        project_id: &str,
        conversation_id: Option<String>,
        agent_id: Option<&str>,
        prompt: &str,
        attachments: Vec<Attachment>,
        now: bool,
    ) -> Result<String, String> {
        if self.stop_all.is_some() {
            return Err("The conversation engine is stopping its agents.".into());
        }
        if self.deleting.contains_key(project_id) {
            return Err("This project is being deleted.".into());
        }
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() && attachments.is_empty() {
            return Err("Enter a message to send.".into());
        }
        // Files picked on this machine are read here, so their size is known here.
        let attachments: Vec<Attachment> = attachments
            .into_iter()
            .map(|mut file| {
                if let Some(path) = &file.path
                    && file.data.is_empty()
                    && let Ok(metadata) = blocking(|| std::fs::metadata(path))
                {
                    file.size = metadata.len();
                }
                file
            })
            .collect();
        if let Some(error) = self.refuse_attachments(conversation_id.as_deref(), &attachments) {
            return Err(error);
        }
        let id = if let Some(id) = conversation_id {
            let Some(thread) = self.locate(&id) else {
                return Err("This conversation no longer exists.".into());
            };
            if thread.status == "archived" {
                return Err("This conversation is archived.".into());
            }
            let live = self.live.entry(id.clone()).or_default();
            if live.storage_failed || live.recovering_storage {
                return Err("Retry storage before sending.".into());
            }
            if live.replacement {
                return Err("Start a replacement session before sending.".into());
            }
            // The composer is never blocked by a running turn (scope R30).
            if live.processing {
                if now {
                    self.send_now(&id, prompt, attachments);
                } else {
                    self.queue(&id, prompt, attachments);
                }
                self.send_live(&id);
                return Ok(id);
            }
            if live.shutting_down {
                return Err("Wait for the agent to stop before sending.".into());
            }
            id
        } else {
            let agent = agent_id
                .and_then(|id| self.agents.entries.iter().find(|entry| entry.id == id))
                .map(|entry| entry.definition.clone())
                .ok_or("Create or select an agent before sending.")?;
            let launch = self
                .harnesses
                .launch(&agent.harness, &agent.command, &agent.arguments)?;
            let mut servers = self.settings.mcp_servers.clone();
            servers.extend(agent.mcp_servers.iter().cloned());
            let store = self.store.clone();
            let title = crate::short(&prompt, 100);
            let (id, thread, live) = blocking(|| {
                let mut store = store.lock().map_err(|e| e.to_string())?;
                let id = store.create_conversation(project_id, &agent, launch, servers, &title)?;
                let saved = store
                    .conversation(&id)
                    .ok_or_else(|| "Created conversation is missing.".to_owned())?;
                Ok::<_, String>((id, saved.to_thread(), live_for(saved)))
            })?;
            self.live.insert(id.clone(), live);
            self.broadcast(Delta::Thread {
                project_id: project_id.to_owned(),
                thread,
            });
            id
        };
        if let Some(error) = self.cannot_start(&id) {
            self.send_live(&id);
            return Err(error);
        }
        self.deliver(&id, prompt, attachments);
        self.send_live(&id);
        Ok(id)
    }

    /// Records the user's message and starts its turn.
    fn deliver(&mut self, id: &str, prompt: String, attachments: Vec<Attachment>) {
        prompt.clone_into(&mut self.live.entry(id.to_owned()).or_default().last_prompt);
        let message = Message {
            role: "user".into(),
            text: prompt.clone(),
            read: true,
            created_at: crate::recency::now().to_string(),
            attachments: attachments.clone(),
            ..Default::default()
        };
        let saved = self.record_visible(
            id,
            "message",
            serde_json::to_value(&message).unwrap_or_default(),
        );
        self.broadcast(Delta::Message {
            id: id.to_owned(),
            message,
        });
        self.attachments.insert(id.to_owned(), attachments.clone());
        if saved {
            self.start_prompt(id, prompt, attachments, false);
        }
    }

    /// Adds a message to the queue of the running turn (scope R31).
    fn queue(&mut self, id: &str, text: String, files: Vec<Attachment>) {
        self.next_queued += 1;
        let queued = Queued {
            id: self.next_queued,
            text,
            files: files.iter().map(|file| file.name.clone()).collect(),
        };
        self.queued_files.insert(queued.id, files);
        self.live
            .entry(id.to_owned())
            .or_default()
            .queued
            .push(queued);
    }

    /// Delivers a message during a turn: through steering when the agent
    /// offers it, else by stopping the turn and sending it next (scope R48).
    fn send_now(&mut self, id: &str, text: String, files: Vec<Attachment>) {
        let steering = self
            .live
            .get(id)
            .is_some_and(|live| live.features.steering && live.processing);
        if steering
            && !self.steering.contains_key(id)
            && let Some(slot) = self.drivers.get(id)
        {
            let mut prompt = vec![acp::Part::Text(text.clone())];
            prompt.extend(files.iter().cloned().map(acp::Part::File));
            if slot.driver.send(acp::Command::Steer { prompt }).is_ok() {
                self.steering.insert(id.to_owned(), (text, files));
                return;
            }
        }
        // The message goes first, then the cancel settles and the queue is sent.
        self.next_queued += 1;
        let queued = Queued {
            id: self.next_queued,
            text,
            files: files.iter().map(|file| file.name.clone()).collect(),
        };
        self.queued_files.insert(queued.id, files);
        self.live
            .entry(id.to_owned())
            .or_default()
            .queued
            .insert(0, queued);
        self.send_after_stop.insert(id.to_owned());
        self.stop(id);
    }

    fn take_queued_parts(
        &mut self,
        id: &str,
        queued: u64,
    ) -> Result<(String, Vec<Attachment>), String> {
        let live = self.live.entry(id.to_owned()).or_default();
        let index = live
            .queued
            .iter()
            .position(|item| item.id == queued)
            .ok_or("That message was already sent.")?;
        let item = live.queued.remove(index);
        let files = self.queued_files.remove(&queued).unwrap_or_default();
        Ok((item.text, files))
    }

    fn take_queued(&mut self, id: &str, queued: u64) -> Result<Value, String> {
        let (text, _) = self.take_queued_parts(id, queued)?;
        self.send_live(id);
        Ok(Value::String(text))
    }

    /// Sends every queued message as one prompt, separated by blank lines.
    fn send_queue(&mut self, id: &str) {
        let queued = std::mem::take(&mut self.live.entry(id.to_owned()).or_default().queued);
        if queued.is_empty() {
            return;
        }
        let text = queued
            .iter()
            .map(|item| item.text.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let files = queued
            .iter()
            .flat_map(|item| self.queued_files.remove(&item.id).unwrap_or_default())
            .collect();
        self.deliver(id, text, files);
    }

    fn start_prompt(
        &mut self,
        id: &str,
        prompt: String,
        attachments: Vec<Attachment>,
        retry: bool,
    ) {
        if self.cannot_start(id).is_some() || !self.set_runtime_status(id, "processing") {
            return;
        }
        if !self.ensure_driver(id) {
            let error = "Could not load this conversation's saved agent settings.".to_owned();
            self.record_visible(id, "error", json!({"message":error}));
            self.live.entry(id.to_owned()).or_default().error = Some(error);
            self.set_runtime_status(id, "blocked");
            return;
        }
        if retry
            && !self.record_visible(
                id,
                "lifecycle",
                json!({"event":"turn_started","retry":true}),
            )
        {
            return;
        }
        let live = self.live.entry(id.to_owned()).or_default();
        if live.storage_failed {
            return;
        }
        live.processing = true;
        live.error = None;
        live.stderr.clear();
        live.auth_required = false;
        live.progress = None;
        live.assistant = None;
        live.quiet_since = None;
        live.permission.clear();
        if !retry {
            live.worked = false;
            live.last_prompt.clone_from(&prompt);
        }
        self.last_traffic
            .insert(id.to_owned(), crate::recency::now_ms());
        if let Some((session, context)) = self.fork_history(id)
            && let Some(slot) = self.drivers.get(id)
        {
            let _ = slot.driver.send(acp::Command::Fork { session, context });
        }
        let retries = u32::try_from(self.settings.retry_limit).unwrap_or(u32::MAX);
        let mut parts = Vec::new();
        if !prompt.is_empty() {
            parts.push(acp::Part::Text(prompt));
        }
        parts.extend(attachments.into_iter().map(acp::Part::File));
        // The worker numbers turns; a prompt it rejects keeps the old number.
        let Some(slot) = self.drivers.get_mut(id) else {
            return;
        };
        if let Err(error) = slot.driver.send(acp::Command::Prompt {
            prompt: parts,
            retries,
        }) {
            self.drivers.remove(id);
            let live = self.live.entry(id.to_owned()).or_default();
            live.processing = false;
            live.error = Some(error.clone());
            self.record_visible(id, "error", json!({"message":error}));
            self.set_runtime_status(id, "blocked");
        }
    }

    /// Saves something the agent reported about its session to the conversation.
    fn save_session_state(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut storage::ConversationSettings),
    ) {
        if let Some(mut settings) = self.conversation_settings(id) {
            let before = settings.clone();
            change(&mut settings);
            if settings != before {
                self.save_conversation_settings(id, settings);
            }
        }
    }

    fn driver_event(&mut self, event: acp::Event) {
        let id = event.conversation_id.clone();
        if self.locate(&id).is_none() {
            return;
        }
        // Traffic is the agent's, whatever turn it belongs to.
        if let acp::EventKind::Traffic(entry) = event.kind {
            self.traffic(&id, &entry);
            return;
        }
        let Some(slot) = self.drivers.get_mut(&id) else {
            return;
        };
        if event.turn < slot.turn {
            return;
        }
        slot.turn = event.turn;
        let was_processing = self.live.get(&id).is_some_and(|l| l.processing);
        let closing = self.live.get(&id).is_some_and(|l| l.shutting_down)
            || self.stop_all.is_some()
            || matches!(self.status_of(&id).as_str(), "completed" | "archived");
        match event.kind {
            acp::EventKind::Traffic(_) | acp::EventKind::Probed => return,
            acp::EventKind::Session {
                session_id,
                replaced,
            } => {
                let saved = self.conversation_settings(&id).is_some_and(|mut settings| {
                    if replaced && let Some(old) = settings.session_id.take() {
                        settings.previous_session_ids.push(old);
                    }
                    settings.session_id = Some(session_id.clone());
                    self.save_conversation_settings(&id, settings)
                });
                if saved {
                    self.record_visible(
                        &id,
                        "lifecycle",
                        json!({"event":"session_ready","session_id":session_id,"replacement":replaced}),
                    );
                }
            }
            acp::EventKind::Agent { features, .. } => {
                self.live.entry(id.clone()).or_default().features = features.clone();
                self.save_session_state(&id, |settings| settings.features = features);
            }
            acp::EventKind::Options(options) => {
                let live = self.live.entry(id.clone()).or_default();
                if live.options != options {
                    live.options.clone_from(&options);
                    self.save_session_state(&id, |settings| settings.options = options);
                }
            }
            acp::EventKind::Commands(commands) => {
                self.live
                    .entry(id.clone())
                    .or_default()
                    .commands
                    .clone_from(&commands);
                self.save_session_state(&id, |settings| settings.commands = commands);
            }
            acp::EventKind::Todo(todo) => {
                self.live
                    .entry(id.clone())
                    .or_default()
                    .todo
                    .clone_from(&todo);
                self.save_session_state(&id, |settings| settings.todo = todo);
            }
            acp::EventKind::Title(title) => {
                self.save_session_state(&id, |settings| title.clone_into(&mut settings.title));
                self.broadcast(Delta::Title {
                    id: id.clone(),
                    title,
                });
                return;
            }
            acp::EventKind::Text { message, text } => {
                self.queue_visible(
                    &id,
                    "assistant_chunk",
                    json!({"text":text,"message":message}),
                );
                self.broadcast(Delta::Text {
                    id,
                    text,
                    message,
                    at: crate::recency::now_ms(),
                });
                return;
            }
            acp::EventKind::Thought { message, text } => {
                self.queue_visible(&id, "thought_chunk", json!({"text":text,"message":message}));
                self.broadcast(Delta::Thought {
                    id,
                    text,
                    message,
                    at: crate::recency::now_ms(),
                });
                return;
            }
            acp::EventKind::Tool {
                id: tool_id,
                title,
                status,
                detail,
                kind,
                paths,
            } => {
                let report = crate::data::ToolReport {
                    id: tool_id,
                    title,
                    status,
                    detail,
                    kind,
                    paths,
                };
                self.queue_visible(
                    &id,
                    "tool",
                    serde_json::to_value(&report).unwrap_or_default(),
                );
                self.broadcast(Delta::Tool {
                    id,
                    report,
                    at: crate::recency::now_ms(),
                });
                return;
            }
            acp::EventKind::Usage { used, size } => {
                self.queue_visible(&id, "usage", json!({"used":used,"size":size}));
                self.broadcast(Delta::Usage { id, used, size });
                return;
            }
            acp::EventKind::Permission {
                request_id,
                title,
                options,
            } => {
                if closing {
                    return;
                }
                if self.record_visible(
                    &id,
                    "permission_request",
                    json!({"request_id":request_id,"title":title,"options":options}),
                ) {
                    self.live
                        .entry(id.clone())
                        .or_default()
                        .permission
                        .push(PendingPermission {
                            request_id,
                            title,
                            options,
                        });
                    self.set_runtime_status(&id, "blocked");
                    if self.subscribers() == 0 {
                        self.stop_for_permission(&id);
                    }
                }
            }
            acp::EventKind::Steered { delivered } => {
                if let Some((text, files)) = self.steering.remove(&id) {
                    if delivered {
                        let message = Message {
                            role: "user".into(),
                            text,
                            read: true,
                            created_at: crate::recency::now().to_string(),
                            attachments: files,
                            ..Default::default()
                        };
                        self.record_visible(
                            &id,
                            "message",
                            serde_json::to_value(&message).unwrap_or_default(),
                        );
                        self.live.entry(id.clone()).or_default().assistant = None;
                        self.broadcast(Delta::Message {
                            id: id.clone(),
                            message,
                        });
                    } else {
                        // The agent didn't take it: it goes out when the turn ends.
                        self.next_queued += 1;
                        let queued = Queued {
                            id: self.next_queued,
                            text,
                            files: files.iter().map(|file| file.name.clone()).collect(),
                        };
                        self.queued_files.insert(queued.id, files);
                        self.live
                            .entry(id.clone())
                            .or_default()
                            .queued
                            .insert(0, queued);
                        if !self.live.get(&id).is_some_and(|live| live.processing) {
                            self.send_queue(&id);
                        }
                    }
                }
            }
            acp::EventKind::SkippedServers(names) => {
                let agent = self
                    .live
                    .get(&id)
                    .and_then(|live| live.execution.as_ref())
                    .map_or_else(|| "This agent".to_owned(), |e| e.name.clone());
                for name in names {
                    let message = format!("Skipped {name}: {agent} doesn't support HTTP servers.");
                    self.record_visible(&id, "note", json!({"message":message}));
                }
                if let Some((project_id, thread)) = self.thread_from_store(&id) {
                    self.broadcast(Delta::Thread { project_id, thread });
                }
            }
            acp::EventKind::OpenUrl(url) => {
                self.broadcast_message(&EngineMessage::Delta(Delta::OpenUrl { url }));
                return;
            }
            acp::EventKind::Auth(_) => {}
            acp::EventKind::Crashed(stderr) => {
                self.live.entry(id.clone()).or_default().stderr = stderr;
            }
            acp::EventKind::Retrying {
                attempt,
                limit,
                error,
            } => {
                if closing {
                    return;
                }
                if self.record_visible(
                    &id,
                    "error",
                    json!({"message":error,"attempt":attempt,"limit":limit}),
                ) {
                    self.live.entry(id.clone()).or_default().progress =
                        Some(format!("Retry {attempt} of {limit}: {error}"));
                }
            }
            acp::EventKind::Error { message, kind } => {
                let saved = self.record_visible(
                    &id,
                    "error",
                    json!({"message":message,"kind":format!("{kind:?}")}),
                );
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.progress = None;
                live.permission.clear();
                live.auth_required = kind == acp::FailureKind::Authentication;
                if saved {
                    live.error = Some(message);
                    if !closing {
                        self.set_runtime_status(&id, "blocked");
                    }
                }
                if self.text_copy_pending.remove(&id) {
                    self.drop_undelivered_fork_session(&id);
                }
            }
            acp::EventKind::Finished { stop_reason } => {
                self.text_copy_pending.remove(&id);
                let reason = match stop_reason {
                    StopReason::EndTurn => "end_turn",
                    StopReason::MaxTokens => "max_tokens",
                    StopReason::MaxTurnRequests => "max_turn_requests",
                    StopReason::Refusal => "refusal",
                    StopReason::Cancelled => "cancelled",
                    StopReason::Signal => "signal",
                };
                self.record_visible(
                    &id,
                    "lifecycle",
                    json!({"event":"turn_finished","reason":reason}),
                );
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.progress = None;
                live.quiet_since = None;
                live.permission.clear();
                if !closing && !live.storage_failed {
                    self.set_runtime_status(&id, "idle");
                    self.send_queue(&id);
                }
            }
            acp::EventKind::Stopped => {
                let saved =
                    self.record_visible(&id, "lifecycle", json!({"event":"turn_cancelled"}));
                let for_permission = self.permission_stopped.remove(&id);
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.permission.clear();
                live.progress = None;
                live.quiet_since = None;
                if for_permission {
                    live.worked = true;
                    live.error = Some(PERMISSION_STOPPED.into());
                    if !closing && !live.storage_failed {
                        self.set_runtime_status(&id, "blocked");
                    }
                } else if live.recovering_storage {
                    live.recovering_storage = false;
                    if saved {
                        live.error = (!live.last_prompt.is_empty()).then(|| {
                            "History is saved. Retry to continue the interrupted turn.".into()
                        });
                        if !closing && !live.storage_failed {
                            self.set_runtime_status(&id, "blocked");
                        }
                    }
                } else if saved && !closing && !live.storage_failed {
                    self.set_runtime_status(&id, "idle");
                    if self.send_after_stop.remove(&id) {
                        self.send_queue(&id);
                    }
                }
            }
            acp::EventKind::ShutdownStuck => {
                let live = self.live.entry(id.clone()).or_default();
                live.shutdown_stuck = true;
                live.error =
                    Some("Agent shutdown is stuck. Force Stop terminates this process.".into());
            }
            acp::EventKind::ShutdownComplete => {
                let saved =
                    self.record_visible(&id, "lifecycle", json!({"event":"process_stopped"}));
                self.drivers.remove(&id);
                self.steering.remove(&id);
                self.last_traffic.remove(&id);
                let finished = matches!(self.status_of(&id).as_str(), "completed" | "archived");
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.shutting_down = false;
                live.quiet_since = None;
                if live.shutdown_stuck && saved && !live.storage_failed {
                    live.error = None;
                }
                live.shutdown_stuck = false;
                live.permission.clear();
                if live.recovering_storage {
                    live.recovering_storage = false;
                    if saved && !live.storage_failed && !finished {
                        live.error =
                            Some("Agent stopped. Retry to continue from the saved session.".into());
                    }
                }
                if let Some(prompt) = self.queued.remove(&id)
                    && self.stop_all.is_none()
                {
                    let files = self.attachments.get(&id).cloned().unwrap_or_default();
                    self.start_prompt(&id, prompt, files, true);
                } else if self.send_after_stop.remove(&id) && self.stop_all.is_none() {
                    self.send_queue(&id);
                }
                self.send_live(&id);
                self.finish_deletions();
                self.finish_stop_all();
                return;
            }
            acp::EventKind::TextCopy => {
                self.text_copy_pending.insert(id.clone());
                if self.record_visible(&id, "fork_text_copy", json!({}))
                    && let Some((project_id, thread)) = self.thread_from_store(&id)
                {
                    self.broadcast(Delta::Thread { project_id, thread });
                }
            }
            acp::EventKind::ReplacementRequired(error) => {
                if closing {
                    return;
                }
                if self.record_visible(&id, "error", json!({"message":error,"restoration":false})) {
                    let live = self.live.entry(id.clone()).or_default();
                    live.replacement = true;
                    live.processing = false;
                    live.error = Some(format!(
                        "{error} Start a new session using saved messages only if you want to continue."
                    ));
                    self.set_runtime_status(&id, "blocked");
                }
            }
        }
        self.send_live(&id);
        // With no client connected, a finished turn's agent closes.
        let processing = self.live.get(&id).is_some_and(|l| l.processing);
        if was_processing && !processing && self.subscribers() == 0 && !self.daemon() {
            self.shutdown_conversation(&id);
            self.send_live(&id);
        }
    }

    /// Keeps a line of ACP traffic and shows it to clients watching it.
    fn traffic(&mut self, id: &str, entry: &crate::conversation::TrafficEntry) {
        self.queue_visible(
            id,
            "traffic",
            serde_json::to_value(entry).unwrap_or_default(),
        );
        if entry.direction != crate::conversation::Direction::ToAgent {
            self.last_traffic.insert(id.to_owned(), entry.at);
            if self
                .live
                .get(id)
                .is_some_and(|live| live.quiet_since.is_some())
            {
                self.live.entry(id.to_owned()).or_default().quiet_since = None;
                self.send_live(id);
            }
        }
        let watchers: Vec<u64> = self
            .watching
            .iter()
            .filter(|(_, watched)| *watched == id)
            .map(|(client, _)| *client)
            .collect();
        for client in watchers {
            self.send_to(
                client,
                &EngineMessage::Traffic {
                    id: id.to_owned(),
                    entry: entry.clone(),
                },
            );
        }
    }

    fn stop_for_permission(&mut self, id: &str) {
        let Some(live) = self.live.get_mut(id) else {
            return;
        };
        if live.permission.is_empty() {
            return;
        }
        live.permission.clear();
        live.progress = Some("Stopping…".into());
        log(format!(
            "Stopping conversation {id}: it needs a permission and no client is connected"
        ));
        self.permission_stopped.insert(id.to_owned());
        self.record_visible(
            id,
            "error",
            json!({"message":PERMISSION_STOPPED,"permission":true}),
        );
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::Cancel);
        }
        self.send_live(id);
    }

    fn stop(&mut self, id: &str) {
        let Some(live) = self.live.get_mut(id) else {
            return;
        };
        if !live.processing {
            return;
        }
        live.permission.clear();
        live.progress = Some("Stopping…".into());
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::Cancel);
        }
        self.send_live(id);
    }

    /// A text-copy fork whose first turn failed: retire its session and agent,
    /// so Retry starts a fresh session and sends the copied history again.
    fn drop_undelivered_fork_session(&mut self, id: &str) {
        let Some(mut settings) = self.conversation_settings(id) else {
            return;
        };
        if let Some(old) = settings.session_id.take() {
            settings.previous_session_ids.push(old);
        }
        if self.save_conversation_settings(id, settings) {
            self.shutdown_conversation(id);
        }
    }

    fn shutdown_conversation(&mut self, id: &str) {
        let live = self.live.entry(id.to_owned()).or_default();
        live.permission.clear();
        live.progress = None;
        if let Some(slot) = self.drivers.get(id)
            && !live.shutting_down
        {
            live.shutting_down = true;
            if let Err(error) = slot.driver.send(acp::Command::Shutdown) {
                live.error = Some(error);
            }
        }
    }

    fn set_status(&mut self, id: &str, status: &str) -> Result<Value, String> {
        let current = self.status_of(id);
        if current == status {
            return Err(match status {
                "completed" => "Already completed",
                "archived" => "Already archived",
                _ => "Already open",
            }
            .into());
        }
        match status {
            "idle" if matches!(current.as_str(), "completed" | "archived") => {
                self.set_runtime_status(id, "idle");
            }
            "completed" | "archived" => {
                self.set_runtime_status(id, status);
                self.shutdown_conversation(id);
            }
            _ => return Err(format!("Cannot change this conversation to {status}.")),
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    /// Why a turn can't start in this conversation now, beyond its own state.
    fn cannot_turn(&self, id: &str) -> Option<String> {
        if self.stop_all.is_some() {
            return Some("The conversation engine is stopping its agents.".into());
        }
        if self
            .project_of(id)
            .is_some_and(|project| self.deleting.contains_key(&project))
        {
            return Some("This project is being deleted.".into());
        }
        match self.status_of(id).as_str() {
            "completed" => Some("Already completed".into()),
            "archived" => Some("This conversation is archived.".into()),
            _ => None,
        }
    }

    fn retry(&mut self, id: &str) -> Result<Value, String> {
        if let Some(error) = self.cannot_turn(id) {
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        if live.processing {
            return Err(ALREADY_PROCESSING.into());
        }
        if live.storage_failed || live.recovering_storage || live.replacement {
            return Err("This conversation can't retry right now.".into());
        }
        let worked = live.worked;
        let prompt = if worked {
            "Continue the interrupted turn from the saved session. Preserve completed work; do not repeat completed tool actions.".to_owned()
        } else {
            live.last_prompt.clone()
        };
        let files = if worked {
            Vec::new()
        } else {
            self.attachments.get(id).cloned().unwrap_or_default()
        };
        if prompt.is_empty() && files.is_empty() {
            return Err("There is nothing to retry.".into());
        }
        if let Some(error) = self.cannot_start(id) {
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        if live.shutting_down && !live.shutdown_stuck {
            // An agent closed in the background; the turn starts once it has exited.
            live.processing = true;
            live.progress = Some("Restarting the agent…".into());
            self.queued.insert(id.to_owned(), prompt);
        } else if live.shutting_down {
            return Err("Agent shutdown is stuck. Use Force Stop first.".into());
        } else {
            self.start_prompt(id, prompt, files, true);
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    fn retry_storage(&mut self, id: &str) -> Result<Value, String> {
        if self.live.get(id).is_none_or(|live| !live.storage_failed) {
            return Err("History is already saved.".into());
        }
        let store = self.store.clone();
        let result = blocking(|| {
            let mut store = store.lock().map_err(|e| e.to_string())?;
            store.retry_unsaved(id)?;
            store
                .conversation(id)
                .map(|conversation| {
                    let worked = conversation
                        .events
                        .iter()
                        .chain(&conversation.unsaved_events)
                        .rev()
                        .take_while(|event| event.kind != "message" || event.data["role"] != "user")
                        .any(|event| {
                            matches!(
                                event.kind.as_str(),
                                "assistant_chunk" | "tool" | "message_update"
                            ) || (event.kind == "message" && event.data["role"] == "assistant")
                        });
                    (
                        conversation.settings.clone(),
                        conversation.to_thread(),
                        worked,
                    )
                })
                .ok_or_else(|| "Conversation is missing from storage.".to_owned())
        });
        let (settings, thread, recovered_work) = match result {
            Ok(result) => result,
            Err(error) => {
                self.live.entry(id.to_owned()).or_default().error = Some(error.clone());
                self.send_live(id);
                return Err(error);
            }
        };
        if let Some(project_id) = self.project_of(id) {
            self.broadcast(Delta::Thread { project_id, thread });
        }
        let live = self.live.entry(id.to_owned()).or_default();
        live.storage_failed = false;
        live.worked |= recovered_work;
        live.recovering_storage = live.processing;
        live.error = if live.recovering_storage {
            Some("History is saved. Waiting for the stopped turn to settle.".into())
        } else if !matches!(settings.status.as_str(), "completed" | "archived")
            && !live.last_prompt.is_empty()
        {
            Some("History is saved. Retry to continue the interrupted turn.".into())
        } else {
            None
        };
        // The cancelled turn settles by itself; with no agent there is nothing to wait for.
        if !self.drivers.contains_key(id) {
            let live = self.live.entry(id.to_owned()).or_default();
            live.processing = false;
            live.recovering_storage = false;
        }
        if !matches!(settings.status.as_str(), "completed" | "archived") {
            self.set_runtime_status(id, "blocked");
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    fn replace_session(&mut self, id: &str) -> Result<Value, String> {
        if let Some(error) = self.cannot_turn(id) {
            return Err(error);
        }
        if !self
            .live
            .get(id)
            .is_some_and(|live| live.replacement && !live.storage_failed && !live.shutting_down)
        {
            return Err("This conversation doesn't need a replacement session.".into());
        }
        let context = self
            .locate(id)
            .map(|thread| history_text(&thread.messages))
            .unwrap_or_default();
        if !self.set_runtime_status(id, "processing") {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        if !self.ensure_driver(id) {
            self.set_runtime_status(id, "blocked");
            return Err("Could not load this conversation's saved agent settings.".into());
        }
        if !self.record_visible(
            id,
            "lifecycle",
            json!({"event":"replacement_session_approved"}),
        ) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        if let Some(slot) = self.drivers.get(id)
            && let Err(error) = slot.driver.send(acp::Command::ReplaceSession { context })
        {
            self.drivers.remove(id);
            self.live.entry(id.to_owned()).or_default().error = Some(error.clone());
            self.set_runtime_status(id, "blocked");
            self.send_live(id);
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        live.replacement = false;
        live.error = None;
        live.processing = true;
        live.assistant = None;
        self.send_live(id);
        Ok(Value::Null)
    }

    /// Copies a conversation through its finished reply `message` into a new
    /// conversation. The source and its agent are left alone.
    fn fork(&mut self, id: &str, message: usize) -> Result<Value, String> {
        if self
            .project_of(id)
            .is_some_and(|project| self.deleting.contains_key(&project))
        {
            return Err("This project is being deleted.".into());
        }
        let running = self.live.get(id).is_some_and(|live| live.processing)
            && self.locate(id).is_some_and(|thread| {
                thread
                    .messages
                    .iter()
                    .rposition(|m| m.role == "user")
                    .is_some_and(|last| message > last)
            });
        if running {
            return Err("Wait for this reply to finish before forking.".into());
        }
        let store = self.store.clone();
        let (fork, live) = blocking(|| {
            let mut store = store.lock().map_err(|e| e.to_string())?;
            let fork = store.fork_conversation(id, message)?;
            let live = store.conversation(&fork).map(live_for).unwrap_or_default();
            Ok::<_, String>((fork, live))
        })?;
        self.touched.insert(fork.clone(), Instant::now());
        self.live.insert(fork.clone(), live);
        if let Some((project_id, thread)) = self.thread_from_store(&fork) {
            self.broadcast(Delta::Thread { project_id, thread });
        }
        self.send_live(&fork);
        Ok(Value::String(fork))
    }

    /// For a fork without a session yet: its source's session, when a native
    /// fork would give the agent exactly the copied history, and that history as text.
    fn fork_history(&self, id: &str) -> Option<(Option<String>, String)> {
        let settings = self.conversation_settings(id)?;
        if settings.session_id.is_some() {
            return None;
        }
        let origin = settings.forked_from?;
        let context = history_text(self.locate(id)?.messages.get(..=origin.message)?);
        let source = &origin.conversation_id;
        let unchanged = self.live.get(source).is_none_or(|live| !live.processing)
            && self
                .locate(source)
                .is_some_and(|thread| thread.messages.len() == origin.message + 1);
        let session = self
            .conversation_settings(source)
            .and_then(|source| source.session_id)
            .filter(|_| unchanged);
        Some((session, context))
    }

    /// Changes a model, effort, mode or other option for this conversation
    /// only, never for its agent definition (scope R17, R18).
    fn set_option(
        &mut self,
        id: &str,
        category: Category,
        option: &str,
        value: String,
    ) -> Result<Value, String> {
        if self.live.get(id).is_some_and(|l| l.processing) {
            return Err("Switch after this turn finishes.".into());
        }
        let mut settings = self
            .conversation_settings(id)
            .ok_or("This conversation no longer exists.")?;
        settings.execution.selections.set(category, option, &value);
        let execution = settings.execution.clone();
        if !self.save_conversation_settings(id, settings) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        self.record_visible(
            id,
            "lifecycle",
            json!({"event":"setting_switched","category":category,"option":option,"value":value}),
        );
        self.live.entry(id.to_owned()).or_default().execution = Some(execution);
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::SetOption {
                category,
                id: option.to_owned(),
                value,
            });
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    fn answer_permission(
        &mut self,
        id: &str,
        request_id: u64,
        option: &str,
    ) -> Result<Value, String> {
        let Some((index, request)) = self.live.get(id).and_then(|live| {
            live.permission
                .iter()
                .enumerate()
                .find(|(_, request)| request.request_id == request_id)
        }) else {
            return Err("Permission already answered".into());
        };
        let Some(choice) = request.options.iter().find(|choice| choice.id == option) else {
            return Err("That choice is no longer offered.".into());
        };
        let denied = choice.kind == PermissionKind::RejectOnce;
        let decision = json!({"request_id":request_id,"option_id":option,"kind":choice.kind,"name":choice.name,"denied":denied});
        if !self.record_visible(id, "permission_decision", decision) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        let result = self
            .drivers
            .get(id)
            .ok_or_else(|| "Agent is no longer running.".to_owned())
            .and_then(|slot| {
                slot.driver.send(acp::Command::Permission {
                    request_id,
                    option_id: option.to_owned(),
                })
            });
        if let Err(error) = result {
            self.drivers.remove(id);
            let live = self.live.entry(id.to_owned()).or_default();
            live.processing = false;
            live.permission.clear();
            live.error = Some(error.clone());
            self.record_visible(id, "error", json!({"message":error}));
            self.set_runtime_status(id, "blocked");
            self.send_live(id);
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        live.permission.remove(index);
        if live.permission.is_empty() {
            self.set_runtime_status(id, "processing");
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    fn mark_read(&mut self, id: &str, through: usize) -> Value {
        if self
            .live
            .get(id)
            .and_then(|live| live.last_read_through)
            .is_some_and(|last| last >= through)
        {
            return Value::Null;
        }
        if self.record_visible(id, "message_read", json!({"through":through})) {
            self.broadcast(Delta::Read {
                id: id.to_owned(),
                through,
            });
        } else {
            self.send_live(id);
        }
        Value::Null
    }

    // -----------------------------------------------------------------------
    // Projects, agents, harnesses and settings

    fn save_project(
        &mut self,
        original: Option<&str>,
        name: &str,
        directory: &Path,
    ) -> Result<Value, String> {
        if original.is_some_and(|id| self.deleting.contains_key(id)) {
            return Err("This project is being deleted.".into());
        }
        let store = self.store.clone();
        let (id, workspace) = blocking(|| {
            let mut store = store.lock().map_err(|e| e.to_string())?;
            let id = store.save_project(original, name, directory)?;
            let workspace = store
                .projects
                .iter()
                .find(|project| project.id == id)
                .map(storage::ProjectRecord::to_workspace)
                .ok_or_else(|| "Saved project is missing.".to_owned())?;
            Ok::<_, String>((id, workspace))
        })?;
        self.upsert_project(original.map(str::to_owned), workspace);
        Ok(Value::String(id))
    }

    fn upsert_project(&mut self, previous: Option<String>, workspace: Workspace) {
        let key = previous
            .as_deref()
            .unwrap_or(&workspace.config.id)
            .to_owned();
        if let Some(project) = self.projects.iter_mut().find(|p| p.config.id == key) {
            project.config = workspace.config.clone();
        } else {
            self.projects.push(workspace.clone());
        }
        self.broadcast_message(&EngineMessage::Delta(Delta::Project {
            previous,
            workspace,
        }));
    }

    fn finish_deletions(&mut self) {
        let ready: Vec<_> = self
            .deleting
            .keys()
            .filter(|id| {
                self.project_conversations(id)
                    .iter()
                    .all(|c| !self.drivers.contains_key(c))
            })
            .cloned()
            .collect();
        for id in ready {
            let waiters = self.deleting.remove(&id).unwrap_or_default();
            let store = self.store.clone();
            let result = blocking(|| {
                store
                    .lock()
                    .map_err(|_| "Project storage is unavailable.".to_owned())
                    .and_then(|mut store| store.delete_project(&id))
            });
            if result.is_ok() {
                for conversation in self.project_conversations(&id) {
                    self.live.remove(&conversation);
                }
                self.projects.retain(|p| p.config.id != id);
                self.broadcast_message(&EngineMessage::Delta(Delta::ProjectRemoved {
                    id: id.clone(),
                }));
            }
            for (client, request) in waiters {
                self.reply(client, request, result.clone().map(|()| Value::Null));
            }
        }
    }

    fn agents_changed(&mut self) {
        self.broadcast(Delta::Agents {
            entries: self.agents.entries.clone(),
            errors: self.agents.errors.clone(),
        });
        self.refresh_icons();
    }

    fn refresh_icons(&mut self) {
        let icons = blocking(disk_icons);
        if icons != self.icons {
            self.icons = icons;
            self.broadcast(Delta::Icons(self.icons.clone()));
        }
    }

    fn detect_harnesses(&self, fetch: bool) {
        let known = self.harnesses.harnesses.clone();
        let input = self.input.clone();
        tokio::task::spawn_blocking(move || {
            let _ = input.send(Input::Harnesses(harness::detect(fetch, known)));
        });
    }

    /// What an agent would run with on this machine, for probes and logins.
    fn standalone(
        &self,
        harness_id: &str,
        command: &str,
        arguments: Vec<String>,
    ) -> Result<storage::ExecutionConfig, String> {
        let launch = if harness_id == harness::CUSTOM || Path::new(command).is_absolute() {
            blocking(|| harness::resolve(command))
                .map(|path| storage::Launch {
                    command: path.to_string_lossy().into_owned(),
                    arguments,
                    environment: Vec::new(),
                })
                .ok_or_else(|| format!("Command {command} was not found on this machine."))?
        } else if self.harnesses.installed.contains_key(harness_id) {
            self.harnesses.launch(harness_id, "", &[])?
        } else {
            return Err(format!(
                "{} is not installed.",
                self.harnesses.label(harness_id, "")
            ));
        };
        Ok(storage::ExecutionConfig {
            name: self.harnesses.label(harness_id, ""),
            harness: harness_id.to_owned(),
            command: launch.command,
            arguments: launch.arguments,
            environment: launch.environment,
            directory: std::env::temp_dir(),
            ..Default::default()
        })
    }

    fn probe(
        &mut self,
        client: u64,
        request: u64,
        probe: u64,
        harness_id: &str,
        command: &str,
        arguments: Vec<String>,
        model: Option<String>,
    ) -> Option<Result<Value, String>> {
        let mut config = match self.standalone(harness_id, command, arguments) {
            Ok(config) => config,
            Err(message) => {
                let failed: Result<acp::Probed, _> = Err(acp::ProbeError {
                    message,
                    auth: Vec::new(),
                    version: false,
                });
                return Some(serde_json::to_value(failed).map_err(|e| e.to_string()));
            }
        };
        config.selections.model = model.unwrap_or_default();
        let (session, results) = acp::probe(config);
        self.probes.insert((client, probe), session);
        let input = self.input.clone();
        tokio::spawn(async move {
            let result = results.recv().await.ok();
            let _ = input.send(Input::ProbeDone {
                client,
                request,
                probe,
                result,
            });
        });
        None
    }

    /// Logs an agent in or out. A terminal method opens the user's own
    /// terminal; the others go through the agent (scope R14).
    fn login(
        &mut self,
        client: u64,
        request: u64,
        harness_id: &str,
        command: &str,
        arguments: Vec<String>,
        method: Option<AuthMethod>,
    ) -> Option<Result<Value, String>> {
        let config = match self.standalone(harness_id, command, arguments) {
            Ok(config) => config,
            Err(error) => return Some(Err(error)),
        };
        if let Some(terminal) = method.as_ref().and_then(|method| method.terminal.clone()) {
            let result = blocking(|| {
                install::open_terminal(
                    Path::new(&config.command),
                    &terminal.arguments,
                    &terminal.environment,
                )
            });
            return Some(result.map(|()| Value::Null));
        }
        let (session, results, urls) = acp::login(config, method.map(|method| method.id));
        self.logins.insert((client, request), session);
        let input = self.input.clone();
        tokio::spawn(async move {
            let url_input = input.clone();
            let opener = tokio::spawn(async move {
                while let Ok(url) = urls.recv().await {
                    let _ = url_input.send(Input::OpenUrl(url));
                }
            });
            let result = results
                .recv()
                .await
                .unwrap_or_else(|_| Err("The login stopped.".into()))
                .map(|()| Value::Null);
            opener.abort();
            let _ = input.send(Input::Done {
                client,
                request,
                result,
                detect: false,
            });
        });
        None
    }

    fn set_settings(&mut self, settings: EngineSettings) -> Result<Value, String> {
        let path = settings_path()?;
        blocking(|| {
            config::seed_yaml(&path, &settings).and_then(|()| config::write_yaml(&path, &settings))
        })?;
        self.settings = settings;
        let retries = u32::try_from(self.settings.retry_limit).unwrap_or(u32::MAX);
        for slot in self.drivers.values() {
            let _ = slot.driver.send(acp::Command::SetRetries(retries));
        }
        self.broadcast(Delta::Settings(self.settings.clone()));
        Ok(Value::Null)
    }

    // -----------------------------------------------------------------------
    // File watching

    fn watch(input: mpsc::UnboundedSender<Input>) -> Option<notify::RecommendedWatcher> {
        use notify::Watcher as _;
        let root = config::directory().ok()?;
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event
                && !matches!(event.kind, notify::EventKind::Access(_))
            {
                let _ = input.send(Input::Watch(event.paths));
            }
        });
        let result = watcher.and_then(|mut watcher| {
            for folder in ["agents", "projects"] {
                let path = root.join(folder);
                let _ = std::fs::create_dir_all(&path);
                watcher.watch(&path, notify::RecursiveMode::Recursive)?;
            }
            Ok(watcher)
        });
        result
            .map_err(|error| log(format!("Cannot watch the data folders: {error}")))
            .ok()
    }

    fn reload_watched(&mut self) {
        let Ok(root) = config::directory() else {
            return;
        };
        let paths = std::mem::take(&mut self.watched);
        let agents_root = root.join("agents");
        let projects_root = root.join("projects");
        if paths.iter().any(|path| path.starts_with(&agents_root)) {
            let changed = blocking(|| self.agents.refresh());
            if changed {
                self.agents_changed();
            }
        }
        // The engine's own transcript writes for running agents don't need a reload.
        let outside = paths.iter().any(|path| {
            let Ok(relative) = path.strip_prefix(&projects_root) else {
                return false;
            };
            let mut parts = relative.iter().map(|part| part.to_string_lossy());
            let conversation = match (parts.next(), parts.next(), parts.next()) {
                (Some(_), Some(folder), Some(id)) if folder == "conversations" => Some(id),
                _ => None,
            };
            conversation.is_none_or(|id| !self.drivers.contains_key(id.as_ref()))
        });
        if outside {
            let input = self.input.clone();
            let started = Instant::now();
            tokio::task::spawn_blocking(move || {
                let store = ProjectStore::with_root(projects_root);
                let _ = input.send(Input::ProjectsLoaded { store, started });
            });
        }
    }

    /// Takes in project files changed outside the engine.
    fn reconcile(&mut self, fresh: ProjectStore, started: Instant) {
        let busy: HashSet<String> = self
            .drivers
            .keys()
            .cloned()
            .chain(
                self.touched
                    .iter()
                    .filter(|(_, at)| **at + Duration::from_secs(1) >= started)
                    .map(|(id, _)| id.clone()),
            )
            .chain(
                self.deleting
                    .keys()
                    .flat_map(|p| self.project_conversations(p)),
            )
            .collect();
        let deleting: HashSet<String> = self.deleting.keys().cloned().collect();
        let store = self.store.clone();
        let changes = {
            let Ok(mut store) = store.lock() else {
                return;
            };
            store.reconcile(fresh, &busy, &deleting)
        };
        for error in &changes.errors {
            log(error);
            self.broadcast_message(&EngineMessage::Delta(Delta::Notice {
                message: error.clone(),
            }));
        }
        for workspace in changes.projects {
            self.upsert_project(None, workspace);
        }
        for id in changes.removed {
            for conversation in self.project_conversations(&id) {
                self.live.remove(&conversation);
            }
            self.projects.retain(|p| p.config.id != id);
            self.broadcast_message(&EngineMessage::Delta(Delta::ProjectRemoved { id }));
        }
        for id in changes.conversations {
            let Some((project_id, thread)) = self.thread_from_store(&id) else {
                continue;
            };
            let live = self
                .store
                .lock()
                .ok()
                .and_then(|store| store.conversation(&id).map(live_for));
            if let Some(live) = live {
                self.live.insert(id.clone(), live);
            }
            self.broadcast(Delta::Thread { project_id, thread });
            self.send_live(&id);
        }
    }
}
