//! One ACP agent process per conversation, spoken to through the official
//! `agent-client-protocol` SDK and its typed v1 messages.
//!
//! This is the only module that knows ACP: it translates the protocol to and from
//! Adeline's own model (`conversation.rs`). Probes and logins are short-lived
//! sessions through the same client; dropping their `Session` stops the agent.
//!
//! Every line both ways, and the agent's stderr, becomes traffic; lines that
//! aren't JSON are kept as traffic and skipped. Session modes offered through
//! `session/set_mode` rather than as a config option appear as `MODE_OPTION`.
//! Usage is in tokens.
//!
//! The `Worker` is three state machines side by side: `Connection` (the process,
//! its link and the handshake, with `AgentSession` for which session to open),
//! `Step` (the active turn) and `Exit` (shutdown, with `ExitWatch` for an exit
//! that is overdue).
use adeline_core::agents::ExecutionConfig;
use adeline_core::{
    agents::InstructionsMode,
    conversation::{
        AgentCommand, Attachment, AuthMethod, Category, Choice, Direction, Features, McpTransport,
        OptionKind, PermissionKind, PermissionOption, SessionOption, StepStatus, StopReason,
        TerminalLogin, TodoStep, ToolKind, ToolStatus, TrafficEntry, TrafficNote,
    },
    harness,
    profiles::{self, Instructions, Profile},
};
use agent_client_protocol::{
    self as sdk, Agent, ConnectionTo, Dispatch, Handled, Lines, Responder, UntypedMessage,
    schema::{MaybeUndefined, ProtocolVersion, v1 as wire},
};
use anyhow::Context as _;
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

mod failure;
mod options;
mod permissions;
mod probe;
mod session;
mod turns;
use adeline_core::protocol::{ProbeError, Probed};
use failure::{broken_pipe, classify_error, looks_like_agent_error, with_context};
use permissions::respond;
pub use probe::{Session, login, probe};
use turns::tool_detail;

const MODE_OPTION: &str = "adeline/session-mode";

pub const CONTINUE_TURN: &str = "Continue the interrupted turn from the saved session. Preserve completed work; do not repeat completed tool actions.";

const STDERR_TAIL: usize = 12;
const TICK: Duration = Duration::from_millis(100);
const LOST_WINDOW: Duration = Duration::from_millis(100);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const OBSERVED_TEXT: usize = 2048;
const MAX_RETRY_DELAY_SECS: u32 = 5;

#[derive(Debug)]
pub struct Driver {
    sender: UnboundedSender<Input>,
}

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
    Steer {
        prompt: Vec<Part>,
    },
    Cancel,
    Permission {
        request_id: u64,
        option_id: String,
    },
    SetRetries(u32),
    SetOption {
        category: Category,
        id: String,
        value: String,
    },
    ReplaceSession {
        context: String,
    },
    Fork {
        session: Option<String>,
        context: String,
    },
    Authenticate {
        method: String,
    },
    Logout,
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
    Usage {
        used: u64,
        size: u64,
    },
    Permission {
        request_id: u64,
        title: String,
        options: Vec<PermissionOption>,
    },
    Steered {
        delivered: bool,
    },
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
    TextCopy,
    Traffic(TrafficEntry),
    OpenUrl(String),
    Auth(Result<(), String>),
    Probed,
    Crashed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Temporary,
    Configuration,
    Authentication,
    Denied,
    Version,
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
            .map_err(|_closed| "Agent worker has stopped.".into())
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.sender.send(Input::Command(Command::Shutdown));
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
    Config,
    Switch,
    Prompt,
    Steer,
    Close,
    Authenticate,
    Logout,
}

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
    Steer(Value),
    Close(wire::CloseSessionRequest),
    Authenticate(wire::AuthenticateRequest),
    Logout(wire::LogoutRequest),
}

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

