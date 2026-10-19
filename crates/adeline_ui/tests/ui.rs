//! Headless UI tests of demo mode: the real `Adeline` view on GPUI's test platform,
//! with no engine. Each test runs in its own process under nextest, in its own
//! temporary home.
mod support;

use adeline_core::{conversation::Category, ipc};
use gpui_kit::{TestAppContext, point, px};
use support::{END_OF_TEXT, MENU, START_OF_TEXT, Ui};

const SELECTION_CHAT: &str = "Make document edits safe when the preview is still updating";

fn no_engine() {
    assert!(
        !ipc::engine_dir().unwrap().exists(),
        "demo mode made an engine folder"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert!(
        runtime.block_on(ipc::connect()).is_err(),
        "an engine answers in demo mode"
    );
}

#[gpui_kit::test]
fn demo_conversation(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, true);

    ui.open_chat("Search long conversations without pausing the interface");
    ui.named("TODO: Check matches across two messages, 3 of 4");
    ui.press("ctrl-shift-t");
    ui.named("TODO");
    ui.press("escape");
    ui.gone("TODO");
    ui.named("Queued messages");
    ui.named("Queued: Also keep the match count visible while typing.");
    ui.press("ctrl-shift-l");
    ui.named("ACP traffic");
    ui.named("Copy all");

    // A turn with no traffic for longer than the notice delay (10 min) offers Stop and Restart.
    ui.open_chat("Import sessions from the old workspace format");
    ui.named("Agent silent for 12 min");

    // An idle chat's menus open from the keyboard and list the agent's choices.
    ui.open_chat(SELECTION_CHAT);
    ui.named("Model: Opus");
    ui.press(&format!("{MENU}-m"));
    ui.named("Search model");
    ui.input("Sonnet");
    ui.press("enter");
    ui.named("Model: Sonnet");
    assert_eq!(
        ui.read(|app, _| app.chat_option(Category::Model))
            .as_deref(),
        Some("sonnet")
    );
    ui.press(&format!("{MENU}-o"));
    ui.named("Search mode");
    ui.input("Accept");
    ui.press("enter");
    ui.named("Mode: Accept edits");
    assert_eq!(
        ui.read(|app, _| app.chat_option(Category::Mode)).as_deref(),
        Some("acceptEdits")
    );
    ui.press(&format!("{MENU}-p"));
    ui.named("Search more options");
    ui.press("escape");
    ui.gone("Search more options");

    // An error under a chat can be dismissed by hand.
    ui.open_chat("Keep the composer usable in a narrow desktop window");
    ui.named("Dismiss error");
    ui.press("ctrl-shift-d");
    ui.gone("Dismiss error");

    no_engine();
}

#[gpui_kit::test]
fn traffic_needs_verbose(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat("Search long conversations without pausing the interface");
    ui.press("ctrl-shift-l");
    ui.gone("ACP traffic");
}

/// Opens a doc from the tree search by keyboard: Ctrl+P, the query, Down into the tree.
fn open_doc(ui: &mut Ui, query: &str, rows_down: usize) {
    ui.press("ctrl-p secondary-a backspace");
    ui.input(query);
    for _ in 0..rows_down {
        ui.press("down");
    }
    ui.press("enter");
}

fn message(ui: &mut Ui) -> String {
    ui.named("Message").value().unwrap_or_default().to_owned()
}

