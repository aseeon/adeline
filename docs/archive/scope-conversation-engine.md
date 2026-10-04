Status: Implemented in v0.1.0. This scope is history. Where it and the code differ, the code is right.

# Scope: Conversation engine

## Purpose and context

Adeline currently runs every agent inside the UI process. Quitting Adeline cancels any running turn, so a long turn is lost and has to be retried after relaunch. This change moves session handling into a separate background process, the **conversation engine**. The UI becomes a client of it, and work can continue after the UI exits.

Reasons for the change:

- **Finish work in the background.** For example, you quit Adeline while a long turn is running (to update it, or to free the screen). You choose "Finish in background", the turn completes, and the reply is there when you reopen Adeline.
- **Remote machines later.** One Adeline instance will need to control several machines. Separating the engine from the UI now should make a relay between machines easier to build.
- **Group chats later.** The protocol isn't decided yet (it may be a light IRC-style server that both UIs and agents connect to). Separate session handling makes that easier too.
- **Lighter concurrency.** Each conversation currently uses three OS threads. These become lightweight tasks.

Current behavior, from the repo:

- `src/runtime_ui.rs`, `Runtime` and `LiveConversation`: the UI entity owns `conversations: HashMap<id, LiveConversation>`. Each live conversation holds an `acp::Driver`. `ensure_driver` creates a driver on the first Send (`start_prompt`) or on `replace_session`.
- `src/acp.rs`, `Driver::spawn` and `Worker::start`: each driver starts one OS thread for the worker (a blocking `mpsc` loop with a 100 ms poll), plus one thread each for the agent's stdout and stderr. That's 3 OS threads and 1 process per conversation.
- `src/harness.rs`, `probe`: a harness probe starts two more OS threads.
- `src/runtime_ui.rs`, `request_runtime_exit`, and `src/acp.rs`, `Worker::shutdown`: on quit, every running turn is cancelled. Adeline then waits about 5 s and shows the "Stopping agents" modal (`src/project_ui.rs`, the `"shutdown"` modal) with an explicit Force stop button.
- `src/runtime_ui.rs`, the `record` closure in `ensure_driver`: drivers write transcripts through `ProjectStore`, which lives in the UI process behind `Arc<Mutex>`.
- `src/config.rs`, `directory`: data lives under `~/.config/adeline/`, with `settings.yml`, `projects/` (`src/storage.rs`, `ProjectStore`) and `agents/` (`src/agents.rs`, `AgentCatalog`). `modes.chats.retry_limit` is currently a UI setting (`src/config.rs`, `Chats`).
- `src/main.rs`: the UI watches `agents/` with `notify`.
- `docs/archive/scope-acp-agent-driver.md`: R20 and R21 currently define exit as a graceful stop of every running agent, with force used only by explicit choice.

Comparable tools, researched during scoping:

- Zeron runs a separate tokio engine that the UI reaches over RPC. It keeps one process per chat run and reaps idle runs.
- Zed shares one process per agent and counts leases.
- Ghostex runs CLI agents in persistent PTY sessions under a daemon.

## Requirements

### Engine process and lifecycle

