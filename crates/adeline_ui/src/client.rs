//! The UI's connections to the conversation engine of every checked machine,
//! as a GPUI global. The local engine is reached through its pipe or socket,
//! remote ones over SSH (`remote.rs`). Each connection's changes arrive in
//! order on its own channel; replies run their callbacks in that order.
//!
//! A connection's `State` is its whole phase, and only the phases with an open
//! link hold its `Link`. `dial` starts `Connecting`. The snapshot or `Resumed`
//! moves it to `Connected` (`go_live`, which sends the requests queued until
//! then), an older local engine to `Mismatch` and, once the user picks, to
//! `Waiting` until it exits. Closing, failing or `Bye` end in the other states;
//! a lost remote waits out its delay in `Disconnected` and dials again.
use super::{Adeline, Harnesses};
use adeline_core::{
    askpass, harness, ipc,
    machines::{self, LOCAL},
    protocol::{
        self, ClientMessage, Command, Delta, EngineMessage, EngineSettings, PROTOCOL, Resume,
        Snapshot, Status,
    },
    remote,
};
use gpui_kit::{App, Global, WeakEntity};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt as _,
    sync::{mpsc, oneshot},
};

pub enum State {
    Demo,
    Connecting { link: Link, retries: u32 },
    Connected(Link),
    Mismatch { status: Status, link: Link },
    Waiting(Link),
    Stopped { unexpected: bool },
    Unavailable(String),
    Disconnected(String),
    SignInFailed(String),
    UpgradeNeeded { version: String, active: usize },
    LocalUpdateNeeded(String),
    Unsupported(String),
}

pub struct Link {
    sender: mpsc::UnboundedSender<String>,
    _cancel: oneshot::Sender<()>,
}

impl State {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Demo | Self::Connected(_) => "Connected",
            Self::Connecting { .. } | Self::Waiting(_) => "Connecting…",
            Self::Mismatch { .. }
            | Self::Stopped { .. }
            | Self::Unavailable(_)
            | Self::Disconnected(_) => "Disconnected",
            Self::SignInFailed(_) => "Sign-in failed",
            Self::UpgradeNeeded { .. } => "Upgrade needed",
            Self::LocalUpdateNeeded(_) => "Local update needed",
            Self::Unsupported(_) => "Unsupported",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Unavailable(error)
            | Self::Disconnected(error)
            | Self::SignInFailed(error)
            | Self::Unsupported(error) => Some(error.clone()),
            Self::Mismatch { status, .. } => Some(format!(
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
        matches!(self, Self::Demo | Self::Connected(_))
    }

    pub fn connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }
}

pub type Callback = Box<dyn FnOnce(Result<Value, String>, &mut App)>;

pub struct Connection {
    pub machine: String,
    pub state: State,
    pub status: Status,
    pub status_at: Instant,
    pub settings: EngineSettings,
    pub harnesses: harness::Catalog,
    pub destination: Option<String>,
    stream: Option<Resume>,
    next: u64,
    pending: HashMap<u64, Callback>,
    outputs: HashMap<u64, Box<dyn FnMut(String, &mut App)>>,
    queued: Vec<(u64, Command)>,
    epoch: u64,
}

impl Connection {
    fn new(machine: &str) -> Self {
        Self {
            machine: machine.to_owned(),
            state: State::Stopped { unexpected: false },
            status: Status::default(),
            status_at: Instant::now(),
            settings: EngineSettings::default(),
            harnesses: harness::Catalog::default(),
            destination: None,
            stream: None,
            next: 0,
            pending: HashMap::new(),
            outputs: HashMap::new(),
            queued: Vec::new(),
            epoch: 0,
        }
    }

    fn demo(machine: &str, harnesses: harness::Catalog) -> Self {
        Self {
            state: State::Demo,
            stream: Some(Resume::default()),
            harnesses,
            ..Self::new(machine)
        }
    }

    pub fn loaded(&self) -> bool {
        self.stream.is_some()
    }

    fn local(&self) -> bool {
        self.machine == LOCAL
    }

    fn take(&mut self) -> State {
        std::mem::replace(&mut self.state, State::Demo)
    }

