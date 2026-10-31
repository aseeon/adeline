# Keyboard shortcuts

What Adeline binds today, read from the code. Where Windows/Linux and macOS differ, both are given. Every binding in the app keymap is registered with both its Ctrl and its Cmd form on every OS, so Ctrl+N also works on macOS (and Cmd means the Windows key on Windows).

## App keymap

Set in `general.keymap` in `settings.yml` (defaults in `Keymap::default`, `crates/adeline_core/src/config.rs`). Changes need a restart. Settings > General > Keymap lists the current values.

| Shortcut | Action | Setting |
|---|---|---|
| Ctrl+, / Cmd+, | Open Settings | `open_settings` |
| Ctrl+N / Cmd+N | New chat | `new_chat` |
| Ctrl+F / Cmd+F | Search chats (Chats mode with a project open), search docs (Docs mode, outside the Editor), otherwise open the projects menu | `focus_search` |
| Ctrl+Enter / Cmd+Enter | Send message | `send_message` |
| Escape | Close the open dialog, popup or menu; in Settings and the agent form, leave (asks about unsaved changes) | `close_dialog_or_popup` |
| Tab | Move focus to the next control | `next_control` |
| Shift+Tab | Move focus to the previous control | `previous_control` |
| Cmd+Q | Quit (asks first if agents are running). Hard-coded, not in the keymap | — |

## Message box

| Shortcut | Action |
|---|---|
| Enter | Send, when Settings > Chats > "Submit on Enter" is on (default). Otherwise inserts a line break |
| Shift+Enter | Insert a line break |
| Ctrl+Enter / Cmd+Enter | Send, when "Submit on Enter" is on |
| Up | With the box empty and messages queued, take the last queued message back to edit it |
| `/` | At the start of the box, list the agent's commands; Up / Down move, Enter or Tab insert, Escape closes the list |
| Ctrl+V / Cmd+V | Paste text, or attach a copied image |

With "Submit on Enter" off, the text box inserts a line break for Enter, Shift+Enter and Ctrl+Enter alike and stops the key there, so the keyboard has no way to send. The send button's icon shows Shift+Enter in that state, which does not send either. (From reading the code in `crates/adeline_ui/src/chat/composer.rs` and gpui-base's `InputState::enter`, not tested.)

## Conversation

Registered with both the Ctrl and the Cmd form on every OS (`bind_keys`, `crates/adeline_ui/src/lib.rs`), not in the keymap setting. The menus are the exception: Alt on Windows and Linux, Cmd+Shift on macOS, because Ctrl+Shift+M, E and O are global hotkeys of tools such as AMD Adrenalin, which take them before Adeline sees them.

| Shortcut | Action |
|---|---|
| Ctrl+Shift+Enter / Cmd+Shift+Enter | Send now: steer the running turn, or stop it and send |
| Ctrl+. / Cmd+. | Stop the running turn |
| Ctrl+Shift+R / Cmd+Shift+R | Restart the agent |
| Alt+M / Cmd+Shift+M | Open the Model menu |
| Alt+E / Cmd+Shift+E | Open the Effort menu |
| Alt+O / Cmd+Shift+O | Open the Mode menu |
| Alt+P / Cmd+Shift+P | Open More options |
| Ctrl+Shift+A / Cmd+Shift+A | Attach files |
| Ctrl+Shift+L / Cmd+Shift+L | Show the ACP traffic tab of the side panel (with `--verbose`) |
| Ctrl+Shift+T / Cmd+Shift+T | Show or hide the Plan tab of the side panel |
| Ctrl+L / Cmd+L | Focus the message box |
| Ctrl+Shift+D / Cmd+Shift+D | Dismiss the errors under the chat |
| Ctrl+Shift+F / Cmd+Shift+F | Fork the chat at its latest reply |
| Ctrl+Shift+G / Cmd+Shift+G | In a fork, open the chat it was forked from |
| Ctrl+Shift+C / Cmd+Shift+C | Copy the last code block of the latest reply |

## Projects and machines

Registered with both the Ctrl and the Cmd form on every OS, not in the keymap setting. Projects switch with Ctrl alone, since Cmd+Tab is the macOS app switcher. The machines menu follows the option menus: Alt on Windows and Linux, Cmd+Shift on macOS.

