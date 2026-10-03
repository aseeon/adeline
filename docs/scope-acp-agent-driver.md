Status: Confirmed

# ACP agent execution and persistent conversations

## Purpose and context

Adeline's local user needs to send real messages to configured agents and continue those conversations across application restarts. ACP is the first agent driver; OMP is the required test harness. The concrete example is an agent named Josh with command `omp.exe`, arguments `["acp"]`, and the saved provider/model, effort and instructions. The first Send creates a conversation in the selected project, starts its agent, sends the message and streams a formatted response.

This scope supersedes the execution exclusion and combined-command requirement in `docs/scope-agents-mvp.md`. Existing agent-management and demo behavior remains applicable except where explicitly changed here. This document captures required behavior, not architecture, implementation tasks or delivery phases.

Repository evidence inspected on 2026-09-26:

- `src/agents.rs`, `AgentDefinition` and `AgentDefinition::validate`: agents have a single command string, required provider/model and effort, optional instructions, and OMP/ACP validation. Definitions are stored in individual `agent.yml` files, not a shared `agents.yml`.
- `src/agent_form.rs`, `FIELDS` and `AgentForm::values`: creation and editing expose one Startup command field.
- `src/interaction.rs`, `Adeline::act` and `Adeline::send`: normal-mode Send is blocked; New Chat opens an empty composer; demo Send creates a thread on the first message and generates a canned reply. Complete currently toggles a status without any process lifecycle.
- `src/data.rs`, `Config`, `Thread` and `Message`: projects have names but no working directory; conversations have no bound agent definition or durable runtime configuration.
- `src/chat_render.rs`, `Adeline::chat_card`, `message_row` and `composer_view`: conversation states and message/composer rendering are existing integration points. The card labels working as Processing, blocked as Attention, and other unfinished conversations as Active.
- `src/config.rs`, `Chats`: existing Chats settings include visibility settings but no retry limit.
- `README.md`, Agents and Demo mode: real definitions persist, while existing normal-mode projects and conversations are not durably stored; demo behavior is simulated and isolated from real definitions.

## Research and rationale

