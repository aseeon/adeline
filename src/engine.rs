//! The conversation engine: the background process that owns agent processes,
//! conversation state and every engine-owned file. One runs per OS user.
//!
//! Every command and agent event goes through one task, in arrival order, so
//! commands from several clients can't interleave or deadlock. Every delta it
//! sends is numbered within this process's `epoch`, and the latest
//! `RECENT_DELTAS` are kept for clients that reconnect.
use crate::{
    acp,
    agents::{self, AgentCatalog},
    config,
    conversation::{Attachment, Category, PermissionKind, TurnState, closed},
    data::{Message, Thread, Workspace},
    harness, install, ipc,
    log::log,
    protocol::{
        ActiveConversation, ClientMessage, Command, Delta, EngineMessage, EngineSettings, Live,
        PROTOCOL, Snapshot, Status,
    },
    storage::{self, ProjectStore},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::block_in_place};

mod cli;
mod events;
mod projects;
mod turns;

pub use cli::main;

const IDLE_EXIT: Duration = Duration::from_secs(60);
const RECENT_DELTAS: usize = 20_000;
const STOP_GRACE: Duration = Duration::from_secs(5);
const QUIET_AFTER_MS: u64 = 5_000;
const ALREADY_PROCESSING: &str =
    "This conversation is already processing (sent from another window)";
const PERMISSION_STOPPED: &str = "Stopped while waiting for permission: no Adeline window was open to answer it. Retry continues the turn.";

fn settings_path() -> Result<PathBuf, String> {
    Ok(ipc::engine_dir()?.join("settings.yml"))
}

fn load_settings() -> EngineSettings {
    // The first start copies the retry limit from the UI's settings.
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
    crate::log::open(dir.join("logs"));
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
    Output {
        client: u64,
        request: u64,
        line: String,
    },
    Done {
        client: u64,
        request: u64,
        result: Result<Value, String>,
        detect: bool,
    },
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
    store: ProjectStore,
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
    logins: HashMap<(u64, u64), acp::Session>,
    watching: HashMap<u64, String>,
    queued_files: HashMap<u64, Vec<Attachment>>,
    next_queued: u64,
    attachments: HashMap<String, Vec<Attachment>>,
    steering: HashMap<String, (String, Vec<Attachment>)>,
    send_after_stop: HashSet<String>,
    last_traffic: HashMap<String, u64>,
    permission_stopped: HashSet<String>,
    text_copy_pending: HashSet<String>,
    restart_prompt: HashMap<String, String>,
    touched: HashMap<String, Instant>,
    watched: Vec<PathBuf>,
    watch_due: Option<Instant>,
    _watcher: Option<notify::RecommendedWatcher>,
    status: Status,
    exit: bool,
    identity: String,
    epoch: String,
    seq: u64,
    recent: std::collections::VecDeque<(u64, String)>,
}

fn unsaved_history(error: &str) -> String {
    format!("History could not be saved: {error}. Restore storage access, then Retry storage.")
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
        .map(|error| unsaved_history(&error))
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

fn list_directory(path: Option<PathBuf>) -> Result<crate::protocol::Listing, String> {
    let path = match path.filter(|path| !path.as_os_str().is_empty()) {
        Some(path) => path,
        None => config::home()?,
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

fn history_text(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn disk_icons() -> BTreeMap<String, String> {
    // Clients can't read these from this machine's disk.
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
        let store = block_in_place(ProjectStore::new);
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
            store,
            projects,
            live,
            drivers: HashMap::new(),
            agents: block_in_place(|| AgentCatalog::new(false)),
            harnesses: harness::Catalog {
                harnesses: block_in_place(harness::load),
                ..Default::default()
            },
            icons: block_in_place(disk_icons),
            settings: block_in_place(load_settings),
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
            restart_prompt: HashMap::new(),
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
            errors: self.store.errors.clone(),
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

    fn command(
        &mut self,
        client: u64,
        request: u64,
        command: Command,
    ) -> Option<Result<Value, String>> {
        // `None` means the reply comes later.
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
                    self.restart_prompt
                        .insert(id.clone(), acp::CONTINUE_TURN.into());
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
                let saved = block_in_place(|| {
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
                let deleted = block_in_place(|| self.agents.delete(&id));
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
                    .map(|id| self.store.traffic(id, 2000))
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
            Command::ListDirectory { path } => block_in_place(|| list_directory(path))
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
        let failed = self.store.flush_due();
        for (id, error) in failed {
            self.storage_failure(&id, &error);
            self.send_live(&id);
        }
        // A running turn with no traffic for a while reads as quiet.
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

    fn conversation_settings(&self, id: &str) -> Option<storage::ConversationSettings> {
        self.store.conversation(id).map(|c| c.settings.clone())
    }

    fn storage_failure(&mut self, id: &str, error: &str) {
        let live = self.live.entry(id.to_owned()).or_default();
        live.storage_failed = true;
        live.recovering_storage = false;
        live.error = Some(unsaved_history(error));
        live.permission.clear();
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::Cancel);
        }
        let status = self.status_of(id);
        if !status.is_empty() && !closed(&status) {
            self.broadcast(Delta::Status {
                id: id.to_owned(),
                status: "blocked".into(),
            });
        }
    }

    fn queue_visible(&mut self, id: &str, kind: &str, data: Value) -> bool {
        self.touched.insert(id.to_owned(), Instant::now());
        let result = self
            .store
            .queue_event(id, storage::TranscriptEvent::new(kind, data));
        if let Err(error) = result {
            if !self.live.get(id).is_some_and(|live| live.storage_failed) {
                self.storage_failure(id, &error);
            }
            false
        } else {
            true
        }
    }

    fn record_visible(&mut self, id: &str, kind: &str, data: Value) -> bool {
        self.touched.insert(id.to_owned(), Instant::now());
        let result = block_in_place(|| {
            self.store
                .record_event(id, &storage::TranscriptEvent::new(kind, data))
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
        let result = block_in_place(|| self.store.update_conversation(&project_id, id, settings));
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
        self.store.projects.iter().find_map(|p| {
            p.conversations
                .iter()
                .find(|c| c.id == id)
                .map(|c| (p.id.clone(), c.to_thread()))
        })
    }

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

    fn drop_undelivered_fork_session(&mut self, id: &str) {
        // Retry then starts a fresh session and sends the copied history again.
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
            "idle" if closed(&current) => {
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
        let store = &mut self.store;
        let (fork, live) = block_in_place(|| {
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

    fn fork_history(&self, id: &str) -> Option<(Option<String>, String)> {
        // The source's session only when a native fork would give exactly the copied history.
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
}
