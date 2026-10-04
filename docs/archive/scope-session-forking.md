Status: Implemented in v0.1.2. This scope is history. Where it and the code differ, the code is right.

# Session forking

## Purpose and context

Anyone who made a mistake during a conversation (a wrong prompt, or an agent that went down the wrong path) needs a way to go back to an earlier point and try again without losing the original. Example: an agent goes wrong after its reply to message 6. The user forks at that reply, gets a new chat that knows messages 1–6, and tries a different approach while the original conversation stays intact.

The idea comes from the Omnigent meta-harness's session forking (Barbaste et al., *Harness Engineering*, arXiv 2609.00006, §14.4).

Current behavior:

- Adeline has no fork. Nothing in `src/` implements one.
- When a saved session can't be restored, Adeline already rebuilds context as text. `Engine` in `src/engine.rs` (the replacement-session path that sends `acp::Command::ReplaceSession`) joins the visible messages as `role: text`. `Worker::configured` and `pending_context` in `src/acp.rs` put a "Saved conversation context…" preamble in front of the next prompt.
- Each conversation keeps its own copied execution settings and permission mode (`ExecutionConfig` and `StoredConversation` in `src/storage.rs`).
- A finished turn's last reply offers Reply, Copy and Retry (`closing_row` in `src/chat_render.rs`). Retry is hidden in demo mode.
- A new chat is not saved and does not start an agent until its first Send (`docs/archive/scope-acp-agent-driver.md`, AC1 and AC6).
- ACP defines `session/fork` only as an unstable draft, gated on `sessionCapabilities.fork`. It forks a whole session, not from a chosen message, and the RFD names only `claude-agent-acp` as an implementation (https://agentclientprotocol.com/rfds/session-fork).

## Requirements

### Starting a fork

- **R1.** Every finished agent reply offers a Fork action alongside Reply, Copy and Retry. Fork is available whatever state the source conversation is in, including Processing, Attention, Active, Completed and Archived. User prompts do not offer Fork.
- **R2.** Forking creates a new conversation in the same project. It is saved immediately, opens with the composer focused, and starts no agent until its first Send.
- **R3.** The fork is titled "‹source title› (fork)".
- **R4.** The fork copies the source conversation's current execution settings (agent, harness, command, arguments, model, effort, instructions and working directory) and its permission mode as they are at the moment of forking. Later changes to either conversation do not affect the other.

### Copied history

- **R5.** The fork contains the source's visible history up to and including the chosen reply: user and agent messages, tool-call entries and errors, displayed as they were in the source. Nothing after the chosen reply is copied.
- **R6.** Raw ACP traffic, lifecycle records and permission decisions from the source are not copied. Permission grants never carry over.
- **R7.** The fork shows a "Forked from ‹source title›" marker before its copied history. The marker links to the immediate source conversation and opens it even if the source is archived. If the source no longer exists, the marker shows the title without a link.
- **R8.** A fork can itself be forked. Its marker points to its immediate source.
- **R9.** Forking does not change, stop, interrupt or otherwise affect the source conversation, including a source that is processing.

### Context on first Send

- **R10.** On the fork's first Send, Adeline uses the harness's native session fork only if all three hold: the harness advertises `sessionCapabilities.fork`, the source's latest message is still the fork point, and the source is not processing.
- **R11.** Otherwise Adeline gives the agent the copied history as text, using the same form as the existing saved-context path. The text marks the history as prior context that must not be repeated, followed by the user's new request.
- **R12.** If the native fork is attempted and fails, Adeline falls back to the text copy without asking.
- **R13.** Whenever the text copy is used (R11 or R12), the fork shows a visible note: "Started from a text copy of the history."
- **R14.** The fork's visible transcript shows only what the user typed as the new user message, not the replayed history or the preamble.
- **R15.** After its first Send, the fork behaves like any other conversation: its own session, restoration, retries, permission handling, completion and archiving.

### Constraints and quality requirements

- **R16.** The text copy has no size limit, the same as the existing saved-context path. If the harness rejects an oversized prompt, the existing error classification and retry handling apply.
- **R17.** Fork works in demo mode. It creates a local copy and the first Send is simulated, as demo Send already is. Demo forks never start real agent work.
- **R18.** A fork and its copied history survive an application restart before and after its first Send.

### Failure and edge cases

- **R19.** If the fork can't be saved, Adeline shows a recoverable error and leaves no partial fork in the conversation list. The source is unaffected.
- **R20.** If the fork's working directory no longer exists, sending reports the missing directory and does not start the agent elsewhere, as for any conversation.

## Boundaries

- Forking copies the conversation only. Files in the working directory are not restored; code changes the source made after the fork point remain on disk.
- Forking from a user prompt (and editing that prompt) is excluded.
- The fork always uses the source's agent. Choosing a different agent when forking is excluded.
- No warning or limit on the size of the copied history.
- Existing rules still apply: no agent starts without a Send, settings are snapshotted per conversation, and the existing restoration, retry and storage-failure behavior is unchanged.

### Rejected ideas

- Text replay only, ignoring the native fork.
- Native fork only, hiding Fork for harnesses that lack it.
- Calling the native fork when the fork is created, which would start an agent before any Send.
- A fork that is a discardable draft until the first Send.
- Copying only messages, or copying the full transcript including raw ACP traffic.
- Leaving forks untitled.
- Blocking forks while the source is processing.
- Hiding Fork in demo mode.
- Asking the user what to do when a native fork fails.

## Domain and data

- A **fork** is a conversation created from another conversation's history. The conversation it came from is its **source**. The **fork point** is the finished agent reply the user chose.
- A fork owns its own settings, event history and session from creation. It references its immediate source for the marker only. Deleting, archiving or completing either conversation does not change the other's history.
- The **text copy** is the copied history given to the agent as prior context when the native fork is not used.

## Interfaces and dependencies

- ACP `session/fork` (unstable draft, gated on `sessionCapabilities.fork`). Used only under R10. The RFD warns that `claude-agent-acp` may not retain configured MCP servers across a fork.
- The existing saved-context path in `src/engine.rs` and `src/acp.rs` (`pending_context`) for the text copy.
- Conversation storage in `src/storage.rs` for creating the fork and its copied history.
- The finished-turn action row in `src/chat_render.rs` (`closing_row`) as the entry point.

## Acceptance criteria

- **AC1 (R1).** Every finished agent reply shows Fork next to Reply, Copy and Retry, in Processing, Attention, Active, Completed and Archived conversations. User prompts show no Fork.
- **AC2 (R2, R3).** Choosing Fork immediately adds a conversation titled "‹source title› (fork)" to the same project, opens it with the composer focused, and starts no harness process.
- **AC3 (R4).** The fork uses the source's agent, model, effort, instructions, working directory and permission mode as they were when forking. Changing the model or permission mode in either conversation afterwards leaves the other unchanged.
- **AC4 (R5, R6).** Forking at the reply to message 6 of a 10-message conversation shows messages, tool-call entries and errors up to that reply and nothing after it. The fork's saved history contains no raw ACP traffic, lifecycle records or permission decisions from the source, and no earlier grant is applied in the fork.
- **AC5 (R7, R8).** The fork shows "Forked from ‹source title›" linking to the source. The link opens an archived source. After the source is deleted, the marker shows the title without a link. Forking a fork produces a marker pointing to the first fork.
- **AC6 (R9).** Forking a conversation that is processing does not interrupt its turn, change its transcript or alter its status.
- **AC7 (R10).** For a harness advertising `sessionCapabilities.fork`, forking at the latest reply of an idle source and sending uses the native fork, and no text copy note appears.
- **AC8 (R10, R11, R13, R14).** Forking at an earlier reply, forking from a harness without the capability, or forking at the latest reply when the source has since advanced or is processing at the fork's first Send: the agent receives the text copy plus the new request, the note "Started from a text copy of the history." appears, and the visible user message is only the typed text.
- **AC9 (R12, R13).** When a native fork call fails, the first Send still proceeds with the text copy without prompting the user, and the note appears.
- **AC10 (R15).** After the first Send, the fork can be stopped, retried, completed, archived and reopened with its context restored, like any conversation.
- **AC11 (R16).** A fork of a very long conversation is created and sent without a size warning. A harness rejection is reported and handled by the existing error and retry rules.
- **AC12 (R17).** In demo mode, Fork creates the copy and the first Send is simulated. No real agent runs.
- **AC13 (R18).** Restarting Adeline before and after a fork's first Send preserves the fork, its title, marker, copied history and settings.
- **AC14 (R19).** A storage failure while forking shows a recoverable error, leaves no partial fork in the list and leaves the source unchanged.
- **AC15 (R20).** If the working directory has been removed, sending from the fork reports the missing directory and starts no agent.

## Decisions and rationale

- Q1: The users are anyone who made a mistake during a conversation and wants to retry from an earlier point.
- Q2: Fork from finished agent replies only; they are the clean cut points.
- Q3, Q8: Use the native fork where possible, otherwise the text copy. Because ACP's draft fork copies a whole session, native forking applies only at the latest reply.
- Q4: The fork uses the source's agent and copied settings, matching per-conversation settings snapshots.
- Q5: The fork shows its copied history and a marker, so the user can see what the agent knows.
- Q6: Files are not restored; that would need checkpoints or worktrees.
- Q7: Forking reads saved history only, so a processing source is allowed.
- Q9: The "(fork)" title suffix keeps the fork recognisable in the list.
- Q10: The fork is saved immediately but starts no agent until Send, keeping the existing no-agent-without-Send rule.
- Q11: Copy visible history, not another session's protocol log.
- Q12: The marker links to the immediate source; forks of forks are allowed.
- Q13: Fork is a local copy, so it works in demo mode.
- Q14: No size limit, consistent with the existing saved-context path.
- Q15: Decide on native forking at the first Send so the fork never gets context past its fork point. Fall back to text on failure without asking, with a visible note.

## Open questions

None.
