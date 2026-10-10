Status: Implemented in v0.1.10. This scope is history. Where it and the code differ, the code is right.

# ACP rebuild on the official SDK

## Purpose and context

Adeline users run coding agents (Claude, Codex, Pi, opencode, OMP) in conversations. Today Adeline talks to them through a hand-rolled ACP client that covers only part of ACP v1. As a result, agents work with fewer features than they offer, setup is manual, and the code is hard to maintain.

Before: adding Claude requires installing `claude-agent-acp` globally by hand. Once it runs, Adeline shows no TODO list, no thoughts and no slash commands, and can't send it a pasted image. After: Claude can be installed from Adeline in the standard global way, and conversations show its TODO list, thoughts (if enabled), slash commands, modes and attachments.

The goal of the rebuild is the highest long-term maintainability, standardization and interoperability. It follows option B of the ACP research: the official Rust SDK, an Adeline-owned domain model, data-driven agent profiles and full ACP v1 coverage.

Current behavior (inspected 2026-10-10):

- `src/acp.rs`, `Worker` and `Driver::spawn`: one tokio actor per conversation over `serde_json::Value`. `initialize` sends `protocolVersion: 1` with empty `clientCapabilities` and rejects any other version. Prompts are text only. Only `session/request_permission` is served; other agent requests get -32601. `permission_request` ignores string request ids. A non-JSON stdout line ends the connection. Thought chunks, TODO lists (`plan` updates), commands, mode, config, title and other updates are ignored, and updates outside a turn are dropped except usage.
- `src/harness.rs`, `probe` and `Session`: a second JSON-RPC client used by the agent form. `parse_registry` guesses executable names from package names and ignores the registry `env` field. `locate` never installs.
- `src/protocol.rs`, `Live.options` and `src/storage.rs`, `ConversationSettings.config_options`: raw ACP JSON (`Vec<Value>`) crosses the engine↔client protocol and is stored.
- `src/agents.rs`, `AgentDefinition` and `PermissionMode`: agents have `permission_mode` (Ask, AllowReads, AllowEverything). `PROTECTED` in `src/acp.rs` is a substring blocklist, recorded as weak in `docs/archive/0.1.2-tech-debt.md`.
- `src/engine.rs`, `Engine::start_prompt`: the title is the first 100 characters of the first prompt. There is no rename.
- `src/acp.rs`: `mcpServers` is always `[]`.
- `src/chat_render.rs`, `attach-chat-files`: the attach button exists. Message images (`data.rs`, `Message.images`) are demo-only.
- `docs/archive/scope-acp-agent-driver.md` and `docs/archive/scope-agent-onboarding.md` define the behavior this scope keeps or supersedes.

Research basis: Zed, Zeron, Waku, Codeg and ACPX, plus the official `agent-client-protocol` Rust SDK 3.3.0 and schema 1.11.0, cloned under `C:\Cloud\Dev\acp-research`.

## Requirements

### Protocol foundation

- **R1.** All ACP communication, including probes, goes through one client implementation built on the official `agent-client-protocol` Rust SDK and its typed v1 messages. The separate probe client is removed.
- **R2.** Adeline speaks ACP v1 only. It proposes protocol version 1, and an agent that cannot use v1 gets a clear configuration error. Protocol v2 is not negotiated in this scope.
- **R3.** Adeline's conversation model is its own and independent of ACP types. No ACP types or raw ACP JSON appear in the engine↔client protocol, in stored conversation settings, or in the UI. These are all Adeline-owned types: model, effort, mode and config options, tool kinds and statuses, permission options, TODO lists, usage, commands and stop reasons. Entries (messages, thoughts, tool calls) are identified by ID and updated in place. A turn has an explicit state: running, idle or needs action. This keeps a later move to v2 to a translation change.
- **R4.** Adeline handles any valid JSON-RPC message from an agent. It accepts string and number request IDs. It skips stdout lines that are not JSON and keeps them for diagnostics without ending the connection. It records unknown update types and methods instead of failing on them. Unsupported agent requests get a JSON-RPC error, never silence.
- **R5.** Adeline uses an ACP feature only when the agent advertises it (load, resume, fork, close, prompt content types, MCP transports, login methods, modes, config options, steering). A feature the agent does not offer is shown as unavailable, not broken.
- **R6.** Agent-specific behavior lives in one profile per agent, not in branches spread through the code. A profile covers:
  - launch details;
  - install and update method per OS;
  - the system-instructions mechanism;
  - turn-end signals;
  - descriptions supplementing the agent's mode descriptions;
  - the agent's plan modes, which Adeline hides (R20).

  Agents without a profile, including other registry agents and Custom agents, run with generic ACP behavior.
