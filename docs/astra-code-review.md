# Adeline code review

Reviewed on 2026-10-31 at commit `cd03b2b499c3aff23fa13fc1ee00e211da387048`, workspace version `0.3.2`.

This is a static review across the six workspace crates and the build, packaging, and CI configuration. It focuses on security, bugs, unnecessary code, duplication, and complexity. Tests, naming, comments, and style are outside the review. No application code was changed.

The findings below come from tracing source paths and, for Markdown images, checking the locked GPUI dependency sources. Triggering scenarios describe consequences of those paths, not executed reproductions. I did not run the application, application tests, or an external vulnerability-advisory scan. This review does not establish that unlisted paths are bug-free.

P1 means fix soon because the issue can expose private data or overwrite user work. P2 means a concrete defect or resource problem worth scheduling. The final simplification item is a maintenance recommendation, not a demonstrated bug.

| ID | Priority | Finding |
| --- | --- | --- |
| R1 | P1 | Temporary attachment files are predictable, non-private, and never cleaned up |
| R2 | P1 | Private application storage inherits permissive Unix modes |
| R3 | P1 | Saving a document can overwrite an unresolved external edit |
| R4 | P2 | MCP credentials are copied into permanent traffic logs and conversation snapshots |
| R5 | P2 | Cached documents stay stale after leaving Docs or losing its watcher |
| R6 | P2 | A special file can block the entire engine command loop |
| R7 | P2 | Editing a queued message discards its attachments |
| R8 | P2 | A delayed Send response clears attachments added afterward |
| R9 | P2 | Connection cancellation starts too late to cancel SSH setup |
| R10 | P2 | Raw agent traffic grows without a retention limit in memory and on disk |
| R11 | P2 | Deleting a project leaves attachment payloads in engine maps |
| R12 | P2 | Whole-settings replacement can silently undo unrelated changes |
| R13 | P2 | Markdown image loading is incomplete and inconsistent between renderers |
| R14 | P2 | Empty attachment content is mistaken for missing content |

## R1. Temporary attachment files are predictable, non-private, and never cleaned up

**Evidence:** [ACP attachment conversion](../crates/adeline_engine/src/acp/turns.rs), `linked_path`, lines 465–478, and [filesystem helpers](../crates/adeline_core/src/files.rs), `unique`, lines 19–25.

When an agent cannot accept embedded files, Adeline decodes an attachment into a directory under `std::env::temp_dir()`. The directory name is `adeline-attachment-<PID>-<counter>`. It uses `create_dir_all` followed by `fs::write`, accepts a directory that already exists, and applies no private permissions. Only the resulting path is retained. There is no owner responsible for removing the temporary file or directory.

On a Unix system with a shared temporary directory and umask `022`, the normal result is a directory readable and traversable by other users and a world-readable attachment. Files can contain credentials, private source, or other documents. The copies outlive the conversation and application.

There is also a write-through-symlink risk. Another local user can pre-create a predicted directory, make that directory writable, and place a symlink at an anticipated attachment filename. `create_dir_all` accepts the existing directory and `fs::write` follows the filename's symlink. That can overwrite a file the Adeline user can write. This scenario uses a real attacker-owned directory with a link inside it, so it does not depend on following a symlink directly in a sticky `/tmp` directory. It requires access to the shared temporary directory and predicting or covering candidate names. It is not a claim of an arbitrary remote attack or a default Windows cross-user exploit.

**Recommendation:** Use an exclusively created, unpredictable temporary directory with private permissions from creation, including mode `0700` on Unix. Create attachment files privately and exclusively inside that owned directory. Keep an owner for the temporary resources for as long as the agent may need them, then clean them up. Handle abandoned resources after crashes without traversing untrusted links. Merely changing the prefix or adding a timestamp would not fix the ownership problem.

## R2. Private application storage inherits permissive Unix modes

