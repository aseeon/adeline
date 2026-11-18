# Adeline code review

Review started November 17, 2026, at commit `6f11a89` on `main` and finished November 18. Adeline 0.4.3, engine protocol 8.

## Summary

I recommend fixing five issues before the next release. Docs can overwrite another editor's save. Agent traffic retains credentials. A permission button can answer a different request from the one displayed. Retry can drop the original prompt and attachment. Changes can display an obsolete diff while its watcher is running.

The review covered the six workspace crates, the CLI and remote bridge, build and packaging scripts, CI, and tests. The crate boundaries are clear. Module headers explain ownership, and the engine and UI share the code that applies conversation updates. The protocol fixtures and real engine tests make compatibility and recovery practical to check. The largest gaps appeared when two clients acted on the same state, or when state had to survive a restart. [Shared update logic](../crates/adeline_core/src/protocol.rs#L591) · [Protocol fixtures](../crates/adeline_core/tests/protocol.rs#L47) · [Engine integration tests](../crates/adeline/tests/engine.rs#L303)

### Verification

| Windows check | Result |
| --- | --- |
| Formatting, TODO check, and both Clippy configurations | Passed |
| Full workspace with all features | First run had 243 passes and one timeout in the Changes watcher test. That test passed alone. The full rerun passed all 244, with the headless executable supplied for the remote installation test. |
| Headless workspace | All 144 passed |
| Separate upgrade test using Adeline 0.1.0 | Automatic approval review blocked this check because it can download and execute a release binary. |

Isolated Windows engine checks reproduced findings 1, 4, 5, 6, 7, and 8. The security findings come from source inspection. The initial watcher timeout occurred in a test that changes line counts. Its cause remains unconfirmed. [Watcher test](../crates/adeline/tests/engine.rs#L860) · [Old release fixture](../crates/adeline/tests/ui.rs#L946)

## Findings

### Critical

#### 1. A stale Docs save overwrites a newer edit

Two clients read the same document. Both saves returned success, and only the second client's text survived. The UI blocks a save once it learns about a conflict. The engine receives no expected revision and replaces the current file. The existing conflict test waits for the warning before pressing Save, leaving the interval before notification uncovered. [Save request](../crates/adeline_core/src/protocol.rs#L249) · [Engine write](../crates/adeline_engine/src/engine/docs.rs#L88) · [Existing UI test](../crates/adeline/tests/ui.rs#L420)

Send the revision the editor read and check it at save time. Give explicit overwrite its own path. Test two clients saving from one revision. The second must receive a conflict, and the first client's text must survive.

#### 2. Agent traffic records MCP credentials in chat history

Session setup includes MCP HTTP header values and process environment values. Adeline copies the outgoing request into a traffic entry, truncates it by length, then writes it to the conversation transcript. A user who shares that transcript can share an active credential. [MCP request fields](../crates/adeline_engine/src/acp/session.rs#L273) · [Outgoing traffic capture](../crates/adeline_engine/src/acp/session.rs#L115) · [Traffic truncation](../crates/adeline_engine/src/acp/session.rs#L678) · [Transcript recording](../crates/adeline_engine/src/engine/events.rs#L428)

Redact these values before creating the diagnostic entry. Use a fake credential in a stub agent test. Verify that the agent receives it and that both the saved transcript and Traffic response omit it.

#### 3. A permission click can answer a different request

The button captures only the option ID. Its handler later looks up the first pending request in the currently selected chat. If another window answers the displayed request before the click is handled, the next request can receive that choice. The engine's duplicate-answer check cannot catch this because the UI supplies the newer request ID. [Permission button](../crates/adeline_ui/src/conversation_ui.rs#L410) · [Click handler](../crates/adeline_ui/src/runtime_ui.rs#L763) · [Engine validation](../crates/adeline_engine/src/engine/conversations.rs#L270)

Bind the button to the displayed machine, conversation, and request. Give each request an identity that survives worker replacement. Test that a queued click on an expired card leaves the next request pending when both offer the same option. [Worker request counter](../crates/adeline_engine/src/acp.rs#L612)

#### 4. Retry after engine restart can lose the entire request

I made a test agent exit before it received its first prompt. After restarting the engine and making the agent available, Retry sent only the generic continuation instruction. The original text and attachment were both missing. Startup treats every blocked turn as having done work and initializes the retry attachment map empty. [Startup recovery](../crates/adeline_engine/src/engine.rs#L247) · [Attachment state](../crates/adeline_engine/src/engine.rs#L406) · [Retry selection](../crates/adeline_engine/src/engine/turns.rs#L376)

Recover the last user message and its attachments from the transcript. Use recorded assistant or tool activity to decide whether to continue previous work. Extend the restart test with a failure before prompt delivery and assert the exact parts the agent receives. [Existing restart test](../crates/adeline/tests/engine.rs#L303)

#### 5. Changes misses edits that preserve line counts

I changed an added line from `alpha` to `bravo`. A direct read returned the new patch, but the watcher sent no update. It delivered the edits immediately before and after this one. The engine suppresses the notification when the file list, statuses, and counts match. The UI then keeps the old diff and can keep a line comment marked current. [Notification filter](../crates/adeline_engine/src/engine/changes.rs#L421) · [UI refresh](../crates/adeline_ui/src/changes.rs#L600) · [Comment validation](../crates/adeline_ui/src/changes.rs#L823)

Use the changed paths to invalidate open and commented files even when the counts match. Extend the watcher test with a second edit of the same added line. Check that the new text appears and the original comment becomes stale.

### Improvements

#### 6. Watch Git metadata for linked worktrees

In a linked worktree, the watcher delivered a file edit, missed the subsequent commit, and delivered the next edit. A direct list immediately after the commit showed a clean tree. Adeline watches the project directory. In a linked worktree, Git updates the index and refs outside that directory. [Watch registration](../crates/adeline_engine/src/engine/docs.rs#L201) · [Project path filter](../crates/adeline_engine/src/engine/changes.rs#L369)

Watch the worktree's Git directory and relevant refs and map those events back to the project. Add a linked worktree to the existing commit test. The clean-tree notification must arrive before another working file changes.

#### 7. Concurrent settings edits silently undo each other

Two clients started with a retry limit of 3. The first changed it to 4. The second changed only a Docs preference. A fresh client saw 3 again. The Docs client sends its entire cached settings object and the engine persists it wholesale. The same path carries the MCP server list. [Docs settings update](../crates/adeline_ui/src/docs.rs#L1513) · [Engine settings write](../crates/adeline_engine/src/engine/projects.rs#L360)

Send field changes or reject stale revisions. Add an integration test where two clients change different settings and a third reads both edits.

#### 8. Importing a populated project skips its conversations' live state

I restored a valid project while the engine was running. A new client saw its chat history, but its snapshot had no live entry for that chat. Restarting the engine restored the entry. Reconciliation adds a new project and skips reporting its conversations. Only the reported conversation IDs receive live state and lifecycle records. [New project branch](../crates/adeline_engine/src/storage.rs#L946) · [Live state creation](../crates/adeline_engine/src/engine/projects.rs#L484)

Use the same conversation initialization for startup and imports. Extend the reconciliation test with a populated project. Check its agent identity, options, and recovery state through a fresh client connection. [Current reconciliation fixture](../crates/adeline_engine/src/storage/tests.rs#L264)

#### 9. Send can submit the same draft twice

The composer keeps the draft and attachments until the engine replies. A second Send during that interval submits them again. On a slow connection, two submissions from a new chat can create two conversations. An existing chat can queue the second copy. [UI submission and acknowledgement](../crates/adeline_ui/src/runtime_ui.rs#L178) · [Engine queue and creation paths](../crates/adeline_engine/src/engine/turns.rs#L63)

Track the submitted draft until acknowledgement. Preserve edits made while it is pending. Test two Send actions against a delayed reply and verify that exactly one prompt reaches the agent.

#### 10. Login URLs open on every subscribed client

The engine knows which client started login. It discards that identity when forwarding the agent's URL, broadcasts the URL, and records it for reconnect replay. A second desktop connected to the same engine can open the first desktop's sign-in flow. [Login forwarding](../crates/adeline_engine/src/engine/projects.rs#L258) · [Broadcast and replay](../crates/adeline_engine/src/engine.rs#L498) · [Browser action](../crates/adeline_ui/src/runtime_ui.rs#L868)

Send a one-time event to the client that requested login. Test with two clients and a stub URL opener. Only the requester should see the event, once, including across a reconnect.

#### 11. Conversation state can collide across machines

Project keys include the machine. The UI's live conversation map uses only the conversation ID. Two machines with a copied chat directory can share that ID. Applying one snapshot overwrites the other's live state. Unchecking one machine can also remove the remaining machine's entry. [Project identity](../crates/adeline_core/src/data.rs#L18) · [Conversation map](../crates/adeline_ui/src/runtime_ui.rs#L22) · [Snapshot removal and insertion](../crates/adeline_ui/src/runtime_ui/machine_sync.rs#L86)

Key live state and chat lookups by machine and conversation. Give two fake engines the same chat ID and different statuses. Verify both statuses after connecting, then uncheck one machine and verify the other again.

#### 12. Unchecking a machine does not cancel SSH setup

Removing a connection drops its cancellation sender. The remote task waits for the probe, any installation, and the bridge greeting before it starts listening for cancellation. Unchecking a machine during setup can leave SSH running or allow installation to proceed. A bridge that never sends its greeting can hold the attempt open. [Connection removal](../crates/adeline_ui/src/client.rs#L398) · [Remote setup](../crates/adeline_ui/src/client.rs#L490) · [Greeting wait](../crates/adeline_core/src/remote.rs#L630) · [Cancellation handling](../crates/adeline_ui/src/client.rs#L557)

Apply cancellation to the whole attempt and bound the wait for a greeting. Use the fake SSH transport to pause setup, uncheck the machine, and verify the child exits before installation starts.

#### 13. Untracked file counts follow links outside the project

The untracked-file scan joins Git's path to the project root and opens it to count lines. Both the metadata lookup and read follow symbolic links. An external target can therefore contribute a line count to Changes even though the normal file viewer checks project containment. [Untracked scan](../crates/adeline_engine/src/engine/changes.rs#L153) · [File read](../crates/adeline_engine/src/engine/changes.rs#L90) · [Viewer path check](../crates/adeline_docs/src/files.rs#L158)

Count only regular files inside the project. Use link metadata before opening an entry. Test that an untracked link is represented without reading or counting its target.

#### 14. The tracked diff size limit applies after allocation

Adeline requests up to a million lines of context and buffers Git's entire output. It checks the 2 MiB preview limit afterward. Opening one large tracked text file can therefore allocate far more than the preview permits. The existing oversized-file test exercises an untracked file. [Git output collection](../crates/adeline_engine/src/engine/changes.rs#L36) · [Tracked patch limit](../crates/adeline_engine/src/engine/changes.rs#L276) · [Size test](../crates/adeline_engine/src/engine/changes.rs#L518)

Read at most the limit plus one byte of patch output and reap the child when the limit is exceeded. Cover a large tracked file with a small edit. Verify both the oversized result and the read bound.

#### 15. Opening Traffic reads the whole transcript on the engine's command loop

The Traffic response returns at most 2,000 records. Storage first reads the entire transcript and collects every saved traffic entry. The command loop waits for that work, so opening an old, busy chat can delay unrelated commands and agent events. [Traffic limit](../crates/adeline_engine/src/engine.rs#L51) · [Transcript scan](../crates/adeline_engine/src/storage.rs#L851) · [Synchronous caller](../crates/adeline_engine/src/engine/projects.rs#L348)

Read backward from the saved end, merge pending entries in order, and perform disk I/O outside the command loop. Test recent traffic ordering across a flush. Use a paused reader to verify a second client can still obtain engine status.

### Nitpick

The README says Adeline never installs harnesses and asks users to sign in separately. The agent form offers installation and login for supported harnesses. Update the onboarding text to describe what users can do in the app and when manual setup is needed. [README](../README.md#L45) · [Installer and login actions](../crates/adeline_ui/src/agent_form/installer.rs#L33)

## Conclusion

Request changes before the next release. Add regression tests for stale saves, credential handling, permission identity, retry before prompt delivery, and diff updates with unchanged counts. Run those cases through the engine or UI that owns the behavior. Include linked worktrees in the Changes checks on Windows, macOS, and Linux.
