# Architecture

Every file in `src/` opens with a `//!` summary, and `grep -rm1 '^//!' src` prints the module map. This doc covers what the map can't show: how the modules connect. Paths below are relative to `src/`.

## One binary, three roles

`main.rs` picks the role:

- UI: `adeline` (or `adeline --demo`) opens the window (`ui::open`).
- Engine: `adeline engine` (`engine::main`) runs a background process that owns agent processes, conversation state and storage. One engine runs per OS user, and it outlives the UI.
- Bridge: `adeline bridge` (`remote::bridge_main`). A remote client runs it over SSH to reach that machine's engine (see Machines).

`ssh` also runs the binary as its `SSH_ASKPASS` helper, and `main` checks for that first (see Machines).

The default `gui` feature builds the full app. `--no-default-features` builds the headless binary: engine, bridge and `--version` only, with no GPUI. Remote installs use it (`adeline-headless-<platform>.zip`). Build it with `--target-dir target/headless` so it doesn't replace the full `adeline` in `target/release`.

UI and engine talk over a same-user named pipe on Windows and a Unix socket elsewhere (`ipc.rs`), one JSON object per line. The message types live in `protocol.rs`, and bumping `PROTOCOL` there is the only version change needed. `scripts/engine-check/engine_client.py` reads it from that file.

## Source layout

The code has two parts:

- The core, `src/*.rs` and their directories: everything the headless build needs. No GPUI.
- The UI, `ui.rs` and `ui/`: the window and everything in it. `main.rs` declares `mod ui` behind `#[cfg(feature = "gui")]`. Apart from `main` choosing what to run and the engine reporting which build it is, that is the only place the code checks the feature. In the headless build, core code that only the UI calls is allowed to be dead.

The UI imports from the core, and nothing in the core imports from `ui`. Keep it that way. It keeps the headless build free of GPUI, and it means the core could become its own crate without untangling anything.

The core, from the bottom up:

