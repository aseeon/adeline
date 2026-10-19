//! Headless UI tests against a live engine: the real `Adeline` window on GPUI's
//! test platform, in a temporary home where the binary's engine runs
//! (`ADELINE_EXE`) with the fake agent. Each test runs in its own process under
//! nextest. Built only with the `test-support` feature, so the headless tests
//! stay free of GPUI.
#![expect(
    clippy::disallowed_methods,
    reason = "tests block on processes off any real UI thread"
)]
mod common;
#[path = "../../adeline_ui/tests/support/mod.rs"]
mod support;

use gpui_kit::TestAppContext;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::{MENU, Ui};

const WAIT: Duration = Duration::from_secs(30);

/// A home with the fake agent where the UI starts its engine; stops it on drop.
struct Live(PathBuf);

impl Live {
    fn new(name: &str) -> Self {
        let home = common::home(name);
        support::set_home(&home);
        support::set_env("ADELINE_EXE", env!("CARGO_BIN_EXE_adeline"));
        support::set_env("ADELINE_FAKE_STATE", home.join("fake-state"));
        Self(home)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Remote machines through `fake_ssh.py`: each destination is a home in
    /// `remotes/` with its own engine.
    fn remotes(&self) -> PathBuf {
        let root = self.0.join("remotes");
        std::fs::create_dir_all(&root).unwrap();
        let fake = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/engine-check/fake_ssh.py")
            .canonicalize()
            .unwrap();
        support::set_env(
            "ADELINE_SSH",
            format!("{} {}", common::python(), fake.display()),
        );
        support::set_env("ADELINE_FAKE_SSH_ROOT", &root);
        root
    }

    fn machines(&self) -> String {
        std::fs::read_to_string(self.0.join(".config/adeline/machines.yml")).unwrap_or_default()
    }

    /// Saves `machines.yml` with (id, name, destinations, engine) entries, all checked.
    fn save_machines(&self, machines: &[(&str, &str, &[&str], Option<&str>)]) {
        let mut text = String::from("machines:\n");
        for (id, name, destinations, engine) in machines {
            writeln!(text, "- id: {id}\n  name: {name}\n  destinations:").unwrap();
            for destination in *destinations {
                writeln!(text, "  - {destination}").unwrap();
            }
            if let Some(engine) = engine {
                writeln!(text, "  engine: {engine}").unwrap();
            }
        }
        text += "checked:\n- local\n";
        for (id, ..) in machines {
            writeln!(text, "- {id}").unwrap();
        }
        std::fs::write(self.0.join(".config/adeline/machines.yml"), text).unwrap();
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        common::adeline(&self.0, &["engine", "stop"]);
        for remote in std::fs::read_dir(self.0.join("remotes"))
            .into_iter()
            .flatten()
        {
            let remote = remote.unwrap().path();
            if remote.is_dir() {
                common::adeline(&remote, &["engine", "stop"]);
            }
        }
    }
}

fn open(cx: &mut TestAppContext) -> Ui<'_> {
    let mut ui = Ui::open(cx, false, false);
    // With remote machines, tabs name their machine: "demo · <this computer>".
    let local = format!("{} · ", common::PROJECT);
    ui.wait("the demo tab", WAIT, |ui| {
        ui.tabs()
            .iter()
            .any(|t| t == common::PROJECT || t.starts_with(&local))
    });
    ui
}

fn send(ui: &mut Ui, prompt: &str) {
    ui.wait("the message box", WAIT, |ui| {
        ui.try_named("Message").is_some()
    });
    ui.press("ctrl-l");
    ui.input(prompt);
    ui.press("enter");
}

// The engine counts the turn before its update reaches the UI, and the UI's
// Stop button shows once it has.
fn wait_processing(ui: &mut Ui, home: &Path) {
    ui.wait("a processing turn", WAIT, |ui| {
        active(home) > 0 && ui.try_named("Stop").is_some()
    });
}