- **R7.** Updates that arrive outside a turn are applied: commands, mode, config options, title and usage.

### Supported agents, installation and login

- **R8.** These agents are supported and verified:
  - Claude, via `@agentclientprotocol/claude-agent-acp`;
  - Codex, via `@agentclientprotocol/codex-acp`;
  - Pi, via `pi-acp` with the `pi` CLI;
  - opencode;
  - OMP.

  Gemini CLI is not in the verified set.
- **R9.** For a supported agent that is not installed, Adeline offers to install it with that agent's documented, standard global installation for the current OS. Install methods per agent:
  - **npm-based adapters:** `npm install -g`.
  - **opencode:** one of its vendor install methods.
  - **OMP:** its own installer.
  - **Required companion tools,** such as the `pi` CLI for Pi: installed in the same offer.

  The result is on PATH like a manual install. Adeline never runs agents through `npx` or another on-demand package runner, and never installs into a folder Adeline owns.
- **R10.** Installing shows the exact commands and asks for confirmation. It then runs them with live output shown in Adeline, and afterwards checks again that the agent is installed and on PATH. A failed install keeps its output visible and offers Retry.
- **R11.** If Node.js is required and missing, Adeline offers to install it through the OS package manager (winget on Windows, Homebrew on macOS). If that package manager is also missing, it explains what is needed and links to nodejs.org.
- **R12.** When an installed supported agent is older than the registry version, Adeline offers an update using the same install method. It never updates without the user's confirmation.
- **R13.** Installing, updating and logging in through Adeline are offered, never required. An agent installed or logged in by hand must be detected (PATH and known install locations) and must work the same as one set up through Adeline.
- **R14.** Adeline supports ACP login using the agent's advertised login methods:
  - **Where offered:** in the agent form when a probe fails for missing authentication, and in a conversation when the agent reports that authentication is required.
  - **Agent-handled login methods:** Adeline calls the agent's login and opens any URL the agent provides.
  - **Terminal login methods:** Adeline opens the user's own OS terminal with the login command, then checks again.
  - **Logout:** offered when the agent supports it.
- **R15.** Install, update, login and probe run on the machine where the agent will run, including remote machines, using that machine's OS-specific methods. Their output streams to the user's client.

### Conversation settings

- **R16.** A new conversation always starts with the default model, effort and mode saved in its agent definition. Other agent options start at the agent's own defaults.
- **R17.** The composer has Model, Effort and Mode menus. They list the live options offered by the running agent, apply a selection to the running session, and save it to that conversation only, never to the agent definition. For a conversation whose agent is not running, the menus offer the options saved from its last session, and the choice applies when the agent next starts. Selection is disabled while a turn is processing. When the agent changes one of these settings itself, the menu shows the new value.
- **R18.** Any other select or boolean option the agent offers appears in one "More options" menu that works for any agent, with the same per-conversation behavior as R17. These options have no defaults in the agent definition.
- **R19.** The agent form gains a default Mode, read from the probe like model and effort. The Mode field is absent when the agent offers no modes.

### Permissions

- **R20.** The agent's own modes are the only permission control. The Mode menu shows each mode's name and description, as given by the agent and supplemented by its profile when missing or unclear. Adeline's permission modes (Ask, Allow reads, Allow everything) and the protected-command list are removed. Modes the agent's profile lists as plan modes appear neither in the Mode menu nor in the agent form's default Mode (R19), so they can't be chosen in Adeline. If the agent switches itself into one, the menu still shows it as the current value (R17).
- **R21.** A permission request shows the options the agent offers, except "reject always", which stays hidden. Remembering "allow always" is up to the agent. An agent without modes is asked about every request it sends. If no UI client is connected when a permission request arrives, the turn is cancelled, as today.
- **R22.** Existing agent definitions stay usable. Their saved permission mode is ignored and dropped the next time the definition is saved. An agent definition without a default mode uses the agent's own default.

### Prompt content and display

