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
use gpui_kit::component::WindowExt;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use support::{MENU, Ui};

const WAIT: Duration = Duration::from_secs(30);

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
    wait_project(&mut ui);
    ui
}

fn wait_project(ui: &mut Ui) {
    let local = format!("{} · ", common::PROJECT);
    ui.wait("the demo tab", WAIT, |ui| {
        ui.tabs()
            .iter()
            .any(|t| t == common::PROJECT || t.starts_with(&local))
    });
}

fn send(ui: &mut Ui, prompt: &str) {
    ui.wait("the message box", WAIT, |ui| {
        ui.try_named("Message").is_some()
    });
    ui.press("ctrl-l");
    ui.input(prompt);
    ui.press("enter");
}

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
    if cfg!(windows) {
        let exe = Path::new(env!("CARGO_BIN_EXE_adeline"));
        assert_eq!(common::windows_subsystem(exe), 2, "not a GUI program");
    }
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
fn quit_stop_all_shared(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let live = Live::new("quit-stop-all-shared");
    let mut ui = open(cx);
    send(&mut ui, "SLOW 30");
    wait_processing(&mut ui, live.path());
    ui.quit();
    ui.named("Stop all");
    let _other = open(second);
    ui.wait("the second client", WAIT, |_| {
        common::engine_status(live.path(), "Connected clients").as_deref() == Some("2")
    });
    ui.pause(Duration::from_secs(1));
    ui.click("Stop all");
    ui.wait("Adeline to quit after Stop all", WAIT, |ui| !ui.is_open());
    assert!(active(live.path()) > 0, "Stop all stopped a shared agent");
}

