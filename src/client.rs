//! The UI's connections to the conversation engine of every checked machine,
//! as a GPUI global. The local engine is reached through its pipe or socket,
//! remote ones over SSH (`remote.rs`). Each connection's changes arrive in
//! order on its own channel; replies run their callbacks in that order.
use crate::{
    Adeline, harness, ipc,
    machines::{self, LOCAL},
    protocol::{
        ClientMessage, Command, Delta, EngineMessage, EngineSettings, PROTOCOL, Resume, Snapshot,
        Status,
    },
    remote,
};
use gpui_kit::{App, Global, WeakEntity};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{io::AsyncWriteExt as _, sync::mpsc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Demo,
    Connecting,
    Starting,
    Connected,
    /// An engine with another protocol version is running.
    Mismatch(Status),
    /// Waiting for that engine to exit.
    Waiting,
    Stopped {
        unexpected: bool,
    },
    /// No engine, and retrying won't help until the user acts.
    Unavailable(String),
    /// A remote connection dropped or couldn't be made; retrying.
    Disconnected(String),
    SignInFailed(String),
    /// An older engine runs on the remote machine.
    UpgradeNeeded {
        version: String,
        active: usize,
    },
    /// The remote machine runs a newer Adeline.
    LocalUpdateNeeded(String),
    Unsupported(String),
}

impl State {
    /// How the machine selector names the state.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Demo | Self::Connected => "Connected",
            Self::Connecting | Self::Starting | Self::Waiting => "Connecting…",
            Self::Mismatch(_)
            | Self::Stopped { .. }
            | Self::Unavailable(_)
            | Self::Disconnected(_) => "Disconnected",
            Self::SignInFailed(_) => "Sign-in failed",
            Self::UpgradeNeeded { .. } => "Upgrade needed",
            Self::LocalUpdateNeeded(_) => "Local update needed",
            Self::Unsupported(_) => "Unsupported",
        }
    }

    /// The error or explanation behind the state.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Unavailable(error)
            | Self::Disconnected(error)
            | Self::SignInFailed(error)
            | Self::Unsupported(error) => Some(error.clone()),
            Self::Mismatch(status) => Some(format!(
                "An older engine (version {}) is still running.",
                status.version
            )),
            Self::Stopped { .. } => Some("The conversation engine is stopped.".into()),
            Self::UpgradeNeeded { version, active } => Some(format!(
                "It runs Adeline {version} with {active} active conversation{}.",
                if *active == 1 { "" } else { "s" }
            )),
            Self::LocalUpdateNeeded(version) => Some(format!(
                "It runs Adeline {version}. Update Adeline on this computer to {version} or later."
            )),
            _ => None,
        }
    }

    pub fn usable(&self) -> bool {
        matches!(self, Self::Demo | Self::Connected)
    }
}

pub type Callback = Box<dyn FnOnce(Result<Value, String>, &mut App)>;

type Slot = Arc<Mutex<Option<(u64, std::sync::mpsc::Sender<Result<Value, String>>)>>>;

pub struct Connection {
    pub machine: String,
    pub state: State,
    pub status: Status,
    /// When `status` arrived, to count uptime from.
    pub status_at: Instant,
    pub settings: EngineSettings,
    pub harnesses: harness::Catalog,
    /// A snapshot has arrived, so there is data to show while reconnecting.
    pub loaded: bool,
    /// The destination a remote machine is connected through.
    pub destination: Option<String>,
    sender: Option<mpsc::UnboundedSender<String>>,
    inbox: Option<async_channel::Receiver<Incoming>>,
    next: u64,
    pending: HashMap<u64, Callback>,
    queued: Vec<(u64, Command)>,
    blocking: Slot,
    epoch: u64,
    bye: bool,
    /// The engine's delta stream as far as this client has it.
    stream: Option<Resume>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    retries: u32,
    /// The user agreed to restart an older remote engine.
    upgrade: bool,
}

impl Connection {
    fn new(machine: &str) -> Self {
        Self {
            machine: machine.to_owned(),
            state: State::Connecting,
            status: Status::default(),
            status_at: Instant::now(),
            settings: EngineSettings::default(),
            harnesses: harness::Catalog::default(),
            loaded: false,
            destination: None,
            sender: None,
            inbox: None,
            next: 0,
            pending: HashMap::new(),
            queued: Vec::new(),
            blocking: Arc::default(),
            epoch: 0,
            bye: false,
            stream: None,
            cancel: None,
            retries: 0,
            upgrade: false,
        }
    }

