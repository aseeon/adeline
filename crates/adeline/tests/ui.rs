//! Headless UI tests against a live engine: the real `Adeline` window on GPUI's
//! test platform, in a temporary home where the binary's engine runs
//! (`ADELINE_EXE`) with the fake agent. Each test runs in its own process under
//! nextest. Built only with the `test-support` feature, so the headless tests
//! stay free of GPUI.
mod common;
#[path = "../../adeline_ui/tests/support/mod.rs"]
mod support;

use gpui_kit::TestAppContext;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::Ui;

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
}

impl Drop for Live {
    fn drop(&mut self) {
        common::adeline(&self.0, &["engine", "stop"]);
    }
}

fn open(cx: &mut TestAppContext) -> Ui<'_> {
    let mut ui = Ui::open(cx, false, false);
    ui.wait("the demo tab", WAIT, |ui| {
        ui.tabs().iter().any(|t| t == common::PROJECT)
    });
    ui
}

fn send(ui: &mut Ui, prompt: &str) {
    ui.wait("the message box", WAIT, |ui| {
        ui.try_named("Message").is_some()
    });
    ui.click("Message");
    ui.input(prompt);
    ui.press("enter");
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
}

#[gpui_kit::test]
fn quit_background(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let live = Live::new("quit-background");
    let mut ui = open(cx);
    send(&mut ui, "SLOW 6");
    ui.wait("a processing turn", WAIT, |_| active(live.path()) > 0);
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
    let conversation = common::conversations(live.path()).pop().unwrap();
    ui.wait("the background turn to finish", WAIT, |_| {
        common::transcript(live.path(), &conversation).contains("turn_finished")
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
    ui.wait("a processing turn", WAIT, |_| active(live.path()) > 0);
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
    ui.click("Show Engine");
    ui.click("Engine");
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
    // Fork has no shortcut.
    ui.click("Fork");
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
    // The link back to the source has no shortcut.
    ui.click("hello");
    ui.wait_gone("Forked from hello", WAIT);
}
