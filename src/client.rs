//! The UI's connection to the conversation engine, as a GPUI global. Changes
//! arrive in order on one channel; replies run their callbacks in that order.
use crate::{
    Adeline, harness, ipc,
    protocol::{
        ClientMessage, Command, Delta, EngineMessage, EngineSettings, PROTOCOL, Snapshot, Status,
    },
};
use gpui_kit::{App, Global, WeakEntity};
use serde_json::Value;
use std::{
    collections::HashMap,
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
    Unavailable(String),
}

pub type Callback = Box<dyn FnOnce(Result<Value, String>, &mut App)>;

type Slot = Arc<Mutex<Option<(u64, std::sync::mpsc::Sender<Result<Value, String>>)>>>;

pub struct Connection {
    pub state: State,
    pub status: Status,
    /// When `status` arrived, to count uptime from.
    pub status_at: Instant,
    pub settings: EngineSettings,
    /// A snapshot has arrived, so there is data to show while reconnecting.
    pub loaded: bool,
    owner: Option<WeakEntity<Adeline>>,
    sender: Option<mpsc::UnboundedSender<String>>,
    inbox: Option<async_channel::Receiver<Incoming>>,
    next: u64,
    pending: HashMap<u64, Callback>,
    queued: Vec<(u64, Command)>,
    blocking: Slot,
    epoch: u64,
    bye: bool,
}

impl Global for Connection {}

#[expect(
    clippy::large_enum_variant,
    reason = "each message is moved once, straight to its handler"
)]
enum Incoming {
    Starting,
    Ready(mpsc::UnboundedSender<String>),
    Message(EngineMessage),
    Closed,
    Failed(String),
    OldGone,
}

/// The connection's own small tokio runtime; GPUI keeps its executors.
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

/// A connection to an already running engine, made before the window opens
/// so the project list shows without delay.
pub struct Initial {
    reader: ipc::Reader,
    writer: ipc::Writer,
    pub status: Status,
    pub snapshot: Option<Snapshot>,
}

fn hello() -> ClientMessage {
    ClientMessage::Hello {
        protocol: PROTOCOL,
        cli: false,
    }
}

pub fn connect_existing() -> Option<Initial> {
    runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            let (mut reader, mut writer) = ipc::connect().await.ok()?;
            ipc::send(&mut writer, &hello()).await.ok()?;
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

/// Sets up the global before the window exists. Without `initial`, finds or
/// starts an engine. `snapshot` is the one taken out of `initial`.
pub fn init(demo: bool, initial: Option<Initial>, snapshot: Option<&Snapshot>, cx: &mut App) {
    let mut connection = Connection {
        state: if demo { State::Demo } else { State::Connecting },
        status: Status::default(),
        status_at: Instant::now(),
        settings: EngineSettings::default(),
        loaded: false,
        owner: None,
        sender: None,
        inbox: None,
        next: 0,
        pending: HashMap::new(),
        queued: Vec::new(),
        blocking: Arc::default(),
        epoch: 0,
        bye: false,
    };
    if let Some(initial) = &initial {
        connection.status = initial.status.clone();
        if let Some(snapshot) = snapshot {
            connection.state = State::Connected;
            connection.loaded = true;
            connection.settings = snapshot.settings.clone();
            harness::set_icons(&snapshot.icons);
            cx.set_global(snapshot.harnesses.clone());
        } else {
            connection.state = State::Mismatch(initial.status.clone());
        }
    }
    cx.set_global(connection);
    if demo {
        return;
    }
    // Messages wait in the inbox until the window's entity is set as owner.
    match initial {
        Some(Initial { reader, writer, .. }) => {
            let (incoming, epoch) = open_inbox(cx);
            let blocking = cx.global::<Connection>().blocking.clone();
            runtime().spawn(run_io(reader, writer, incoming, blocking));
            listen(epoch, cx);
        }
        None => connect(true, cx),
    }
}

pub fn set_owner(owner: WeakEntity<Adeline>, cx: &mut App) {
    cx.global_mut::<Connection>().owner = Some(owner);
}

pub fn state(cx: &App) -> &State {
    &cx.global::<Connection>().state
}

pub fn connection(cx: &App) -> &Connection {
    cx.global::<Connection>()
}

fn open_inbox(cx: &mut App) -> (async_channel::Sender<Incoming>, u64) {
    let (sender, receiver) = async_channel::unbounded();
    let connection = cx.global_mut::<Connection>();
    connection.epoch += 1;
    connection.inbox = Some(receiver);
    connection.sender = None;
    connection.bye = false;
    (sender, connection.epoch)
}

fn listen(epoch: u64, cx: &mut App) {
    let Some(inbox) = cx.global::<Connection>().inbox.clone() else {
        return;
    };
    cx.spawn(async move |cx| {
        while let Ok(incoming) = inbox.recv().await {
            cx.update(|cx| handle(epoch, incoming, cx));
        }
    })
    .detach();
}

/// Connects again; `start` launches an engine when none is running.
pub fn connect(start: bool, cx: &mut App) {
    let (incoming, epoch) = open_inbox(cx);
    let blocking = cx.global::<Connection>().blocking.clone();
    cx.global_mut::<Connection>().state = State::Connecting;
    runtime().spawn(async move {
        let connection = match ipc::connect().await {
            Ok(connection) => Ok(connection),
            Err(_) if start => {
                let _ = incoming.send(Incoming::Starting).await;
                ipc::connect_or_start(false).await
            }
            Err(error) => Err(error.to_string()),
        };
        match connection {
            Ok((reader, mut writer)) => match ipc::send(&mut writer, &hello()).await {
                Ok(()) => run_io(reader, writer, incoming, blocking).await,
                Err(error) => {
                    let _ = incoming.send(Incoming::Failed(error.to_string())).await;
                }
            },
            Err(error) => {
                let _ = incoming.send(Incoming::Failed(error)).await;
            }
        }
    });
    listen(epoch, cx);
    cx.refresh_windows();
}

async fn run_io(
    mut reader: ipc::Reader,
    mut writer: ipc::Writer,
    incoming: async_channel::Sender<Incoming>,
    blocking: Slot,
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
            return;
        }
    }
    let _ = incoming.send(Incoming::Closed).await;
}