#[gpui_kit::test]
fn settings_engine(cx: &mut TestAppContext) {
    let live = Live::new("settings-engine");
    let mut ui = open(cx);
    let before = ui.cx.windows();
    ui.press("ctrl-,");
    ui.new_window(&before);
    ui.input("engine");
    ui.named("Keep conversation engine running");
    ui.named("Automatic retry limit");
    ui.resize(960., 900.);
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
fn unavailable_beside_a_remote(cx: &mut TestAppContext) {
    let live = Live::new("unavailable-remote");
    live.remotes();
    live.save_machines(&[("m-away", "Away", &["down-away"], None)]);
    std::fs::write(live.path().join(".config/adeline/engine"), "not a folder").unwrap();
    let mut ui = Ui::open(cx, false, false);
    ui.wait("the local engine's banner", WAIT, |ui| {
        ui.find(|l| l.ends_with(": Conversation engine unavailable"))
            .is_some()
    });
    ui.named("Retry");
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
    ui.press("ctrl-shift-g");
    ui.wait_gone("Forked from hello", WAIT);
}

#[gpui_kit::test]
fn chat_from_another_client(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let _live = Live::new("chat-elsewhere");
    let mut ui = open(cx);
    send(&mut ui, "hello");
    ui.wait_named("Fork", WAIT);
    let mut other = open(second);
    other.wait("hello opened", WAIT, |ui| {
        ui.read(|app, _| app.open_chat() == Some("hello"))
    });
    other.press("secondary-n");
    send(&mut other, "elsewhere");
    ui.wait("the other chat in the list", WAIT, |ui| {
        ui.find(|label| label.starts_with("elsewhere,")).is_some()
    });
    let open = ui.read(|app, _| app.open_chat().map(str::to_owned));
    assert_eq!(open.as_deref(), Some("hello"));
}

#[gpui_kit::test]
fn startup_requests_reach_a_running_engine(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let live = Live::new("startup-requests");
    let mut ui = open(cx);
    send(&mut ui, "SLOW 3");
    wait_processing(&mut ui, live.path());
    ui.press("secondary-n");
    let conversation = common::conversations(live.path(), common::PROJECT)
        .pop()
        .unwrap();
    let transcript = || common::transcript(live.path(), common::PROJECT, &conversation);
    ui.wait("the unread reply", WAIT, |_| {
        transcript().contains("turn_finished")
    });
    assert!(!transcript().contains("message_read"), "{}", transcript());
    let mut other = open(second);
    other.wait("the reopened chat marked read", WAIT, |_| {
        transcript().contains("message_read")
    });
}

#[gpui_kit::test]
fn open_folder_name_taken(cx: &mut TestAppContext) {
    let live = Live::new("name-taken");
    let other = live.path().join("other").join(common::PROJECT);
    std::fs::create_dir_all(&other).unwrap();
    let mut ui = open(cx);
    ui.press("ctrl-o");
    ui.wait("the folder picker", WAIT, |ui| ui.cx.did_prompt_for_paths());
    ui.cx
        .simulate_path_prompt_response(|_| Some(vec![other.clone()]));
    ui.wait("the refusal", WAIT, |ui| {
        ui.containing("already exists").is_some()
    });
    assert_eq!(ui.with(|window, cx| window.notifications(cx).len()), 1);
    let projects = std::fs::read_dir(live.path().join(".config/adeline/projects"))
        .unwrap()
        .count();
    assert_eq!(projects, 1, "a second project was created");
    assert_eq!(ui.tabs().len(), 1, "{:?}", ui.tabs());
}

#[gpui_kit::test]
fn docs_after_project_recreated(cx: &mut TestAppContext) {
    let live = Live::new("recreate");
    std::fs::write(live.path().join("work/old.md"), "# Old\n").unwrap();
    let moved = live.path().join("moved").join(common::PROJECT);
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::write(moved.join("new.md"), "# New\n").unwrap();
    let mut ui = open(cx);
    ui.press("ctrl-2");
    ui.wait_named("old.md", WAIT);

    ui.press("ctrl-w ctrl-f down delete");
    ui.cx.executor().advance_clock(Duration::from_secs(7));
    let definition = live
        .path()
        .join(".config/adeline/projects")
        .join(common::PROJECT);
    ui.wait("the project deleted", WAIT, |_| !definition.exists());
    ui.press("ctrl-o");
    ui.wait("the folder picker", WAIT, |ui| ui.cx.did_prompt_for_paths());
    ui.cx
        .simulate_path_prompt_response(|_| Some(vec![moved.clone()]));
    ui.wait("the new project's tab", WAIT, |ui| {
        ui.tabs().iter().any(|t| t.starts_with(common::PROJECT))
    });
    ui.wait_named("Message", WAIT);
    ui.press("ctrl-2");
    ui.wait_named("new.md", WAIT);
    ui.gone("old.md");
}

fn doc_text(ui: &mut Ui, label: &str) -> Option<String> {
    ui.try_named(label)
        .and_then(|doc| doc.value().map(str::to_owned))
}

#[gpui_kit::test]
fn docs_refresh_after_leaving(cx: &mut TestAppContext) {
    let live = Live::new("docs-refresh");
    let doc = live.path().join("work/a.md");
    std::fs::write(&doc, "one\n").unwrap();
    let mut ui = open(cx);
    ui.press("ctrl-2");
    ui.wait_named("a.md", WAIT);
    ui.press("ctrl-p");
    ui.input("a.md");
    ui.press("down enter");
    ui.wait_named("a.md, Viewer", WAIT);
    ui.press("ctrl-shift-v");
    ui.wait("the doc's text", WAIT, |ui| {
        doc_text(ui, "a.md, Editor").as_deref() == Some("one\n")
    });

    ui.press("ctrl-1");
    ui.wait_gone("Docs tree", WAIT);
    ui.pause(Duration::from_secs(1));
    std::fs::write(&doc, "two\n").unwrap();
    std::fs::write(live.path().join("work/b.md"), "# B\n").unwrap();
    ui.press("ctrl-2");
    ui.wait("the changed text", WAIT, |ui| {
        doc_text(ui, "a.md, Editor").as_deref() == Some("two\n")
    });
}

#[gpui_kit::test]
fn docs_conflict_blocks_save(cx: &mut TestAppContext) {
    let live = Live::new("docs-conflict");
    let doc = live.path().join("work/a.md");
    std::fs::write(&doc, "one\n").unwrap();
    let mut ui = open(cx);
    ui.press("ctrl-2");
    ui.wait_named("a.md", WAIT);
    ui.press("ctrl-p");
    ui.input("a.md");
    ui.press("down enter");
    ui.wait_named("a.md, Viewer", WAIT);
    ui.press("ctrl-shift-v");
    ui.wait_named("a.md, Editor", WAIT);
    ui.input("mine ");
    ui.wait_named("a.md, unsaved", WAIT);

    std::fs::write(&doc, "theirs\n").unwrap();
    ui.wait_named("Keep my version", WAIT);
    ui.press("ctrl-s");
    ui.pause(Duration::from_secs(1));
    assert_eq!(std::fs::read_to_string(&doc).unwrap(), "theirs\n");

    ui.click("Keep my version");
    ui.wait_gone("Keep my version", WAIT);
    ui.press("ctrl-s");
    ui.wait("my version saved", WAIT, |_| {
        std::fs::read_to_string(&doc).unwrap() == "mine one\n"
    });
}

#[gpui_kit::test]
fn a_cancelled_quit_still_guards_later_doc_edits(cx: &mut TestAppContext) {
    let live = Live::new("docs-quit-cancelled");
    std::fs::write(live.path().join("work/a.md"), "one\n").unwrap();
    let mut ui = open(cx);
    send(&mut ui, "SLOW 30");
    wait_processing(&mut ui, live.path());
    ui.press("ctrl-2");
    ui.wait_named("a.md", WAIT);
    ui.press("ctrl-p");
    ui.input("a.md");
    ui.press("down enter");
    ui.wait_named("a.md, Viewer", WAIT);
    ui.press("ctrl-shift-v");
    ui.wait_named("a.md, Editor", WAIT);
    ui.input("x");
    ui.wait_named("a.md, unsaved", WAIT);
    ui.quit();
    ui.wait_named("Save changes to 1 doc?", WAIT);
    ui.click("Save all");
    ui.wait_named("Finish in background", WAIT);
    ui.click("Cancel");
    ui.wait_gone("Finish in background", WAIT);
    ui.click("a.md, Editor");
    ui.input("y");
    ui.wait_named("a.md, unsaved", WAIT);
    ui.quit();
    ui.wait_named("Save changes to 1 doc?", WAIT);
    assert!(ui.is_open(), "the quit skipped the unsaved doc");
}

#[gpui_kit::test]
fn a_doc_edited_after_a_discarded_quit_is_unsaved(cx: &mut TestAppContext) {
    let live = Live::new("docs-quit-discarded");
    std::fs::write(
        live.path().join("work/a.md"),
        "one
",
    )
    .unwrap();
    let mut ui = open(cx);
    send(&mut ui, "SLOW 30");
    wait_processing(&mut ui, live.path());
    ui.press("ctrl-2");
    ui.wait_named("a.md", WAIT);
    ui.press("ctrl-p");
    ui.input("a.md");
    ui.press("down enter");
    ui.wait_named("a.md, Viewer", WAIT);
    ui.press("ctrl-shift-v");
    ui.wait_named("a.md, Editor", WAIT);
    ui.input("x");
    ui.wait_named("a.md, unsaved", WAIT);
    ui.quit();
    ui.wait_named("Save changes to 1 doc?", WAIT);
    ui.click("Discard");
    ui.wait_named("Finish in background", WAIT);
    ui.click("Cancel");
    ui.wait_gone("Finish in background", WAIT);
    ui.wait_gone("a.md, unsaved", WAIT);
    ui.click("a.md, Editor");
    ui.input("y");
    ui.wait_named("a.md, unsaved", WAIT);
}

#[gpui_kit::test]
fn an_older_engine_is_stopped_and_replaced(cx: &mut TestAppContext) {
    use adeline_core::{ipc, protocol};
    let _live = Live::new("old-engine");
    std::fs::create_dir_all(ipc::engine_dir().unwrap()).unwrap();
    let (bound, ready) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut listener = ipc::Listener::bind().unwrap();
            bound.send(()).unwrap();
            let (mut reader, mut writer) = listener.accept().await.unwrap();
            reader.next_line().await.unwrap();
            let status = protocol::Status {
                version: "0.0.1".into(),
                daemon: true,
                ..Default::default()
            };
            let welcome = protocol::EngineMessage::Welcome { status };
            ipc::send(&mut writer, &welcome).await.unwrap();
            while let Ok(Some(line)) = reader.next_line().await {
                if line.contains("shutdown") {
                    break;
                }
            }
        });
    });
    ready.recv().unwrap();
    let mut ui = Ui::open(cx, false, false);
    ui.wait_named("Restart engine", WAIT);
    ui.click("Restart engine");
    wait_project(&mut ui);
}

