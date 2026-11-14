//! Shared by the UI tests here and in `crates/adeline/tests`: the real `Adeline`
//! window on GPUI's test platform, found by accessibility labels (the names a user
//! and UI Automation see) and driven by shortcuts from `docs/shortcuts.md`. Finding,
//! clicking, pressing and typing go through `adeline_ui::control`, which `adeline ui`
//! uses too.
#![allow(dead_code, reason = "each test file uses part of it")]
#![expect(
    clippy::disallowed_methods,
    reason = "tests wait for other processes off any real UI thread"
)]

use adeline_ui::control;
use gpui_kit::base::test_support::{ElementSnapshot, snapshots};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, ClipboardItem, ElementId, Entity, KeyDownEvent,
    KeyUpEvent, Keystroke, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const MENU: &str = if cfg!(target_os = "macos") {
    "cmd-shift"
} else {
    "alt"
};
pub const END_OF_TEXT: &str = if cfg!(target_os = "macos") {
    "cmd-down"
} else {
    "ctrl-end"
};
pub const START_OF_TEXT: &str = if cfg!(target_os = "macos") {
    "cmd-up"
} else {
    "ctrl-home"
};

#[expect(
    unsafe_code,
    reason = "Adeline reads its home and binary from the environment"
)]
pub fn set_env(name: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: nextest runs each test in its own process, and nothing reads the environment yet.
    unsafe { std::env::set_var(name, value) };
}

pub fn set_home(home: &std::path::Path) {
    set_env("HOME", home);
    set_env("USERPROFILE", home);
}

pub fn temp_home(name: &str) -> PathBuf {
    let temp = if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let home = temp.join(format!("adeline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    set_home(&home);
    home
}

pub fn options(width: f32, height: f32) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(width), px(height)),
        })),
        ..Default::default()
    }
}

pub struct Ui<'a> {
    pub cx: &'a mut TestAppContext,
    pub window: AnyWindowHandle,
    pub main: AnyWindowHandle,
    pub adeline: Entity<adeline_ui::Adeline>,
}

impl<'a> Ui<'a> {
    pub fn open(cx: &'a mut TestAppContext, demo: bool, verbose: bool) -> Self {
        Self::open_sized(cx, demo, verbose, 1440., 940.)
    }

    pub fn open_sized(
        cx: &'a mut TestAppContext,
        demo: bool,
        verbose: bool,
        width: f32,
        height: f32,
    ) -> Self {
        if !demo {
            cx.executor().allow_parking();
        }
        let before = cx.windows();
        let adeline = cx.update(|cx| adeline_ui::start(demo, verbose, options(width, height), cx));
        let window = *cx
            .windows()
            .iter()
            .find(|window| !before.contains(window))
            .expect("Adeline window");
        let mut ui = Self {
            cx,
            window,
            main: window,
            adeline,
        };
        ui.settle();
        ui
    }

    pub fn demo(cx: &'a mut TestAppContext, verbose: bool) -> Self {
        temp_home("ui");
        Self::open(cx, true, verbose)
    }

