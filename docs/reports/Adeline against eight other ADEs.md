# Adeline against eight other ADEs

Compared on 2026-11-13: Warp, Zeron, Ghostex, Waku, Arbor, diri, Okena and monocode. Notes per repo are in `docs/research_notes/Adeline against eight other ADEs/`. The scouts read READMEs, trees and manifests, not source code.

## Where Adeline already stands well

Six of the eight also split a daemon from the UI, so Adeline's engine design is the norm. Adeline still leads on a few points. It speaks ACP to any agent from the registry, where most rivals write one adapter per CLI. It has remote machines over SSH with headless self-install, saved agent profiles, forking, recovery, plain YAML config and a Docs mode. Steering, queueing and context usage events already exist.

## What every strong rival has and Adeline lacks

All eight have at least three of these. Adeline has none.

| Gap | Who has it |
| --- | --- |
| Git worktree per chat | Zeron, Ghostex, Waku, Arbor, diri, Okena, monocode |
| Diff review of the agent's changes | Zeron, Ghostex, Waku, Arbor, diri, Okena, monocode |
| OS notifications when an agent finishes or waits | Zeron, Ghostex, Arbor, diri, monocode |
| Embedded terminal | all eight |
| Usage and rate limits page | Warp, Waku, diri, Okena, monocode |

## Recommendations, in order

1. **Worktree per chat.** An option on New Chat: create a worktree and branch under the project, run the agent there, and show the branch in the chat header. On Complete or Archive, warn about unpushed or uncommitted work, then offer merge, keep or delete. The engine owns it, so it works on remote machines too. Shell out to `git` like diri does. This is the feature that makes parallel agents safe, and every rival treats it as core.
2. **Changes view.** A side panel tab with the chat's diff against its base: file list with +/- counts, unified or side-by-side diff, and a "changed files" line under each agent reply (Waku). Use `imara-diff` or `similar` with `syntect`. Add Commit and Push buttons after that.
3. **Attention.** OS notifications from the engine when a turn ends, fails or asks for permission, sent only when no window has focus (Zeron). A taskbar or Dock badge with the waiting count. Sort or mark waiting chats in the chat list, and add a "next waiting chat" shortcut (diri). Cheap, since the engine already knows each chat's phase.
4. **Checkpoints and rewind.** Snapshot the worktree at each turn (a hidden commit or `git stash create`) so Retry or Fork from an earlier message also restores the files (Waku, monocode). This builds on forking and needs item 1.
5. **Usage page.** Adeline already receives `UsageUpdate`. Show context use per chat, then a per-agent history, then Claude and Codex rate-limit windows (Okena).
6. **Doc annotations.** In Docs mode, tag passages ("Clarify", "Needs tests") and send them to a chat as numbered notes (Ghostex). It fits Docs mode with little new code.
7. **Engine MCP server.** Expose engine commands as MCP tools so an agent can start chats on other worktrees and read their results (Arbor, diri, monocode `/operator`). This gives coordinator and worker setups without new UI.

## Later or skip

- Embedded terminal. Every rival has one (`alacritty_terminal` + `portable-pty` is the common stack), but it is a big build. A "Open terminal here" button that launches the system terminal in the chat's worktree covers most needs first.
- Issue and PR inbox, CI status (Arbor, monocode). Worth it after worktrees and diffs.
- Mobile and web clients, cloud sync (Zeron, Ghostex, Okena). Large, and SSH machines cover the remote case today.
- Import sessions started in the agent CLI (Waku `/resume`), if ACP `session/load` allows it.
- Secret redaction in transcripts (Warp).
- Self-update with rollback (Zeron), since the engine outlives the UI.