**Evidence:** [configuration](../crates/adeline_core/src/config.rs), `directory` at line 266 and `init` at line 316, [filesystem helpers](../crates/adeline_core/src/files.rs), `write_new` at line 70 and `replace` at line 96, and [conversation persistence](../crates/adeline_engine/src/storage/disk.rs), `write_conversation` at line 5.

The application stores its data below `~/.config/adeline`. Directory creation and file creation generally use the process defaults. The project and conversation stores do not enforce private modes. Their path checks reject links and unexpected file types but do not check confidentiality permissions.

On Unix, with umask `022` and traversable parent directories, other local users can read conversation transcripts and execution snapshots. Those snapshots include prompts, attachments, instructions, and MCP configuration. The private permissions applied to the engine IPC directory do not protect sibling project or agent directories.

Atomic replacement introduces a second problem: `files::replace` creates a new file with default permissions and renames it over the original. A user who tightened a file to `0600` can have it changed back to `0644` by an ordinary save. This is conditional on filesystem and parent-directory permissions, not a claim that every installation is exposed.

**Recommendation:** Define a private application-data boundary and create it with mode `0700` on Unix. Use mode `0600` for sensitive files and private temporary replacements, and repair existing application-owned data safely. On Windows, preserve or establish the intended user-only ACL rather than translating Unix mode bits. Keep application-state writes distinct from project-document writes, where preserving the project's existing permissions is appropriate.

## R3. Saving a document can overwrite an unresolved external edit

**Evidence:** [document save handling](../crates/adeline_ui/src/docs.rs), `docs_save` at line 795, [save controls and conflict banner](../crates/adeline_ui/src/docs/view.rs), lines 810–820 and 865 onward, and [engine document writes](../crates/adeline_engine/src/engine/docs.rs), `write` at line 75.

The UI detects an external change to a dirty document and offers “Keep my version” and “Take the version on disk.” However, the ordinary Save control remains enabled. `docs_save` checks `read_only` but does not check `doc.conflict`. Ctrl+S and Save All reach the same unrestricted write path.

At the protocol boundary, `WriteDoc` carries the replacement text and `create` flag, with no expected revision or original-content hash. The engine reads the latest file to preserve its text format, then replaces it without checking that it is the version the user edited.

For example, edit a document in Adeline, have an agent or another editor change it on disk, then press Ctrl+S after the conflict banner appears. The external changes are overwritten without choosing “Keep my version.” Even disabling Save while the banner is present would leave a race when a disk change happens before its watcher notification arrives.

**Recommendation:** Make unresolved conflicts block ordinary Save and Save All. Require an explicit overwrite decision for “Keep my version.” Add an expected content revision to the engine write operation and reject stale writes there, returning the current version for resolution. Check against content or a sufficiently strong revision, rather than relying only on millisecond timestamps. Account for external writers during the write operation. A protocol-shape change must bump `PROTOCOL` under the repository's versioning rules.

## R4. MCP credentials are copied into permanent traffic logs and conversation snapshots

**Evidence:** [ACP transport](../crates/adeline_engine/src/acp/session.rs), `connect_lines` at line 109, `traffic_sink` at line 194, `mcp_servers` at line 265, and `trimmed_traffic` at line 625, [traffic persistence](../crates/adeline_engine/src/engine/events.rs), `traffic` at line 387, and [conversation creation](../crates/adeline_engine/src/storage.rs), lines 646–669.

MCP stdio environment values and HTTP header values are serialized into session requests. Every outgoing wire line passes through the traffic sink. The sink truncates long lines but does not redact secrets. The engine persists traffic before checking whether anyone is watching it, so normal use records this data even when the traffic viewer is closed.

Conversation creation also copies the full MCP server configuration into its durable execution snapshot. Removing or changing a token in the current server settings therefore leaves historical copies in conversations. Diagnostic traffic creates further copies that can be displayed, backed up, or shared as logs. R2 increases the risk on permissively configured Unix systems, but the extra copies remain an issue even with private filesystem permissions.

