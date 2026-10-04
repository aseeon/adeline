# Engine checks

End-to-end checks for `docs/archive/scope-conversation-engine.md`. Windows only, Python 3 stdlib.

1. `cargo build --release --locked` (the runner never builds).
2. `python scripts/engine-check/run_checks.py` runs every check; `--fast` skips the ~90 s background/idle-exit check, `--only AC11` filters by name, `--keep` keeps the temp home.
3. Each run uses a fresh temp home (`USERPROFILE`/`HOME`), so your real engine and data are never touched. Exit code 1 means a check failed.

`fake_agent.py` is a scriptable ACP agent (prompt keywords `SLOW n`, `PERMISSION`, `FAIL n`, `HANG_ON_CLOSE`, `NO_FORK`); `python scripts/engine-check/test_fake_agent.py` checks it on its own.
`ui_check.py` drives the real window through UI Automation (pywinauto), including `--demo`. Add new UI checks to it with `@check("...")`. Run it with `python scripts/engine-check/ui_check.py` after the release build.
`engine_client.py` is the protocol client; `python scripts/engine-check/engine_client.py` prints the pipe name for the current user.
