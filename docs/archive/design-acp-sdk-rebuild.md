Status: Implemented in v0.1.10. This scope is history. Where it and the code differ, the code is right.

# Design: ACP rebuild on the official SDK

Scope: [scope-acp-sdk-rebuild.md](scope-acp-sdk-rebuild.md). This document covers what people see and do. It does not cover architecture or task slicing.

Design rules: the GPUI Kit Design Guides (`.claude/skills/gpui-kit-design-guides/references/design-guides.md`). Where this document is silent, the existing Adeline pattern named here applies.

## Experience

### Actors

- **Chat user:** runs agents in conversations, switches model, effort and mode, answers permissions, queues messages.
- **Agent setup user:** the same person in Settings > Agents and the "Add an agent" window, installing, logging in and configuring agents, locally or on a remote machine.

### Surfaces affected

| Surface | Today (evidence) | Change | R IDs |
|---|---|---|---|
| Composer menu row | `Adeline::composer_view` (`src/chat_render.rs`): `+` files popover, Agent, Model, Effort, permission dropdown `chat-permission-mode` at the trailing end. `Fit::for_width` shortens labels when narrow | Permission dropdown removed. Mode and More options menus added. Menus work while the agent is stopped | R17, R18, R20, R5 |
| Composer send button | `send-chat-message`: Send, or Stop while processing. A send during a turn is rejected (`ALREADY_PROCESSING`, `src/engine.rs`) | Queue and Send now during a turn. Stop remains reachable | R30, R48 |
| Queued messages | None | Pending entries under the running turn, each with Edit, Remove, Send now | R31 |
| Attachments | `+` opens "Attach context" with Add a file… / Add a directory… (`src/views.rs`). Message images are demo-only (`Message.images`) | Paste, drop, pick. Attachment chips in the composer. Blocked-image reason. 20 MB error | R23 |
| Slash commands | None | `/` at the start opens a filterable list above the composer | R25 |
| Transcript: thoughts | Thinking indicator (`thinking_label`, `runtime_footer` in `src/runtime_ui.rs`) | With "Show thinking" on, thought text streams | R24 |
| Chat header: TODO | Header holds title, `context_meter`, complete, archive (`Adeline::chats`, `src/views.rs`) | TODO button showing the current step, expanding to the full list | R26 |
| Transcript: activity | Thinking label carries retry progress text | Real activity state: streaming, tool running, waiting for permission, background tasks, quiet | R32 |
| Transcript: hang notice | None | "Agent silent for N min" with Stop and Restart | R34 |
| Permission card | `permission-request` card in `runtime_footer`: Adeline-composed labels ("Allow once: X", "(harness remembers this choice)") | Agent's own option labels, reject always hidden | R21 |
| Transcript errors | `live.error` text plus Retry / replacement consent / Force Stop buttons | Adds auth-required (Log in, Retry), crash with stderr tail, v1 configuration error | R2, R47, R50 |
| Chat title | Header, list rows (`chat_title`). No rename | Title can change from the agent. No new control | R27 |
| ACP traffic view | None. Closest pattern: `activity_detail` mono rows in the Agent activity panel (`src/activity.rs`) | Per-conversation raw traffic and stderr, copy, shortcut | R36 |
| Agent form: install, update | `harness_status` (`src/agent_form.rs`): red dot, "Not installed", website link. Save refuses | Install offer with commands, confirm, live output, recheck, Retry. Node.js offer. Update offer | R9–R13, R15, R49 |
| Agent form: login | Probe failure lists login methods as text | Log in (agent-handled or OS terminal), Log out | R14 |
| Agent form: fields | Model, Effort Selects. "Default permission mode" RadioGroup. Instructions textarea with Append/Overwrite | Permission mode removed. Default Mode Select added. Per-agent MCP servers list. Instructions hidden without a mechanism | R19, R22, R28, R29 |
| Settings > Chats | `mode_options(Section::Chats)` switches in a card | "Show thinking" next to "Hide tool calls". Hang notice minutes (0 = off) | R24, R34 |
| Settings: MCP servers | None. List pattern: `machines_settings` rows with Edit / Remove | Global MCP server list | R28 |
| Demo | `assets/workspace.json` threads, messages, decisions, activity | Canned examples of every new feature | R40 |
| Shortcuts | `docs/shortcuts.md` | Every new control gets one | R44 |

### No UI impact

