//! The macOS menu bar. Other platforms store the menus and never show them, so this runs
//! everywhere and the app's own menus stay the same on each platform.
//!
//! Menu items dispatch GPUI actions, which reach the focused element first. The app-wide
//! handlers here catch the ones no element handled, such as with focus in another window.
use super::*;
use gpui_kit::base::input;

actions!(
    adeline,
    [
        About,
        StopAllAgents,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom
    ]
);

/// Sets the menus and handles their actions for the main window `handle`, owned by `owner`.
/// Runs after `config::bind_keys`, because macOS shows each item's shortcut from the keymap.
pub(super) fn init(
    handle: WindowHandle<Root>,
    owner: WeakEntity<Adeline>,
    demo_mode: bool,
    cx: &mut App,
) {
    let on_owner = |action: Action| {
        let owner = owner.clone();
        move |cx: &mut App| {
            let _ = handle.update(cx, |_, window, cx| {
                owner.update(cx, |app, cx| app.act(action.clone(), window, cx))
            });
        }
    };
    let about = on_owner(Action::About);
    cx.on_action(move |_: &About, cx| about(cx));
    let settings = on_owner(Action::AppSettings);
    cx.on_action(move |_: &OpenSettings, cx| settings(cx));
    let new_chat = on_owner(Action::NewChat);
    cx.on_action(move |_: &NewThread, cx| new_chat(cx));
    let stop_all = on_owner(Action::StopAll);
    cx.on_action(move |_: &StopAllAgents, cx| stop_all(cx));
    cx.on_action(move |_: &Quit, cx| settings::request_close(handle, owner.clone(), cx));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(|_: &Zoom, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.zoom_window());
        }
    });

    let mut app_items = vec![
        MenuItem::action("About Adeline", About),
        MenuItem::separator(),
        MenuItem::action("Settings…", OpenSettings),
    ];
    // Demo mode has no engine, so no agents to stop.
    if !demo_mode {
        app_items.push(MenuItem::action("Stop All Agents", StopAllAgents));
    }
    app_items.extend([
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Hide Adeline", Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action("Quit Adeline", Quit),
    ]);
    cx.set_menus([
        Menu::new("Adeline").items(app_items),
        Menu::new("File").items([MenuItem::action("New Chat", NewThread)]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", input::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
    ]);
}
