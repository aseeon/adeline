# Completion

When application code changes, run the required lint, checks and tests, then rebuild the runnable app with `cargo build --release --locked` before reporting completion. Confirm that the build succeeds.

When a feature or bug fix is implemented and passes those checks, start `scripts/package.ps1`, the slower shipping build, in the background so a fresh dist package gets built. Don't wait for it before reporting completion. Report whether it succeeded once it finishes.

Changes limited to documentation or agent instructions do not require application tests or a rebuild.

# Versioning

Bump the version in `Cargo.toml` with every impactful change. A user-visible bug fix or behavior change bumps the patch (third) number. A big new feature or rework bumps the minor (second) number and resets the patch to 0. Leave the major (first) number unchanged. Documentation, agent-instruction, and internal-only changes keep the current version. Commit the bump with the change it versions, together with the updated `Cargo.lock` (`--locked` builds fail on a stale lock).

# UI testing

Test UI with Python scripts driving UI Automation whenever possible. Fall back to computer use only for what UI Automation cannot reach.
