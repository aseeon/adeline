//! One ACP agent process per conversation, spoken to through the official
//! `agent-client-protocol` SDK and its typed v1 messages. This is the only
//! module that knows ACP: it translates the protocol to and from Adeline's
//! own model (`conversation.rs`). Probes and logins use the same client.
use crate::{
    agents::InstructionsMode,
    conversation::{
        AgentCommand, Attachment, AuthMethod, Category, Choice, Direction, Features, McpTransport,
        OptionKind, PermissionKind, PermissionOption, SessionOption, StepStatus, StopReason,
        TerminalLogin, TodoStep, ToolKind, ToolStatus, TrafficEntry, TrafficNote,
    },
    harness,
    profiles::{self, Instructions, Profile},
    storage::ExecutionConfig,
};
use agent_client_protocol::{
    self as sdk, Agent, ConnectionTo, Dispatch, Handled, Lines, Responder, UntypedMessage,
    schema::{MaybeUndefined, ProtocolVersion, v1 as wire},
};
use async_channel::Sender;
use futures::StreamExt as _;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    process::{Child, Command as ProcessCommand},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
    task::block_in_place,
};

/// The option ID Adeline gives an agent's session modes when it offers them
/// through `session/set_mode` rather than as a config option.
pub const MODE_OPTION: &str = "adeline/session-mode";

/// How many stderr lines an exit error quotes.
const STDERR_TAIL: usize = 12;

pub struct Driver {
    sender: UnboundedSender<Input>,
}

/// A part of a prompt.
#[derive(Clone, Debug)]
pub enum Part {
    Text(String),
    File(Attachment),
}

fn text_of(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            Part::File(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Puts `lead` before the prompt's text.
fn prefixed(parts: &[Part], lead: impl FnOnce(&str) -> String) -> Vec<Part> {
    let text = text_of(parts);
    let mut out = vec![Part::Text(lead(&text))];
    out.extend(
        parts
            .iter()
            .filter(|part| matches!(part, Part::File(_)))
            .cloned(),
    );
    out
}

#[derive(Debug)]
pub enum Command {
    Prompt {
        prompt: Vec<Part>,
        retries: u32,
    },
    /// Delivers a message into the running turn through the agent's steering.
    Steer {
        prompt: Vec<Part>,
    },
    Cancel,
    Permission {
        request_id: u64,
        option_id: String,
    },
    /// Changes the running turn's retry limit; later turns pass their own.
    SetRetries(u32),
    /// Sets an option; applied now when idle, else at the next setup.
    SetOption {
        category: Category,
        id: String,
        value: String,
    },
    ReplaceSession {
        context: String,
    },
    /// Gives a new fork's first session its copied history: a native fork of
    /// `session` when the agent offers one, else `context` as text.
    Fork {
        session: Option<String>,
        context: String,
    },
    /// Starts the agent if needed and asks it to log in with one of its methods.
    Authenticate {
        method: String,
    },
    Logout,
    /// Starts the agent, opens a session in its folder and reports what it offers.
    Probe,
    Shutdown,
    ForceStop,
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
    /// The agent answered `initialize`.
    Agent {
        name: String,
        version: String,
        features: Features,
    },
    Options(Vec<SessionOption>),
    Commands(Vec<AgentCommand>),
    Todo(Vec<TodoStep>),
    Title(String),
    Text {
        message: Option<String>,
        text: String,
    },
    Thought {
        message: Option<String>,
        text: String,
    },
    Tool {
        id: String,
        title: String,
        status: ToolStatus,
        detail: String,
        kind: ToolKind,
        paths: Vec<String>,
    },
    /// How much of the agent's context window the session uses, in tokens.
    Usage {
        used: u64,
        size: u64,
    },
    Permission {
        request_id: u64,
        title: String,
        options: Vec<PermissionOption>,
    },
    /// A Send now went through steering; `false` means the agent didn't take it.
    Steered {
        delivered: bool,
    },
    /// MCP servers left out because the agent doesn't support their transport.
    SkippedServers(Vec<String>),
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
        stop_reason: StopReason,
    },
    Stopped,
    ShutdownStuck,
    ShutdownComplete,
    ReplacementRequired(String),
    /// A fork's first prompt carries its history as text.
    TextCopy,
    Traffic(TrafficEntry),
    /// The agent asked Adeline to open a URL, such as a login page.
    OpenUrl(String),
    /// A login or logout finished.
    Auth(Result<(), String>),
    /// A probe's session is set up and configured.
    Probed,
    /// The agent's last stderr lines, just before the error its exit causes.
    Crashed(String),
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
        events: Sender<Event>,
    ) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let worker = Worker::new(
            conversation_id,
            config,
            session_id,
            events,
            sender.clone(),
            receiver,
        );
        tokio::spawn(worker.run());
        Self { sender }
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        self.sender
            .send(Input::Command(command))
            .map_err(|_| "Agent worker has stopped.".into())
    }
}

enum Input {
    Command(Command),
    Connected {
        epoch: u64,
        link: Link,
    },
    Reply {
        epoch: u64,
        id: u64,
        result: Result<Reply, sdk::Error>,
    },
    Update {
        epoch: u64,
        notification: Box<wire::SessionNotification>,
    },
    Permission {
        epoch: u64,
        request: Box<wire::RequestPermissionRequest>,
        responder: Responder<wire::RequestPermissionResponse>,
    },
    OpenUrl(String),
    Lost {
        epoch: u64,
        error: String,
    },
    Stderr {
        epoch: u64,
        line: String,
    },
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Input")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    Initialize,
    Setup,
    /// One step of applying the saved selections after setup.
    Config,
    /// A user's change while idle.
    Switch,
    Prompt,
    Steer,
    Close,
    Authenticate,
    Logout,
}

/// The requests Adeline sends, typed.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "each request is moved once, straight to the SDK"
)]
enum Outgoing {
    Initialize(wire::InitializeRequest),
    New(wire::NewSessionRequest),
    Load(wire::LoadSessionRequest),
    Resume(wire::ResumeSessionRequest),
    Fork(wire::ForkSessionRequest),
    SetConfig(wire::SetSessionConfigOptionRequest),
    SetMode(wire::SetSessionModeRequest),
    Prompt(wire::PromptRequest),
    /// The steering extension, which the schema has no type for.
    Steer(Value),
    Close(wire::CloseSessionRequest),
    Authenticate(wire::AuthenticateRequest),
    Logout(wire::LogoutRequest),
}

/// A session setup's answer, whichever request it was.
#[derive(Debug, Default)]
struct SessionReply {
    session_id: Option<String>,
    modes: Option<wire::SessionModeState>,
    options: Option<Vec<wire::SessionConfigOption>>,
}

#[derive(Debug)]
enum Reply {
    Initialize(Box<wire::InitializeResponse>),
    Session(SessionReply),
    Options(Vec<wire::SessionConfigOption>),
    Empty,
    Prompt(wire::StopReason),
    Steer(Value),
}

/// The live connection to an agent process.
enum Link {
    Sdk {
        connection: ConnectionTo<Agent>,
        /// Dropping or firing it ends the connection, which closes stdin.
        _close: oneshot::Sender<()>,
    },
    #[cfg(test)]
    Test(Arc<std::sync::Mutex<Vec<Outgoing>>>),
}

impl Link {
    fn send(&self, id: u64, epoch: u64, outgoing: Outgoing, inputs: &UnboundedSender<Input>) {
        let connection = match self {
            Self::Sdk { connection, .. } => connection,
            #[cfg(test)]
            Self::Test(sent) => {
                sent.lock().expect("test outbox").push(outgoing);
                return;
            }
        };
        macro_rules! forward {
            ($request:expr, $map:expr) => {{
                let sent = connection.send_request($request);
                let inputs = inputs.clone();
                tokio::spawn(async move {
                    let result = sent.block_task().await.map($map);
                    let _ = inputs.send(Input::Reply { epoch, id, result });
                });
            }};
        }
        match outgoing {
            Outgoing::Initialize(request) => {
                forward!(request, |reply| Reply::Initialize(Box::new(reply)));
            }
            Outgoing::New(request) => forward!(request, |reply| Reply::Session(SessionReply {
                session_id: Some(reply.session_id.0.to_string()),
                modes: reply.modes,
                options: reply.config_options,
            })),
            Outgoing::Load(request) => forward!(request, |reply| Reply::Session(SessionReply {
                session_id: None,
                modes: reply.modes,
                options: reply.config_options,
            })),
            Outgoing::Resume(request) => forward!(request, |reply| Reply::Session(SessionReply {
                session_id: None,
                modes: reply.modes,
                options: reply.config_options,
            })),
            Outgoing::Fork(request) => forward!(request, |reply| Reply::Session(SessionReply {
                session_id: Some(reply.session_id.0.to_string()),
                modes: reply.modes,
                options: reply.config_options,
            })),
            Outgoing::SetConfig(request) => {
                forward!(request, |reply| Reply::Options(reply.config_options));
            }
            Outgoing::SetMode(request) => forward!(request, |_| Reply::Empty),
            Outgoing::Prompt(request) => {
                forward!(request, |reply| Reply::Prompt(reply.stop_reason));
            }
            Outgoing::Steer(params) => match UntypedMessage::new("_session/steering", params) {
                Ok(request) => forward!(request, Reply::Steer),
                Err(error) => {
                    let _ = inputs.send(Input::Reply {
                        epoch,
                        id,
                        result: Err(error),
                    });
                }
            },
            Outgoing::Close(request) => forward!(request, |_| Reply::Empty),
            Outgoing::Authenticate(request) => forward!(request, |_| Reply::Empty),
            Outgoing::Logout(request) => forward!(request, |_| Reply::Empty),
        }
    }