    fn go_live(&mut self) {
        self.state = match self.take() {
            State::Connecting { link, .. } => {
                for (id, command) in self.queued.drain(..) {
                    send_line(&link.sender, id, command);
                }
                State::Connected(link)
            }
            state => state,
        };
    }

    fn mismatch(&mut self, status: Status) {
        self.state = match self.take() {
            State::Connecting { link, .. } => State::Mismatch { status, link },
            state => state,
        };
    }
}

struct Secret {
    prompt: String,
    answer: String,
    sessions: Vec<u64>,
}

pub struct Connections {
    list: Vec<Connection>,
    owner: Option<WeakEntity<Adeline>>,
    prompts: VecDeque<askpass::Prompt>,
    askpass: String,
    secrets: HashMap<String, Vec<Secret>>,
    focus: String,
    demo: bool,
}

impl Global for Connections {}

#[expect(
    clippy::large_enum_variant,
    reason = "each message is moved once, straight to its handler"
)]
enum Incoming {
    Linked { destination: String, status: Status },
    Message(EngineMessage),
    Closed,
    Failed(String),
    Remote(remote::Failure),
}

struct Wire {
    incoming: async_channel::Sender<Incoming>,
    outgoing: mpsc::UnboundedReceiver<String>,
    cancelled: oneshot::Receiver<()>,
}

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
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
        let harnesses = cx.global::<Harnesses>().0.clone();
        for machine in machines::checked() {
            let mut connection = Connection::demo(&machine, harnesses.clone());
            if machine == "vortex" {
                connection.state = State::Disconnected("Demo machine offline.".into());
            }
            connections.list.push(connection);
        }
        cx.set_global(connections);
        return;
    }
    let (prompts, inbox) = async_channel::unbounded();
    match runtime().block_on(async { askpass::serve_askpass(prompts) }) {
        Ok(address) => connections.askpass = address,
        Err(error) => tracing::event!(
            name: "client.askpass.failed",
            tracing::Level::WARN,
            error.message = format!("{error:#}"),
            "{{error.message}}"
        ),
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
    let Some(Initial {
        reader,
        writer,
        status,
        ..
    }) = initial
    else {
        connect(LOCAL, cx);
        return;
    };
    let wire = attach(LOCAL, 0, cx).expect("local connection");
    set_status(LOCAL, status.clone(), cx);
    let local = get_mut(LOCAL, cx).expect("local connection");
    if let Some(snapshot) = snapshot {
        local.settings = snapshot.settings.clone();
        local.harnesses = snapshot.harnesses.clone();
        local.stream = Some(Resume {
            epoch: snapshot.epoch.clone(),
            seq: snapshot.seq,
        });
        local.go_live();
        harness::set_icons(&snapshot.icons);
        cx.set_global(Harnesses(snapshot.harnesses.clone()));
    } else {
        local.mismatch(status);
    }
    runtime().spawn(run_io(reader, writer, wire, None));
}

pub fn set_owner(owner: WeakEntity<Adeline>, cx: &mut App) {
    cx.global_mut::<Connections>().owner = Some(owner);
}

pub fn get<'a>(machine: &str, cx: &'a App) -> Option<&'a Connection> {
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

pub fn usable(machine: &str, cx: &App) -> bool {
    state(machine, cx).is_some_and(State::usable)
}

pub fn connections(cx: &App) -> &[Connection] {
    &cx.global::<Connections>().list
}

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
        let connections = cx.global_mut::<Connections>();
        connections.secrets.remove(machine);
        let at = connections.list.iter().position(|c| c.machine == *machine);
        let removed = connections.list.remove(at.expect("connection"));
        for done in removed.pending.into_values() {
            done(Err("The machine was unchecked.".into()), cx);
        }
        let machine = machine.clone();
        update_owner(cx, move |app, cx| app.machine_unchecked(&machine, cx));
    }
    let demo = cx.global::<Connections>().demo;
    for machine in checked {
        if get(&machine, cx).is_some() {
            continue;
        }
        if demo {
            let connection = Connection::demo(&machine, cx.global::<Harnesses>().0.clone());
            cx.global_mut::<Connections>().list.push(connection);
            let machine = machine.clone();
            update_owner(cx, move |app, cx| app.machine_checked(&machine, cx));
        } else {
            cx.global_mut::<Connections>()
                .list
                .push(Connection::new(&machine));
            connect(&machine, cx);
        }
    }
    let order = machines::all();
    cx.global_mut::<Connections>()
        .list
        .sort_by_key(|c| order.iter().position(|m| *m == c.machine));
    cx.refresh_windows();
}

