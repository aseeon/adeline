# Storage and files

Adeline keeps its data in plain files that people can read and edit. These decisions keep those files safe from crashes, bad edits and links.

## Writing files

- `files.rs` syncs every write before it replaces anything. `replace` swaps the new file in with one rename, so the original stays until it is replaced.
- Settings files that are missing get seeded with defaults. Existing ones are never replaced (`config.rs`).
- Render code reads settings through `config::with`, which doesn't clone them.
- Agent definition files older than `agents::VERSION` are left alone and not listed.

## Project storage

- A conversation's agent and launch settings are fixed when it is created. Only its option selections change.
- A conversation's settings keep what the agent last offered, so its menus work while the agent is stopped.
- `queue_event` holds streamed events for `flush_due` to write within `FLUSH_INTERVAL_MS`. `record_event` writes at once, together with everything queued before it.
- Nothing is read or deleted through a link. `storage/paths.rs` checks that a path is a real directory or file, and that a working directory lies outside project storage.

## Times

- Event timestamps are epoch milliseconds, and a project's `opened_at` is in epoch seconds.
- In the UI's model (`data.rs`), `u64` times are milliseconds since the epoch and `i64` times are seconds. A time of 0 means unknown.