R1, R3, R4 (except what reaches the traffic view), R6, R7 (reflected through the surfaces above), R33, R35, R37 (old conversations are simply absent), R38, R39 (existing surfaces kept), R41, R42, R43 (a property every flow above must keep: no blocking spinner over the window), R45, R46.

## Design decisions

Accepted by the user in round 1 (DQ1–DQ7, all recommended options).

- **DD1 Composer menu row** (R17, R18, R20). Order: `+`, divider, Agent, Model, Effort, More options (`⋯` icon button), then Mode at the trailing end, in the slot `chat-permission-mode` holds today. Mode replaces the permission picker because it is the permission control now. Each Mode menu item shows the mode name, with its description (the agent's, or the profile's supplement) as secondary text. Plan modes listed in the profile are left out. If the agent switches itself into one, the trigger shows its name as the current value and the menu has no checked item. Narrow windows shorten labels through `Fit::for_width` as today. Rejected: one combined "Session" popover, because switching model would take two steps.
- **DD2 Typing during a turn** (R30, R48). Enter (or Ctrl+Enter with "Submit on Enter" off) queues the message. Send now is Ctrl+Shift+Enter / Cmd+Shift+Enter, and also an item behind a caret on the send button. During a turn Stop is its own icon button beside send, with a shortcut. Today Stop only replaces send. Rejected: Enter means Send now, because on agents without steering that cancels the turn.
- **DD3 Editing a queued message** (R31). Edit moves the text back into the composer and removes the entry. Enter queues it again. Rejected: inline editing inside the entry, which would need a second editor.
- **DD4 Thoughts** (R24). With "Show thinking" on, thought text streams in muted type under the agent header. When the reply starts, it collapses into a "Thought for 12s" disclosure that can be reopened. Rejected: always expanded.
- **DD5 Activity and hang notice** (R32, R34). The existing Thinking row in `runtime_footer` becomes the activity row: "Running cargo test…", "Waiting for your permission", "Background tasks running", "Quiet". With "Show thinking" off, streaming thoughts still read as the Thinking indicator. The hang notice replaces the row: "Agent silent for 10 min" with Stop and Restart. Rejected: the state in the chat header or the activity panel.
- **DD6 Install, update and login** (R9–R15, R49). Inline in the agent form, where `harness_status` and the probe box sit:
  1. "Not installed" carries an Install… button.
  2. The button shows the exact commands to confirm.
  3. After confirmation a monospace live-output area appears in place.
  4. Adeline then checks again and shows Retry on failure.

  Update and login use the same place. Rejected: a separate dialog, because the form is sometimes a window already and dialogs would stack.
- **DD7 ACP traffic view** (R36). A second tab in the right panel next to "Agent activity", per conversation and resizable. Its shortcut opens the panel on that tab. Rejected: a separate window (more machinery) and a dialog (blocks the chat).

- **DD8 TODO list in the chat header** (R26). The user chose this in round 3, and the scope's R26 and AC24 were updated to match.
  - **Label:** the agent's TODO list (ACP `plan`) is labelled "TODO" in the UI, never "Plan".
  - **Placement:** a header button between the title and the context meter in `Adeline::chats` (`src/views.rs`). It shows a status icon (spinner while a step runs), the current step's text truncated, and a muted "2 of 4" count, with a dropdown caret. The current step is the first in-progress step, or the first pending one if none is in progress. The title keeps priority: as the header narrows, the step text truncates first, then the button collapses to its icon and count.
  - **Expanded:** activating the button opens a Kit `Popover` holding the full list under the heading "TODO". Each step shows its status icon (done is a check in muted text, in progress is a spinner, pending is an empty circle) and its text, which wraps. The button stays visibly pressed while the popover is open. Escape closes it and returns focus to the button. Shortcut: Ctrl+Shift+T (Cmd on macOS).
  - **Updates:** each update replaces the list, and the button and an open popover change in place. A new prompt leaves the list as it is.
  - **Lifecycle:** with no list, or after the agent sends an empty one, the button is absent. When every step is done it shows "4 of 4 done" until the agent replaces the list. After an Adeline restart the saved list shows again, including while the agent is stopped.
  - **Rejected:** an inline transcript entry, and a list pinned above the composer.

Accepted in round 2 (DQ9–DQ14).

