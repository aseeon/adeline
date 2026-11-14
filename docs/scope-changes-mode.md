Status: Confirmed

# Changes mode

## Purpose and context

Agents edit a project's files, and today the user reviews those edits in another tool (an IDE or `git diff`), then retypes the feedback into the chat. Changes mode shows the project's uncommitted changes in Adeline, next to the conversation. The user reads the diff, comments on any lines of any file, and sends all the comments to the agent in one message.

Example flow: an agent edits `src/app.rs` and adds `src/cache.rs`. In Chats, the Activity panel shows "Changes +42 −7". The user clicks it. Changes mode opens on `src/app.rs`'s diff, with the same chat in the right island. The user selects lines 12–18 and writes "this retries forever, cap it at 3". A card `src/app.rs:12–18 · this retries forever…` appears in the composer. The user selects `README.md`'s file name in the tree, comments "mention the cache here", types "Fix these two" in the composer and sends. The agent gets one message: both comments with the lines they cover, then "Fix these two".

Current behavior:

- Modes sit on the mode rail behind flags in `crates/adeline_core/src/config.rs` (`Features`, `Features::enabled`, `Section`). Docs is on by default and the planned modes are off. The rail is drawn in `crates/adeline_ui/src/project_bar.rs`.
- Docs mode already has the three-part layout this mode reuses: a tree in the left island, the main pane, and a conversation with a chat chooser in the right island (`crates/adeline_ui/src/panes.rs`, `docs/view.rs`, `docs/archive/scope-docs-mode.md` R7, R26–R28). Its "note on a selection" puts a quote with the path and line range into the composer (`crates/adeline_ui/src/docs.rs`, the draft built around line 248).
- Docs reads, lists and watches project files only through the engine (`protocol::Command::ListDocs`, `ReadDoc`, `WatchDocs`, `Delta::Docs`, answered in `crates/adeline_engine/src/engine/docs.rs`), so remote projects work. `--demo` answers from `assets/demo-docs.json`.
- The composer is one input with one attachment list, shared by all chats (`crates/adeline_ui/src/lib.rs`, `Adeline::composer`, `Adeline::attachments`). Neither survives a restart.
- The Activity panel (`crates/adeline_ui/src/activity.rs`, `ActivityPanel`) lists what the open chat's agent did.
- Adeline has no git code today.
- Diff and code views come from GPUI Kit 0.9.1, which `docs/scope-ely-components.md` brings in first. Its `diff` module (`Diff`, `DiffState`, `DiffFile`) shows a read-only patch, unified or split (`DiffMode`), with gutter markers, line decorations, line selection and conflicts. Its `Editor` has a read-only mode (`Editor::readonly`).

## Requirements

### The mode

- R1. Changes is a mode on the mode rail, after Docs. It works on the open project. Switching project tabs switches the tree, the open file and the right-island chat to that project's.
- R2. Changes is on by default and can be turned off in Settings › Features. When it is off, the mode is hidden from the rail and the Activity summary (R27) is hidden.
- R3. Changes works the same for projects on remote machines as for local ones. The project's engine runs git and reads the files on its own machine.
- R4. Changes works in `--demo` with a sample tree, sample changes and a sample conversation.

### What counts as a change

- R5. The changes are the working tree compared with `HEAD` on whatever branch is checked out, including `main`. Changes mode needs no worktree or separate branch.
- R6. The changes include staged, unstaged and untracked files.
- R7. Each changed file has a status: modified, added, deleted, renamed or untracked, and counts of added and removed lines.

### Layout

- R8. The left island shows the files tree. The main pane shows the selected file. The right island shows a conversation, as in Docs mode. The left and right islands can be shown, hidden and resized, as in Docs.

### Left island: the files tree

- R9. The tree shows every file in the project, in folders, since any file can take a comment.
- R10. Files ignored by `.gitignore`, and files in hidden folders, are left out while the "Respect .gitignore" setting is on, as in Docs.
- R11. A changed file's name has the color of its status: added (untracked included), modified, deleted or renamed. A changed file also shows its +A −D counts.
- R12. Deleted files appear in the tree at their old path. A renamed file appears once, at its new path, with its old path shown.
- R13. A search box filters the tree, as in Docs.
- R14. A "Changed only" toggle limits the tree to changed files. It is off by default.
- R15. Double-clicking a file name, or right-clicking it and choosing "Comment on file", starts a comment on the whole file.

### Main pane

