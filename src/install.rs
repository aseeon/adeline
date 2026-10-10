//! Installing and updating supported agents and Node.js with their standard
//! global installs (scope R9–R12), and terminal logins (scope R14). The engine
//! runs these on its own machine and streams the output to the client.
use crate::profiles::{self, Step};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::AsyncBufReadExt as _;

/// What to install.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A supported agent, by harness ID; also its update.
    Agent { harness: String },
    /// Node.js through the OS package manager.
    Node,
}

/// One command of an install, as shown to confirm and as run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Planned {
    pub program: String,
    pub arguments: Vec<String>,
}

impl Planned {
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.arguments.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn planned(step: &Step) -> Planned {
    Planned {
        program: step.program.to_owned(),
        arguments: step.arguments.iter().map(|&a| a.to_owned()).collect(),
    }
}

/// The commands an install runs on this machine, or why it can't.
pub fn plan(target: &Target, catalog: &crate::harness::Catalog) -> Result<Vec<Planned>, String> {
    match target {
        Target::Agent { harness } => {
            let profile = profiles::find(harness, "").ok_or_else(|| {
                "Adeline can't install this agent. Install it by hand.".to_owned()
            })?;
            if profile.install.needs_node && catalog.node.npm.is_none() {
                return Err(format!("{} needs Node.js.", profile.name));
            }
            Ok(profile.install.steps().iter().map(planned).collect())
        }
        Target::Node => {
            if catalog.node.manager.is_none() {
                return Err(node_missing());
            }
            Ok(vec![if cfg!(windows) {
                Planned {
                    program: "winget".into(),
                    arguments: [
                        "install",
                        "--exact",
                        "--id",
                        "OpenJS.NodeJS.LTS",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                    ]
                    .map(str::to_owned)
                    .to_vec(),
                }
            } else {
                Planned {
                    program: "brew".into(),
                    arguments: vec!["install".into(), "node".into()],
                }
            }])
        }
    }
}

/// Why Node.js can't be installed from Adeline.
pub fn node_missing() -> String {
    if cfg!(windows) {
        "Node.js is required, and winget isn't available to install it. Install Node.js from nodejs.org, then check again.".into()
    } else if cfg!(target_os = "macos") {
        "Node.js is required, and Homebrew isn't available to install it. Install Node.js from nodejs.org, then check again.".into()
    } else {
        "Node.js is required. Install it with your package manager or from nodejs.org, then check again.".into()
    }
}

/// Runs the commands one after another, sending each output line to
/// `output`. Stops at the first failure.
pub async fn run(
    steps: Vec<Planned>,
    output: std::sync::Arc<dyn Fn(String) + Send + Sync>,
) -> Result<(), String> {
    for step in steps {
        output(format!("> {}", step.display()));
        let program = crate::harness::program(&step.program)
            .ok_or_else(|| format!("{} was not found on this machine.", step.program))?;
        let mut command = std::process::Command::new(&program);
        command.args(&step.arguments);
        // npm's shims need `node` from the same folder on PATH.
        if let Some(folder) = program.parent() {
            command.env("PATH", with_path(folder));
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000);
        }
        let (mut child, stdin, stdout, stderr) =
            tokio::task::block_in_place(|| crate::platform::spawn_piped(&mut command, true))
                .map_err(|error| format!("Could not start {}: {error}", step.program))?;
        drop(stdin);
        let mut readers = Vec::new();
        for reader in std::iter::once(stdout).chain(stderr) {
            let output = output.clone();
            readers.push(tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(reader).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    output(line);
                }
            }));
        }
        for reader in readers {
            let _ = reader.await;
        }
        let status = tokio::task::block_in_place(|| child.wait())
            .map_err(|error| format!("{} failed: {error}", step.program))?;
        if !status.success() {
            return Err(format!("{} failed ({status}).", step.display()));
        }
    }
    Ok(())
}

/// PATH with `folder` first.
fn with_path(folder: &Path) -> std::ffi::OsString {
    let mut paths = vec![folder.to_path_buf()];
    paths.extend(
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    std::env::join_paths(paths).unwrap_or_default()
}

/// Opens the user's terminal running `command` with `arguments` (scope R14).
#[expect(
    clippy::disallowed_methods,
    reason = "The engine runs this on its runtime, off any UI thread"
)]
pub fn open_terminal(
    command: &Path,
    arguments: &[String],
    environment: &[(String, String)],
) -> Result<(), String> {
    let line = std::iter::once(quote(&command.to_string_lossy()))
        .chain(arguments.iter().map(|arg| quote(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut terminal = if cfg!(windows) {
        let mut terminal = std::process::Command::new("cmd");
        terminal.args(["/C", "start", "Adeline login", "cmd", "/K", &line]);
        terminal
    } else if cfg!(target_os = "macos") {
        let exports = environment
            .iter()
            .fold(String::new(), |mut exports, (name, value)| {
                use std::fmt::Write as _;
                let _ = write!(exports, "export {name}={}; ", quote(value));
                exports
            });
        let script = format!("{exports}{line}")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let mut terminal = std::process::Command::new("osascript");
        terminal.args([
            "-e",
            &format!("tell application \"Terminal\" to do script \"{script}\""),
            "-e",
            "tell application \"Terminal\" to activate",
        ]);
        terminal
    } else {
        let mut terminal = std::process::Command::new("x-terminal-emulator");
        terminal.args(["-e", "sh", "-c", &format!("{line}; exec sh")]);
        terminal
    };
    terminal.envs(environment.iter().map(|(k, v)| (k, v)));
    terminal
        .spawn()
        .map(drop)
        .map_err(|error| format!("Could not open a terminal: {error}. Run this yourself: {line}"))
}

fn quote(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@\\".contains(c))
    {
        text.to_owned()
    } else if cfg!(windows) {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}
