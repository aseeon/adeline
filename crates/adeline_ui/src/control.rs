//! The control endpoint of a control build (`control` feature).
//!
//! `adeline --control` listens on its own pipe or socket, and `adeline ui <command>`
//! sends it one JSON request and prints the reply. Requests run on the UI thread in
//! arrival order.
//!
//! Elements are found by accessibility label, as GPUI Kit observes them. `find`,
//! `click`, `press` and `type_text` are shared with the UI tests' `Ui` helper and
//! the pixel tests, so the command line and the tests follow one rule.
//!
//! `--hidden` keeps the window off screen. The platform draws no frames for a
//! hidden window, so each request draws one itself, as the Windows pixel test does.
use crate::client;
use adeline_core::{ipc, platform};
use anyhow::{Context as _, bail};
use gpui_kit::base::test_support::{ElementSnapshot, snapshots};
use gpui_kit::*;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FRAME_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "lowercase")]
enum Request {
    Info,
    Tree,
    Press {
        keys: String,
    },
    Type {
        text: String,
    },
    Click {
        label: String,
    },
    Wait {
        label: String,
        gone: bool,
        timeout_ms: u64,
    },
    Shot {
        path: PathBuf,
        element: Option<String>,
    },
}

/// The visible elements labelled exactly `label`.
pub fn visible(window: &Window, label: &str) -> Vec<ElementSnapshot> {
    snapshots(window)
        .into_iter()
        .filter(|element| element.visible() && element.label() == Some(label))
        .collect()
}

/// The one visible element labelled `label`. No match, or several, is an error
/// that names the candidates.
pub fn find(window: &Window, label: &str) -> Result<ElementSnapshot> {
    let mut found = visible(window, label);
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => {
            let mut names: Vec<_> = snapshots(window)
                .iter()
                .filter(|element| element.visible())
                .filter_map(|element| element.label().map(str::to_owned))
                .collect();
            names.sort();
            names.dedup();
            bail!("No visible element is labelled {label:?}. Visible labels: {names:?}")
        }
        _ => {
            let at: Vec<_> = found.iter().map(|element| element.bounds()).collect();
            bail!(
                "{} visible elements are labelled {label:?}, at {at:?}",
                found.len()
            )
        }
    }
}

pub fn click(window: &mut Window, cx: &mut App, position: Point<Pixels>) {
    let modifiers = Modifiers::default();
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            modifiers,
            pressed_button: None,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

/// Presses keys in GPUI's syntax, several separated by spaces: `ctrl-n escape`.
pub fn press(window: &mut Window, cx: &mut App, keys: &str) -> Result<()> {
    for key in keys.split_whitespace() {
        let keystroke =
            Keystroke::parse(key).map_err(|error| anyhow::anyhow!("{key:?}: {error}"))?;
        window.dispatch_keystroke(keystroke, cx);
    }
    Ok(())
}

pub fn type_text(window: &mut Window, cx: &mut App, text: &str) {
    for character in text.chars() {
        let text = character.to_string();
        let Ok(mut keystroke) = Keystroke::parse(&text) else {
            continue;
        };
        keystroke.key_char = Some(text);
        window.dispatch_keystroke(keystroke, cx);
    }
}

/// The part of a frame captured at `scale` that `bounds` covers, cut to the frame.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "device pixels, clamped to the frame"
)]
pub fn crop(frame: &RgbaImage, bounds: Bounds<Pixels>, scale: f32) -> Option<RgbaImage> {
    let edge = |logical: Pixels, limit: u32| {
        ((logical.as_f32() * scale).round().max(0.) as u32).min(limit)
    };
    let left = edge(bounds.origin.x, frame.width());
    let top = edge(bounds.origin.y, frame.height());
    let right = edge(bounds.origin.x + bounds.size.width, frame.width());
    let bottom = edge(bounds.origin.y + bounds.size.height, frame.height());
    (right > left && bottom > top)
        .then(|| image::imageops::crop_imm(frame, left, top, right - left, bottom - top).to_image())
}

/// The window's last frame at device pixel size, or the part showing `element`.
pub fn capture(window: &Window, element: Option<&str>) -> Result<RgbaImage> {
    if cfg!(target_os = "linux") {
        bail!("The official GPUI release can't capture a Linux window yet");
    }
    let frame = window.render_to_image()?;
    let Some(label) = element else {
        return Ok(frame);
    };
    let found = find(window, label)?;
    crop(&frame, found.bounds(), window.scale_factor())
        .with_context(|| format!("{label:?} lies outside the window"))
}