fn attach(machine: &str, retries: u32, cx: &mut App) -> Option<Wire> {
    let (sender, outgoing) = mpsc::unbounded_channel();
    let (cancel, cancelled) = oneshot::channel();
    let (incoming, inbox) = async_channel::unbounded();
    let connection = get_mut(machine, cx)?;
    connection.epoch += 1;
    connection.state = State::Connecting {
        link: Link {
            sender,
            _cancel: cancel,
        },
        retries,
    };
    let epoch = connection.epoch;
    let machine = machine.to_owned();
    cx.spawn(async move |cx| {
        while let Ok(incoming) = inbox.recv().await {
            cx.update(|cx| handle(&machine, epoch, incoming, cx));
        }
    })
    .detach();
    Some(Wire {
        incoming,
        outgoing,
        cancelled,
    })
}

pub fn connect(machine: &str, cx: &mut App) {
    dial(machine, 0, false, cx);
}

pub fn retry(machine: &str, cx: &mut App) {
    connect(machine, cx);
}

pub fn upgrade(machine: &str, cx: &mut App) {
    dial(machine, 0, true, cx);
}

fn dial(machine: &str, retries: u32, replace: bool, cx: &mut App) {
    if get(machine, cx).is_none() || cx.global::<Connections>().demo {
        return;
    }
    if machine == LOCAL {
        let wire = attach(machine, retries, cx).expect("connection");
        runtime().spawn(dial_local(wire, replace));
    } else {
        let Some(saved) = machines::remote(machine) else {
            unavailable(machine, "This machine is no longer saved.", cx);
            return;
        };
        let resume = get(machine, cx).and_then(|c| c.stream.clone());
        let askpass = remote::Askpass {
            address: cx.global::<Connections>().askpass.clone(),
            machine: machine.to_owned(),
        };
        let wire = attach(machine, retries, cx).expect("connection");
        runtime().spawn(async move {
            match remote::connect(&saved, &hello(resume), replace, &askpass).await {
                Ok(found) => {
                    let linked = Incoming::Linked {
                        destination: found.destination,
                        status: found.status,
                    };
                    if wire.incoming.send(linked).await.is_ok() {
                        run_io(found.reader, found.writer, wire, Some(found.child)).await;
                    }
                }
                Err(failure) => {
                    let _ = wire.incoming.send(Incoming::Remote(failure)).await;
                }
            }
        });
    }
    cx.refresh_windows();
}

async fn dial_local(mut wire: Wire, replace: bool) {
    while replace && ipc::connect().await.is_ok() {
        if wire.cancelled.try_recv() != Err(oneshot::error::TryRecvError::Empty) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let error = match ipc::connect_or_start(false).await {
        Ok((reader, mut writer)) => match ipc::send(&mut writer, &hello(None)).await {
            Ok(()) => return run_io(reader, writer, wire, None).await,
            Err(error) => error.to_string(),
        },
        Err(error) => format!("{error:#}"),
    };
    let _ = wire.incoming.send(Incoming::Failed(error)).await;
}

async fn run_io(
    mut reader: ipc::Reader,
    mut writer: ipc::Writer,
    wire: Wire,
    _child: Option<tokio::process::Child>,
) {
    let Wire {
        incoming,
        mut outgoing,
        cancelled,
    } = wire;
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
            if incoming.send(Incoming::Message(message)).await.is_err() {
                return false;
            }
        }
        true
    };
    tokio::select! {
        closed = read => if closed {
            let _ = incoming.send(Incoming::Closed).await;
        },
        _ = cancelled => {}
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
    connection.outputs.clear();
    connection.queued.clear();
    for done in pending {
        done(Err(error.to_owned()), cx);
    }
}

