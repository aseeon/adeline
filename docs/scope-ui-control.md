Status: Draft

# UI control and pixel tests

## Purpose and context

Agents that change Adeline's UI can check their work in two ways today. Headless GPUI tests (`crates/adeline_ui/tests/ui.rs`, `crates/adeline/tests/ui.rs`) check labels, focus and bounds, but never pixels. Computer use checks the real window, but it is slow, needs a desktop in front, and drives the user's real pointer. Nothing in between lets an agent ask the running app what it shows, or prove that something was drawn.

Users are the agents and people who develop Adeline, on Windows and macOS.

Intended outcome:

1. A development build of Adeline that an agent can inspect and drive from the command line: list what the window shows, press shortcuts, type, click by label and save the current frame as a PNG.
2. Tests that render real frames on the GPU and check their pixels, run by `cargo nextest` in CI.

Herdr (`herdrdev/herdr-app`) has both: `herdr-app ls/status/do/shot` and `tests/shot_pixels.rs`. It gets frame capture from a patched copy of gpui-pre. Adeline stays on the official gpui-kit 0.7.0 and gpui-pre 0.3.7 releases.

What the official releases provide:

- `Window::render_to_image()` draws the last frame's scene into a texture and reads it back as an `RgbaImage`, without presenting it. The window can be covered. The real Windows window implements it with DirectX (`gpui-pre-windows`) and the real macOS window with Metal (`gpui-pre-macos`). The real Linux window doesn't implement it in 0.3.7 and returns an error.
- `HeadlessAppContext::with_platform(text_system, assets, renderer)` runs GPUI on its test platform with real text shaping. `gpui_platform::current_headless_renderer()` gives it a GPU renderer: Metal on macOS, wgpu on Linux, none on Windows. `capture_screenshot(window)` then returns real pixels.
- `gpui_kit::base::test_support::snapshots(window)` lists the observed elements of the last paint: label, role, value, bounds, visibility, focus, disabled. The `Ui` helper in `crates/adeline_ui/tests/support/mod.rs` builds on it.
- `Window::dispatch_keystroke` and `Window::simulate_mouse_move` are public. Clicks go through `Window::dispatch_event`.

All of these except `dispatch_keystroke` sit behind the `test-support` feature of gpui-pre, gpui-platform and gpui-base, which `gpui-kit/test-support` turns on. In a real app that feature is mostly inert. The executor keeps the platform scheduler when the dispatcher isn't a test one. It adds two costs: entity leak bookkeeping (`leak-detection`) and an element registry for every `.test_support()` element. So it belongs in development builds only, and never in the dist package.

Current behavior:

- `main.rs` (`crates/adeline`) knows `engine`, `bridge` and `--version`. Anything else opens the UI. Several UI processes can run at once, and none of them listens for anything.
- `Action` (`crates/adeline_ui/src/lib.rs`) is private and addresses projects and chats by index, so it is no stable surface for a command line.
- `ipc.rs` already makes same-user named pipes on Windows and Unix sockets elsewhere.
- `scripts/engine-check/ui_check.py` drives the real release window on Windows through UI Automation.

## Requirements

### Control build

- R1. A cargo feature `control` on `adeline` and `adeline_ui` turns on `gpui-kit/test-support` and compiles in the control endpoint. The local release build (`cargo build --release --locked --features control`) has it. Default builds, the headless build and `scripts/package.ps1` / the dist profile leave it off.
- R2. A UI process opens the control endpoint only when started with `--control`, for example `adeline --demo --control`. Started without it, a control build opens nothing, so the everyday app can't be reached. A build without the feature rejects `--control` with a message saying it needs a control build.
- R3. With `--control`, the UI listens on a same-user pipe or socket of its own, named after its process ID, next to the engine's in `ipc::engine_dir()`. It removes it on exit.
- R4. `adeline ui <command>` connects to that endpoint, sends one JSON request, prints one JSON reply and exits 0, or prints `{"error": "..."}` and exits 1. With one control UI running it picks that one. With several it fails and lists them, and `--pid <n>` picks one. With none it says to start Adeline with `--control`.
- R5. The flag gates the endpoint only. The test-support bookkeeping (entity leak tracking and the element registry) is compiled into every control build and runs with or without the flag.
- R6. Requests run on the UI thread between frames, in arrival order, like any other window event.

