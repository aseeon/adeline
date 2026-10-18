Status: Draft

# Headless UI tests

## Purpose and context

UI checks today run only on Windows, through UI Automation (`scripts/engine-check/ui_check.py`, pywinauto). They need a release build, a real desktop and a window in front, and they take minutes. CI doesn't run them. On macOS the only UI check is computer use.

Users are the agents and people who change Adeline's UI, on Windows and macOS.

Intended outcome: every one of the 21 checks in `ui_check.py` becomes a `cargo nextest` test that runs on every OS in CI, with no window on screen and no automation client. `ui_check.py` shrinks to one smoke test of the real window.

Current behavior:

- `adeline_ui` has no GPUI tests. `crates/adeline/tests/engine.rs` tests the engine end to end through `adeline bridge` in a temporary home, with `scripts/engine-check/fake_agent.py` as the agent.
- `adeline_ui::open` (`crates/adeline_ui/src/lib.rs`) does all setup inside `application().run(...)`: globals, fonts, config, theme, harnesses, the engine connection (`client::init`), key bindings, then the window with `Root`. A test can't call it.
- `ipc.rs` starts the local engine by running `std::env::current_exe()`, and remote installs copy `current_exe()` (`remote.rs`). In a test process that is the test binary.
- `ADELINE_SSH` swaps the `ssh` command. `fake_ssh.py` copies how Windows OpenSSH runs commands.
- The gpui-kit 0.7.0 `test-support` feature runs GPUI on its test platform: a fake window, display and clipboard, a deterministic scheduler and a fake clock. `gpui_kit::test::TestWindowExt` finds elements by ID and dispatches clicks, keys and text. GPUI's `VisualTestContext` adds `simulate_keystrokes`, `simulate_resize` and `debug_bounds`. `TestAppContext::disable_accessibility()` turns the accessibility tree off. View caching is part of GPUI, so it runs on the test platform too. Zed tests its whole UI this way.

## Requirements

### Harness

- R1. `adeline_ui` exposes one setup function that both `open` and tests call. It takes the demo and verbose flags and builds everything `open` builds, apart from the platform application and the native window options.
- R2. A test opens the real `Adeline` view in a headless window of a set size, wrapped in `Root`, in demo mode or against a live engine.
- R3. Tests drive the UI through key bindings from `docs/shortcuts.md` wherever one exists, and through clicks, drags and text input otherwise.
- R4. Tests assert what a user would notice: element labels and values, focus, visibility and bounds, the fake clipboard, and the `Adeline` model.
- R5. Elements a test finds get a stable ID. Custom elements that `find` can't see opt in with `.test_support()`.
- R6. Live engine tests run the real binary in a temporary home, with `fake_agent.py` as the agent, the way `engine.rs` does. The engine helpers move into a module both test files share.
- R7. Every test runs on Windows, macOS and Linux CI.

### Starting Adeline from a test

- R8. When `ADELINE_EXE` is set, `ipc.rs` starts the engine from that path, and `remote.rs` installs that binary on remote machines, in place of `current_exe()`. Tests set it to `CARGO_BIN_EXE_adeline`. Unset, nothing changes.

### Demo checks

These go in `crates/adeline_ui/tests/`.

- R9. `check_demo_conversation`: TODO list, queue, traffic tab, hang notice, the model, mode and more-options menus, dismissing an error, and no engine.
- R10. `check_traffic_needs_verbose`.
- R11. `check_docs_demo`: tree, Viewer, Editor, save, notes into the composer, chooser, doc cards, a Viewer selection staying out of the conversation, the unsaved-docs prompt on quit, and the reopened doc in a second app.
- R12. `check_demo_machines`: machine labels, show and hide, the disconnected look, and the Settings machine dropdown.
- R13. `check_selection_stays_in_panel`, with a demo doc and a demo chat.
- R14. `check_close_tab`, through hover and the close button.
- R15. `check_tabs_fold`: in a narrow window (`simulate_resize`) the inactive tabs shorten, the rest fold into the "+N more projects" menu, and the Projects menu stays clear of the window controls. The test then widens and narrows the window again, and opens a second window at the narrow size, and checks that the tabs have the same bounds each time.
- R16. `check_transcript_selection`: a selection across a reply survives hovering the chat list, and copies the same text before and after.

R15 and R16 replace the pixel comparisons with bounds and copied text. GPUI's test platform never connects assistive technology (its window's `a11y_init` does nothing), so every headless test draws the regions as cached views, the path the view caching bugs were on. The uncached path that a screen reader gets is left to the smoke test (R28). `find` still sees elements, because the Kit reads their accessibility properties directly. The test helper draws frames the way the platform does, redrawing only notified views: Kit's `render_frame` and its interaction helpers refresh the whole window first, which would hide a caching bug.

### Engine checks

