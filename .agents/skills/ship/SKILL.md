---
name: ship
description: Commit, push, and watch the GitHub CI run until it passes. Use when asked to commit and push, watch or observe CI, or fix CI until it passes.
---

Run the commands in bash (Git Bash on Windows).

1. If application code changed and the Completion checks in `AGENTS.md` have not passed since the last edit, run them now, along with the version bump that `AGENTS.md` asks for.
2. Commit on the branch you are on. Split unrelated changes into separate commits. Commit messages need no `[skip ci]`: the `paths-ignore` list in `.github/workflows/ci.yml` already skips pushes that touch only docs or skills.
3. Push.
4. Skip steps 4 to 6 if the user said not to watch CI, or if the push touched only paths in `paths-ignore`. Otherwise find the run for the pushed commit: `gh run list --commit "$(git rev-parse HEAD)" --json databaseId,status,url`. It can take a few seconds to appear. Cancel any older run on the same branch that is still in progress (`gh run cancel <id>`).
5. Start one watcher as a background command: `gh run watch <id> --exit-status --interval 30 > /dev/null 2>&1; gh run view <id>`. Wait for its notification. Keep exactly one watcher alive per run.
6. If the run fails, read `gh run view <id> --log-failed`, fix the root cause, run that check locally, and go back to step 2. A failure that does not reproduce locally and looks like a flake (a UI check the user may have disturbed, a timing test) gets one `gh run rerun <id> --failed` first. Stop after 10 failed runs, or the limit the user gave, and report what still fails.

You are done when the CI run for `HEAD` is green, or nothing needed CI. Report the run number and URL.