- **R23.** Attachments:
  - **Images:** can be pasted, dropped or picked. Sent as image content only when the agent advertises image support; otherwise attaching is blocked with a short reason.
  - **Other files:** can be dropped or picked. Sent as embedded content when the agent supports embedded context, and as a file link otherwise.
  - **Size:** a file over 20 MB is rejected with a clear error.
- **R24.** A Chats setting, "Show thinking", next to the setting that hides tool calls, is off by default. Off: thoughts show as the generic Thinking indicator. On: thought text streams into the conversation. Thoughts are always recorded in the transcript.
- **R25.** Typing `/` at the start of the composer opens a filterable list of the commands the agent advertises, with descriptions and input hints. The list is navigable by keyboard and follows the agent's updates. While the agent is not running, the conversation's last saved list is shown.
- **R26.** The agent's TODO list (ACP calls it `plan`) is shown in the chat header:
  - **Collapsed:** by default the header shows only the current step and its status.
  - **Expanded:** the user can expand it to see the full list with each step's status.
  - **Updates:** each update from the agent replaces the whole list. A new prompt does not clear it. It disappears when the agent sends an empty list.
  - **Persistence:** it is saved with the conversation and restored after an Adeline restart.
  - **History:** earlier versions of the list are not displayed. They remain in the raw traffic (R38).
- **R27.** A title from the agent replaces the automatic title (the first 100 characters of the first prompt). Later title updates from the agent replace it again.
- **R28.** MCP servers:
  - **Where configured:** a global list in Settings, plus a per-agent list in the agent form. Both are sent when a session starts.
  - **Server types:** stdio (command, arguments, environment) and HTTP (URL, headers).
  - **Unusable servers:** a server whose transport the agent does not support is skipped with a visible note.
  - **Saving:** a conversation saves the servers it started with, like its other execution settings.
- **R29.** System instructions are delivered through the agent profile's mechanism:
  - **OMP:** command-line flags, as today.
  - **Claude:** `_meta.systemPrompt` on session creation. Append mode uses the append form, and overwrite mode replaces the default prompt.
  - **Codex, Pi and opencode:** no instructions until a mechanism is verified.
  - **Agent form:** the instructions field stays hidden for agents without a mechanism.

  For every agent with a mechanism, the guidance begins with the name sentence defined in onboarding R16.

### Turns

- **R30.** The composer is never blocked by a running turn. It offers:
  - **Queue:** the message is delivered when the turn ends.
  - **Send now:** the message is delivered immediately, through steering when the agent advertises it. Otherwise Adeline cancels the turn and sends the message as the next prompt.
- **R31.** Each queued message appears as a separate pending entry under the running turn and can be edited, removed or sent now. When the turn ends, all queued messages are sent as one prompt, separated by blank lines.
- **R32.** The conversation shows the agent's activity from real signals: streaming, tool running, waiting for permission, background tasks running (where the agent reports them), or quiet.
- **R33.** A turn ends on the agent's prompt response, or on a turn-end signal declared in the agent's profile. Adeline never ends a turn on its own because of silence.
- **R34.** A hang notice ("Agent silent for N min", with Stop and Restart) appears when there has been no ACP traffic and no open tool or permission for the configured time. The time is set in Chats settings, defaults to 10 minutes, and 0 turns the notice off. Adeline takes no automatic action.
- **R35.** Each conversation has its own agent process. A crash affects only that conversation.

### Diagnostics

- **R36.** Each conversation has an ACP traffic view. It shows the raw ACP messages in both directions plus the agent's stderr, with copy, and opens with a keyboard shortcut.

### Data

- **R37.** Fresh start: conversations saved before this change stay on disk untouched but are hidden. Agent definitions are kept, subject to R22.
- **R38.** Raw ACP traffic is still recorded in each conversation's transcript, for diagnostics and the R36 view. Adeline's own events remain the source of the displayed history.

### Retained behavior

- **R39.** Everything the earlier scopes define stays as it is, rebuilt on the new layer, except where this scope changes it (R20–R22 for permissions; R24, R14 and R29 for earlier decisions on thoughts, login and instructions). That includes:
  - automatic retries with the configurable limit;
  - continuing an interrupted turn;
  - a replacement session only with the user's consent;
  - blocking prompts when storage fails;
  - graceful shutdown with the stuck notice and Force Stop;
  - fork;
  - remote engines;
  - demo mode.
