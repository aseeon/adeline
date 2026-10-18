//! Headless UI tests: the real `Adeline` view on GPUI's test platform, driven by
//! its shortcuts (`docs/shortcuts.md`). Elements are found by their accessibility
//! labels, the names a user and UI Automation see. Each test runs in its own
//! process under nextest, in its own temporary home.
use adeline_core::{conversation::Category, ipc};
use gpui_kit::base::test_support::{ElementSnapshot, snapshots};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, TestAppContext, Window, WindowBounds,
    WindowOptions, point, px, size,
};

// The option menus: Alt on Windows and Linux, Cmd+Shift on macOS.
const MENU: &str = if cfg!(target_os = "macos") {
    "cmd-shift"
} else {
    "alt"
};

struct Ui<'a> {
    cx: &'a mut TestAppContext,
    window: AnyWindowHandle,
    adeline: Entity<adeline_ui::Adeline>,
}

impl<'a> Ui<'a> {
    #[expect(
        unsafe_code,
        reason = "the config folder follows HOME, read from the environment"
    )]
    fn demo(cx: &'a mut TestAppContext, verbose: bool) -> Self {
        let home = std::env::temp_dir().join(format!("adeline-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        for name in ["HOME", "USERPROFILE"] {
            // SAFETY: nextest runs each test in its own process, and nothing reads the environment yet.
            unsafe { std::env::set_var(name, &home) };
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(1440.), px(940.)),
            })),
            ..Default::default()
        };
        let adeline = cx.update(|cx| adeline_ui::start(true, verbose, options, cx));
        let window = cx.windows()[0];
        let mut ui = Self {
            cx,
            window,
            adeline,
        };
        ui.settle();
        ui
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx
            .update_window(self.window, |_, window, cx| f(window, cx))
            .expect("Adeline window")
    }

    fn settle(&mut self) {
        self.cx.run_until_parked();
        self.with(|window, cx| window.render_frame(cx));
    }

    fn press(&mut self, keys: &str) {
        self.with(|window, cx| window.press(keys, cx));
        self.settle();
    }

    fn input(&mut self, text: &str) {
        self.with(|window, cx| window.input(text, cx));
        self.settle();
    }

    fn try_named(&mut self, label: &str) -> Option<ElementSnapshot> {
        self.with(|window, _| {
            snapshots(window)
                .into_iter()
                .find(|element| element.label() == Some(label) && element.visible())
        })
    }

    fn named(&mut self, label: &str) -> ElementSnapshot {
        self.try_named(label)
            .unwrap_or_else(|| panic!("no element named {label:?}. Named: {:?}", self.names()))
    }

    fn gone(&mut self, label: &str) {
        assert!(self.try_named(label).is_none(), "{label:?} still shows");
    }

    fn names(&mut self) -> Vec<String> {
        self.with(|window, _| {
            let mut names: Vec<_> = snapshots(window)
                .iter()
                .filter_map(|element| element.label().map(str::to_owned))
                .collect();
            names.sort();
            names
        })
    }

    fn click(&mut self, label: &str) {
        let id = self.named(label).path().last().cloned().unwrap();
        self.with(|window, cx| window.click(id, cx));
        self.settle();
    }

    // The chat list has no shortcut to a chat by name, so this clicks its row.
    fn open_chat(&mut self, title: &str) {
        let prefix = format!("{title},");
        let id = self
            .with(|window, _| {
                snapshots(window).into_iter().find(|element| {
                    element
                        .label()
                        .is_some_and(|label| label.starts_with(&prefix))
                })
            })
            .unwrap_or_else(|| panic!("no chat {title:?}. Named: {:?}", self.names()))
            .path()
            .last()
            .cloned()
            .unwrap();
        self.with(|window, cx| window.click(id, cx));
        self.settle();
        let open = self
            .cx
            .read(|cx| self.adeline.read(cx).open_chat().map(str::to_owned));
        assert_eq!(open.as_deref(), Some(title));
    }

    fn chat_option(&mut self, category: Category) -> Option<String> {
        self.cx
            .read(|cx| self.adeline.read(cx).chat_option(category))
    }
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
    ui.open_chat("Make document edits safe when the preview is still updating");
    ui.named("Model: Opus");
    ui.press(&format!("{MENU}-m"));
    ui.named("Search model");
    ui.input("Sonnet");
    ui.press("enter");
    ui.named("Model: Sonnet");
    assert_eq!(ui.chat_option(Category::Model).as_deref(), Some("sonnet"));
    ui.press(&format!("{MENU}-o"));
    ui.named("Search mode");
    ui.input("Accept");
    ui.press("enter");
    ui.named("Mode: Accept edits");
    assert_eq!(
        ui.chat_option(Category::Mode).as_deref(),
        Some("acceptEdits")
    );
    ui.press(&format!("{MENU}-p"));
    ui.named("Search more options");
    ui.press("escape");
    ui.gone("Search more options");

    // An error under a chat can be dismissed by hand. No shortcut reaches Dismiss.
    ui.open_chat("Keep the composer usable in a narrow desktop window");
    ui.click("Dismiss error");
    ui.gone("Dismiss error");

    // Demo mode never starts an engine.
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
