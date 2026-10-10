# Architecture

Every file in `src/` opens with a `//!` summary. `grep -m1 '^//!' src/*.rs` gives the module map. This doc shows how the modules connect, which the map can't.

## Two processes

One binary, two roles (`main` in `src/main.rs`):

- UI: `adeline` (or `adeline --demo`). A single GPUI root view, `Adeline`, with its `impl` blocks split across `interaction.rs`, `runtime_ui.rs`, `views.rs`, `chat_render.rs`, `panes.rs`, `project_ui.rs` and `settings.rs`.
- Engine: `adeline engine` (`engine::main`). A background process that owns agent processes, conversation state and storage. It outlives the UI.
- Bridge: `adeline bridge` (`remote::bridge_main`). A remote client runs it over SSH to reach that machine's engine (see Machines).

The default `gui` feature holds the UI. `--no-default-features` builds the headless binary: engine, bridge and `--version` only, without GPUI. Its UI-only modules and items are behind `#[cfg(feature = "gui")]`, and code only the UI calls is allowed to be dead there. Remote installs use the headless build (`adeline-headless-<platform>.zip`). Build it with `--target-dir target/headless` so it doesn't replace the full `adeline` in `target/release`.

They talk over a same-user named pipe on Windows and a Unix socket elsewhere (`ipc.rs`), one JSON object per line. The message types live in `protocol.rs`, and bumping `PROTOCOL` there is the only version change needed. `scripts/engine-check/engine_client.py` reads it from that file.

## A user action, end to end

1. A click, key binding or macOS menu-bar item calls `app.act(Action::…)`. `Action` is in `main.rs`, keys are bound in `config::bind_keys`, and the menu bar is in `menu_bar.rs`.
2. `Adeline::act` (`interaction.rs`) dispatches it. Actions that touch a conversation go to `runtime_ui.rs`.
3. `runtime_ui.rs` sends a `protocol::Command` through `client.rs`, the UI's engine connection.
4. `Engine::command` (`engine.rs`) handles it, persists through `storage.rs`, and sends `acp::Command`s to the conversation's worker.
5. `acp.rs` runs one ACP stdio agent process per conversation. Its events come back to `Engine::driver_event`.
6. The engine broadcasts a `protocol::Delta`, and `Adeline::apply_delta` (`runtime_ui.rs`) updates the UI's model (`data.rs`).

## Agents and ACP

`acp.rs` speaks ACP through the official `agent-client-protocol` SDK, which owns JSON-RPC framing, request IDs and the typed schema. Its `Worker` is a state machine fed by `Input`s (SDK requests and notifications, replies, process exit, `Command`s from the engine) and is unit-tested through a fake `Link` in `acp_tests.rs`. It translates the SDK's types into Adeline's own model in `conversation.rs` (session options by category, TODO steps, tool calls, permission options, features, attachments, MCP servers, traffic entries); nothing outside `acp.rs` sees an SDK type. Every line in and out, and the agent's stderr, is reported as a `TrafficEntry`, which the engine keeps in the transcript and streams to clients watching the traffic tab.

`profiles.rs` knows each supported agent (Claude, Codex, Pi, opencode, OMP): its command, install steps, how it takes system instructions, which modes are plan modes, and how its turns end. `harness.rs` finds installed agents and their versions, and `install.rs` plans and runs installs and opens login terminals. The engine runs those for a client (`PlanInstall`, `Install`, `Login`), so a remote machine installs on itself.

A new conversation feature usually touches every step: `Action`, `act`, `runtime_ui`, `Command`/`Delta` in `protocol.rs`, `Engine::command`, and often `acp.rs` and `storage.rs`.

## Machines

One client shows the projects of every checked machine (`machines.rs`: saved remote machines, checked ones, `machines.yml` on the client only). Each project in the UI carries its `machine`, and `Workspace::key()` (`machine:id`) tells apart same-named projects on two machines. A machine's projects sit together in `Adeline::projects`, so `protocol::apply` runs on that machine's slice.

`client.rs` keeps one `Connection` per checked machine. Every request names its machine: `Adeline::request` uses the open project's, `machine_request` any other. The local engine is reached through `ipc.rs`. A remote one goes through `remote.rs` over the system `ssh`:

1. A probe finds the remote platform, its engine identity (`engine/id`) and the Adeline in `~/.adeline/bin` (its `version` file). Unix hosts answer `sh`; Windows hosts a base64 PowerShell script, which reads the same under cmd and PowerShell.
2. A missing Adeline is installed in the client's exact version: the headless build from its GitHub release or, with no release and the same platform, the client's own executable, sent over `ssh` stdin.
3. `adeline bridge` joins the SSH session to that machine's engine pipe or socket, starting the engine like a local client would. No network port opens anywhere.
4. The client checks the welcome: the saved engine identity, and `PROTOCOL`. The Adeline version doesn't matter, so a 0.1.7 client uses a 0.1.6 engine with the same protocol. A newer protocol asks for a local update. An older one is replaced (step 2's install, when the installed Adeline is older) and its engine restarted, after the user consents.

`ssh` runs this executable as `SSH_ASKPASS` with `ADELINE_ASKPASS` set (`main` checks it first), and the helper hands each prompt to the UI over an owner-only pipe or socket. A password typed during one connect answers the same prompt in that connect's later `ssh` runs and is then forgotten.

The engine numbers every delta it broadcasts and keeps the latest ones. A reconnecting client sends its place (`Hello.resume`) and gets `Resumed` plus only the deltas it missed, or a full snapshot if the engine restarted or the gap is too long. A dropped remote machine keeps its last state on screen, read-only, while `client.rs` reconnects with growing delays.

`ADELINE_SSH` replaces the `ssh` command line; `scripts/engine-check/fake_ssh.py` uses it to simulate remote Windows machines on one computer.

## Demo mode

`--demo` never starts or connects to the engine. The UI loads `assets/workspace.json` through `data::load`, and `runtime_ui.rs` handles actions locally (look for the `if self.demo_mode` branches). A UI feature has to work in both paths. Demo mode also fakes the machines: `machines.rs` keeps Matrix and Vortex in memory, `data::load` puts one project on each machine, and `client::init` shows Vortex disconnected.

## Assets

`build.rs` embeds every file in `assets/` and every theme in `bundled-themes/`. Dropping a file in is enough. `icon("name")` loads `assets/name.svg`. Icons are Phosphor regular SVGs, and each one gets a row in `assets/PHOSPHOR.md` (app name → Phosphor name).

## Tests

- Rust unit tests sit next to the code (`#[cfg(test)]`). Storage tests are in `storage_tests.rs`.
- End-to-end engine and UI checks are in `scripts/engine-check/`, Windows only. See its README.