- **R40.** Demo mode shows canned examples of the new conversation features:
  - thoughts;
  - TODO list;
  - slash commands;
  - Mode and More options menus;
  - attachments;
  - queued messages;
  - activity states;
  - the hang notice;
  - the ACP traffic view.

### Constraints and quality requirements

- **R41.** Storage and streaming:
  - One conversation's streaming or storage never delays another conversation.
  - Streamed events are written to disk within 250 ms, in batches.
  - Turn ends, permission answers and lifecycle events are on disk before they are acknowledged or shown as done.
  - A crash can lose at most the last 250 ms of streamed text, never a decision.
- **R42.** Force Stop and engine shutdown end the agent's whole process tree, including processes started through npm shims, on Windows and macOS.
- **R43.** Installing, updating, logging in, probing and agent startup never block the UI.
- **R44.** Every new control has a keyboard shortcut, listed in `docs/shortcuts.md`.
- **R45.** `PROTOCOL` in `src/protocol.rs` is bumped for the changed engine↔client messages.
- **R46.** Everything works on Windows and macOS, and the headless build keeps working.

### Failure and edge cases

- **R47.** If an agent reports that authentication is required mid-conversation, Adeline offers login (R14) and manual retry. Authentication failures are never retried automatically.
- **R48.** If an agent does not advertise steering, "Send now" cancels the running turn and waits for the cancellation to settle before sending.
- **R49.** If Node.js is missing and no supported package manager exists, install stays unavailable with an explanation and the nodejs.org link. Manual installation remains possible.
- **R50.** An agent that closes its stdout or exits mid-turn produces an error in that conversation only. The stderr tail is included in the error.

## Boundaries

- Adeline does not serve file or terminal access to agents (`fs/*`, `terminal/*`). Agents keep using their own tools.
- No import of an agent's past sessions through `session/list`.
- No protocol v2 negotiation.
- No installs into an Adeline-owned folder, and no `npx`.
- Gemini CLI is not in the verified set. It still runs as a generic registry agent if installed.
- Existing conversations are not migrated (R37).
- No plan mode support. Plan modes are hidden from mode choices (R20). Plans an agent proposes in plan mode (for example Claude's ExitPlanMode, or Codex's plan review) get no special display. The TODO list (R26) is a separate feature and is unaffected.
- This scope supersedes:
  - onboarding R3, about never installing;
  - onboarding R17 and agent-driver R27, about logging in only outside Adeline;
  - agent-driver R17, which allowed only the generic Thinking indicator;
  - agent-driver R22–R24 and the `permission_mode` field, about permission modes.

  It extends onboarding R14 and R16, about instructions.

### Rejected ideas

- Keeping Adeline's permission modes alongside agent modes with a warning, and translating Adeline's modes into agent modes per agent (Waku-style).
- Showing "reject always".
- One agent process shared by several conversations.
- Defaults for "More options" in the agent definition.
- A per-conversation thinking toggle.
- A fixed hang threshold.
- Deleting old conversations, or resetting agent definitions.
- Opening the user's terminal for installs instead of running them inside Adeline.
- Installing only on the local machine.
- Waiting for the real turn end with only Stop as a way out.
- Ending a turn on a silence timeout.
- Showing the TODO list as conversation entries, alone or alongside the header.
- Clearing the TODO list when a new prompt is sent.
- Showing a plan mode plan's Markdown in the permission card.

## Domain and data

- **Agent profile:** per-agent data for supported agents (R6). Agents without a profile behave generically.
- **Agent definition:** gains a default mode and a per-agent MCP server list, and loses the permission mode.
- **Conversation:**
  - **Saved at creation:** the agent's resolved launch, model, effort, mode, other options, MCP servers and instructions.
  - **Later changes:** model, effort, mode and other options may change during the conversation and are saved to it alone (R17, R18).
  - **Restored with the conversation:** its last known command list, option lists and TODO list.
- **TODO list:** the agent's checklist for its current work, called `plan` in ACP. A conversation has at most one. Each step has text and a status: pending, in progress or completed. "Plan" in Adeline's wording refers only to an agent's plan mode, which is out of scope.
- **Turn states:** running, idle, needs action.
- **Activity states:** streaming, tool running, waiting for permission, background tasks, quiet.
- **Queued message:** belongs to a running turn. It is pending until it is sent, edited or removed.

