# Architecture

Every file in `src/` opens with a `//!` summary. `grep -m1 '^//!' src/*.rs` gives the module map. This doc shows how the modules connect, which the map can't.

## Two processes

One binary, two roles (`main` in `src/main.rs`):

- UI: `adeline` (or `adeline --demo`). A single GPUI root view, `Adeline`, with its `impl` blocks split across `interaction.rs`, `runtime_ui.rs`, `views.rs`, `chat_render.rs`, `panes.rs`, `project_ui.rs` and `settings.rs`.
- Engine: `adeline engine` (`engine::main`). A background process that owns agent processes, conversation state and storage. It outlives the UI.

They talk over a same-user named pipe on Windows and a Unix socket elsewhere (`ipc.rs`), one JSON object per line. The message types live in `protocol.rs`, and bumping `PROTOCOL` there is the only version change needed. `scripts/engine-check/engine_client.py` reads it from that file.

## A user action, end to end

1. A click or key binding calls `app.act(Action::…)`. `Action` is in `main.rs`, and keys are bound in `config::bind_keys`.
2. `Adeline::act` (`interaction.rs`) dispatches it. Actions that touch a conversation go to `runtime_ui.rs`.
3. `runtime_ui.rs` sends a `protocol::Command` through `client.rs`, the UI's engine connection.
4. `Engine::command` (`engine.rs`) handles it, persists through `storage.rs`, and sends `acp::Command`s to the conversation's worker.
5. `acp.rs` runs one ACP stdio agent process per conversation. Its events come back to `Engine::driver_event`.
6. The engine broadcasts a `protocol::Delta`, and `Adeline::apply_delta` (`runtime_ui.rs`) updates the UI's model (`data.rs`).

A new conversation feature usually touches every step: `Action`, `act`, `runtime_ui`, `Command`/`Delta` in `protocol.rs`, `Engine::command`, and often `acp.rs` and `storage.rs`.

## Demo mode

`--demo` never starts or connects to the engine. The UI loads `assets/workspace.json` through `data::load`, and `runtime_ui.rs` handles actions locally (look for the `if self.demo_mode` branches). A UI feature has to work in both paths.

## Assets

`build.rs` embeds every file in `assets/` and every theme in `bundled-themes/`. Dropping a file in is enough. `icon("name")` loads `assets/name.svg`. Icons are Phosphor regular SVGs, and each one gets a row in `assets/PHOSPHOR.md` (app name → Phosphor name).

## Tests

- Rust unit tests sit next to the code (`#[cfg(test)]`). Storage tests are in `storage_tests.rs`.
- End-to-end engine and UI checks are in `scripts/engine-check/`, Windows only. See its README.