- R16. A changed file shows its diff in GPUI Kit's `Diff`, unified by default, with a switch to side by side. The choice is remembered.
- R17. An unchanged file shows its content in GPUI Kit's `Editor` in read-only mode, syntax highlighted.
- R18. A deleted file shows its old content as removed lines.
- R19. A binary file shows "Binary file changed" with no diff.
- R20. A diff too large to show says "Diff too large" with the file's counts.

### Comments

- R21. Selecting one or more lines in the main pane, whether added, removed or unchanged, offers a comment on them. This works on changed and unchanged files alike. A small comment box opens under the selection.
- R22. Saving a comment adds a card to the composer of the conversation in the right island. The card shows the file name, the line range (none for a whole-file comment), and the first words of the comment. The commented range keeps a marker in the gutter.
- R23. Clicking a card or a gutter marker opens the comment again for editing. The card's × deletes the comment and its marker.
- R24. Comment cards stay in the shared composer, like file attachments today. If the user switches chats, the cards go to whichever chat the message is sent to.
- R25. On send, the message is the comments first, in the order they were added, each separated by an empty line, then the composer's typed text. Each comment gives the file path and line range, the lines it covers in a fenced block, and the comment text. A whole-file comment gives the path and the comment.
- R26. Nothing reaches the agent until the user sends. After sending, the cards and gutter markers clear. Sending while the agent is in a turn queues or steers the message the same way a typed message does now, with the comments included.

### Chats mode: the Activity summary

- R27. While Changes is on, the Activity panel in Chats shows a "Changes" subheading with the Changes icon and the project's totals, "+A −D", over all uncommitted changes in the project.
- R28. Clicking the summary opens Changes mode on the first changed file in tree order, with the same chat in the right island.
- R29. The summary hides when the project has no changes, when it is not a git repository, or when git is missing on its machine.

### The right island's conversation

- R30. The right island follows Docs mode's rules. Entering from the Activity summary shows the chat the user came from. Otherwise it shows the chat last used in Changes for this project, or the Chats selection when there is none. The chat chooser switches chats.

### Live updates

- R31. The tree, the diff, the counts and the Activity summary update by themselves when files change on disk or in git, for example when an agent edits a file or the user commits from a terminal.
- R32. A comment keeps the lines it quoted as they were when written. If its lines change afterwards, its card and marker say so, and the message still sends the quoted lines.

### Keyboard

- R33. Every Changes action is reachable by keyboard. Shortcuts cover: next and previous changed file, next and previous hunk, comment on the selection, comment on the selected file, switching between unified and side by side, toggling "Changed only", focusing the tree search, and opening the chat chooser. Switching to Changes uses the existing switch-to-mode-by-position shortcut. All are listed in `docs/shortcuts.md`.

## Boundaries

- No staging, unstaging, reverting, committing or pushing. The mode only reads changes and sends comments.
- No comparison against another branch or commit. The base is always `HEAD`.
- No per-chat tracking of which chat made which change.
- No drafts per chat. Cards live in the one shared composer.
- Unsent comments don't survive a restart of the app.

### Rejected ideas

- Comparing only what the open chat's agent changed. With one shared branch, Adeline can't tell which chat made an edit.
- Sending each comment as its own draft right away, as Docs notes do. Comments collect as cards and go out together.
- Comments after the typed text. The comments come first.
- Building the diff view in Adeline. It comes from GPUI Kit's `Diff`.
- Ely's `DiffViewer` and `CodeEditor`. GPUI Kit 0.9.1 has a diff view and a better editor, and Adeline already uses Kit.

## Domain and data

- **Change:** a file whose working tree content differs from `HEAD`, or that is untracked. It has a status and line counts.
- **Comment:** text written on a line range of a file, or on a whole file. It holds the path, the range, a copy of the lines it covers as they were when written, and its text. It lives in the composer until sent or deleted.
- **Comment card:** the composer's chip for a comment.
- **Summary:** the project's total added and removed lines over all changes.

## Interfaces and dependencies

- The UI never runs git or reads files itself. The engine lists the changes, reads a file's old and new content, and watches for changes, for a client, through new protocol commands and deltas. This changes the protocol's shape, so `PROTOCOL` is bumped.
- The engine uses the `git` command line on the project's machine.
- GPUI Kit's `Diff` and `Editor` draw the main pane. They need Kit 0.9.1, so the Kit upgrade in `docs/scope-ely-components.md` must ship first.
- `docs/architecture.md` gets a Changes mode section.

## Constraints and quality requirements

- R34. Every element a test finds has an accessibility label, as the UI tests require.
- R35. The diff and the editor follow Adeline's theme, light and dark, like the rest of the Kit views.

## Failure and edge cases