    fn local(&self) -> bool {
        self.machine == LOCAL
    }
}

/// A password typed during one connect, reused by the next `ssh` session of
/// the same connect and forgotten when it ends.
struct Secret {
    prompt: String,
    answer: String,
    sessions: Vec<u64>,
}

pub struct Connections {
    list: Vec<Connection>,
    owner: Option<WeakEntity<Adeline>>,
    prompts: VecDeque<remote::Prompt>,
    askpass: String,
    secrets: HashMap<String, Vec<Secret>>,
    /// The machine whose harnesses are the `harness::Catalog` global.
    focus: String,
    demo: bool,
}

impl Global for Connections {}

#[expect(
    clippy::large_enum_variant,
    reason = "each message is moved once, straight to its handler"
)]
enum Incoming {
    Starting,
    Ready(mpsc::UnboundedSender<String>),
    /// A remote engine welcomed this client through `destination`.
    Linked {
        destination: String,
        status: Status,
    },
    Message(EngineMessage),
    Closed,
    Failed(String),
    Remote(remote::Failure),
    OldGone,
}

/// The connections' own small tokio runtime; GPUI keeps its executors.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("adeline-engine-client")
            .enable_all()
            .build()
            .expect("start the engine client runtime")
    })
}

/// A connection to an already running local engine, made before the window
/// opens so the project list shows without delay.
pub struct Initial {
    reader: ipc::Reader,
    writer: ipc::Writer,
    pub status: Status,
    pub snapshot: Option<Snapshot>,
}

fn hello(resume: Option<Resume>) -> ClientMessage {
    ClientMessage::Hello {
        protocol: PROTOCOL,
        cli: false,
        resume,
    }
}

pub fn connect_existing() -> Option<Initial> {
    runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            let (mut reader, mut writer) = ipc::connect().await.ok()?;
            ipc::send(&mut writer, &hello(None)).await.ok()?;
            let mut welcome = None;
            while let Ok(Some(line)) = reader.next_line().await {
                match serde_json::from_str(&line).ok()? {
                    EngineMessage::Welcome { status } if status.protocol != PROTOCOL => {
                        return Some(Initial {
                            reader,
                            writer,
                            status,
                            snapshot: None,
                        });
                    }
                    EngineMessage::Welcome { status } => welcome = Some(status),
                    EngineMessage::Snapshot(snapshot) => {
                        return Some(Initial {
                            reader,
                            writer,
                            status: welcome?,
                            snapshot: Some(*snapshot),
                        });
                    }
                    _ => {}
                }
            }
            None
        })
        .await
        .ok()
        .flatten()
    })
}

/// Sets up the global before the window exists and connects every checked
/// machine. `initial` is a local connection made already; `snapshot` is the
/// one taken out of it.
pub fn init(demo: bool, initial: Option<Initial>, snapshot: Option<&Snapshot>, cx: &mut App) {
    let mut connections = Connections {
        list: Vec::new(),
        owner: None,
        prompts: VecDeque::new(),
        askpass: String::new(),
        secrets: HashMap::new(),
        focus: LOCAL.into(),
        demo,
    };
    if demo {
        let harnesses = cx.global::<harness::Catalog>().clone();
        for machine in machines::checked() {
            let mut connection = Connection::new(&machine);
            // One demo machine shows what a dropped connection looks like.
            connection.state = if machine == "vortex" {
                State::Disconnected("Demo machine offline.".into())
            } else {
                State::Demo
            };
            connection.loaded = true;
            connection.harnesses = harnesses.clone();
            connections.list.push(connection);
        }
        cx.set_global(connections);
        return;
    }
    let (prompts, inbox) = async_channel::unbounded();
    match runtime().block_on(async { remote::serve_askpass(prompts) }) {
        Ok(address) => connections.askpass = address,
        Err(error) => crate::engine::log(&error),
    }
    cx.spawn(async move |cx| {
        while let Ok(prompt) = inbox.recv().await {
            cx.update(|cx| on_prompt(prompt, cx));
        }
    })
    .detach();
    cx.set_global(connections);
    for machine in machines::checked() {
        cx.global_mut::<Connections>()
            .list
            .push(Connection::new(&machine));
        if machine != LOCAL {
            connect(&machine, cx);
        }
    }
    if !machines::is_checked(LOCAL) {
        return;
    }
    let local = get_mut(LOCAL, cx).expect("local connection");
    if let Some(initial) = &initial {
        local.status = initial.status.clone();
        if let Some(snapshot) = snapshot {
            local.state = State::Connected;
            local.loaded = true;
            local.settings = snapshot.settings.clone();
            local.harnesses = snapshot.harnesses.clone();
            local.stream = Some(Resume {
                epoch: snapshot.epoch.clone(),
                seq: snapshot.seq,
            });
            harness::set_icons(&snapshot.icons);
            cx.set_global(snapshot.harnesses.clone());
        } else {
            local.state = State::Mismatch(initial.status.clone());
        }
    }
    // Messages wait in the inbox until the window's entity is set as owner.
    match initial {
        Some(Initial { reader, writer, .. }) => {
            let (incoming, epoch, cancel) = open_inbox(LOCAL, cx);
            let blocking = get(LOCAL, cx).expect("local connection").blocking.clone();
            runtime().spawn(run_io(reader, writer, incoming, blocking, cancel, None));
            listen(LOCAL, epoch, cx);
        }
        None => connect(LOCAL, cx),
    }
}

