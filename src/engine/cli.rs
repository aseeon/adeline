//! The `adeline engine` command line: status, start and stop.
use super::run;
use crate::{
    ipc,
    protocol::{ClientMessage, Command, EngineMessage, PROTOCOL, Status},
};
use serde_json::Value;
use std::time::Duration;

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

async fn request(command: Command) -> Result<(Value, ipc::Reader), String> {
    let (mut reader, mut writer) = ipc::connect().await.map_err(|e| e.to_string())?;
    let hello = ClientMessage::Hello {
        protocol: PROTOCOL,
        cli: true,
        resume: None,
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
    println!("Build: {}", crate::build(status.headless));
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
