# The engine and its protocol

The engine is a background process that owns agent processes, conversation state and every engine-owned file. One engine runs per OS user, and it outlives the UI. Clients talk to it over a local pipe or socket, or over SSH from another machine, so nothing in the protocol assumes the client shares the engine's machine.

## One task, in arrival order

Every command and agent event goes through one task, in arrival order (`engine.rs`). Commands from several clients can't interleave or deadlock.

## Deltas and reconnects

Every delta the engine sends is numbered within its process's `epoch`, and the engine keeps the latest `RECENT_DELTAS`. A client that reconnects with `resume` gets `Resumed` and the deltas it missed while the engine still holds them, or else a full snapshot. `apply` runs in the engine and in every client alike, so all of them hold the same state. A CLI `Hello` doesn't count as a connected client.

## Lifecycle

`phase.rs` is the engine's only record of a conversation's lifecycle. `Live`'s phase flags (`project`) and the runtime `ThreadStatus` (`status`) are computed from it, and its methods are the transitions that the handlers in `turns.rs`, `events.rs`, `conversations.rs` and `projects.rs` call.

- Storage failure is a flag beside the phase. It can happen in any phase and lasts until Retry storage.
- A turn that storage stops carries `Stop::Storage`. It shows as recovering once storage is retried, until that turn ends.
- `text_copy` marks a fork whose copied history no turn has confirmed yet. A stopped turn leaves it set, since a stop can't tell whether the history reached the agent.

## Transport

`ipc.rs` uses a same-user named pipe on Windows and a Unix socket elsewhere, with one JSON object per line.

- `connect` fails with `NotFound` when no engine runs.
- `engine/id` is the engine's lasting identity. Remote clients read it over SSH to tell engines apart.
- A UI started with `--control` listens on its own pipe or socket, named after its process ID (`control_address`).

## Logging

The engine's log file is rotated at `LOG_CAP` and kept to three old copies. Code logs through `tracing` events with dot-separated names and named fields. A process that never calls `log::open` drops them.

## Protocol versions

Bump `PROTOCOL` whenever a message changes shape. A client and an engine work together exactly when their `PROTOCOL` matches.

- `hello`, `welcome`, `status`, `stop_all` and `shutdown` must keep working across versions.
- Fields added later have defaults, so older engines send `at: 0` and no `headless`.
- Times are epoch milliseconds.

## Replies

Replies other than `null`:

- `Send` and `Fork` give a conversation ID, `SaveProject` a project ID and `SaveAgent` an agent ID.
- `TakeQueued` gives the message's text and attachments as `(String, Vec<Attachment>)`.
- `Probe` gives a `Result<Probed, ProbeError>`, and `PlanInstall` a `Vec<install::Planned>`.
- `ListDirectory` gives a `Listing`, of the home folder when there is no path.
- `WatchTraffic` gives the traffic recorded so far.
- `ListDocs` gives the project's doc paths, `ReadDoc` a `DocFile`, `WriteDoc` the saved file's modification time, and `ReadImage` the image's bytes in base64.
- `ListFiles` gives every project file's path, `ListChanges` a `Changes` and `ReadChange` a `ChangeFile`.
- `DeleteProject`, `StopAll`, `Shutdown` and `Login` reply once done. `Install` streams `Output` lines first.

`WriteDoc` with `modified` fails when the file on disk has another modification time, so a save can't replace an edit the client hasn't read.

## Watching docs and changes

Doc paths are relative to the project root, with `/` separators. While a client watches a project's docs (`WatchDocs`), `Delta::Docs` reports docs created, changed or removed on disk. `relist` means a folder changed, so the client lists the docs again.

Changes compare the working tree with `HEAD`. While a client watches a project's changes (`WatchChanges`), `Delta::Changes` sends them again after they change on disk or in git, with `files` when the project's file list changed too.

## Pinning the wire format

`crates/adeline_core/tests/protocol.rs` pins the shape of every message. `tests/wire/<PROTOCOL>.jsonl` holds a sample of each message, and each sample must read back unchanged, so a renamed, added, removed or retyped field fails the test. Collections hold one entry, so a sample pins each element's shape too.

When the test fails, the change breaks engines and clients on other machines. Then:

1. Bump `PROTOCOL` in `protocol.rs`.
2. Rename the samples file to the new number.
3. Replace the failing lines with what the failure says they now read as.