#[gpui_kit::test]
fn docs_demo(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    // Ctrl+2 picks the second mode on the rail.
    ui.press("ctrl-2");
    ui.named("Docs tree");
    open_doc(&mut ui, "mdguide", 2);
    ui.named("docs/markdown-guide.md, Viewer");
    ui.press("ctrl-shift-v");
    ui.named("docs/markdown-guide.md, Editor");
    ui.named("Bold (Ctrl+B)");
    ui.press(END_OF_TEXT);
    ui.input("hello");
    ui.named("markdown-guide.md, unsaved");
    ui.press("ctrl-s");
    ui.named("markdown-guide.md");

    // A note quotes the selection with its path and line into the composer.
    open_doc(&mut ui, "notes/plan", 2);
    ui.press("ctrl-shift-v");
    ui.named("notes/plan.md, Editor");
    ui.press(&format!("{START_OF_TEXT} shift-down ctrl-shift-n"));
    let draft = message(&mut ui);
    assert!(
        draft.starts_with("`notes/plan.md`, line 1:"),
        "note draft {draft:?}"
    );

    // The chooser finds a chat by its title; its reply shows doc cards.
    ui.press("ctrl-shift-k");
    ui.named("Search conversations");
    ui.input("Make document edits");
    ui.press("enter");
    // Cards come and go with the replies, so Tab reaches them instead of a shortcut.
    ui.activate("2 more docs, hidden");
    ui.named("2 more docs, shown");
    ui.activate("Open notes/deleted-draft.md");
    ui.named("notes/deleted-draft.md no longer exists");
    ui.activate("Open docs/architecture.md");
    let viewer = ui.named("docs/architecture.md, Viewer").bounds();

    // Selecting text has no keyboard path, so this drags across the Viewer. The
    // selection stays in the doc, and Ctrl+Shift+N sends it to the composer.
    ui.drag(
        viewer.origin + point(px(30.), px(60.)),
        viewer.origin + point(viewer.size.width * (2. / 3.), px(160.)),
    );
    ui.set_clipboard("sentinel");
    ui.press("secondary-c");
    let copied = ui.clipboard();
    assert_ne!(copied, "sentinel", "the Viewer selection didn't copy");
    assert!(
        !copied.contains(SELECTION_CHAT) && !copied.contains("Reply"),
        "the selection reached the conversation: {copied:?}"
    );
    ui.press("ctrl-shift-n");
    let draft = message(&mut ui);
    assert!(
        draft.contains("`docs/architecture.md`, line"),
        "Viewer note draft {draft:?}"
    );

    // Quitting with unsaved edits asks first; Cancel keeps Adeline open.
    open_doc(&mut ui, "notes/plan", 2);
    ui.named("notes/plan.md, Editor");
    ui.input("xyz");
    ui.quit();
    ui.named("Save changes to 1 doc?");
    ui.click("Cancel");
    ui.gone("Save changes to 1 doc?");
    assert!(ui.is_open(), "Cancel quit Adeline");
    ui.quit();
    ui.named("Save changes to 1 doc?");
    ui.click("Discard");
    assert!(!ui.is_open(), "Discard didn't quit Adeline");
    drop(ui);

    // Docs reopens the project's doc and view where they were left.
    let mut ui = Ui::open(second, true, false);
    ui.press("ctrl-2");
    ui.named("notes/plan.md, Editor");
    no_engine();
}

#[gpui_kit::test]
fn demo_machines(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    // Tabs read "name · machine", and ", disconnected" for a dropped machine.
    let tabs = ui.tabs();
    assert!(tabs.iter().any(|t| t.contains("· Matrix")), "{tabs:?}");
    assert!(
        tabs.iter()
            .any(|t| t.contains("· Vortex") && t.ends_with(", disconnected")),
        "{tabs:?}"
    );
    // Each machine's toggle in the selector is reached with Tab.
    ui.press(&format!("{MENU}-h"));
    ui.named("Vortex, Disconnected");
    ui.named("Matrix, Connected");
    ui.activate("Show Matrix");
    assert!(
        !ui.tabs().iter().any(|t| t.contains("Matrix")),
        "{:?}",
        ui.tabs()
    );
    ui.activate("Show Matrix");
    assert!(
        ui.tabs().iter().any(|t| t.contains("Matrix")),
        "{:?}",
        ui.tabs()
    );
    ui.press("escape");

    let before = ui.cx.windows();
    ui.press("ctrl-,");
    ui.new_window(&before);
    ui.named("Search settings");
    let general = |ui: &mut Ui| {
        ui.find(|l| l == "Show General" || l == "Hide General")
            .is_some()
    };
    assert!(general(&mut ui), "General is missing for the local machine");
    ui.press(&format!("{MENU}-h enter down enter"));
    assert!(!general(&mut ui), "General shows for a remote machine");
    assert!(
        ui.find(|l| l == "Show Engine" || l == "Hide Engine")
            .is_some(),
        "Engine is missing for a remote machine"
    );
    ui.press(&format!("{MENU}-h enter up enter"));
    assert!(
        general(&mut ui),
        "General didn't return for the local machine"
    );
    no_engine();
}

#[gpui_kit::test]
fn close_tab(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    let tabs = ui.tabs();
    let name = tabs.last().unwrap().clone();
    ui.open_tab(&name);
    ui.press("ctrl-w");
    assert!(!ui.tabs().contains(&name), "{:?}", ui.tabs());
    ui.pause(std::time::Duration::from_millis(500));
    assert!(!ui.tabs().contains(&name), "the tab came back");
}

