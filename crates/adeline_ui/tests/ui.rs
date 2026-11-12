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

const RUNNING_CHAT: &str = "Search long conversations without pausing the interface";

#[gpui_kit::test]
fn plan_tab_toggles(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(RUNNING_CHAT);
    ui.press("ctrl-shift-t");
    ui.named("Plan steps");
    ui.press("ctrl-shift-t");
    ui.gone("Plan steps");
    no_engine();
}

#[gpui_kit::test]
fn the_todo_button_shows_the_plan_in_docs(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(RUNNING_CHAT);
    ui.press("ctrl-2");
    ui.named("Docs tree");
    ui.press("ctrl-shift-t");
    ui.named("Plan steps");
    ui.press("escape");
    ui.gone("Plan steps");
    no_engine();
}

#[gpui_kit::test]
fn escape_hides_slash_commands_until_the_text_changes(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(RUNNING_CHAT);
    ui.press("ctrl-l");
    ui.input("/re");
    ui.named("Commands");
    ui.press("escape");
    ui.gone("Commands");
    ui.input("v");
    ui.named("Commands");
}

#[gpui_kit::test]
fn queued_messages_show(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(RUNNING_CHAT);
    ui.named("Queued messages");
    ui.named("Queued: Also keep the match count visible while typing.");
    no_engine();
}

#[gpui_kit::test]
fn traffic_shows_with_verbose(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, true);
    ui.open_chat(RUNNING_CHAT);
    ui.press("ctrl-shift-l");
    ui.named("ACP traffic");
    ui.named("Copy all");
    no_engine();
}

#[gpui_kit::test]
fn silent_agent_notice(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat("Import sessions from the old workspace format");
    ui.named("Agent silent for 12 min");
    no_engine();
}

#[gpui_kit::test]
fn model_menu_picks_a_model(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
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
    no_engine();
}

#[gpui_kit::test]
fn mode_menu_picks_a_mode(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(SELECTION_CHAT);
    ui.press(&format!("{MENU}-o"));
    ui.named("Search mode");
    ui.input("Accept");
    ui.press("enter");
    ui.named("Mode: Accept edits");
    assert_eq!(
        ui.read(|app, _| app.chat_option(Category::Mode)).as_deref(),
        Some("acceptEdits")
    );
    no_engine();
}

#[gpui_kit::test]
fn more_options_menu_closes_with_escape(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(SELECTION_CHAT);
    ui.press(&format!("{MENU}-p"));
    ui.named("Search more options");
    ui.press("escape");
    ui.gone("Search more options");
}

#[gpui_kit::test]
fn an_error_can_be_dismissed(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat("Keep the composer usable in a narrow desktop window");
    ui.named("Dismiss error");
    ui.press("ctrl-shift-d");
    ui.gone("Dismiss error");
    no_engine();
}

#[gpui_kit::test]
fn copy_code_block(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat("Choose a font for code blocks");
    let code = "general:\n  appearance:\n    code_font: JetBrains Mono";
    ui.press("ctrl-shift-c");
    assert_eq!(ui.clipboard(), code);
    ui.set_clipboard("");
    ui.gone("Copy code");
    let button = ui
        .elements()
        .into_iter()
        .find(|element| element.label() == Some("Copy code"))
        .expect("a hidden copy button");
    ui.hover(&button);
    ui.click("Copy code");
    assert_eq!(ui.clipboard(), code);
}

#[gpui_kit::test]
fn expanded_activity_row_shows_the_whole_command(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat("Keep search text when changing filters");
    ui.press("ctrl-shift-i");
    let row = "cargo test --workspace \\";
    let collapsed = ui.named(row).bounds().size.height;
    ui.activate(row);
    ui.named("Show in chat");
    assert!(ui.named(row).bounds().size.height > collapsed);
    ui.activate(row);
    ui.gone("Show in chat");
    assert_eq!(ui.named(row).bounds().size.height, collapsed);
    no_engine();
}

#[gpui_kit::test]
fn chat_list_options_stay_open_while_switching(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    let tool_calls = |ui: &mut Ui| {
        ui.all(|name| name == "Tool calls")
            .iter()
            .find_map(|element| element.checked())
    };
    ui.activate("Chat list options");
    assert_eq!(tool_calls(&mut ui), Some(true));
    ui.press("down down enter");
    assert_eq!(tool_calls(&mut ui), Some(false));
    ui.named("Archived chats");
    no_engine();
}

#[gpui_kit::test]
fn traffic_needs_verbose(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.open_chat(RUNNING_CHAT);
    ui.press("ctrl-shift-l");
    ui.gone("ACP traffic");
}

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

