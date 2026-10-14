Status: Implemented in v0.2.0. This scope is history. Where it and the code differ, the code is right.

# Docs mode

## Purpose and context

Agents write a lot of documentation (scopes, plans, notes, config). Today the user reads and edits those files in another editor, away from the conversation that produced them. Docs mode makes Adeline the main viewer and editor for a project's doc files, with the project's conversation open beside the file, so the user can read, edit and talk to the agent about a doc in one place.

Example flow: an agent writes `docs/scope-x.md` during a chat. A card for the file appears in the chat. The user clicks it, Docs opens on that file with the chat in the right panel, the user reads it rendered (mermaid included), highlights a paragraph and sends "this contradicts R4" to the chat. The agent edits the file and the Viewer updates by itself.

Current behavior:

- The mode rail already lists Docs, behind a feature flag that is off by default (`crates/adeline_core/src/config.rs` `Section::Docs`, `Features::docs`; `crates/adeline_ui/src/project_bar.rs`, the modes loop). Selecting it shows an empty view (`crates/adeline_ui/src/lib.rs`, the `self.section == Section::Chats` branch).
- `config::Modes` reserves a `docs` key, kept only so old settings files load (`config.rs` `Modes`).
- Chats mode has quick settings (the `mode-settings` popover, `crates/adeline_ui/src/views.rs` `command_entries`, built from `settings.rs` `mode_options`) and a full Settings › Modes › Chats page (`settings/general.rs`).
- Chat replies render Markdown through gpui-kit's `TextView` (`chat/transcript.rs`). There is no mermaid rendering.
- The engine can list a directory for a client (`protocol::Command::ListDirectory`) but cannot read, write, watch or rename project files for a client.
- Chat search lives in `crates/adeline_core/src/prepared.rs`. No fuzzy-matching library is a dependency.
- `docs/shortcuts.md` lists "Switch mode" as a missing shortcut.
- `docs/architecture.md`, "Modes (planned)", describes the intended shape: engine-made cards, `adeline://` routes, a mode view with the chat in a sidebar.

## Requirements

### The mode

- R1. Docs mode works on the open project. With several project tabs, it shows the active tab's project, and switching tabs switches the tree, the open doc and the right-panel chat to that project's.
- R2. Docs mode is enabled by default. It can still be turned off in Settings › Features.
- R3. Docs mode works the same for projects on remote machines as for local ones. All file reads, writes, listings, renames, deletes and change notifications go through that project's engine.
- R4. Docs mode works in `--demo` with sample docs, sample cards and a sample conversation.

### Which files are docs

- R5. A doc is a file with the extension `.md`, `.markdown`, `.yaml`, `.yml`, `.toml` or `.txt` inside the project folder.
- R6. Files ignored by the project's `.gitignore` rules, and files inside hidden folders, are not docs while the "Respect .gitignore" setting is on (the default). With it off, ignore rules don't apply.

### Layout

- R7. Docs mode has a left panel (the docs tree), a main panel (the open doc) and a right panel (a conversation). The left and right panels can each be shown or hidden and resized.

### Left panel: the docs tree

- R8. The tree shows only docs and the folders that contain docs, folders first, each level sorted alphabetically. It is styled like the Chats mode conversation list.
- R9. A search box above the tree filters it by file name and path, fuzzy, as the user types. Matching files' folders expand to show them.
- R10. The tree updates by itself when docs are created, renamed or deleted on disk, by the agent or anything else.
- R11. The tree marks docs that have unsaved edits.
- R12. From the tree, the user can create a new doc (choosing its folder and name; it opens in the Editor), rename a doc, and delete a doc. Delete always asks for confirmation, and the confirmation says the delete is permanent.

### Main panel: Viewer and Editor

