//! One ACP agent process per conversation, spoken to through the official
//! `agent-client-protocol` SDK and its typed v1 messages. This is the only
//! module that knows ACP: it translates the protocol to and from Adeline's
//! own model (`conversation.rs`). Probes and logins are short-lived sessions
//! through the same client; dropping their `Session` stops the agent.
//!
//! Every line both ways, and the agent's stderr, becomes traffic; lines that
//! aren't JSON are kept as traffic and skipped. Session modes offered through
//! `session/set_mode` rather than as a config option appear as `MODE_OPTION`.
//! Usage is in tokens.
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
use failure::{broken_pipe, classify_error, looks_like_agent_error, with_context};
use permissions::respond;
pub use probe::{ProbeError, Probed, Session, login, probe};
use turns::tool_detail;

const MODE_OPTION: &str = "adeline/session-mode";

pub const CONTINUE_TURN: &str = "Continue the interrupted turn from the saved session. Preserve completed work; do not repeat completed tool actions.";

const STDERR_TAIL: usize = 12;
// How often the worker checks its child process, deadlines and retries.
const TICK: Duration = Duration::from_millis(100);
// Exit reports arriving this soon after a lost connection belong to it.
const LOST_WINDOW: Duration = Duration::from_millis(100);
// How long an exiting agent gets to drain its last messages before it's killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
// Setup requests (initialize, session, options) that take longer count as a hung agent.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
// Enough of a reply's start to spot an error the agent wrote as text.
const OBSERVED_TEXT: usize = 2048;
// Retry delays grow one second per attempt up to this many seconds.
const MAX_RETRY_DELAY_SECS: u32 = 5;

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
    // The agent speaks another ACP version.
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
    // The steering extension, which the schema has no type for.
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
        // Dropping or firing it ends the connection, which closes stdin.
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
    connecting: bool,
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
    setup: Option<Request>,
    configured: bool,
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
    // A fork's source session and history, until its first session is set up.
    fork: Option<(Option<String>, String)>,
    forking: bool,
    probing: bool,
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

    fn stopped_talking(&self) -> bool {
        self.child.is_some() && self.link.is_none() && !self.connecting
    }

    async fn run(mut self) {
        let mut tick = tokio::time::interval(TICK);
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

    fn tick(&mut self) -> bool {
        // Returns true once the worker is done.
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
                        .is_some_and(|when| when.elapsed() < LOST_WINDOW) => {}
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
                        .get_or_insert_with(|| Instant::now() + SHUTDOWN_GRACE);
                }
                Err(error) => {
                    self.link = None;
                    self.emit(EventKind::Error {
                        message: format!("Checking agent process failed: {error}"),
                        kind: FailureKind::Temporary,
                    });
                    self.shutdown_deadline
                        .get_or_insert_with(|| Instant::now() + SHUTDOWN_GRACE);
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
                self.shutdown_deadline = Some(Instant::now() + SHUTDOWN_GRACE);
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
                    .get_or_insert_with(|| Instant::now() + SHUTDOWN_GRACE);
            }
            (Request::Authenticate | Request::Logout, _) => {
                self.emit(EventKind::Auth(Ok(())));
                if self.active.is_some() && !self.configured && self.setup.is_none() {
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
