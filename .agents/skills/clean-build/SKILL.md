---
name: clean-build
description: Delete stale build output from target/ in the checkout and its worktrees. Use when asked to clean up build output, clear target/, or free disk space.
---

1. Run `scripts/clean-build` in bash (Git Bash on Windows). It deletes everything in `target/` of the main checkout and of each worktree under `.claude/worktrees/`, except `target/release` (the runnable app) and the macOS package `target/dist/Adeline.app`. The Windows package `dist/Adeline.exe` sits outside `target/`, so it stays. If the script says a build, test or debug Adeline is running, tell the user and stop.
2. In the main checkout, find what the script does not reach with `git status --short --ignored`: extra target folders made by earlier agents (a `CARGO_TARGET_DIR` such as `target-*/`) and loose logs and screenshots. Delete those. Keep `dist/`, `.claude/worktrees/`, `.env*` files and anything you can't identify, and list the ones you kept for the user.
3. In a worktree, touch only `target/`, since its branch may hold unmerged commits.

The cleanup is done when the script has run without skipping, and every ignored or untracked path left in the main checkout is one you kept on purpose.