fn edit_doc(ui: &mut Ui, query: &str, label: &str) {
    ui.press("ctrl-2");
    ui.named("Docs tree");
    open_doc(ui, query, 2);
    ui.named(&format!("{label}, Viewer"));
    ui.press("ctrl-shift-v");
    ui.named(&format!("{label}, Editor"));
}

#[gpui_kit::test]
fn docs_editor_edits_and_saves(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    edit_doc(&mut ui, "mdguide", "docs/markdown-guide.md");
    ui.named("Bold (Ctrl+B)");
    ui.press(END_OF_TEXT);
    ui.input("hello");
    ui.named("markdown-guide.md, unsaved");
    ui.press("ctrl-s");
    ui.named("markdown-guide.md");
    no_engine();
}

#[gpui_kit::test]
fn editor_note_quotes_the_selection(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    edit_doc(&mut ui, "notes/plan", "notes/plan.md");
    ui.press(&format!("{START_OF_TEXT} shift-down ctrl-shift-n"));
    let draft = message(&mut ui);
    assert!(
        draft.starts_with("`notes/plan.md`, line 1:"),
        "note draft {draft:?}"
    );
}

fn show_doc_cards(ui: &mut Ui) {
    ui.press("ctrl-2");
    ui.press("ctrl-shift-k");
    ui.named("Search conversations");
    ui.input("Make document edits");
    ui.press("enter");
    ui.resize(1440., 1200.);
    ui.activate("2 more docs, hidden");
    ui.named("2 more docs, shown");
}

#[gpui_kit::test]
fn doc_card_of_a_deleted_doc(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    show_doc_cards(&mut ui);
    ui.activate("Open notes/deleted-draft.md");
    ui.named("notes/deleted-draft.md no longer exists");
    no_engine();
}

#[gpui_kit::test]
fn viewer_note_quotes_the_selection(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    show_doc_cards(&mut ui);
    ui.activate("Open docs/architecture.md");
    let viewer = ui.named("docs/architecture.md, Viewer").bounds();
    ui.drag(
        viewer.origin + point(px(30.), px(60.)),
        viewer.origin + point(viewer.size.width * (2. / 3.), px(160.)),
    );
    ui.set_clipboard("sentinel");
    ui.press("secondary-c");
    assert_ne!(
        ui.clipboard(),
        "sentinel",
        "the Viewer selection didn't copy"
    );
    ui.press("ctrl-shift-n");
    let draft = message(&mut ui);
    assert!(
        draft.contains("`docs/architecture.md`, line"),
        "Viewer note draft {draft:?}"
    );
}

#[gpui_kit::test]
fn unsaved_docs_ask_before_closing(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    edit_doc(&mut ui, "notes/plan", "notes/plan.md");
    ui.input("xyz");
    ui.close();
    ui.named("Save changes to 1 doc?");
    ui.click("Cancel");
    ui.gone("Save changes to 1 doc?");
    assert!(ui.is_open(), "Cancel quit Adeline");
    ui.quit();
    ui.named("Save changes to 1 doc?");
    ui.click("Discard");
    assert!(!ui.is_open(), "Discard didn't quit Adeline");
}

#[gpui_kit::test]
fn docs_reopen_where_left(cx: &mut TestAppContext, second: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    edit_doc(&mut ui, "notes/plan", "notes/plan.md");
    ui.quit();
    assert!(!ui.is_open(), "Adeline didn't quit");
    drop(ui);
    let mut ui = Ui::open(second, true, false);
    ui.press("ctrl-2");
    ui.named("notes/plan.md, Editor");
    no_engine();
}

