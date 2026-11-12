# Implicit state machines

Date: 2026-11-12. Code at commit `61030e1`.

An implicit state machine is a lifecycle the code tracks without one enum that names its states. The phase sits in several bools, `Option`s, strings or counters, and scattered guards keep them in step.

Three agents read `adeline_engine`, `adeline_ui`, and `adeline_core` with `adeline_docs` and `main.rs`. Nothing was run. A defect marked "confirmed" was checked a second time in the code. A defect marked "suspected" comes from one reading and needs a test.

Paths are relative to `crates/`. Line numbers match the commit above.

## Confirmed defects

| Where | What happens |
| --- | --- |
| `adeline_ui/src/conversation_ui.rs:831` | Escape sets `slash_dismissed`. Nothing resets it, so the slash command list stays hidden until restart. |
| `adeline_ui/src/docs.rs:1429` | The quit path sets `docs.unsaved_ok` and never clears it. If the user then cancels the quit, later unsaved docs get no warning. |
| `adeline_core/src/remote.rs:173` | The "older engine didn't make way" error is unreachable. Round 1 of the loop always returns at line 161, so the user gets the consent prompt again. |
| `adeline_ui/src/conversation_ui.rs:894` | Nothing sets `menu` to `"todo"`, so that popover never opens. |
| `adeline_engine/src/engine.rs:572` | `Live.turn` (`TurnState`) is computed and sent on the wire. Nothing reads it. |
| `adeline_engine/src/acp.rs:804` | `Worker.previous_session` is written and later discarded at `acp/session.rs:439`. Nothing reads it. |

## The most tangled

### 1. Conversation lifecycle

State sits in three places that must agree.

- `Live` in `adeline_core/src/protocol.rs:371`: `running`, `processing`, `shutting_down`, `shutdown_stuck`, `storage_failed`, `replacement`, `recovering_storage`, `auth_required`, `error`, `progress`, `permission`, `turn`, `quiet_since`, `worked`.
- Engine side tables in `adeline_engine/src/engine.rs:179`: `drivers`, `steering`, `send_after_stop`, `permission_stopped`, `text_copy_pending`, `restart_prompt`.
- The saved `ThreadStatus` in `ConversationSettings.status` (`adeline_engine/src/storage.rs:46`).

States: no agent, agent idle, turn running, waiting for permission, retrying, stopping, steer in flight, replacement needed, storage failed, recovering storage, shutting down, restart queued, shutdown stuck.

`progress` holds the retry and stopping phases as display text. `engine.rs:461` derives the status string "retrying" from `progress.starts_with("Retry")`, so a wording change at `engine/events.rs:240` breaks it.

The engine pairs `ThreadStatus` with `Live` by hand, calling `set_runtime_status` next to each write (`engine/turns.rs:252, 260, 313, 449, 470`, `engine/events.rs:252, 296, 314, 328, 336, 340`).

Suspected paths:

- `recovering_storage` stays true when the turn ends by finishing or failing. Only `turn_stopped` (`events.rs:330`) and `shutdown_complete` (`events.rs:364`) clear it. Send and retry then refuse at `turns.rs:55` and `turns.rs:357`.
- `processing` true with an idle worker. `events.rs:244` sets `replacement` while the worker keeps its turn. If the driver shuts down before `ReplaceSession`, the new worker has no turn, and `turns.rs:498` sets `processing` with nothing to end it. Force Stop is the only exit.
- Status `Processing` with `processing` false and no driver. A worker `ForceStop` (`acp.rs:821`) drops the turn without an event, and `shutdown_complete` never touches the status. Turn ends while `closing` is true skip the status write too.
- `send_after_stop` survives if the turn finishes just as the user presses send now. The next plain stop then sends the queue (`events.rs:341`).
- `permission_stopped` is removed only in `turn_stopped` (`events.rs:321`). If that turn ends by error or Force Stop, the next ordinary stop is reported as a permission stop.
- `shutdown_stuck` true with `shutting_down` false. The worker emits `ShutdownStuck` for an agent that stopped talking while not closing (`acp/failure.rs:179`).
- `shutting_down` true with no driver after a `StopAll` without exit (`engine.rs:920`). A late `ShutdownComplete` is dropped at `events.rs:35`.
- `progress` keeps "Retry N of M" for the whole retried turn.

