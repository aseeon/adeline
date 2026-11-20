# Remote machines

One client shows the projects of every checked machine. Remote machines are reached through the system `ssh`, and no network port opens anywhere.

## Connecting

- `remote.rs` finds the remote platform and its Adeline, installs this client's version there, and runs `adeline bridge`. The bridge joins the SSH session to the remote engine's local pipe or socket.
- The SSH session starts in the home folder on every platform, so remote paths are relative to it.
- `ssh` runs with the client's askpass address, so its prompts reach the UI.

## Askpass

`ssh` runs Adeline's executable as its `SSH_ASKPASS` helper with `ADELINE_ASKPASS` set, and `askpass_main` hands the prompt to the client that started `ssh`.

- The client listens on an owner-only named pipe on Windows, or a 0600 Unix socket elsewhere, so only the same user can ask or read answers.
- One JSON line goes each way: the `Ask`, then the answer, or `null` when the user cancels.

## Saved machines

- Saved machines live in `machines.yml` on the client. Other clients never see it.
- Demo mode keeps fake machines in memory and never touches the file.
- The last checked machine can't be unchecked.
- Removing a machine leaves its engine and data untouched.
- A file that can't be read or parsed is never overwritten.

## Connection states

`client.rs` keeps one connection per checked machine. Each connection's changes arrive in order on its own channel, and replies run their callbacks in that order.

A connection's `State` is its whole phase, and only the phases with an open link hold its `Link`.

- `dial` starts `Connecting`.
- The snapshot or `Resumed` moves it to `Connected`. `go_live` then sends the requests queued until then.
- An older local engine moves it to `Mismatch`. Once the user picks, it moves to `Waiting` until that engine exits.
- Closing, failing or `Bye` end in the other states. A lost remote waits out its delay in `Disconnected` and dials again.

With several machines checked, a local engine that would get a full-window screen gets a banner instead (`runtime_ui/screens.rs`).
