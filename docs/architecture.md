# Architecture

Every source file opens with a `//!` summary, and `grep -rm1 '^//!' crates` prints the module map. This doc covers what the map can't show: how the modules connect. Paths below are relative to a crate's `src/`.

## One binary, three roles

`main.rs` (`crates/adeline`) picks the role:

- UI: `adeline` (or `adeline --demo`) opens the window (`adeline_ui::open`).
- Engine: `adeline engine` (`engine::main`) runs a background process that owns agent processes, conversation state and storage. One engine runs per OS user, and it outlives the UI.
- Bridge: `adeline bridge` (`remote::bridge_main`). A remote client runs it over SSH to reach that machine's engine (see Machines).

`ssh` also runs the binary as its `SSH_ASKPASS` helper, and `main` checks for that first (see Machines).

The default `gui` feature builds the full app. `--no-default-features` builds the headless binary: engine, bridge and `--version` only, with no GPUI. Remote installs use it (`adeline-headless-<platform>.zip`). Build it with `--target-dir target/headless` so it doesn't replace the full `adeline` in `target/release`.

UI and engine talk over a same-user named pipe on Windows and a Unix socket elsewhere (`ipc.rs`), one JSON object per line. The message types live in `protocol.rs`, and bumping `PROTOCOL` there is the only version change needed. `scripts/engine-check/engine_client.py` reads it from that file.

## Source layout

The code is a Cargo workspace. Each crate is in `crates/`, from the bottom up:

- `adeline_assets`: every file in `assets/` and every theme in `bundled-themes/`, embedded (see Assets).
- `adeline_core`: everything the engine and the UI share. No GPUI.
- `adeline_engine`: the engine, with `acp` and `storage`. Only the binary depends on it.
- `adeline_ui`: the window and everything in it. Only the binary depends on it.
- `adeline`: the binary, just `main.rs`. Its `gui` feature adds `adeline_ui`. Apart from `main` choosing what to run and the engine reporting which build it is, that is the only place the code checks the feature.

The engine and the UI don't import each other, and the core imports neither. Keep it that way: a type both need belongs in the core. Cargo enforces it, so the headless build stays free of GPUI. An edit in one crate rebuilds that crate and the crates above it, not the ones beside it: a UI edit leaves the engine alone, and the other way round.

The core, from the bottom up:

- Helpers that import nothing from Adeline: `files.rs` (atomic writes, safe folder names), `platform.rs` (process containment and spawning), `recency.rs`, `log.rs`. `config.rs` (preferences, `settings.yml`) sits just above them.
- The model: `conversation.rs` (Adeline's own conversation model; no ACP types), `data.rs` (projects, chats and messages as clients see them, plus the demo workspace), `prepared.rs` (chat search), and `protocol.rs` (engine ↔ client messages). `data` and `protocol` still import each other: the demo's live state is a `protocol::Live`.
- Agents: `profiles.rs` (what Adeline knows about each supported agent), `harness.rs` (installed agents and their versions), `install.rs` and `agents.rs` (the user's agent definitions).
- Machines: `machines.rs` (saved machines and destination parsing), `remote.rs` (SSH transport) and `askpass.rs`.

The engine crate adds `acp.rs` (agent processes), `storage.rs` (persistence) and `engine.rs`, which uses all of the above.

A large module becomes a directory: `foo.rs` keeps the types, the state and the dispatch, and each child in `foo/` adds an `impl` block or helpers for one concern. Children import from the parent with `use super::…`, and items they share with the parent or each other are `pub(super)`. Big test suites go in `foo/tests.rs`. `engine`, `acp`, `storage`, and the UI's `chat`, `settings`, `agent_form`, `project_bar` and `runtime_ui` follow this pattern.

## A user action, end to end

1. A click, key binding or macOS menu-bar item calls `app.act(Action::…)`. `Action` is in `adeline_ui`'s `lib.rs`, keys are bound in `bind_keys`, and the menu bar is in `menu_bar.rs`.
2. `Adeline::act` (`interaction.rs`) dispatches it. Actions that touch a conversation go to `runtime_ui.rs`.
3. `runtime_ui.rs` sends a `protocol::Command` through `client.rs`, the UI's engine connection.
4. `Engine::command` (`engine.rs`) handles it, persists through `storage.rs`, and sends `acp::Command`s to the conversation's worker.
5. `acp.rs` runs one ACP stdio agent process per conversation. Its events come back to `Engine::driver_event`.
6. The engine broadcasts a `protocol::Delta`, and `Adeline::apply_delta` (`runtime_ui.rs`) updates the UI's model (`data.rs`) and notifies the views that show the change (see The UI below).

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

One client shows the projects of every checked machine. `machines.rs` keeps the saved remote machines and which ones are checked, in `machines.yml` on the client only. Each project in the UI carries its `machine`, and `Workspace::key()` (`machine:id`) tells apart same-named projects on two machines. A machine's projects sit together in `Adeline::projects`, so `protocol::apply` runs on that machine's slice (`runtime_ui/machine_sync.rs`).

`client.rs` keeps one `Connection` per checked machine. Every request names its machine: `Adeline::request` uses the open project's machine, and `machine_request` any other. The local engine is reached through `ipc.rs`. A remote one goes through `remote.rs` over the system `ssh`:

1. A probe finds the remote platform, its engine identity (`engine/id`) and the Adeline in `~/.adeline/bin` (its `version` file). Unix hosts answer `sh`. Windows hosts get a base64 PowerShell script, which reads the same under cmd and PowerShell.
2. A missing Adeline is installed in the client's exact version. That's the headless build from its GitHub release, or, if there is no release and the platform matches, the client's own executable sent over `ssh` stdin.
3. `adeline bridge` joins the SSH session to that machine's engine pipe or socket, starting the engine like a local client would. No network port opens anywhere.
4. The client checks the welcome message: the saved engine identity, and `PROTOCOL`. The Adeline version doesn't matter, so a 0.1.7 client uses a 0.1.6 engine with the same protocol. A newer remote protocol asks for a local update. An older one gets replaced, after the user consents: step 2's install runs (when the installed Adeline is older) and the engine restarts.

`ssh` runs this executable as `SSH_ASKPASS` with `ADELINE_ASKPASS` set. The helper in `askpass.rs` hands each prompt to the UI over an owner-only pipe or socket. A password typed during one connect answers the same prompt in that connect's later `ssh` runs, and is then forgotten.

The engine numbers every delta it broadcasts and keeps the latest ones. A reconnecting client sends its place (`Hello.resume`). It gets `Resumed` plus only the deltas it missed, or a full snapshot if the engine restarted or the gap is too long. A dropped remote machine keeps its last state on screen, read-only, while `client.rs` reconnects with growing delays. `runtime_ui/screens.rs` draws what shows while an engine isn't usable.

`ADELINE_SSH` replaces the `ssh` command line. `scripts/engine-check/fake_ssh.py` uses it to simulate remote Windows machines on one computer.

## The UI

The UI crate's `lib.rs` holds the root view `Adeline`, the `Action` enum, shared drawing helpers (`row`, `col`, `text`, `icon`) and `open`, which builds the window. `Adeline`'s `impl` blocks are spread over its modules, one concern each: `interaction.rs` (dispatch), `runtime_ui.rs` (conversations, mirrored from the engine), `views.rs`, `panes.rs`, `project_bar.rs`, `project_ui.rs`, `conversation_ui.rs` and `settings.rs`.

Each region of the main window is its own GPUI view: the chat list, the transcript, the composer, the chat header, the side panel and the project tabs. GPUI redraws a notified view and every view above it. Most regions are drawn cached through `chat::region`: GPUI reuses their last layout and paint until they are notified, so a keystroke in the composer doesn't redraw the side panel. A cached view that redraws also redraws every view inside it, so the chat list and the transcript, whose rows animate, are drawn uncached and cache each row instead. The regions read `Adeline`'s state, so `Adeline::notify_cached_views` notifies them whenever `Adeline` is notified. A change that `Adeline` doesn't notify for has to notify the region that shows it.

Two GPUI limits shape this. GPUI leaves a replayed view out of the accessibility tree, so while assistive technology is connected nothing is cached. And it doesn't replay a cached view's window control areas, so the title bar is drawn uncached around its cached project tabs.

`chat.rs` declares the chat regions and holds the drawing helpers other surfaces share. Each region lives in a child: `chat/list.rs` (the chat list and sidebar), `chat/transcript.rs` and `chat/composer.rs`.

The Settings and agent windows are separate windows (`settings.rs`), one child per page. The agent form they share is in `agent_form.rs`.

`client.rs` holds the engine connections as a GPUI global. `ui_state.rs` keeps what this UI remembers for itself and doesn't share with other clients. `theme.rs` and `fonts.rs` turn `config.rs`'s preferences into GPUI themes and fonts.

## Demo mode

`--demo` never starts or connects to the engine. The UI loads `assets/workspace.json` through `data::load`, and `runtime_ui.rs` handles actions locally (look for the `if self.demo_mode` branches). A UI feature has to work in both paths. Demo mode also fakes the machines: `machines.rs` keeps Matrix and Vortex in memory, `data::load` puts one project on each machine, and `client::init` shows Vortex disconnected.

## Assets

`adeline_assets`'s `build.rs` embeds every file in `assets/` and every theme in `bundled-themes/`, and `adeline_assets::embedded` returns a file's bytes. Dropping a file in is enough. The binary's `build.rs` makes the app icon from `assets/adeline.close.up.svg`. `icon("name")` loads `assets/name.svg`. Icons are Phosphor regular SVGs, and each one gets a row in `assets/PHOSPHOR.md` (app name → Phosphor name).

## Tests

- Rust unit tests sit next to the code (`#[cfg(test)]`). The large suites are `storage/tests.rs` and `acp/tests.rs`.
- End-to-end engine and UI checks are in `scripts/engine-check/`, Windows only. See its README.
