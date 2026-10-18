# Engine checks

End-to-end checks for `docs/archive/scope-conversation-engine.md` and `docs/archive/scope-acp-sdk-rebuild.md`. Windows only, Python 3 stdlib.

1. `cargo build --release --locked` (the runner never builds).
2. `python scripts/engine-check/run_checks.py` runs every check; `--fast` skips the ~90 s background/idle-exit check, `--only AC11` filters by name, `--keep` keeps the temp home.
3. Each run uses a fresh temp home (`USERPROFILE`/`HOME`), so your real engine and data are never touched. Exit code 1 means a check failed.

`fake_agent.py` is a scriptable ACP agent (prompt keywords such as `SLOW n`, `PERMISSION`, `FAIL n`, `EXIT`, `SILENT`; its docstring lists them all), shared with the cross-platform engine tests in `crates/adeline/tests/engine.rs`; `python scripts/engine-check/test_fake_agent.py` checks it on its own. Its failures use JSON-RPC code -32603: ACP reserves -32000 for "log in first", which Adeline doesn't retry.
`ui_check.py` is the smoke test of the real release window through UI Automation (pywinauto): it opens a demo chat and quits with the close button. Every other UI check is a headless `cargo nextest` test (see `AGENTS.md`, "UI testing"). Run it with `python scripts/engine-check/ui_check.py` after the release build.
`fake_ssh.py` stands in for `ssh` in the remote tests of `crates/adeline/tests/ui.rs` (`ADELINE_SSH`), on every OS: each destination is a separate home in the test's temp folder with its own engine, reached the way this OS's OpenSSH runs commands. Host name prefixes simulate timeouts, changed or unknown host keys, passwords and ARM hosts; see its docstring.
`perf_check.py` measures frame times while it drives the window (idle, scroll, chat switching, typing, hover, resize, a fast `STREAM` reply). It needs the profiling build, `cargo build --profile dist --locked --features profiler --target-dir target/profiling`, whose `ADELINE_FRAME_LOG` writes every drawn frame to a CSV. Keep the window in front: GPUI caps an unfocused window at about 30 fps. A UI Automation client switches on the window's accessibility tree, and with it on the window caches no view, so the measured launch is driven by positions read from an earlier launch, without UI Automation. `--a11y` measures with the tree on, as a screen reader would see it. `--only idle,typing` runs only the named scenarios.
`engine_client.py` is the protocol client; `python scripts/engine-check/engine_client.py` prints the pipe name for the current user.