- **R1.** The conversation engine runs as its own background process. The Adeline UI never runs agents itself: on launch it connects to a running engine, or starts one and then connects.
- **R2.** At most one engine runs per OS user. If several starts happen at the same moment (two UIs, or a UI and `adeline engine start`), they all end up on one engine. The engine that loses the race exits quietly, and its client connects to the winner.
- **R3.** The engine ships inside the same `adeline.exe`, started with the `engine` subcommand. No separate engine artifact is shipped.
- **R4.** Command-line control:
  - `adeline engine status` prints whether the engine is running, its PID, version, protocol version, daemon mode, uptime, number of connected clients, and every active conversation (project, title, and whether it's processing, retrying, or waiting for permission). It exits 0 when the engine is running and 1 when it isn't.
  - `adeline engine start [--daemon]` starts the engine in the background and returns. If an engine is already running, it says so and exits 0. `--daemon` forces daemon mode for that engine.
  - `adeline engine stop` runs the Stop-all flow (R8) without prompting, even when clients are connected. It prints what it stopped and returns once the engine has exited.
- **R5.** Outside daemon mode, the engine exits on its own after 60 seconds with no active conversations and no connected clients. A client connecting during those 60 seconds cancels the countdown. A conversation is **active** while a turn is processing, a retry is pending, or a permission request is waiting with a client connected.
- **R6.** In daemon mode the engine never exits on its own. Two things turn daemon mode on: the engine setting "Keep conversation engine running", or `--daemon`. In daemon mode, idle agent processes also stay alive when no client is connected.

### Quitting the UI

- **R7.** When the last connected client quits while any conversation is active, the UI shows a dialog with **Stop all**, **Finish in background** and **Cancel**. Cancel keeps Adeline open. If nothing is active, the UI quits with no dialog. When other clients are still connected, closing a UI only disconnects it: no dialog appears and nothing stops. Stop all is also available as a menu action at any time. Idle agent processes are stopped whichever of Stop all or Finish in background is chosen (outside daemon mode, R6).
- **R8.** **Stop all** cancels every running turn and every pending retry, then asks each agent to close gracefully. Any agent still running after 5 seconds is killed automatically, with no second confirmation. The dialog closes only after every agent process has actually exited. `adeline engine stop` and Settings › Engine › Stop engine use the same flow. For application exit, this replaces the exit behavior in `docs/archive/scope-acp-agent-driver.md` R20 and R21.
- **R9.** **Finish in background** lets the UI exit while the engine keeps every active turn running, including pending automatic retries up to the retry limit. Running out of retries ends a turn the same way it does today, with the error recorded. When a turn finishes, its agent process closes gracefully (outside daemon mode). Once nothing is active and no client is connected, R5 applies.
- **R10.** If a turn needs a permission while no client is connected, the engine stops that turn immediately and records it as interrupted, waiting for permission. That covers a request already waiting when the last client disconnected, and a request that arrives while none is connected. Daemon mode follows the same rule. After a client reconnects, Retry continues the turn.
- **R11.** If Adeline is reopened while the engine is still running background work, the UI connects to that engine. Running conversations keep streaming live, and Stop, sending, answering permissions and every other conversation action work as usual. A connected client cancels the auto-shutdown countdown (R5).

### Multiple clients

- **R12.** Several clients can connect to the engine at once. Every client sees the same live state, and every change, including changes made by other clients, reaches all of them.
- **R13.** The engine handles each conversation's commands strictly in the order they arrive. A Send that arrives while that conversation is processing is rejected with a visible message: "This conversation is already processing (sent from another window)". The rejected client's composer text is kept.
- **R14.** Each command is checked against the conversation's state at the moment it arrives. A command that no longer applies is rejected with a reason, for example "Already completed" or "Permission already answered". The first answer to a permission request wins, and other clients see it resolved. Repeating Stop is harmless. Project-level actions, such as deleting a project, are ordered the same way against that project's conversations. No command ever waits on another client's input, and concurrent commands must never deadlock the engine or leave a conversation inconsistent.

### Data and settings ownership

- **R15.** The engine is the only process that reads or writes projects, conversations, transcripts, agent definitions and engine settings. The UI keeps every feature it has now, including creating, editing and deleting agents and projects, but it does all of this by sending requests to the engine. The engine broadcasts the results to every client.
- **R16.** Storage paths stay the same: `~/.config/adeline/projects/` and `~/.config/adeline/agents/`. The engine's own files go in `~/.config/adeline/engine/`, as `settings.yml` and `logs/`.
- **R17.** UI-local state stays in the UI and isn't shared between clients: theme, window layout, open tabs, composer drafts, project sort order, and when each project was last opened. Each UI records when it last opened each project, keyed by project ID. On first start, the existing `opened_at` values from `project.yml` are copied into UI-local state.
- **R18.** Engine settings live in `engine/settings.yml` and take effect immediately, including for work already running in the background. On first start, the engine copies `modes.chats.retry_limit` from the UI's `settings.yml` into its own settings. After that, the UI no longer reads or writes that key.
- **R19.** The main Settings window gets an **Engine** group. It contains every engine setting (Keep conversation engine running, and the retry limit, which moves there from Chats), the same status fields as `adeline engine status`, and Stop engine or Start engine. Chats keeps only UI preferences.
- **R20.** The engine watches its data folders. Agent definitions and project files changed outside Adeline, by hand or by an agent, reach every connected client live.
- **R21.** The engine handles harness detection, harness probes, and registry and icon downloads. Clients request these from the engine and receive the results from it.
- **R22.** Every requirement in `docs/archive/scope-acp-agent-driver.md` still applies and is now enforced by the engine. That includes transcripts (R12 there), one process and session per conversation (R15), Stop (R18), permissions (R22–R24) and retries (R26–R27). Only the application-exit behavior in R20 and R21 is replaced, by R7–R9 here. For Complete, Archive and project deletion, R21 there still holds: graceful first, then an **explicit** Force stop.

### Connection and compatibility

- **R23.** Clients connect locally only: through a named pipe on Windows, or a Unix socket elsewhere. Only the same OS user can connect. The protocol must not assume the client runs on the same machine: no shared file handles, and no local paths used as identity.
- **R24.** Client and engine agree on a protocol version when they connect. If the versions don't match, the UI says "An older engine is finishing N conversations" and offers **Wait** or **Stop them now**. Wait means the UI connects once the old engine has exited on its own; Stop them now runs R8. When the old engine is in daemon mode, the UI offers **Restart engine**, but only while that engine has no active conversations.

### Concurrency model

- **R25.** No OS thread belongs to a single conversation or probe. Each conversation's ACP worker, stdout reader and stderr reader run as three tokio tasks. The harness probe and its reader also run as tokio tasks. Blocking work, such as file I/O, waiting on a process, or running `curl`, goes to a bounded blocking pool (`spawn_blocking`). The UI keeps using GPUI's executors for its own work.

### Operations and failure handling

- **R26.** The engine writes logs to `~/.config/adeline/engine/logs/`, with a size cap and rotation.
- **R27.** If the engine crashes or is killed, every agent process it started dies with it (through a Windows job object, or a process group elsewhere), so nothing is left running. Connected UIs show "Conversation engine stopped unexpectedly", and the next action that needs the engine starts it again. Turns that were running show as interrupted, with Retry.
- **R28.** If the engine is stopped on purpose while a UI is open (`adeline engine stop`, or Stop engine in Settings), the UI shows "Conversation engine stopped" with a **Start engine** button. The UI doesn't restart the engine on its own, but the next action that needs the engine, such as Send or opening Settings › Agents, starts it.
- **R29.** If the engine can't start, or the UI can't connect to it, the UI shows a full-window "Conversation engine unavailable" state with the reason, the log path and **Retry**. Settings stays available for UI-only preferences, and nothing is written to the engine's files.
- **R30.** When an engine is already running, the project list appears without any visible delay compared with today. On a cold start, the UI shows "Starting conversation engine…", and projects appear within 2 seconds on a typical machine.
- **R31.** Demo mode never starts or connects to the engine.

## Boundaries

### Out of scope

- A system tray icon for the engine. Excluded for now; R4's command-line subcommands take its place.
- Starting the engine automatically at OS login.
- Connecting to remote machines, relays between machines, and group chats. These motivate the change, but R23 only requires that the protocol doesn't rule them out.
- OS notifications while the UI is closed.
- Moving existing GPUI `background_executor` work (`src/chat.rs`, `src/harness.rs` icon loading) onto tokio.

### Interactions with existing functionality

- The quit flow in `request_runtime_exit` and the "Stopping agents" modal are replaced by R7–R9 when the UI quits. Force stop for Complete, Archive and project deletion stays the same (R22).
- Every existing settings, agent and project screen keeps its current behavior, but its reads and writes go through the engine (R15).
- Interrupted conversations keep today's Retry behavior, which now also covers R10 and R27.

### Rejected ideas

- Running the engine inside the UI process and handing work over only at quit. A running agent's pipes can't be moved to another process.
- Stop all that waits for an explicit Force stop click, and Stop all that kills immediately without a graceful close.
- Background conversations that are read-only until they finish, or that stop when Adeline reopens.
- Waiting indefinitely when a permission is needed with no client connected.
- A quit dialog without Cancel, and a "remember my choice" option.
- Moving only the ACP driver to tokio, and requiring a single task per conversation. One `select!` loop risks blocking the pipes; the three-task layout stays.
- A separate engine binary.
- TCP on localhost as the transport.
- Allowing only one client at a time.
- Queueing a Send that arrives during processing, or merging it into the running turn. Last-write-wins for conflicting commands.
- A quit dialog that stops work for other connected clients.
- The UI keeping project or agent files, or writing them directly with the engine picking up changes through file watching.
- Moving `projects/` and `agents/` under `engine/`.
- Clients sending engine-relevant settings along with each command.
- A configurable stop grace period.
- The UI restarting an intentionally stopped engine on its own.
- A read-only UI cache shown while the engine is unavailable.
- A shared "last opened" order for projects across clients.
- A throwaway engine for demo mode.

## Domain and data

- **Conversation engine:** the background process that owns agent processes, conversation state, and every engine-owned file. There's one per OS user.
- **Client:** any connected Adeline UI, or a future relay. `adeline engine status` and `stop` are command-line requests to the engine; they don't keep a client connection open.
- **Active conversation:** a turn is processing, a retry is pending, or a permission is waiting with a client connected (R5).
- **Daemon mode:** the engine never exits on its own, and idle agents stay alive (R6).
- **Engine-owned data:** projects, conversations, transcripts, agent definitions, harness discovery results, and `engine/settings.yml`.
- **UI-local data:** theme, layout, open tabs, drafts, project sort order, and when each project was last opened, stored per client.
- **State changes added by this scope:** a turn becomes interrupted when it needs a permission with no client connected (R10) or when the engine crashes (R27). The engine moves between starting, running, the 60-second idle countdown, and stopping. A connected client sees the engine as connected, version-mismatched, stopped, or unavailable.

## Interfaces and dependencies

- **New:** the `adeline engine` subcommands (`status`, `start [--daemon]`, `stop`), a local client–engine protocol with version negotiation, and the tokio runtime added to Adeline.
- **Changed:** the quit flow, settings storage (`retry_limit` moves), every screen that reads or writes projects, conversations or agents, harness detection and probing, file watching, and the Settings window (new Engine group).
- **Packaging:** `scripts/package.ps1` keeps shipping a single `Adeline.exe`.

## Constraints and quality requirements

- R2, R14, R23 and R25 are the binding constraints: one engine per user, command ordering that can't deadlock, a local same-user transport, and no OS thread per conversation.
- R30 sets the startup timing targets.

## Failure and edge cases

Covered by R10 (a permission needed with no client), R13 and R14 (simultaneous or conflicting commands), R24 (version mismatch after an update), R27 (engine crash), R28 (intentional stop), R29 (engine unavailable), and R9 (retries running out in the background).

## Acceptance criteria

- **AC1 (R1, R3).** Launching Adeline with no engine running starts `adeline.exe engine` as a separate process, and the UI connects to it. Agent processes are children of the engine, not of the UI.
- **AC2 (R2).** Starting two UIs, or a UI and `adeline engine start`, at the same moment leaves exactly one engine running, and every client is connected to it.
- **AC3 (R4).** `adeline engine status` prints every listed field and exits 0 when the engine is running, and 1 when it isn't. `start` returns after starting the engine, and says so without starting a second one if an engine is already running. `--daemon` turns daemon mode on. `stop` runs the R8 flow, prints what it stopped, and returns only after the engine has exited.
- **AC4 (R5, R6).** Outside daemon mode, an engine with no active conversations and no clients exits 60 seconds after the last of the two went away. A client connecting within the 60 seconds keeps it running. In daemon mode it never exits on its own, and idle agents stay alive with no clients connected.
- **AC5 (R7).** Quitting the last client while a turn is active shows Stop all, Finish in background and Cancel, and Cancel keeps Adeline open. With nothing active, the UI quits with no dialog. Quitting while another client is connected shows no dialog and stops nothing. Stop all is also available from the menu.
- **AC6 (R8).** Stop all cancels turns and pending retries, closes agents gracefully, and kills any agent still running 5 seconds later with no further click. The dialog closes only after every agent process has exited. `engine stop` and Settings › Stop engine behave the same way.
- **AC7 (R9).** After Finish in background, a running turn completes with the UI closed, and a pending retry still runs. The finished reply and any errors are in the transcript when Adeline reopens. Idle agents were stopped at quit (outside daemon mode). The engine exits on its own once the work is done (R5).
- **AC8 (R10).** In Ask mode, a turn that is waiting on a permission, or that requests one, while no client is connected is stopped and shown as interrupted, waiting for permission. This happens in daemon mode too. Retry after reconnecting continues it.
- **AC9 (R11).** Reopening Adeline during background work shows those conversations streaming live, and Stop, Send and permission answers work. The engine doesn't exit while the UI is connected.
- **AC10 (R12, R13).** With two clients connected, changes from one appear in the other. If both send to the same idle conversation at nearly the same moment, exactly one turn starts. The other client sees the rejection message and keeps its composer text.
- **AC11 (R14).** If two clients answer the same permission, the first answer applies and the second gets "Permission already answered". Archiving a conversation from one client while the other sends to it gives a consistent result on both, with a reason for the rejected command. Repeated Stop commands are harmless. Running conflicting commands concurrently in a stress test never deadlocks the engine or corrupts a transcript.
- **AC12 (R15, R16).** Creating, editing and deleting agents and projects from Settings works as before. Every client sees the change. Files under `projects/` and `agents/` are written only by the engine process. Paths are unchanged, and the engine's files appear only under `engine/`.
- **AC13 (R17).** Opening a project in one client doesn't change another client's project order. After the upgrade, each UI's recency order matches the previous `opened_at` values.
- **AC14 (R18, R19).** After the upgrade, the retry limit matches the old `modes.chats.retry_limit` and is stored in `engine/settings.yml`. Settings has an Engine group with the daemon toggle, the retry limit, the status fields and Stop engine or Start engine. Chats no longer shows the retry limit. Changing the retry limit affects a turn already running in the background.
- **AC15 (R20).** Editing an `agent.yml` or a project file on disk updates every connected client with no restart.
- **AC16 (R21).** Harness detection, probes and icon downloads run in the engine process. Clients display the results the engine sends them.
- **AC17 (R22).** The acceptance criteria in `docs/archive/scope-acp-agent-driver.md` still pass, except for exit behavior. Complete, Archive and project deletion still require an explicit Force stop when an agent hangs.
- **AC18 (R23).** The engine accepts connections only through a local named pipe or Unix socket, and only from the same OS user. The protocol doesn't depend on the client and engine sharing files or paths.
- **AC19 (R24).** A newer UI connecting to an older engine that has active conversations shows Wait and Stop them now, and each behaves as described. For an older engine in daemon mode, Restart engine is offered only when nothing is active.
- **AC20 (R25).** With 20 live conversations, 5 of them processing, the engine's OS thread count is the same as with 1 conversation, apart from blocking-pool threads that come and go. A harness probe leaves no permanent threads behind.
- **AC21 (R26).** The engine writes logs to `engine/logs/`, and they stay within the size cap through rotation.
- **AC22 (R27).** Killing the engine leaves no agent processes running. Connected UIs show the unexpected-stop message, and the next action starts the engine again. Turns that were running show as interrupted, with Retry.
- **AC23 (R28).** `adeline engine stop` with a UI open shows "Conversation engine stopped" and Start engine, with no automatic restart. Send starts the engine again.
- **AC24 (R29).** If the engine can't start, the UI shows the unavailable state with the reason, the log path and Retry. UI-only settings can still be changed, and no engine files are written.
- **AC25 (R30).** With an engine already running, the project list appears with no visible delay. On a cold start, the starting state appears, followed by projects within 2 seconds.
- **AC26 (R31).** Demo mode runs with no engine process started or connected.

## Decisions and rationale

- **The engine always runs as a separate process (Q2).** There's one code path, and staying alive after the UI quits is just the client going away. Agent pipes can't be handed between processes.
- **The engine owns projects, conversations and agents (Q18, Q18a, Q38).** Conversations are stored inside project folders. A project is a working directory on one machine. Deleting a project has to stop its agents first. Multiple clients need one owner that broadcasts changes. Remote machines will need the same ownership. Per-client preferences stay in the UI.
- **Three tokio tasks per conversation (Q7).** Zed uses about four tasks per connection. Its fourth exists only to bring work back to GPUI's foreground thread, and the engine has no GPUI. A single `select!` task could stall pipe reading and freeze an agent on a full stderr pipe.
- **A single executable with an `engine` subcommand (Q9).** This matches Zeron's `zeron headless`/`daemon` design, minus the in-process fallback. One artifact means the UI and engine versions match after a fresh start.
- **Stop a turn when a permission is needed with no client connected (Q5, Q30).** Background work never hangs and never goes beyond the permission mode you chose.
- **Reject Sends during processing (Q15).** This matches today's per-conversation gate in `send_real` (`src/runtime_ui.rs`). ACP has no way to steer a running turn.
- **60-second idle grace (Q23)** so a UI restart or a plain `engine start` doesn't make the engine exit immediately. **5-second kill grace (Q33)**, the same as today's shutdown wait.
- **Paths stay the same (Q21)** to avoid extra directory levels. Only the engine's own files go under `engine/`.

## Open questions

None.
