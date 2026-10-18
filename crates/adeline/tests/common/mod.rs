//! Shared by the engine and UI tests: a temporary home with one project and
//! `scripts/engine-check/fake_agent.py` as its only agent, and the real binary
//! run in it. Needs Python 3 on PATH.
#![allow(dead_code, reason = "each test file uses part of it")]
#![expect(
    clippy::disallowed_methods,
    reason = "tests block on processes off any UI thread"
)]

use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub const PROJECT: &str = "demo";

/// Creates the home `adeline-<name>-<pid>` in the temporary folder, with the
/// project `demo`.
pub fn home(name: &str) -> PathBuf {
    // macOS limits a socket path to 104 bytes, and its temporary folder is long.
    #[cfg(unix)]
    let temp = PathBuf::from("/tmp");
    #[cfg(not(unix))]
    let temp = std::env::temp_dir();
    let root = temp.join(format!("adeline-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    populate(&root, PROJECT);
    root
}

/// Gives `root` the fake agent and a project named `project` in `root/work`.
pub fn populate(root: &Path, project: &str) {
    let config = root.join(".config").join("adeline");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    let agent = config.join("agents").join("fake");
    fs::create_dir_all(&agent).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/engine-check/fake_agent.py")
        .canonicalize()
        .unwrap();
    // JSON strings are valid YAML, quoting and all.
    fs::write(
        agent.join("agent.yml"),
        format!(
            "version: 1
name: Fake
harness: custom
command: {}
arguments: [{}]
",
            json!(python()),
            json!(script)
        ),
    )
    .unwrap();
    let folder = config.join("projects").join(project);
    fs::create_dir_all(folder.join("conversations")).unwrap();
    fs::write(
        folder.join("project.yml"),
        format!(
            "name: {project}
directory: {}
",
            json!(work)
        ),
    )
    .unwrap();
}

pub fn python() -> String {
    for name in ["python3", "python"] {
        if let Ok(output) = Command::new(name)
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            && output.status.success()
        {
            return String::from_utf8_lossy(&output.stdout).trim().to_owned();
        }
    }
    panic!("The engine tests need Python 3 on PATH.");
}

/// Runs the binary with `home` as its home.
pub fn adeline(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_adeline"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()
        .unwrap()
}

/// A line of `adeline engine status`, such as "PID", or None while no engine runs.
pub fn engine_status(home: &Path, field: &str) -> Option<String> {
    let output = adeline(home, &["engine", "status"]);
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{field}: ")).map(str::to_owned))
}

pub fn engine_pid(home: &Path) -> Option<u32> {
    engine_status(home, "PID").and_then(|pid| pid.parse().ok())
}

/// The saved events of a conversation in `project`.
pub fn transcript(home: &Path, project: &str, conversation: &str) -> String {
    fs::read_to_string(
        home.join(".config/adeline/projects")
            .join(project)
            .join("conversations")
            .join(conversation)
            .join("transcript.jsonl"),
    )
    .unwrap_or_default()
}

pub fn conversations(home: &Path, project: &str) -> Vec<String> {
    fs::read_dir(
        home.join(".config/adeline/projects")
            .join(project)
            .join("conversations"),
    )
    .map(|entries| {
        entries
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect()
    })
    .unwrap_or_default()
}

/// The PE subsystem of a Windows program: 2 for a GUI program, 3 for a console one.
pub fn windows_subsystem(exe: &Path) -> u16 {
    let data = fs::read(exe).unwrap();
    let pe = u32::from_le_bytes(data[0x3C..0x40].try_into().unwrap()) as usize;
    u16::from_le_bytes(data[pe + 24 + 68..pe + 24 + 70].try_into().unwrap())
}
