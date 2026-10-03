//! The conversation engine: the background process that owns agent processes,
//! conversation state and every engine-owned file. One runs per OS user.
//!
//! Every command and agent event goes through one task, in arrival order, so
//! commands from several clients can't interleave or deadlock.
use crate::{
    acp,
    agents::{self, AgentCatalog, PermissionMode},
    config,
    data::{Message, Thread, Workspace},
    harness, ipc,
    protocol::{
        ActiveConversation, ClientMessage, Command, Delta, EngineMessage, EngineSettings, Live,
        PROTOCOL, PendingPermission, PermissionOption, Snapshot, Status,
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
const STOP_GRACE: Duration = Duration::from_secs(5);
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
    if let Err(error) = crate::platform::contain_children() {
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
    Harnesses(Vec<harness::Harness>, HashMap<String, PathBuf>),
    ProbeDone {
        client: u64,
        request: u64,
        probe: u64,
        result: Option<Result<harness::Probed, harness::ProbeError>>,
    },
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
    probes: HashMap<(u64, u64), harness::Probe>,
    permission_stopped: HashSet<String>,
    /// Turns waiting for their conversation's closing agent to exit.
    queued: HashMap<String, String>,
    /// When the engine itself last wrote each conversation, for file watching.
    touched: HashMap<String, Instant>,
    watched: Vec<PathBuf>,
    watch_due: Option<Instant>,
    _watcher: Option<notify::RecommendedWatcher>,
    status: Status,
    exit: bool,
}

/// Runs blocking file work without stalling the runtime's other tasks.
fn blocking<T>(work: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(work)
}

fn live_for(conversation: &storage::StoredConversation) -> Live {
    let thread = conversation.to_thread();
    let last_user = thread.messages.iter().rposition(|m| m.role == "user");
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
        options: conversation.settings.config_options.clone(),
        permission_mode: Some(conversation.settings.permission_mode),
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
            permission_stopped: HashSet::new(),
            queued: HashMap::new(),
            touched: HashMap::new(),
            watched: Vec::new(),
            watch_due: None,
            status: Status::default(),
            exit: false,
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

    fn broadcast_message(&self, message: &EngineMessage) {
        let Ok(mut line) = serde_json::to_string(message) else {
            return;
        };
        line.push('\n');
        for client in self.clients.values().filter(|c| c.subscribed) {
            let _ = client.sender.send(line.clone());
        }
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
            Input::Harnesses(harnesses, installed) => {
                self.harnesses.harnesses = harnesses;
                self.harnesses.installed = installed;
                self.harnesses.detected = true;
                self.broadcast(Delta::Harnesses(self.harnesses.clone()));
                self.refresh_icons();
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
            ClientMessage::Hello { protocol, cli } => {
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
                    self.send_to(client, &EngineMessage::Snapshot(Box::new(self.snapshot())));
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
                permission_mode,
                prompt,
            } => self
                .send(
                    &project_id,
                    conversation_id,
                    agent_id.as_deref(),
                    permission_mode,
                    &prompt,
                )
                .map(Value::String),
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
            Command::RetryStorage { id } => {
                conversation(self, &id).and_then(|()| self.retry_storage(&id))
            }
            Command::ReplaceSession { id } => {
                conversation(self, &id).and_then(|()| self.replace_session(&id))
            }
            Command::SetPermissionMode { id, mode } => {
                conversation(self, &id).map(|()| self.set_permission_mode(&id, mode))
            }
            Command::SwitchSetting { id, effort, value } => {
                conversation(self, &id).and_then(|()| self.switch_setting(&id, effort, value))
            }
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
            } => {
                return self.probe(client, request, probe, &harness, &command, arguments, model);
            }
            Command::CancelProbe { probe } => {
                self.probes.remove(&(client, probe));
                Ok(Value::Null)
            }
            Command::SetSettings { settings } => self.set_settings(settings),
            Command::Status => {
                serde_json::to_value(self.compute_status()).map_err(|e| e.to_string())
            }
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
        let store = self.store.clone();
        let record: acp::Recorder = Arc::new(move |id: &str, direction: &str, value: &Value| {
            let mut store = store.lock().map_err(|e| e.to_string())?;
            if direction == "session" {
                let (project_id, mut settings) = store
                    .projects
                    .iter()
                    .find_map(|p| {
                        p.conversations
                            .iter()
                            .find(|c| c.id == id)
                            .map(|c| (p.id.clone(), c.settings.clone()))
                    })
                    .ok_or_else(|| "Conversation is missing from storage.".to_owned())?;
                if value["replaced"].as_bool() == Some(true) {
                    let old = value["old_session_id"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| settings.session_id.take());
                    if let Some(old) = old {
                        settings.previous_session_ids.push(old);
                    }
                }
                settings.session_id = value["session_id"].as_str().map(str::to_owned);
                let event_result = store
                    .record_event(id, &storage::TranscriptEvent::new("session", value.clone()));
                let settings_result = store.update_conversation(&project_id, id, settings);
                event_result.and(settings_result)
            } else if matches!(direction, "visible_text" | "visible_tool" | "visible_usage") {
                let kind = match direction {
                    "visible_text" => "assistant_chunk",
                    "visible_tool" => "tool",
                    _ => "usage",
                };
                store.record_event(id, &storage::TranscriptEvent::new(kind, value.clone()))
            } else {
                store.record_raw(id, direction, value)
            }
        });
        let driver = acp::Driver::spawn(
            id.to_owned(),
            settings.execution,
            settings.session_id,
            settings.permission_mode,
            self.events.clone(),
            record,
        );
        self.drivers.insert(id.to_owned(), Slot { driver, turn: 0 });
        self.live.entry(id.to_owned()).or_default().running = true;
        true
    }

    /// Why this conversation's agent cannot start: an old snapshot, or a
    /// harness that is no longer installed.
    fn cannot_start(&self, id: &str) -> Option<String> {
        let execution = self.live.get(id)?.execution.as_ref()?;
        if execution.legacy() {
            return Some(
                "This conversation was created by an older Adeline version; start a new chat."
                    .into(),
            );
        }
        let command = Path::new(&execution.command);
        (command.is_absolute() && !command.is_file()).then(|| {
            format!(
                "The {} harness is not installed.",
                self.harnesses
                    .label(&execution.harness, &execution.identity)
            )
        })
    }

    fn send(
        &mut self,
        project_id: &str,
        conversation_id: Option<String>,
        agent_id: Option<&str>,
        permission_mode: Option<PermissionMode>,
        prompt: &str,
    ) -> Result<String, String> {
        if self.stop_all.is_some() {
            return Err("The conversation engine is stopping its agents.".into());
        }
        if self.deleting.contains_key(project_id) {
            return Err("This project is being deleted.".into());
        }
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() {
            return Err("Enter a message to send.".into());
        }
        let id = if let Some(id) = conversation_id {
            let Some(thread) = self.locate(&id) else {
                return Err("This conversation no longer exists.".into());
            };
            if thread.status == "archived" {
                return Err("This conversation is archived.".into());
            }
            let live = self.live.entry(id.clone()).or_default();
            if live.processing {
                return Err(ALREADY_PROCESSING.into());
            }
            if live.shutting_down {
                return Err("Wait for the agent to stop before sending.".into());
            }
            if live.storage_failed || live.recovering_storage {
                return Err("Retry storage before sending.".into());
            }
            if live.replacement {
                return Err("Start a replacement session before sending.".into());
            }
            id
        } else {
            let mut agent = agent_id
                .and_then(|id| self.agents.entries.iter().find(|entry| entry.id == id))
                .map(|entry| entry.definition.clone())
                .ok_or("Create or select an agent before sending.")?;
            let launch = self
                .harnesses
                .launch(&agent.harness, &agent.command, &agent.arguments)?;
            if let Some(mode) = permission_mode {
                agent.permission_mode = mode;
            }
            let store = self.store.clone();
            let title = crate::short(&prompt, 100);
            let (id, thread, live) = blocking(|| {
                let mut store = store.lock().map_err(|e| e.to_string())?;
                let id = store.create_conversation(project_id, &agent, launch, &title)?;
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
        prompt.clone_into(&mut self.live.entry(id.clone()).or_default().last_prompt);
        let saved = self.record_visible(
            &id,
            "message",
            json!({"role":"user","text":prompt,"read":true}),
        );
        self.broadcast(Delta::Message {
            id: id.clone(),
            message: Message {
                role: "user".into(),
                text: prompt.clone(),
                read: true,
                created_at: crate::recency::now().to_string(),
                ..Default::default()
            },
        });
        if saved {
            self.start_prompt(&id, prompt, false);
        }
        self.send_live(&id);
        Ok(id)
    }

    fn start_prompt(&mut self, id: &str, prompt: String, retry: bool) {
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
        live.progress = None;
        live.assistant = None;
        live.permission.clear();
        if !retry {
            live.worked = false;
            live.last_prompt.clone_from(&prompt);
        }
        let retries = u32::try_from(self.settings.retry_limit).unwrap_or(u32::MAX);
        // The worker numbers turns; a prompt it rejects keeps the old number.
        let Some(slot) = self.drivers.get_mut(id) else {
            return;
        };
        if let Err(error) = slot.driver.send(acp::Command::Prompt {
            text: prompt,
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

    fn driver_event(&mut self, event: acp::Event) {
        let id = event.conversation_id.clone();
        if self.locate(&id).is_none() {
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
            acp::EventKind::Session {
                session_id,
                replaced,
            } => {
                self.record_visible(
                    &id,
                    "lifecycle",
                    json!({"event":"session_ready","session_id":session_id,"replacement":replaced}),
                );
            }
            acp::EventKind::Options(options) => {
                let live = self.live.entry(id.clone()).or_default();
                if live.options != options {
                    live.options.clone_from(&options);
                    if let Some(mut settings) = self.conversation_settings(&id) {
                        settings.config_options = options;
                        self.save_conversation_settings(&id, settings);
                    }
                }
            }
            acp::EventKind::Text(text) => {
                // Streamed text is durably recorded by the worker before dispatch.
                self.broadcast(Delta::Text {
                    id,
                    text,
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
                self.broadcast(Delta::Tool {
                    id,
                    tool_id,
                    title,
                    status,
                    detail,
                    kind,
                    paths,
                    at: crate::recency::now_ms(),
                });
                return;
            }
            acp::EventKind::Usage { used, size } => {
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
                let options: Vec<_> = options
                    .into_iter()
                    .map(|option| PermissionOption {
                        option_id: option.option_id,
                        name: option.name,
                        kind: option.kind,
                    })
                    .collect();
                let choices: Vec<_> = options
                    .iter()
                    .map(|option| json!({"id":option.option_id,"name":option.name,"kind":option.kind}))
                    .collect();
                if self.record_visible(
                    &id,
                    "permission_request",
                    json!({"request_id":request_id,"title":title,"options":choices}),
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
                if saved {
                    live.error = Some(message);
                    if !closing {
                        self.set_runtime_status(&id, "blocked");
                    }
                }
            }
            acp::EventKind::Finished { stop_reason } => {
                self.record_visible(
                    &id,
                    "lifecycle",
                    json!({"event":"turn_finished","reason":stop_reason}),
                );
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.progress = None;
                live.permission.clear();
                if !closing && !live.storage_failed {
                    self.set_runtime_status(&id, "idle");
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
                let finished = matches!(self.status_of(&id).as_str(), "completed" | "archived");
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.shutting_down = false;
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
                    self.start_prompt(&id, prompt, true);
                }
                self.send_live(&id);
                self.finish_deletions();
                self.finish_stop_all();
                return;
            }
            acp::EventKind::StorageError(error) => self.storage_failure(&id, &error),
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
        let prompt = if live.worked {
            "Continue the interrupted turn from the saved session. Preserve completed work; do not repeat completed tool actions.".to_owned()
        } else {
            live.last_prompt.clone()
        };
        if prompt.is_empty() {
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
            self.start_prompt(id, prompt, true);
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
        live.permission_mode = Some(settings.permission_mode);
        live.error = if live.recovering_storage {
            Some("History is saved. Waiting for the stopped turn to settle.".into())
        } else if !matches!(settings.status.as_str(), "completed" | "archived")
            && !live.last_prompt.is_empty()
        {
            Some("History is saved. Retry to continue the interrupted turn.".into())
        } else {
            None
        };
        if let Some(slot) = self.drivers.get(id) {
            if slot.driver.send(acp::Command::ResumeStorage).is_ok() {
                let _ = slot
                    .driver
                    .send(acp::Command::SetPermissionMode(settings.permission_mode));
            } else {
                self.drivers.remove(id);
                let live = self.live.entry(id.to_owned()).or_default();
                live.processing = false;
                live.recovering_storage = false;
            }
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
            .map(|thread| {
                thread
                    .messages
                    .iter()
                    .map(|m| format!("{}: {}", m.role, m.text))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
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

    fn switch_setting(&mut self, id: &str, effort: bool, value: String) -> Result<Value, String> {
        if self.live.get(id).is_some_and(|l| l.processing) {
            return Err("Wait for the turn to finish before switching.".into());
        }
        let mut settings = self
            .conversation_settings(id)
            .ok_or("This conversation no longer exists.")?;
        let kind = if effort {
            settings.execution.effort.clone_from(&value);
            harness::Kind::Effort
        } else {
            settings.execution.model.clone_from(&value);
            harness::Kind::Model
        };
        let execution = settings.execution.clone();
        if !self.save_conversation_settings(id, settings) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        self.record_visible(
            id,
            "lifecycle",
            json!({"event":"setting_switched","kind":format!("{kind:?}"),"value":value}),
        );
        self.live.entry(id.to_owned()).or_default().execution = Some(execution);
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::SetOption { kind, value });
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    fn set_permission_mode(&mut self, id: &str, mode: PermissionMode) -> Value {
        let Some(mut settings) = self.conversation_settings(id) else {
            return Value::Null;
        };
        settings.permission_mode = mode;
        if self.save_conversation_settings(id, settings) {
            self.live.entry(id.to_owned()).or_default().permission_mode = Some(mode);
            if self.record_visible(id, "permission_mode", json!({"mode":mode}))
                && let Some(slot) = self.drivers.get(id)
                && let Err(error) = slot.driver.send(acp::Command::SetPermissionMode(mode))
            {
                self.drivers.remove(id);
                self.live.entry(id.to_owned()).or_default().error = Some(error.clone());
                self.record_visible(id, "error", json!({"message":error}));
                self.set_runtime_status(id, "blocked");
            }
        }
        self.send_live(id);
        Value::Null
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
        let Some(choice) = request.options.iter().find(|choice| {
            choice.option_id == option
                && matches!(
                    choice.kind.as_str(),
                    "allow_once" | "allow_always" | "reject_once"
                )
        }) else {
            return Err("That choice is no longer offered.".into());
        };
        let decision = json!({"request_id":request_id,"option_id":option,"kind":choice.kind,"name":choice.name});
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
            let (harnesses, installed) = harness::detect(fetch, known);
            let _ = input.send(Input::Harnesses(harnesses, installed));
        });
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
        let launch = if harness_id == harness::CUSTOM {
            blocking(|| harness::resolve(command))
                .map(|path| (path, arguments))
                .ok_or_else(|| format!("Command {command} was not found on this machine."))
        } else {
            match (
                self.harnesses.installed.get(harness_id),
                self.harnesses.get(harness_id),
            ) {
                (Some(path), Some(harness)) => Ok((path.clone(), harness.arguments.clone())),
                _ => Err(format!(
                    "{} is not installed.",
                    self.harnesses.label(harness_id, "")
                )),
            }
        };
        let (command, arguments) = match launch {
            Ok(launch) => launch,
            Err(message) => {
                let failed: Result<harness::Probed, _> = Err(harness::ProbeError {
                    message,
                    login: Vec::new(),
                });
                return Some(serde_json::to_value(failed).map_err(|e| e.to_string()));
            }
        };
        let (handle, results) = harness::probe(command, arguments, model);
        self.probes.insert((client, probe), handle);
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