### Commands

- R7. `adeline ui list` prints the running control UIs: process ID, demo or live, window size.
- R8. `adeline ui tree` prints every observed element of the focused window: label, role, value, bounds, visible, focused, disabled, checked, selected, expanded. This is what `Ui::elements` sees in tests.
- R9. `adeline ui press <keys>` dispatches keystrokes in GPUI's syntax (`ctrl-n`, `cmd-shift-p`, several separated by spaces), as `Ui::press` does. Shortcuts from `docs/shortcuts.md` are the way to move around, in line with AGENTS.md.
- R10. `adeline ui type <text>` types into the focused element.
- R11. `adeline ui click <label>` clicks the centre of the one visible element with that label. No match, or several, fails and names the candidates. The command line, the `Ui` helper and computer use then share one rule: a control without a shortcut gets clicked, and the missing shortcut gets reported.
- R12. `adeline ui wait <label> [--gone] [--timeout <ms>]` returns once the element appears, or disappears, or fails at the timeout (default 5 s).
- R13. `adeline ui shot [file.png] [--element <label>]` saves the focused window's last frame at device pixel size and prints its absolute path. The default is `adeline-shot.png` in the current folder. With `--element` it saves only that element: its bounds from `snapshots`, scaled to device pixels and cut to the window. It finds the element by the rule in R11. On Linux it fails with a message saying the official GPUI release can't capture a Linux window yet.
- R14. The find, click and wait logic lives once in `adeline_ui` behind `control`, and the test `Ui` helper calls it, so tests and the command line can't drift apart.

### Pixel tests

- R15. `crates/adeline_ui/tests/pixels.rs` opens the real `Adeline` view in demo mode in a `HeadlessAppContext` with the platform's real text system and `current_headless_renderer()`, at a fixed size and scale.
- R16. Tests assert pixel facts a user would notice, measured inside an element's bounds from `snapshots`: a region is drawn in the expected theme color, text or an icon left pixels that differ from the background, a selection or focus ring shows, a Mermaid diagram rendered. No whole-frame golden images, since fonts and antialiasing differ between OSes and drivers.
- R17. A failing test writes its frame as a PNG under `target/pixel-failures/<test>.png` and names the path.
- R18. CI runs them on macOS (Metal) and Linux (wgpu on Mesa's lavapipe, from `mesa-vulkan-drivers`). On Windows `current_headless_renderer()` returns none, so the tests print that and pass without checking.
- R19. On Windows, one smoke test in `crates/adeline_ui/tests/pixels_windows.rs` covers the DirectX renderer and DirectWrite text. It is a test binary with `harness = false`, so it owns the real Windows run loop. It opens the demo in a hidden window (`show: false`), waits until the window has drawn, captures it with `Window::render_to_image()` and checks a few pixel facts: the window drew in the theme colors, text left pixels, an icon drew. It finds elements with the logic shared with `adeline ui` (R14), since the `Ui` helper only drives the test platform. A failure writes its frame as in R17. The test is built only on Windows.
- R20. Before the rest of R19 is built, one capture runs on a GitHub `windows-latest` runner. That runner has no GPU, so this confirms that DXGI offers a software adapter there and that `render_to_image` returns real pixels. If it doesn't, the smoke test runs locally only and CI skips it with a message.

### Docs

- R21. `docs/architecture.md` describes the control endpoint and pixel tests. AGENTS.md's "UI testing" section says when to use `adeline ui` and pixel tests before computer use, adds `--features control` to its release build command, and says to launch with `--control` for these checks.

## Out of scope

- Driving the UI through `Action` or a list of named commands. Shortcuts and labels already cover it, and they test what a user does.
- Remote control from another machine. The endpoint is local and same-user.
- Linux capture of a live window, until a GPUI release implements `render_to_image` for it.
- Golden image comparison.
- Cropping `shot` to coordinates. `--element` covers checking one control.
- The full pixel suite on Windows. It would need the real run loop for every test, losing the test platform's fixed scheduler and clock. The R19 smoke test covers what only Windows draws.