## Interfaces and dependencies

- **Rust SDK:** the official `agent-client-protocol` crate.
- **ACP registry:** for agent metadata and update versions.
- **Install tools:** npm, winget, Homebrew, and each agent's vendor installers.
- **Terminal:** the user's OS terminal, for terminal login.
- **Engine↔client protocol:** carries all new state (R45).
- **Remote engines:** run install, login and probe for their own machine (R15).
- **Testing:** the UI Automation suite in `scripts/engine-check/` covers the new controls on Windows.

## Acceptance criteria

- **AC1** (R1): No ACP message is built or parsed from untyped JSON outside the SDK layer. The agent form's probe uses the same client as conversations.
- **AC2** (R2): An agent answering with an unsupported protocol version shows a configuration error. Supported agents connect on v1.
- **AC3** (R3, R45): `protocol.rs`, stored conversation settings and UI types contain no ACP crate types or raw ACP JSON. `PROTOCOL` is bumped.
- **AC4** (R4): A fake agent that prints a non-JSON line, uses a string ID for a permission request, and sends an unknown update keeps working. The permission is answered, and the line and update appear in the traffic view.
- **AC5** (R5): Against an agent advertising no optional features, the related controls show as unavailable and no unadvertised method is called.
- **AC6** (R6): Agent-specific behavior for the five agents is defined in their profiles. A Custom agent runs a full conversation with generic behavior.
- **AC7** (R7): Commands, mode, config option, title and usage updates sent between turns are reflected in the UI.
- **AC8** (R8, R13): For each of Claude, Codex, Pi, opencode and OMP, the following work wherever the agent supports them:
  1. Install, or detection of a manual install
  2. Login, or a manual login
  3. Probe
  4. A new conversation with defaults
  5. Streaming text and thoughts
  6. Tool calls with permissions
  7. Stop
  8. Queue and Send now
  9. Model, effort and mode switching
  10. More options
  11. Image and file attachments
  12. Slash commands
  13. TODO list
  14. Title
  15. MCP servers
  16. Continuing after an Adeline restart
  17. Fork

  Unsupported items show as unavailable.
- **AC9** (R9, R10): Installing a missing npm-based agent shows the `npm install -g` command, runs it after confirmation with live output, and the agent is then detected on PATH. Pi's offer includes the `pi` CLI. A failing install shows its output and Retry.
- **AC10** (R11, R49): With Node missing, Adeline offers winget or Homebrew. Without those, it shows the explanation and the nodejs.org link.
- **AC11** (R12): An agent older than the registry version shows an update offer. Nothing updates without confirmation.
- **AC12** (R14, R47): Login is offered on a probe auth failure and on an auth-required error in a conversation:
  - An agent-handled login method completes and opens the URL.
  - A terminal login method opens the OS terminal.
  - Logout appears only when the agent supports it.
