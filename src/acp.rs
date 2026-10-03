//! One ACP stdio process per conversation. Protocol I/O runs off the GPUI thread.
use crate::{
    agents::{InstructionsMode, PermissionMode},
    harness::{self, Kind},
    storage::ExecutionConfig,
};
use async_channel::Sender;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    process::{Child, ChildStdin, Command as ProcessCommand},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::block_in_place,
};

pub type Recorder = Arc<dyn Fn(&str, &str, &Value) -> Result<(), String> + Send + Sync>;

pub struct Driver {
    sender: UnboundedSender<Input>,
}

#[derive(Debug)]
pub enum Command {
    Prompt {
        text: String,
        retries: u32,
    },
    Cancel,
    Permission {
        request_id: u64,
        option_id: String,
    },
    SetPermissionMode(PermissionMode),
    /// Changes the running turn's retry limit; later turns pass their own.
    SetRetries(u32),
    /// Switches the model or effort; applied now when idle, else at next setup.
    SetOption {
        kind: Kind,
        value: String,
    },
    ReplaceSession {
        context: String,
    },
    Shutdown,
    ForceStop,
    ResumeStorage,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub conversation_id: String,
    pub turn: u64,
    pub kind: EventKind,
}

#[derive(Debug, Clone)]
pub enum EventKind {
    Session {
        session_id: String,
        replaced: bool,
    },
    Text(String),
    Tool {
        id: String,
        title: String,
        status: String,
        detail: String,
        /// The protocol's tool kind: `read`, `edit`, `delete`, `move`, `execute`, ...
        kind: String,
        /// Files the call reads or changes.
        paths: Vec<String>,
    },
    /// The config options the session now offers.
    Options(Vec<Value>),
    /// How much of the agent's context window the session uses, in tokens.
    Usage {
        used: u64,
        size: u64,
    },
    Permission {
        request_id: u64,
        title: String,
        options: Vec<PermissionChoice>,
    },
    Retrying {
        attempt: u32,
        limit: u32,
        error: String,
    },
    Error {
        message: String,
        kind: FailureKind,
    },
    Finished {
        stop_reason: String,
    },
    Stopped,
    ShutdownStuck,
    ShutdownComplete,
    StorageError(String),
    ReplacementRequired(String),
}

#[derive(Debug, Clone)]
pub struct PermissionChoice {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Temporary,
    Configuration,
    Authentication,
    Denied,
}

impl Driver {
    pub fn spawn(
        conversation_id: String,
        config: ExecutionConfig,
        session_id: Option<String>,
        permission_mode: PermissionMode,
        events: Sender<Event>,
        record: Recorder,
    ) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let worker = Worker {
            id: conversation_id,
            config,
            session_id,
            permission_mode,
            events,
            record,
            sender: sender.clone(),
            receiver,
            child: None,
            stdin: None,
            epoch: 0,
            next_request: 0,
            turn: 0,
            pending: HashMap::new(),
            handshake: None,
            permissions: HashMap::new(),
            capabilities: Value::Null,
            options: Vec::new(),
            tools: HashMap::new(),
            setup: None,
            configured: false,
            restore_required: false,
            active: None,
            unprocessed: Vec::new(),
            blocked: false,
            closing: false,
            forced: false,
            shutdown_deadline: None,
            stuck_reported: false,
            diagnostics: VecDeque::new(),
            timeout_reported: false,
            lost_pending: None,
            lost_at: None,
            lost_during_initialize: false,
            replacement: None,
            previous_session: None,
            pending_context: None,
            replacing: false,
        };
        tokio::spawn(worker.run());
        Self { sender }
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        self.sender
            .send(Input::Command(command))
            .map_err(|_| "Agent worker has stopped.".into())
    }
}

#[derive(Debug)]
enum Input {
    Command(Command),
    Wire { epoch: u64, message: Value },
    Lost { epoch: u64, error: String },
    Diagnostic { epoch: u64, line: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    Initialize,
    Setup,
    Model,
    Effort,
    Switch,
    Prompt,
    Close,
}

/// The latest state of one tool call. Updates may omit fields they don't change.
#[derive(Clone, Default)]
struct ToolState {
    title: String,
    status: String,
    detail: String,
    kind: String,
    paths: Vec<String>,
}

struct Turn {
    text: String,
    retries: u32,
    attempt: u32,
    worked: bool,
    observed_text: String,
    denied: bool,
    cancelled: bool,
    prompt_request: Option<u64>,
    retry_at: Option<Instant>,
}

struct Worker {
    id: String,
    config: ExecutionConfig,
    session_id: Option<String>,
    permission_mode: PermissionMode,
    events: Sender<Event>,
    record: Recorder,
    sender: UnboundedSender<Input>,
    receiver: UnboundedReceiver<Input>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    epoch: u64,
    next_request: u64,
    pending: HashMap<u64, Request>,
    handshake: Option<(u64, Request, Instant)>,
    permissions: HashMap<u64, (u64, Vec<PermissionChoice>)>,
    capabilities: Value,
    options: Vec<Value>,
    tools: HashMap<String, ToolState>,
    setup: Option<Request>,
    configured: bool,
    restore_required: bool,
    active: Option<Turn>,
    turn: u64,
    unprocessed: Vec<(Value, bool)>,
    blocked: bool,
    closing: bool,
    shutdown_deadline: Option<Instant>,
    stuck_reported: bool,
    diagnostics: VecDeque<String>,
    timeout_reported: bool,
    lost_pending: Option<String>,
    lost_at: Option<Instant>,
    lost_during_initialize: bool,
    replacement: Option<String>,
    previous_session: Option<String>,
    pending_context: Option<String>,
    forced: bool,
    replacing: bool,
}

impl Worker {
    fn emit(&self, kind: EventKind) {
        let _ = self.events.try_send(Event {
            conversation_id: self.id.clone(),
            turn: self.turn,
            kind,
        });
    }

    fn record(&mut self, direction: &str, message: &Value) -> bool {
        match block_in_place(|| (self.record)(&self.id, direction, message)) {
            Ok(()) => true,
            Err(error) => {
                if !self.blocked {
                    self.blocked = true;
                    self.emit(EventKind::StorageError(format!(
                        "Conversation history could not be saved: {error}. Retry saving before sending again."
                    )));
                    self.cancel();
                }
                false
            }
        }
    }