### 2. ACP `Worker`

`docs/architecture.md` calls the worker a state machine fed by `Input`s. No enum names its phases. About 29 fields on `Worker` (`adeline_engine/src/acp.rs:476`) and 6 on `Turn` (`acp.rs:442`) hold them.

Process, link, handshake and session setup use `child`, `link`, `connecting`, `epoch`, `pending`, `handshake`, `features`, `setup`, `configured`, `config_step`, `restore_required`, `replacement`, `pending_context`, `replacing`, `probing` and `auth`.

States: no process, connecting, connected but not initialized, initialized, session setup, applying saved options, ready, stopped talking, restore refused, replacing, auth queued.

`advance` (`acp.rs:840`) picks the next step from `child`, `link`, `connecting`, `configured`, `pending.is_empty()`, `setup` and `features`.

Oddities:

- `setup` only ever holds `Some(Request::Setup)`. It is a bool.
- `pending.is_empty()` at `acp.rs:850` stands for "no setup step in flight", but the same map holds Steer, Switch, Close, Authenticate and Logout.
- `handshake` is one slot. A second tracked request overwrites the first one's deadline (`acp.rs:865`).
- `close_link` (`acp/session.rs:527`) leaves `configured` true with no link.
- Suspected: `replacing` stays set after a failed replacement. Only `acp/options.rs:111` clears it. The next prompt's text is then overwritten with the saved context (`options.rs:114`).
- `closing && !self.probing` appears at `acp.rs:618, 899` and `session.rs:408, 455`. `shutdown` clears `probing` first, so `!probing` adds nothing.
- `acp.rs:922` restarts setup after auth when a turn is active. Only login drivers get Authenticate, and they hold no turn, so the branch looks unreachable.

### 3. Client `Connection`

`adeline_ui/src/client.rs:86`. `State` (`client.rs:25`) names 13 states, but the phase also needs `loaded`, `sender`, `inbox`, `queued`, `epoch`, `bye`, `stream`, `cancel`, `retries` and `upgrade`.

Phases without a name: linked and waiting for the welcome (`Connecting` with `sender` set), first load or reconnect (split by `loaded`), closed on purpose or lost (split by `bye`), waiting out a retry delay (`Disconnected` with `retries > 0`).

Suspected paths:

- `Connected` with `sender` None. `init` sets `Connected` (`client.rs:299`) before `Incoming::Ready` arrives. Requests sent then go to `queued`, and only a snapshot or resume flushes it. `WatchTraffic` and `MarkRead` from `open_last_active_chat` may wait until the next reconnect.
- `Mismatch` or `Waiting` with `sender` None. `replace_old` sends Shutdown only if a sender exists (`client.rs:906`).
- `Connecting` with no task, when `machines::remote` returns None after `open_inbox` (`client.rs:462`).
- A local `Mismatch`, `Waiting` or `Unavailable` has no screen when two or more machines are checked (`runtime_ui/screens.rs:8, 179`).

### 4. Root modal

`adeline_ui/src/lib.rs:463`. `modal: Option<&'static str>` takes 15 values: "about", "add-project", "settings", "rename-project", "folder-machine", "browser", "delete-project", "delete-project-shutdown", "upgrade", "prompt", "quit", "doc-new", "doc-rename", "doc-delete", "doc-unsaved".

Each value needs its own sibling field: `browser`, `upgrade_machine`, `delete_project`, `rename_project`, `docs.target` or `docs.then`. Whether the dialog is on screen is a second hidden variable. `act` opens or closes it by comparing `modal` before and after (`interaction.rs:111, 131`), and code outside `act` must do so itself.

Paths found by reading:

- `modal` set with no dialog on screen. The 6 second delete timer (`project_bar/menu.rs:337`) sets "delete-project-shutdown" outside `act`. While it holds, `show_prompt` returns early (`runtime_ui.rs:787`), so an SSH password prompt is queued and hidden.
- Any project removal clears any modal. `remove_project_at` (`project_ui.rs:400`) resets `modal`, `selected` and `section` for a `ProjectRemoved` from any machine.
- A new modal string overwrites an open one without clearing the old sibling field (`docs.rs:1469`, `Action::About`, `MachineUpgrade`).
- Suspected: "browser" with `browser` None after `browse_pick` returns early (`project_ui.rs:569`).