| Shortcut | Action |
|---|---|
| Ctrl+P / Cmd+P | Open the projects menu (in Docs mode, search docs) |
| Ctrl+Tab / Ctrl+Shift+Tab | Switch to the next / previous open project, wrapping at the ends |
| Ctrl+W / Cmd+W | Close the project |
| Ctrl+O / Cmd+O | Open a folder as a project (asks for the machine when several are checked) |
| Alt+H / Cmd+Shift+H | Open the machines menu (with a remote machine saved). In Settings, focus the Machine dropdown, which Enter opens |

The machines menu, the machine choice for a folder and the folder browser list one control per machine or folder, so they get no shortcut of their own: Tab moves to a machine's box, a button or a folder, and Space or Enter presses it.

## Modes

Registered with both the Ctrl and the Cmd form on every OS, not in the keymap setting.

| Shortcut | Action |
|---|---|
| Ctrl+1 … Ctrl+9 / Cmd+1 … Cmd+9 | Switch to the mode at that position on the mode rail (enabled modes only) |
| Ctrl+B / Cmd+B | Show or hide the chat list (in Docs mode, the tree). In the Editor it stays Bold |
| Ctrl+Shift+B / Cmd+Shift+B | Show or hide the side panel |

## Docs

Registered with both the Ctrl and the Cmd form on every OS (`docs::key_bindings`, `crates/adeline_ui/src/docs.rs`), not in the keymap setting. They work while Docs mode shows.