pub fn set_owner(owner: WeakEntity<Adeline>, cx: &mut App) {
    cx.global_mut::<Connections>().owner = Some(owner);
}

pub fn connection<'a>(machine: &str, cx: &'a App) -> Option<&'a Connection> {
    get(machine, cx)
}

fn get<'a>(machine: &str, cx: &'a App) -> Option<&'a Connection> {
    cx.global::<Connections>()
        .list
        .iter()
        .find(|c| c.machine == machine)
}

fn get_mut<'a>(machine: &str, cx: &'a mut App) -> Option<&'a mut Connection> {
    cx.global_mut::<Connections>()
        .list
        .iter_mut()
        .find(|c| c.machine == machine)
}

pub fn state<'a>(machine: &str, cx: &'a App) -> Option<&'a State> {
    get(machine, cx).map(|c| &c.state)
}

/// Whether a machine's projects take actions now.
pub fn usable(machine: &str, cx: &App) -> bool {
    state(machine, cx).is_some_and(State::usable)
}

/// Every connection, in machine order.
pub fn connections(cx: &App) -> &[Connection] {
    &cx.global::<Connections>().list
}

/// Connects newly checked machines and drops unchecked ones. Runs after the
/// current update, so callers may be updating the window.
pub fn sync(cx: &mut App) {
    cx.defer(sync_now);
}

fn sync_now(cx: &mut App) {
    let checked = machines::checked();
    let gone: Vec<String> = connections(cx)
        .iter()
        .map(|c| c.machine.clone())
        .filter(|machine| !checked.contains(machine))
        .collect();
    for machine in &gone {
        if let Some(connection) = get_mut(machine, cx) {
            connection.epoch += 1;
            connection.cancel = None;
            connection.sender = None;
        }
        fail_pending(machine, "The machine was unchecked.", cx);
        cx.global_mut::<Connections>()
            .list
            .retain(|c| c.machine != *machine);
        cx.global_mut::<Connections>().secrets.remove(machine);
        let machine = machine.clone();
        update_owner(cx, move |app, cx| app.machine_unchecked(&machine, cx));
    }
    let demo = cx.global::<Connections>().demo;
    for machine in checked {
        if get(&machine, cx).is_some() {
            continue;
        }
        let mut connection = Connection::new(&machine);
        if demo {
            connection.state = State::Demo;
            connection.loaded = true;
            connection.harnesses = cx.global::<harness::Catalog>().clone();
            cx.global_mut::<Connections>().list.push(connection);
            let machine = machine.clone();
            update_owner(cx, move |app, cx| app.machine_checked(&machine, cx));
        } else {
            cx.global_mut::<Connections>().list.push(connection);
            connect(&machine, cx);
        }
    }
    let order = machines::all();
    cx.global_mut::<Connections>()
        .list
        .sort_by_key(|c| order.iter().position(|m| *m == c.machine));
    cx.refresh_windows();
}

fn open_inbox(
    machine: &str,
    cx: &mut App,
) -> (
    async_channel::Sender<Incoming>,
    u64,
    tokio::sync::oneshot::Receiver<()>,
) {
    let (sender, receiver) = async_channel::unbounded();
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let connection = get_mut(machine, cx).expect("connection");
    connection.epoch += 1;
    connection.inbox = Some(receiver);
    connection.sender = None;
    connection.bye = false;
    connection.cancel = Some(cancel);
    (sender, connection.epoch, cancelled)
}