### 5. One doc's buffer and disk state

`adeline_ui/src/docs.rs:160`. Fields: `saved`, `text`, `dirty`, `modified`, `read_only`, `editor`, `conflict`, `gone`, plus `Docs.shown`, `Project.open` and `Project.failed`.

States: not open, opening, open failed, shown clean, shown dirty, stashed dirty, conflict, gone, read only. `modified == 0` marks a placeholder that has not loaded, and disk values are forced to `.max(1)` to keep the marker free.

Paths found by reading:

- `conflict` set on a clean doc. A failed reload fills `conflict` from `saved` (`docs.rs:961`), and `docs_discard` keeps it. The banner then claims unsaved edits.
- `docs_close_open` removes the doc without checking `dirty` (`docs.rs:1607`). The "gone" banner's Close button reaches it, where the text exists only in memory.
- Suspected: delete clears `shown` by path alone (`docs.rs:1355`), so a reply after a project switch can clear the other project's doc.
- Suspected: `pending_anchor` survives a failed open and applies to the next doc.

## Engine

### One turn, with retry and backoff

`Turn` in `adeline_engine/src/acp.rs:442`: `retries`, `attempt`, `worked`, `observed_text`, `denied`, `cancelled`, `prompt_request`, `retry_at`. With `Worker.active` and `Worker.turn`.

States: idle, waiting for the session, prompt in flight, backoff, cancelling, parked for replacement. `denied` sits on top.

`acp/turns.rs:353` sets `retries = attempt` to mean "do not retry". `lost` does not clear `prompt_request`, so a cancelled turn stays in cancelling if the process never exits.

### Worker shutdown and process exit

`acp.rs:504`: `closing`, `shutdown_deadline`, `stuck_reported`, `timeout_reported`, `lost_pending`, `lost_at`, `lost_during_initialize`, `forced`.

States: running, closing and waiting for the cancelled turn, close request sent, link dropped, lost and waiting for exit, forced, stuck, done.

`shutdown_deadline` is also armed while not closing, so `ShutdownStuck` really means "the process should have exited". `lost_at` is never cleared. If the kill fails, `closing` is true and `forced` is false, and only another `ForceStop` gets out (`acp.rs:829`).

### Fork handoff

Split between the worker (`fork`, `forking` at `acp.rs:517`), the engine (`text_copy_pending` at `engine.rs:202`) and storage (`forked_from` with `session_id: None`).

States: fork without a session, native fork in flight, fell back to a text copy, text copy sent and unconfirmed, done.

Suspected: `turn_stopped` does not clear `text_copy_pending`. Stop the first turn of a text copy fork, and a later failed turn drops a session the agent already used.

### Save state of one conversation

`adeline_engine/src/storage.rs:105`: `unsaved_events`, `storage_error`, `unsaved_since`, `persisted_len`, `needs_rollback`, `pending_settings`, `load_error`, `interrupted`.

States: clean, queued, event write failed, settings write failed, torn tail from load, unreadable transcript, interrupted. Load rewrites `Processing` to `Blocked` in memory (`storage/disk.rs:173`). No broken path found.

### Stop all and idle exit

`engine.rs:167`: `StopAll { deadline, forced, waiters, stopped, exit }`, `stop_all: Option<StopAll>`, `idle_since`, `exit`.

States: not stopping, graceful, killing, finished. Guards on `stop_all.is_some()` sit at `turns.rs:22, 333` and `events.rs:44, 372, 376`.

### Smaller ones

- Project deletion: a key in `deleting` (`engine.rs:191`) means "waiting for this project's agents to stop". Cancel does not undo shutdowns already sent.
- Client connection: `Client { sender, writer, subscribed }` (`engine.rs:148`). A second `Hello` is not refused.
- Debounce timers: `watch_due` and `docs_due` (`engine.rs:205, 211`).
- Docs watcher: `docs_watcher` exists only while `docs_watching` is not empty. A folder whose `watch` call failed is still recorded as watched (`engine/docs.rs:251`).

### Counters that drop stale results

