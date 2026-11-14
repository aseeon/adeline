# Scout notes

Four Sonnet scouts read each repo's README, file tree, Cargo.toml and a few docs through `gh api` on 2026-11-13. Nobody cloned or ran the code. "Inferred" marks a claim drawn from file or crate names only.

## Warp (warpdotdev/warp)

Rust with its own UI framework, AGPLv3. Terminal first, with a built-in agent or any CLI agent. Crates show secret redaction, managed secrets, harness usage, MCP, LSP, a code editor, repo indexing, `.agents/` config, a skills lockfile and a web-compiled terminal (inferred from crate names). Too large and too terminal-centric to be a good source.

## Zeron (zeronsh/zeron)

Rust + GPUI, MIT. The closest peer: an engine daemon that outlives the UI, with an in-process fallback. The engine owns agents, terminals, worktrees and diffs. The Sessions sidebar sorts by attention. The engine sends desktop notifications for finished runs, questions and failures, and only when the window is in the background. Optional multi-device sync over Cloudflare (Loro CRDT). Transactional self-update with rollback. Android app.

## Ghostex (maddada/Ghostex)

Rust + GPUI with embedded Ghostty, CEF and code-server. Chat view and raw CLI view one hotkey apart. Annotation layer: tag Markdown or HTML passages with "Looks good / Clarify / Needs tests" and send them to the agent as numbered notes. Agents drive agents through a `ghostex` CLI, with a coordinator and a workers panel. Mid-task handoff to another agent. Phone apps with push on finish. Worktree modals (inferred).

## Waku (egoist/waku)

Rust + GPUI, GPL-3.0. Thin client over `waku-daemon` with a versioned WebSocket protocol, and TS types generated from it for a web client. Git checkpoints tied to turns, so rewind restores files and conversation. Steer or queue follow-ups, shown as cards. Inline diffs and a changed-files summary per response. PTY terminal, worktrees, a skills page that discovers each CLI's skills, a usage page and context gauge, `/resume` to import CLI sessions, commit messages from a cheap model.

## Arbor (penso/arbor)

Rust + GPUI, MIT. Worktrees created from GitHub or GitLab issues, with branch rules and an unpushed-commit warning on delete. Daemon-held PTY tabs per worktree. Side-by-side diff with PR cards and inline review comments, commit and push buttons. ACP agents plus OpenAI-compatible endpoints. Detects running agent processes. SSH and mosh remotes, an HTTP daemon and an MCP server. Crates: `alacritty_terminal`, `portable-pty`, `git2`, `gix`, `gix-diff`, `imara-diff`, `syntect`, `notify-rust`.

## diri (cristicretu/diri)

Rust + GPUI, AGPL-3.0. Many agents in terminals, local or over SSH. Agents defined as JSON manifests. A holder process per terminal survives restarts. Worktree and branch per task, diff review, commit, PR check follow-up. MCP server so agents spawn agents. Scheduled runs and folder triggers. Usage and cost dashboard. "Jump to next waiting agent". Uses the `git` and `gh` CLIs, with one batched GraphQL poll for PR checks.

## Okena (contember/okena)

Rust + GPUI + gpui-component. A terminal multiplexer for agent CLIs. Worktrees as projects with path templates and merge, stash or push on close. Git hooks that start an agent, for example on a rebase conflict. Unified and side-by-side diffs. Claude and Codex rate-limit display (5h and 7d windows). Crates: `alacritty_terminal`, `portable-pty`, `gix`.

## monocode (hardbeat920/monocode)

Tauri/React in production, with a GPUI native preview in the same repo. Eleven providers, each over its own transport behind one trait. Worktree per session, change review, checkpoints and rewind, steering. `/operator` agent that opens sessions and manages worktrees. Persistent agents with memory and schedules. Inbox of GitHub, GitLab, Linear and Jira items with CI status. Notifications and Dock badge. Crates: `alacritty_terminal`, `similar`, `tree-sitter`, `syntect`, `mermaid-rs-renderer`.