    fn cancel(&self, session: &str) {
        match self {
            Self::Sdk { connection, .. } => {
                let _ =
                    connection.send_notification(wire::CancelNotification::new(session.to_owned()));
            }
            #[cfg(test)]
            Self::Test(_) => {}
        }
    }
}

/// The latest state of one tool call. Updates may omit fields they don't change.
#[derive(Clone, Default)]
struct ToolState {
    title: String,
    status: ToolStatus,
    detail: String,
    kind: ToolKind,
    paths: Vec<String>,
}

struct Turn {
    prompt: Vec<Part>,
    retries: u32,
    attempt: u32,
    worked: bool,
    observed_text: String,
    denied: bool,
    cancelled: bool,
    prompt_request: Option<u64>,
    retry_at: Option<Instant>,
}

impl Turn {
    fn new(prompt: Vec<Part>, retries: u32) -> Self {
        Self {
            prompt,
            retries,
            attempt: 0,
            worked: false,
            observed_text: String::new(),
            denied: false,
            cancelled: false,
            prompt_request: None,
            retry_at: None,
        }
    }
}

struct PendingPermission {
    turn: u64,
    options: Vec<PermissionOption>,
    responder: Option<Responder<wire::RequestPermissionResponse>>,
}

struct Worker {
    id: String,
    config: ExecutionConfig,
    profile: Option<&'static Profile>,
    session_id: Option<String>,
    events: Sender<Event>,
    sender: UnboundedSender<Input>,
    receiver: UnboundedReceiver<Input>,
    child: Option<Child>,
    link: Option<Link>,
    /// The process is running and its connection is still being set up.
    connecting: bool,
    epoch: u64,
    next_request: u64,
    next_permission: u64,
    pending: HashMap<u64, Request>,
    handshake: Option<(u64, Request, Instant)>,
    permissions: HashMap<u64, PendingPermission>,
    /// What the agent offered at `initialize`; `None` until it answered.
    features: Option<Features>,
    raw_options: Vec<wire::SessionConfigOption>,
    modes: Option<wire::SessionModeState>,
    options: Vec<SessionOption>,
    tools: HashMap<String, ToolState>,
    setup: Option<Request>,
    configured: bool,
    /// How many of the saved selections setup has applied.
    config_step: usize,
    restore_required: bool,
    active: Option<Turn>,
    turn: u64,
    closing: bool,
    shutdown_deadline: Option<Instant>,
    stuck_reported: bool,
    stderr: VecDeque<String>,
    timeout_reported: bool,
    lost_pending: Option<String>,
    lost_at: Option<Instant>,
    lost_during_initialize: bool,
    replacement: Option<String>,
    previous_session: Option<String>,
    pending_context: Option<String>,
    forced: bool,
    replacing: bool,
    /// A fork's source session and history, until its first session is set up.
    fork: Option<(Option<String>, String)>,
    /// The session being set up is a native fork.
    forking: bool,
    /// Reports the session's options once configured, then stops.
    probing: bool,
    /// A login (`Some(method)`) or logout (`None`) to send once the agent
    /// has answered `initialize`.
    #[expect(
        clippy::option_option,
        reason = "nothing pending, a logout, or a login"
    )]
    auth: Option<Option<String>>,
}

impl Worker {
    fn new(
        id: String,
        config: ExecutionConfig,
        session_id: Option<String>,
        events: Sender<Event>,
        sender: UnboundedSender<Input>,
        receiver: UnboundedReceiver<Input>,
    ) -> Self {
        Self {
            id,
            profile: profiles::find(&config.harness, &config.identity),
            config,
            session_id,
            events,
            sender,
            receiver,
            child: None,
            link: None,
            connecting: false,
            epoch: 0,
            next_request: 0,
            next_permission: 0,
            pending: HashMap::new(),
            handshake: None,
            permissions: HashMap::new(),
            features: None,
            raw_options: Vec::new(),
            modes: None,
            options: Vec::new(),
            tools: HashMap::new(),
            setup: None,
            configured: false,
            config_step: 0,
            restore_required: false,
            active: None,
            turn: 0,
            closing: false,
            shutdown_deadline: None,
            stuck_reported: false,
            stderr: VecDeque::new(),
            timeout_reported: false,
            lost_pending: None,
            lost_at: None,
            lost_during_initialize: false,
            replacement: None,
            previous_session: None,
            pending_context: None,
            forced: false,
            replacing: false,
            fork: None,
            forking: false,
            probing: false,
            auth: None,
        }
    }

    fn emit(&self, kind: EventKind) {
        let _ = self.events.try_send(Event {
            conversation_id: self.id.clone(),
            turn: self.turn,
            kind,
        });
    }