- Worker `epoch` (`acp.rs:487`), carried by each `Input`.
- `Slot.turn` with `Event.turn` (`engine.rs:162`, `events.rs:38`). Each new worker restarts at 0, so it cannot tell two workers of one conversation apart.
- `PendingPermission.turn` (`acp/permissions.rs:78`).
- Request ids in `pending` (`acp.rs:875`).
- Delta `epoch`, `seq` and `recent` (`engine.rs:216`), used by resume.
- File reloads use time in place of a counter: `touched` and a 1 second window (`engine/projects.rs:430`).

## UI

### Quit and close

Spread over `runtime.quitting`, `runtime.stopping_all` (`runtime_ui.rs:25`), `docs.then`, `docs.unsaved_ok` (`docs.rs:198`), the modals "quit" and "doc-unsaved", and the `pending` flags of the Settings and agent windows.

States: idle, asking about unsaved docs, unsaved cleared, asking about agents, stopping, quitting.

Besides the confirmed `unsaved_ok` defect:

- `quitting` is never reset. If `settings::can_close` blocks the close after "Finish in background", the busy check is skipped next time.
- `engine_stopped` for one machine clears `stopping_all` while other machines' replies are still counted (`runtime_ui.rs:943`).
- The "doc-unsaved" dialog for one project lists every project's unsaved docs. Discard clears only the closing project, and Save all saves all.

### Deleting a project

`pending_removal`, `removal_generation`, `delete_project`, `project_error` (`lib.rs:470`), plus the modals "delete-project" and "delete-project-shutdown".

States: idle, undo window, confirm, waiting for agents, failed, gone. `project_error` is shared with add, rename and settings. Demo mode skips the confirm states.

### Settings window

`adeline_ui/src/settings.rs:533`. One `pending` bool covers five phases: the save or discard dialog, the "changed outside" dialog, a save in flight, the delete dialog, and a delete in flight. Six fields together pick the page: `machines_page`, `agent_page`, `agent_form`, `group`, `subgroup`, `search_page`.

- `agent_page` set with `agent_form` None after `AfterAgent::CloseOwner` (`settings.rs:1097`). The page says "Select an agent" while the row stays selected.
- Suspected: while a save is in flight, `leave` returns early, and `can_close` blocks the main window too. A reply that never comes locks both.
- External change is tracked twice: `external_changed` and a fresh comparison with the catalog.

### Agent form

`adeline_ui/src/agent_form.rs:181`: `probing`, `offered`, `probe`, `probe_generation`, `install`, `login`.

- `Probing` is an enum, but `offered` keeps a second copy of the result that outlives it.
- `install: Option<Install>` has no request id. Picking another harness during a plan or install lets the old reply land under the new harness (`agent_form/installer.rs:17, 51`).
- Login status is a string in `login`. Nothing blocks a second click while one runs.

### Menus

`menu: Option<&'static str>` and `command_popup` (`lib.rs:494`). Names: "app", "mode-settings", "machines", "projects", "agents", "agent", "files", "chats", "model", "effort", "mode", "more", "todo".

`command_popup` is never cleared, and `refocus_after_menu` reads it (`views.rs:281`). A menu and a modal can both be set.

### Smaller ones

- Slash command list: `slash` and `slash_dismissed` (`lib.rs:500`). See the confirmed defects.
- Doc images: a missing key, a key in `wanted`, `Some(None)` and `Some(Some)` each mean a phase (`docs/viewer.rs:30`). There is no "requested" state, so each redraw before the reply sends another `ReadImage`.
- Mermaid cache (`docs/viewer.rs:377`): clearing the map at 64 entries drops the markers of diagrams still rendering.
- Chat list search: `generation`, `pending`, `searching` (`chat/list.rs:433`). `toggle_fold` clears `searching` while a search still runs.
- Remote folder browser: `listing` and `error` (`project_ui.rs:15`), with no request id. Two quick clicks can show the older listing.
- Docs tree listing: `listed` and `list_error` (`docs.rs:176`).
- Open project and chat: `project` and `selected` index three parallel vectors (`lib.rs:454, 479`). Every insert and remove must touch all three. No broken path found.
- Glide animation (`panes.rs:644`), the rail pill slide (`lib.rs:508`), `McpEditor.http` (`mcp_ui.rs:18`), and the chat search bar.

## Core

### SSH connect handshake