The useful ideas from Zed are a persistent process connection, protocol initialization and capability negotiation, routing events to their session, streaming updates independently of the final prompt result, handling permission requests, retaining diagnostic information, and treating cancellation separately from process shutdown. Zed uses the ACP Rust library and separates protocol I/O from foreground UI work. Its broader editor integrations are not automatically requirements for Adeline. See [`AcpConnection::stdio`, `client_builder`, `AcpSession`, and `cancel`](https://github.com/zed-industries/zed/blob/main/crates/agent_servers/src/acp.rs).

ACP defines session creation with an absolute working directory, capability-dependent session restoration, streamed session updates and prompt cancellation. Restoration can replay history, so restoring must not duplicate the displayed conversation. See [session setup](https://agentclientprotocol.com/protocol/v1/session-setup) and [prompt turns](https://agentclientprotocol.com/protocol/v1/prompt-turn). Permission choices are supplied by the harness; Adeline must describe their actual scope rather than inventing unsupported permission grants. See [tool calls and permissions](https://agentclientprotocol.com/protocol/v1/tool-calls).

OMP's inspected source advertises session loading, resuming and closing, and exposes model and thinking configuration. Cancellation normally ends a turn while preserving the process; its own cleanup failure can close a session, which Adeline must treat as a recovery case. See [`AcpAgent`](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/modes/acp/acp-agent.ts). OMP serves ACP over standard input/output and handles disconnect teardown; diagnostic output is separate from protocol output. See [`runAcpMode`](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/modes/acp/acp-mode.ts).

OMP distinguishes adding instructions from replacing its default instruction template. This scope requires adding instructions while retaining harness defaults. Its documented append route supports that behavior; the delivery mechanism must be verified against the installed harness rather than assuming a universal ACP system-instructions field. See [OMP system prompt customization](https://github.com/can1357/oh-my-pi/blob/main/docs/system-prompt-customization.md).

These findings support a small implementation that honors negotiated capabilities and handles the complete lifecycle required below. Source inspection is not an end-to-end compatibility test of the installed OMP executable.

## Requirements

### Agent configuration and conversation creation

- **R1.** Implement real agent execution through ACP, with OMP as the required harness. Use the configured provider/model and effort for the conversation. Replace normal-mode Send's no-op behavior with real prompting. Keep the existing isolated, simulated demo experience.
- **R2.** Replace the combined startup command with required Command and optional Arguments in agent YAML and both creation and editing UI. Command contains only the executable name or path. Arguments is an ordered list of literal strings, editable as individual entries. Preserve spaces inside each value; do not insert prefixes, subcommands or split entries on whitespace. An omitted Arguments value means no arguments. The test definition is `command: omp.exe` and `arguments: [acp]`.
- **R3.** Migrate existing combined-command definitions to the separate fields, preserving their intended executable and arguments. Identify ambiguous definitions for correction rather than silently changing what runs. The existing `omp.exe acp` example must migrate successfully.
- **R4.** New Chat opens a draft composer. The first nonempty Send creates the conversation and starts its process after agent selection. When exactly one agent is available, select it automatically; with multiple agents the user must choose. Without an available agent, require agent creation or selection before sending.
- **R5.** Bind each conversation to its chosen agent and save the initial execution configuration, including command, arguments, name, provider/model, effort, system instructions and working directory. Do not allow changing agents or these execution settings during that conversation. Later agent or project edits apply to new conversations. Permission mode is the explicit mutable exception in R22.
- **R6.** Append `You are an agent named <agent-name>` to the harness's default system instructions, then append the configured system instructions when present. Always include the name sentence, including when custom instructions are nonempty. Treat this as system guidance, not an ordinary user message, and retain the harness defaults.

### Projects and durable data

- **R7.** Store each project under `~/.config/adeline/projects/<normalized-project-name>/`. Its `project.yml` contains exactly two fields for this scope: `name` and `directory`. The directory field is the agent's working directory, distinct from Adeline's configuration folder.
- **R8.** Reuse the agent folder-naming rules for projects: lowercase the name, convert spaces and punctuation to hyphens, collapse repeated hyphens, remove leading/trailing hyphens, and reject empty, invalid or colliding folder names. Renaming a project also renames its configuration folder while preserving its conversations.
- **R9.** Provide project settings for editing name and working directory, and confirmed project deletion. Require an existing working directory when creating or editing a project. If it later disappears, retain the project and history but block agent startup with an actionable error.
- **R10.** Permit changing a project's working directory only when it has no Processing, Attention or Active conversations. An unfinished conversation waiting for the next user message is still Active. All conversations must therefore be completed or archived before changing the directory. Previously created conversations retain their own saved directory when reopened.
- **R11.** Create `conversations/` inside each project configuration folder. Each conversation occupies its own folder with a unique generated name and contains `conversation.yml` and `transcript.jsonl`. Conversation settings, restoration information and special states, including completed and archived, persist in `conversation.yml`.
- **R12.** Persist the full conversation event history in `transcript.jsonl`: raw ACP requests, responses and notifications in both directions, tool calls and results, permission decisions, user-visible messages, timestamps and lifecycle events. Include errors affecting the conversation, including failures that cause retries. Routine diagnostic output does not have to be retained merely because it appears on stderr.
- **R13.** Reload saved projects, conversation settings, states and transcripts at application startup. Preserve readable conversation history without requiring a running agent. Changes made through Adeline appear immediately. Live synchronization of externally edited project or conversation files is not required; load those changes at startup. Existing agent-definition watching remains applicable.
- **R14.** Project deletion removes Adeline's project record and its saved conversations, leaving the actual working directory untouched. The confirmation must explain that running agents will stop and saved conversations will be deleted. Gracefully stop the project's agents before deleting its saved data. If shutdown is stuck, retain the project until shutdown succeeds or the user explicitly forces termination under R21.

### Prompting, streaming and lifecycle

- **R15.** Give each live conversation its own agent process and session. Keep processes running between turns and when the user switches conversations or closes a project tab. Different conversations must retain independent messages, execution state, permissions and cancellation.
- **R16.** Display the user's submitted message and a generic Thinking indicator while awaiting response content. Stream assistant text into the conversation incrementally and update its formatting as content arrives, without waiting for the complete response. Preserve partial content after cancellation or failure. Keep the UI usable during startup, prompting, retries and streaming.
- **R17.** Show tool calls, their status and results in expandable conversation entries. Add a Chats setting to hide tool calls entirely from the visible conversation. Hiding them must not remove transcript records or hide permission requests. Use generic Thinking rather than displaying reasoning text.
- **R18.** While the current prompt is processing, replace Send with Stop using a square icon. Stop requests cancellation of that turn and pending retries while keeping the agent process running. Once cancellation settles, another prompt can use the same conversation. Cancel pending permission interactions for the stopped turn; late events must not corrupt a subsequent turn.
- **R19.** Marking a conversation Completed or archiving it cancels pending work and retries and gracefully closes its agent. Preserve its transcript and settings. These actions affect only that conversation.
- **R20.** On reopening a completed or archived conversation, restart its agent on the next Send and restore its prior context. The same restoration behavior applies when continuing after an application restart. Do not automatically restart agents or resubmit interrupted prompts merely because Adeline starts or a saved transcript is viewed. Application exit gracefully stops its running agents. (Replaced for application exit by R7–R9 of `docs/scope-conversation-engine.md`.)
- **R21.** Prefer graceful shutdown for completion, archiving, deletion and application exit. If the agent does not cooperate, show that shutdown is stuck and offer an explicit Force Stop action. Never silently escalate to forceful termination or report that cleanup succeeded while the agent remains running.

### Permissions

- **R22.** Support Ask and Allow everything permission modes. Each conversation initially copies its agent's default permission mode, saves its own mode and allows that mode to be changed during the conversation. Changing permission mode does not change its bound agent or other saved execution settings.
- **R23.** In Ask mode, show the request and actual permission scope offered by the harness. Support Allow once, Allow for the conversation when the harness offers it, and Deny once. Do not expose permanent denial. If the harness lacks a conversation-wide grant, show only supported grants rather than inventing one. A denial must resolve that request without being treated as a transient failure to retry.
- **R24.** In Allow everything mode, automatically approve permission requests. Persist this conversation mode across process and application restarts. Preserve individual conversation-wide grants across restarts only when the harness supports restoring them; otherwise ask again rather than silently broadening access.

### Failure and recovery

- **R25.** Surface startup, communication and prompt failures with a clear error. Preserve the submitted prompt, received response content and related events. Distinguish recoverable failures from missing configuration, missing authentication and user-denied requests.
- **R26.** Automatically retry temporary failures, including temporary connection or provider failures, up to five additional attempts after the initial attempt. Make the limit editable in Chats settings; zero disables automatic retries. A successful operation resets its retry count. Show retry progress and stop automatic attempts when exhausted. Stop, Complete, Archive, project deletion and application exit cancel pending retries.
- **R27.** Do not automatically retry invalid configuration, missing authentication or denied permission. For authentication failures, show instructions to authenticate through the harness outside Adeline, then offer manual retry. An embedded login flow is not required.
- **R28.** When a temporary failure interrupts a turn that has performed work, restore its session and ask the agent to continue the interrupted turn, preserving completed work. Do not blindly resend the original prompt or replay recorded tool actions. This is a continuation policy, not a guarantee that a harness can never repeat an action.
- **R29.** If the harness cannot restore the session, explain the problem and offer to start a new session using saved conversation content as context. Proceed only after the user chooses that recovery. Keep existing history and distinguish the replacement session from the original session. Loading or replaying saved context must not duplicate visible messages.
- **R30.** If transcript persistence fails, block new prompts, request cancellation of current processing, preserve unsaved events in memory and show a recoverable storage error. Do not silently continue processing without durable history or falsely report unsaved events as saved.

### Constraints and quality requirements

- **R31.** Negotiate ACP compatibility and capabilities before using optional operations. Report unsupported required behavior clearly instead of silently substituting different execution settings. Keep session events associated with their correct conversation and keep protocol traffic separate from process diagnostics. Implement only the client capabilities that can actually be honored.
- **R32.** During implementation, run the repository's required lint, checks and tests, then rebuild the runnable app and confirm build success, as required by `AGENTS.md`. Documentation-only scoping changes do not require application tests or a rebuild.

## Domain and data

- An agent definition is a reusable saved configuration. A conversation retains its initial execution settings independently of subsequent definition edits.
- A project groups conversations and supplies the initial working directory. Its configuration folder is not its working directory.
- A conversation owns its saved settings and event history. Its generated folder name is its stable storage identity, independent of its displayed title.
- A process is the running harness instance for a conversation. A session is the harness's conversation context, which can outlive a process if the harness supports persistence.
- Processing means a prompt is in progress, Attention means user intervention is needed, and Active includes an unfinished conversation waiting for another prompt. Completion and archiving close the process without deleting history.
- Permission mode belongs to the conversation after copying the agent default. A grant's actual scope comes from the harness; transcript storage does not itself make a grant restorable.
- A retry belongs to a failed operation or interrupted turn. It does not create a duplicate user message or authorize replaying tool actions from the transcript.

Required storage example:

```text
~/.config/adeline/
  agents/
    josh/
      agent.yml
  projects/
    example-project/
      project.yml
      conversations/
        <unique-generated-name>/
          conversation.yml
          transcript.jsonl
```

Example project definition:

```yaml
name: Example Project
directory: C:/work/example-project
```

Required command split in the agent definition:

```yaml
command: omp.exe
arguments:
  - acp
```

A longer argument example is `["acp", "--arg1", "value with spaces", "--arg2", "value with spaces2"]`. Every entry is passed literally as one argument. The list editor must support adding, removing and ordering these entries.

## Interfaces and dependencies

Affected interfaces are agent creation and Settings, project creation and Settings, the new-conversation composer and agent picker, conversation rendering and tool entries, permission requests and mode selection, Send/Stop, Completed and Archive, saved-history loading, and Chats settings for retries and tool visibility.

The execution dependency is the locally installed configured harness and its ACP capabilities. OMP authentication is managed through the harness. Adeline must retain enough saved information to request restoration, while acknowledging that restoration also depends on the harness retaining its own session data.

## Boundaries

- ACP is the first driver and OMP is the required integration example. This scope does not promise support for every ACP harness or every capability exposed by Zed.
- Changing the bound agent, model, effort, system instructions or working directory of an existing conversation is excluded for this scope. Permission mode remains editable.
- Live external synchronization of project and conversation files, embedded authentication, automatic creation of missing working directories and permanent denial choices are excluded by user decisions.
- The complete ACP event history is retained; the visible UI uses generic Thinking rather than exposing reasoning text.
- This scope does not require saving routine stderr diagnostics unrelated to conversation failures.
- Existing demo isolation remains in effect. Real execution and persistent user history must not turn demo actions into real agent work.

### Rejected ideas

- Starting a process merely on New Chat rather than first Send.
- Replacing harness default instructions or omitting the name sentence when custom instructions exist.
- A single free-text Arguments field, automatic `--` prefixes, or an implicitly inserted `acp` argument.
- Agent switching or applying edited execution settings midway through an existing conversation.
- Automatically rebuilding context in a new session without explaining failed restoration and obtaining the user's choice.
- Retrying authentication, invalid configuration or permission denials automatically.
- Blindly resending the original prompt after partial execution.
- Silent forceful shutdown, or deleting a project's saved data while its agent has not stopped.
- Continuing agent processing after transcript storage fails.
- Inventing unsupported conversation-wide permission grants.

## Acceptance criteria

- **AC1 (R1, R4, R15).** With Josh configured for OMP/ACP, New Chat does not launch OMP. The first nonempty Send creates the conversation, launches the configured agent and obtains a real response. With one agent selection is automatic; with multiple agents a choice is required; with none no prompt starts. Demo Send remains simulated.
- **AC2 (R2, R3).** Both agent forms expose Command and an ordered Arguments list. Missing Command fails validation, an empty list is valid, and a value containing spaces reaches the process as one argument. `omp.exe acp` migrates to `omp.exe` plus `["acp"]`; ambiguous legacy input is reported without changing its meaning.
- **AC3 (R5, R6).** A new conversation uses its saved model and effort, and its harness receives the name sentence followed by any custom instructions while retaining harness defaults. Subsequent agent edits do not change that conversation's execution settings, including after restarting its process.
- **AC4 (R7, R8, R9).** Creating Example Project writes the normalized project folder and a `project.yml` containing only name and directory. Names follow the established normalization and collision rules. Project settings can rename the project and configuration folder without losing conversations. A nonexistent working directory cannot be saved as a new selection.
- **AC5 (R10).** Any Processing, Attention or Active conversation blocks project directory changes. When all are completed or archived, the directory can change. New conversations use the new directory and reopened old conversations retain their saved directory.
- **AC6 (R11, R12, R13).** A first Send creates a uniquely named conversation folder with both required files. After restarting Adeline, settings, special states, messages, tool history and errors remain available. The event log includes ordered raw ACP traffic, timestamps, lifecycle events and permission decisions. The visible transcript is not duplicated by restoration replay.
- **AC7 (R13).** Project and conversation changes made through Adeline appear immediately and survive restart. External changes load at startup; no live watcher for these files is required. Existing agent-definition refresh behavior is retained.
- **AC8 (R9, R25).** If a saved working directory disappears, the project and transcript remain readable; sending reports the missing directory and does not start the agent in a substitute location.
- **AC9 (R14, R21).** Cancelling project deletion preserves everything. Confirming warns of stopped agents and deleted conversations, stops agents gracefully and then removes only Adeline's saved project data. The working directory is untouched. An unresponsive agent prevents deletion until successful shutdown or explicit Force Stop.
- **AC10 (R15, R16).** Two conversations can run independently. Switching conversations or closing a project tab does not stop either. Each displays only its own updates. Thinking appears while awaiting content, text and formatting update before the turn finishes, and the interface remains responsive.
- **AC11 (R17, R12).** Tool calls show expandable status and results. Enabling the hide-tool-calls setting removes those entries from view while keeping transcript records and actionable permission requests. Reasoning content is not rendered as a reasoning transcript.
- **AC12 (R18).** During a prompt, Send becomes a square Stop control. Stop cancels processing and retries, retains partial text and keeps the process alive under normal cancellation. A later Send uses the same conversation. Pending permission requests and late updates from the stopped turn cannot affect a new turn.
- **AC13 (R19, R20, R21).** Complete and Archive gracefully close only the affected conversation's agent and preserve history. Reopening and sending restores context. Viewing saved history or launching Adeline starts no agent or interrupted prompt automatically. Exiting stops live agents gracefully; stuck shutdown is reported and force is an explicit choice.
- **AC14 (R22, R24).** A new conversation copies its agent's permission mode. Changing that conversation's mode affects subsequent requests and persists across restart without changing other execution settings or other conversations.
- **AC15 (R23, R24).** Ask mode shows the harness-provided request and supported grant scopes, including one-time denial. Unsupported conversation-wide grants and permanent denial are not offered. Allow everything approves requests automatically. Individual grants are restored only when supported; otherwise another request asks again.
- **AC16 (R25, R26).** A temporary failure preserves the prompt and partial response, records the failure and visibly retries at most five additional times by default. The Chats setting changes the limit; zero disables automatic retries. Success resets the operation's count and exhaustion leaves an actionable error rather than an endless retry loop.
- **AC17 (R26, R27).** Stop, Complete, Archive, project deletion and application exit prevent pending retries from restarting work. Invalid configuration, denied permission and missing authentication do not auto-retry. Authentication errors direct the user to the harness and permit manual retry after authentication.
- **AC18 (R28, R29).** After interrupted tool work, recovery restores the session and requests continuation rather than replaying the original prompt or stored tool actions. If restoration is unavailable, Adeline explains that and offers a new session using saved context; declining does not start that replacement session.
- **AC19 (R30).** A transcript write failure blocks new prompts, requests cancellation, retains unsaved events in memory and displays a recoverable storage error. It does not falsely report those events as saved.
- **AC20 (R31).** Unsupported protocol versions or required capabilities produce a useful error. Optional restoration and shutdown operations are used only when supported. Protocol and diagnostic output remain separate, and events are routed to the owning conversation.
- **AC21 (R32).** The implementation's required lint, checks and tests pass, and the runnable app rebuild succeeds before implementation completion is reported.

## Decisions and rationale

- Q1-Q5: First Send starts execution; projects gain a directory; always append the agent-name sentence and then custom instructions; support Ask and Allow everything; split and migrate commands.
- Q6-Q12: Permission mode belongs to a conversation and starts from an agent default; use harness-offered permission scopes; bind the agent; restore context on continuation; persist projects and complete conversation events; add bounded retries; keep agents running across navigation and project-tab closure.
- Q13-Q19: Retries are automatic; failed restoration requires an explicit recovery choice; snapshot execution settings; restrict directory changes to projects without unfinished conversations; normalize project folders; support project maintenance; retain protocol events and conversation-impacting errors; expose only supported grants.
- Q20-Q26: Retry temporary failures only, with five additional attempts and zero disabling retries; delete projects after graceful agent shutdown; authentication retries are manual; show expandable tool calls with a hide setting; Arguments is an ordered literal list; restore grants only when supported; require existing working directories.
- Q27-Q31: `acp` must be an explicit argument; stuck shutdown offers explicit Force Stop; retries continue restored work; transcript failures stop further processing; external project/conversation changes load at startup.
- The user's initial numbered request and these answers together define the scope. Later answers supersede earlier alternatives, including the original conditional agent-name sentence and the initially proposed manual-only retry policy.

## Open questions

None. The user confirmed the complete scope and acceptance criteria on 2026-09-26.