const LONG: Duration = Duration::from_secs(90);
const EXE: &str = if cfg!(windows) {
    "adeline.exe"
} else {
    "adeline"
};

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

fn install(home: &Path, exe: &[u8]) -> PathBuf {
    let bin = home.join(".adeline/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(EXE), exe).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(
        bin.join(EXE),
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(bin.join("version"), env!("CARGO_PKG_VERSION")).unwrap();
    bin
}

fn drop_bridges(remotes: &Path) {
    for record in std::fs::read_dir(remotes.join("bridges"))
        .into_iter()
        .flatten()
    {
        let record = record.unwrap().path();
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
                .args(["-KILL", "--", &format!("-{}", group.trim()), &ssh])
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

fn wait_remote(ui: &mut Ui, live: &Live, what: &str, mut done: impl FnMut(&mut Ui) -> bool) {
    let deadline = Instant::now() + LONG;
    ui.wait(what, LONG + LONG, |ui| {
        if done(ui) {
            return true;
        }
        if Instant::now() > deadline {
            ui.press(&format!("{MENU}-h"));
            let menu = ui.names();
            panic!(
                "timed out waiting for {what}. Machines menu: {menu:?}\nmachines.yml:\n{}",
                live.machines()
            );
        }
        false
    });
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
    let live = Live::new("ssh-host-keys");
    let remotes = live.remotes();
    for (alias, host) in [
        ("newkey-fresh", "fresh"),
        ("twin", "desktop"),
        ("other", "other"),
    ] {
        std::fs::write(remotes.join(format!("{alias}.alias")), host).unwrap();
    }
    let desktop = make_remote(&remotes, "desktop");
    install(
        &desktop,
        &std::fs::read(env!("CARGO_BIN_EXE_adeline")).unwrap(),
    );
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
    ui.click("Retry");
    ui.wait_named("Trust Fresh?", LONG);
    ui.click("Accept");
    ui.wait("Fresh to connect", LONG, |ui| has_tab(ui, "app · Fresh"));
    wait_remote(
        &mut ui,
        &live,
        "Desktop through its second destination",
        |ui| has_tab(ui, "app · Desktop"),
    );
    assert!(
        common::engine_pid(&remotes.join("other")).is_none(),
        "a refused destination's engine was started"
    );
    wait_remote(
        &mut ui,
        &live,
        "Twin to be refused as a second Desktop",
        |_| !live.machines().contains("Twin"),
    );
}

#[gpui_kit::test]
fn install_twice_at_once(cx: &mut TestAppContext) {
    let live = Live::new("install-twice");
    let remotes = live.remotes();
    std::fs::write(remotes.join("twin.alias"), "desktop").unwrap();
    let desktop = make_remote(&remotes, "desktop");
    live.save_machines(&[
        ("m-desktop", "Desktop", &["desktop"], None),
        ("m-twin", "Twin", &["twin"], None),
    ]);
    let mut ui = Ui::open(cx, false, false);
    wait_remote(&mut ui, &live, "both installs to finish", |ui| {
        ui.press(&format!("{MENU}-h"));
        let failed = ui
            .containing("Installing Adeline")
            .map(|e| e.label().unwrap().to_owned());
        ui.press("escape");
        assert_eq!(failed, None);
        let rows = machine_rows(ui);
        let settled = |name: &str| {
            rows.iter().any(|row| row == &format!("{name}, Connected"))
                || !live.machines().contains(name)
        };
        settled("Desktop") && settled("Twin")
    });
    assert!(desktop.join(".adeline/bin").join(EXE).is_file());
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
    let bin = install(&server, &headless);
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

#[ignore = "downloads Adeline 0.1.0"]
#[gpui_kit::test]
fn upgrade_old_engine(cx: &mut TestAppContext) {
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
    let newer = remotes.join("newer/.adeline/bin");
    std::fs::create_dir_all(&newer).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_adeline"), newer.join(EXE)).unwrap();
    std::fs::write(newer.join("version"), "99.0.0").unwrap();
    live.save_machines(&[
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
    ui.click("Upgrade");
    ui.wait_named("Upgrade Old?", WAIT);
    ui.click("Upgrade now (stops them)");
    ui.wait("the new engine on Old", LONG, |_| {
        common::engine_status(&old, "Version").as_deref() == Some(env!("CARGO_PKG_VERSION"))
    });
    ui.wait("Old to connect", LONG, |ui| {
        machine_state(ui, "Old").as_deref() == Some("Connected")
    });
}

#[gpui_kit::test]
fn browse_remote_folders(cx: &mut TestAppContext) {
    let live = Live::new("browse");
    let desktop = make_remote(&live.remotes(), "desktop");
    std::fs::create_dir_all(desktop.join("code/site")).unwrap();
    live.save_machines(&[("m-desktop", "Desktop", &["desktop"], Some("desktop-engine"))]);
    let mut ui = open(cx);
    ui.wait("Desktop's project", LONG, |ui| has_tab(ui, "app · Desktop"));
    browse_to_site(&mut ui);
    ui.wait("the new remote project", WAIT, |ui| {
        has_tab(ui, "site · Desktop")
    });
    let sites = site_projects(&desktop);
    assert_eq!(sites.len(), 1, "{sites:?}");
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