- **AC13** (R15): Installing and probing for a remote machine run on that machine and stream output to the client.
- **AC14** (R16): A new conversation starts with the agent definition's model, effort and mode, even after other conversations changed theirs.
- **AC15** (R17): A model, effort or mode change applies to the running agent and that conversation only. It is offered while the agent is stopped and applied on next start, and is disabled during a turn. An agent-side mode change updates the menu.
- **AC16** (R18): An agent offering an extra option shows it under More options. Changing it affects only that conversation.
- **AC17** (R19): The agent form shows a default Mode for agents with modes and omits it otherwise.
- **AC18** (R20): The Mode menu shows mode names and descriptions. The Ask, Allow reads and Allow everything modes and the protected-command list no longer exist. Claude's plan mode, listed in its profile, is absent from the Mode menu and the agent form's default Mode.
- **AC19** (R21): A permission request shows the agent's options without "reject always". With no client connected, the turn is cancelled.
- **AC20** (R22): An existing `agent.yml` with `permission_mode` loads, works and loses the field when saved.
- **AC21** (R23): A pasted image reaches an image-capable agent. Image attaching is blocked for agents without image support. A file is embedded or linked according to capability. A file over 20 MB is rejected with an error.
- **AC22** (R24): With "Show thinking" off, the Thinking indicator appears. With it on, thought text streams. The transcript contains thoughts either way.
- **AC23** (R25): `/` opens the agent's commands, filters as you type, works by keyboard, and shows the saved list while the agent is stopped.
- **AC24** (R26): The chat header shows the current TODO step and expands to the full list. A step status change updates it in place, and a new list replaces it. A new prompt leaves it in place, an empty list removes it, and it is still shown after an Adeline restart.
- **AC25** (R27): An agent title replaces the automatic title, and a later title replaces it again.
- **AC26** (R28): Global and per-agent MCP servers reach the agent at session start. An unsupported-transport server is skipped with a note.
- **AC27** (R29): Claude receives the instructions in append and overwrite modes, with the name sentence first. OMP behaves as today. Codex, Pi and opencode show no instructions field.
- **AC28** (R30, R48): During a running turn, Queue delivers the message after the turn. Send now steers an agent with steering, and cancels then sends on one without.
- **AC29** (R31): Queued messages can be edited, removed and sent now. At turn end they arrive as one prompt separated by blank lines.
- **AC30** (R32): The conversation shows streaming, tool running, waiting for permission, background tasks and quiet as they occur.
- **AC31** (R33): A profile-declared turn-end signal ends the turn without a prompt response. No turn ends because of silence alone.
- **AC32** (R34): After the configured silence with nothing open, the hang notice appears with Stop and Restart. A value of 0 disables it, and a long-running tool never triggers it.
- **AC33** (R35, R50): Killing one conversation's agent shows an error with the stderr tail in that conversation only.
- **AC34** (R36, R44): The traffic view opens by shortcut, shows both directions and stderr, and copies. Every new control's shortcut is in `docs/shortcuts.md`.
- **AC35** (R37): Conversations from before the change are hidden and their files are unchanged. Agent definitions remain listed.
- **AC36** (R38): The transcript holds the raw ACP traffic. Displayed history is rebuilt from Adeline's events.
- **AC37** (R39): The existing `engine-check` scenarios pass on the new layer: retry, interrupted-turn continuation, replacement consent, storage block, stuck shutdown with Force Stop, fork and remote.
- **AC38** (R40): Demo mode shows each new feature listed in R40.
- **AC39** (R41): Two conversations streaming at once do not slow each other. Killing the engine mid-stream loses at most 250 ms of text, and no turn end or permission answer.
- **AC40** (R42): After Force Stop of an agent started through an npm shim, no process from its tree remains, on Windows and macOS.
- **AC41** (R43, R46): The UI stays responsive during install, login, probe and startup, on Windows and macOS. The headless build compiles and runs the engine.

## Decisions and rationale

- **Option B from the research:** the official SDK and an Adeline-owned model, for maintainability, standardization and interoperability. The SDK is adopted even though it changes fast.
- **v1 only, with a model shaped for v2:** v2 is still a draft. Modelling on entries updated by ID and an explicit turn state keeps v2 a translation change.
- **Agent modes are the only permission control:**
  - The ACP spec makes modes the agent's way to decide whether it asks.
  - Agents enforce modes before acting.
  - The protected list was bypassable (`docs/archive/0.1.2-tech-debt.md`) and skipped silently by modes that don't ask.
- **No `fs/*` or `terminal/*`:**
  - It would add a large security surface.
  - Adeline has no unsaved editor buffers to offer.
  - v2 removes both.
  - Zeron and Waku declined it for the same reasons.
- **Standard global installs:** agents installed through Adeline behave exactly like ones installed by hand and stay usable outside Adeline. `npx` stays excluded for predictability.
- **One process per conversation:** crash isolation, and per-conversation launch settings such as OMP's instruction flags.
- **No timeout ends a turn:** adapters holding turns open (Zeron's experience) are handled by never blocking the user, by real activity signals, and by profile-declared turn-end signals.
- **Fresh start for conversations:** keeps the rebuild free of the old raw-JSON storage format.
- **TODO list in the header, replaced on every update:** ACP v1 requires the client to replace the whole list on each update, and v1 lists carry no ID, so a conversation has one current list. The header keeps it visible without filling the transcript. The name "TODO list" avoids confusion with plan mode.
- **No plan mode support:** the user considers plan mode a dead feature that Claude may disable entirely. Plan modes are hidden through the agent profiles rather than left visible as ordinary modes.

## Open questions

None.