- R13. The main panel shows the open doc in either the Viewer or the Editor, and the user can switch between them. The view a doc opens in follows the "Open files in" setting (default Viewer).
- R14. The Editor shows the raw text of any doc, with a toolbar island at the top of the panel.
- R15. For Markdown, the toolbar has: undo, redo, heading 1–3, bold, italic, strikethrough, inline code, code block, link, bullet list, numbered list, task list, quote, table, find and replace, save, and the Viewer/Editor switch.
- R16. For `.yaml`, `.yml`, `.toml` and `.txt`, the toolbar has: undo, redo, find and replace, save, and the Viewer/Editor switch.
- R17. The Viewer renders Markdown with GitHub-style extended Markdown: headings, lists, tables, task lists, strikethrough, autolinks, and fenced code with syntax highlighting.
- R18. The Viewer also supports:
  - clicking a task-list checkbox, which updates the file;
  - images by relative path, loaded through the engine;
  - relative links to other docs, which open them in Docs, and heading anchors, which scroll to the heading;
  - GitHub alerts (`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`, `> [!CAUTION]`);
  - footnotes;
  - a front matter block (`---` YAML at the top), shown as a small table instead of raw text.
- R19. The Viewer renders mermaid diagrams. The supported diagram types are those of the best mermaid renderer available to Adeline, which must include at least flowchart, sequence, class, state, entity relationship, gantt and pie. A diagram it can't render (unsupported type or syntax error) shows its source with a short "Can't render: <reason>" note.
- R20. For `.yaml`, `.yml`, `.toml` and `.txt`, the Viewer shows the source, syntax highlighted, read-only.
- R21. Links in the Viewer: links to docs open in Docs; `http(s)` links open in the system browser; links to project files that are not docs open in the operating system's default app for local projects, and copy the file's path for remote projects.

### Saving and outside changes

- R22. Edits are saved explicitly (save shortcut or toolbar). A doc with unsaved edits shows an unsaved marker.
- R23. When a doc without unsaved edits changes on disk, the open view reloads by itself.
- R24. When a doc with unsaved edits changes on disk, Docs asks the user to keep their version or take the version on disk. Neither version is lost without the user choosing.
- R25. Switching to another doc or another mode keeps unsaved edits in memory. Closing the project or quitting Adeline with unsaved docs asks: Save all, Discard, or Cancel.

### Right panel: the conversation

- R26. The right panel shows a full conversation, as in Chats mode: transcript and composer.
- R27. The chat header's title is a conversation chooser. It lists the project's conversations with their status, searches them the same way the Chats list does (titles and content), and hides completed and archived chats according to the Chats mode toggles. It offers "New chat". It opens with a shortcut and is used with arrow keys, Enter and Escape.
- R28. Entering Docs shows: the card's chat when entered from a card; otherwise the chat last shown in Docs for this project; otherwise the chat selected in Chats mode.

### Notes on a selection

- R29. Selecting text in the Viewer (any doc type) shows a popover to send a note about it to the conversation in the right panel. In the Editor, the same action is available on the selected text through a shortcut.
- R30. A mermaid diagram offers a "Note on this diagram" action, which quotes the diagram's source.
- R31. Sending a note puts a draft in the right panel's composer, quoting the selected text with the doc's path and line range. The user edits and sends it; nothing reaches the agent until they do.
- R32. If the project has no conversation, sending a note starts a new chat with the draft in its composer.

### Chat cards

- R33. While Docs is enabled, every doc (R5, R6) that the agent created or edited during a turn gets a card in that turn's reply.
- R34. A card shows the file name, its path (long paths shortened to fit, full path available on hover and to screen readers), and the date and time of the file's last edit. It has one primary action, Open, and a copy-path action.
- R35. A file has one card per chat. When the agent changes the file again, the existing card updates in place instead of a new card being added.
- R36. A reply shows at most 4 cards: the 4 most recently changed docs. If more docs changed, a "+N more docs" chip expands to a plain list of the rest, and each item opens like a card.
- R37. Opening a card switches to Docs mode on that file, with the card's chat in the right panel.
- R38. A card exists only for a file the engine has confirmed is a doc inside the project. Cards never carry commands; their actions are only Open and copy path.
- R39. Cards appear for turns after Docs mode ships. Existing chats get no cards for earlier turns.
- R40. With Docs disabled, no new cards are made, and existing cards show as plain links.

