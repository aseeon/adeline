# Docs and Changes modes

Docs and Changes are the first modes beyond chat. Both go through the project's engine for every file they read or write, so remote projects work like local ones. `--demo` answers the same requests from `assets/demo-docs.json` and `assets/demo-changes.json`.

## Cards

Cards are a chat reply's links to what a mode shows, such as written docs (`card.rs`).

- The engine makes cards from agent output it has checked. Cards carry no commands: a client can only open the card's route or copy its path.
- A route is `adeline://<mode>/<path>`, relative to the chat's project.
- A chat holds one card per route, so a later change updates the card in place.

## Which files are docs

- A doc's path is relative to the project root, with `/` separators.
- With `respect_gitignore`, files that the project's `.gitignore` files or `.git/info/exclude` ignore are not docs, and neither are files in hidden folders. `.git` itself never holds docs.
- A file that is too large, binary, UTF-16 or not UTF-8 opens read-only.
- A save keeps the file's line endings and its trailing newline, or lack of one.

## The Editor and the Viewer

- Each tool on the Markdown toolbar turns the text and its selection into one `Edit`. Inline styles wrap or unwrap the selection. Line tools set or clear a prefix on every line the selection touches. Block tools put a code fence or a table on lines of their own.
- Offsets are UTF-8 byte offsets. A selection past the end or off a char boundary is clamped, so no input panics. `\r\n` counts as one line end.
- Edits stay in memory until saved. A doc's buffer lives in the shared Editor state while the doc is open, and in its `Doc` otherwise.
- Alert and task bodies in the Viewer are views of their own, with the same extensions.
- The Viewer's renderers run outside the app's entities, so clicks reach `Adeline` as actions that carry the byte offsets of the source they act on.
- Mermaid diagrams render to a PNG at 2x, for high-DPI screens.

## Changes

- The engine runs `git` on a blocking thread, so a slow repository never holds up the engine's task.
- While Changes is on, the open project's changes are watched in every mode, so the Activity panel can sum them.

## Switching modes

Entering Docs or Changes remembers the Chats selection, and leaving restores it. What Docs shows for each project is kept in `ui_state.rs`.