    /// The agent stopped answering: its process runs but the connection is gone.
    fn stopped_talking(&self) -> bool {
        self.child.is_some() && self.link.is_none() && !self.connecting
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
                Some(Some(input)) => self.input(input),
                Some(None) => self.shutdown(),
                None => {}
            }
            if self.tick() {
                break;
            }
        }
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::Command(command) => self.command(command),
            Input::Connected { epoch, link } if epoch == self.epoch => {
                self.connecting = false;
                self.link = Some(link);
                if self.closing && self.active.is_none() && !self.probing {
                    self.close_link();
                } else {
                    self.initialize();
                }
            }
            Input::Reply { epoch, id, result } if epoch == self.epoch => self.reply(id, result),
            Input::Update {
                epoch,
                notification,
            } if epoch == self.epoch => self.update(*notification),
            Input::Permission {
                epoch,
                request,
                responder,
            } => {
                if epoch == self.epoch {
                    self.permission_request(&request, responder);
                } else {
                    respond(Some(responder), None);
                }
            }
            Input::OpenUrl(url) => self.emit(EventKind::OpenUrl(url)),
            Input::Lost { epoch, error } if epoch == self.epoch => self.lost(error),
            Input::Stderr { epoch, line } if epoch == self.epoch => {
                if self.stderr.len() == STDERR_TAIL {
                    self.stderr.pop_front();
                }
                self.stderr.push_back(line);
            }
            _ => {}
        }
    }

    /// Process and deadline bookkeeping, every 100 ms. Returns true once the
    /// worker is done.
    fn tick(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) if self.forced => {
                    self.child = None;
                    self.link = None;
                    self.emit(EventKind::ShutdownComplete);
                    return true;
                }
                Ok(Some(_))
                    if self
                        .lost_at
                        .is_some_and(|when| when.elapsed() < Duration::from_millis(100)) => {}
                Ok(Some(status)) if self.lost_pending.is_some() => {
                    let error = self.lost_pending.take().expect("checked above");
                    let stderr = self.stderr.make_contiguous().join("\n");
                    let missing_initialize = self.lost_during_initialize;
                    self.child = None;
                    self.link = None;
                    self.connecting = false;
                    self.pending.clear();
                    self.handshake = None;
                    self.cancel_permissions();
                    self.setup = None;
                    self.configured = false;
                    self.epoch += 1;
                    self.shutdown_deadline = None;
                    self.stuck_reported = false;
                    if self.closing {
                        self.emit(EventKind::ShutdownComplete);
                        return true;
                    }
                    if !self.timeout_reported {
                        if !stderr.is_empty() {
                            self.emit(EventKind::Crashed(stderr.clone()));
                        }
                        let error = format!("{error} Agent exited: {status}.");
                        let kind = classify_error(&format!("{error}\n{stderr}"), None);
                        if missing_initialize
                            && kind == FailureKind::Temporary
                            && self.config.harness == harness::OMP
                        {
                            self.fail(
                                format!("{error} The configured OMP Arguments must include `acp`."),
                                FailureKind::Configuration,
                            );
                        } else {
                            self.fail(error, kind);
                        }
                    }
                }
                Ok(Some(_)) => {
                    // Wait for the connection to drain the last messages.
                    self.shutdown_deadline
                        .get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
                }
                Err(error) => {
                    self.link = None;
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
            return true;
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
        if !self.closing
            && !self.stopped_talking()
            && self
                .active
                .as_ref()
                .is_some_and(|turn| turn.retry_at.is_some_and(|when| Instant::now() >= when))
        {
            self.retry_now();
        }
        false
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Prompt { prompt, retries } => {
                if self.closing || self.active.is_some() {
                    self.emit(EventKind::Error {
                        message: "This conversation is already processing or shutting down.".into(),
                        kind: FailureKind::Configuration,
                    });
                } else if self.restore_required {
                    self.emit(EventKind::ReplacementRequired("The saved session could not be restored. Confirm a replacement session before prompting.".into()));
                } else if self.stopped_talking() {
                    self.emit(EventKind::Error {
                        message: "Agent communication stopped. Wait for shutdown or use Force Stop before retrying.".into(),
                        kind: FailureKind::Temporary,
                    });
                } else if !prompt.is_empty() {
                    self.turn += 1;
                    let prompt = if let Some(context) = self.pending_context.take() {
                        prefixed(&prompt, |text| with_context(&context, text))
                    } else {
                        prompt
                    };
                    self.active = Some(Turn::new(prompt, retries));
                    self.advance();
                }
            }
            Command::Steer { prompt } => self.steer(&prompt),
            Command::Cancel => self.cancel(),
            Command::Permission {
                request_id,
                option_id,
            } => self.permission(request_id, &option_id),
            Command::SetRetries(retries) => {
                if let Some(turn) = &mut self.active {
                    turn.retries = retries;
                }
            }
            Command::SetOption {
                category,
                id,
                value,
            } => self.switch(category, &id, &value),
            Command::ReplaceSession { context } => {
                if !self.closing && self.stopped_talking() {
                    self.emit(EventKind::Error {
                        message: "Agent communication stopped. Use Force Stop before starting a replacement session.".into(),
                        kind: FailureKind::Temporary,
                    });
                } else if !self.closing
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
                    } else if self.link.is_some() {
                        self.setup_session();
                    }
                }
            }
            Command::Fork { session, context } => self.fork = Some((session, context)),
            Command::Authenticate { method } => self.authenticate(Some(method)),
            Command::Logout => self.authenticate(None),
            Command::Probe => {
                self.probing = true;
                self.advance();
            }
            Command::Shutdown => self.shutdown(),
            Command::ForceStop => {
                self.closing = true;
                self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
                self.active = None;
                self.cancel_permissions();
                if let Some(child) = &mut self.child {
                    match crate::platform::kill_tree(child) {
                        Ok(()) => self.forced = true,
                        Err(error) => self.emit(EventKind::Error {
                            message: format!("Force stopping agent failed: {error}"),
                            kind: FailureKind::Temporary,
                        }),
                    }
                }
            }
        }
    }

    /// Moves a new turn or probe on: starts the agent, sets up its session or
    /// sends the prompt, whichever comes next.
    fn advance(&mut self) {
        if self.child.is_none() && self.link.is_none() && !self.connecting {
            self.start();
        } else if self.link.is_none() {
            // Initialize follows once the connection is up.
        } else if self.configured {
            if self.probing {
                self.emit(EventKind::Probed);
            } else {
                self.send_prompt();
            }
        } else if self.pending.is_empty() && self.setup.is_none() {
            if self.features.is_none() {
                self.initialize();
            } else {
                self.setup_session();
            }
        }
    }

    fn start(&mut self) {
        if self.closing {
            return;
        }
        if !self.config.directory.is_absolute() || !self.config.directory.is_dir() {
            self.fail("The saved working directory is missing or not absolute. Restore it or change the project for a new conversation.".into(), FailureKind::Configuration);
            return;
        }
        let command_path = Path::new(&self.config.command);
        let existing_path =
            command_path.is_file() || self.config.directory.join(command_path).is_file();
        if self.config.command.trim().is_empty()
            || (self.config.command.split_whitespace().count() != 1 && !existing_path)
        {
            self.fail("Agent command must name one executable; edit the agent definition for new conversations.".into(), FailureKind::Configuration);
            return;
        }
        let flags = self.instructions() == Instructions::Flags;
        if flags
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
            .envs(self.config.environment.iter().map(|(k, v)| (k, v)))
            .current_dir(&self.config.directory);
        if flags {
            let flag = match self.config.instructions_mode {
                InstructionsMode::Append => "--append-system-prompt",
                InstructionsMode::Overwrite => "--system-prompt",
            };
            // A trailing newline forces OMP's literal-text route, not its single-line file lookup.
            process.arg(format!("{flag}={}\n", self.guidance()));
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: ACP uses pipes.
        }
        match block_in_place(|| crate::platform::spawn_piped(&mut process, true)) {
            Ok((child, stdin, stdout, stderr)) => {
                self.configured = false;
                self.features = None;
                self.raw_options.clear();
                self.modes = None;
                self.options.clear();
                self.handshake = None;
                self.timeout_reported = false;
                self.lost_during_initialize = true;
                self.epoch += 1;
                self.stderr.clear();
                match tokio::process::ChildStdin::from_std(stdin) {
                    Ok(stdin) => {
                        self.connect(stdin, stdout);
                        if let Some(stderr) = stderr {
                            self.read_stderr(stderr);
                        }
                        self.child = Some(child);
                        self.connecting = true;
                    }
                    Err(error) => {
                        let mut child = child;
                        let _ = crate::platform::kill_tree(&mut child);
                        self.fail(
                            format!("Starting {} failed: {error}", self.config.command),
                            FailureKind::Configuration,
                        );
                    }
                }
            }
            Err(error) => self.fail(
                format!("Starting {} failed: {error}", self.config.command),
                FailureKind::Configuration,
            ),
        }
    }

    /// Runs the SDK connection over the process's stdio. Every line both ways
    /// becomes traffic; lines that are not JSON are kept as traffic and skipped.
    fn connect(&self, stdin: tokio::process::ChildStdin, stdout: crate::platform::Reader) {
        let outgoing = futures::sink::unfold(stdin, move |mut stdin, line: String| async move {
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            Ok::<_, std::io::Error>(stdin)
        });
        let lines = BufReader::new(stdout).lines();
        let incoming = futures::stream::unfold(lines, |mut lines| async move {
            match lines.next_line().await {
                Ok(Some(line)) => Some((Ok(line), lines)),
                Ok(None) => None,
                Err(error) => Some((Err(error), lines)),
            }
        });
        self.connect_lines(outgoing, incoming);
    }

    /// Runs the SDK connection over lines both ways: the agent's stdio, or a
    /// test's channels.
    fn connect_lines(
        &self,
        outgoing: impl futures::Sink<String, Error = std::io::Error> + Send + 'static,
        incoming: impl futures::Stream<Item = std::io::Result<String>> + Send + 'static,
    ) {
        use futures::SinkExt as _;
        let epoch = self.epoch;
        let inputs = self.sender.clone();
        let traffic = self.traffic_sink();
        let outgoing_traffic = traffic.clone();
        let outgoing = outgoing.with(move |line: String| {
            outgoing_traffic(Direction::ToAgent, &line, TrafficNote::None);
            futures::future::ready(Ok::<_, std::io::Error>(line))
        });
        let incoming = incoming.filter_map(move |line| {
            let kept = match line {
                Ok(line) if line.trim().is_empty() => None,
                Ok(line) => {
                    let note = classify_line(&line);
                    traffic(Direction::FromAgent, &line, note);
                    (note != TrafficNote::NotJson).then_some(Ok(line))
                }
                Err(error) => Some(Err(error)),
            };
            futures::future::ready(kept)
        });
        let handler_inputs = inputs.clone();
        tokio::spawn(async move {
            let (close, closed) = oneshot::channel::<()>();
            let mut close = Some(close);
            let connected_inputs = inputs.clone();
            let result = sdk::Client
                .builder()
                .name("adeline")
                .on_receive_dispatch(
                    async move |dispatch: Dispatch, _connection: ConnectionTo<Agent>| {
                        Ok(route(dispatch, &handler_inputs, epoch))
                    },
                    sdk::on_receive_dispatch!(),
                )
                .connect_with(
                    Lines::new(Box::pin(outgoing), Box::pin(incoming)),
                    async move |connection: ConnectionTo<Agent>| {
                        let link = Link::Sdk {
                            connection: connection.clone(),
                            _close: close.take().expect("connected once"),
                        };
                        let _ = connected_inputs.send(Input::Connected { epoch, link });
                        // Either the agent closes its output or the worker drops the link.
                        tokio::select! {
                            () = connection.incoming_closed() => {}
                            _ = closed => {}
                        }
                        Ok(())
                    },
                )
                .await;
            let error = match result {
                Ok(()) => "Agent closed protocol output.".to_owned(),
                Err(error) if sdk::is_incoming_transport_closed(&error) => {
                    "Agent closed protocol output.".to_owned()
                }
                Err(error) => format!("ACP connection failed: {error}"),
            };
            let _ = inputs.send(Input::Lost { epoch, error });
        });
    }

    fn read_stderr(&self, stderr: crate::platform::Reader) {
        let epoch = self.epoch;
        let inputs = self.sender.clone();
        let traffic = self.traffic_sink();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = if line.len() > 1024 {
                    line.chars().take(1024).collect()
                } else {
                    line
                };
                traffic(Direction::Stderr, &line, TrafficNote::None);
                if inputs.send(Input::Stderr { epoch, line }).is_err() {
                    break;
                }
            }
        });
    }

    /// Reports one line of traffic, without waiting for it to be saved.
    fn traffic_sink(&self) -> Arc<dyn Fn(Direction, &str, TrafficNote) + Send + Sync> {
        let events = self.events.clone();
        let id = self.id.clone();
        Arc::new(move |direction, line, note| {
            let _ = events.try_send(Event {
                conversation_id: id.clone(),
                turn: 0,
                kind: EventKind::Traffic(TrafficEntry {
                    at: crate::recency::now_ms(),
                    direction,
                    text: trimmed_traffic(line),
                    note,
                }),
            });
        })
    }

    fn instructions(&self) -> Instructions {
        self.profile
            .map_or(Instructions::None, |profile| profile.instructions)
    }

    fn guidance(&self) -> String {
        harness::guidance(&self.config.name, &self.config.system_instructions)
    }

    fn initialize(&mut self) {
        let capabilities = wire::ClientCapabilities::new()
            .auth(wire::AuthCapabilities::new().terminal(true))
            .elicitation(
                wire::ElicitationCapabilities::new().url(wire::ElicitationUrlCapabilities::new()),
            )
            .session(
                wire::ClientSessionCapabilities::new().config_options(
                    wire::SessionConfigOptionsCapabilities::new()
                        .boolean(wire::BooleanConfigOptionCapabilities::new()),
                ),
            );
        let request = wire::InitializeRequest::new(ProtocolVersion::V1)
            .client_capabilities(capabilities)
            .client_info(
                wire::Implementation::new("adeline", env!("CARGO_PKG_VERSION"))
                    .title("Adeline".to_owned()),
            );
        self.request(Outgoing::Initialize(request), Request::Initialize);
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
                    "Agent did not respond to ACP {request:?} within 30 seconds. Check the agent and provider connection."
                ),
                FailureKind::Temporary,
            )
        };
        self.timeout_reported = true;
        self.close_link();
        self.lost_pending = Some(message.clone());
        self.lost_at = Some(Instant::now());
        self.shutdown_deadline = Some(Instant::now() + Duration::from_secs(5));
        self.fail(message, kind);
    }

    fn request(&mut self, outgoing: Outgoing, request: Request) -> Option<u64> {
        let link = self.link.as_ref()?;
        self.next_request += 1;
        let id = self.next_request;
        link.send(id, self.epoch, outgoing, &self.sender);
        self.pending.insert(id, request);
        if !matches!(
            request,
            Request::Prompt | Request::Close | Request::Steer | Request::Authenticate
        ) {
            self.handshake = Some((id, request, Instant::now() + Duration::from_secs(30)));
        }
        Some(id)
    }

    /// The MCP servers the agent can take, reporting the ones it can't.
    fn mcp_servers(&self) -> Vec<wire::McpServer> {
        let features = self.features.clone().unwrap_or_default();
        let mut skipped = Vec::new();
        let mut servers = Vec::new();
        for server in &self.config.mcp_servers {
            match &server.transport {
                McpTransport::Stdio {
                    command,
                    arguments,
                    environment,
                } => servers.push(wire::McpServer::Stdio(
                    wire::McpServerStdio::new(server.name.clone(), command.clone())
                        .args(arguments.clone())
                        .env(
                            environment
                                .iter()
                                .map(|(name, value)| wire::EnvVariable::new(name, value))
                                .collect(),
                        ),
                )),
                McpTransport::Http { url, headers } if features.mcp_http => {
                    servers.push(wire::McpServer::Http(
                        wire::McpServerHttp::new(server.name.clone(), url.clone()).headers(
                            headers
                                .iter()
                                .map(|(name, value)| wire::HttpHeader::new(name, value))
                                .collect(),
                        ),
                    ));
                }
                McpTransport::Http { .. } => skipped.push(server.name.clone()),
            }
        }
        if !skipped.is_empty() {
            self.emit(EventKind::SkippedServers(skipped));
        }
        servers
    }

    /// `_meta` for session setup: Claude's system prompt (scope R29).
    fn session_meta(&self) -> Option<wire::Meta> {
        if self.instructions() != Instructions::SessionMeta {
            return None;
        }
        let guidance = self.guidance();
        let prompt = match self.config.instructions_mode {
            InstructionsMode::Append => json!({"append": guidance}),
            InstructionsMode::Overwrite => Value::String(guidance),
        };
        let mut meta = wire::Meta::new();
        meta.insert("systemPrompt".into(), prompt);
        Some(meta)
    }

    fn setup_session(&mut self) {
        self.configured = false;
        self.config_step = 0;
        self.forking = false;
        let features = self.features.clone().unwrap_or_default();
        let cwd = self.config.directory.clone();
        let servers = self.mcp_servers();
        let meta = self.session_meta();
        let outgoing = if let Some(id) = &self.session_id {
            if features.resume {
                Outgoing::Resume(
                    wire::ResumeSessionRequest::new(id.clone(), cwd)
                        .mcp_servers(servers)
                        .meta(meta),
                )
            } else if features.load {
                Outgoing::Load(
                    wire::LoadSessionRequest::new(id.clone(), cwd)
                        .mcp_servers(servers)
                        .meta(meta),
                )
            } else {
                self.restore_failed("The agent does not support restoring sessions.");
                return;
            }
        } else if let Some((Some(source), _)) = &self.fork
            && features.fork
        {
            self.forking = true;
            Outgoing::Fork(
                wire::ForkSessionRequest::new(source.clone(), cwd)
                    .mcp_servers(servers)
                    .meta(meta),
            )
        } else {
            Outgoing::New(
                wire::NewSessionRequest::new(cwd)
                    .mcp_servers(servers)
                    .meta(meta),
            )
        };
        if self.request(outgoing, Request::Setup).is_some() {
            self.setup = Some(Request::Setup);
        }
    }

    fn reply(&mut self, id: u64, result: Result<Reply, sdk::Error>) {
        let Some(request) = self.pending.remove(&id) else {
            return;
        };
        if self
            .handshake
            .is_some_and(|(expected, _, _)| expected == id)
        {
            self.handshake = None;
        }
        match result {
            Ok(reply) => self.succeeded(id, request, reply),
            Err(error) => self.failed(request, &error),
        }
    }

    fn failed(&mut self, request: Request, error: &sdk::Error) {
        // The connection ended: the process exit reports it, with its status.
        if sdk::is_incoming_transport_closed(error) {
            match request {
                Request::Setup => self.setup = None,
                Request::Steer => self.emit(EventKind::Steered { delivered: false }),
                Request::Authenticate | Request::Logout => {
                    self.emit(EventKind::Auth(Err("The agent stopped.".into())));
                }
                _ => {}
            }
            return;
        }
        // A failed native fork falls back to a new session with the text copy.
        if request == Request::Setup && self.forking {
            self.setup = None;
            if let Some((session, _)) = &mut self.fork {
                *session = None;
            }
            self.setup_session();
            return;
        }
        let text = error_text(error);
        let code = i64::from(i32::from(error.code));
        match request {
            Request::Steer => {
                self.emit(EventKind::Steered { delivered: false });
                return;
            }
            Request::Authenticate | Request::Logout => {
                self.emit(EventKind::Auth(Err(text)));
                return;
            }
            Request::Switch => {
                self.emit(EventKind::Error {
                    message: format!("The agent did not change the setting: {text}"),
                    kind: FailureKind::Configuration,
                });
                // The agent keeps its own value; the menus show it again.
                self.emit(EventKind::Options(self.options.clone()));
                return;
            }
            _ => {}
        }
        let kind =
            if request == Request::Prompt && self.active.as_ref().is_some_and(|turn| turn.denied) {
                FailureKind::Denied
            } else {
                classify_error(&text, Some(code))
            };
        if request == Request::Setup
            && self.session_id.is_some()
            && kind != FailureKind::Authentication
            && (code == -32601
                || [
                    "session not found",
                    "unknown session",
                    "no such session",
                    "cannot resume session",
                    "cannot load session",
                ]
                .iter()
                .any(|needle| contains_ascii(&text, needle)))
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
    }

    fn succeeded(&mut self, id: u64, request: Request, reply: Reply) {
        match (request, reply) {
            (Request::Initialize, Reply::Initialize(response)) => self.initialized(&response),
            (Request::Setup, Reply::Session(session)) => self.session_ready(session),
            (Request::Config | Request::Switch, reply) => {
                if let Reply::Options(options) = reply {
                    self.raw_options = options;
                    self.refresh_options();
                }
                if self.closing && !self.probing {
                    self.close_session();
                    return;
                }
                if request == Request::Config {
                    self.configure_next();
                }
            }
            (Request::Prompt, Reply::Prompt(reason)) => self.prompt_result(id, reason),
            (Request::Steer, Reply::Steer(outcome)) => {
                let delivered = matches!(
                    outcome.get("outcome").and_then(Value::as_str),
                    Some("injected" | "startedNewTurn")
                );
                self.emit(EventKind::Steered { delivered });
            }
            (Request::Close, _) => {
                self.close_link();
                self.shutdown_deadline
                    .get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
            }
            (Request::Authenticate | Request::Logout, _) => {
                self.emit(EventKind::Auth(Ok(())));
                if self.active.is_some() && !self.configured && self.setup.is_none() {
                    self.setup_session();
                }
            }
            (request, reply) => {
                engine_log(&format!("Unexpected ACP reply to {request:?}: {reply:?}"));
            }
        }
    }

    fn initialized(&mut self, response: &wire::InitializeResponse) {
        self.lost_during_initialize = false;
        if response.protocol_version != ProtocolVersion::V1 {
            let name = &self.config.name;
            self.fail(
                format!(
                    "{name} uses ACP version {}. Adeline supports version 1.",
                    response.protocol_version
                ),
                FailureKind::Configuration,
            );
            return;
        }
        let capabilities = &response.agent_capabilities;
        let steering = response
            .meta
            .as_ref()
            .and_then(|meta| meta.get("steering"))
            .and_then(|steering| steering.get("supported"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && self.profile.is_none_or(|profile| profile.steering);
        let features = Features {
            images: capabilities.prompt_capabilities.image,
            embedded_files: capabilities.prompt_capabilities.embedded_context,
            load: capabilities.load_session,
            resume: capabilities.session_capabilities.resume.is_some(),
            fork: capabilities.session_capabilities.fork.is_some(),
            close: capabilities.session_capabilities.close.is_some(),
            steering,
            logout: capabilities.auth.logout.is_some(),
            mcp_http: capabilities.mcp_capabilities.http,
            mcp_sse: capabilities.mcp_capabilities.sse,
            auth: response.auth_methods.iter().map(auth_method).collect(),
            known: true,
        };
        let (name, version) = response
            .agent_info
            .as_ref()
            .map(|info| (info.name.clone(), info.version.clone()))
            .unwrap_or_default();
        self.features = Some(features.clone());
        self.emit(EventKind::Agent {
            name,
            version,
            features,
        });
        if self.closing && !self.probing {
            self.close_link();
            return;
        }
        if let Some(method) = self.auth.take() {
            self.send_auth(method);
            return;
        }
        if self.active.is_some() || self.probing || self.replacing {
            self.setup_session();
        }
    }

    fn session_ready(&mut self, session: SessionReply) {
        self.restore_required = false;
        self.setup = None;
        let session_id = if let Some(id) = &self.session_id {
            id.clone()
        } else if let Some(id) = session.session_id {
            id
        } else {
            self.fail(
                "ACP session/new returned no session ID.".into(),
                FailureKind::Configuration,
            );
            return;
        };
        let was_replacement = self.replacing;
        self.session_id = Some(session_id.clone());
        self.raw_options = session.options.unwrap_or_default();
        self.modes = session.modes;
        self.previous_session.take();
        if let Some((_, context)) = self.fork.take()
            && !std::mem::take(&mut self.forking)
        {
            if let Some(turn) = &mut self.active {
                turn.prompt = prefixed(&turn.prompt, |text| with_context(&context, text));
            } else {
                self.pending_context = Some(context);
            }
            self.emit(EventKind::TextCopy);
        }
        self.emit(EventKind::Session {
            session_id,
            replaced: was_replacement,
        });
        self.refresh_options();
        if self.closing && !self.probing {
            self.close_session();
            return;
        }
        self.configure_next();
    }

    /// Rebuilds the options from the agent's config options and modes.
    fn refresh_options(&mut self) {
        self.options = translate_options(&self.raw_options, self.modes.as_ref(), self.profile);
        self.emit(EventKind::Options(self.options.clone()));
    }

    /// The saved selections in the order setup applies them: model first,
    /// because some agents (Codex) offer effort only once a model is set.
    fn selection_steps(&self) -> Vec<(Category, String, String)> {
        let selections = &self.config.selections;
        let mut steps = vec![
            (Category::Model, String::new(), selections.model.clone()),
            (Category::Effort, String::new(), selections.effort.clone()),
            (Category::Mode, String::new(), selections.mode.clone()),
        ];
        steps.extend(
            selections
                .other
                .iter()
                .map(|(id, value)| (Category::Other, id.clone(), value.clone())),
        );
        steps
    }

    /// Applies the next saved selection, then moves on. An empty value keeps
    /// the agent's default.
    fn configure_next(&mut self) {
        let steps = self.selection_steps();
        while let Some((category, id, mut value)) = steps.get(self.config_step).cloned() {
            self.config_step += 1;
            // A probe sets the current model, so efforts that depend on it show.
            if self.probing && category == Category::Model && value.is_empty() {
                value = crate::conversation::option(&self.options, Category::Model)
                    .map(SessionOption::current)
                    .unwrap_or_default();
            }
            if value.is_empty() {
                continue;
            }
            let option = match category {
                Category::Other => self.options.iter().find(|option| option.id == id),
                category => crate::conversation::option(&self.options, category),
            }
            .cloned();
            let label = match category {
                Category::Model => "model",
                Category::Effort => "effort",
                Category::Mode => "mode",
                Category::Other => "option",
            };
            let Some(option) = option else {
                if matches!(category, Category::Model | Category::Effort) {
                    self.fail(format!("Agent does not expose required {label} configuration; saved settings cannot be honored. Log in with the configured agent or choose one that offers this option."), FailureKind::Configuration);
                    return;
                }
                continue;
            };
            if !option.offers(&value) && !profiles::is_plan_mode(self.profile, &value) {
                if matches!(category, Category::Model | Category::Effort) {
                    self.fail(format!("Agent does not offer saved {label} value '{value}'. Choose a supported setting for this conversation or in the agent definition."), FailureKind::Configuration);
                    return;
                }
                continue;
            }
            if category != Category::Model && option.current() == value {
                continue;
            }
            if self.set_option(&option, &value, Request::Config) {
                return;
            }
            return;
        }
        self.configured();
    }

    /// Sends one option change. Returns whether a request went out.
    fn set_option(&mut self, option: &SessionOption, value: &str, request: Request) -> bool {
        let Some(session) = self.session_id.clone() else {
            return false;
        };
        let outgoing = if option.id == MODE_OPTION {
            Outgoing::SetMode(wire::SetSessionModeRequest::new(session, value.to_owned()))
        } else if matches!(option.kind, OptionKind::Boolean { .. }) {
            Outgoing::SetConfig(wire::SetSessionConfigOptionRequest::new(
                session,
                option.id.clone(),
                value == "true",
            ))
        } else {
            Outgoing::SetConfig(wire::SetSessionConfigOptionRequest::new(
                session,
                option.id.clone(),
                value,
            ))
        };
        if option.id == MODE_OPTION {
            // `session/set_mode` answers without the new state.
            if let Some(modes) = &mut self.modes {
                modes.current_mode_id = wire::SessionModeId::new(value.to_owned());
            }
        }
        self.request(outgoing, request).is_some()
    }

    fn configured(&mut self) {
        self.configured = true;
        if self.probing {
            self.emit(EventKind::Probed);
            return;
        }
        if self.replacing {
            self.replacing = false;
            let context = self.replacement.take().unwrap_or_default();
            if !context.trim().is_empty() {
                if let Some(turn) = &mut self.active {
                    turn.prompt = vec![Part::Text(format!(
                        "Saved conversation context follows. Treat it as history, not a new user request. Do not repeat completed tool actions.\n\n{context}\n\nContinue the interrupted turn from where it stopped; do not repeat completed work."
                    ))];
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

    fn switch(&mut self, category: Category, id: &str, value: &str) {
        self.config.selections.set(category, id, value);
        // Without a ready, idle session the next setup applies the new value.
        if !self.configured || self.active.is_some() || self.link.is_none() {
            return;
        }
        let option = match category {
            Category::Other => self.options.iter().find(|option| option.id == id),
            category => crate::conversation::option(&self.options, category),
        }
        .cloned();
        if let Some(option) = option
            && option.offers(value)
        {
            self.set_option(&option, value, Request::Switch);
            if option.id == MODE_OPTION {
                self.refresh_options();
            }
        }
    }

    fn send_prompt(&mut self) {
        if self.closing || !self.configured {
            return;
        }
        let Some(session_id) = self.session_id.clone() else {
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
        let blocks = match self.content(&turn.prompt) {
            Ok(blocks) => blocks,
            Err(error) => {
                self.fail(error, FailureKind::Configuration);
                return;
            }
        };
        let request = self.request(
            Outgoing::Prompt(wire::PromptRequest::new(session_id, blocks)),
            Request::Prompt,
        );
        if let Some(turn) = &mut self.active {
            turn.prompt_request = request;
            turn.retry_at = None;
        }
    }

    /// A prompt as ACP content: text, images, and files embedded or linked by
    /// what the agent supports (scope R23).
    fn content(&self, parts: &[Part]) -> Result<Vec<wire::ContentBlock>, String> {
        let features = self.features.clone().unwrap_or_default();
        let mut blocks = Vec::new();
        for part in parts {
            match part {
                Part::Text(text) => {
                    blocks.push(wire::ContentBlock::Text(wire::TextContent::new(
                        text.clone(),
                    )));
                }
                Part::File(file) if file.image() => {
                    if !features.images {
                        return Err(format!("{} can't receive images.", self.config.name));
                    }
                    let data = attachment_data(file)?;
                    blocks.push(wire::ContentBlock::Image(wire::ImageContent::new(
                        data,
                        file.mime.clone(),
                    )));
                }
                Part::File(file) if features.embedded_files => {
                    let bytes = attachment_bytes(file)?;
                    let uri = file_uri(file);
                    let resource = match String::from_utf8(bytes) {
                        Ok(text) => wire::EmbeddedResourceResource::TextResourceContents(
                            wire::TextResourceContents::new(text, uri).mime_type(file.mime.clone()),
                        ),
                        Err(error) => wire::EmbeddedResourceResource::BlobResourceContents(
                            wire::BlobResourceContents::new(
                                crate::conversation::base64(error.as_bytes()),
                                uri,
                            )
                            .mime_type(file.mime.clone()),
                        ),
                    };
                    blocks.push(wire::ContentBlock::Resource(wire::EmbeddedResource::new(
                        resource,
                    )));
                }
                Part::File(file) => {
                    let path = linked_path(file)?;
                    blocks.push(wire::ContentBlock::ResourceLink(
                        wire::ResourceLink::new(file.name.clone(), path_uri(&path))
                            .mime_type(file.mime.clone())
                            .size(i64::try_from(file.size).ok()),
                    ));
                }
            }
        }
        Ok(blocks)
    }

    fn steer(&mut self, prompt: &[Part]) {
        let steerable = self
            .features
            .as_ref()
            .is_some_and(|features| features.steering)
            && self
                .active
                .as_ref()
                .is_some_and(|turn| turn.prompt_request.is_some() && !turn.cancelled);
        let Some(session) = self.session_id.clone().filter(|_| steerable) else {
            self.emit(EventKind::Steered { delivered: false });
            return;
        };
        let blocks = match self.content(prompt) {
            Ok(blocks) => blocks,
            Err(error) => {
                self.emit(EventKind::Error {
                    message: error,
                    kind: FailureKind::Configuration,
                });
                self.emit(EventKind::Steered { delivered: false });
                return;
            }
        };
        let params = json!({
            "sessionId": session,
            "prompt": blocks,
            "_meta": {"steering": {"idleBehavior": "promptRequired"}},
        });
        if self
            .request(Outgoing::Steer(params), Request::Steer)
            .is_none()
        {
            self.emit(EventKind::Steered { delivered: false });
        }
    }

    fn authenticate(&mut self, method: Option<String>) {
        if self.link.is_some() && self.features.is_some() {
            self.send_auth(method);
        } else {
            self.auth = Some(method);
            if self.child.is_none() {
                self.start();
            }
        }
    }

    fn send_auth(&mut self, method: Option<String>) {
        let outgoing = match method {
            Some(method) => (
                Outgoing::Authenticate(wire::AuthenticateRequest::new(method)),
                Request::Authenticate,
            ),
            None => (
                Outgoing::Logout(wire::LogoutRequest::new()),
                Request::Logout,
            ),
        };
        if self.request(outgoing.0, outgoing.1).is_none() {
            self.emit(EventKind::Auth(Err("The agent is not running.".into())));
        }
    }

    fn update(&mut self, notification: wire::SessionNotification) {
        if Some(notification.session_id.0.as_ref()) != self.session_id.as_deref() {
            return;
        }
        let ends_turn = self.profile.is_some_and(|profile| {
            profile.turn_end.iter().any(|key| {
                notification
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get(*key))
                    .and_then(Value::as_bool)
                    == Some(true)
            })
        });
        let in_turn = self.setup.is_none()
            && self
                .active
                .as_ref()
                .is_some_and(|turn| !turn.cancelled && turn.prompt_request.is_some());
        match notification.update {
            // Session state counts between turns too (scope R7).
            wire::SessionUpdate::UsageUpdate(usage) => {
                if self.setup.is_none() {
                    self.emit(EventKind::Usage {
                        used: usage.used,
                        size: usage.size,
                    });
                }
            }
            wire::SessionUpdate::AvailableCommandsUpdate(update) => {
                self.emit(EventKind::Commands(
                    update
                        .available_commands
                        .into_iter()
                        .map(|command| AgentCommand {
                            name: command.name,
                            description: command.description,
                            hint: match command.input {
                                Some(wire::AvailableCommandInput::Unstructured(input)) => {
                                    input.hint
                                }
                                _ => String::new(),
                            },
                        })
                        .collect(),
                ));
            }
            wire::SessionUpdate::CurrentModeUpdate(update) => {
                if let Some(modes) = &mut self.modes {
                    modes.current_mode_id = update.current_mode_id;
                    self.refresh_options();
                }
            }
            wire::SessionUpdate::ConfigOptionUpdate(update) => {
                self.raw_options = update.config_options;
                self.refresh_options();
            }
            wire::SessionUpdate::SessionInfoUpdate(update) => {
                if let MaybeUndefined::Value(title) = update.title
                    && !title.trim().is_empty()
                {
                    self.emit(EventKind::Title(title));
                }
            }
            wire::SessionUpdate::Plan(plan) if self.setup.is_none() => {
                self.emit(EventKind::Todo(
                    plan.entries
                        .into_iter()
                        .map(|entry| TodoStep {
                            text: entry.content,
                            status: match entry.status {
                                wire::PlanEntryStatus::InProgress => StepStatus::InProgress,
                                wire::PlanEntryStatus::Completed => StepStatus::Completed,
                                _ => StepStatus::Pending,
                            },
                        })
                        .collect(),
                ));
            }
            wire::SessionUpdate::AgentMessageChunk(chunk) if in_turn => {
                if let Some(text) = chunk_text(&chunk)
                    && !text.is_empty()
                {
                    if let Some(turn) = &mut self.active {
                        turn.worked = true;
                        if turn.observed_text.len() < 2048 {
                            turn.observed_text.push_str(&text);
                        }
                    }
                    self.emit(EventKind::Text {
                        message: chunk.message_id.map(|id| id.0.to_string()),
                        text,
                    });
                }
            }
            wire::SessionUpdate::AgentThoughtChunk(chunk) if in_turn => {
                if let Some(text) = chunk_text(&chunk)
                    && !text.is_empty()
                {
                    if let Some(turn) = &mut self.active {
                        turn.worked = true;
                    }
                    self.emit(EventKind::Thought {
                        message: chunk.message_id.map(|id| id.0.to_string()),
                        text,
                    });
                }
            }
            // Tool calls outside a turn are background work the agent reports.
            wire::SessionUpdate::ToolCall(call) if self.setup.is_none() => {
                let update = wire::ToolCallUpdate::new(
                    call.tool_call_id,
                    wire::ToolCallUpdateFields::new()
                        .kind(call.kind)
                        .status(call.status)
                        .title(call.title)
                        .content(call.content)
                        .locations(call.locations)
                        .raw_output(call.raw_output),
                );
                self.tool(update, in_turn);
            }
            wire::SessionUpdate::ToolCallUpdate(update) if self.setup.is_none() => {
                self.tool(update, in_turn);
            }
            // Replay, user echoes and notices stay in the raw traffic.
            _ => {}
        }
        if ends_turn && in_turn {
            self.finish(StopReason::Signal);
        }
    }

    fn tool(&mut self, update: wire::ToolCallUpdate, in_turn: bool) {
        if in_turn && let Some(turn) = &mut self.active {
            turn.worked = true;
        }
        let id = update.tool_call_id.0.to_string();
        let fields = update.fields;
        let entry = self.tools.entry(id.clone()).or_default();
        if let Some(title) = fields.title {
            entry.title = title;
        }
        if let Some(status) = fields.status {
            entry.status = tool_status(status);
        }
        let detail = tool_detail(fields.raw_output.as_ref(), fields.content.as_deref());
        if !detail.is_empty() {
            entry.detail = detail;
        }
        if let Some(kind) = fields.kind {
            entry.kind = tool_kind(kind);
        }
        if let Some(locations) = fields.locations {
            entry.paths = locations
                .iter()
                .map(|location| location.path.to_string_lossy().into_owned())
                .collect();
        }
        let ToolState {
            title,
            status,
            detail,
            kind,
            paths,
        } = entry.clone();
        self.emit(EventKind::Tool {
            id,
            title,
            status,
            detail,
            kind,
            paths,
        });
    }

    fn permission_request(
        &mut self,
        request: &wire::RequestPermissionRequest,
        responder: Responder<wire::RequestPermissionResponse>,
    ) {
        if Some(request.session_id.0.as_ref()) != self.session_id.as_deref()
            || self
                .active
                .as_ref()
                .is_none_or(|turn| turn.cancelled || turn.prompt_request.is_none())
        {
            respond(Some(responder), None);
            return;
        }
        let options: Vec<_> = request
            .options
            .iter()
            .filter_map(|option| {
                let kind = match option.kind {
                    wire::PermissionOptionKind::AllowOnce => PermissionKind::AllowOnce,
                    wire::PermissionOptionKind::AllowAlways => PermissionKind::AllowAlways,
                    wire::PermissionOptionKind::RejectOnce => PermissionKind::RejectOnce,
                    // "Reject always" stays hidden (scope R21).
                    _ => return None,
                };
                Some(PermissionOption {
                    id: option.option_id.0.to_string(),
                    name: option.name.clone(),
                    kind,
                })
            })
            .collect();
        if options.is_empty() {
            respond(Some(responder), None);
            if let Some(turn) = &mut self.active {
                turn.denied = true;
            }
            return;
        }
        let tool = &request.tool_call.fields;
        let mut title = tool
            .title
            .clone()
            .unwrap_or_else(|| "Agent requests permission".to_owned());
        let detail = tool_detail(tool.raw_output.as_ref(), tool.content.as_deref());
        if !detail.is_empty() {
            title.push('\n');
            title.push_str(&detail);
        }
        if let Some(input) = tool.raw_input.as_ref().filter(|input| !input.is_null()) {
            use std::fmt::Write as _;
            let _ = write!(title, "\nInput: {input}");
        }
        self.next_permission += 1;
        let request_id = self.next_permission;
        self.permissions.insert(
            request_id,
            PendingPermission {
                turn: self.turn,
                options: options.clone(),
                responder: Some(responder),
            },
        );
        self.emit(EventKind::Permission {
            request_id,
            title,
            options,
        });
    }

    fn permission(&mut self, id: u64, option_id: &str) {
        let Some(mut pending) = self.permissions.remove(&id) else {
            return;
        };
        if pending.turn != self.turn || self.active.as_ref().is_none_or(|active| active.cancelled) {
            respond(pending.responder.take(), None);
            return;
        }
        if let Some(option) = pending.options.iter().find(|option| option.id == option_id) {
            let denied = option.kind == PermissionKind::RejectOnce;
            respond(pending.responder.take(), Some(option_id));
            if denied && let Some(active) = &mut self.active {
                active.denied = true;
            }
        } else {
            respond(pending.responder.take(), None);
        }
    }

    fn cancel_permissions(&mut self) {
        for (_, mut pending) in self.permissions.drain() {
            respond(pending.responder.take(), None);
        }
    }

    fn cancel(&mut self) {
        self.cancel_permissions();
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.cancelled = true;
        turn.retry_at = None;
        if turn.prompt_request.is_some() {
            if let (Some(link), Some(session)) = (&self.link, &self.session_id) {
                link.cancel(session);
            }
        } else {
            self.active = None;
            self.emit(EventKind::Stopped);
        }
    }

    /// Ends the active turn as finished.
    fn finish(&mut self, stop_reason: StopReason) {
        self.active = None;
        self.cancel_permissions();
        self.emit(EventKind::Finished { stop_reason });
    }

    fn prompt_result(&mut self, id: u64, reason: wire::StopReason) {
        let Some(turn) = &mut self.active else {
            return;
        };
        // A late answer to a turn a profile signal already ended.
        if turn.prompt_request != Some(id) {
            return;
        }
        turn.prompt_request = None;
        if turn.cancelled {
            self.active = None;
            self.emit(EventKind::Stopped);
            if self.closing {
                self.close_session();
            }
            return;
        }
        let reason = match reason {
            wire::StopReason::EndTurn => StopReason::EndTurn,
            wire::StopReason::MaxTokens => StopReason::MaxTokens,
            wire::StopReason::MaxTurnRequests => StopReason::MaxTurnRequests,
            wire::StopReason::Refusal => StopReason::Refusal,
            _ => StopReason::Cancelled,
        };
        if reason == StopReason::Cancelled {
            self.active = None;
            self.emit(EventKind::Stopped);
        } else if self.active.as_ref().is_some_and(|turn| turn.denied) {
            self.fail("Permission denied by the user.".into(), FailureKind::Denied);
        } else if reason == StopReason::EndTurn
            && self
                .active
                .as_ref()
                .is_some_and(|turn| looks_like_agent_error(&turn.observed_text))
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
        } else {
            self.finish(reason);
        }
        if self.closing {
            self.close_session();
        }
    }

    fn fail(&mut self, message: String, kind: FailureKind) {
        let message = if kind == FailureKind::Authentication {
            format!("{message} Log in, then retry.")
        } else {
            message
        };
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
        self.cancel_permissions();
    }

    fn retry_now(&mut self) {
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.retry_at = None;
        if turn.worked {
            turn.prompt = vec![Part::Text(continue_interrupted_turn().into())];
            if self.child.is_none() {
                self.start();
            } else {
                self.setup_session();
            }
        } else if self.child.is_none() {
            self.start();
        } else if !self.configured {
            if self.features.is_none() {
                self.initialize();
            } else {
                self.setup_session();
            }
        } else {
            self.send_prompt();
        }
    }

    fn restore_failed(&mut self, reason: &str) {
        self.restore_required = true;
        self.emit(EventKind::ReplacementRequired(format!("{reason} You can start a replacement session with saved conversation content after confirming.")));
        if let Some(turn) = &mut self.active {
            turn.prompt_request = None;
            turn.retry_at = None;
        }
    }

    fn lost(&mut self, message: String) {
        let initializing = self.lost_during_initialize
            || self
                .pending
                .values()
                .any(|request| *request == Request::Initialize);
        self.lost_during_initialize = initializing;
        let invalid_initialize =
            !self.closing && initializing && message.starts_with("ACP connection failed");
        self.link = None;
        self.connecting = false;
        self.pending.clear();
        self.cancel_permissions();
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
        self.probing = false;
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
        if let Some(session_id) = self.session_id.clone()
            && self
                .features
                .as_ref()
                .is_some_and(|features| features.close)
            && self.link.is_some()
        {
            self.request(
                Outgoing::Close(wire::CloseSessionRequest::new(session_id)),
                Request::Close,
            );
            return;
        }
        self.close_link();
    }

    /// Ends the connection, which closes the agent's stdin.
    fn close_link(&mut self) {
        self.link = None;
        self.connecting = false;
    }
}

/// Routes one message from the agent into the worker.
fn route(dispatch: Dispatch, inputs: &UnboundedSender<Input>, epoch: u64) -> Handled<Dispatch> {
    match dispatch {
        Dispatch::Request(message, responder) => {
            match message.method() {
                "session/request_permission" => {
                    match serde_json::from_value::<wire::RequestPermissionRequest>(
                        message.params().clone(),
                    ) {
                        Ok(request) => {
                            let _ = inputs.send(Input::Permission {
                                epoch,
                                request: Box::new(request),
                                responder: responder.cast(),
                            });
                        }
                        Err(error) => {
                            let _ = responder.respond_with_error(
                                sdk::Error::invalid_params().data(error.to_string()),
                            );
                        }
                    }
                }
                "elicitation/create" => {
                    let url = serde_json::from_value::<wire::CreateElicitationRequest>(
                        message.params().clone(),
                    )
                    .ok()
                    .and_then(|request| match request.mode {
                        wire::ElicitationMode::Url(url) => Some(url.url),
                        _ => None,
                    });
                    let action = if let Some(url) = url {
                        let _ = inputs.send(Input::OpenUrl(url));
                        wire::ElicitationAction::Accept(wire::ElicitationAcceptAction::new())
                    } else {
                        wire::ElicitationAction::Decline
                    };
                    let _ = responder
                        .cast::<wire::CreateElicitationResponse>()
                        .respond(wire::CreateElicitationResponse::new(action));
                }
                // Adeline offers no `fs/*` or `terminal/*` (scope boundaries).
                method => {
                    let _ = responder
                        .respond_with_error(sdk::Error::method_not_found().data(method.to_owned()));
                }
            }
            Handled::Yes
        }
        Dispatch::Notification(message) => {
            if message.method() == "session/update"
                && let Ok(notification) =
                    serde_json::from_value::<wire::SessionNotification>(message.params().clone())
            {
                let _ = inputs.send(Input::Update {
                    epoch,
                    notification: Box::new(notification),
                });
            }
            Handled::Yes
        }
        dispatch @ Dispatch::Response(..) => Handled::No {
            message: dispatch,
            retry: false,
        },
    }
}

/// Answers a permission request: the chosen option, or cancelled.
fn respond(responder: Option<Responder<wire::RequestPermissionResponse>>, option: Option<&str>) {
    let Some(responder) = responder else {
        return;
    };
    let outcome = option.map_or(wire::RequestPermissionOutcome::Cancelled, |option| {
        wire::RequestPermissionOutcome::Selected(wire::SelectedPermissionOutcome::new(
            option.to_owned(),
        ))
    });
    let _ = responder.respond(wire::RequestPermissionResponse::new(outcome));
}

/// The session update types and methods Adeline understands; anything else
/// is marked unknown in the traffic view (scope R4).
fn classify_line(line: &str) -> TrafficNote {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return TrafficNote::NotJson;
    };
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return TrafficNote::None;
    };
    let known = match method {
        "session/update" => value.pointer("/params/update").is_some_and(|update| {
            serde_json::from_value::<wire::SessionUpdate>(update.clone()).is_ok()
        }),
        "session/request_permission" | "elicitation/create" | "elicitation/complete" => true,
        _ => false,
    };
    if known {
        TrafficNote::None
    } else {
        TrafficNote::Unknown
    }
}

/// Traffic lines as kept: long ones shortened, attached file data dropped.
fn trimmed_traffic(line: &str) -> String {
    const LIMIT: usize = 64 * 1024;
    if line.len() <= LIMIT {
        line.to_owned()
    } else {
        let mut end = LIMIT;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}… [{} more bytes]", &line[..end], line.len() - end)
    }
}

fn auth_method(method: &wire::AuthMethod) -> AuthMethod {
    match method {
        wire::AuthMethod::Terminal(terminal) => AuthMethod {
            id: terminal.id.0.to_string(),
            name: terminal.name.clone(),
            description: terminal.description.clone().unwrap_or_default(),
            terminal: Some(TerminalLogin {
                arguments: terminal.args.clone(),
                environment: terminal
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            }),
        },
        wire::AuthMethod::Agent(agent) => AuthMethod {
            id: agent.id.0.to_string(),
            name: agent.name.clone(),
            description: agent.description.clone().unwrap_or_default(),
            terminal: None,
        },
        _ => AuthMethod {
            id: method.id().0.to_string(),
            name: method.id().0.to_string(),
            description: String::new(),
            terminal: None,
        },
    }
}

/// The agent's config options and modes in Adeline's terms. Plan modes in
/// the profile are left out of the choices (scope R20).
fn translate_options(
    raw: &[wire::SessionConfigOption],
    modes: Option<&wire::SessionModeState>,
    profile: Option<&Profile>,
) -> Vec<SessionOption> {
    let mut options: Vec<SessionOption> = Vec::new();
    for option in raw {
        let id = option.id.0.to_string();
        let mut category = match &option.category {
            Some(wire::SessionConfigOptionCategory::Mode) => Category::Mode,
            Some(wire::SessionConfigOptionCategory::Model) => Category::Model,
            Some(wire::SessionConfigOptionCategory::ThoughtLevel) => Category::Effort,
            Some(_) => Category::Other,
            None => match id.as_str() {
                "model" => Category::Model,
                "thinking" | "effort" | "reasoning_effort" | "thought_level" => Category::Effort,
                "mode" => Category::Mode,
                _ => Category::Other,
            },
        };
        // Each of the composer's own menus shows one option.
        if category != Category::Other && options.iter().any(|o| o.category == category) {
            category = Category::Other;
        }
        let kind = match &option.kind {
            wire::SessionConfigKind::Boolean(boolean) => OptionKind::Boolean {
                current: boolean.current_value,
            },
            wire::SessionConfigKind::Select(select) => {
                let mut choices = Vec::new();
                match &select.options {
                    wire::SessionConfigSelectOptions::Ungrouped(items) => {
                        choices.extend(items.iter().map(|item| select_choice(item, "")));
                        if !choices.is_empty() && choices.iter().all(|c| c.value.contains('/')) {
                            for choice in &mut choices {
                                choice.group = choice
                                    .value
                                    .split('/')
                                    .next()
                                    .unwrap_or_default()
                                    .to_owned();
                            }
                        }
                    }
                    wire::SessionConfigSelectOptions::Grouped(groups) => {
                        for group in groups {
                            choices.extend(
                                group
                                    .options
                                    .iter()
                                    .map(|item| select_choice(item, &group.name)),
                            );
                        }
                    }
                    _ => {}
                }
                if category == Category::Mode {
                    choices.retain(|choice| !profiles::is_plan_mode(profile, &choice.value));
                    for choice in &mut choices {
                        choice.description =
                            profiles::mode_description(profile, &choice.value, &choice.description);
                    }
                }
                OptionKind::Select {
                    current: select.current_value.0.to_string(),
                    choices,
                }
            }
            _ => continue,
        };
        options.push(SessionOption {
            id,
            name: option.name.clone(),
            description: option.description.clone().unwrap_or_default(),
            category,
            kind,
        });
    }
    if let Some(modes) = modes
        && !options
            .iter()
            .any(|option| option.category == Category::Mode)
    {
        options.push(SessionOption {
            id: MODE_OPTION.into(),
            name: "Mode".into(),
            description: String::new(),
            category: Category::Mode,
            kind: OptionKind::Select {
                current: modes.current_mode_id.0.to_string(),
                choices: modes
                    .available_modes
                    .iter()
                    .filter(|mode| !profiles::is_plan_mode(profile, &mode.id.0))
                    .map(|mode| Choice {
                        value: mode.id.0.to_string(),
                        name: mode.name.clone(),
                        description: profiles::mode_description(
                            profile,
                            &mode.id.0,
                            mode.description.as_deref().unwrap_or_default(),
                        ),
                        group: String::new(),
                    })
                    .collect(),
            },
        });
    }
    options
}

fn select_choice(item: &wire::SessionConfigSelectOption, group: &str) -> Choice {
    Choice {
        value: item.value.0.to_string(),
        name: item.name.clone(),
        description: item.description.clone().unwrap_or_default(),
        group: group.to_owned(),
    }
}

fn chunk_text(chunk: &wire::ContentChunk) -> Option<String> {
    match &chunk.content {
        wire::ContentBlock::Text(text) => Some(text.text.clone()),
        wire::ContentBlock::ResourceLink(link) => Some(format!("[{}]({})", link.name, link.uri)),
        _ => None,
    }
}

fn tool_kind(kind: wire::ToolKind) -> ToolKind {
    match kind {
        wire::ToolKind::Read => ToolKind::Read,
        wire::ToolKind::Edit => ToolKind::Edit,
        wire::ToolKind::Delete => ToolKind::Delete,
        wire::ToolKind::Move => ToolKind::Move,
        wire::ToolKind::Search => ToolKind::Search,
        wire::ToolKind::Execute => ToolKind::Execute,
        wire::ToolKind::Think => ToolKind::Think,
        wire::ToolKind::Fetch => ToolKind::Fetch,
        wire::ToolKind::SwitchMode => ToolKind::SwitchMode,
        _ => ToolKind::Other,
    }
}

fn tool_status(status: wire::ToolCallStatus) -> ToolStatus {
    match status {
        wire::ToolCallStatus::InProgress => ToolStatus::InProgress,
        wire::ToolCallStatus::Completed => ToolStatus::Completed,
        wire::ToolCallStatus::Failed => ToolStatus::Failed,
        _ => ToolStatus::Pending,
    }
}

fn tool_detail(raw_output: Option<&Value>, content: Option<&[wire::ToolCallContent]>) -> String {
    if let Some(raw) = raw_output.filter(|value| !value.is_null()) {
        return raw.as_str().map_or_else(|| raw.to_string(), str::to_owned);
    }
    let mut detail = String::new();
    for item in content.into_iter().flatten() {
        let (path, text) = match item {
            wire::ToolCallContent::Content(content) => match &content.content {
                wire::ContentBlock::Text(text) => (None, text.text.clone()),
                _ => continue,
            },
            wire::ToolCallContent::Diff(diff) => (
                Some(diff.path.to_string_lossy().into_owned()),
                diff.new_text.clone(),
            ),
            _ => continue,
        };
        if !detail.is_empty() {
            detail.push('\n');
        }
        if let Some(path) = path {
            detail.push_str(&path);
            detail.push_str(":\n");
        }
        detail.push_str(&text);
    }
    detail
}

fn attachment_bytes(file: &Attachment) -> Result<Vec<u8>, String> {
    if !file.data.is_empty() {
        return crate::conversation::unbase64(&file.data)
            .ok_or_else(|| format!("{} could not be read.", file.name));
    }
    let path = file
        .path
        .as_ref()
        .ok_or_else(|| format!("{} has no content.", file.name))?;
    block_in_place(|| std::fs::read(path)).map_err(|error| crate::files::error(path, error))
}

fn attachment_data(file: &Attachment) -> Result<String, String> {
    if file.data.is_empty() {
        attachment_bytes(file).map(|bytes| crate::conversation::base64(&bytes))
    } else {
        Ok(file.data.clone())
    }
}

/// A file to link: its own path, else a copy of its bytes in a temporary folder.
fn linked_path(file: &Attachment) -> Result<std::path::PathBuf, String> {
    if let Some(path) = &file.path {
        return Ok(path.clone());
    }
    let bytes = attachment_bytes(file)?;
    let folder = std::env::temp_dir().join(crate::files::unique("adeline-attachment"));
    let name = Path::new(&file.name)
        .file_name()
        .map_or_else(|| "attachment".into(), |name| name.to_os_string());
    let path = folder.join(name);
    block_in_place(|| std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, bytes)))
        .map_err(|error| crate::files::error(&path, error))?;
    Ok(path)
}

