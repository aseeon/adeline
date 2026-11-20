# Agents and ACP

Adeline runs one ACP agent process per conversation and speaks to it through the official `agent-client-protocol` SDK and its typed v1 messages.

## Only `acp/` knows ACP

`acp.rs` and its children are the only modules that see SDK types. They translate the protocol to and from Adeline's own model in `conversation.rs`. The engine ↔ client protocol, storage and the UI use only that model, so a change in ACP stays inside `acp/`.

The model renames a few things:

- The TODO list is what ACP calls the `plan`.
- Session modes that an agent offers through `session/set_mode`, rather than as a config option, appear as the option `MODE_OPTION`.
- `StopReason::Signal` ends a turn on the profile's turn-end signal, without a prompt response.
- An `Attachment` carries its bytes base64 encoded, or a `path` on the engine's machine to read instead. `read_attachment` reads a local file into one.
- Usage is counted in tokens, and traffic times are epoch milliseconds.

## Traffic

Every line in both directions, and the agent's stderr, becomes traffic. Lines that aren't JSON are kept as traffic and skipped.

## The worker

The `Worker` runs three state machines side by side: `Connection` (the process, its link and the handshake, with `AgentSession` for which session to open), `Step` (the active turn) and `Exit` (shutdown, with `ExitWatch` for an exit that is overdue).

- Probes and logins are short-lived sessions through the same client. Dropping their `Session` stops the agent.
- Permission request IDs are unique across workers. An answer to a request from a worker that has since been replaced can't answer the new worker's request.

## Profiles and installs

- `profiles.rs` holds what Adeline knows about each supported agent beyond generic ACP. Agents without a profile, including Custom agents and other registry agents, get generic behavior.
- The engine detects installed harnesses on its blocking pool, and clients get a copy (`harness.rs`).
- The engine runs installs and logins on its own machine and streams the output to the client, so a remote machine installs on itself. A run stops at its first failed step (`install.rs`).