These go in `crates/adeline/tests/ui.rs`, which can reach the binary through `CARGO_BIN_EXE_adeline`. A dev-dependency on the UI would build GPUI into the headless tests too, so the file is a `[[test]]` that requires the binary's `test-support` feature (`gui` plus GPUI's test platform). `--all-features` builds it, and the headless run skips it. It includes the demo tests' helper from `crates/adeline_ui/tests/support/mod.rs` by path.

- R17. `check_launch`: the UI starts the engine (R8), and the engine runs as a process separate from the test.
- R18. `check_quit_background`: the quit request shows the dialog, Cancel keeps the app open, and Finish in background quits it. The turn finishes with no app open, and a second app reuses the same engine.
- R19. `check_quit_idle` and `check_quit_stop_all`.
- R20. `check_settings_engine`: Stop engine, the stopped banner, the UI not restarting the engine on its own, and Start engine.
- R21. `check_unavailable`.
- R22. `check_fork`, both the engine half and the demo half.

### Remote checks

These go in `crates/adeline/tests/`.

- R23. The fake SSH runs on every OS. It is a Rust test helper binary, or `fake_ssh.py` reworked to run commands the way each OS's OpenSSH does. Each destination keeps its own temporary home with its own engine, and host name prefixes still simulate timeouts, changed or unknown host keys, passwords and ARM hosts.
- R24. `check_demo_machines` moves in R12. These move here: `check_add_machine`, `check_drop`, `check_ssh_failures` and `check_upgrade_and_browse`.
- R25. `check_upgrade_and_browse` needs Adeline 0.1.0 for the current OS. The test downloads it once into a cache folder and is `#[ignore]` by default, so CI doesn't depend on the network. A separate CI step runs it with `--run-ignored`.

### Headless build checks

- R26. `check_headless_cli` runs in the headless nextest step (`--no-default-features`), where `CARGO_BIN_EXE_adeline` is the headless build. It checks the usage text and exit code 2, the version marker, `engine status` with `Build: headless`, and on Windows that the binary is a console program (PE subsystem 3). The "engine shows no window" assertion is dropped. The headless build has no GPUI, so it can't open one.
- R27. `check_headless_remote` needs the full and the headless build at once. CI builds the headless binary first (`cargo build --locked --no-default-features --target-dir target/headless`) and passes its path in `ADELINE_HEADLESS_EXE`. The test installs it on a fake remote, then connects, runs a prompt and reconnects. Without the variable the test is skipped with a message naming the build command.

### What stays in ui_check.py

- R28. One smoke test of the real release window, through UI Automation: launch in demo mode, open a chat, quit. It covers what the test platform can't: the native window and its close button, the title bar controls, real drawing, and the accessibility tree as UI Automation shows it.
- R29. Each check is deleted from `ui_check.py` in the commit that adds its Rust replacement. When all 21 are gone, `ui_check.py` holds only R28.

### Improving the suite

- R30. Flows with timing or ordering, such as queueing, reconnecting and the hang notice, use the fake clock (`advance_clock`) and `run_until_parked` in place of sleeps.
- R31. Race-prone demo flows run with several scheduler seeds (`iterations`). A failure prints its seed, and `SEED=n` replays it.
- R32. Each control a test has to click because no shortcut reaches it is reported to the user, then the shortcut is added. Known gaps from `ui_check.py`: opening a chat by title from the list (the chooser covers it), doc cards, Fork, Dismiss error, Note on selection, the close button on a project tab, and the machine selector.

## Boundaries

### Excluded

- Pixel tests. Zed's screenshot tests draw with Metal and run only on macOS. GPUI's headless renderer supports macOS and Linux and returns nothing on Windows.
- IME composition and the macOS menu bar.
- Performance. `perf_check.py` stays as it is, and so does `run_checks.py`.

### Interactions with existing functionality

- The headless build stays free of GPUI. `adeline_ui` stays out of it, and the new dev-dependencies sit under the `gui` side only.
- Every UI feature has to work in demo mode and against an engine (`docs/architecture.md`), so demo tests cover most UI code without an engine.
- Tests that read or write process globals (config, machines, the engine connection, `ADELINE_EXE`) rely on nextest running each test in its own process.
- `ADELINE_EXE` is read only where Adeline runs its own binary. Users never need it.

### Rejected ideas

- Running the release binary under a virtual display (Xvfb) and driving it with an accessibility client. It is still slow, still per OS, and still out of process.
- A test-only switch in production code that turns behavior off. `ADELINE_EXE` swaps a path and keeps the behavior.

## Phases

1. Harness: R1 to R5, the dev-dependency, and `check_demo_conversation` (R9), to prove the harness on all three OSes in CI.
2. Demo checks: R10 to R16.
3. Engine checks: R6, R8 and R17 to R22.
4. Remote and headless build checks: R23 to R27. Then `ui_check.py` drops to the smoke test (R28).
5. Suite improvements (R30 to R32), and updates to `AGENTS.md` ("UI testing"), `scripts/engine-check/README.md` and `docs/architecture.md`.

Each phase deletes the Python checks it replaces (R29).

## Interfaces and dependencies