#[gpui_kit::test]
fn settings_hides_general_for_a_remote_machine(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
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
fn starting_and_switching_tabs_open_last_active_chat(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    let open_chat = |ui: &mut Ui| ui.read(|app, _| app.open_chat().map(str::to_owned));
    let latest = Some("Search long conversations without pausing the interface".to_owned());
    assert_eq!(open_chat(&mut ui), latest);
    let tabs = ui.tabs();
    let home = tabs.iter().find(|t| t.starts_with("ade-project")).unwrap();
    let other = tabs.iter().find(|t| t.contains("· Matrix")).unwrap();
    ui.open_tab(other);
    assert!(
        open_chat(&mut ui).is_some(),
        "{other} opened on Start a chat"
    );
    ui.open_tab(home);
    assert_eq!(open_chat(&mut ui), latest);
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

fn drag_pane(ui: &mut Ui, dx: f32) -> f32 {
    let before = ui.named("Resize panel").bounds();
    let from = before.center();
    ui.drag(from, point(from.x + px(dx), from.y));
    f32::from(ui.named("Resize panel").bounds().left() - before.left())
}

fn side_pane(cx: &mut TestAppContext) -> Ui<'_> {
    let mut ui = Ui::demo(cx, false);
    if ui.try_named("Resize panel").is_none() {
        ui.press("ctrl-shift-b");
    }
    ui
}

fn pane_width(ui: &mut Ui, rail: &str) -> f32 {
    f32::from(ui.named(rail).bounds().left() - ui.named("Resize panel").bounds().left())
}

fn design_scale(ui: &mut Ui) -> f32 {
    f32::from(ui.named("Agent activity").bounds().size.width) / 38.
}

fn widest_docs_pane(ui: &mut Ui) {
    ui.press("ctrl-2");
    open_doc(ui, "mdguide", 2);
    ui.resize(2400., 900.);
    drag_pane(ui, -2000.);
}

#[gpui_kit::test]
fn side_pane_resizes_and_its_rail_stays(cx: &mut TestAppContext) {
    let mut ui = side_pane(cx);
    let rail = ui.named("Side panel").bounds();
    let moved = drag_pane(&mut ui, -120.);
    assert!((moved + 120.).abs() < 4., "moved {moved}");
    assert_eq!(ui.named("Side panel").bounds(), rail);
}

#[gpui_kit::test]
fn each_mode_keeps_its_pane_width(cx: &mut TestAppContext) {
    let mut ui = side_pane(cx);
    drag_pane(&mut ui, -120.);
    let chats = pane_width(&mut ui, "Agent activity");
    ui.press("ctrl-2");
    open_doc(&mut ui, "mdguide", 2);
    assert!((drag_pane(&mut ui, 60.) - 60.).abs() < 4.);
    ui.press("ctrl-1");
    assert!((pane_width(&mut ui, "Agent activity") - chats).abs() < 1.);
}

#[gpui_kit::test]
fn pane_width_stops_at_twice_the_default(cx: &mut TestAppContext) {
    let mut ui = side_pane(cx);
    let scale = design_scale(&mut ui);
    let default = pane_width(&mut ui, "Agent activity");
    widest_docs_pane(&mut ui);
    let most = pane_width(&mut ui, "Chat");
    let grown = (880. - 330.) * scale;
    assert!(
        (most - default - grown).abs() < grown * 0.02,
        "{most} at most, {default} by default, scale {scale}"
    );
}

#[gpui_kit::test]
fn narrow_window_keeps_room_for_the_doc(cx: &mut TestAppContext) {
    let mut ui = side_pane(cx);
    let scale = design_scale(&mut ui);
    widest_docs_pane(&mut ui);
    ui.resize(1200., 900.);
    let doc = ui.named("Resize panel").bounds().left() - ui.named("Search docs").bounds().right();
    assert!(f32::from(doc) >= 320. * scale, "{doc:?} left for the doc");
}

#[gpui_kit::test]
fn ctrl_shift_b_hides_the_chat_in_docs(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "mdguide", 2);
    ui.named("Resize panel");
    ui.press("ctrl-shift-b");
    ui.gone("Resize panel");
    ui.press("ctrl-shift-b");
    ui.named("Resize panel");
}

#[gpui_kit::test]
fn resizing_a_pane_selects_no_text(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "mdguide", 2);
    drag_pane(&mut ui, 60.);
    let from = ui.named("Resize panel").bounds().center();
    let to = ui.named("docs/markdown-guide.md, Viewer").bounds().origin + point(px(100.), px(300.));
    for step in 1..=20 {
        ui.move_to(from + (to - from) * (step as f32 / 20.));
    }
    assert!(!ui.with(gpui_kit::base::TextSelection::has_selection));
}