### Settings

- R41. Docs has quick settings in a popover, like Chats mode, and a Settings › Modes › Docs page with the same options. The options are:
  - Respect .gitignore (on);
  - Show left panel (on);
  - Show conversation panel (on);
  - Open files in: Viewer or Editor (Viewer);
  - Load remote images (off).

### Keyboard

- R42. Every Docs action is reachable by keyboard, with shortcuts for: switching to each enabled mode by its position on the mode rail (all modes, not only Docs); toggling Viewer/Editor; focusing the tree search; opening the conversation chooser; sending a note on the selection; new doc, rename doc, delete doc; opening the focused chat card; and every toolbar tool. All are listed in `docs/shortcuts.md`.

### Remembered state

- R43. Per project, Docs remembers across restarts, on this machine only: the open doc, its Viewer/Editor state, its scroll position, panel widths and visibility, expanded folders, and the right-panel chat.

## Constraints and quality requirements

- R44. The tree and its search stay responsive with up to 10,000 docs in a project: filtering keeps up with typing without noticeable lag.
- R45. Files up to 10 MB open normally. A larger file opens read-only with a notice.
- R46. Viewer rendering, mermaid included, works fully offline.
- R47. Remote images (`http(s)` URLs) are not fetched unless "Load remote images" is on. While it's off, each shows as a link.
- R48. Saving keeps the file's existing line endings (LF or CRLF) and its trailing newline, or lack of one.
- R49. Docs mode meets the app's accessibility basics: every control has an accessible name, the tree, chooser, cards and popovers work with assistive technology, and focus is always visible.

## Failure and edge cases

- R50. A file that is not valid UTF-8, or looks binary, opens read-only with a notice.
- R51. When the open doc is deleted or moved outside Docs, it keeps its content and shows a banner saying so, offering Save (recreates the file) or Close. A rename done in Docs follows the file without a banner.
- R52. When a remote machine drops, its open doc stays on screen read-only with its last content. Unsaved edits are kept, and saving works again after the machine reconnects.
- R53. Opening a card whose file no longer exists opens Docs with the card's chat on the right and a main panel saying "<path> no longer exists". The card shows a muted deleted style once the engine has noticed the file is gone.
- R54. Creating or renaming a doc to a name that already exists, or to an invalid name, is refused with a message; nothing is overwritten.
- R55. A failed read, save, rename or delete shows the error and leaves the doc and the user's edits as they were.

## Boundaries

### Exclusions

- Only one project at a time; Docs does not show several projects' docs together.
- Moving docs between folders (by drag or command) is not part of this scope.
- Full-text search inside docs is not part of the tree search.
- Math rendering (`$…$`, `$$…$$`) is not part of this scope.
- Non-doc files (source code and the rest) are not shown or edited in Docs.

### Interactions with existing functionality

- The right panel reuses the Chats mode conversation (transcript, composer) and the Chats list search behavior and toggles.
- Docs quick settings and its Settings page follow the Chats mode pattern; the reserved `docs` key in `config::Modes` becomes the Docs settings.
- Mode-switch shortcuts cover every mode on the rail, filling the "Switch mode" gap in `docs/shortcuts.md`.
- Chat cards follow the card and `adeline://` route design in `docs/architecture.md`, "Modes (planned)".

### Rejected ideas

- Autosave, and last-write-wins saving (Q4).
- Opening `.yaml`/`.toml`/`.txt` in the Editor only, with no Viewer (Q5).
- Sending a note straight to the agent without a composer draft (Q6).
- Cards only for created `.md` files, or created files only (Q14).
- Dropping docs beyond the card limit, or counting the limit per chat (Q15).
- Prompting on every switch away from an unsaved doc (Q19).
- Leaving Docs off by default (Q21).
- Fetching remote images by default (Q25).
- Building cards for turns before Docs shipped (Q31).
- A short excerpt of the file on the card (Q29).

## Domain and data

