Status: Implemented in v0.1.0. This scope is history. Where it and the code differ, the code is right.

# GPUI migration to gpui-pre 0.3.6

## Purpose and context

Move Adeline from GPUI 0.2.2 to exactly gpui-pre 0.3.6 so future work can use gpui-whiteboard and GPUI Kit components. The migration serves maintainers preparing those integrations and existing users whose working project, chat, and settings workflows must continue unchanged.

The migration itself adds no whiteboard functionality and adopts no GPUI Kit components. Compatibility with those libraries is the required outcome.

### Repository evidence

- `Cargo.toml`, `[dependencies]`: the current dependency is exactly `gpui = "=0.2.2"`.
- `src/main.rs`, `main`: starts the GPUI application, initializes assets, fonts, settings, themes and input, opens the main window, and routes close requests through application shutdown handling.
- `src/project_ui.rs`, `Adeline::create_project` and `Adeline::save_project_settings`: real projects use the persistent project store and surface validation or storage errors. Demo projects follow a separate path.
- `src/runtime_ui.rs`, `Adeline::watch_runtime`: restores saved conversation runtime state, permission modes and execution configuration, and exposes interrupted-turn and storage errors.
- `src/config.rs`, `Settings`, `Appearance`, `Keymap`, `init` and `update`: application preferences include appearance and keyboard settings; updates re-read saved settings to preserve hand edits and reject malformed files.
- `src/storage_tests.rs`, `project_rename_preserves_history_and_directory_snapshot`: covers project validation, name conflicts, saved project fields, restrictions on changing an active project's directory, and preservation of conversation execution settings.
- `README.md`, "Agents" and "Projects and conversations": describes the existing real-agent chat, permission, recovery, history and shutdown behavior being preserved.
- `README.md`, "Platform setup" and "Validation": documents Windows, macOS and Linux support, including X11 and Wayland, and the existing repository checks and release builds. Windows is the documented locally built and visually exercised platform; this scope does not claim that other platforms were exercised during scoping.

### External compatibility evidence

Checked on 2026-09-27:

- [GPUI Kit 0.6.6](https://docs.rs/crate/gpui-kit/0.6.6) is the current published release and pins `gpui-pre =0.3.6` and `gpui-pre-platform =0.3.6`.
- [gpui-whiteboard 0.5.1](https://docs.rs/crate/gpui-whiteboard/0.5.1) declares `gpui-pre ^0.3`, permitting 0.3.6. Its documentation requires the host application and whiteboard to resolve to the same GPUI version.
- These declarations establish compatible version constraints, not observed application runtime compatibility. No application migration or runtime verification was performed as part of this scope.

## Requirements

### Framework and library compatibility

- **R1.** Adeline must use exactly gpui-pre 0.3.6 in place of GPUI 0.2.2. The target is an exact release, not a minimum version or an automatically advancing 0.3.x range.
- **R2.** The migrated application must be compatible with GPUI Kit 0.6.6 and gpui-whiteboard 0.5.1, allowing future use of their components and whiteboard with the application's GPUI entities and windows without another GPUI-version migration or incompatible GPUI types. This change does not require adopting either library into product workflows.

### Existing workflows and data

- **R3.** Preserve working project creation and project settings behavior, including validation, opening created projects, saved names and working directories, project renaming, and existing restrictions on working-directory changes.
- **R4.** Preserve existing agent creation and editing behavior, including saved definitions, validation, and the distinction between settings used for new conversations and the execution configuration retained by existing conversations.
- **R5.** Preserve existing real-chat behavior: starting a conversation, sending messages and follow-ups, streaming responses, viewing tool activity and saved history, switching conversations, permission decisions, cancellation, completion/archive, recovery and graceful process shutdown. Existing permission boundaries and recovery choices must remain unchanged.
- **R6.** Preserve application settings behavior and saved preferences, including themes, interface and code fonts and sizes, keyboard shortcuts, and existing chat settings. Existing users must retain their choices after restarting the migrated application.
- **R7.** Existing saved projects, agent definitions, conversations, transcripts and application preferences must remain usable without manual conversion, recreation or data loss. Migration must preserve existing conversation execution configuration and history.

### Constraints and quality requirements

- **R8.** Preserve Windows, macOS and Linux desktop compatibility, including both X11 and Wayland on Linux. Preserve existing platform-specific window behavior, including the custom Windows title bar and native macOS/Linux decorations.
- **R9.** Preserve current layout, themes, fonts, shortcuts and interactions in the working workflows. Minor framework rendering differences are acceptable; pixel-identical rendering is not required.
- **R10.** The migrated application must continue to meet the repository's required formatting, lint, check, test and runnable release-build requirements. Passing compilation alone is not sufficient to establish preservation of the required user workflows.

### Failure and edge cases

- **R11.** Preserve existing validation and failure handling within the retained workflows. Invalid project or agent input must continue to be rejected with an error; malformed saved settings must not be silently overwritten; chat permission failures, interrupted turns and storage failures must retain their existing user-visible handling and recovery choices. A framework migration must not bypass permission decisions or discard unsaved conversation history to appear successful.

## Boundaries

### User-chosen exclusions

- Implementing whiteboard functionality is outside this scope. It is a reason for the migration, not a deliverable of it.
- Integrating GPUI Kit into product workflows or replacing existing controls with GPUI Kit components is outside this scope. Compatibility is sufficient for this change.
- Completing unfinished or demo-only functionality is outside this migration's preservation requirement. The agreed preservation baseline is the working project, real-chat, agent-management and settings behavior described above.
- The preservation boundary is not permission to remove unrelated existing functionality or saved data.

### Rejected ideas

- Allowing the GPUI target to float across later compatible releases: the user chose exactly 0.3.6 to match GPUI Kit's pin.
- Requiring pixel-identical rendering: the user accepted minor framework rendering differences while preserving layout and interaction.

## Domain and data

- A project has a saved name and working directory. Conversations belong to projects.
- An agent definition supplies configuration for new conversations. Existing conversations retain their saved execution configuration.
- A conversation has persisted history and lifecycle state, with existing permission and recovery behavior.
- Application preferences include appearance, keyboard and mode settings.
- This scope requires preservation of these existing relationships and ownership rules. It introduces no new whiteboard data, ownership rules or persistence behavior.

## Interfaces and dependencies

- The desktop framework changes to gpui-pre 0.3.6.
- GPUI Kit 0.6.6 and gpui-whiteboard 0.5.1 are the compatibility reference versions, not requirements to expose new UI.
- Existing OMP/ACP agent interaction, local project and conversation storage, settings files, fonts and platform window interactions retain their current behavior.
- Choosing dependency declarations, application bootstrap APIs or code organization belongs to implementation work, not this scope.

## Acceptance criteria

- **AC1 (R1):** The application resolves and runs on exactly gpui-pre 0.3.6 rather than the previous GPUI 0.2.2 framework.
- **AC2 (R2):** GPUI Kit 0.6.6 and gpui-whiteboard 0.5.1 can coexist with the migrated application's GPUI version and exchange the GPUI types needed to embed their UI without a version conflict or a second incompatible GPUI type universe. No production component replacement or whiteboard feature is required.
- **AC3 (R3, R7):** A user can create and open a project using a valid existing working directory, restart the application, reopen that project, and change its settings under the existing rules. Renaming preserves its conversation history; an active conversation still prevents a prohibited working-directory change.
- **AC4 (R4, R7):** A user can create and edit an agent and use it for a new conversation. Previously saved agents remain available, and editing an agent does not change the execution configuration already saved for an existing conversation.
- **AC5 (R5, R7):** A user can send a real prompt, receive streamed output and tool activity, send a follow-up, switch away and back, and reopen saved history after restart without losing prior messages or changing conversation configuration.
- **AC6 (R5, R11):** Existing Ask/Allow everything behavior, offered permission choices, Stop, completion/archive, recovery choices and graceful shutdown retain their current effects. Cancellation preserves partial output; interrupted turns and storage failures remain visible and recoverable through their existing paths.
- **AC7 (R6, R7):** Existing settings load without manual conversion; changes to supported preferences still take effect and persist across restart. Saved font, theme, shortcut and chat-setting choices are retained.
- **AC8 (R8):** The migrated application supports the existing working workflows on Windows, macOS and Linux, with both X11 and Wayland retained and each platform's existing title-bar/decorations behavior preserved.
- **AC9 (R9):** Project, chat, agent and settings workflows retain their current layout, selected typography/theme, shortcuts and interaction behavior. Minor rendering differences do not constitute failure; changed workflow behavior does.
- **AC10 (R10):** Required repository checks and tests pass, the runnable release app rebuild succeeds, and the working project/chat/settings behavior is observed in the migrated application. Platform compatibility evidence is distinguished from locally observed runtime behavior.
- **AC11 (R11):** Invalid project paths, conflicting names or invalid agent definitions still produce errors rather than successful saves; malformed settings are not overwritten; existing permission, interrupted-turn and storage-failure protections remain effective.

## Decisions and rationale

- **Q1:** The migration enables future gpui-whiteboard use and GPUI Kit components.
- **Q2:** Success preserves genuinely implemented project creation, chats and settings rather than adding unfinished functionality.
- **Q3:** Preserve existing platform compatibility.
- **Q4:** Pin exactly 0.3.6 because GPUI Kit pins it. Checking the current published GPUI Kit release confirmed that this target remains correct.
- **Q5:** Whiteboard implementation is outside this scope.
- **Q6:** GPUI Kit compatibility is sufficient; adopting components is outside this scope.
- **Q7:** Preserve the connected existing workflows and saved data, including agent creation/editing, project settings, chat history, streaming, permissions, cancellation, recovery and saved preferences, without manual conversion.
- **Q8:** Preserve layout, themes, fonts, shortcuts and interactions; minor framework rendering differences are acceptable and pixel identity is unnecessary.

## Open questions

None.

The user confirmed this scope and its acceptance criteria on 2026-09-27. No open questions remain.
