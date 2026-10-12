//! Starting the engine process and serving its clients: the single-instance
//! lock, the runtime, and one task per connection.
use super::*;

pub(super) fn run(daemon: bool) -> i32 {
    let dir = match ipc::engine_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("{error:#}");
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
    tracing::event!(
        name: "engine.start",
        tracing::Level::INFO,
        engine.version = env!("CARGO_PKG_VERSION"),
        engine.protocol = PROTOCOL,
        engine.daemon = daemon,
        "Engine {{engine.version}} starting"
    );
    if let Err(error) = crate::platform::contain_children(&dir) {
        tracing::event!(
            name: "engine.process_group.failed",
            tracing::Level::WARN,
            error.message = format!("{error:#}"),
            "Cannot tie agent processes to the engine: {{error.message}}"
        );
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(BLOCKING_THREADS)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::event!(
                name: "engine.runtime.failed",
                tracing::Level::ERROR,
                error.message = %error,
                "Cannot start the runtime: {{error.message}}"
            );
            return 1;
        }
    };
    let code = runtime.block_on(serve(daemon));
    // A detection or probe still running mustn't hold up the exit.
    runtime.shutdown_timeout(Duration::from_secs(1));
    crate::platform::stop_children();
    tracing::event!(name: "engine.exit",tracing::Level::INFO, "Engine exited");
    drop(lock);
    code
}

async fn serve(daemon: bool) -> i32 {
    let mut listener = match ipc::Listener::bind() {
        Ok(listener) => listener,
        Err(error) => {
            tracing::event!(
                name: "engine.listen.failed",
                tracing::Level::ERROR,
                error.message = format!("{error:#}"),
                "{{error.message}}"
            );
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
                    tracing::event!(
                        name: "engine.client.accept_failed",
                        tracing::Level::WARN,
                        error.message = %error,
                        "Accepting a client failed: {{error.message}}"
                    );
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });
    let mut tick = tokio::time::interval(TICK);
    while !engine.exit {
        // Agent events stream fast; their status changes go out on the next tick.
        tokio::select! {
            Some(input) = inputs.recv() => {
                engine.input(input);
                engine.refresh_status();
            }
            Ok(event) = agent_events.recv() => engine.driver_event(event),
            _ = tick.tick() => {
                engine.tick();
                engine.refresh_status();
            }
        }
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
                Err(error) => tracing::event!(
                    name: "engine.client.unreadable",
                    tracing::Level::WARN,
                    client.id = client,
                    error.message = %error,
                    "Client {{client.id}} sent an unreadable message: {{error.message}}"
                ),
            }
        }
        let _ = input.send(Input::Disconnected { client });
    });
}