- `gpui-kit = { workspace = true, features = ["test-support"] }` under `[dev-dependencies]` in `crates/adeline_ui/Cargo.toml` and `crates/adeline/Cargo.toml`.
- `gpui_kit::test::TestWindowExt` and `TestAppContextExt`, and GPUI's `TestAppContext` and `VisualTestContext` from gpui-pre 0.3.7.
- `fake_agent.py` (Python 3 on PATH, as `engine.rs` already needs).
- `ADELINE_EXE` (R8) and `ADELINE_HEADLESS_EXE` (R27).
- `.github/workflows/ci.yml`: the headless build before the full nextest step (R27), and the ignored-test step (R25).

## Acceptance criteria

- AC1. `cargo nextest run --locked --workspace --all-features` runs the UI tests on Windows, macOS and Linux CI, with no window shown.
- AC2. `open` and the tests call the same setup function.
- AC3. Each of the 21 checks in today's `ui_check.py` has a Rust test, and `ui_check.py` holds only the smoke test.
- AC4. Each ported test fails when its feature breaks. For each one, break the feature once by hand and see the test fail.
- AC5. The view caching tests (R15, R16) fail when a cached view draws stale content. Check this by reverting a caching fix once. Result: replacing the title bar's deferred notify in `project_bar/tabs.rs` with a plain refresh fails R15. Caching the whole transcript again does not fail R16, so the selection loss described in `docs/archive/handoff-view-caching.md` no longer happens with gpui-kit 0.7.
- AC6. A UI test failure with several seeds prints the seed, and `SEED=n` reproduces it.
- AC7. The remote tests pass on Windows, macOS and Linux with the fake SSH.
- AC8. The headless nextest run (`--exclude adeline_ui --no-default-features`) passes, builds no GPUI, and runs the headless CLI test.
- AC9. `AGENTS.md` says to write headless UI tests, and to use `ui_check.py` only for the smoke test.

## Decisions and rationale

- Demo mode first. It needs no engine, so its tests are fast and run the same on every OS. It covers the same views as the engine path.
- Engine and remote tests live in `crates/adeline/tests/`, because only the binary crate's tests get `CARGO_BIN_EXE_adeline`.
- `ADELINE_EXE` over pre-starting the engine. It lets a test check that the UI starts the engine itself, and the same variable covers remote installs.
- Bounds and copied text in place of pixels for the view caching checks. The bug they guard against changes layout and selection, which the test platform reproduces. Pixels would need a renderer that doesn't exist on Windows.
- Remotes last. They need the most new pieces (R8, R23), and they are the slowest and flakiest checks today.
- One smoke test stays in `ui_check.py`, so a release build is still checked once as a real window.

## Open questions

- Answered in phase 1: `config::init` and `fonts::init` both work on the test platform. `config::init` reads and seeds `settings.yml` under `HOME` (`USERPROFILE` on Windows), so each test points both at its own temporary folder. That is safe because nextest runs each test in its own process. `fonts::init` adds the bundled Chivo fonts to the test platform's text system and lists its font names. `TestAppContext` has no asset source, so SVG icons don't load, and no check depends on them.
- Answered in phase 1: Kit components such as `Button` and `Input` register themselves with their accessibility labels. Plain elements need `.test_support()` after their `.id(...)`: so far the chat rows, the TODO list, the queue and its items, the hang notice and the ACP traffic log. Many IDs hold generated project and chat IDs, so `tests/ui.rs` finds elements by accessibility label, the same names UI Automation used, through `gpui_kit::base::test_support::snapshots`. That function is `#[doc(hidden)]`, so a gpui-kit upgrade can change it. `ElementSnapshot::focused()` panics on an element whose focus binding wasn't observed, so ask it only of elements that track focus.
- Answered in phase 2: `VisualTestContext::simulate_close` runs the window's close handler, as the close button does, so a test reaches the quit dialog without calling `interaction::request_close`. Quitting removes the main window, so a test checks `cx.windows()`.
- Answered in phase 3: close enough. `quit_background` quits with Finish in background, sees the turn finish, then opens a second app in the same process, which reuses the engine. The first app's connection stays open until the test process exits, so the engine never sees that client leave mid-test. Clicks wait until their target stops moving, because dialogs slide in on wall-clock time and an early click misses. Engine tests call `allow_parking()`, because the engine connection wakes the UI from its own threads.
- Answered in phase 4: a reworked `fake_ssh.py`, since `fake_agent.py` needs Python anyway. It runs the remote command through cmd.exe on Windows and `/bin/sh -c` elsewhere, fakes `uname` for ARM hosts on Unix, and drops the client's `ADELINE_EXE` and askpass variables, as real SSH would. It records each bridge session, so a test drops the connection by killing the bridge (on Windows only the bridge: killing its process tree would kill the engine it started). `ADELINE_EXE` makes a remote install copy that binary in place of downloading the release. The live UI tests get 4 minutes each in `.config/nextest.toml`.