`adeline_core/src/remote.rs:78` (`connect`) and `:111` (`attempt`). The phase sits in local variables: `sign_in`, `network`, `probe`, and a two round loop. `Machine.engine` None means "never paired".

Phases: probe as Unix, probe as Windows, check the platform, check the engine ID, install, bridge and wait for the welcome, check the ID again, compare protocols, then install, shut the old engine down and loop once.

- See the confirmed defect at line 173.
- Suspected: `bridge` waits for `Welcome` with no time limit (`remote.rs:628`).
- `probe.version` means both "what is on disk" and "what I just installed" (line 139).
- `Unsupported`, `LocalUpdate` and `Upgrade` end the loop over destinations at once (line 98).

### Model types

- Streaming cursor: `Live.assistant` (`protocol.rs:396`) is an index into `thread.messages`, held in another struct. `Delta::Thread` replaces the thread without resetting it. Each message's `thought_started` and `thought_ended` use 0 for unset.
- Delta stream position: `Resume { epoch, seq }` (`protocol.rs:308`). Both sides must count the same messages. `Hello { cli: true, resume: Some(..) }` is allowed and ignored.
- Thread search cache: `unread_messages`, `search_text`, `search_stale` (`data.rs:35`). An empty `search_text` means "not prepared".
- `Activity` (`data.rs:141`): `running` repeats `status`, the title carries the status as a suffix that `name()` strips again, `kind` is a string tag, and `finished == 0` means "not finished".
- `SearchRecord` (`prepared.rs:34`) flattens `ThreadStatus` into four bools and picks a group by priority order.
- Turn timing: `turns`, `streamed_chars`, `streamed_ms`, `last_text` (`data.rs:60`). No call ends a turn, so the last one stays open.
- `Decision { selected, resolved }` (`data.rs:176`) and `Attachment { data, path }` (`conversation.rs:364`), where an empty file and an oversized one differ only by `size`.

### Stores and helpers

- Agent catalog (`agents.rs:125`): `demo` and `root: Result` give three modes, and every method branches on them. If `write_moved` fails to remove the old folder, the agent appears twice (`agents.rs:314`).
- Machines store (`machines.rs:29`): `STORE: Mutex<Option<Store>>`, `DEMO`, `unreadable`. `set_last_folder` changes memory first and ignores a failed save.
- Password reuse: a `SESSION` counter per ssh process (`remote.rs:76`). The prompt kind comes from ssh's English wording (`askpass.rs:21`).
- Harness catalog (`harness.rs:69`): `detected` plus the `installed` map give unknown, installed and not installed.
- Settings load state (`config.rs:259`): thread local, so another thread sees defaults.
- Engine start: connect, else spawn, then poll every 50 ms for 10 s (`ipc.rs:224`).
- Log file (`log.rs:19`): if the file cannot open, lines are dropped and reported as written.
- Unix child containment (`platform.rs:91`): `record_group` records nothing if `contain_children` has not run.

### Checked and clean

`adeline_docs` (`format.rs`, `files.rs`, `text.rs`, `tree.rs`) and `adeline/src/main.rs` keep no hidden phase. Their scanners are fixed ladders of tests or loops with no state between items.

## Explicit state machines seen

These are enums with named states, listed so the sweep's coverage is clear.

- Core: `ThreadStatus`, `ToolStatus`, `StepStatus`, `TurnState`, `StopReason`, `remote::Failure`, `prepared::Generation`, `prepared::Group`, `recency::Period`, `install::Target`, and the wire messages in `protocol.rs`.
- Engine: `acp::Input`, `acp::Command`, `acp::EventKind`, `Request`, `FailureKind`, `engine::Input`. These carry events and results, and hold no phase.
- UI: `client::State`, `client::Incoming`, `agent_form::Probing`, `agent_form::Install`, `settings::AfterAgent`, `docs::Then`, `docs::After`, `project_ui::BrowserPurpose`, `panes::SideTab`, `Action` and `Area`.

## Not read in full

In the UI crate: `theme.rs`, `fonts.rs`, `activity.rs`, `control.rs`, `titlebar.rs`, `menu_bar.rs`, `chat.rs`, most of `conversation_ui.rs` and `docs/view.rs`, and the drawing halves of `settings/*.rs`. Their struct definitions were read and their state fields searched.