fn lose(machine: &str, error: String, cx: &mut App) {
    let connection = get_mut(machine, cx).expect("connection");
    let retries = match connection.state {
        State::Connecting { retries, .. } => retries + 1,
        _ => 1,
    };
    connection.state = State::Disconnected(error);
    let delay = Duration::from_secs(1 << retries.min(5)).min(Duration::from_secs(30));
    let epoch = connection.epoch;
    let machine = machine.to_owned();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(delay).await;
        cx.update(|cx| {
            if get(&machine, cx)
                .is_some_and(|c| c.epoch == epoch && matches!(c.state, State::Disconnected(_)))
            {
                dial(&machine, retries, false, cx);
            }
        });
    })
    .detach();
}

fn stopped(machine: &str, unexpected: bool, cx: &mut App) {
    get_mut(machine, cx).expect("connection").state = State::Stopped { unexpected };
    fail_pending(machine, "The conversation engine stopped.", cx);
    let target = machine.to_owned();
    update_owner(cx, move |app, cx| {
        app.engine_stopped(&target, unexpected, cx);
    });
    cx.refresh_windows();
}

fn unavailable(machine: &str, error: &str, cx: &mut App) {
    get_mut(machine, cx).expect("connection").state = State::Unavailable(error.to_owned());
    fail_pending(machine, error, cx);
    cx.refresh_windows();
}

fn connection_closed(machine: &str, cx: &mut App) {
    match state(machine, cx) {
        Some(State::Mismatch { .. } | State::Waiting(_)) => dial(LOCAL, 0, true, cx),
        Some(State::Connecting { .. } | State::Connected(_)) if machine != LOCAL => {
            lose(machine, "The connection was lost.".into(), cx);
            let error = format!("{} disconnected.", machines::name(machine));
            fail_pending(machine, &error, cx);
            update_owner(cx, |app, cx| app.refresh_runtime_views(cx));
            cx.refresh_windows();
        }
        Some(State::Connecting { .. } | State::Connected(_)) => stopped(machine, true, cx),
        _ => {}
    }
}

fn adopt_engine(machine: &str, engine: &str, cx: &mut App) -> anyhow::Result<()> {
    let saved = machines::remote(machine).and_then(|m| m.engine);
    if saved.is_some() || engine.is_empty() {
        return Ok(());
    }
    let local = get(LOCAL, cx).map(|c| c.status.engine_id.clone());
    let existing = machines::with_engine(engine, machine)
        .map(|m| m.name)
        .or_else(|| (local.as_deref() == Some(engine)).then(|| machines::name(LOCAL)));
    if let Some(existing) = existing {
        anyhow::bail!(
            "This machine is already saved as \"{existing}\". Add its destination to \"{existing}\" instead."
        );
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
        cx.set_global(Harnesses(catalog.clone()));
    }
    if let Some(connection) = get_mut(machine, cx) {
        connection.harnesses = catalog;
    }
}

fn handle(machine: &str, epoch: u64, incoming: Incoming, cx: &mut App) {
    if get(machine, cx).is_none_or(|c| c.epoch != epoch) {
        return;
    }
    match incoming {
        Incoming::Linked {
            destination,
            status,
        } => linked(machine, destination, status, cx),
        Incoming::Message(EngineMessage::Welcome { status }) => {
            set_status(machine, status.clone(), cx);
            if status.protocol != PROTOCOL {
                get_mut(machine, cx).expect("connection").mismatch(status);
                cx.refresh_windows();
            }
        }
        Incoming::Message(EngineMessage::Snapshot(snapshot)) => {
            snapshot_arrived(machine, *snapshot, cx);
        }
        Incoming::Message(EngineMessage::Resumed) => resumed(machine, cx),
        Incoming::Message(EngineMessage::Delta(delta)) => delta_arrived(machine, delta, cx),
        Incoming::Message(EngineMessage::Reply { id, result }) => {
            let connection = get_mut(machine, cx).expect("connection");
            connection.outputs.remove(&id);
            let done = connection.pending.remove(&id);
            if let Some(done) = done {
                done(result, cx);
            }
        }
        Incoming::Message(EngineMessage::Output { request, line }) => {
            output_arrived(machine, request, line, cx);
        }
        Incoming::Message(EngineMessage::Traffic { id, entry }) => {
            update_owner(cx, move |app, cx| app.traffic_arrived(&id, entry, cx));
        }
        Incoming::Message(EngineMessage::Bye) => stopped(machine, false, cx),
        Incoming::Closed => connection_closed(machine, cx),
        Incoming::Failed(error) => unavailable(machine, &error, cx),
        Incoming::Remote(failure) => remote_failed(machine, failure, cx),
    }
}