    async fn run(mut self) {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let input = tokio::select! {
                input = self.receiver.recv() => Some(input),
                _ = tick.tick() => None,
            };
            match input {
                Some(Some(Input::Command(command))) => self.command(command),
                Some(Some(Input::Wire { epoch, message })) if epoch == self.epoch => {
                    if self.blocked {
                        self.unprocessed.push((message, false));
                    } else if self.record("incoming", &message) {
                        self.incoming(&message);
                    } else {
                        self.unprocessed.push((message, true));
                    }
                }
                Some(Some(Input::Lost { epoch, error })) if epoch == self.epoch => {
                    self.lost(error);
                }
                Some(Some(Input::Diagnostic { epoch, line })) if epoch == self.epoch => {
                    if self.diagnostics.len() == 8 {
                        self.diagnostics.pop_front();
                    }
                    self.diagnostics.push_back(line);
                }
                Some(None) => self.shutdown(),
                _ => (),
            }
            if let Some(child) = &mut self.child {
                match child.try_wait() {
                    Ok(Some(_)) if self.forced => {
                        self.child = None;
                        self.stdin = None;
                        self.emit(EventKind::ShutdownComplete);
                        break;
                    }
                    Ok(Some(_))
                        if self
                            .lost_at
                            .is_some_and(|when| when.elapsed() < Duration::from_millis(100)) => {}
                    Ok(Some(status)) if self.lost_pending.is_some() => {
                        let error = self.lost_pending.take().expect("checked above");
                        let diagnostics = self.diagnostics.make_contiguous().join("\n");
                        let missing_initialize = self.lost_during_initialize;
                        self.child = None;
                        self.stdin = None;
                        self.pending.clear();
                        self.handshake = None;
                        self.permissions.clear();
                        self.setup = None;
                        self.configured = false;
                        self.epoch += 1;
                        self.shutdown_deadline = None;
                        self.stuck_reported = false;
                        if self.closing {
                            self.emit(EventKind::ShutdownComplete);
                            break;
                        }
                        if !self.timeout_reported {
                            let error = if diagnostics.is_empty() {
                                format!("{error} Agent exited: {status}")
                            } else {
                                format!("{error} Agent exited: {status}. {diagnostics}")
                            };
                            let kind = classify_error(&error, None);
                            if missing_initialize
                                && kind == FailureKind::Temporary
                                && self.config.harness == harness::OMP
                            {
                                self.fail(
                                    format!(
                                        "{error} The configured OMP Arguments must include `acp`."
                                    ),
                                    FailureKind::Configuration,
                                );
                            } else {
                                self.fail(error, kind);
                            }
                        }
                    }
                    Ok(Some(_)) => {
                        // Wait for the stdout reader to drain final protocol messages.
                        self.shutdown_deadline
                            .get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
                    }
                    Err(error) => {
                        self.stdin = None;
                        self.emit(EventKind::Error {
                            message: format!("Checking agent process failed: {error}"),
                            kind: FailureKind::Temporary,
                        });
                        self.shutdown_deadline
                            .get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
                    }
                    Ok(None) => (),
                }
            } else if self.closing {
                self.emit(EventKind::ShutdownComplete);
                break;
            }
            if self
                .shutdown_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
                && !self.stuck_reported
            {
                self.stuck_reported = true;
                self.emit(EventKind::ShutdownStuck);
            }
            if self
                .handshake
                .is_some_and(|(_, _, deadline)| Instant::now() >= deadline)
            {
                self.handshake_timeout();
            }
            if !self.blocked
                && !self.closing
                && (self.child.is_none() || self.stdin.is_some())
                && self
                    .active
                    .as_ref()
                    .is_some_and(|turn| turn.retry_at.is_some_and(|when| Instant::now() >= when))
            {
                self.retry_now();
            }
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Prompt { text, retries } => {
                if self.blocked {
                    self.emit(EventKind::StorageError(
                        "Save conversation history before prompting.".into(),
                    ));
                } else if self.closing || self.active.is_some() {
                    self.emit(EventKind::Error {
                        message: "This conversation is already processing or shutting down.".into(),
                        kind: FailureKind::Configuration,
                    });
                } else if self.restore_required {
                    self.emit(EventKind::ReplacementRequired("The saved session could not be restored. Confirm a replacement session before prompting.".into()));
                } else if self.child.is_some() && self.stdin.is_none() {
                    self.emit(EventKind::Error {
                        message: "Agent communication stopped. Wait for shutdown or use Force Stop before retrying.".into(),
                        kind: FailureKind::Temporary,
                    });
                } else if !text.trim().is_empty() {
                    self.turn += 1;
                    let text = if let Some(context) = self.pending_context.take() {
                        format!(
                            "Saved conversation context (prior history; do not repeat completed tool actions):\n{context}\n\nNew user request:\n{text}"
                        )
                    } else {
                        text
                    };
                    self.active = Some(Turn {
                        text,
                        retries,
                        attempt: 0,
                        worked: false,
                        denied: false,
                        observed_text: String::new(),
                        cancelled: false,
                        prompt_request: None,
                        retry_at: None,
                    });
                    if self.child.is_none() {
                        self.start();
                    } else if self.configured {
                        self.send_prompt();
                    } else if self.pending.is_empty() && self.setup.is_none() {
                        if self.capabilities.is_null() {
                            self.initialize();
                        } else {
                            self.configure(Request::Model);
                        }
                    }
                }
            }
            Command::Cancel => self.cancel(),
            Command::Permission {
                request_id,
                option_id,
            } => self.permission(request_id, &option_id),
            Command::SetPermissionMode(mode) => self.permission_mode = mode,
            Command::SetRetries(retries) => {
                if let Some(turn) = &mut self.active {
                    turn.retries = retries;
                }
            }
            Command::SetOption { kind, value } => self.switch(kind, &value),
            Command::ReplaceSession { context } => {
                if !self.blocked && !self.closing && self.stdin.is_none() && self.child.is_some() {
                    self.emit(EventKind::Error {
                        message: "Agent communication stopped. Use Force Stop before starting a replacement session.".into(),
                        kind: FailureKind::Temporary,
                    });
                } else if !self.blocked
                    && !self.closing
                    && self
                        .active
                        .as_ref()
                        .is_none_or(|turn| turn.prompt_request.is_none())
                {
                    self.replacement = Some(context);
                    self.replacing = true;
                    self.previous_session = self.session_id.take();
                    self.restore_required = false;
                    if self.child.is_none() {
                        self.start();
                    } else {
                        self.setup_session();
                    }
                }
            }
            Command::ResumeStorage => {
                if self.record("lifecycle", &json!({"storage_resumed":true})) {
                    self.blocked = false;
                    let mut pending = std::mem::take(&mut self.unprocessed).into_iter();
                    while let Some((message, saved)) = pending.next() {
                        if !saved && !self.record("incoming", &message) {
                            self.unprocessed.push((message, true));
                            self.unprocessed.extend(pending);
                            break;
                        }
                        self.incoming(&message);
                        if self.blocked {
                            self.unprocessed.extend(pending);
                            break;
                        }
                    }
                    if self.active.as_ref().is_some_and(|turn| {
                        turn.cancelled && (turn.prompt_request.is_none() || self.child.is_none())
                    }) {
                        self.active = None;
                        self.emit(EventKind::Stopped);
                    }
                }
            }
            Command::Shutdown => self.shutdown(),
            Command::ForceStop => {
                self.closing = true;
                self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
                self.active = None;
                self.cancel_permissions();
                if let Some(child) = &mut self.child {
                    match child.kill() {
                        Ok(()) => self.forced = true,
                        Err(error) => {
                            self.emit(EventKind::Error {
                                message: format!("Force stopping agent failed: {error}"),
                                kind: FailureKind::Temporary,
                            });
                        }
                    }
                }
            }
        }
    }