| Shortcut | Action |
|---|---|
| Ctrl+P / Cmd+P | Search docs (focus the tree's search field) |
| Ctrl+Shift+V / Cmd+Shift+V | Switch the open doc between the Viewer and the Editor |
| Ctrl+S / Cmd+S | Save the open doc (recreates a deleted one) |
| Ctrl+Alt+N / Cmd+Alt+N | New doc |
| Ctrl+Shift+K / Cmd+Shift+K | Choose the conversation on the right |
| Ctrl+Shift+N / Cmd+Shift+N | Note on the selection, in the Viewer or the Editor: quote it with its path and lines in the message box |

In the tree search field, Down moves into the tree. With the tree focused, no modifier:

| Shortcut | Action |
|---|---|
| Up / Down | Move to the previous / next row |
| Home / End | Move to the first / last row |
| Right / Left | Expand / collapse a folder; Left on a doc moves to its folder |
| Enter or Space | Open the doc, or expand or collapse the folder |
| F2 | Rename the doc |
| Delete | Delete the doc (asks first) |

In the Editor (the toolbar's tools):

| Shortcut | Action |
|---|---|
| Ctrl+Alt+1, 2, 3 / Cmd+Alt+1, 2, 3 | Heading 1, 2, 3 |
| Ctrl+B / Cmd+B | Bold |
| Ctrl+I / Cmd+I | Italic |
| Ctrl+Shift+X / Cmd+Shift+X | Strikethrough |
| Ctrl+Alt+C / Cmd+Alt+C | Inline code |
| Ctrl+Alt+B / Cmd+Alt+B | Code block |
| Ctrl+K / Cmd+K | Link |
| Ctrl+Shift+8 / Cmd+Shift+8 | Bullet list |
| Ctrl+Shift+7 / Cmd+Shift+7 | Numbered list |
| Ctrl+Shift+9 / Cmd+Shift+9 | Task list |
| Ctrl+Shift+. / Cmd+Shift+. | Quote |
| Ctrl+Alt+T / Cmd+Alt+T | Table |
| Ctrl+Z / Cmd+Z, Ctrl+Y / Cmd+Shift+Z | Undo, redo (from GPUI Kit) |
| Ctrl+F / Cmd+F | Find in the doc (from GPUI Kit) |
| Ctrl+H / Cmd+H | Find and replace (from GPUI Kit) |

Doc cards come and go with the replies, so they get no shortcut of their own. Tab moves to a card's Open button or to its "+N more docs" button, and Enter presses it.

## Chat list

With the chat list focused, no modifier:

| Shortcut | Action |
|---|---|
| Up / Down | Select the previous / next chat |
| Home / End | Select the first / last chat |
| Enter or Space | Open the chat |

In the chat search (Ctrl+F), Enter opens the first chat it lists.

## Projects menu

While the projects menu is open, no modifier:

| Shortcut | Action |
|---|---|
| Up / Down | Move the highlight, wrapping at the ends |
| Enter | Open the highlighted project, or the first match of the search |
| Delete | Close the highlighted open project, or remove a closed one (with undo). With search text, Delete edits the text |
| Escape | Close the menu |

## Settings

Settings > Search settings finds every page, and Tab moves on to the matching controls. Tab also reaches the navigation's group headers, and Enter or Space folds or unfolds a group.

## Focused controls (from GPUI Kit)

| Shortcut | Action |
|---|---|
| Enter or Space | Activate the focused button or popup trigger |
| Enter / Escape | Confirm / cancel a dialog |
| Up / Down, Enter, Escape | Move, pick, close in a dropdown or menu (agent, model, effort, mode, machine pickers) |
| Up / Down | Step a number field |
| Ctrl+C / Cmd+C | Copy selected text in a transcript |
| Ctrl+A / Cmd+A | Select all text in a transcript |

## Text fields (from GPUI Kit)

All text fields (message box, search fields, dialog fields) get the kit's editing keys. Windows/Linux first, macOS second:

| Shortcut | Action |
|---|---|
| Ctrl+Z / Cmd+Z | Undo |
| Ctrl+Y / Cmd+Shift+Z | Redo |
| Ctrl+X, C, V / Cmd+X, C, V | Cut, copy, paste |
| Ctrl+A / Cmd+A | Select all |
| Left, Right, Up, Down | Move the cursor |
| Ctrl+Left/Right / Alt+Left/Right | Move by word |
| Home, End / Cmd+Left, Cmd+Right, Ctrl+A, Ctrl+E | Start / end of line |
| Ctrl+Home, Ctrl+End / Cmd+Up, Cmd+Down | Start / end of text |
| Page Up / Page Down | Move by page |
| Shift + any movement key | Extend the selection |
| Backspace / Delete | Delete a character back / forward |
| Ctrl+Backspace, Ctrl+Delete / Alt+Backspace, Alt+Delete | Delete a word back / forward |
| Cmd+Backspace, Cmd+Delete (macOS) | Delete to start / end of line |
| Ctrl+Alt+Up/Down, Shift+Alt+Up/Down / Cmd+Alt+Up/Down | Add a cursor above / below |
| Ctrl+Cmd+Space (macOS) | Character palette |
| Escape | Clear the selection or close a completion menu first; otherwise passes on to close the dialog |

## macOS menu bar

The menus (`crates/adeline_ui/src/menu_bar.rs`) show the keymap's shortcuts: Settings… (Cmd+,), New Chat (Cmd+N), Quit Adeline (Cmd+Q), and the system Edit items (Undo, Redo, Cut, Copy, Paste, Select All).

## Actions without a shortcut

Reachable only by mouse, or by Tab to the control and Enter/Space.

Menu bar items with no key:

- About Adeline
- Stop All Agents
- Hide Adeline (the usual Cmd+H), Hide Others (Cmd+Option+H), Show All
- Minimize (Cmd+M), Zoom

Projects:

- Add a project, pick the project's machine
- Rename a project
- Delete a project
- Toggle the projects menu sort order
- Change a project's color tint
- Undo removing a closed project

Chats:

- Filter chats by agent, clear chat filters
- Show completed chats, show archived chats
- Mark a chat complete, archive a chat (the chat row's right-click menu)
- Reply to (quote) a message
- Copy a message
- Fork a chat at an earlier reply
- Expand or collapse a tool call; hide all tool calls
- Force stop
- Answer a permission request or a decision the agent asks for
- Edit, remove or send now a queued message other than the last
- Log in to an agent from a chat
- Expand a thought
- Remove an attachment, preview an attached image
- Copy the ACP traffic
- Select text in a message

Composer pickers:

- Pick the agent
- Open the agent menu, open the current agent's settings
- Add an agent

Panels:

- Docs: toggle the conversation panel; resize the panels

Machines and engine:

- Retry a failed machine connection
- Manage machines (Settings on the machines page)
- Upgrade a remote machine's engine
- Answer an SSH prompt
- Start, retry or wait for the engine; stop an older engine
- Retry storage, replace a session

Settings:

- Save settings
- Open mode settings
- Toggle "Submit on Enter"
- Add, edit or remove an MCP server
- Install, update or log in to an agent (the agent form)
- Toggle a mode on or off

Quit dialog:

- Stop all agents and quit, or quit and let turns finish
