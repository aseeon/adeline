# Coding Standards

Design with maintainability in mind and implement minimal code that solves the problem. Don't overengineer. Reduce complexity, embrace simplicity. 

Prefer composition over inherticance. Prefer decoupling and clear separation of concerns. 

Prefer sparse - top of the module level comments. Avoid class and function level comments. Code should be self-evident.

# Navigation

`docs/architecture.md` traces a user action from UI to engine to agent and back, and covers demo mode and assets. Read it before your first code change in a session, or when you need to find where something lives. When a change moves a module or alters a flow it describes, update it in the same commit.

# Scope docs

Scopes in `docs/` are live specs, with `Status: Draft` or `Status: Confirmed`. In the commit that ships a scope, change its first line to `Status: Implemented in vX.Y.Z. This scope is history. Where it and the code differ, the code is right.` using the version that commit sets, then `git mv` it to `docs/archive/` and update any paths that point to it. Before relying on a claim from an archived scope, check it against the code.

Research reports go in `docs/reports/`, and the notes behind each report go in `docs/research_notes/<report title>/`.

# Completion

Agents develop Adeline on Windows and macOS. Keep instructions OS agnostic, and name the per-OS command wherever one differs.

When application code changes, run the checks CI runs, then rebuild the runnable app with `cargo build --release --locked` before reporting completion. Confirm that the build succeeds. The checks:

- `cargo fmt --all -- --check`
- `scripts/check-todos` (a bash script, run it through Git Bash on Windows)
- clippy: `scripts/clippy.ps1` on Windows, `scripts/clippy` on macOS and Linux (checks the full and the headless build)
- `cargo nextest run --locked --workspace --all-features`
- `cargo nextest run --locked --workspace --exclude adeline_ui --no-default-features` (the headless build)

Local clippy only sees code for the current OS. Code behind `cfg(target_os = ...)` for another OS is checked only by CI.

When a feature or bug fix is implemented and passes those checks, start the slower shipping build in the background so a fresh dist package gets built: `scripts/package.ps1` on Windows, `cargo build --profile dist --locked && scripts/macos-app` on macOS (builds `target/dist/Adeline.app`), `cargo build --profile dist --locked` on Linux. Don't wait for it before reporting completion. Report whether it succeeded once it finishes.

Changes limited to documentation or agent instructions do not require application tests or a rebuild.

# Versioning

Bump the version in `[workspace.package]` in the root `Cargo.toml` with every impactful change. A user-visible bug fix, behavior change or new feature bumps the patch (third) number. Bump the minor (second) number, resetting the patch to 0, only for a new Mode or a rework the user calls one. Leave the major (first) number unchanged. Documentation, agent-instruction, and internal-only changes keep the current version. Commit the bump with the change it versions, together with the updated `Cargo.lock` (`--locked` builds fail on a stale lock).

Bump `PROTOCOL` in `crates/adeline_core/src/protocol.rs` in the commit that changes the shape of any message between the engine and its clients. A client and an engine work together exactly when their `PROTOCOL` matches, whatever their Adeline versions, so an unbumped shape change breaks remote machines running another version. `crates/adeline_core/tests/protocol.rs` pins every message's shape and fails on such a change; its header says how to record the new shape.

# Testing

Test behavior a user or another machine would notice: state machines, failures, persistence, the engine through its protocol. No tests of constants, styling, one-line helpers or framework internals.

Prefer high-impact integration tests: one test that drives a real flow across modules, through the protocol or the UI, over several unit tests of its parts. Write a unit test where the logic is intricate enough that an integration test would not pin down which case broke.

# UI testing

On Windows, test UI by extending `scripts/engine-check/ui_check.py`, which drives the real window through UI Automation (pywinauto), `--demo` included. See `scripts/engine-check/README.md`. Fall back to computer use only for what UI Automation cannot reach. The `scripts/engine-check/` suite is Windows only, so on macOS use computer use for UI checks.

Navigate the app with keyboard shortcuts (listed in `docs/shortcuts.md`) wherever one exists, in UI Automation and computer use alike, rather than moving the pointer and clicking. Each time a test has to click because no shortcut reaches a control, tell the user which shortcut is missing, then add it.