fn listen(machine: &str, epoch: u64, cx: &mut App) {
    let Some(inbox) = get(machine, cx).and_then(|c| c.inbox.clone()) else {
        return;
    };
    let machine = machine.to_owned();
    cx.spawn(async move |cx| {
        while let Ok(incoming) = inbox.recv().await {
            cx.update(|cx| handle(&machine, epoch, incoming, cx));
        }
    })
    .detach();
}

/// Connects a machine again. The local engine starts when none is running;
/// a remote one starts through `adeline bridge`.
pub fn connect(machine: &str, cx: &mut App) {
    if get(machine, cx).is_none() || cx.global::<Connections>().demo {
        return;
    }
    let (incoming, epoch, cancel) = open_inbox(machine, cx);
    let connection = get_mut(machine, cx).expect("connection");
    let blocking = connection.blocking.clone();
    connection.state = State::Connecting;
    if machine == LOCAL {
        runtime().spawn(async move {
            let connection = if let Ok(connection) = ipc::connect().await {
                Ok(connection)
            } else {
                let _ = incoming.send(Incoming::Starting).await;
                ipc::connect_or_start(false).await
            };
            match connection {
                Ok((reader, mut writer)) => match ipc::send(&mut writer, &hello(None)).await {
                    Ok(()) => run_io(reader, writer, incoming, blocking, cancel, None).await,
                    Err(error) => {
                        let _ = incoming.send(Incoming::Failed(error.to_string())).await;
                    }
                },
                Err(error) => {
                    let _ = incoming.send(Incoming::Failed(error)).await;
                }
            }
        });
    } else {
        let Some(saved) = machines::remote(machine) else {
            return;
        };
        let target = remote::Remote {
            name: saved.name,
            destinations: saved.destinations,
            engine: saved.engine,
        };
        let resume = connection.stream.clone().filter(|_| connection.loaded);
        let upgrade = std::mem::take(&mut connection.upgrade);
        let askpass = remote::Askpass {
            address: cx.global::<Connections>().askpass.clone(),
            machine: machine.to_owned(),
        };
        runtime().spawn(async move {
            match remote::connect(&target, &hello(resume), upgrade, &askpass).await {
                Ok(link) => {
                    let linked = Incoming::Linked {
                        destination: link.destination,
                        status: link.status,
                    };
                    if incoming.send(linked).await.is_ok() {
                        run_io(
                            link.reader,
                            link.writer,
                            incoming,
                            blocking,
                            cancel,
                            Some(link.child),
                        )
                        .await;
                    }
                }
                Err(failure) => {
                    let _ = incoming.send(Incoming::Remote(failure)).await;
                }
            }
        });
    }
    listen(machine, epoch, cx);
    cx.refresh_windows();
}

/// Connects a machine the user asked to retry, after any failure.
pub fn retry(machine: &str, cx: &mut App) {
    if let Some(connection) = get_mut(machine, cx) {
        connection.retries = 0;
        connect(machine, cx);
    }
}

/// The user agreed to restart an older remote engine, stopping its agents.
pub fn upgrade(machine: &str, cx: &mut App) {
    if let Some(connection) = get_mut(machine, cx) {
        connection.upgrade = true;
        connection.retries = 0;
        connect(machine, cx);
    }
}

async fn run_io(
    mut reader: ipc::Reader,
    mut writer: ipc::Writer,
    incoming: async_channel::Sender<Incoming>,
    blocking: Slot,
    cancel: tokio::sync::oneshot::Receiver<()>,
    // The `ssh` process lives as long as the connection.
    _child: Option<tokio::process::Child>,
) {
    let (sender, mut outgoing) = mpsc::unbounded_channel::<String>();
    let _ = incoming.send(Incoming::Ready(sender)).await;
    tokio::spawn(async move {
        while let Some(line) = outgoing.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
    });
    let read = async {
        while let Ok(Some(line)) = reader.next_line().await {
            let Ok(message) = serde_json::from_str::<EngineMessage>(&line) else {
                continue;
            };
            // A blocking request's reply skips the queue; everything before it is queued.
            if let EngineMessage::Reply { id, result } = &message
                && let Ok(mut slot) = blocking.lock()
                && slot.as_ref().is_some_and(|(waiting, _)| waiting == id)
                && let Some((_, reply)) = slot.take()
            {
                let _ = reply.send(result.clone());
                continue;
            }
            if incoming.send(Incoming::Message(message)).await.is_err() {
                return false;
            }
        }
        true
    };
    // A dropped cancel sender (an unchecked machine) ends the connection too.
    tokio::select! {
        closed = read => if closed {
            let _ = incoming.send(Incoming::Closed).await;
        },
        _ = cancel => {}
    }
}