**Recommendation:** Redact credential-bearing fields before diagnostic events are created, using the protocol structure for environment values, headers, and credential-bearing URLs. Truncation is not redaction. Separate resumable execution configuration from secret storage so snapshots can refer to credentials without copying them. Define retention and migration behavior for existing logs and snapshots. Do not strip values from the actual request sent to the agent.

## R5. Cached documents stay stale after leaving Docs or losing its watcher

**Evidence:** [document state](../crates/adeline_ui/src/docs.rs), `docs_enter` at line 492, `docs_leave` at line 533, `docs_list` at line 576, `docs_open` at line 639, and `docs_connected` at line 1477, plus [engine watcher setup](../crates/adeline_engine/src/engine/docs.rs), `watch_docs` at line 207.

Leaving Docs unsubscribes from document watching but retains the project's `listed` flag and cached document contents. Entering Docs again lists only if the project was never listed. Opening a cached document with a positive `modified` value returns it without rereading it.

When no other client watches that project, the engine drops the filesystem watcher. Changes during that gap have no event to replay. Reconnecting does relist filenames, but the list response does not refresh cached file contents. Re-establishing a filesystem watch does not itself supply the missed changes.

A source-derived reproduction is to open a file, switch to Chats, change the file and create another document outside Adeline, then return to Docs. The old content and old list can remain. A similar stale-content case occurs when the engine or watcher is absent during a connection outage. Dirty buffers are especially dangerous because a later save reaches R3.

**Recommendation:** Treat watch establishment as a reconciliation point. Subscribe and obtain a current file snapshot, then compare revisions for cached documents. Refresh clean buffers and turn changed dirty buffers into explicit conflicts. Relist after an unobserved interval even if the project was listed earlier. Coordinate the snapshot with notifications so changes during reconciliation are not lost.

## R6. A special file can block the entire engine command loop

**Evidence:** [engine document I/O](../crates/adeline_engine/src/engine/docs.rs), `read` at line 56, `image` at line 122, and `docs_command` at line 147, [path resolution](../crates/adeline_docs/src/files.rs), `resolve`, and [engine dispatch](../crates/adeline_engine/src/engine/serve.rs), lines 115–130.

Document and image reads check the pathname and metadata length, then call `std::fs::read`. They do not require a regular file. A Unix FIFO can have a permitted name such as `wait.png`, remain inside the project, and report a size of zero. Referencing it as an image passes these checks and can block waiting for a writer indefinitely.

The read runs inside `block_in_place` from the single engine dispatch task. That allows Tokio to run other tasks, but it does not let this dispatch loop advance. All clients' commands, agent event handling, cancellation, and shutdown processing wait behind the blocked read. A slow directory traversal also holds the same dispatcher, although it normally finishes.

This requires a special file in the working directory, for example one created by a local tool. An ordinary Git checkout does not create a FIFO by itself. The metadata size checks also do not bound bytes read if a regular file grows between the check and read.

**Recommendation:** Reject non-regular files before attempting a potentially blocking open, use safe open behavior where needed, validate the opened handle, and bound actual bytes read. Run document reads and directory scans as owned background operations that return results to the dispatcher. Moving the same unbounded read to a detached worker alone would only move the leak, so bound concurrency and cancellation too.

## R7. Editing a queued message discards its attachments

**Evidence:** [engine queue handling](../crates/adeline_engine/src/engine/turns.rs), `take_queued_parts` at line 187 and `take_queued` at line 203, and [UI queue editing](../crates/adeline_ui/src/runtime_ui.rs), `take_queued` at line 259.

`take_queued_parts` removes both the queued text and the attachment payloads. `take_queued` immediately discards the returned files with `let (text, _)` and returns only a JSON string. The UI restores only that text to the composer.

Queue a message with a document or image while the agent is busy, then choose Edit on that queued message. The attachment disappears from the queue and is not restored to the draft. Sending the edited message silently sends less context than the original message contained.