fn update_owner(cx: &mut App, change: impl FnOnce(&mut Adeline, &mut gpui_kit::Context<Adeline>)) {
    if let Some(owner) = cx.global::<Connection>().owner.clone() {
        let _ = owner.update(cx, change);
    }
}

fn fail_pending(error: &str, cx: &mut App) {
    let connection = cx.global_mut::<Connection>();
    let pending: Vec<_> = connection.pending.drain().map(|(_, done)| done).collect();
    connection.queued.clear();
    for done in pending {
        done(Err(error.to_owned()), cx);
    }
}

fn handle(epoch: u64, incoming: Incoming, cx: &mut App) {
    if cx.global::<Connection>().epoch != epoch {
        return;
    }
    match incoming {
        Incoming::Starting => {
            cx.global_mut::<Connection>().state = State::Starting;
            cx.refresh_windows();
        }
        Incoming::Ready(sender) => cx.global_mut::<Connection>().sender = Some(sender),
        Incoming::Message(EngineMessage::Welcome { status }) => {
            let connection = cx.global_mut::<Connection>();
            connection.status = status.clone();
            connection.status_at = Instant::now();
            if status.protocol != PROTOCOL {
                connection.state = State::Mismatch(status);
                cx.refresh_windows();
            }
        }
        Incoming::Message(EngineMessage::Snapshot(snapshot)) => {
            let connection = cx.global_mut::<Connection>();
            connection.state = State::Connected;
            connection.loaded = true;
            connection.status = snapshot.status.clone();
            connection.status_at = Instant::now();
            connection.settings = snapshot.settings.clone();
            harness::set_icons(&snapshot.icons);
            cx.set_global(snapshot.harnesses.clone());
            update_owner(cx, |app, cx| app.apply_snapshot(*snapshot, cx));
            let connection = cx.global_mut::<Connection>();
            if let Some(sender) = &connection.sender {
                for (id, command) in connection.queued.drain(..) {
                    send_line(sender, id, command);
                }
            }
            cx.refresh_windows();
        }
        Incoming::Message(EngineMessage::Delta(delta)) => match delta {
            Delta::EngineStatus(status) => {
                let connection = cx.global_mut::<Connection>();
                connection.status = status;
                connection.status_at = Instant::now();
                cx.refresh_windows();
            }
            Delta::Settings(settings) => {
                cx.global_mut::<Connection>().settings = settings;
                cx.refresh_windows();
            }
            Delta::Harnesses(catalog) => {
                cx.set_global(catalog);
                cx.refresh_windows();
            }
            Delta::Icons(icons) => {
                harness::set_icons(&icons);
                cx.refresh_windows();
            }
            delta => update_owner(cx, |app, cx| app.apply_delta(delta, cx)),
        },
        Incoming::Message(EngineMessage::Reply { id, result }) => {
            let done = cx.global_mut::<Connection>().pending.remove(&id);
            if let Some(done) = done {
                done(result, cx);
            }
        }
        Incoming::Message(EngineMessage::Bye) => cx.global_mut::<Connection>().bye = true,
        Incoming::Closed => {
            let connection = cx.global_mut::<Connection>();
            connection.sender = None;
            if matches!(connection.state, State::Waiting | State::Mismatch(_)) {
                return;
            }
            let unexpected = !connection.bye;
            connection.state = State::Stopped { unexpected };
            fail_pending("The conversation engine stopped.", cx);
            update_owner(cx, |app, cx| app.engine_stopped(unexpected, cx));
            cx.refresh_windows();
        }
        Incoming::Failed(error) => {
            cx.global_mut::<Connection>().state = State::Unavailable(error.clone());
            fail_pending(&error, cx);
            cx.refresh_windows();
        }
        Incoming::OldGone => connect(true, cx),
    }
}