fn update_owner(
    cx: &mut App,
    change: impl FnOnce(&mut Adeline, &mut gpui_kit::Context<Adeline>) + 'static,
) {
    if let Some(owner) = cx.global::<Connections>().owner.clone() {
        let _ = owner.update(cx, change);
    }
}

fn fail_pending(machine: &str, error: &str, cx: &mut App) {
    let Some(connection) = get_mut(machine, cx) else {
        return;
    };
    let pending: Vec<_> = connection.pending.drain().map(|(_, done)| done).collect();
    connection.queued.clear();
    for done in pending {
        done(Err(error.to_owned()), cx);
    }
}

fn flush_queued(machine: &str, cx: &mut App) {
    if let Some(connection) = get_mut(machine, cx)
        && let Some(sender) = &connection.sender
    {
        for (id, command) in connection.queued.drain(..) {
            send_line(sender, id, command);
        }
    }
}

/// Tries a dropped remote machine again after a growing delay.
fn schedule_retry(machine: &str, cx: &mut App) {
    let Some(connection) = get_mut(machine, cx) else {
        return;
    };
    connection.retries += 1;
    let delay = Duration::from_secs(1 << connection.retries.min(5)).min(Duration::from_secs(30));
    let epoch = connection.epoch;
    let machine = machine.to_owned();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(delay).await;
        cx.update(|cx| {
            if get(&machine, cx)
                .is_some_and(|c| c.epoch == epoch && matches!(c.state, State::Disconnected(_)))
            {
                connect(&machine, cx);
            }
        });
    })
    .detach();
}

/// A new remote machine's engine: saved with it, unless another saved
/// machine already has it.
fn adopt_engine(machine: &str, engine: &str, cx: &mut App) -> Result<(), String> {
    let saved = machines::remote(machine).and_then(|m| m.engine);
    if saved.is_some() || engine.is_empty() {
        return Ok(());
    }
    let local = get(LOCAL, cx).map(|c| c.status.engine_id.clone());
    let existing = machines::with_engine(engine, machine)
        .map(|m| m.name)
        .or_else(|| (local.as_deref() == Some(engine)).then(|| machines::name(LOCAL)));
    if let Some(existing) = existing {
        return Err(format!(
            "This machine is already saved as \"{existing}\". Add its destination to \"{existing}\" instead."
        ));
    }
    machines::edit(machine, |m| {
        m.engine = Some(engine.to_owned());
        Ok(())
    })
}

fn set_status(machine: &str, status: Status, cx: &mut App) {
    if let Some(connection) = get_mut(machine, cx) {
        connection.status = status;
        connection.status_at = Instant::now();
    }
}

fn set_harnesses(machine: &str, catalog: harness::Catalog, cx: &mut App) {
    if cx.global::<Connections>().focus == machine {
        cx.set_global(catalog.clone());
    }
    if let Some(connection) = get_mut(machine, cx) {
        connection.harnesses = catalog;
    }
}