**Recommendation:** Return a typed queued-prompt value containing text and attachments, and restore both to the originating draft. Make removal without editing explicit so it does not share a lossy return contract. Keep the complete queued prompt under one owner instead of splitting its text and bytes between separate structures. Update `PROTOCOL` if the reply contract changes.

## R8. A delayed Send response clears attachments added afterward

**Evidence:** [send callback](../crates/adeline_ui/src/runtime_ui.rs), `send_real`, lines 144–225, and [attachment input](../crates/adeline_ui/src/conversation_ui.rs), `attach` at line 629.

Sending clones the current prompt and attachments into a request. The completion callback preserves text typed since sending by comparing the current text with the submitted text. It unconditionally calls `app.attachments.clear()`, however, without identifying which draft or attachment set the response acknowledges.

On a slow remote connection, send a prompt and then attach a new file before the response arrives. The successful response removes the new file even though it was not included in the request. Navigation can also make the callback act on the currently displayed draft rather than the draft it submitted. The UI's attachment path has no pending-send guard that prevents this interleaving.

**Recommendation:** Give drafts and attachments stable identities. Capture the originating draft and submitted attachment IDs, then remove only that submitted content on acknowledgment. Preserve additions and edits made while the request was in flight. The text comparison should also be scoped to the originating draft rather than only comparing strings.

## R9. Connection cancellation starts too late to cancel SSH setup

**Evidence:** [UI connections](../crates/adeline_ui/src/client.rs), `open_inbox` at line 417 and `connect` at line 449, and [SSH setup](../crates/adeline_core/src/remote.rs), `connect` at line 89, `run` at line 332, and `bridge` at line 645.

The UI creates a cancellation receiver when it starts a connection, but passes it to `run_io` only after `remote::connect` finishes. Probing, installation, authentication, and the bridge's initial Welcome wait run before cancellation is observed.

The setup helpers also have unbounded waits. `run` reads stdout to EOF and waits for the child, and `bridge` waits for a Welcome line. SSH's transport connection timeout does not bound a remote command that is connected but never finishes.

If a remote shell startup command hangs or a bridge stays alive without replying, disconnecting or removing the machine does not stop that setup attempt. Retrying can start another attempt while the old one remains. A hung first destination prevents trying subsequent destinations. Epoch checks keep stale responses from updating the visible connection, but do not cancel the work itself.

**Recommendation:** Apply cancellation to the complete connection lifecycle, including probes, downloads, installation, authentication, and initial protocol negotiation. Give noninteractive phases deadlines and retain ownership of their child processes and helper tasks so cancellation stops and reaps them. Treat time spent waiting for an intentional user authentication response separately from a stuck machine.

## R10. Raw agent traffic grows without a retention limit in memory and on disk

**Evidence:** [traffic recording](../crates/adeline_engine/src/engine/events.rs), `traffic` at line 387, [transcript persistence](../crates/adeline_engine/src/storage/disk.rs), `flush_events` at line 72 and `load_conversation` at line 137, [traffic retrieval](../crates/adeline_engine/src/storage.rs), `traffic` at line 840, and [UI cap](../crates/adeline_ui/src/runtime_ui.rs), `TRAFFIC_CAP` at line 20.

Raw ACP messages and stderr are added to the same permanent event history used for conversations, whether or not diagnostic traffic is being viewed. Flushing moves them into `StoredConversation.events` rather than evicting them. Loading a conversation reads its whole transcript and retains all parsed events.

The per-line truncation and 5,000-entry UI cap do not limit total engine retention. The retrieval limit only limits the result returned to a viewer. Long or chatty sessions accumulate raw protocol copies in addition to semantic messages, tools, and attachment data. Historical traffic consequently increases both disk use and the memory and I/O required to load the workspace.

No performance numbers were measured in this review. The finding is the unbounded retention path, not a claim about a particular current startup time.