- **DD9 Attachments** (R23). Attachments appear as chips above the text in the composer, each with a remove button and its size in muted type.
  - **Image chips:** show a small miniature of the image instead of an icon, cropped to a square and rounded on the chip's inner radius tier. The user changed the proposal from icon-only chips.
  - **File chips:** show the file icon and name.
  - **Sent messages:** show the same chips under the user's text. Activating an image chip opens a preview dialog. Escape closes it and returns focus to the chip.
  - **Errors:** shown under the chips, in `danger` text, as complete sentences: "design.psd is 31 MB. Files over 20 MB can't be attached." and "Codex can't receive images." The rejected file is not added.
  - **Rejected:** large thumbnails in the composer.
- **DD10 Slash commands** (R25). A Kit `Command` popover anchored above the composer opens when `/` is typed at the start of the text. Each row shows the command in monospace, its input hint in muted monospace and the description below in muted text. Typing filters the list. Up/Down move the highlight. Enter or Tab inserts `/name ` and closes the list. It never sends. Escape closes the list, keeps the text and leaves focus in the composer. While the agent is stopped, the saved list is shown unchanged. With no commands the popover does not open. Rejected: sending a command that takes no input as soon as it is picked.
- **DD11 MCP servers** (R28).
  - **Global list:** "MCP servers" is the first page in the Settings Agents group. It uses the `machines_settings` row pattern: one row per server with name and type, Edit… and Remove, and an "Add server…" button.
  - **Editor:** a dialog with Name, Type (a segmented control, Command / HTTP) and the fields for that type. Command needs a command, arguments (the argument list editor from the agent form) and environment variables (name/value rows). HTTP needs a URL and headers (name/value rows).
  - **Per-agent list:** the agent form gets the same list as "Additional MCP servers".
  - **Skipped servers:** a skipped server is reported as a muted note at the start of the conversation: "Skipped docs-search: Codex doesn't support HTTP servers."
  - **Rejected:** a General page, and reporting skips only in the traffic view.
- **DD12 Login required in a conversation** (R14, R47). A card in the footer, built like the permission card, reads "Claude Code needs you to log in." It has one button per advertised login method, labelled with the method's name, and Retry. A terminal method's button ends with "in terminal…", for example "Log in in terminal…", because it opens another window. After an agent-handled login completes, the card shows "Logged in" with Retry still offered. Rejected: sending the user to the agent form.
- **DD13 Features the agent doesn't offer** (R5).
  - **Menus:** a Mode, Model, Effort or More options menu with nothing to offer is hidden, as `setting_menu` already hides an empty Model or Effort.
  - **Content features:** stay where they are and explain themselves where they are used. Attaching an image to an agent without image support shows "Codex can't receive images." (DD9).
  - **Send now without steering:** still offered. Its tooltip reads "Stops the turn, then sends".
  - **Fork, continue and similar commands:** follow the guide's rule: disabled with a reason in the tooltip when the agent doesn't advertise them.
  - **Rejected:** keeping empty menus visible but disabled.
- **DD14 Shortcuts** (R44). Ctrl means Cmd on macOS. All are added to `docs/shortcuts.md`, and the ones for buttons appear in their tooltips.

  | Control | Shortcut |
  |---|---|
  | Send now (composer, or the queue when the composer is empty) | Ctrl+Shift+Enter |
  | Stop, including the hang notice's Stop | Ctrl+. |
  | Restart the agent (hang notice) | Ctrl+Shift+R |
  | Model, Effort, Mode, More options menus | Ctrl+Shift+M, E, O, P |
  | Attach file… | Ctrl+Shift+A |
  | Edit the last queued message | Up in an empty composer |
  | ACP traffic view | Ctrl+Shift+L |
  | TODO list (header) | Ctrl+Shift+T |

  The thought disclosure and queued-entry buttons are reached with Tab and activated with Enter, like the tool summary today.

Settled by existing patterns. No question was needed for these; each follows a named convention:

- **DD15 Agent form fields** (R19, R22, R29).
  - **Removed:** the "Default permission mode" RadioGroup.
  - **Default mode:** a "Default mode" `Select` follows Effort and is filled from the probe like Model and Effort. It is absent when the agent offers no modes. Each option shows the mode name, with its description as the item's secondary text. Plan modes listed in the profile are left out.
  - **Instructions:** the field and its Append/Overwrite choice are absent for agents without a mechanism, which replaces today's muted "does not accept" note.