fn handle(machine: &str, epoch: u64, incoming: Incoming, cx: &mut App) {
    if get(machine, cx).is_none_or(|c| c.epoch != epoch) {
        return;
    }
    let local = machine == LOCAL;
    match incoming {
        Incoming::Starting => {
            get_mut(machine, cx).expect("connection").state = State::Starting;
            cx.refresh_windows();
        }
        Incoming::Ready(sender) => get_mut(machine, cx).expect("connection").sender = Some(sender),
        Incoming::Linked {
            destination,
            status,
        } => {
            cx.global_mut::<Connections>().secrets.remove(machine);
            if let Err(error) = adopt_engine(machine, &status.engine_id, cx) {
                let connection = get_mut(machine, cx).expect("connection");
                connection.epoch += 1;
                connection.cancel = None;
                connection.state = State::Unavailable(error.clone());
                let _ = machines::remove(machine);
                update_owner(cx, move |app, cx| app.notify_toast(&error, cx));
                sync_now(cx);
                return;
            }
            get_mut(machine, cx).expect("connection").destination = Some(destination);
            set_status(machine, status, cx);
            cx.refresh_windows();
        }
        Incoming::Message(EngineMessage::Welcome { status }) => {
            let mismatch = status.protocol != PROTOCOL;
            set_status(machine, status.clone(), cx);
            if mismatch {
                get_mut(machine, cx).expect("connection").state = State::Mismatch(status);
                cx.refresh_windows();
            }
        }
        Incoming::Message(EngineMessage::Snapshot(snapshot)) => {
            let connection = get_mut(machine, cx).expect("connection");
            connection.state = State::Connected;
            connection.loaded = true;
            connection.retries = 0;
            connection.settings = snapshot.settings.clone();
            connection.stream = Some(Resume {
                epoch: snapshot.epoch.clone(),
                seq: snapshot.seq,
            });
            set_status(machine, snapshot.status.clone(), cx);
            harness::set_icons(&snapshot.icons);
            set_harnesses(machine, snapshot.harnesses.clone(), cx);
            let target = machine.to_owned();
            update_owner(cx, move |app, cx| {
                app.apply_snapshot(&target, *snapshot, cx);
            });
            flush_queued(machine, cx);
            cx.refresh_windows();
        }
        Incoming::Message(EngineMessage::Resumed) => {
            let connection = get_mut(machine, cx).expect("connection");
            connection.state = State::Connected;
            connection.retries = 0;
            flush_queued(machine, cx);
            let target = machine.to_owned();
            update_owner(cx, move |app, cx| app.machine_resumed(&target, cx));
            cx.refresh_windows();
        }
        Incoming::Message(EngineMessage::Delta(delta)) => {
            if let Some(stream) = &mut get_mut(machine, cx).expect("connection").stream {
                stream.seq += 1;
            }
            match delta {
                Delta::EngineStatus(status) => {
                    set_status(machine, status, cx);
                    cx.refresh_windows();
                }
                Delta::Settings(settings) => {
                    get_mut(machine, cx).expect("connection").settings = settings;
                    cx.refresh_windows();
                }
                Delta::Harnesses(catalog) => {
                    set_harnesses(machine, catalog, cx);
                    cx.refresh_windows();
                }
                Delta::Icons(icons) => {
                    harness::set_icons(&icons);
                    cx.refresh_windows();
                }
                delta => {
                    let target = machine.to_owned();
                    update_owner(cx, move |app, cx| app.apply_delta(&target, delta, cx));
                }
            }
        }
        Incoming::Message(EngineMessage::Reply { id, result }) => {
            let done = get_mut(machine, cx)
                .expect("connection")
                .pending
                .remove(&id);
            if let Some(done) = done {
                done(result, cx);
            }
        }
        Incoming::Message(EngineMessage::Bye) => {
            get_mut(machine, cx).expect("connection").bye = true;
        }
        Incoming::Closed => {
            let connection = get_mut(machine, cx).expect("connection");
            connection.sender = None;
            connection.cancel = None;
            if matches!(connection.state, State::Waiting | State::Mismatch(_)) {
                return;
            }
            if !local && !connection.bye {
                // The engine may still be running: keep what was received, and catch up later.
                connection.state = State::Disconnected("The connection was lost.".into());
                let error = format!("{} disconnected.", machines::name(machine));
                fail_pending(machine, &error, cx);
                schedule_retry(machine, cx);
                let target = machine.to_owned();
                update_owner(cx, move |app, cx| app.refresh_machine(&target, cx));
                cx.refresh_windows();
                return;
            }
            let unexpected = !connection.bye;
            connection.state = State::Stopped { unexpected };
            fail_pending(machine, "The conversation engine stopped.", cx);
            let target = machine.to_owned();
            update_owner(cx, move |app, cx| {
                app.engine_stopped(&target, unexpected, cx);
            });
            cx.refresh_windows();
        }
        Incoming::Failed(error) => {
            get_mut(machine, cx).expect("connection").state = State::Unavailable(error.clone());
            fail_pending(machine, &error, cx);
            cx.refresh_windows();
        }
        Incoming::Remote(failure) => {
            cx.global_mut::<Connections>().secrets.remove(machine);
            let retry = matches!(failure, remote::Failure::Network(_));
            let upgrade = matches!(failure, remote::Failure::Upgrade { .. });
            let state = match failure {
                remote::Failure::Network(error) => State::Disconnected(error),
                remote::Failure::SignIn(error) => State::SignInFailed(error),
                remote::Failure::Fatal(error) => State::Unavailable(error),
                remote::Failure::Unsupported(error) => State::Unsupported(error),
                remote::Failure::LocalUpdate(version) => State::LocalUpdateNeeded(version),
                remote::Failure::Upgrade { version, active } => {
                    State::UpgradeNeeded { version, active }
                }
            };
            get_mut(machine, cx).expect("connection").state = state;
            let error = format!("{} is not connected.", machines::name(machine));
            fail_pending(machine, &error, cx);
            if retry {
                schedule_retry(machine, cx);
            }
            let target = machine.to_owned();
            update_owner(cx, move |app, cx| {
                if upgrade {
                    app.ask_upgrade(&target, cx);
                }
                app.refresh_machine(&target, cx);
            });
            cx.refresh_windows();
        }
        Incoming::OldGone => connect(LOCAL, cx),
    }
}