**Recommendation:** Separate replayable conversation events from optional diagnostic traffic. Give traffic a bounded in-memory buffer and an explicit disk retention or rotation policy, and load older diagnostics only on demand. Bound incoming line sizes before allocating entire lines where practical. Preserve the semantic history required for resume and display without making every debug byte permanent.

## R11. Deleting a project leaves attachment payloads in engine maps

**Evidence:** [engine state](../crates/adeline_engine/src/engine.rs), lines 189–220, [payload storage](../crates/adeline_engine/src/engine/turns.rs), `deliver` at line 114 and `queue` at line 123, and [project deletion](../crates/adeline_engine/src/engine/projects.rs), `finish_deletions` at line 68.

Each delivered prompt is copied into the `attachments` map by conversation ID. Queued attachment bytes live in a separate `queued_files` map keyed by queue ID, while the queue's text and filenames live in `Live`.

After stopping a project's drivers and deleting its storage, `finish_deletions` removes the project's `Live` values and workspace entries. It does not remove the associated entries from `attachments` or `queued_files`. There is no removal path for the former map, and removal from the latter occurs when queued items are taken or sent, not when their project is deleted.

Repeatedly creating, using, and deleting projects therefore retains attachment bytes until the engine exits. Deleting a project with unsent queued files also discards the queue entries needed to locate those files through normal operations. This is both a memory leak and a failure to release private data after deletion.

**Recommendation:** Add one removal operation that owns cleanup for a conversation and use it for deletion and reconciliation. Then move conversation-specific transient state under one owner: live state, driver, retry attachments, steering, and queued prompts. A queued prompt should own its attachment payloads. This would remove several cross-map invariants and make deletion much harder to implement incompletely. Avoid a broader engine rewrite just to achieve that ownership boundary.

## R12. Whole-settings replacement can silently undo unrelated changes

**Evidence:** [Docs settings synchronization](../crates/adeline_ui/src/docs.rs), `docs_sync_engine` at line 1502, [settings commands](../crates/adeline_ui/src/settings/engine.rs), `save_engine_settings` at line 19 and `save_mcp_servers` at line 225, and [engine replacement](../crates/adeline_engine/src/engine/projects.rs), `set_settings` at line 244.

Clients update individual controls by cloning the last received `EngineSettings`, modifying one part, and sending the entire object. The engine persists and adopts that object without a revision check or field-level merge.

For example, two clients start with the same settings. One updates MCP servers. Before the second client receives that change, it synchronizes its Docs settings using its old snapshot. Its request restores the old MCP server list. Rapid changes on a delayed connection can produce the same problem in a single client. Docs synchronization is particularly surprising because changing a document-view preference can overwrite unrelated engine configuration.

**Recommendation:** Send narrow commands or typed patches for independent settings, and apply them to the engine's current state. For replacement of an edited collection such as the MCP server list, use an expected revision and report conflicts. Do not rely on optimistic local cache changes alone, since multiple clients remain valid. Bump `PROTOCOL` for the changed wire contract.

## R13. Markdown image loading is incomplete and inconsistent between renderers

**Evidence:** [application startup](../crates/adeline_ui/src/lib.rs), `open` at line 951, and [Markdown rendering](../crates/adeline_ui/src/docs/viewer.rs), `nested` at line 202, `render_alert` at line 210, `render_task` at line 251, and `markdown` at line 460. Dependency checks used the locked `gpui-pre-platform 0.3.7`, `gpui-pre 0.3.7`, and `gpui-base 0.7.0` sources.

There are two connected problems:

1. Desktop startup calls `application().with_assets(Assets).run(...)` without installing an HTTP client. The desktop platform constructor uses GPUI's default `NullHttpClient`, whose `send` returns “No HttpClient available.” No later application or kit initialization installs one. Enabling remote images makes the top-level image resolver return a URI resource, but does not provide the HTTP client needed to load it. The setting therefore cannot make HTTP images work in this desktop build.
2. Alert and task bodies create fresh Markdown views through `nested`. These views receive neither the outer Markdown extensions nor its project-relative image resolver. A local image that works in an ordinary paragraph fails when put inside an alert or task. Remote images in those bodies also skip the disabled-image link policy.