- R36. In a project that isn't a git repository, the main pane says so. When git is missing on the project's machine, it says that instead. The mode stays on the rail.
- R37. A repository with no commits shows every file as added.
- R38. A file with merge conflicts shows as modified, with the conflict markers in its diff.
- R39. A submodule shows as one line with its commit change and no diff.
- R40. If the remote machine disconnects, the mode shows the machine's offline state as other modes do. Comment cards already in the composer stay.
- R41. If a file can't be read (permissions, or deleted after listing), the main pane says so and the tree keeps working.

## Acceptance criteria

- AC1 (R1, R8). Changes is on the rail after Docs. Opening it shows the tree on the left, the file in the middle and a conversation on the right. Switching project tabs switches all three.
- AC2 (R2). Turning Changes off in Settings › Features hides it from the rail and hides the Activity summary. It is on in a fresh settings file.
- AC3 (R3). On a remote machine's project, the tree, diffs and summary match that machine's working tree.
- AC4 (R4). `--demo` shows sample changes, a tree and a conversation.
- AC5 (R5, R6, R7). On `main` with one staged edit, one unstaged edit and one new untracked file, all three show with the right status and counts.
- AC6 (R9, R10). The tree lists unchanged files too, and leaves out ignored files while "Respect .gitignore" is on.
- AC7 (R11, R12). Added, modified, deleted and renamed files show in their status colors with +A −D. A deleted file sits at its old path. A renamed file shows once, with its old path.
- AC8 (R13, R14). The search box filters the tree. "Changed only" leaves only changed files, and is off at first.
- AC9 (R15). Double-clicking a file name, and its right-click "Comment on file", both open a whole-file comment.
- AC10 (R16). A modified file shows a unified diff. The switch shows it side by side, and the choice is kept after reopening.
- AC11 (R17, R18, R19, R20). An unchanged file shows highlighted and read-only. A deleted file shows as removed lines. A binary file shows "Binary file changed". A file over the size limit shows "Diff too large" with counts.
- AC12 (R21, R22). Selecting lines 12–18 of a changed file, or lines of an unchanged file, opens a comment box. Saving adds a card with the file, `12–18` and the first words, and a gutter marker.
- AC13 (R23). Clicking the card or the marker reopens the comment for editing. The × removes the card and the marker.
- AC14 (R24). After adding cards, switching chats in the chooser keeps the cards in the composer, and they go to the chat the message is sent to.
- AC15 (R25). Sending two comments and the text "Fix these two" gives one message: comment 1, an empty line, comment 2, an empty line, "Fix these two". Each comment has the path, range, fenced lines and text. A whole-file comment has the path and text.
- AC16 (R26). Nothing is sent before the user sends. After sending, the cards and markers are gone. Sending during a turn queues or steers like a typed message, with the comments in it.
- AC17 (R27). In Chats, the Activity panel shows "Changes" with its icon and the project totals.
- AC18 (R28, R30). Clicking the summary opens Changes on the first changed file, with the same chat on the right. Entering Changes another way shows the last chat used there, or the Chats selection.
- AC19 (R29). The summary hides with no changes, in a non-git project, and when git is missing.
- AC20 (R31). An agent's edit to a file updates the tree, the diff, the counts and the summary without user action. A commit made in a terminal clears the committed changes.
- AC21 (R32). After an edit moves or rewrites commented lines, the card and marker say the lines changed, and the sent message still quotes the original lines.
- AC22 (R33). Each listed action works by its shortcut alone, and `docs/shortcuts.md` lists them.
- AC23 (R34, R35). UI tests find the tree, the diff, the comment box and the cards by label. The diff and the editor change colors with the theme.
- AC24 (R36). A non-git project and a machine without git each show their message in the main pane.
- AC25 (R37, R38, R39). A repository with no commits shows all files as added. A conflicted file shows its markers. A submodule shows one commit line.
- AC26 (R40, R41). Disconnecting a remote machine shows its offline state and keeps the cards. An unreadable file shows a message while the tree keeps working.

## Decisions and rationale

- The base is `HEAD` on the current branch, so the mode works on `main` with no worktrees.
- The `git` command line, run by the engine, finds changes and old content. It follows the user's git config, adds no C library, and its absence is easy to report (R36).
- GPUI Kit's `Diff` for changed files and its `Editor` read-only for unchanged ones, so Adeline doesn't build its own diff view and adds no UI library beside Kit.
- Comments come first and the typed text last, so the agent reads the context before the request.
- Comments carry the lines they cover, so the agent sees what the user saw even after the file changes.
- The Activity summary counts the whole project, since edits can't be traced to a chat.
- The shortcut set in R33 was left to the scope writer.

## Open questions

None.