    pub fn with<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx
            .update_window(self.window, |_, window, cx| f(window, cx))
            .expect("open window")
    }

    pub fn read<R>(&mut self, f: impl FnOnce(&adeline_ui::Adeline, &App) -> R) -> R {
        let adeline = self.adeline.clone();
        self.cx.read(|cx| f(adeline.read(cx), cx))
    }

    pub fn settle(&mut self) {
        self.cx.run_until_parked();
        if self.cx.windows().contains(&self.window) {
            self.with(|window, cx| window.draw(cx).clear(cx));
            self.cx.run_until_parked();
        }
    }

    pub fn pause(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.tick();
        }
    }

    fn tick(&mut self) {
        std::thread::sleep(Duration::from_millis(20));
        self.cx.executor().advance_clock(Duration::from_millis(20));
        self.settle();
    }

    pub fn wait(&mut self, what: &str, timeout: Duration, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            if done(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}. Named: {:?}",
                self.names()
            );
            self.tick();
        }
    }

    pub fn wait_named(&mut self, label: &str, timeout: Duration) -> ElementSnapshot {
        self.wait(&format!("{label:?}"), timeout, |ui| {
            ui.try_named(label).is_some()
        });
        self.named(label)
    }

    pub fn wait_gone(&mut self, label: &str, timeout: Duration) {
        self.wait(&format!("{label:?} to go"), timeout, |ui| {
            ui.try_named(label).is_none()
        });
    }

    pub fn press(&mut self, keys: &str) {
        for key in keys.split_whitespace() {
            self.with(|window, cx| control::press(window, cx, key))
                .unwrap_or_else(|error| panic!("{error:#}"));
            self.settle();
        }
    }

    pub fn input(&mut self, text: &str) {
        self.with(|window, cx| control::type_text(window, cx, text));
        self.settle();
    }

    pub fn elements(&mut self) -> Vec<ElementSnapshot> {
        self.with(|window, _| snapshots(window))
    }

    pub fn find(&mut self, matches: impl Fn(&str) -> bool) -> Option<ElementSnapshot> {
        self.elements()
            .into_iter()
            .find(|element| element.visible() && element.label().is_some_and(&matches))
    }

    pub fn all(&mut self, matches: impl Fn(&str) -> bool) -> Vec<ElementSnapshot> {
        self.elements()
            .into_iter()
            .filter(|element| element.visible() && element.label().is_some_and(&matches))
            .collect()
    }

    pub fn try_named(&mut self, label: &str) -> Option<ElementSnapshot> {
        self.with(|window, _| control::visible(window, label).into_iter().next())
    }

    pub fn named(&mut self, label: &str) -> ElementSnapshot {
        self.try_named(label)
            .unwrap_or_else(|| panic!("no element named {label:?}. Named: {:?}", self.names()))
    }

    pub fn containing(&mut self, text: &str) -> Option<ElementSnapshot> {
        self.find(|name| name.contains(text))
    }

    pub fn gone(&mut self, label: &str) {
        assert!(self.try_named(label).is_none(), "{label:?} still shows");
    }

    pub fn names(&mut self) -> Vec<String> {
        let mut names: Vec<_> = self
            .elements()
            .iter()
            .filter(|element| element.visible())
            .filter_map(|element| element.label().map(str::to_owned))
            .collect();
        names.sort();
        names
    }

    pub fn unique(&mut self, label: &str) -> ElementSnapshot {
        self.with(|window, _| control::find(window, label))
            .unwrap_or_else(|error| panic!("{error:#}"))
    }

    pub fn click(&mut self, label: &str) {
        let mut element = self.unique(label);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.tick();
            let now = self.unique(label);
            let settled = now.bounds() == element.bounds() && !self.covered(&now);
            if settled || Instant::now() > deadline {
                break;
            }
            element = now;
        }
        self.click_element(&element);
    }

    // A Kit dialog stays mounted while it fades out, and its focus trap keeps a
    // click outside it from moving focus until then.
    fn covered(&mut self, element: &ElementSnapshot) -> bool {
        let dialog = |path: &[ElementId]| {
            path.iter()
                .find(|id| matches!(id, ElementId::NamedInteger(name, _) if name == "dialog-instance"))
                .cloned()
        };
        let target = dialog(element.path());
        self.elements()
            .iter()
            .any(|other| dialog(other.path()).is_some_and(|open| Some(open) != target))
    }

    pub fn click_element(&mut self, element: &ElementSnapshot) {
        self.click_at(element.bounds().center());
    }

    pub fn hover(&mut self, element: &ElementSnapshot) {
        self.move_to(element.bounds().center());
    }

    pub fn click_at(&mut self, position: Point<Pixels>) {
        self.with(|window, cx| control::click(window, cx, position));
        self.settle();
    }

    pub fn move_to(&mut self, position: Point<Pixels>) {
        VisualTestContext::from_window(self.window, self.cx).simulate_mouse_move(
            position,
            None,
            Modifiers::default(),
        );
        self.settle();
    }

    pub fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        let mut cx = VisualTestContext::from_window(self.window, self.cx);
        cx.simulate_mouse_move(from, None, Modifiers::default());
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        for step in 1..=10 {
            let at = from + (to - from) * (step as f32 / 10.);
            cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::default());
        }
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        self.settle();
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.cx
            .simulate_window_resize(self.window, size(px(width), px(height)));
        self.settle();
    }

    pub fn quit(&mut self) {
        self.press("cmd-q");
    }

    pub fn close(&mut self) {
        VisualTestContext::from_window(self.main, self.cx).simulate_close();
        self.settle();
    }

    pub fn is_open(&mut self) -> bool {
        self.cx.windows().contains(&self.main)
    }

    pub fn clipboard(&mut self) -> String {
        self.cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default()
    }

    pub fn set_clipboard(&mut self, text: &str) {
        self.cx
            .write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    }

    pub fn new_window(&mut self, before: &[AnyWindowHandle]) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(window) = self.cx.windows().into_iter().find(|w| !before.contains(w)) {
                self.window = window;
                self.settle();
                return;
            }
            assert!(Instant::now() < deadline, "no new window opened");
            self.tick();
        }
    }

    pub fn main_window(&mut self) {
        self.window = self.main;
        self.settle();
    }

    pub fn open_chat(&mut self, title: &str) {
        self.press("ctrl-f secondary-a backspace");
        self.input(title);
        self.press("enter");
        let open = self.read(|app, _| app.open_chat().map(str::to_owned));
        assert_eq!(open.as_deref(), Some(title));
    }

    pub fn tab_to(&mut self, label: &str) {
        self.with(|window, _| window.activate_window());
        for _ in 0..200 {
            if self
                .all(|name| name == label)
                .iter()
                .any(|element| element.focused() == Some(true))
            {
                return;
            }
            self.press("tab");
        }
        panic!("Tab never reached {label:?}. Named: {:?}", self.names());
    }

    pub fn activate(&mut self, label: &str) {
        self.tab_to(label);
        let key = Keystroke::parse("space").expect("Space is a keystroke");
        let mut cx = VisualTestContext::from_window(self.window, self.cx);
        cx.simulate_event(KeyDownEvent {
            keystroke: key.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke: key });
        self.settle();
    }

    pub fn open_tab(&mut self, name: &str) {
        for _ in 0..=self.tabs().len() {
            if self.tab(name).selected() == Some(true) {
                return;
            }
            self.press("ctrl-tab");
        }
        panic!("Ctrl+Tab never reached the tab {name:?}");
    }

    pub fn tabs(&mut self) -> Vec<String> {
        self.elements()
            .into_iter()
            .filter(|element| element.visible() && element.role() == Some(gpui_kit::Role::Tab))
            .filter_map(|element| {
                let label = element.label()?;
                let trimmed = match label.rfind(" (") {
                    Some(at)
                        if label.ends_with(')')
                            && label[at + 2..label.len() - 1]
                                .chars()
                                .all(|c| c.is_ascii_digit()) =>
                    {
                        &label[..at]
                    }
                    _ => label,
                };
                Some(trimmed.to_owned())
            })
            .collect()
    }

    pub fn tab(&mut self, name: &str) -> ElementSnapshot {
        let tabs = self.tabs();
        self.elements()
            .into_iter()
            .find(|element| {
                element.role() == Some(gpui_kit::Role::Tab)
                    && element.label().is_some_and(|label| {
                        label == name || label.starts_with(&format!("{name} ("))
                    })
            })
            .unwrap_or_else(|| panic!("no tab {name:?}. Tabs: {tabs:?}"))
    }
}