fn send_line(sender: &mpsc::UnboundedSender<String>, id: u64, command: Command) {
    if let Ok(mut line) = serde_json::to_string(&ClientMessage::Request { id, command }) {
        line.push('\n');
        let _ = sender.send(line);
    }
}

/// Why a machine can't take a command now, or `None` when it can.
fn refusal(connection: &Connection) -> Option<String> {
    let name = machines::name(&connection.machine);
    match &connection.state {
        State::Demo => Some("Demo mode doesn't run the conversation engine.".to_owned()),
        State::Unavailable(error) if connection.local() => {
            Some(format!("The conversation engine is unavailable: {error}"))
        }
        State::Mismatch(_) | State::Waiting => {
            Some("An older conversation engine is still running.".to_owned())
        }
        State::Unavailable(_)
        | State::Disconnected(_)
        | State::SignInFailed(_)
        | State::UpgradeNeeded { .. }
        | State::LocalUpdateNeeded(_)
        | State::Unsupported(_) => Some(format!("{name} is not connected.")),
        _ => None,
    }
}

/// Sends a command to a machine's engine; `done` gets the reply. A stopped
/// engine is started again for it.
pub fn request(machine: &str, command: Command, done: Callback, cx: &mut App) {
    let Some(connection) = get_mut(machine, cx) else {
        let error = format!("{} is not checked.", machines::name(machine));
        cx.defer(move |cx| done(Err(error), cx));
        return;
    };
    if let Some(error) = refusal(connection) {
        // Callers may be mid-update; their reply comes after it, like any other.
        cx.defer(move |cx| done(Err(error), cx));
        return;
    }
    connection.next += 1;
    let id = connection.next;
    connection.pending.insert(id, done);
    match (&connection.state, &connection.sender) {
        (State::Connected, Some(sender)) => send_line(sender, id, command),
        (State::Stopped { .. }, _) => {
            connection.queued.push((id, command));
            connect(machine, cx);
        }
        _ => connection.queued.push((id, command)),
    }
}

/// Sends a command and waits for its reply, applying every change the engine
/// sent before it. Never call this while updating the `Adeline` entity.
pub fn request_blocking(machine: &str, command: Command, cx: &mut App) -> Result<Value, String> {
    let name = machines::name(machine);
    let Some(connection) = get_mut(machine, cx) else {
        return Err(format!("{name} is not checked."));
    };
    let (State::Connected, Some(sender)) = (&connection.state, &connection.sender) else {
        return Err(if connection.local() {
            "The conversation engine isn't running. Start it in Settings › Engine.".into()
        } else {
            format!("{name} is not connected.")
        });
    };
    connection.next += 1;
    let id = connection.next;
    let (reply, replies) = std::sync::mpsc::channel();
    if let Ok(mut slot) = connection.blocking.lock() {
        *slot = Some((id, reply));
    }
    send_line(sender, id, command);
    // Remote machines answer over the network.
    let wait = if machine == LOCAL { 10 } else { 30 };
    let result = replies
        .recv_timeout(Duration::from_secs(wait))
        .unwrap_or_else(|_| Err("The conversation engine didn't answer in time.".into()));
    let connection = get(machine, cx).expect("connection");
    if let Ok(mut slot) = connection.blocking.lock() {
        *slot = None;
    }
    let epoch = connection.epoch;
    if let Some(inbox) = connection.inbox.clone() {
        while let Ok(incoming) = inbox.try_recv() {
            handle(machine, epoch, incoming, cx);
        }
    }
    result
}

/// Waits for an older local engine to exit, then starts this version's
/// engine. With `stop`, asks it to stop its agents and exit first.
pub fn replace_old(stop: bool, cx: &mut App) {
    let Some(connection) = get_mut(LOCAL, cx) else {
        return;
    };
    if stop && let Some(sender) = &connection.sender {
        send_line(sender, 0, Command::Shutdown);
    }
    connection.state = State::Waiting;
    let epoch = connection.epoch;
    let (gone, inbox) = async_channel::unbounded();
    runtime().spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if ipc::connect().await.is_err() {
                let _ = gone.send(Incoming::OldGone).await;
                return;
            }
        }
    });
    cx.spawn(async move |cx| {
        if let Ok(incoming) = inbox.recv().await {
            cx.update(|cx| handle(LOCAL, epoch, incoming, cx));
        }
    })
    .detach();
    cx.refresh_windows();
}

