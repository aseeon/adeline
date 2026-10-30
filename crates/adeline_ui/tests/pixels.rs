//! Pixel tests: the real `Adeline` view in demo mode on GPUI's test platform, with
//! the platform's real text system and a real GPU renderer (Metal on macOS, wgpu on
//! Linux), at a fixed size and scale. Each test checks what a user would notice
//! inside an element's bounds, and a failure saves its frame under
//! `target/pixel-failures`. Windows has no headless renderer, so these tests pass
//! there without checking, and `pixels_windows.rs` covers DirectX instead.
//!
//! macOS builds its text system on the main thread, so this binary has no test
//! harness: `main` answers nextest's `--list` and runs the test it names.
#[path = "support/pixels.rs"]
mod pixels;
mod support;

use adeline_ui::control;
use gpui_kit::base::test_support::ElementSnapshot;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::platform::{current_headless_renderer, current_platform};
use gpui_kit::{AnyWindowHandle, HeadlessAppContext};
use pixels::{Frame, check_demo, dominant, drawn_on, same};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Headless {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    test: &'static str,
}

impl Headless {
    fn open(test: &'static str) -> Option<Self> {
        if current_headless_renderer()
            .expect("start the headless renderer")
            .is_none()
        {
            println!("This OS has no headless GPU renderer, so {test} checks nothing.");
            return None;
        }
        support::temp_home(test);
        let mut cx = HeadlessAppContext::with_platform(
            current_platform(true).text_system(),
            Arc::new(adeline_ui::Assets),
            current_headless_renderer,
        );
        let window = cx.update(|cx| {
            adeline_ui::start(true, false, support::options(1440., 940.), cx);
            cx.windows()[0]
        });
        let mut headless = Self { cx, window, test };
        headless.settle();
        Some(headless)
    }

    fn settle(&mut self) {
        self.cx.run_until_parked();
        self.cx
            .update_window(self.window, |_, window, cx| window.draw(cx).clear(cx))
            .expect("draw the window");
        self.cx.run_until_parked();
    }

    fn press(&mut self, keys: &str) {
        self.cx
            .update_window(self.window, |_, window, cx| {
                control::press(window, cx, keys)
            })
            .expect("open window")
            .unwrap_or_else(|error| panic!("{error:#}"));
        self.settle();
    }

    fn input(&mut self, text: &str) {
        self.cx
            .update_window(self.window, |_, window, cx| {
                control::type_text(window, cx, text);
            })
            .expect("open window");
        self.settle();
    }

    fn shows(&mut self, label: &str) -> bool {
        self.cx
            .update_window(self.window, |_, window, _| {
                !control::visible(window, label).is_empty()
            })
            .expect("open window")
    }

    // Waits in real time for work off the UI thread, such as diagrams.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test thread waits for other threads"
    )]
    fn wait(&mut self, label: &str, gone: bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.shows(label) == gone {
            assert!(Instant::now() < deadline, "timed out waiting for {label:?}");
            std::thread::sleep(Duration::from_millis(20));
            self.cx.advance_clock(Duration::from_millis(20));
            self.settle();
        }
    }

    fn find(&mut self, label: &str) -> ElementSnapshot {
        self.cx
            .update_window(self.window, |_, window, _| control::find(window, label))
            .expect("open window")
            .unwrap_or_else(|error| panic!("{error:#}"))
    }

    fn frame(&mut self) -> Frame {
        let image = self
            .cx
            .capture_screenshot(self.window)
            .expect("capture the window");
        let scale = self
            .cx
            .update_window(self.window, |_, window, _| window.scale_factor())
            .expect("open window");
        Frame {
            image,
            scale,
            test: self.test,
        }
    }

    fn theme_background(&mut self) -> gpui_kit::Hsla {
        self.cx.update(|cx| cx.theme().background)
    }
}

const TESTS: [(&str, fn()); 3] = [
    (
        "demo_chat_draws_in_theme_colors",
        demo_chat_draws_in_theme_colors,
    ),
    ("selected_chat_row_shows", selected_chat_row_shows),
    ("mermaid_diagram_renders", mermaid_diagram_renders),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--list") {
        if !args.iter().any(|arg| arg == "--ignored") {
            for (name, _) in TESTS {
                println!("{name}: test");
            }
        }
        return;
    }
    let only = args.iter().find(|arg| !arg.starts_with("--"));
    for (name, test) in TESTS {
        if only.is_none_or(|only| only == name) {
            test();
            println!("{name}: ok");
        }
    }
}

fn demo_chat_draws_in_theme_colors() {
    let Some(mut ui) = Headless::open("demo_chat_draws_in_theme_colors") else {
        return;
    };
    let background = ui.theme_background();
    let frame = ui.frame();
    check_demo(&frame, &mut |label| ui.find(label), background);
}

fn selected_chat_row_shows() {
    let Some(mut ui) = Headless::open("selected_chat_row_shows") else {
        return;
    };
    let frame = ui.frame();
    let selected = ui.find(
        "Search long conversations without pausing the interface, Claude Code, 2 messages, updated now, running",
    );
    let other = ui.find(
        "Import sessions from the old workspace format, Claude Code, 2 messages, updated 3m, running",
    );
    frame.check(selected.selected() == Some(true), || {
        "The open chat's row isn't selected".to_owned()
    });
    let (selected, other) = (
        dominant(&frame.region(&selected)),
        dominant(&frame.region(&other)),
    );
    frame.check(!same(selected, other), || {
        format!("The selected chat row looks like the others: {selected:?}")
    });
}

fn mermaid_diagram_renders() {
    let Some(mut ui) = Headless::open("mermaid_diagram_renders") else {
        return;
    };
    ui.press("ctrl-2");
    ui.press("ctrl-p");
    ui.input("diagrams");
    ui.press("down down enter");
    ui.wait("Mermaid diagram", false);
    ui.wait("Mermaid diagram, rendering", true);
    let frame = ui.frame();
    // The demo doc has two diagrams. Check the first.
    let first = ui
        .cx
        .update_window(ui.window, |_, window, _| {
            control::visible(window, "Mermaid diagram")
                .into_iter()
                .min_by(|a, b| {
                    a.bounds()
                        .origin
                        .y
                        .as_f32()
                        .total_cmp(&b.bounds().origin.y.as_f32())
                })
        })
        .expect("open window")
        .expect("a Mermaid diagram");
    let diagram = frame.region(&first);
    let behind = dominant(&diagram);
    frame.check(drawn_on(&diagram, behind) > 2000, || {
        "The Mermaid diagram drew nothing".to_owned()
    });
}