- **DD16 Install, update, login and Node.js in the agent form** (R9–R15, R49). All of these appear in the DD6 area under the agent picker.
  - **Update:** "Installed 0.9.1. Version 0.10.0 is available." with an "Update…" button that uses the same confirm, output and recheck steps.
  - **Node.js missing:** "Claude Code needs Node.js." with "Install Node.js…". This shows the `winget` or `brew` command in the same confirm step.
  - **No package manager:** a sentence explaining that Node.js is required, and an external "nodejs.org" `Link` (a URL, so Link, not Button).
  - **Remote machines:** the confirm text names the machine: "Adeline will run this on build-box:".
  - **Probe login:** a probe failing for authentication shows the DD12 login buttons in the probe box.
  - **Logout:** a ghost "Log out" button sits beside the status line when the agent supports it.
  - **Version error:** a probe against a non-v1 agent shows "Codex uses ACP version 2. Adeline supports version 1." with no Retry.
- **DD17 Settings > Chats** (R24, R34).
  - **Show thinking:** a "Show thinking" `Switch` row follows "Hide tool calls". Its description reads "Stream the agent's reasoning into the chat."
  - **Hang notice:** a number field labelled "Silence notice after (minutes)", default 10, with the description "0 turns the notice off."
  - **Chat settings popover:** "Show thinking" also appears in the control-bar "Chat settings" popover, like the other chat switches.
- **DD18 Transcript errors** (R2, R50). A crash shows the existing `live.error` line and a collapsed "Agent output" disclosure. The disclosure holds the stderr tail in monospace with a Copy button. The existing Retry, replacement-consent and Force Stop controls are unchanged (R39).
- **DD19 Permission card** (R21). The `permission-request` card shows the agent's option names verbatim instead of Adeline-composed labels. The first allow option stays primary, because it is the Enter commit. Reject always is not shown.
- **DD20 Hang notice styling** (R34). It is an inline `warning` Alert in the footer, so its meaning is not carried by color alone. Its title is "Agent silent for 10 min" and its actions are Stop and Restart (default Buttons).
- **DD21 Traffic view content** (R36). It is a panel tab labelled "ACP traffic" beside "Agent activity", using the Kit `Tabs`.
  - **Rows:** one row per message in monospace, with a direction marker ("→ agent", "← agent", "stderr") and a timestamp column. Non-JSON lines are marked "not JSON" and unknown updates "unknown".
  - **Copy:** "Copy all" sits in the tab's toolbar, and each row has a context menu with "Copy message".
  - **Behavior:** it scrolls to the newest message unless the user has scrolled up.
- **DD22 Demo content** (R40). `assets/workspace.json` gains one demo conversation per feature in R40, so that each one is visible without an engine: thoughts, a TODO list, slash commands, the Mode and More options menus, image and file attachments, two queued messages, each activity state, a hang notice and a populated traffic tab.

### Design guide conformance

These choices follow the GPUI Kit Design Guides and apply to every DD above:

- **Primary buttons:** only the Enter commit in a decision area is primary: Install and Update in their confirm steps, and the first allow option in a permission card. Queued-entry buttons, Stop, Retry, Log in and Log out are ghost or default.
- **Visible actions:** no essential action is hover-only. Queued entries always show Edit, Remove and Send now as small ghost buttons, and Remove is also in the entry's context menu.
- **Dropdown triggers:** the send button's caret and all composer menus stay visibly pressed while their popup is open (Kit `DropdownMenu`, `Popover`).
- **Icon-only buttons:** Stop, More options and remove-attachment have accessible names. Tooltips show the name and shortcut where one exists.
- **Ongoing work:** shown with a spinner next to the text ("Installing", "Running cargo test…"), not with animated dots.
- **Color and tokens:** status never relies on color alone. TODO steps differ by icon shape, and completed steps are muted, not green, to keep the emphasis budget. All colors come from `cx.theme()` tokens, and spacing uses rem helpers.
- **Errors:** errors sit next to the control or entry they describe and say what happened and how to recover.
- **Focus:** Escape closes the topmost popup or dialog and returns focus to its trigger. The slash list keeps focus in the composer.
- **Motion:** the thought collapse and disclosures use the Kit's short expand transition and honor reduced motion.
- **Copy:** labels are sentence case, with "…" on controls that open a dialog or ask for more input (Install…, Update…, Add server…, Log in in terminal…).

## Verification

Checks are done in the real window, through `scripts/engine-check/ui_check.py` on Windows and computer use on macOS, and in demo mode where noted.

