# Completion

When application code changes, run the required lint, checks and tests, then rebuild the runnable app with `cargo build --release --locked` before reporting completion. Confirm that the build succeeds.

When a feature or bug fix is implemented and passes those checks, start `scripts/package.ps1`, the slower shipping build, in the background so a fresh dist package gets built. Don't wait for it before reporting completion. Report whether it succeeded once it finishes.

Changes limited to documentation or agent instructions do not require application tests or a rebuild.