enum Link {
    Sdk {
        connection: ConnectionTo<Agent>,
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

#[derive(Clone, Default)]
struct ToolState {
    title: String,
    status: ToolStatus,
    detail: String,
    kind: ToolKind,
    paths: Vec<String>,
}

enum Connection {
    Down,
    Connecting,
    Up(Link, Setup),
    Dropped,
    Lost(Lost),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Setup {
    Uninitialized,
    Initializing,
    Initialized,
    Opening { fork: bool },
    Configuring(usize),
    Ready,
}

impl Setup {
    fn initialized(self) -> bool {
        !matches!(self, Self::Uninitialized | Self::Initializing)
    }
}

struct Lost {
    error: String,
    at: Instant,
    initializing: bool,
    reported: bool,
}

enum AgentSession {
    New,
    Saved(String),
    Unrestorable(String),
    Replacing,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Waiting,
    Prompting(u64),
    Backoff(Instant),
    Cancelling(u64),
    Parked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Running,
    Closing,
    CloseSent,
    Forced,
}

enum ExitWatch {
    Idle,
    Until(Instant),
    Stuck,
}

impl ExitWatch {
    fn arm(&mut self) {
        if matches!(self, Self::Idle) {
            *self = Self::Until(Instant::now() + SHUTDOWN_GRACE);
        }
    }

    fn restart(&mut self) {
        if !matches!(self, Self::Stuck) {
            *self = Self::Until(Instant::now() + SHUTDOWN_GRACE);
        }
    }
}

struct Turn {
    prompt: Vec<Part>,
    retries: u32,
    attempt: u32,
    worked: bool,
    observed_text: String,
    denied: bool,
    replacement: Option<String>,
    step: Step,
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
            replacement: None,
            step: Step::Waiting,
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
    session: AgentSession,
    events: Sender<Event>,
    sender: UnboundedSender<Input>,
    receiver: UnboundedReceiver<Input>,
    child: Option<Child>,
    connection: Connection,
    epoch: u64,
    next_request: u64,
    next_permission: u64,
    pending: HashMap<u64, Request>,
    handshake: Option<(u64, Request, Instant)>,
    permissions: HashMap<u64, PendingPermission>,
    features: Option<Features>,
    raw_options: Vec<wire::SessionConfigOption>,
    modes: Option<wire::SessionModeState>,
    options: Vec<SessionOption>,
    tools: HashMap<String, ToolState>,
    active: Option<Turn>,
    turn: u64,
    exit: Exit,
    watch: ExitWatch,
    stderr: VecDeque<String>,
    pending_context: Option<String>,
    fork: Option<(Option<String>, String)>,
    probing: bool,
    attachments: std::cell::OnceCell<tempfile::TempDir>,
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
            session: session_id.map_or(AgentSession::New, AgentSession::Saved),
            events,
            sender,
            receiver,
            child: None,
            connection: Connection::Down,
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
            active: None,
            turn: 0,
            exit: Exit::Running,
            watch: ExitWatch::Idle,
            stderr: VecDeque::new(),
            pending_context: None,
            fork: None,
            probing: false,
            attachments: std::cell::OnceCell::new(),
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

    fn stopped_talking(&self) -> bool {
        matches!(self.connection, Connection::Dropped | Connection::Lost(_))
    }

    fn setup(&self) -> Option<Setup> {
        match self.connection {
            Connection::Up(_, setup) => Some(setup),
            _ => None,
        }
    }

    fn set_setup(&mut self, next: Setup) {
        if let Connection::Up(_, setup) = &mut self.connection {
            *setup = next;
        }
    }

    fn session_id(&self) -> Option<&str> {
        match &self.session {
            AgentSession::Saved(id) | AgentSession::Unrestorable(id) => Some(id),
            AgentSession::New | AgentSession::Replacing => None,
        }
    }

    fn end_turn(&mut self) {
        if let Some(context) = self.active.take().and_then(|turn| turn.replacement) {
            self.pending_context = Some(context);
        }
    }

    async fn run(mut self) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                Some(input) = self.receiver.recv() => self.input(input),
                _ = tick.tick() => {}
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
                self.connection = Connection::Up(link, Setup::Uninitialized);
                if self.exit != Exit::Running && self.active.is_none() {
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

    fn tick(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) if self.exit == Exit::Forced => {
                    self.child = None;
                    self.connection = Connection::Down;
                    self.emit(EventKind::ShutdownComplete);
                    return true;
                }
                Ok(Some(status)) => match &self.connection {
                    Connection::Lost(lost) if lost.at.elapsed() < LOST_WINDOW => {}
                    Connection::Lost(_) => {
                        if self.exited(status) {
                            return true;
                        }
                    }
                    _ => self.watch.arm(),
                },
                Err(error) => {
                    if let Connection::Up(..) = self.connection {
                        self.connection = Connection::Dropped;
                    }
                    self.emit(EventKind::Error {
                        message: format!("Checking agent process failed: {error}"),
                        kind: FailureKind::Temporary,
                    });
                    self.watch.arm();
                }
                Ok(None) => (),
            }
        } else if self.exit != Exit::Running {
            self.emit(EventKind::ShutdownComplete);
            return true;
        }
        if let ExitWatch::Until(deadline) = self.watch
            && Instant::now() >= deadline
        {
            self.watch = ExitWatch::Stuck;
            self.emit(EventKind::ShutdownStuck);
        }
        if self
            .handshake
            .is_some_and(|(_, _, deadline)| Instant::now() >= deadline)
        {
            self.handshake_timeout();
        }
        if self.exit == Exit::Running
            && !self.stopped_talking()
            && self.active.as_ref().is_some_and(
                |turn| matches!(turn.step, Step::Backoff(when) if Instant::now() >= when),
            )
        {
            self.retry_now();
        }
        false
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Prompt { prompt, retries } => {
                if self.exit != Exit::Running || self.active.is_some() {
                    self.emit(EventKind::Error {
                        message: "This conversation is already processing or shutting down.".into(),
                        kind: FailureKind::Configuration,
                    });
                } else if let AgentSession::Unrestorable(_) = self.session {
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
            Command::ReplaceSession { context } => self.replace_session(context),
            Command::Fork { session, context } => self.fork = Some((session, context)),
            Command::Authenticate { method } => self.authenticate(Some(method)),
            Command::Logout => self.authenticate(None),
            Command::Probe => {
                if self.exit == Exit::Running {
                    self.probing = true;
                    self.advance();
                }
            }
            Command::Shutdown => self.shutdown(),
            Command::ForceStop => self.force_stop(),
        }
    }

    fn advance(&mut self) {
        match self.connection {
            Connection::Down => self.start(),
            Connection::Up(_, Setup::Ready) if self.probing => self.emit(EventKind::Probed),
            Connection::Up(_, Setup::Ready) => self.send_prompt(),
            Connection::Up(_, Setup::Uninitialized) => self.initialize(),
            Connection::Up(_, Setup::Initialized) => self.setup_session(),
            _ => {}
        }
    }

    fn request(&mut self, outgoing: Outgoing, request: Request) -> Option<u64> {
        let Connection::Up(link, _) = &self.connection else {
            return None;
        };
        self.next_request += 1;
        let id = self.next_request;
        link.send(id, self.epoch, outgoing, &self.sender);
        self.pending.insert(id, request);
        if !matches!(
            request,
            Request::Prompt | Request::Close | Request::Steer | Request::Authenticate
        ) {
            self.handshake = Some((id, request, Instant::now() + HANDSHAKE_TIMEOUT));
        }
        Some(id)
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

    fn succeeded(&mut self, id: u64, request: Request, reply: Reply) {
        match (request, reply) {
            (Request::Initialize, Reply::Initialize(response)) => self.initialized(&response),
            (Request::Setup, Reply::Session(session)) => self.session_ready(session),
            (Request::Config | Request::Switch, reply) => {
                if let Reply::Options(options) = reply {
                    self.raw_options = options;
                    self.refresh_options();
                }
                if self.exit != Exit::Running {
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
                self.watch.arm();
            }
            (Request::Authenticate | Request::Logout, _) => {
                self.emit(EventKind::Auth(Ok(())));
                if self.active.is_some() && self.setup() == Some(Setup::Initialized) {
                    self.setup_session();
                }
            }
            (request, reply) => {
                tracing::event!(
                    name: "acp.reply.unexpected",
                    tracing::Level::WARN,
                    acp.request = ?request,
                    acp.reply = ?reply,
                    "Unexpected ACP reply to {{acp.request}}: {{acp.reply}}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