#[gpui_kit::test]
fn a_lost_mouse_up_ends_the_selection(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton, VisualTestContext};
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "mdguide", 2);
    let doc = ui.named("docs/markdown-guide.md, Viewer").bounds();
    let selected = |ui: &mut Ui| ui.with(gpui_kit::base::TextSelection::selected_text);
    let window = ui.window;
    let mut vcx = VisualTestContext::from_window(window, ui.cx);
    let from = doc.origin + point(px(40.), px(240.));
    vcx.simulate_mouse_move(from, None, Modifiers::default());
    vcx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_move(
        from + point(px(120.), px(0.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    let dragged = selected(&mut ui);
    assert!(!dragged.is_empty());
    ui.move_to(from + point(px(200.), px(200.)));
    assert_eq!(selected(&mut ui), dragged);
}

fn finish_glide(ui: &mut Ui) {
    for _ in 0..15 {
        ui.cx
            .executor()
            .advance_clock(std::time::Duration::from_millis(20));
        ui.settle();
    }
}

#[gpui_kit::test]
fn the_composer_glides_between_chats_and_the_right_island(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    let chats = ui.named("Message").bounds().left();
    ui.press("ctrl-2");
    finish_glide(&mut ui);
    let island = ui.named("Message").bounds().left();
    assert!(
        island > chats,
        "{island:?} is in the island, right of {chats:?}"
    );
    ui.press("ctrl-1");
    finish_glide(&mut ui);
    assert_eq!(ui.named("Message").bounds().left(), chats);
}

#[gpui_kit::test]
fn selection_stays_in_panel(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    ui.press("ctrl-shift-k");
    ui.input("Make document edits");
    ui.press("enter");
    open_doc(&mut ui, "mdguide", 2);
    let viewer = ui.named("docs/markdown-guide.md, Viewer").bounds();
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
    let (first, second) = (replies[0], replies[1]);
    ui.drag(
        point(second.left() + px(2.), first.bottom() + px(40.)),
        point(second.left() + px(402.), second.top() - px(20.)),
    );
    ui.set_clipboard("sentinel");
    ui.press("secondary-c");
    let copied = ui.clipboard();
    assert!(copied.contains("paragraph insertion"), "copied {copied:?}");
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
    let open = ui.read(|app, _| app.open_chat().map(str::to_owned));
    assert_eq!(open, Some(format!("{SELECTION_CHAT} (fork)")));
    assert_eq!(ui.all(|l| l == "Reply").len(), 2);
    ui.set_clipboard("sentinel");
    ui.activate("Copy");
    assert!(
        ui.clipboard()
            .starts_with("An older parse result could replace a newer preview."),
        "copied {:?}",
        ui.clipboard()
    );
    no_engine();
}

#[gpui_kit::test]
fn docs_failed_rename_keeps_editing(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "notes/plan", 2);
    ui.press("ctrl-shift-v");
    ui.named("notes/plan.md, Editor");
    ui.press("ctrl-p secondary-a backspace");
    ui.input("notes/plan");
    ui.press("down down f2");
    ui.named("Rename plan.md");
    ui.press("secondary-a");
    ui.input("review-checklist.md");
    ui.activate("Rename");
    ui.press("escape");
    ui.gone("Rename plan.md");
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

#[gpui_kit::test]
fn page_down_and_up_scroll_the_viewer(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "mdguide", 2);
    ui.gone("Mark as not done");
    ui.press("pagedown");
    ui.named("Mark as not done");
    ui.press("pageup");
    ui.gone("Mark as not done");
}

#[gpui_kit::test]
fn docs_task_checkboxes_show_and_toggle(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "notes/plan", 2);
    ui.named("notes/plan.md, Viewer");
    let count = |ui: &mut Ui, label: &str| ui.all(|l| l == label).len();
    assert_eq!(count(&mut ui, "Mark as done"), 3);
    ui.activate("Mark as not done");
    assert_eq!(count(&mut ui, "Mark as done"), 4);
    assert_eq!(count(&mut ui, "Mark as not done"), 0);
}

#[gpui_kit::test]
fn docs_diagrams_render_in_the_background(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    ui.press("ctrl-2");
    open_doc(&mut ui, "diagrams", 2);
    ui.named("docs/diagrams.md, Viewer");
    ui.wait("both diagrams", std::time::Duration::from_secs(20), |ui| {
        ui.all(|l| l == "Mermaid diagram").len() == 2
    });
}

#[gpui_kit::test]
fn delete_agent(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    let before = ui.cx.windows();
    ui.press("ctrl-,");
    ui.new_window(&before);
    ui.activate("Show Agents");
    ui.activate("Grok Build");
    ui.activate("Delete agent…");
    ui.activate("Delete");
    ui.gone("Grok Build");
}

#[gpui_kit::test]
fn the_ligatures_switch_writes_the_setting(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    let before = ui.cx.windows();
    ui.press("ctrl-,");
    ui.new_window(&before);
    ui.activate("Appearance");
    let ligatures = || adeline_core::config::current().general.appearance.ligatures;
    let was = ligatures();
    ui.activate("Ligatures");
    assert_ne!(ligatures(), was, "the switch didn't change Ligatures");
    ui.activate("Ligatures");
    assert_eq!(ligatures(), was, "the switch didn't change Ligatures back");
}

#[gpui_kit::test]
fn edit_an_agent_opens_the_open_chats_agent(cx: &mut TestAppContext) {
    let mut ui = Ui::demo(cx, false);
    let main = ui.cx.windows();
    let edit_agent = |ui: &mut Ui| {
        ui.press(&format!("{MENU}-s"));
        ui.press("down enter");
        ui.new_window(&main);
        let name = ui.named("Name").value().map(str::to_owned);
        ui.main_window();
        name
    };
    ui.open_chat("Pick a default agent for new chats");
    assert_eq!(edit_agent(&mut ui).as_deref(), Some("Codex"));
    ui.press("ctrl-n");
    assert_eq!(edit_agent(&mut ui).as_deref(), Some("Claude Code"));
}