    fn start(&mut self) {
        if self.blocked || self.closing {
            return;
        }
        if !self.config.directory.is_absolute() || !self.config.directory.is_dir() {
            self.fail("The saved working directory is missing or not absolute. Restore it or change the project for a new conversation.".into(), FailureKind::Configuration);
            return;
        }
        let command_path = std::path::Path::new(&self.config.command);
        let existing_path =
            command_path.is_file() || self.config.directory.join(command_path).is_file();
        if self.config.command.trim().is_empty()
            || (self.config.command.split_whitespace().count() != 1 && !existing_path)
        {
            self.fail("Agent command must name one executable; edit the agent definition for new conversations.".into(), FailureKind::Configuration);
            return;
        }
        let instructions =
            harness::supports_instructions(&self.config.harness, &self.config.identity);
        if instructions
            && self.config.arguments.iter().any(|arg| {
                arg == "--system-prompt"
                    || arg.starts_with("--system-prompt=")
                    || arg == "--system-prompt-template"
                    || arg.starts_with("--system-prompt-template=")
            })
        {
            self.fail("Agent arguments replace OMP's default prompt. Remove system-prompt override to keep OMP defaults.".into(), FailureKind::Configuration);
            return;
        }
        let mut process = ProcessCommand::new(&self.config.command);
        process
            .args(&self.config.arguments)
            .current_dir(&self.config.directory);
        if instructions {
            let flag = match self.config.instructions_mode {
                InstructionsMode::Append => "--append-system-prompt",
                InstructionsMode::Overwrite => "--system-prompt",
            };
            // A trailing newline forces OMP's literal-text route, not its single-line file lookup.
            process.arg(format!(
                "{flag}={}\n",
                harness::guidance(&self.config.name, &self.config.system_instructions)
            ));
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x08000000); // CREATE_NO_WINDOW: ACP uses pipes.
        }
        match block_in_place(|| crate::platform::spawn_piped(&mut process, true)) {
            Ok((child, stdin, stdout, stderr)) => {
                self.configured = false;
                self.capabilities = Value::Null;
                self.options.clear();
                self.handshake = None;
                self.timeout_reported = false;
                self.lost_during_initialize = false;
                self.epoch += 1;
                let epoch = self.epoch;
                let sender = self.sender.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    loop {
                        match lines.next_line().await {
                            Ok(None) => {
                                let _ = sender.send(Input::Lost {
                                    epoch,
                                    error: "Agent closed protocol output.".into(),
                                });
                                break;
                            }
                            Ok(Some(line)) => match serde_json::from_str(&line) {
                                Ok(message) => {
                                    if sender.send(Input::Wire { epoch, message }).is_err() {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    let _ = sender.send(Input::Lost {
                                        epoch,
                                        error: format!("Invalid ACP JSON: {error}"),
                                    });
                                    break;
                                }
                            },
                            Err(error) => {
                                let _ = sender.send(Input::Lost {
                                    epoch,
                                    error: format!("Reading ACP output failed: {error}"),
                                });
                                break;
                            }
                        }
                    }
                });
                // stderr is diagnostic output, never ACP traffic.
                self.diagnostics.clear();
                if let Some(stderr) = stderr {
                    let sender = self.sender.clone();
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(stderr).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let line = if line.len() > 1024 {
                                line.chars().take(1024).collect()
                            } else {
                                line
                            };
                            if sender.send(Input::Diagnostic { epoch, line }).is_err() {
                                break;
                            }
                        }
                    });
                }
                self.stdin = Some(stdin);
                self.child = Some(child);
                self.initialize();
            }
            Err(error) => self.fail(
                format!("Starting {} failed: {error}", self.config.command),
                FailureKind::Configuration,
            ),
        }
    }
    fn initialize(&mut self) {
        self.request("initialize", json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"adeline","title":"Adeline","version":env!("CARGO_PKG_VERSION")}}), Request::Initialize);
    }

    fn handshake_timeout(&mut self) {
        let Some((id, request, _)) = self.handshake.take() else {
            return;
        };
        self.pending.remove(&id);
        self.setup = None;
        let (message, kind) = if request == Request::Initialize {
            ("Agent did not respond to ACP initialize. Check that its configured Arguments include `acp` and that the command supports ACP.".to_owned(), FailureKind::Configuration)
        } else {
            (
                format!(
                    "Agent did not respond to ACP {request:?} within 30 seconds. Check the harness and provider connection."
                ),
                FailureKind::Temporary,
            )
        };
        self.timeout_reported = true;
        self.stdin = None;
        self.lost_pending = Some(message.clone());
        self.lost_at = Some(Instant::now());
        self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
        self.fail(message, kind);
    }

    fn write_wire(&mut self, value: &Value, even_if_unsaved: bool) -> bool {
        if self.stdin.is_none() {
            return false;
        }
        let saved = self.record("outgoing", value);
        if !saved && !even_if_unsaved {
            return false;
        }
        let Some(stdin) = &mut self.stdin else {
            return false;
        };
        if let Err(error) =
            block_in_place(|| writeln!(stdin, "{value}").and_then(|()| stdin.flush()))
        {
            self.lost(format!("Writing ACP request failed: {error}"));
            return false;
        }
        true
    }

    fn request(&mut self, method: &str, params: Value, request: Request) -> Option<u64> {
        self.next_request += 1;
        let id = self.next_request;
        let mut message = json!({"jsonrpc":"2.0","id":id,"method":method});
        message["params"] = params;
        if self.write_wire(&message, false) {
            self.pending.insert(id, request);
            if request != Request::Prompt && request != Request::Close {
                self.handshake = Some((id, request, Instant::now() + Duration::from_secs(30)));
            }
            Some(id)
        } else {
            None
        }
    }

    fn setup_session(&mut self) {
        if self.blocked {
            return;
        }
        self.configured = false;
        let cwd = self.config.directory.to_string_lossy().into_owned();
        let (method, params) = if let Some(id) = &self.session_id {
            if self
                .capabilities
                .pointer("/sessionCapabilities/resume")
                .is_some_and(|capability| !capability.is_null())
            {
                (
                    "session/resume",
                    json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}),
                )
            } else if self.capabilities.get("loadSession") == Some(&Value::Bool(true)) {
                (
                    "session/load",
                    json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}),
                )
            } else {
                self.restore_failed("The agent does not support restoring sessions.");
                return;
            }
        } else {
            ("session/new", json!({"cwd":cwd,"mcpServers":[]}))
        };
        if self.request(method, params, Request::Setup).is_some() {
            self.setup = Some(Request::Setup);
        }
    }

    fn incoming(&mut self, message: &Value) {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            match method {
                "session/update" => self.update(message),
                "session/request_permission" => self.permission_request(message),
                _ => {
                    if let Some(id) = message.get("id") {
                        let response = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("Unsupported client method: {method}")}});
                        self.write_wire(&response, false);
                    }
                }
            }
            return;
        }
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return;
        };
        let Some(request) = self.pending.remove(&id) else {
            return;
        };
        if self
            .handshake
            .is_some_and(|(expected, _, _)| expected == id)
        {
            self.handshake = None;
        }
        if let Some(error) = message.get("error") {
            let text = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Unknown ACP error");
            let kind = if request == Request::Prompt
                && self.active.as_ref().is_some_and(|turn| turn.denied)
            {
                FailureKind::Denied
            } else {
                classify_error(text, error.get("code").and_then(Value::as_i64))
            };
            if request == Request::Setup
                && self.session_id.is_some()
                && (error.get("code").and_then(Value::as_i64) == Some(-32601)
                    || [
                        "session not found",
                        "unknown session",
                        "no such session",
                        "cannot resume session",
                        "cannot load session",
                    ]
                    .iter()
                    .any(|needle| contains_ascii(text, needle)))
            {
                self.setup = None;
                self.restore_failed(&format!("Cannot restore the saved agent session: {text}"));
            } else if request == Request::Close {
                self.emit(EventKind::Error {
                    message: format!("Closing the agent session failed: {text}"),
                    kind,
                });
                self.shutdown_deadline
                    .get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
            } else {
                if request == Request::Setup {
                    self.setup = None;
                }
                self.fail(format!("ACP {request:?} failed: {text}"), kind);
            }
            return;
        }
        let result = message.get("result").unwrap_or(&Value::Null);
        match request {
            Request::Initialize => {
                if result.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
                    self.fail(
                        "Agent uses an unsupported ACP protocol version (Adeline supports v1)."
                            .into(),
                        FailureKind::Configuration,
                    );
                    return;
                }
                self.capabilities = result
                    .get("agentCapabilities")
                    .cloned()
                    .unwrap_or(Value::Null);
                if self.closing {
                    self.stdin = None;
                    return;
                }
                self.setup_session();
            }
            Request::Setup => {
                self.restore_required = false;
                self.setup = None;
                let session_id = if let Some(id) = &self.session_id {
                    id.clone()
                } else if let Some(id) = result.get("sessionId").and_then(Value::as_str) {
                    id.to_owned()
                } else {
                    self.fail(
                        "ACP session/new returned no session ID.".into(),
                        FailureKind::Configuration,
                    );
                    return;
                };
                let was_replacement = self.replacing;
                self.session_id = Some(session_id.clone());
                self.options = result
                    .get("configOptions")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let previous_session = self.previous_session.take();
                if !self.record("session", &json!({"session_id":session_id,"replaced":was_replacement,"old_session_id":previous_session})) { return; }
                self.emit(EventKind::Session {
                    session_id,
                    replaced: was_replacement,
                });
                self.emit(EventKind::Options(self.options.clone()));
                if self.closing {
                    self.close_session();
                    return;
                }
                self.configure(Request::Model);
            }
            Request::Model | Request::Effort | Request::Switch => {
                if let Some(options) = result.get("configOptions").and_then(Value::as_array) {
                    self.options.clone_from(options);
                    self.emit(EventKind::Options(self.options.clone()));
                }
                if self.closing {
                    self.close_session();
                    return;
                }
                match request {
                    Request::Model => self.configure(Request::Effort),
                    Request::Effort => self.configured(),
                    _ => {}
                }
            }
            Request::Prompt => self.prompt_result(result),
            Request::Close => {
                self.stdin = None;
                self.shutdown_deadline
                    .get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
            }
        }
    }

    fn configured(&mut self) {
        self.configured = true;
        if self.replacing {
            self.replacing = false;
            let context = self.replacement.take().unwrap_or_default();
            if !context.trim().is_empty() {
                if let Some(turn) = &mut self.active {
                    turn.text = format!(
                        "Saved conversation context follows. Treat it as history, not a new user request. Do not repeat completed tool actions.\n\n{context}\n\nContinue the interrupted turn from where it stopped; do not repeat completed work."
                    );
                    turn.attempt = 0;
                } else {
                    self.pending_context = Some(context);
                }
            }
        }
        if self.active.is_some() {
            self.send_prompt();
        }
    }

    /// Applies the saved model (`Request::Model`) or effort (`Request::Effort`),
    /// then moves on. An empty value keeps the harness's default. The model is
    /// always set, because some harnesses (Codex) offer effort only after that.
    fn configure(&mut self, request: Request) {
        let (kind, value, label) = if request == Request::Model {
            (Kind::Model, self.config.model.clone(), "model")
        } else {
            (Kind::Effort, self.config.effort.clone(), "effort")
        };
        let next = |worker: &mut Self| {
            if request == Request::Model {
                worker.configure(Request::Effort);
            } else {
                worker.configured();
            }
        };
        if value.is_empty() {
            next(self);
            return;
        }
        let Some(setting) = harness::setting(&self.options, kind) else {
            self.fail(format!("Agent does not expose required {label} configuration; saved settings cannot be honored. Authenticate with the configured harness or choose one that offers this option."), FailureKind::Configuration);
            return;
        };
        if !setting.offers(&value) {
            self.fail(format!("Agent does not offer saved {label} value '{value}'. Choose a supported setting for this conversation or in the agent definition."), FailureKind::Configuration);
            return;
        }
        if request == Request::Effort && setting.current == value {
            next(self);
            return;
        }
        let Some(session_id) = &self.session_id else {
            return;
        };
        self.request(
            "session/set_config_option",
            json!({"sessionId":session_id,"configId":setting.id,"value":value}),
            request,
        );
    }

    fn switch(&mut self, kind: Kind, value: &str) {
        match kind {
            Kind::Model => value.clone_into(&mut self.config.model),
            Kind::Effort => value.clone_into(&mut self.config.effort),
        }
        // Without a ready, idle session the next setup applies the new value.
        if !self.configured || self.active.is_some() || self.stdin.is_none() {
            return;
        }
        let Some(setting) = harness::setting(&self.options, kind) else {
            return;
        };
        if let Some(session_id) = &self.session_id
            && setting.offers(value)
        {
            self.request(
                "session/set_config_option",
                json!({"sessionId":session_id,"configId":setting.id,"value":value}),
                Request::Switch,
            );
        }
    }

    fn send_prompt(&mut self) {
        if self.blocked || self.closing || !self.configured {
            return;
        }
        let Some(session_id) = &self.session_id else {
            return;
        };
        let Some(turn) = &self.active else {
            return;
        };
        if turn.cancelled
            || turn.prompt_request.is_some()
            || turn.retry_at.is_some_and(|when| Instant::now() < when)
        {
            return;
        }
        let text = turn.text.clone();
        let request = self.request(
            "session/prompt",
            json!({"sessionId":session_id,"prompt":[{"type":"text","text":text}]}),
            Request::Prompt,
        );
        if let Some(turn) = &mut self.active {
            turn.prompt_request = request;
            turn.retry_at = None;
        }
    }

    fn update(&mut self, message: &Value) {
        let params = &message["params"];
        if params.get("sessionId").and_then(Value::as_str) != self.session_id.as_deref() {
            return;
        }
        let update = &params["update"];
        // Context usage describes the session, so it counts between turns too.
        if update.get("sessionUpdate").and_then(Value::as_str) == Some("usage_update") {
            if let (Some(used), Some(size)) = (
                update.get("used").and_then(Value::as_u64),
                update.get("size").and_then(Value::as_u64),
            ) && self.setup.is_none()
                && self.record("visible_usage", &json!({"used":used,"size":size}))
            {
                self.emit(EventKind::Usage { used, size });
            }
            return;
        }
        if self.setup.is_some()
            || self
                .active
                .as_ref()
                .is_none_or(|turn| turn.cancelled || turn.prompt_request.is_none())
        {
            return;
        }
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    if let Some(turn) = &mut self.active {
                        turn.worked = true;
                        if turn.observed_text.len() < 2048 {
                            turn.observed_text.push_str(text);
                        }
                    }
                    if self.record("visible_text", &json!({"turn":self.turn,"text":text})) {
                        self.emit(EventKind::Text(text.to_owned()));
                    }
                }
            }
            Some("tool_call" | "tool_call_update") => {
                if let Some(turn) = &mut self.active {
                    turn.worked = true;
                }
                let id = string(update, "toolCallId");
                let entry = self.tools.entry(id.clone()).or_default();
                if let Some(title) = update.get("title").and_then(Value::as_str) {
                    entry.title = title.into();
                }
                if let Some(status) = update.get("status").and_then(Value::as_str) {
                    entry.status = status.into();
                }
                let detail = tool_detail(update);
                if !detail.is_empty() {
                    entry.detail = detail;
                }
                if let Some(kind) = update.get("kind").and_then(Value::as_str) {
                    entry.kind = kind.into();
                }
                if let Some(locations) = update.get("locations").and_then(Value::as_array) {
                    entry.paths = locations
                        .iter()
                        .filter_map(|location| location.get("path")?.as_str().map(str::to_owned))
                        .collect();
                }
                let ToolState {
                    title,
                    status,
                    detail,
                    kind,
                    paths,
                } = entry.clone();
                if self.record(
                    "visible_tool",
                    &json!({"turn":self.turn,"id":id,"title":title,"status":status,"detail":detail,"kind":kind,"paths":paths}),
                ) {
                    self.emit(EventKind::Tool {
                        id,
                        title,
                        status,
                        detail,
                        kind,
                        paths,
                    });
                }
            }
            _ => (), // Reasoning and replay are retained in raw history, not displayed.
        }
    }

    fn permission_request(&mut self, message: &Value) {
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return;
        };
        let params = &message["params"];
        if params.get("sessionId").and_then(Value::as_str) != self.session_id.as_deref()
            || self
                .active
                .as_ref()
                .is_none_or(|turn| turn.cancelled || turn.prompt_request.is_none())
        {
            self.permission_outcome(id, None);
            return;
        }
        let options: Vec<_> = params["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| {
                let kind = option.get("kind")?.as_str()?;
                if !matches!(kind, "allow_once" | "allow_always" | "reject_once") {
                    return None;
                }
                Some(PermissionChoice {
                    option_id: option.get("optionId")?.as_str()?.to_owned(),
                    name: string(option, "name"),
                    kind: kind.to_owned(),
                })
            })
            .collect();
        let read_only = matches!(params["toolCall"]["kind"].as_str(), Some("read" | "search"));
        if self.permission_mode == PermissionMode::AllowEverything
            || (self.permission_mode == PermissionMode::AllowReads && read_only)
        {
            if let Some(option) = options
                .iter()
                .find(|option| option.kind == "allow_once")
                .or_else(|| options.iter().find(|option| option.kind == "allow_always"))
            {
                self.permission_outcome(id, Some(&option.option_id));
            } else {
                self.permission_outcome(id, None);
                if let Some(turn) = &mut self.active {
                    turn.denied = true;
                }
            }
        } else if options.is_empty() {
            self.permission_outcome(id, None);
            if let Some(turn) = &mut self.active {
                turn.denied = true;
            }
        } else {
            let tool = &params["toolCall"];
            let mut title = tool
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Agent requests permission")
                .to_owned();
            let detail = tool_detail(tool);
            if !detail.is_empty() {
                title.push('\n');
                title.push_str(&detail);
            }
            if let Some(input) = tool.get("rawInput").filter(|input| !input.is_null()) {
                use std::fmt::Write as _;
                let _ = write!(title, "\nInput: {input}");
            }
            self.permissions.insert(id, (self.turn, options.clone()));
            self.emit(EventKind::Permission {
                request_id: id,
                title,
                options,
            });
        }
    }

    fn permission(&mut self, id: u64, option_id: &str) {
        let Some((turn, choices)) = self.permissions.remove(&id) else {
            return;
        };
        if turn != self.turn || self.active.as_ref().is_none_or(|active| active.cancelled) {
            self.permission_outcome(id, None);
            return;
        }
        if let Some(option) = choices.iter().find(|option| option.option_id == option_id) {
            let denied = option.kind == "reject_once";
            self.permission_outcome(id, Some(option_id));
            if denied && let Some(active) = &mut self.active {
                active.denied = true;
            }
        } else {
            self.permission_outcome(id, None);
        }
    }

    fn permission_outcome(&mut self, id: u64, selected: Option<&str>) {
        let outcome = selected.map_or(
            json!({"outcome":"cancelled"}),
            |option_id| json!({"outcome":"selected","optionId":option_id}),
        );
        self.write_wire(
            &json!({"jsonrpc":"2.0","id":id,"result":{"outcome":outcome}}),
            self.blocked,
        );
    }

    fn cancel_permissions(&mut self) {
        let ids: Vec<_> = self.permissions.keys().copied().collect();
        self.permissions.clear();
        for id in ids {
            self.permission_outcome(id, None);
        }
    }

    fn cancel(&mut self) {
        self.cancel_permissions();
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.cancelled = true;
        turn.retry_at = None;
        let request_id = turn.prompt_request;
        if request_id.is_some() {
            if let Some(session_id) = &self.session_id {
                self.write_wire(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session_id}}), self.blocked);
            }
        } else {
            self.active = None;
            self.emit(EventKind::Stopped);
        }
    }

    fn prompt_result(&mut self, result: &Value) {
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.prompt_request = None;
        if turn.cancelled {
            self.active = None;
            self.emit(EventKind::Stopped);
            if self.closing {
                self.close_session();
            }
            return;
        }
        let reason = result
            .get("stopReason")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if reason == "cancelled" {
            self.active = None;
            self.emit(EventKind::Stopped);
        } else if self.active.as_ref().is_some_and(|turn| turn.denied) {
            self.fail("Permission denied by the user.".into(), FailureKind::Denied);
        } else if reason == "end_turn"
            && self
                .active
                .as_ref()
                .is_some_and(|turn| looks_like_harness_error(&turn.observed_text))
        {
            let error = self
                .active
                .as_ref()
                .map_or(String::new(), |turn| turn.observed_text.clone());
            // The model can write this text too, so report it without retrying.
            if let Some(turn) = &mut self.active {
                turn.retries = turn.attempt;
            }
            self.fail(error.clone(), classify_error(&error, None));
        } else if matches!(reason, "end_turn" | "max_tokens" | "refusal") {
            self.active = None;
            self.emit(EventKind::Finished {
                stop_reason: reason.into(),
            });
        } else {
            self.fail(
                format!("Agent stopped the prompt: {reason}"),
                FailureKind::Temporary,
            );
        }
        if self.closing {
            self.close_session();
        }
    }

    fn fail(&mut self, message: String, kind: FailureKind) {
        let message = if kind == FailureKind::Authentication {
            format!("{message} Authenticate through the harness outside Adeline, then retry.")
        } else {
            message
        };
        self.record(
            "lifecycle",
            &json!({"error":message,"kind":format!("{kind:?}"),"turn":self.turn}),
        );
        if self.blocked {
            return;
        }
        if kind == FailureKind::Temporary
            && !self.closing
            && let Some(turn) = &mut self.active
            && !turn.cancelled
            && !turn.denied
            && turn.attempt < turn.retries
        {
            turn.attempt += 1;
            let attempt = turn.attempt;
            let limit = turn.retries;
            turn.prompt_request = None;
            turn.retry_at = Some(Instant::now() + Duration::from_secs(u64::from(attempt.min(5))));
            self.emit(EventKind::Retrying {
                attempt,
                limit,
                error: message,
            });
            return;
        }
        self.emit(EventKind::Error { message, kind });
        self.active = None;
    }

    fn retry_now(&mut self) {
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.retry_at = None;
        if turn.worked {
            turn.text = continue_interrupted_turn().into();
            if self.child.is_none() {
                self.start();
            } else {
                self.setup_session();
            }
        } else if self.child.is_none() {
            self.start();
        } else if !self.configured {
            if self.capabilities.is_null() {
                self.initialize();
            } else {
                self.setup_session();
            }
        } else {
            self.send_prompt();
        }
    }

    fn restore_failed(&mut self, reason: &str) {
        self.record(
            "lifecycle",
            &json!({"restore_failed":reason,"turn":self.turn}),
        );
        self.restore_required = true;
        self.emit(EventKind::ReplacementRequired(format!("{reason} You can start a replacement session with saved conversation content after confirming.")));
        if let Some(turn) = &mut self.active {
            turn.prompt_request = None;
            turn.retry_at = None;
        }
    }

    fn lost(&mut self, message: String) {
        let invalid_initialize = !self.closing
            && (message.starts_with("Invalid ACP JSON")
                || message.starts_with("Reading ACP output failed"))
            && self
                .pending
                .values()
                .any(|request| *request == Request::Initialize);
        self.lost_during_initialize = self
            .pending
            .values()
            .any(|request| *request == Request::Initialize);
        self.stdin = None;
        self.pending.clear();
        self.permissions.clear();
        self.setup = None;
        self.configured = false;
        self.handshake = None;
        self.lost_pending = Some(message.clone());
        self.lost_at = Some(Instant::now());
        self.shutdown_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
        if invalid_initialize && self.config.harness == harness::OMP {
            self.timeout_reported = true;
            self.fail(
                format!("{message} The configured OMP Arguments must include `acp`."),
                FailureKind::Configuration,
            );
        } else if invalid_initialize {
            self.timeout_reported = true;
            self.fail(message, FailureKind::Configuration);
        } else if self.child.is_none() && !self.closing {
            self.fail(message, FailureKind::Temporary);
        }
    }

    fn shutdown(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.replacement = None;
        self.cancel();
        self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
        if self.active.is_none() {
            self.close_session();
        }
    }

    fn close_session(&mut self) {
        if self
            .pending
            .values()
            .any(|request| *request == Request::Close)
        {
            return;
        }
        if let Some(session_id) = &self.session_id
            && self
                .capabilities
                .pointer("/sessionCapabilities/close")
                .is_some_and(|capability| !capability.is_null())
            && self.stdin.is_some()
        {
            self.request(
                "session/close",
                json!({"sessionId":session_id}),
                Request::Close,
            );
            return;
        }
        self.stdin = None;
    }
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn tool_detail(update: &Value) -> String {
    if let Some(raw) = update.get("rawOutput").filter(|value| !value.is_null()) {
        return raw.as_str().map_or_else(|| raw.to_string(), str::to_owned);
    }
    let mut detail = String::new();
    for item in update
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let text = item
            .pointer("/content/text")
            .and_then(Value::as_str)
            .or_else(|| item.get("newText").and_then(Value::as_str));
        if let Some(text) = text {
            if !detail.is_empty() {
                detail.push('\n');
            }
            if let Some(path) = item.get("path").and_then(Value::as_str) {
                detail.push_str(path);
                detail.push_str(":\n");
            }
            detail.push_str(text);
        }
    }
    detail
}