| AC | Design check |
|---|---|
| AC1, AC3, AC6, AC36, AC39, AC40 | No UI impact: internal structure, storage and process handling |
| AC2 | Probe against a non-v1 fake agent shows the DD16 version message without Retry |
| AC4 | The traffic tab shows the non-JSON line and the unknown update with their markers (DD21). The permission card is answered |
| AC5 | Against a fake agent with no optional features, the Mode and More options menus are hidden, image attach shows its reason, and Send now carries the "Stops the turn, then sends" tooltip (DD13) |
| AC7 | Between turns, a mode change updates the Mode label. A command update changes the `/` list. A title update changes the header and list row |
| AC8 | Per agent, each listed item is reachable through the surfaces in this document, or is hidden or explained per DD13 |
| AC9 | DD6 steps in order: Install…, command shown, confirm, live output, recheck to "found on path". The failure state keeps the output and offers Retry. Pi's command list includes the `pi` CLI |
| AC10 | DD16 Node.js offer with the winget/brew command, and the no-package-manager text with the nodejs.org Link |
| AC11 | DD16 update line and Update… confirm. Nothing runs before Update is pressed |
| AC12 | DD12 card in a conversation and DD16 buttons in the probe box. A terminal method's button opens the OS terminal. Log out appears only for agents that support it |
| AC13 | With a remote machine selected, the confirm step names the machine and output streams into the same area |
| AC14 | A new chat's Model, Effort and Mode labels match the agent definition after another chat changed its own |
| AC15 | Menus change only the current chat. With the agent stopped they still open. During a turn they are disabled with the tooltip "Switch after this turn finishes". An agent-side mode change updates the label |
| AC16 | An extra agent option appears in the More options menu (DD1) and changes only that chat |
| AC17 | The agent form shows "Default mode" for an agent with modes and omits it otherwise (DD15) |
| AC18 | The Mode menu lists names with descriptions, without Claude's plan mode. The form's Default mode also omits it. No permission picker or permission RadioGroup exists anywhere |
| AC19 | The permission card shows the agent's option names verbatim and no reject always option (DD19) |
| AC20 | No UI impact beyond AC18. An old definition opens in the form without a permission field |
| AC21 | A pasted image becomes a miniature chip and appears in the sent message. A file shows as a file chip. The image and size errors appear under the chips (DD9) |
| AC22 | Settings > Chats has "Show thinking" after "Hide tool calls". Off: the Thinking indicator. On: streamed muted text collapsing to "Thought for Ns" (DD4, DD17) |
| AC23 | `/` opens the list, typing filters, Up/Down/Enter/Tab/Escape behave per DD10, and the saved list shows with the agent stopped |
| AC24 | The header TODO button shows the current step, and Ctrl+Shift+T or a click opens the full list. A step change updates both in place, and a new list replaces them. Sending a prompt keeps the list, an empty list removes the button, and the list is back after an Adeline restart (DD8) |
| AC25 | Header and list row show the agent's title, then the later one |
| AC26 | Settings Agents > MCP servers and the agent form list exist (DD11). A skipped server shows the start-of-conversation note |
| AC27 | The instructions field is present for Claude and OMP and absent for Codex, Pi and opencode (DD15) |
| AC28 | During a turn Enter queues and Ctrl+Shift+Enter sends now. The caret menu offers both (DD2) |
| AC29 | Queued entries show Edit, Remove and Send now. Edit moves the text to the composer, and Up in an empty composer edits the last one (DD3, DD14) |
| AC30 | The activity row reads each state as it occurs (DD5) |
| AC31 | No UI impact beyond the activity row returning to idle |
| AC32 | The DD20 notice appears after the configured silence, is absent at 0, and is absent while a tool is running |
| AC33 | The crashed chat shows the error with the "Agent output" disclosure. Other chats are unaffected (DD18) |
| AC34 | Ctrl+Shift+L opens the panel on "ACP traffic", which shows both directions and stderr, and "Copy all" copies. `docs/shortcuts.md` lists every DD14 shortcut |
| AC35 | Pre-change chats are absent from the chat list. Agents remain in Settings > Agents |
| AC37 | Existing retry, continue, replacement consent, storage block and Force Stop controls look and behave as today |
| AC38 | `--demo` shows each R40 feature (DD22) |
| AC41 | During install, login, probe and startup, the window keeps responding to typing and navigation. Only the area doing the work shows a spinner |

Every R ID is covered by a DD above or listed under "No UI impact".

## Open questions

None.