- **Doc:** a file in the open project that matches R5 and R6. Its identity is its path relative to the project root.
- **Docs tree:** the docs and the folders that contain them, for one project.
- **Open doc:** the doc in the main panel, in Viewer or Editor, with possibly unsaved edits held in memory by the UI.
- **Card:** a reference in a chat reply to a doc the agent created or edited in that turn. One per doc per chat, updated in place, owned by the engine and stored with the conversation.
- **Note:** a composer draft quoting a selection (or a diagram's source) with the doc's path and line range.
- **Docs settings:** the R41 options, stored with the other mode settings.
- **Docs UI state:** the R43 state, per project, stored by this UI only.

## Interfaces and dependencies

- The engine gains the ability, for a client, to list a project's docs, read and write a doc, create, rename and delete a doc, read images by relative path, and report doc changes on disk. This changes the engine protocol, so `PROTOCOL` is bumped.
- The engine detects created and edited docs from the agent's tool calls and emits cards, which travel to clients with the conversation.
- A mermaid renderer that works offline (R19, R46).
- A well-known fuzzy-matching library for the tree search (R9).
- Settings › Features, Settings › Modes, the mode rail, `docs/shortcuts.md`, and `scripts/engine-check/ui_check.py` coverage for the new UI.

## Acceptance criteria

- AC1 (R1). With two project tabs open in Docs, switching tabs shows the other project's tree, open doc and chat.
- AC2 (R2). On a fresh settings file, Docs appears on the mode rail; turning it off in Settings › Features removes it.
- AC3 (R3, R52). On a remote project, docs list, open, save, rename and delete through that machine's engine; when the machine drops, the open doc turns read-only with unsaved edits kept and saving works after reconnect.
- AC4 (R4). `--demo` shows Docs with sample docs, a sample card that opens Docs, and a conversation on the right.
- AC5 (R5, R6). In a project with `a.md`, `b.yaml`, `c.toml`, `d.txt`, `e.rs`, `target/f.md` (ignored) and `.hidden/g.md`, the tree shows only the first four; with "Respect .gitignore" off, `target/f.md` and `.hidden/g.md` also appear.
- AC6 (R7, R41). Each panel hides and shows from its quick setting, and widths change by resizing.
- AC7 (R8). Folders without docs don't appear; folders come before files; each level is alphabetical.
- AC8 (R9). Typing "scpdoc" finds `docs/scope-docs-mode.md` and expands `docs/`.
- AC9 (R10). A doc the agent creates appears in the tree without any user action; one it deletes disappears.
- AC10 (R11, R22). Editing a doc shows the unsaved marker in the tree and the editor until it is saved.
- AC11 (R12, R54). New doc creates the file in the chosen folder and opens it in the Editor; rename renames it on disk; delete asks with a permanent-delete warning before removing it; a duplicate or invalid name is refused and nothing is overwritten.
- AC12 (R13). The Viewer/Editor switch toggles the view; with "Open files in: Editor", docs open in the Editor.
- AC13 (R14, R15). On a Markdown doc, each listed toolbar tool performs its action on the text.
- AC14 (R16). On a `.toml` doc, the toolbar shows only undo, redo, find and replace, save and the view switch.
- AC15 (R17, R18). A sample document with every listed Markdown feature renders each correctly; ticking a task checkbox changes `[ ]` to `[x]` in the file; a relative doc link opens that doc; an anchor link scrolls; front matter shows as a table.
- AC16 (R19). Flowchart, sequence, class, state, ER, gantt and pie diagrams render; a diagram with a syntax error shows its source and a "Can't render" note.
- AC17 (R20). A `.yaml` doc in the Viewer shows highlighted, read-only source.
- AC18 (R21). A doc link opens in Docs, an `https` link opens the browser, a `.rs` link opens the OS default app locally and copies the path on a remote project.
- AC19 (R23). Changing a clean open doc on disk updates the view without a prompt.
- AC20 (R24). Changing a doc with unsaved edits on disk asks keep-mine or take-theirs, and the chosen version is what remains.
- AC21 (R25). Switching docs and modes keeps unsaved edits; quitting with unsaved docs asks Save all, Discard or Cancel, and Cancel keeps Adeline open.
- AC22 (R26, R27). The right panel's chooser lists project chats with status, finds a chat by a word in its content, respects the completed and archived toggles, offers New chat, and works by keyboard alone.
- AC23 (R28). Entering from a card shows the card's chat; entering otherwise shows the chat last used in Docs for this project, or the Chats mode selection when there is none.
- AC24 (R29, R31). Selecting a sentence in the Viewer and choosing the note action puts a draft in the composer quoting the sentence with path and line range; nothing is sent until the user sends it. The Editor shortcut does the same on a selection.
- AC25 (R30). "Note on this diagram" quotes the mermaid source in the draft.
- AC26 (R32). In a project with no chats, sending a note starts a new chat with the draft.
- AC27 (R33, R34). After a turn in which the agent edits `notes/plan.md`, the reply shows a card with `plan.md`, its path and the last-edit date and time, with Open and copy path; a long path is shortened and the full path shows on hover.
- AC28 (R35). A later turn that edits the same file updates the existing card in place; the chat has one card for it.
- AC29 (R36). A turn that changes 6 docs shows 4 cards and a "+2 more docs" chip that expands to two items that open their docs.
- AC30 (R37). Opening a card switches to Docs on that file with the card's chat on the right.
- AC31 (R38). A file the agent wrote outside the project, or a non-doc file, gets no card.
- AC32 (R39). Chats from before Docs shipped show no cards for their earlier turns.
- AC33 (R40). With Docs disabled, a new turn editing a doc makes no card and existing cards show as plain links.
- AC34 (R41). The quick settings popover and Settings › Modes › Docs show the five options with their defaults, and changing one in either place changes it in both.
- AC35 (R42). Every listed shortcut works and appears in `docs/shortcuts.md`; Docs can be used without the pointer.
- AC36 (R43). After restarting Adeline, Docs reopens each project's doc, view, scroll position, panels, expanded folders and chat as they were.
- AC37 (R44). In a project with 10,000 docs, the tree loads and search filters while typing without noticeable lag.
- AC38 (R45). A 9 MB doc opens editable; an 11 MB doc opens read-only with a notice.
- AC39 (R46, R47). With the network off, a doc with mermaid renders fully; with "Load remote images" off, an `https` image shows as a link and no request is made; turning it on loads the image.
- AC40 (R48). Saving a CRLF file without a trailing newline after a one-line edit changes only that line in `git diff`.
- AC41 (R49). The tree, chooser, cards, toolbar and popovers expose accessible names and are reachable and usable with a screen reader.
- AC42 (R50). A Latin-1 file opens read-only with a notice.
- AC43 (R51). Deleting the open doc from outside shows the banner; Save recreates the file; renaming it inside Docs follows it without a banner.
- AC44 (R53). Opening a card for a deleted file shows "<path> no longer exists" with the chat on the right, and the card turns muted.
- AC45 (R55). A save that fails (for example, a read-only file) shows the error and keeps the unsaved edits.

## Decisions and rationale

- Remote projects work like local ones, because the rest of Adeline does and files live with the engine (Q3).
- Explicit save with conflict prompts, so neither the user's nor the agent's work is lost silently (Q4).
- Notes go to the composer as drafts, reusing the Reply pattern, so nothing reaches the agent unseen (Q6).
- Chat cards are part of this scope (Q7), limited to 4 per reply with an overflow list (Q15).
- Mermaid diagram types follow the best available renderer, with a guaranteed minimum (Q17).
- Mode-switch shortcuts cover every mode by rail position, not per-mode keys (Q20).
- Remote images are off by default because agent-written docs are untrusted and a fetched URL can leak data (Q25).
- Tree search uses a well-known fuzzy-matching library rather than custom matching code (Q27).
- No cards for turns before Docs ships, because old turns refer to files that have since changed (Q31).

## Open questions

None.