For example, compare an ordinary `![diagram](diagram.png)` with the same image inside `> [!NOTE]` or `- [ ]`. The nested views resolve the image through the dependency's default URL resource path instead of the project's engine-backed image store.

The missing HTTP client currently prevents an actual HTTP fetch through the nested path too. This review therefore does **not** claim an active remote-image privacy leak. However, adding an HTTP client alone would make the nested path capable of fetching remote images while the outer policy says they are disabled.

**Recommendation:** Give all Markdown views one shared rendering policy, including project context, image resolution, and remote-image permissions. Make nested rendering inherit it. If remote images are supported, install the required client while ensuring that disabled images cannot reach it. Fix both paths together rather than only making the top-level setting work.

## R14. Empty attachment content is mistaken for missing content

**Evidence:** [attachment loading](../crates/adeline_core/src/conversation.rs), `Attachment` at line 367 and `read_attachment` at line 383, and [attachment decoding](../crates/adeline_engine/src/acp/turns.rs), `attachment_bytes` at line 445.

Reading a valid empty file produces `size: 0`, an empty base64 string, and `path: None`. The UI accepts it. The engine interprets an empty `data` string as “read from path,” then reports that the attachment has no content because there is no path. This affects both embedded-file conversion and temporary-file fallback.

**Recommendation:** Represent absent data separately from present empty data. A typed payload such as inline bytes versus engine-local path would remove the ambiguity. A smaller fix can explicitly accept a valid zero-length inline attachment, provided validation still distinguishes it from missing nonempty content. If the serialized representation changes, update `PROTOCOL`.

## Additional simplification: share transcript event application

**Evidence:** [persisted replay](../crates/adeline_engine/src/storage.rs), `StoredConversation::to_thread` at line 116, and [live updates](../crates/adeline_core/src/protocol.rs), `apply` at line 531.

Persisted replay and live protocol handling independently implement the effects of messages, text and thought chunks, tools, read state, usage, and cards. They already share some useful methods such as `Thread::stream`, `apply_tool`, and `put_card`, but still duplicate dispatch and state-transition rules. Storage additionally decodes string event names and untyped JSON field by field.

This is a concrete maintenance cost: a new event or semantic change must be implemented in multiple paths for the conversation to behave the same before and after reload. I did not establish an additional user-visible divergence that warrants a separate bug finding.

**Recommendation:** Introduce a small typed conversation-event layer with one application function for the shared thread state. Adapt persisted events and live deltas into it. Keep connection status, transport bookkeeping, storage recovery, and genuinely different lifecycle concerns outside that function. Migrate incrementally rather than introducing a general event framework.

The other high-value simplifications are part of the fixes above: one conversation owner in R11, one Markdown rendering policy in R13, and explicit complete prompt payloads in R7/R14. They address observed bugs rather than merely reducing line counts.

## Dead and unused code

I did not establish a substantial production-code block that is clearly safe to delete. Public APIs, platform-specific branches, generated assets, compatibility decoding, and demo paths need more than a low reference count to prove they are unused. The ownership and duplication findings above are stronger cleanup targets. There are no recommendations here to remove code solely because a search found few references.

## Suggested order

Fix R1 and R2 to protect private files, and R3 to protect user edits. Address credential copying in R4 alongside the retention changes in R10. Then repair the document reconciliation and attachment flows, followed by connection cancellation and settings concurrency. Keep the shared-renderer change coupled to enabling HTTP images. The ownership and event-application simplifications can be done in small steps alongside the affected fixes.

Only this report was added. No fixes, version changes, application tests, or rebuilds were performed.