fn linked(machine: &str, destination: String, status: Status, cx: &mut App) {
    cx.global_mut::<Connections>().secrets.remove(machine);
    if let Err(error) = adopt_engine(machine, &status.engine_id, cx) {
        let error = format!("{error:#}");
        let connection = get_mut(machine, cx).expect("connection");
        connection.epoch += 1;
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

fn snapshot_arrived(machine: &str, snapshot: Snapshot, cx: &mut App) {
    let connection = get_mut(machine, cx).expect("connection");
    connection.settings = snapshot.settings.clone();
    connection.stream = Some(Resume {
        epoch: snapshot.epoch.clone(),
        seq: snapshot.seq,
    });
    connection.go_live();
    set_status(machine, snapshot.status.clone(), cx);
    harness::set_icons(&snapshot.icons);
    set_harnesses(machine, snapshot.harnesses.clone(), cx);
    let target = machine.to_owned();
    update_owner(cx, move |app, cx| {
        app.apply_snapshot(&target, snapshot, cx);
        app.docs_connected(&target, cx);
    });
    cx.refresh_windows();
}

fn resumed(machine: &str, cx: &mut App) {
    get_mut(machine, cx).expect("connection").go_live();
    let target = machine.to_owned();
    update_owner(cx, move |app, cx| {
        app.refresh_runtime_views(cx);
        app.docs_connected(&target, cx);
    });
    cx.refresh_windows();
}

fn delta_arrived(machine: &str, delta: Delta, cx: &mut App) {
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

fn output_arrived(machine: &str, request: u64, line: String, cx: &mut App) {
    let output = get_mut(machine, cx)
        .expect("connection")
        .outputs
        .remove(&request);
    if let Some(mut output) = output {
        output(line, cx);
        if let Some(connection) = get_mut(machine, cx) {
            connection.outputs.insert(request, output);
        }
    }
}

fn remote_failed(machine: &str, failure: remote::Failure, cx: &mut App) {
    cx.global_mut::<Connections>().secrets.remove(machine);
    let upgrade = matches!(failure, remote::Failure::Upgrade { .. });
    let state = match failure {
        remote::Failure::Network(error) => {
            lose(machine, error, cx);
            None
        }
        remote::Failure::SignIn(error) => Some(State::SignInFailed(error)),
        remote::Failure::Fatal(error) => Some(State::Unavailable(error)),
        remote::Failure::Unsupported(error) => Some(State::Unsupported(error)),
        remote::Failure::LocalUpdate(version) => Some(State::LocalUpdateNeeded(version)),
        remote::Failure::Upgrade { version, active } => {
            Some(State::UpgradeNeeded { version, active })
        }
    };
    if let Some(state) = state {
        get_mut(machine, cx).expect("connection").state = state;
    }
    let error = format!("{} is not connected.", machines::name(machine));
    fail_pending(machine, &error, cx);
    let target = machine.to_owned();
    update_owner(cx, move |app, cx| {
        if upgrade {
            app.ask_upgrade(&target, cx);
        }
        app.refresh_runtime_views(cx);
    });
    cx.refresh_windows();
}

fn send_line(sender: &mpsc::UnboundedSender<String>, id: u64, command: Command) {
    if let Ok(mut line) = serde_json::to_string(&ClientMessage::Request { id, command }) {
        line.push('\n');
        let _ = sender.send(line);
    }
}

fn refusal(connection: &Connection) -> Option<String> {
    let name = machines::name(&connection.machine);
    match &connection.state {
        State::Connecting { .. } | State::Connected(_) | State::Stopped { .. } => None,
        State::Demo => Some("Demo mode doesn't run the conversation engine.".to_owned()),
        State::Unavailable(error) if connection.local() => {
            Some(format!("The conversation engine is unavailable: {error}"))
        }
        State::Mismatch { .. } | State::Waiting(_) => {
            Some("An older conversation engine is still running.".to_owned())
        }
        State::Unavailable(_)
        | State::Disconnected(_)
        | State::SignInFailed(_)
        | State::UpgradeNeeded { .. }
        | State::LocalUpdateNeeded(_)
        | State::Unsupported(_) => Some(format!("{name} is not connected.")),
    }
}

pub fn request(machine: &str, command: Command, done: Callback, cx: &mut App) {
    send_request(machine, command, done, cx);
}

fn send_request(machine: &str, command: Command, done: Callback, cx: &mut App) -> Option<u64> {
    let Some(connection) = get_mut(machine, cx) else {
        let error = format!("{} is not checked.", machines::name(machine));
        cx.defer(move |cx| done(Err(error), cx));
        return None;
    };
    if let Some(error) = refusal(connection) {
        cx.defer(move |cx| done(Err(error), cx));
        return None;
    }
    connection.next += 1;
    let id = connection.next;
    connection.pending.insert(id, done);
    match &connection.state {
        State::Connected(link) => send_line(&link.sender, id, command),
        State::Stopped { .. } => {
            connection.queued.push((id, command));
            connect(machine, cx);
        }
        _ => connection.queued.push((id, command)),
    }
    Some(id)
}

pub fn request_with_output(
    machine: &str,
    command: Command,
    output: Box<dyn FnMut(String, &mut App)>,
    done: Callback,
    cx: &mut App,
) {
    if let Some(id) = send_request(machine, command, done, cx)
        && let Some(connection) = get_mut(machine, cx)
    {
        connection.outputs.insert(id, output);
    }
}

pub fn replace_old(stop: bool, cx: &mut App) {
    let Some(connection) = get_mut(LOCAL, cx) else {
        return;
    };
    connection.state = match connection.take() {
        State::Mismatch { link, .. } | State::Waiting(link) => {
            if stop {
                send_line(&link.sender, 0, Command::Shutdown);
            }
            State::Waiting(link)
        }
        state => state,
    };
    cx.refresh_windows();
}

pub fn ensure(machine: &str, cx: &mut App) {
    if matches!(state(machine, cx), Some(State::Stopped { .. })) {
        connect(machine, cx);
    }
}

pub fn refresh_harnesses(machine: &str, fetch: bool, cx: &mut App) {
    if state(machine, cx).is_some_and(State::connected) {
        request(
            machine,
            Command::RefreshHarnesses { fetch },
            Box::new(|_, _| {}),
            cx,
        );
    }
}

pub fn focus(machine: &str, cx: &mut App) {
    let connections = cx.global_mut::<Connections>();
    if connections.focus == machine {
        return;
    }
    machine.clone_into(&mut connections.focus);
    if let Some(catalog) = get(machine, cx).map(|c| c.harnesses.clone())
        && !cx.global::<Connections>().demo
    {
        cx.set_global(Harnesses(catalog));
    }
}

fn on_prompt(prompt: askpass::Prompt, cx: &mut App) {
    let connections = cx.global_mut::<Connections>();
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

pub fn prompt(cx: &App) -> Option<(String, String, bool)> {
    cx.global::<Connections>()
        .prompts
        .front()
        .map(|p| (p.machine.clone(), p.text.clone(), p.host_key()))
}

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
    async_channel::Receiver<Result<protocol::Probed, protocol::ProbeError>>,
) {
    let (probe, sender) = match get_mut(machine, cx) {
        Some(connection) => {
            connection.next += 1;
            let sender = match &connection.state {
                State::Connected(link) => Some(link.sender.clone()),
                _ => None,
            };
            (connection.next, sender)
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
                    Err(protocol::ProbeError {
                        message,
                        auth: Vec::new(),
                        version: false,
                    })
                });
            let _ = results.try_send(result);
        }),
        cx,
    );
    (handle, receiver)
}