fn send_line(sender: &mpsc::UnboundedSender<String>, id: u64, command: Command) {
    if let Ok(mut line) = serde_json::to_string(&ClientMessage::Request { id, command }) {
        line.push('\n');
        let _ = sender.send(line);
    }
}

/// Sends a command; `done` gets the engine's reply. A stopped engine is
/// started again for it.
pub fn request(command: Command, done: Callback, cx: &mut App) {
    let connection = cx.global_mut::<Connection>();
    let refused = match &connection.state {
        State::Demo => Some("Demo mode doesn't run the conversation engine.".to_owned()),
        State::Unavailable(error) => {
            Some(format!("The conversation engine is unavailable: {error}"))
        }
        State::Mismatch(_) | State::Waiting => {
            Some("An older conversation engine is still running.".to_owned())
        }
        _ => None,
    };
    if let Some(error) = refused {
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
            connect(true, cx);
        }
        _ => connection.queued.push((id, command)),
    }
}

/// Sends a command and waits for its reply, applying every change the engine
/// sent before it. Never call this while updating the `Adeline` entity.
pub fn request_blocking(command: Command, cx: &mut App) -> Result<Value, String> {
    let connection = cx.global_mut::<Connection>();
    let (State::Connected, Some(sender)) = (&connection.state, &connection.sender) else {
        return Err("The conversation engine isn't running. Start it in Settings › Engine.".into());
    };
    connection.next += 1;
    let id = connection.next;
    let (reply, replies) = std::sync::mpsc::channel();
    if let Ok(mut slot) = connection.blocking.lock() {
        *slot = Some((id, reply));
    }
    send_line(sender, id, command);
    let result = replies
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| Err("The conversation engine didn't answer in time.".into()));
    let connection = cx.global::<Connection>();
    if let Ok(mut slot) = connection.blocking.lock() {
        *slot = None;
    }
    let epoch = connection.epoch;
    if let Some(inbox) = connection.inbox.clone() {
        while let Ok(incoming) = inbox.try_recv() {
            handle(epoch, incoming, cx);
        }
    }
    result
}

/// Waits for an older engine to exit, then starts this version's engine.
/// With `stop`, asks it to stop its agents and exit first.
pub fn replace_old(stop: bool, cx: &mut App) {
    let connection = cx.global_mut::<Connection>();
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
            cx.update(|cx| handle(epoch, incoming, cx));
        }
    })
    .detach();
    cx.refresh_windows();
}

/// Starts the engine again after it was stopped, for an action that needs it.
pub fn ensure(cx: &mut App) {
    if matches!(state(cx), State::Stopped { .. }) {
        connect(true, cx);
    }
}

pub fn refresh_harnesses(fetch: bool, cx: &mut App) {
    if matches!(state(cx), State::Connected) {
        request(Command::RefreshHarnesses { fetch }, Box::new(|_, _| {}), cx);
    }
}

/// A probe running in the engine. Dropping it stops the probe.
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
    harness: String,
    command: String,
    arguments: Vec<String>,
    model: Option<String>,
    cx: &mut App,
) -> (
    Probe,
    async_channel::Receiver<Result<harness::Probed, harness::ProbeError>>,
) {
    let connection = cx.global_mut::<Connection>();
    connection.next += 1;
    let probe = connection.next;
    let handle = Probe {
        id: probe,
        sender: connection.sender.clone(),
    };
    let (results, receiver) = async_channel::bounded(1);
    request(
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