/// Starts a machine's engine again after it was stopped, for an action that needs it.
pub fn ensure(machine: &str, cx: &mut App) {
    if matches!(state(machine, cx), Some(State::Stopped { .. })) {
        connect(machine, cx);
    }
}

pub fn refresh_harnesses(machine: &str, fetch: bool, cx: &mut App) {
    if matches!(state(machine, cx), Some(State::Connected)) {
        request(
            machine,
            Command::RefreshHarnesses { fetch },
            Box::new(|_, _| {}),
            cx,
        );
    }
}

/// Makes `machine`'s harnesses the `harness::Catalog` global, for the agent
/// pages showing that machine.
pub fn focus(machine: &str, cx: &mut App) {
    let connections = cx.global_mut::<Connections>();
    if connections.focus == machine {
        return;
    }
    machine.clone_into(&mut connections.focus);
    if let Some(catalog) = get(machine, cx).map(|c| c.harnesses.clone())
        && !cx.global::<Connections>().demo
    {
        cx.set_global(catalog);
    }
}

// ---------------------------------------------------------------------------
// SSH prompts

fn on_prompt(prompt: remote::Prompt, cx: &mut App) {
    let connections = cx.global_mut::<Connections>();
    // An earlier session of this connect already had this answer.
    if !prompt.host_key()
        && let Some(secret) = connections
            .secrets
            .get_mut(&prompt.machine)
            .and_then(|secrets| secrets.iter_mut().find(|s| s.prompt == prompt.text))
        && !secret.sessions.contains(&prompt.session)
    {
        secret.sessions.push(prompt.session);
        let _ = prompt.reply.try_send(Some(secret.answer.clone()));
        return;
    }
    connections.prompts.push_back(prompt);
    update_owner(cx, |app, cx| app.show_prompt(cx));
    cx.refresh_windows();
}

/// The `ssh` prompt waiting for the user, if any: machine and text.
pub fn prompt(cx: &App) -> Option<(String, String, bool)> {
    cx.global::<Connections>()
        .prompts
        .front()
        .map(|p| (p.machine.clone(), p.text.clone(), p.host_key()))
}

/// Answers the waiting prompt; `None` cancels it.
pub fn answer_prompt(answer: Option<String>, cx: &mut App) {
    let connections = cx.global_mut::<Connections>();
    let Some(prompt) = connections.prompts.pop_front() else {
        return;
    };
    if let Some(answer) = &answer
        && !prompt.host_key()
    {
        let secrets = connections
            .secrets
            .entry(prompt.machine.clone())
            .or_default();
        secrets.retain(|s| s.prompt != prompt.text);
        secrets.push(Secret {
            prompt: prompt.text.clone(),
            answer: answer.clone(),
            sessions: vec![prompt.session],
        });
    }
    let _ = prompt.reply.try_send(answer);
    cx.refresh_windows();
}

// ---------------------------------------------------------------------------
// Probes

/// A probe running in an engine. Dropping it stops the probe.
pub struct Probe {
    id: u64,
    sender: Option<mpsc::UnboundedSender<String>>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        if let Some(sender) = &self.sender {
            send_line(sender, 0, Command::CancelProbe { probe: self.id });
        }
    }
}

pub fn probe(
    machine: &str,
    harness: String,
    command: String,
    arguments: Vec<String>,
    model: Option<String>,
    cx: &mut App,
) -> (
    Probe,
    async_channel::Receiver<Result<harness::Probed, harness::ProbeError>>,
) {
    let (probe, sender) = match get_mut(machine, cx) {
        Some(connection) => {
            connection.next += 1;
            (connection.next, connection.sender.clone())
        }
        None => (0, None),
    };
    let handle = Probe { id: probe, sender };
    let (results, receiver) = async_channel::bounded(1);
    request(
        machine,
        Command::Probe {
            probe,
            harness,
            command,
            arguments,
            model,
        },
        Box::new(move |result, _| {
            let result = result
                .and_then(|value| serde_json::from_value(value).map_err(|e| e.to_string()))
                .unwrap_or_else(|message| {
                    Err(harness::ProbeError {
                        message,
                        login: Vec::new(),
                    })
                });
            let _ = results.try_send(result);
        }),
        cx,
    );
    (handle, receiver)
}
