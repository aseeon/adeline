# Tests and control builds

Tests drive the real binary, the real `Adeline` view and a fake agent, so they check what a user or another machine would notice. A control build lets agents drive the running app the same way the tests do.

## Process per test

nextest runs each test in its own process, so a test may set environment variables and its own temporary home. That is also why `std::env::set_var` is sound in the test helpers. The UI tests against live engines connect, install and wait on remote machines, so they get four times the default slow timeout (`.config/nextest.toml`).

## Keeping GPUI out of headless tests

- `crates/adeline/tests/ui.rs` is built only with the `test-support` feature, so the headless tests stay free of GPUI.
- `crates/adeline/tests/headless.rs` runs in the headless test step (`--no-default-features`), where `CARGO_BIN_EXE_adeline` is the headless build.

## One way to find and drive elements

- The control endpoint, the UI tests' `Ui` helper and the pixel tests share `control::find`, `click`, `press` and `type_text`. The command line and the tests then follow one rule.
- Elements are found by accessibility label, as GPUI Kit observes them.
- `adeline --control` listens on its own pipe or socket, and `adeline ui <command>` sends it one JSON request and prints the reply. Requests run on the UI thread in arrival order.
- `--hidden` keeps the window off screen. The platform draws no frames for a hidden window, so each request draws one itself, as the Windows pixel test does.

## Waiting on dialogs

- A Kit dialog stays mounted while it fades out, and its focus trap keeps a click outside it from moving focus until then. The `Ui` helper waits for that.
- A fade advances once per tick, and a slow runner ticks less often, so the wait is capped by time (10 s), not by ticks.

## Pixel tests

- Each pixel test checks what a user would notice inside an element's bounds. A failure saves its frame under `target/pixel-failures`.
- There are no golden images, because fonts and antialiasing differ between OSes and drivers.
- macOS builds its text system on the main thread, and the test harness doesn't run tests there. So `pixels.rs` has no harness (`harness = false`): its `main` answers nextest's `--list` and runs the test it names.
- Windows has no headless renderer, so `pixels.rs` passes there without checking. `pixels_windows.rs` owns the real run loop in a hidden window and covers DirectX instead. It also has no harness and answers `--list` itself.

## The engine-check scripts

`scripts/engine-check/` holds the Windows end-to-end checks and their fakes.

- `engine_client.py` reads `PROTOCOL` from `protocol.rs`, so the Rust constant stays the only copy.
- It rebuilds the engine's pipe name the way the Rust code does: FNV-1a 64 over the lowercased UTF-8 bytes of `USERPROFILE` joined with `.config/adeline`. It copies `Path::join`'s rule, which adds a separator unless the base is empty, ends in one, or is a bare drive (`C:`). The engine reads `USERPROFILE`, so a test sets it before connecting to reach a test engine.
- Its reader thread polls with `PeekNamedPipe`, so a read never blocks the synchronous pipe handle while another thread writes. Error 231 (`ERROR_PIPE_BUSY`) means the next pipe instance isn't ready yet, and a broken pipe means the engine went away or the client closed.
- `run_checks.py` sets `USERPROFILE` and `HOME` for itself, so every `adeline.exe` it starts, and every agent the engine starts, inherits the temporary home. Each check starts and ends with no engine running, and a string a check returns is reported.
- `run_checks.py` first runs `adeline engine status` with a timeout, because a build without the engine subcommand would open the UI instead.
- Its `kill` uses `SIGTERM`, which Python turns into `TerminateProcess` on Windows.
- The resume check builds a long history first, so a full snapshot would be large and a resume shows.
- The permission check without a client waits for the request before closing the client in one run, and closes first in the other, so the request arrives after the client left.
- `fake_agent.py` runs each `session/prompt` on its own thread, so cancels and answers keep flowing. It creates the cancel event when the prompt arrives, in arrival order, so a cancel right after the prompt isn't lost. Its `FAIL` counter lives on disk, so it survives a restart of the agent. A message without a `method` is a response to one of the fake agent's own requests.
- `fake_ssh.py` gives each remote machine its own environment, without the client's `ADELINE_ASKPASS`, `ADELINE_EXE` and `SSH_ASKPASS`. On Unix each session gets its own process group, so a test can drop the whole session.