fn continue_interrupted_turn() -> &'static str {
    "Continue the interrupted turn from where you stopped. Do not repeat completed work or tool actions."
}

fn contains_ascii(text: &str, needle: &str) -> bool {
    text.as_bytes()
        .windows(needle.len())
        .any(|bytes| bytes.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Whether an ACP error message means the harness needs a login.
pub fn needs_login(message: &str) -> bool {
    classify_error(message, None) == FailureKind::Authentication
}

fn classify_error(message: &str, code: Option<i64>) -> FailureKind {
    if code == Some(401)
        || ["auth", "api key", "login", "unauthorized"]
            .iter()
            .any(|needle| contains_ascii(message, needle))
    {
        FailureKind::Authentication
    } else if contains_ascii(message, "permission denied")
        || contains_ascii(message, "denied by user")
    {
        FailureKind::Denied
    } else if matches!(code, Some(-32602..=-32600))
        || [
            "invalid model",
            "unsupported",
            "unknown acp",
            "unknown option",
            "unknown argument",
            "unrecognized option",
        ]
        .iter()
        .any(|needle| contains_ascii(message, needle))
    {
        FailureKind::Configuration
    } else {
        FailureKind::Temporary
    }
}

fn looks_like_harness_error(text: &str) -> bool {
    let text = text.trim_start();
    [
        "error:",
        "api error:",
        "authentication failed",
        "missing api key",
        "no api key",
        "authentication required",
        "not logged in",
        "you need to log in",
        "rate limit exceeded",
        "provider error:",
    ]
    .iter()
    .any(|prefix| {
        text.get(..prefix.len())
            .is_some_and(|begin| begin.eq_ignore_ascii_case(prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(record: Recorder) -> (Worker, async_channel::Receiver<Event>) {
        let (events, received) = async_channel::unbounded();
        let (sender, receiver) = mpsc::unbounded_channel();
        (
            Worker {
                id: "conversation-1".into(),
                config: ExecutionConfig {
                    version: crate::agents::VERSION,
                    name: "Josh".into(),
                    harness: harness::OMP.into(),
                    identity: String::new(),
                    command: "omp.exe".into(),
                    arguments: vec!["acp".into()],
                    model: "openai-codex/gpt-6-sol".into(),
                    effort: "high".into(),
                    system_instructions: String::new(),
                    instructions_mode: InstructionsMode::Append,
                    directory: std::env::current_dir().expect("cwd"),
                },
                session_id: Some("session-1".into()),
                permission_mode: PermissionMode::Ask,
                events,
                record,
                sender,
                receiver,
                child: None,
                stdin: None,
                epoch: 0,
                next_request: 0,
                pending: HashMap::new(),
                handshake: None,
                permissions: HashMap::new(),
                capabilities: Value::Null,
                options: Vec::new(),
                tools: HashMap::new(),
                setup: None,
                configured: true,
                restore_required: false,
                active: None,
                turn: 1,
                unprocessed: Vec::new(),
                blocked: false,
                closing: false,
                shutdown_deadline: None,
                stuck_reported: false,
                diagnostics: VecDeque::new(),
                lost_pending: None,
                lost_at: None,
                timeout_reported: false,
                lost_during_initialize: false,
                replacement: None,
                previous_session: None,
                pending_context: None,
                replacing: false,
                forced: false,
            },
            received,
        )
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "The protocol test runs on a test thread, not the UI thread."
    )]
    /// Runs `step` with a live stdin, so requests are written and recorded.
    fn sent_requests(worker: &mut Worker, step: impl FnOnce(&mut Worker)) {
        #[cfg(windows)]
        let mut process = {
            use std::os::windows::process::CommandExt as _;
            let mut process = ProcessCommand::new("powershell.exe");
            process.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$null = [Console]::In.ReadToEnd()",
            ]);
            process.creation_flags(0x0800_0000);
            process
        };
        #[cfg(not(windows))]
        let mut process = {
            let mut process = ProcessCommand::new("sh");
            process.args(["-c", "cat >/dev/null"]);
            process
        };
        let mut child = process.stdin(std::process::Stdio::piped()).spawn().unwrap();
        worker.stdin = child.stdin.take();
        step(worker);
        worker.stdin = None;
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn effort_offered_only_after_the_model_is_set_is_still_applied() {
        let (sent, received) = std::sync::mpsc::channel();
        let (mut worker, events) = worker(Arc::new(move |_, direction, message| {
            if direction == "outgoing" {
                sent.send(message.clone()).unwrap();
            }
            Ok(())
        }));
        worker.configured = false;
        worker.config.model = "gpt-6-astra".into();
        // Codex: the model is already current and effort is not offered yet.
        worker.options = vec![json!({"id":"model","category":"model","type":"select",
            "currentValue":"gpt-6-astra","options":[{"value":"gpt-6-astra"}]})];
        sent_requests(&mut worker, |worker| {
            worker.configure(Request::Model);
            let id = *worker.pending.keys().next().expect("model request");
            worker.incoming(&json!({"id":id,"result":{"configOptions":[
                {"id":"model","category":"model","type":"select","currentValue":"gpt-6-astra",
                 "options":[{"value":"gpt-6-astra"}]},
                {"id":"reasoning_effort","category":"thought_level","type":"select",
                 "currentValue":"medium","options":[{"value":"medium"},{"value":"high"}]}
            ]}}));
        });
        let requests: Vec<_> = received.try_iter().collect();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["params"]["configId"], "model");
        assert_eq!(
            requests[1]["params"],
            json!({"sessionId":"session-1","configId":"reasoning_effort","value":"high"})
        );
        assert!(std::iter::from_fn(|| events.try_recv().ok()).any(
            |event| matches!(event.kind, EventKind::Options(ref options) if options.len() == 2)
        ));
    }

    #[test]
    fn harness_without_model_or_effort_options_keeps_its_defaults() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.configured = false;
        worker.config.model.clear();
        worker.config.effort.clear();
        worker.configure(Request::Model);
        assert!(worker.configured);
        assert!(worker.pending.is_empty());
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn switching_while_idle_sets_the_option_and_while_stopped_waits_for_setup() {
        let (sent, received) = std::sync::mpsc::channel();
        let (mut worker, _) = worker(Arc::new(move |_, direction, message| {
            if direction == "outgoing" {
                sent.send(message.clone()).unwrap();
            }
            Ok(())
        }));
        worker.options = vec![json!({"id":"thinking","category":"thought_level",
            "options":[{"value":"low"},{"value":"high"}]})];
        sent_requests(&mut worker, |worker| {
            worker.command(Command::SetOption {
                kind: Kind::Effort,
                value: "low".into(),
            });
        });
        assert_eq!(worker.config.effort, "low");
        assert_eq!(received.try_recv().unwrap()["params"]["value"], "low");
        worker.command(Command::SetOption {
            kind: Kind::Model,
            value: "xai/grok".into(),
        });
        assert_eq!(worker.config.model, "xai/grok");
        assert!(received.try_recv().is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn non_omp_start_passes_only_configured_arguments() {
        let (mut worker, _) = worker(Arc::new(|_, _, _| Ok(())));
        worker.config.harness = "Other".into();
        #[cfg(windows)]
        {
            worker.config.command = "cmd.exe".into();
            worker.config.arguments = vec!["/C".into(), "echo".into(), "1".into()];
        }
        #[cfg(not(windows))]
        {
            worker.config.command = "sh".into();
            worker.config.arguments = vec![
                "-c".into(),
                "printf '%s\\n' \"$*\"".into(),
                "sh".into(),
                "1".into(),
            ];
        }
        worker.start();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), worker.receiver.recv()).await,
            Ok(Some(Input::Wire { message, .. })) if message == json!(1)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn driver_reports_an_agent_that_exits_at_once() {
        let (Worker { mut config, .. }, _) = worker(Arc::new(|_, _, _| Ok(())));
        config.harness = "Other".into();
        #[cfg(windows)]
        let (command, arguments) = ("cmd.exe", ["/C", "exit 1"]);
        #[cfg(not(windows))]
        let (command, arguments) = ("sh", ["-c", "exit 1"]);
        config.command = command.into();
        config.arguments = arguments.map(Into::into).to_vec();
        let (events, received) = async_channel::unbounded();
        let driver = Driver::spawn(
            "conversation-1".into(),
            config,
            None,
            PermissionMode::Ask,
            events,
            Arc::new(|_, _, _| Ok(())),
        );
        driver
            .send(Command::Prompt {
                text: "hello".into(),
                retries: 0,
            })
            .unwrap();
        let error = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let EventKind::Error { message, .. } = received.recv().await.unwrap().kind {
                    break message;
                }
            }
        })
        .await
        .unwrap();
        assert!(error.contains("Agent exited"), "{error}");
    }

    fn turn(retries: u32) -> Turn {
        Turn {
            text: "original user request".into(),
            retries,
            attempt: 0,
            worked: false,
            observed_text: String::new(),
            denied: false,
            cancelled: false,
            prompt_request: Some(1),
            retry_at: None,
        }
    }

    #[test]
    fn cancel_discards_pending_retry_and_permission() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        let mut pending = turn(5);
        pending.prompt_request = None;
        pending.retry_at = Some(Instant::now() + Duration::from_secs(30));
        worker.active = Some(pending);
        worker.permissions.insert(
            7,
            (
                1,
                vec![PermissionChoice {
                    option_id: "allow-once".into(),
                    name: "Allow once".into(),
                    kind: "allow_once".into(),
                }],
            ),
        );
        worker.cancel();
        assert!(worker.active.is_none());
        assert!(worker.permissions.is_empty());
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Stopped
        ));
    }

    #[test]
    fn denied_permission_settles_without_retry_even_when_retries_remain() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.active = Some(turn(5));
        worker.permissions.insert(
            7,
            (
                1,
                vec![PermissionChoice {
                    option_id: "deny-once".into(),
                    name: "Deny once".into(),
                    kind: "reject_once".into(),
                }],
            ),
        );
        worker.permission(7, "deny-once");
        assert!(worker.active.as_ref().unwrap().denied);
        worker.prompt_result(&json!({"stopReason":"end_turn"}));
        assert!(worker.active.is_none());
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Error {
                kind: FailureKind::Denied,
                ..
            }
        ));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn error_looking_reply_is_reported_without_retry() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        let mut active = turn(5);
        active.observed_text = "Error: Provider timeout".into();
        worker.active = Some(active);
        worker.prompt_result(&json!({"stopReason":"end_turn"}));
        assert!(worker.active.is_none());
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Error {
                kind: FailureKind::Temporary,
                ..
            }
        ));
    }

    #[test]
    fn temporary_failure_obeys_retry_limit_and_preserves_original_until_work() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.active = Some(turn(2));
        for expected in 1..=2 {
            worker.fail("Provider timed out".into(), FailureKind::Temporary);
            let active = worker.active.as_ref().unwrap();
            assert_eq!(active.attempt, expected);
            assert_eq!(active.text, "original user request");
            assert!(active.retry_at.is_some());
            assert!(
                matches!(events.try_recv().unwrap().kind, EventKind::Retrying { attempt, .. } if attempt == expected)
            );
        }
        worker.fail("Provider timed out".into(), FailureKind::Temporary);
        assert!(worker.active.is_none());
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Error {
                kind: FailureKind::Temporary,
                ..
            }
        ));
        assert_ne!(continue_interrupted_turn(), "original user request");
        assert!(continue_interrupted_turn().contains("Do not repeat completed work"));
    }

    #[test]
    fn changing_retries_affects_the_running_turn() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.active = Some(turn(0));
        worker.command(Command::SetRetries(2));
        worker.fail("Provider timed out".into(), FailureKind::Temporary);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Retrying {
                attempt: 1,
                limit: 2,
                ..
            }
        ));
    }

    #[test]
    fn unavailable_saved_model_fails_before_prompt() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.configured = false;
        worker.active = Some(turn(0));
        worker.options =
            vec![json!({"id":"model","category":"model","options":[{"value":"other/model"}]})];
        worker.send_prompt();
        assert!(worker.pending.is_empty());
        worker.configure(Request::Model);
        assert!(worker.active.is_none());
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Error {
                kind: FailureKind::Configuration,
                ..
            }
        ));
    }

    #[test]
    fn shutdown_during_setup_never_starts_configuration_or_prompt() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.session_id = None;
        worker.active = Some(turn(0));
        worker.active.as_mut().unwrap().prompt_request = None;
        worker.pending.insert(2, Request::Setup);
        worker.shutdown();
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Stopped
        ));
        worker.incoming(&json!({"id":2,"result":{"sessionId":"new","configOptions":[
            {"id":"model","options":[{"value":"openai-codex/gpt-6-sol"}]}
        ]}}));
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Session { .. }
        ));
        assert!(worker.pending.is_empty());
        assert!(worker.active.is_none());
    }

    #[test]
    fn restored_history_is_recorded_without_replaying_visible_chunks() {
        let (written, recorded) = std::sync::mpsc::channel();
        let (mut worker, events) = worker(Arc::new(move |_, direction, _| {
            written
                .send(direction.to_owned())
                .map_err(|error| error.to_string())
        }));
        worker.active = Some(turn(0));
        worker.setup = Some(Request::Setup);
        let chunk = json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"old text"}}}});
        assert!(worker.record("incoming", &chunk));
        worker.update(&chunk);
        assert!(events.try_recv().is_err());
        worker.setup = None;
        worker.update(&chunk);
        assert!(
            matches!(&events.try_recv().unwrap().kind, EventKind::Text(text) if text == "old text")
        );
        assert_eq!(
            recorded.try_iter().collect::<Vec<_>>(),
            vec!["incoming".to_owned(), "visible_text".to_owned()]
        );
    }

    #[test]
    fn tool_kind_and_paths_survive_updates_that_omit_them() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.active = Some(turn(0));
        let update = |update: Value| json!({"params":{"sessionId":"session-1","update":update}});
        worker.update(&update(json!({
            "sessionUpdate":"tool_call","toolCallId":"t1","title":"Read SKILL.md",
            "kind":"read","status":"pending","locations":[{"path":"/repo/SKILL.md","line":3}]
        })));
        worker.update(&update(json!({
            "sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"
        })));
        let mut last = None;
        while let Ok(event) = events.try_recv() {
            last = Some(event.kind);
        }
        let last = last.expect("tool events");
        assert!(matches!(
            last,
            EventKind::Tool { ref status, ref kind, ref paths, .. }
                if status == "completed" && kind == "read" && paths == &["/repo/SKILL.md"]
        ));
    }

    #[test]
    fn usage_is_reported_between_turns() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        assert!(worker.active.is_none());
        worker.update(&json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"usage_update","used":38_000,"size":200_000}}}));
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Usage {
                used: 38_000,
                size: 200_000
            }
        ));
    }

    #[test]
    fn handshake_timeout_reports_missing_acp_and_bounds_setup_wait() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.active = Some(turn(5));
        worker.pending.insert(3, Request::Initialize);
        worker.handshake = Some((3, Request::Initialize, Instant::now()));
        worker.handshake_timeout();
        assert!(worker.pending.is_empty());
        assert!(worker.active.is_none());
        assert!(matches!(events.try_recv().unwrap().kind, EventKind::Error {
            kind: FailureKind::Configuration, message
        } if message.contains("`acp`")));
        worker.active = Some(turn(1));
        worker.pending.insert(4, Request::Setup);
        worker.handshake = Some((4, Request::Setup, Instant::now()));
        worker.handshake_timeout();
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Retrying { attempt: 1, .. }
        ));
    }

    #[test]
    fn failed_storage_blocks_processing_and_cancels_active_turn() {
        let fail = std::sync::atomic::AtomicBool::new(true);
        let (mut worker, events) = worker(Arc::new(move |_, _, _| {
            if fail.swap(false, std::sync::atomic::Ordering::SeqCst) {
                Err("disk full".into())
            } else {
                Ok(())
            }
        }));
        worker.active = Some(turn(5));
        assert!(!worker.record("incoming", &json!({"method":"session/update"})));
        assert!(worker.blocked);
        assert!(worker.active.as_ref().unwrap().cancelled);
        worker.command(Command::Prompt {
            text: "must not run".into(),
            retries: 5,
        });
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::StorageError(_)
        ));
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::StorageError(_)
        ));
        assert_eq!(worker.turn, 1);
    }

    #[test]
    fn storage_recovery_replays_saved_response_before_next_prompt() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        let mut turn = turn(5);
        turn.cancelled = true;
        worker.active = Some(turn);
        worker.blocked = true;
        worker.pending.insert(1, Request::Prompt);
        worker.unprocessed.push((
            json!({"jsonrpc":"2.0","id":1,"result":{"stopReason":"cancelled"}}),
            false,
        ));
        worker.command(Command::ResumeStorage);
        assert!(!worker.blocked);
        assert!(worker.unprocessed.is_empty());
        assert!(worker.active.is_none());
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Stopped
        ));
    }

    #[test]
    fn unavailable_session_needs_consent_but_missing_auth_needs_login() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.pending.insert(1, Request::Setup);
        worker.active = Some(turn(0));
        worker.incoming(&json!({"id":1,"error":{"code":-32000,"message":"ACP session not found"}}));
        assert!(worker.restore_required);
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::ReplacementRequired(_)
        ));
        let (mut worker, events) = self::worker(Arc::new(|_, _, _| Ok(())));
        worker.pending.insert(1, Request::Setup);
        worker.active = Some(turn(5));
        worker.incoming(&json!({"id":1,"error":{"code":401,"message":"Authentication required"}}));
        assert!(!worker.restore_required);
        assert!(worker.active.is_none());
        assert!(matches!(events.try_recv().unwrap().kind, EventKind::Error {
            kind: FailureKind::Authentication, message
        } if message.contains("outside Adeline")));
    }

    #[test]
    fn permission_request_shows_harness_scope_without_permanent_deny() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.active = Some(turn(0));
        worker.permission_request(&json!({"id":7,"params":{"sessionId":"session-1",
        "toolCall":{"toolCallId":"tool-1","title":"Editing file","rawInput":{"path":"src/main.rs"}},
        "options":[
            {"optionId":"allow","name":"Allow once","kind":"allow_once"},
            {"optionId":"reject-forever","name":"Deny permanently","kind":"reject_always"}
        ]}}));
        let EventKind::Permission { title, options, .. } = events.try_recv().unwrap().kind else {
            panic!("permission request missing")
        };
        assert!(title.contains("src/main.rs"));
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].kind, "allow_once");
    }

    #[test]
    fn allow_reads_approves_reads_and_asks_for_edits() {
        let (mut worker, events) = worker(Arc::new(|_, _, _| Ok(())));
        worker.active = Some(turn(0));
        worker.permission_mode = PermissionMode::AllowReads;
        let request = |id, kind| {
            json!({"id":id,"params":{"sessionId":"session-1",
            "toolCall":{"toolCallId":"tool-1","title":"Tool","kind":kind},
            "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]}})
        };
        worker.permission_request(&request(7, "read"));
        assert!(events.try_recv().is_err());
        worker.permission_request(&request(8, "edit"));
        assert!(matches!(
            events.try_recv().unwrap().kind,
            EventKind::Permission { request_id: 8, .. }
        ));
    }

    #[test]
    fn failure_classification_avoids_retrying_auth_and_configuration() {
        assert_eq!(
            classify_error("Missing API key; run omp login", None),
            FailureKind::Authentication
        );
        assert_eq!(
            classify_error("Unknown argument --append-system-prompt", None),
            FailureKind::Configuration
        );
        assert_eq!(
            classify_error("Permission denied by user", None),
            FailureKind::Denied
        );
        assert_eq!(
            classify_error("Provider timeout", None),
            FailureKind::Temporary
        );
        assert!(looks_like_harness_error("Error: Provider timeout"));
        assert!(!looks_like_harness_error(
            "I can explain a provider timeout."
        ));
    }
}