fn tree(window: &Window) -> Value {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let elements: Vec<_> = snapshots(window)
        .iter()
        .map(|element| {
            let bounds = element.bounds();
            // `focused` panics on an element whose focus binding was missed.
            let focused =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| element.focused()))
                    .ok()
                    .flatten();
            json!({
                "label": element.label(),
                "role": element.role().map(|role| format!("{role:?}")),
                "value": element.value(),
                "bounds": {
                    "x": bounds.origin.x.as_f32(),
                    "y": bounds.origin.y.as_f32(),
                    "width": bounds.size.width.as_f32(),
                    "height": bounds.size.height.as_f32(),
                },
                "visible": element.visible(),
                "focused": focused,
                "disabled": element.disabled(),
                "checked": element.checked(),
                "selected": element.selected(),
                "expanded": element.expanded(),
            })
        })
        .collect();
    std::panic::set_hook(hook);
    json!(elements)
}

struct Hidden;
impl Global for Hidden {}

/// Opens this UI's control endpoint, removed again when the app quits.
pub fn serve(demo: bool, hidden: bool, cx: &mut App) -> Result<()> {
    if hidden {
        cx.set_global(Hidden);
    }
    let listener = client::runtime().block_on(async { ipc::Listener::bind_control() })?;
    let (requests, inbox) = async_channel::unbounded();
    client::runtime().spawn(accept(listener, requests));
    #[cfg(unix)]
    cx.on_app_quit(|_| {
        if let Ok(address) = ipc::control_address(std::process::id()) {
            let _ = std::fs::remove_file(address);
        }
        async {}
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Ok((request, reply)) = inbox.recv().await {
            let value = handle(request, demo, cx)
                .await
                .unwrap_or_else(|error| json!({ "error": format!("{error:#}") }));
            let _ = reply.send(value).await;
        }
    })
    .detach();
    Ok(())
}

async fn accept(
    mut listener: ipc::Listener,
    requests: async_channel::Sender<(Request, async_channel::Sender<Value>)>,
) {
    while let Ok((mut reader, mut writer)) = listener.accept().await {
        let requests = requests.clone();
        tokio::spawn(async move {
            let Ok(Some(line)) = reader.next_line().await else {
                return;
            };
            let reply = match serde_json::from_str(&line) {
                Ok(request) => {
                    let (reply, answer) = async_channel::bounded(1);
                    if requests.send((request, reply)).await.is_err() {
                        return;
                    }
                    answer
                        .recv()
                        .await
                        .unwrap_or_else(|_| json!({ "error": "Adeline is quitting" }))
                }
                Err(error) => json!({ "error": format!("Bad request: {error}") }),
            };
            let _ = ipc::send(&mut writer, &reply).await;
        });
    }
}

/// Waits until the window has drawn a frame, or `FRAME_TIMEOUT` for a window
/// the platform doesn't draw, such as a minimized one.
async fn next_frame(window: AnyWindowHandle, cx: &mut AsyncApp) -> Result<()> {
    if cx.update(|cx| cx.has_global::<Hidden>()) {
        return cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    }
    let (drawn, frame) = async_channel::bounded(1);
    let timeout = drawn.clone();
    let executor = cx.background_executor().clone();
    cx.background_executor()
        .spawn(async move {
            executor.timer(FRAME_TIMEOUT).await;
            let _ = timeout.try_send(());
        })
        .detach();
    cx.update_window(window, |_, window, _| {
        window.on_next_frame(move |_, _| {
            let _ = drawn.try_send(());
        });
    })?;
    let _ = frame.recv().await;
    Ok(())
}

async fn handle(request: Request, demo: bool, cx: &mut AsyncApp) -> Result<Value> {
    let window = cx
        .update(|cx| cx.active_window().or_else(|| cx.windows().first().copied()))
        .context("Adeline has no window")?;
    next_frame(window, cx).await?;
    let done = json!({});
    match request {
        Request::Info => cx.update_window(window, |_, window, _| {
            let size = window.viewport_size();
            json!({
                "pid": std::process::id(),
                "mode": if demo { "demo" } else { "live" },
                "width": size.width.as_f32(),
                "height": size.height.as_f32(),
            })
        }),
        Request::Tree => cx.update_window(window, |_, window, _| tree(window)),
        Request::Press { keys } => {
            cx.update_window(window, |_, window, cx| press(window, cx, &keys))??;
            Ok(done)
        }
        Request::Type { text } => {
            cx.update_window(window, |_, window, cx| type_text(window, cx, &text))?;
            Ok(done)
        }
        Request::Click { label } => {
            cx.update_window(window, |_, window, cx| -> Result<()> {
                let element = find(window, &label)?;
                click(window, cx, element.bounds().center());
                Ok(())
            })??;
            Ok(done)
        }
        Request::Wait {
            label,
            gone,
            timeout_ms,
        } => {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            loop {
                let shows =
                    cx.update_window(window, |_, window, _| !visible(window, &label).is_empty())?;
                if shows != gone {
                    return Ok(done);
                }
                if Instant::now() >= deadline {
                    let what = if gone { "to go" } else { "to show" };
                    bail!("Timed out after {timeout_ms} ms waiting for {label:?} {what}");
                }
                next_frame(window, cx).await?;
            }
        }
        Request::Shot { path, element } => {
            let image =
                cx.update_window(window, |_, window, _| capture(window, element.as_deref()))??;
            image
                .save(&path)
                .with_context(|| format!("Cannot save {}", path.display()))?;
            Ok(json!({ "path": path }))
        }
    }
}

