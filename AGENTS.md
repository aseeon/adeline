# Completion

When application code changes, run the required lint, checks and tests, then rebuild the runnable app with `cargo build --release --locked` before reporting completion. Confirm that the build succeeds. Run `scripts/package.ps1`, the slower shipping build, only when asked to package.

Changes limited to documentation or agent instructions do not require application tests or a rebuild.
