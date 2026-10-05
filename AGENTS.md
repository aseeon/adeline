# Navigation

Before changing code that spans modules, read `docs/architecture.md`. It traces a user action from UI to engine to agent and back, and covers demo mode and assets.

# Scope docs

Scopes in `docs/` are live specs, with `Status: Draft` or `Status: Confirmed`. In the commit that ships a scope, change its first line to `Status: Implemented in vX.Y.Z. This scope is history. Where it and the code differ, the code is right.` using the version that commit sets, then `git mv` it to `docs/archive/` and update any paths that point to it. Before relying on a claim from an archived scope, check it against the code.

# Completion

Agents develop Adeline on Windows and macOS. Keep instructions OS agnostic, and name the per-OS command wherever one differs.

When application code changes, run the checks CI runs, then rebuild the runnable app with `cargo build --release --locked` before reporting completion. Confirm that the build succeeds. The checks:

- `cargo fmt --all -- --check`
- `scripts/check-todos` (a bash script, run it through Git Bash on Windows)
- clippy: `scripts/clippy.ps1` on Windows, `scripts/clippy` on macOS and Linux (checks the full and the headless build)
- `cargo nextest run --locked --all-features`
- `cargo nextest run --locked --no-default-features` (the headless build)

Local clippy only sees code for the current OS. Code behind `cfg(target_os = ...)` for another OS is checked only by CI.

When a feature or bug fix is implemented and passes those checks, start the slower shipping build in the background so a fresh dist package gets built: `scripts/package.ps1` on Windows, `cargo build --profile dist --locked && scripts/macos-app` on macOS (builds `target/dist/Adeline.app`), `cargo build --profile dist --locked` on Linux. Don't wait for it before reporting completion. Report whether it succeeded once it finishes.

Changes limited to documentation or agent instructions do not require application tests or a rebuild.

# Versioning

Bump the version in `Cargo.toml` with every impactful change. A user-visible bug fix, behavior change or new feature bumps the patch (third) number. Bump the minor (second) number, resetting the patch to 0, only for a new Mode or a rework the user calls one. Leave the major (first) number unchanged. Documentation, agent-instruction, and internal-only changes keep the current version. Commit the bump with the change it versions, together with the updated `Cargo.lock` (`--locked` builds fail on a stale lock).

Bump `PROTOCOL` in `src/protocol.rs` in the commit that changes the shape of any message between the engine and its clients. A client and an engine work together exactly when their `PROTOCOL` matches, whatever their Adeline versions, so an unbumped shape change breaks remote machines running another version.

# UI testing

On Windows, test UI by extending `scripts/engine-check/ui_check.py`, which drives the real window through UI Automation (pywinauto), `--demo` included. See `scripts/engine-check/README.md`. Fall back to computer use only for what UI Automation cannot reach. The `scripts/engine-check/` suite is Windows only, so on macOS use computer use for UI checks.