/// `adeline ui <command>`: prints one JSON reply and returns the exit code.
pub fn cli(args: &[String]) -> i32 {
    platform::attach_parent_console();
    match run_cli(args.to_vec()) {
        Ok(value) => {
            println!("{value:#}");
            0
        }
        Err(error) => {
            println!("{}", json!({ "error": format!("{error:#}") }));
            1
        }
    }
}

const USAGE: &str = "Usage: adeline ui [--pid <n>] list | tree | press <keys> | type <text> | click <label> | wait <label> [--gone] [--timeout <ms>] | shot [file.png] [--element <label>]";

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|arg| arg != flag);
    args.len() != before
}

fn take_option(args: &mut Vec<String>, option: &str) -> Result<Option<String>> {
    let Some(at) = args.iter().position(|arg| arg == option) else {
        return Ok(None);
    };
    if at + 1 >= args.len() {
        bail!("{option} needs a value");
    }
    let value = args.remove(at + 1);
    args.remove(at);
    Ok(Some(value))
}

fn request(command: &str, mut args: Vec<String>) -> Result<Request> {
    Ok(match command {
        "tree" => Request::Tree,
        "press" => Request::Press {
            keys: args.join(" "),
        },
        "type" => Request::Type {
            text: args.join(" "),
        },
        "click" => Request::Click {
            label: args.join(" "),
        },
        "wait" => {
            let gone = take_flag(&mut args, "--gone");
            let timeout_ms = take_option(&mut args, "--timeout")?
                .map(|ms| ms.parse().context("--timeout takes milliseconds"))
                .transpose()?
                .unwrap_or(5000);
            Request::Wait {
                label: args.join(" "),
                gone,
                timeout_ms,
            }
        }
        "shot" => {
            let element = take_option(&mut args, "--element")?;
            let file = args.first().map_or("adeline-shot.png", String::as_str);
            Request::Shot {
                path: std::path::absolute(file)?,
                element,
            }
        }
        _ => bail!("{USAGE}"),
    })
}

async fn ask(pid: u32, request: &Request) -> std::io::Result<Value> {
    let (mut reader, mut writer) =
        ipc::connect_to(&ipc::control_address(pid).map_err(std::io::Error::other)?).await?;
    ipc::send(&mut writer, request).await?;
    let line = reader
        .next_line()
        .await?
        .ok_or_else(|| std::io::Error::other("The control UI closed the connection"))?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

// The control UIs that answer, by process ID. Sockets left by UIs that died are removed.
async fn running() -> Result<Vec<(u32, Value)>> {
    let mut found = Vec::new();
    for pid in ipc::control_pids()? {
        match ask(pid, &Request::Info).await {
            Ok(info) => found.push((pid, info)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                #[cfg(unix)]
                let _ = std::fs::remove_file(ipc::control_address(pid)?);
            }
            Err(error) => bail!("UI {pid}: {error}"),
        }
    }
    Ok(found)
}

fn run_cli(mut args: Vec<String>) -> Result<Value> {
    let pid: Option<u32> = take_option(&mut args, "--pid")?
        .map(|pid| pid.parse().context("--pid takes a process ID"))
        .transpose()?;
    if args.is_empty() {
        bail!("{USAGE}");
    }
    let command = args.remove(0);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        if command == "list" {
            let infos: Vec<_> = running().await?.into_iter().map(|(_, info)| info).collect();
            return Ok(json!(infos));
        }
        let request = request(&command, args)?;
        let pid = if let Some(pid) = pid {
            pid
        } else {
            let pids: Vec<_> = running().await?.into_iter().map(|(pid, _)| pid).collect();
            match pids[..] {
                [pid] => pid,
                [] => bail!(
                    "No control UI is running. Start one from a control build with --control, for example `adeline --demo --control`"
                ),
                _ => bail!("Several control UIs are running: {pids:?}. Pick one with --pid <n>"),
            }
        };
        let reply = ask(pid, &request)
            .await
            .with_context(|| format!("Cannot reach control UI {pid}"))?;
        if let Some(error) = reply.get("error").and_then(Value::as_str) {
            bail!("{error}");
        }
        Ok(reply)
    })
}