fn active(home: &Path) -> usize {
    common::engine_status(home, "Active conversations")
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

#[gpui_kit::test]
fn launch(cx: &mut TestAppContext) {
    let live = Live::new("launch");
    let mut ui = open(cx);
    let mut pid = None;
    ui.wait("a running engine", WAIT, |_| {
        pid = common::engine_pid(live.path());
        pid.is_some()
    });
    assert_ne!(
        pid,
        Some(std::process::id()),
        "the engine runs in the UI process"
    );
    if cfg!(windows) {
        let exe = Path::new(env!("CARGO_BIN_EXE_adeline"));
        assert_eq!(common::windows_subsystem(exe), 2, "not a GUI program");
    }
}

#[gpui_kit::test]
fn quit_background(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let live = Live::new("quit-background");
    let mut ui = open(cx);
    send(&mut ui, "SLOW 6");
    wait_processing(&mut ui, live.path());
    ui.quit();
    ui.named("Finish in background");
    ui.named("Stop all");
    ui.click("Cancel");
    ui.wait_gone("Finish in background", WAIT);
    assert!(ui.is_open(), "Cancel closed Adeline");
    ui.quit();
    ui.click("Finish in background");
    assert!(!ui.is_open(), "Finish in background didn't quit");
    let pid = common::engine_pid(live.path()).expect("the engine stopped with the UI");
    // The turn finishes with no window open.
    let conversation = common::conversations(live.path(), common::PROJECT)
        .pop()
        .unwrap();
    ui.wait("the background turn to finish", WAIT, |_| {
        common::transcript(live.path(), common::PROJECT, &conversation).contains("turn_finished")
    });
    drop(ui);
    let _ui = open(second);
    assert_eq!(
        common::engine_pid(live.path()),
        Some(pid),
        "reopening started another engine"
    );
}

#[gpui_kit::test]
fn quit_idle(cx: &mut TestAppContext) {
    let _live = Live::new("quit-idle");
    let mut ui = open(cx);
    ui.quit();
    assert!(!ui.is_open(), "quitting with nothing running asked first");
}

#[gpui_kit::test]
fn quit_stop_all(cx: &mut TestAppContext) {
    let live = Live::new("quit-stop-all");
    let mut ui = open(cx);
    send(&mut ui, "SLOW 30");
    wait_processing(&mut ui, live.path());
    ui.quit();
    ui.click("Stop all");
    ui.wait("Adeline to quit after Stop all", WAIT, |ui| !ui.is_open());
    ui.wait("the agents to stop", WAIT, |_| active(live.path()) == 0);
}

#[gpui_kit::test]
fn settings_engine(cx: &mut TestAppContext) {
    let live = Live::new("settings-engine");
    let mut ui = open(cx);
    let before = ui.cx.windows();
    ui.press("ctrl-,");
    ui.new_window(&before);
    // The search reaches every settings page.
    ui.input("engine");
    ui.named("Keep conversation engine running");
    ui.named("Automatic retry limit");
    ui.click("Stop engine");
    ui.wait("the engine to stop", WAIT, |_| {
        common::engine_pid(live.path()).is_none()
    });
    ui.main_window();
    ui.wait_named("Conversation engine stopped", WAIT);
    ui.pause(Duration::from_secs(2));
    assert!(
        common::engine_pid(live.path()).is_none(),
        "the UI restarted the engine on its own"
    );
    ui.click("Start engine");
    ui.wait("the engine to start again", WAIT, |_| {
        common::engine_pid(live.path()).is_some()
    });
}

#[gpui_kit::test]
fn unavailable(cx: &mut TestAppContext) {
    let live = Live::new("unavailable");
    // A file where the engine's folder belongs stops it from starting.
    let engine_dir = live.path().join(".config/adeline/engine");
    std::fs::write(&engine_dir, "not a folder").unwrap();
    let mut ui = Ui::open(cx, false, false);
    ui.wait_named("Conversation engine unavailable", WAIT);
    ui.named("Retry");
    assert_eq!(
        std::fs::read_to_string(&engine_dir).unwrap(),
        "not a folder"
    );
    assert!(common::engine_pid(live.path()).is_none());
}

#[gpui_kit::test]
fn fork(cx: &mut TestAppContext) {
    let _live = Live::new("fork");
    let mut ui = open(cx);
    send(&mut ui, "hello");
    ui.wait_named("Fork", WAIT);
    ui.press("ctrl-shift-f");
    ui.wait_named("Forked from hello", WAIT);
    ui.wait("the fork in the chat list", WAIT, |ui| {
        ui.find(|label| label.starts_with("hello (fork),"))
            .is_some()
    });
    send(&mut ui, "after");
    ui.wait("the fork's own reply", WAIT, |ui| {
        ui.all(|l| l == "Fork").len() == 2
    });
    ui.named("Forked from hello");
    // Ctrl+Shift+G goes back to the chat the fork came from.
    ui.press("ctrl-shift-g");
    ui.wait_gone("Forked from hello", WAIT);
}

const LONG: Duration = Duration::from_secs(90);
const EXE: &str = if cfg!(windows) {
    "adeline.exe"
} else {
    "adeline"
};

/// Gives a remote home the fake agent, the project `app` and a known engine identity.
fn make_remote(remotes: &Path, host: &str) -> PathBuf {
    let home = remotes.join(host);
    common::populate(&home, "app");
    std::fs::create_dir_all(home.join(".config/adeline/engine")).unwrap();
    std::fs::write(
        home.join(".config/adeline/engine/id"),
        format!("{host}-engine"),
    )
    .unwrap();
    home
}

/// Kills the running `adeline bridge` sessions, as a dropped network would.
fn drop_bridges(remotes: &Path) {
    for record in std::fs::read_dir(remotes.join("bridges"))
        .into_iter()
        .flatten()
    {
        let record = record.unwrap().path();
        // The bridge runs under the session's cmd.exe. Killing its tree would kill
        // the engine it started too.
        #[cfg(windows)]
        {
            let shell = std::fs::read_to_string(&record).unwrap_or_default();
            let filter = format!("ParentProcessId={}", shell.trim());
            let children = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command"])
                .arg(format!(
                    "(Get-CimInstance Win32_Process -Filter '{filter}').ProcessId"
                ))
                .output()
                .unwrap();
            for pid in String::from_utf8_lossy(&children.stdout).split_whitespace() {
                std::process::Command::new("taskkill")
                    .args(["/F", "/PID", pid])
                    .output()
                    .unwrap();
            }
        }
        #[cfg(not(windows))]
        {
            let ssh = record.file_name().unwrap().to_string_lossy().into_owned();
            let group = std::fs::read_to_string(&record).unwrap_or_default();
            std::process::Command::new("kill")
                .args(["-KILL", &format!("-{}", group.trim()), &ssh])
                .output()
                .unwrap();
        }
    }
}

fn settings(ui: &mut Ui) {
    let before = ui.cx.windows();
    ui.press("ctrl-,");
    ui.new_window(&before);
}

fn type_into(ui: &mut Ui, label: &str, text: &str) {
    ui.click(label);
    ui.press("secondary-a backspace");
    ui.input(text);
}

/// Each machine's row in the machine selector, such as "Desktop, Connected".
fn machine_rows(ui: &mut Ui) -> Vec<String> {
    ui.press(&format!("{MENU}-h"));
    let rows = ui
        .elements()
        .into_iter()
        .filter(|e| e.visible() && e.role() == Some(gpui_kit::Role::ListItem))
        .filter_map(|e| e.label().map(str::to_owned))
        .filter(|label| label.contains(", "))
        .collect();
    ui.press("escape");
    rows
}

fn machine_state(ui: &mut Ui, name: &str) -> Option<String> {
    machine_rows(ui)
        .into_iter()
        .find_map(|row| row.strip_prefix(&format!("{name}, ")).map(str::to_owned))
}

fn has_tab(ui: &mut Ui, name: &str) -> bool {
    ui.tabs().iter().any(|tab| tab == name)
}

fn remote_turn_finishes(ui: &mut Ui, remote: &Path) {
    ui.wait("a remote conversation", WAIT, |_| {
        !common::conversations(remote, "app").is_empty()
    });
    let conversation = common::conversations(remote, "app").pop().unwrap();
    ui.wait("the remote turn to finish", WAIT, |_| {
        common::transcript(remote, "app", &conversation).contains("turn_finished")
    });
}

#[gpui_kit::test]
fn add_machine(cx: &mut TestAppContext) {
    let live = Live::new("add-machine");
    let remotes = live.remotes();
    let desktop = make_remote(&remotes, "desktop");
    common::populate(live.path(), "app");
    let mut ui = open(cx);
    assert!(
        ui.try_named("Machines").is_none(),
        "the selector shows with no remote machine"
    );
    settings(&mut ui);
    // The machine dropdown lists this computer, then "Add machine…".
    ui.press(&format!("{MENU}-h enter down enter"));
    type_into(&mut ui, "Machine name", "Desktop");
    type_into(&mut ui, "Destination", "me@desktop");
    ui.click("Add machine");
    ui.main_window();
    ui.wait_named("Machines", WAIT);
    ui.wait("the remote project's tab", LONG, |ui| {
        has_tab(ui, "app · Desktop")
    });
    assert!(
        ui.tabs()
            .iter()
            .any(|t| t.starts_with("app · ") && t != "app · Desktop"),
        "{:?}",
        ui.tabs()
    );
    assert!(
        desktop.join(".adeline/bin").join(EXE).is_file(),
        "nothing was installed"
    );
    ui.open_tab("app · Desktop");
    send(&mut ui, "hello");
    remote_turn_finishes(&mut ui, &desktop);
    assert!(live.machines().contains("engine: "), "{}", live.machines());
}

#[gpui_kit::test]
fn drop_and_reconnect(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let live = Live::new("drop");
    let remotes = live.remotes();
    let desktop = make_remote(&remotes, "desktop");
    live.save_machines(&[("m-desktop", "Desktop", &["desktop"], None)]);
    let mut ui = open(cx);
    ui.wait("the remote tab", LONG, |ui| has_tab(ui, "app · Desktop"));
    ui.open_tab("app · Desktop");
    send(&mut ui, "SLOW 6");
    ui.wait("a remote turn", WAIT, |_| active(&desktop) > 0);
    let conversation = common::conversations(&desktop, "app").pop().unwrap();
    drop_bridges(&remotes);
    ui.wait("the disconnected banner", WAIT, |ui| {
        ui.containing("Desktop is disconnected").is_some()
    });
    assert!(
        ui.tabs().iter().any(|t| t.contains("disconnected")),
        "{:?}",
        ui.tabs()
    );
    ui.wait("the reconnect", LONG, |ui| {
        machine_state(ui, "Desktop").as_deref() == Some("Connected")
    });
    ui.wait("the turn to finish", WAIT, |_| {
        common::transcript(&desktop, "app", &conversation).contains("turn_finished")
    });
    ui.wait("the remote turn to end", WAIT, |_| active(&desktop) == 0);
    // Unchecking a machine disconnects it.
    ui.press(&format!("{MENU}-h"));
    ui.activate("Show Desktop");
    ui.press("escape");
    ui.wait("Desktop's tab to go", WAIT, |ui| {
        !ui.tabs().iter().any(|t| t.contains("Desktop"))
    });
    ui.wait("the connection to close", WAIT, |_| {
        common::engine_status(&desktop, "Connected clients").as_deref() == Some("0")
    });
    drop(ui);
    let mut ui = open(second);
    ui.pause(Duration::from_secs(2));
    assert!(
        !ui.tabs().iter().any(|t| t.contains("Desktop")),
        "an unchecked machine came back on restart"
    );
    ui.press(&format!("{MENU}-h"));
    ui.activate("Show Desktop");
    ui.press("escape");
    ui.wait("Desktop's tab to return", LONG, |ui| {
        has_tab(ui, "app · Desktop")
    });
}

#[gpui_kit::test]
fn ssh_failures(cx: &mut TestAppContext) {
    // SSH prompts, refusals and failure states.
    let live = Live::new("ssh-failures");
    let remotes = live.remotes();
    std::fs::write(remotes.join("pw-locked.alias"), "locked").unwrap();
    live.save_machines(&[
        ("m-locked", "Locked", &["pw-locked"], None),
        ("m-wrong", "Wrong", &["pw-wrong"], None),
        ("m-away", "Away", &["down-away"], None),
        ("m-moved", "Moved", &["changed-moved"], None),
    ]);
    let mut ui = open(cx);
    let (mut locked, mut wrong) = (0, 0);
    ui.wait("Locked to sign in and Wrong to fail", LONG, |ui| {
        if let Some(prompt) = ui.find(|l| l.starts_with("Sign in to ")) {
            let machine = prompt.label().unwrap()["Sign in to ".len()..].to_owned();
            let answer = if machine == "Locked" {
                locked += 1;
                if locked > 1 { "secret" } else { "nope" }
            } else {
                wrong += 1;
                "nope"
            };
            type_into(ui, "SSH answer", answer);
            ui.click("Continue");
            return false;
        }
        let rows = machine_rows(ui);
        rows.iter().any(|r| r == "Locked, Connected")
            && rows.iter().any(|r| r == "Wrong, Sign-in failed")
    });
    let asked = wrong;
    ui.pause(Duration::from_secs(6));
    assert!(
        ui.find(|l| l == "Sign in to Wrong").is_none() && wrong == asked,
        "Wrong kept retrying"
    );
    ui.press(&format!("{MENU}-h"));
    ui.named("Away, Disconnected");
    assert!(
        ui.containing("Connection timed out").is_some(),
        "the timeout isn't shown"
    );
    ui.named("Moved, Disconnected");
    assert!(ui.containing("REMOTE HOST IDENTIFICATION").is_some());
    ui.press("escape");
    assert!(!live.machines().contains("secret"));
}

#[gpui_kit::test]
fn ssh_host_keys(cx: &mut TestAppContext) {
    // Host keys, another engine at a destination, and an engine saved twice.
    let live = Live::new("ssh-host-keys");
    let remotes = live.remotes();
    for (alias, host) in [
        ("newkey-fresh", "fresh"),
        ("twin", "desktop"),
        ("other", "other"),
    ] {
        std::fs::write(remotes.join(format!("{alias}.alias")), host).unwrap();
    }
    make_remote(&remotes, "desktop");
    live.save_machines(&[
        ("m-fresh", "Fresh", &["newkey-fresh"], None),
        (
            "m-desktop",
            "Desktop",
            &["other", "desktop"],
            Some("desktop-engine"),
        ),
        ("m-twin", "Twin", &["twin"], None),
    ]);
    make_remote(&remotes, "fresh");
    let mut ui = Ui::open(cx, false, false);
    ui.wait_named("Trust Fresh?", LONG);
    ui.click("Reject");
    ui.wait("Fresh to be refused", WAIT, |ui| {
        machine_state(ui, "Fresh").as_deref() == Some("Disconnected")
    });
    ui.press(&format!("{MENU}-h"));
    assert!(ui.containing("Host key verification failed").is_some());
    // Retry sits in the selector, under the refused machine.
    ui.click("Retry");
    ui.wait_named("Trust Fresh?", LONG);
    ui.click("Accept");
    ui.wait("Fresh to connect", LONG, |ui| has_tab(ui, "app · Fresh"));
    ui.wait("Desktop through its second destination", LONG, |ui| {
        has_tab(ui, "app · Desktop")
    });
    assert!(
        common::engine_pid(&remotes.join("other")).is_none(),
        "a refused destination's engine was started"
    );
    ui.wait("Twin to be refused as a second Desktop", WAIT, |_| {
        !live.machines().contains("Twin")
    });
}

#[gpui_kit::test]
fn headless_remote(cx: &mut TestAppContext) {
    let Some(headless) = std::env::var_os("ADELINE_HEADLESS_EXE") else {
        eprintln!(
            "Skipped: set ADELINE_HEADLESS_EXE to a headless build from \
             `cargo build --locked --no-default-features --target-dir target/headless`"
        );
        return;
    };
    let headless = std::fs::read(headless).unwrap();
    let live = Live::new("headless-remote");
    let remotes = live.remotes();
    let server = make_remote(&remotes, "server");
    let bin = server.join(".adeline/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(EXE), &headless).unwrap();
    std::fs::write(bin.join("version"), env!("CARGO_PKG_VERSION")).unwrap();
    live.save_machines(&[("m-server", "Server", &["server"], None)]);
    let mut ui = open(cx);
    ui.wait("the headless remote's tab", LONG, |ui| {
        has_tab(ui, "app · Server")
    });
    ui.open_tab("app · Server");
    send(&mut ui, "SLOW 4");
    ui.wait("the remote turn", WAIT, |_| active(&server) > 0);
    assert_eq!(
        common::engine_status(&server, "Build").as_deref(),
        Some("headless")
    );
    let conversation = common::conversations(&server, "app").pop().unwrap();
    drop_bridges(&remotes);
    ui.wait("the disconnected banner", WAIT, |ui| {
        ui.containing("Server is disconnected").is_some()
    });
    ui.wait("the reconnect", LONG, |ui| {
        machine_state(ui, "Server").as_deref() == Some("Connected")
    });
    ui.wait("the turn to finish", WAIT, |_| {
        common::transcript(&server, "app", &conversation).contains("turn_finished")
    });
    assert_eq!(
        std::fs::read(bin.join(EXE)).unwrap(),
        headless,
        "the headless build was replaced"
    );
    assert_eq!(
        common::engine_status(live.path(), "Build").as_deref(),
        Some("full")
    );
}

/// Adeline 0.1.0 for this OS from its release, cached in the temporary folder.
fn old_adeline() -> PathBuf {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "aarch64" if os != "linux" => "arm64",
        arch => arch,
    };
    let folder = std::env::temp_dir().join(format!("adeline-0.1.0-{os}-{arch}"));
    let exe = folder.join(EXE);
    if !exe.is_file() {
        let url = format!(
            "https://github.com/aseeon/adeline/releases/download/v0.1.0/adeline-{os}-{arch}.zip"
        );
        // Python is already needed for the fake agent, and unzips the same everywhere.
        let fetched = std::process::Command::new(common::python())
            .args([
                "-c",
                "import sys, urllib.request, zipfile; zipfile.ZipFile(urllib.request.urlretrieve(sys.argv[1])[0]).extractall(sys.argv[2])",
                &url,
            ])
            .arg(&folder)
            .output()
            .unwrap();
        assert!(
            fetched.status.success(),
            "couldn't download {url}: {fetched:?}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    exe
}

fn browse_to_site(ui: &mut Ui) {
    // The machine choice and the folder rows are lists, reached with Tab.
    ui.press("ctrl-o");
    ui.wait_named("Open a folder on which machine?", WAIT);
    ui.activate("Desktop");
    ui.wait_named("Open folder on Desktop", WAIT);
    ui.wait_named("code/", WAIT);
    ui.activate("code/");
    ui.wait_named("site/", WAIT);
    ui.activate("site/");
    ui.wait_gone("site/", WAIT);
    ui.activate("Choose this folder");
}

fn site_projects(desktop: &Path) -> Vec<String> {
    std::fs::read_dir(desktop.join(".config/adeline/projects"))
        .unwrap()
        .filter_map(|entry| std::fs::read_to_string(entry.unwrap().path().join("project.yml")).ok())
        .filter(|yaml| yaml.contains("name: site"))
        .collect()
}

// Downloads Adeline 0.1.0, so CI runs it in its own step with `--run-ignored`.
#[ignore = "downloads Adeline 0.1.0"]
#[gpui_kit::test]
fn upgrade_and_browse(cx: &mut TestAppContext) {
    let live = Live::new("upgrade");
    let remotes = live.remotes();
    let old = remotes.join("old");
    std::fs::create_dir_all(&old).unwrap();
    let started = std::process::Command::new(old_adeline())
        .args(["engine", "start"])
        .env("HOME", &old)
        .env("USERPROFILE", &old)
        .output()
        .unwrap();
    assert!(started.status.success(), "{started:?}");
    // A newer Adeline that speaks this client's protocol is used as it is.
    let newer = remotes.join("newer/.adeline/bin");
    std::fs::create_dir_all(&newer).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_adeline"), newer.join(EXE)).unwrap();
    std::fs::write(newer.join("version"), "99.0.0").unwrap();
    let desktop = make_remote(&remotes, "desktop");
    std::fs::create_dir_all(desktop.join("code/site")).unwrap();
    live.save_machines(&[
        ("m-desktop", "Desktop", &["desktop"], Some("desktop-engine")),
        ("m-old", "Old", &["old"], None),
        ("m-newer", "Newer", &["newer"], None),
        ("m-board", "Board", &["arm-board"], None),
    ]);
    let mut ui = open(cx);
    ui.wait_named("Upgrade Old?", LONG);
    ui.click("Later");
    ui.wait("every machine's state", LONG, |ui| {
        let rows = machine_rows(ui);
        rows.iter().any(|r| r == "Board, Unsupported")
            && rows.iter().any(|r| r == "Newer, Connected")
    });
    assert_eq!(
        machine_state(&mut ui, "Old").as_deref(),
        Some("Upgrade needed")
    );
    assert_eq!(
        std::fs::read_to_string(newer.join("version")).unwrap(),
        "99.0.0"
    );
    ui.press(&format!("{MENU}-h"));
    assert!(
        ui.containing("No Adeline build for").is_some(),
        "the unsupported platform isn't named"
    );
    assert!(
        ui.containing("It runs Adeline 0.1.0 with 0 active conversations")
            .is_some(),
        "the old version isn't named"
    );
    ui.click("Upgrade…");
    ui.wait_named("Upgrade Old?", WAIT);
    ui.click("Upgrade now (stops them)");
    ui.wait("the new engine on Old", LONG, |_| {
        common::engine_status(&old, "Version").as_deref() == Some(env!("CARGO_PKG_VERSION"))
    });
    ui.wait("Old to connect", LONG, |ui| {
        machine_state(ui, "Old").as_deref() == Some("Connected")
    });

    // Open folder on a remote machine browses its folders.
    ui.wait("Desktop's project", LONG, |ui| has_tab(ui, "app · Desktop"));
    browse_to_site(&mut ui);
    ui.wait("the new remote project", WAIT, |ui| {
        has_tab(ui, "site · Desktop")
    });
    let sites = site_projects(&desktop);
    assert_eq!(sites.len(), 1, "{sites:?}");
    assert!(sites[0].contains("site"), "{sites:?}");
    browse_to_site(&mut ui);
    ui.pause(Duration::from_secs(2));
    assert_eq!(
        ui.tabs().iter().filter(|t| *t == "site · Desktop").count(),
        1,
        "{:?}",
        ui.tabs()
    );
    assert_eq!(
        site_projects(&desktop).len(),
        1,
        "opening the same folder again made another project"
    );
}