- Helpers that import nothing from Adeline: `files.rs` (atomic writes, safe folder names), `platform.rs` (process containment and spawning), `recency.rs`, `log.rs`. `config.rs` (preferences, `settings.yml`) sits just above them.
- The model: `conversation.rs` (Adeline's own conversation model; no ACP types), `data.rs` (projects, chats and messages as clients see them, plus the demo workspace), `prepared.rs` (chat search), and `protocol.rs` (engine ↔ client messages). `data` and `protocol` still import each other: the demo's live state is a `protocol::Live`.
- Agents: `profiles.rs` (what Adeline knows about each supported agent), `harness.rs` (installed agents and their versions), `install.rs`, `agents.rs` (the user's agent definitions) and `acp.rs`.
- Persistence: `storage.rs`.
- Machines: `machines.rs` (saved machines and destination parsing), `remote.rs` (SSH transport) and `askpass.rs`.
- `engine.rs`, which uses all of the above.

A large module becomes a directory: `foo.rs` keeps the types, the state and the dispatch, and each child in `foo/` adds an `impl` block or helpers for one concern. Children import from the parent with `use super::…`, and items they share with the parent or each other are `pub(super)`. Big test suites go in `foo/tests.rs`. `engine`, `acp`, `storage`, `ui/chat`, `ui/settings`, `ui/agent_form`, `ui/project_bar` and `ui/runtime_ui` follow this pattern.

## A user action, end to end

1. A click, key binding or macOS menu-bar item calls `app.act(Action::…)`. `Action` is in `ui.rs`, keys are bound in `ui::bind_keys`, and the menu bar is in `ui/menu_bar.rs`.
2. `Adeline::act` (`ui/interaction.rs`) dispatches it. Actions that touch a conversation go to `ui/runtime_ui.rs`.
3. `runtime_ui.rs` sends a `protocol::Command` through `ui/client.rs`, the UI's engine connection.
4. `Engine::command` (`engine.rs`) handles it, persists through `storage.rs`, and sends `acp::Command`s to the conversation's worker.
5. `acp.rs` runs one ACP stdio agent process per conversation. Its events come back to `Engine::driver_event`.
6. The engine broadcasts a `protocol::Delta`, and `Adeline::apply_delta` (`ui/runtime_ui.rs`) updates the UI's model (`data.rs`).

A new conversation feature usually touches every step: `Action`, `act`, `runtime_ui`, `Command`/`Delta` in `protocol.rs`, `Engine::command`, and often `acp.rs` and `storage.rs`.

## The engine

Every command and agent event goes through one task, in arrival order, so commands from several clients can't interleave or deadlock. `engine.rs` keeps that task's state, its dispatch (`command`, `input`, `tick`) and broadcasting. Its children:

- `serve.rs`: the single-instance lock, the runtime, and one task per client connection.
- `turns.rs`: sending, queueing, starting, stopping and retrying prompts.
- `conversations.rs`: a conversation's driver, forks, option changes, permission answers and read markers.
- `events.rs`: events from agent workers.
- `projects.rs`: projects, the agent catalog, harness commands, engine settings and watched files.
- `cli.rs`: the `adeline engine` command line.

`storage.rs` keeps durable projects and conversations. `storage/disk.rs` reads and writes a conversation's files, and `storage/paths.rs` checks that nothing is read or deleted through a link or outside project storage.

## Agents and ACP

`acp.rs` speaks ACP through the official `agent-client-protocol` SDK, which owns JSON-RPC framing, request IDs and the typed schema. Its `Worker` is a state machine fed by `Input`s: SDK requests and notifications, replies, process exit, and `Command`s from the engine. `acp.rs` keeps the types and the worker's loop. Its children handle session setup and shutdown (`session.rs`), options, turns, permissions, failures and retries, and probes and logins (`probe.rs`). `acp/tests.rs` drives the worker through a fake `Link`.

The worker translates SDK types into `conversation.rs`: session options by category, TODO steps, tool calls, permission options, features, attachments, MCP servers and traffic entries. Nothing outside `acp/` sees an SDK type. Every line in and out, and the agent's stderr, is reported as a `TrafficEntry`. The engine keeps those in the transcript and streams them to clients watching the traffic tab.

`profiles.rs` knows each supported agent (Claude, Codex, Pi, opencode, OMP): its command, install steps, how it takes system instructions, which modes are plan modes, and how its turns end. `harness.rs` finds installed agents and their versions, and `install.rs` plans and runs installs and opens login terminals. The engine runs those for a client (`PlanInstall`, `Install`, `Login`), so a remote machine installs on itself.

## Machines

One client shows the projects of every checked machine. `machines.rs` keeps the saved remote machines and which ones are checked, in `machines.yml` on the client only. Each project in the UI carries its `machine`, and `Workspace::key()` (`machine:id`) tells apart same-named projects on two machines. A machine's projects sit together in `Adeline::projects`, so `protocol::apply` runs on that machine's slice (`ui/runtime_ui/machine_sync.rs`).

`ui/client.rs` keeps one `Connection` per checked machine. Every request names its machine: `Adeline::request` uses the open project's machine, and `machine_request` any other. The local engine is reached through `ipc.rs`. A remote one goes through `remote.rs` over the system `ssh`:

1. A probe finds the remote platform, its engine identity (`engine/id`) and the Adeline in `~/.adeline/bin` (its `version` file). Unix hosts answer `sh`. Windows hosts get a base64 PowerShell script, which reads the same under cmd and PowerShell.
2. A missing Adeline is installed in the client's exact version. That's the headless build from its GitHub release, or, if there is no release and the platform matches, the client's own executable sent over `ssh` stdin.
3. `adeline bridge` joins the SSH session to that machine's engine pipe or socket, starting the engine like a local client would. No network port opens anywhere.
4. The client checks the welcome message: the saved engine identity, and `PROTOCOL`. The Adeline version doesn't matter, so a 0.1.7 client uses a 0.1.6 engine with the same protocol. A newer remote protocol asks for a local update. An older one gets replaced, after the user consents: step 2's install runs (when the installed Adeline is older) and the engine restarts.

`ssh` runs this executable as `SSH_ASKPASS` with `ADELINE_ASKPASS` set. The helper in `askpass.rs` hands each prompt to the UI over an owner-only pipe or socket. A password typed during one connect answers the same prompt in that connect's later `ssh` runs, and is then forgotten.

The engine numbers every delta it broadcasts and keeps the latest ones. A reconnecting client sends its place (`Hello.resume`). It gets `Resumed` plus only the deltas it missed, or a full snapshot if the engine restarted or the gap is too long. A dropped remote machine keeps its last state on screen, read-only, while `ui/client.rs` reconnects with growing delays. `ui/runtime_ui/screens.rs` draws what shows while an engine isn't usable.

`ADELINE_SSH` replaces the `ssh` command line. `scripts/engine-check/fake_ssh.py` uses it to simulate remote Windows machines on one computer.

## The UI

`ui.rs` holds the root view `Adeline`, the `Action` enum, shared drawing helpers (`row`, `col`, `text`, `icon`) and `open`, which builds the window. `Adeline`'s `impl` blocks are spread over the modules in `ui/`, one concern each: `interaction.rs` (dispatch), `runtime_ui.rs` (conversations, mirrored from the engine), `views.rs`, `panes.rs`, `project_bar.rs`, `project_ui.rs`, `conversation_ui.rs` and `settings.rs`.

The chat regions are separate GPUI views, so typing or scrolling one doesn't redraw the others. `ui/chat.rs` declares them and holds the drawing helpers other surfaces share. Each region lives in a child: `chat/list.rs` (the chat list and sidebar), `chat/transcript.rs` and `chat/composer.rs`.

The Settings and agent windows are separate windows (`ui/settings.rs`), one child per page. The agent form they share is in `ui/agent_form.rs`.

`ui/client.rs` holds the engine connections as a GPUI global. `ui/ui_state.rs` keeps what this UI remembers for itself and doesn't share with other clients. `ui/theme.rs` and `ui/fonts.rs` turn `config.rs`'s preferences into GPUI themes and fonts.

## Demo mode

`--demo` never starts or connects to the engine. The UI loads `assets/workspace.json` through `data::load`, and `ui/runtime_ui.rs` handles actions locally (look for the `if self.demo_mode` branches). A UI feature has to work in both paths. Demo mode also fakes the machines: `machines.rs` keeps Matrix and Vortex in memory, `data::load` puts one project on each machine, and `client::init` shows Vortex disconnected.

## Assets

`build.rs` embeds every file in `assets/` and every theme in `bundled-themes/`. Dropping a file in is enough. `icon("name")` loads `assets/name.svg`. Icons are Phosphor regular SVGs, and each one gets a row in `assets/PHOSPHOR.md` (app name → Phosphor name).

## Tests

- Rust unit tests sit next to the code (`#[cfg(test)]`). The large suites are `storage/tests.rs` and `acp/tests.rs`.
- End-to-end engine and UI checks are in `scripts/engine-check/`, Windows only. See its README.