fn file_uri(file: &Attachment) -> String {
    file.path
        .as_deref()
        .map_or_else(|| format!("file:///{}", file.name), path_uri)
}

fn path_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

fn error_text(error: &sdk::Error) -> String {
    match error.data.as_ref().and_then(Value::as_str) {
        Some(data) if !data.is_empty() && !error.message.contains(data) => {
            format!("{} ({data})", error.message)
        }
        _ => error.message.clone(),
    }
}

fn engine_log(message: &str) {
    crate::engine::log(message);
}

/// A prompt that carries saved conversation history ahead of the new request.
fn with_context(context: &str, text: &str) -> String {
    format!(
        "Saved conversation context (prior history; do not repeat completed tool actions):\n{context}\n\nNew user request:\n{text}"
    )
}

fn continue_interrupted_turn() -> &'static str {
    "Continue the interrupted turn from where you stopped. Do not repeat completed work or tool actions."
}

fn contains_ascii(text: &str, needle: &str) -> bool {
    text.as_bytes()
        .windows(needle.len())
        .any(|bytes| bytes.eq_ignore_ascii_case(needle.as_bytes()))
}

fn classify_error(message: &str, code: Option<i64>) -> FailureKind {
    if matches!(code, Some(401 | -32000))
        || ["auth", "api key", "login", "unauthorized", "logged in"]
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

fn looks_like_agent_error(text: &str) -> bool {
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

// ---------------------------------------------------------------------------
// Probes and logins: short-lived sessions through the same client.

/// What a probe learned about an agent.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Probed {
    /// The name the agent reported at `initialize`.
    pub identity: String,
    pub version: String,
    pub features: Features,
    pub options: Vec<SessionOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProbeError {
    pub message: String,
    /// The agent's login methods, when it needs a login.
    #[serde(default)]
    pub auth: Vec<AuthMethod>,
    /// The agent speaks another ACP version; retrying won't help.
    #[serde(default)]
    pub version: bool,
}

/// A running probe or login. Dropping it stops the agent.
pub struct Session {
    _driver: Driver,
}

/// Starts the agent in a temporary folder, opens a session without
/// prompting, applies `config`'s selections and reports what it offers.
pub fn probe(
    mut config: ExecutionConfig,
) -> (Session, async_channel::Receiver<Result<Probed, ProbeError>>) {
    let folder = std::env::temp_dir().join(crate::files::unique("adeline-probe"));
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
                        version: message.contains("Adeline supports version 1"),
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

/// Starts the agent and logs in with `method`, or logs out without one.
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

#[cfg(test)]
#[path = "acp_tests.rs"]
mod tests;
