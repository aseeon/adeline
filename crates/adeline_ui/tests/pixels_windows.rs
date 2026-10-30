//! The Windows pixel smoke test: the demo in a hidden real window, drawn by
//! DirectX with DirectWrite text, captured with `Window::render_to_image`. It owns
//! the real run loop, so it has no test harness and answers nextest's `--list`
//! itself, like `pixels.rs`. Other OSes run the full suite in `pixels.rs`.
#[cfg(windows)]
#[path = "support/pixels.rs"]
mod pixels;
#[cfg(windows)]
mod support;

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use adeline_ui::control;
    use gpui_kit::component::ActiveTheme as _;
    use gpui_kit::*;
    use pixels::{Frame, check_demo};
    use std::time::Duration;

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--list") {
        if !args.iter().any(|arg| arg == "--ignored") {
            println!("smoke: test");
        }
        return;
    }
    support::temp_home("pixels-windows");
    application().with_assets(adeline_ui::Assets).run(|cx| {
        let options = WindowOptions {
            show: false,
            focus: false,
            ..support::options(1440., 940.)
        };
        adeline_ui::start(true, false, options, cx);
        let window = cx.windows()[0];
        cx.spawn(async move |cx| {
            // A hidden window keeps Windows' default placement until it shows, and
            // gets no frames from the platform: size it and draw a frame here.
            cx.update_window(window, |_, window, _| {
                window.resize(size(px(1440.), px(940.)));
            })
            .expect("resize the window");
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            cx.update_window(window, |_, window, cx| {
                window.draw(cx).clear(cx);
                let frame = Frame {
                    image: window.render_to_image().expect("capture the window"),
                    scale: window.scale_factor(),
                    test: "pixels_windows",
                };
                let mut find = |label: &str| {
                    control::find(window, label)
                        .unwrap_or_else(|error| frame.fail(&format!("{error:#}")))
                };
                check_demo(&frame, &mut find, cx.theme().background);
            })
            .expect("update the window");
            println!("smoke: ok");
            std::process::exit(0);
        })
        .detach();
    });
}