#[gpui_kit::test]
fn selection_stays_in_panel(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    // The conversation beside the doc: its messages are what the selection used to jump into.
    ui.press("ctrl-shift-k");
    ui.input("Make document edits");
    ui.press("enter");
    open_doc(&mut ui, "mdguide", 2);
    let viewer = ui.named("docs/markdown-guide.md, Viewer").bounds();
    // Selecting text has no keyboard path. This drags from mid-doc up and left, past
    // the doc's text into the docs tree.
    ui.drag(
        viewer.origin + point(px(300.), px(330.)),
        viewer.origin - point(px(120.), px(-40.)),
    );
    ui.set_clipboard("sentinel");
    ui.press("secondary-c");
    let copied = ui.clipboard();
    assert!(
        copied.len() > 20 && copied != "sentinel",
        "copied {copied:?}"
    );
    let docs = include_str!("../../../assets/demo-docs.json");
    let first = copied.lines().find(|line| line.trim().len() > 10).unwrap();
    assert!(
        docs.contains(first.trim()),
        "the selection missed the doc: {copied:?}"
    );
    for leak in [
        "older parse result",
        "tick two checklist items",
        "paragraph insertion",
    ] {
        assert!(
            !copied.contains(leak),
            "the selection reached the conversation: {copied:?}"
        );
    }
}

// GPUI's test platform never connects assistive technology, so these windows draw
// the regions as cached views: the path the view caching bugs were on.

#[gpui_kit::test]
fn transcript_selection(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    assert!(!ui.with(|window, _| window.is_a11y_active()));
    let rows: Vec<_> = ui
        .elements()
        .into_iter()
        .filter(|e| e.visible() && e.role() == Some(gpui_kit::Role::ListBoxOption))
        .map(|e| e.bounds().center())
        .collect();
    ui.open_chat(SELECTION_CHAT);
    let mut replies: Vec<_> = ui
        .all(|l| l == "Reply")
        .iter()
        .map(|e| e.bounds())
        .collect();
    replies.sort_by(|a, b| a.top().partial_cmp(&b.top()).unwrap());
    assert!(replies.len() >= 2, "the chat has fewer than two replies");
    // Reply text sits above each reply's own buttons. Selecting text has no
    // keyboard path, so this drags from the second message across the second reply.
    let (first, second) = (replies[0], replies[1]);
    ui.drag(
        point(second.left() + px(2.), first.bottom() + px(40.)),
        point(second.left() + px(402.), second.top() - px(20.)),
    );
    ui.set_clipboard("sentinel");
    ui.press("secondary-c");
    let copied = ui.clipboard();
    assert!(copied.contains("paragraph insertion"), "copied {copied:?}");
    // Hovering the chat list redraws it and not the transcript.
    for row in rows.iter().take(6) {
        ui.move_to(*row);
    }
    ui.set_clipboard("sentinel");
    ui.press("secondary-c");
    assert_eq!(
        ui.clipboard(),
        copied,
        "redrawing the chat list lost the selection"
    );
}

#[gpui_kit::test]
fn demo_fork(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(SELECTION_CHAT);
    ui.press("ctrl-shift-f");
    ui.named(&format!("Forked from {SELECTION_CHAT}"));
    no_engine();
}

#[gpui_kit::test]
fn docs_failed_rename_keeps_editing(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "notes/plan", 2);
    ui.press("ctrl-shift-v");
    ui.named("notes/plan.md, Editor");
    // F2 on the tree row renames; a taken name fails.
    ui.press("ctrl-p secondary-a backspace");
    ui.input("notes/plan");
    ui.press("down down f2");
    ui.named("Rename plan.md");
    ui.press("secondary-a");
    ui.input("review-checklist.md");
    ui.activate("Rename");
    ui.press("escape");
    ui.gone("Rename plan.md");
    // The doc still shows, so the view switches, edits count and Ctrl+S saves.
    ui.press("ctrl-shift-v");
    ui.named("notes/plan.md, Viewer");
    ui.press("ctrl-shift-v");
    ui.named("notes/plan.md, Editor");
    ui.press(END_OF_TEXT);
    ui.input("x");
    ui.named("plan.md, unsaved");
    ui.press("ctrl-s");
    ui.named("plan.md");
}
