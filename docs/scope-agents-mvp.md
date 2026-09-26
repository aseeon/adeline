Status: Confirmed
Implemented: Yes

# Agents MVP

## Purpose and context

Adeline's local user needs persistent, configurable agents that can be created, maintained and selected in the application. Normal startup must present user data without predefined demo content. Starting with `--demo` must restore the complete existing demonstration experience.

The concrete creation example is an agent named Josh using harness OMP, driver ACP, command `omp.exe acp`, provider/model `openai-codex/gpt-6-luna`, effort Max, and optional system instructions such as `You are a helpful coding assistant.` Saving creates `~/.config/adeline/agents/josh/agent.yml` and makes Josh available in both agent selectors. Running the harness is outside this scope.

Current repository evidence:

- `src/config.rs`, `config::directory`: resolves the user's home directory and appends `.config/adeline`, including on Windows.
- `src/main.rs`, `Adeline::new`, `AGENTS` and `MACHINES`: startup currently loads demo workspaces and services, hardcoded agents and machines, and seeded collaboration content.
- `src/data.rs`, `load`: reads the bundled workspace fixture.
- `src/collaboration_modes.rs`, `ProjectCollaboration::seed`: creates project-specific collaboration examples.
- `src/interaction.rs`, `Adeline::act`, `Action::AddAgent` and `Action::SaveAgent`: creation currently uses an in-app name dialog and stores names only in memory.
- `src/views.rs`, `Adeline::menu_view`: the Agents selector and composer selector use the same available-agent list; model and effort choices are currently predefined UI options.
- `src/interaction.rs`, `Adeline::send`: sending currently appends a canned demo reply without connecting to a harness.
- `src/settings.rs`, `GROUPS`, `SettingsWindow::render` and `open_at`: Settings has grouped navigation in a separate native window.

## Requirements

### Definitions and storage

- **R1.** Store each real agent's complete definition in its own directory under the current user's Adeline configuration directory: `~/.config/adeline/agents/<normalized-name>/agent.yml`. For this MVP, `agent.yml` is the only definition artifact in that directory. Saved definitions must be available after restarting Adeline.
- **R2.** Each definition contains name, harness, driver, full harness startup command, full provider/model string, default effort, and system instructions. Every field except system instructions is required. OMP with ACP is the only required supported harness/driver combination. Provider/model is entered as a full string. Effort is restricted to Low, Medium, High, Extra High and Max. System instructions are stored for future execution support.
- **R3.** Derive the directory name by lowercasing the agent name, converting spaces and punctuation to hyphens, collapsing repeated hyphens, and removing leading and trailing hyphens. Reject results that are empty, invalid as folder names, or collide with an existing agent folder. Never overwrite another agent because of a name collision. For example, `Josh Smith` becomes `josh-smith`.
- **R4.** Validate the required fields, permitted effort values and folder name when saving. Validation must not launch the command or attempt to verify model or harness capabilities through execution.

### Creation and selection

- **R5.** The existing Add an Agent entry point opens a separate window modeled after the Settings window. It provides all definition fields and a Save button at the bottom.
- **R6.** A successful creation saves the directory and `agent.yml`, immediately adds the agent to both the Agents selector and the composer agent selector, and closes the creation window. Automatically select the new agent only when it is the first agent. Creating subsequent agents preserves the current selection.
- **R7.** Selecting an agent loads the exact configuration saved in its `agent.yml`. The composer displays the saved model and effort and does not provide overrides for them in this MVP. Saved changes to the selected agent immediately update its loaded configuration and displayed values.
- **R8.** If the selected agent is deleted or its definition becomes invalid, clear the selection and show `Select an agent`. Do not silently select a replacement. A normal installation with no definitions has no predefined agents to choose from.

### Editing and deletion

- **R9.** Add an Agents group to the Settings window, with individual agents listed beneath it. Selecting an agent allows all its definition fields to be edited. An explicit Save button persists edits. Saving a changed name also renames the agent's directory using the same naming and collision rules as creation.
- **R10.** Allow agent deletion from Settings after confirmation. Confirmed deletion removes that agent's folder and removes the agent from Settings and both selectors.
- **R11.** When closing a creation or editing window, or leaving an agent's Settings page with unsaved changes, offer Save, Discard and Cancel. Save applies the normal validation and persistence rules; Discard abandons the edits; Cancel keeps the current form and its edits open.
- **R12.** Detect external additions, edits and removals of agent definitions immediately, without requiring a restart or manual refresh. Reflect valid changes in Settings, both selectors and the selected agent's loaded configuration.
- **R13.** If an external change conflicts with unsaved Settings edits, preserve those edits and ask whether to reload the external definition or overwrite it before saving. Do not silently discard edits or overwrite the external change.
- **R14.** For unreadable or invalid definitions, report which file failed and why. Keep valid agents available even when another definition fails. Invalid definitions must not remain selectable as valid agents.

### Normal startup and demo mode

- **R15.** Without `--demo`, start without predefined projects, agents, conversations or other sample workspace content. This applies across the app, including content such as documents, services, workflows and collaboration examples. Load saved user agent definitions normally; an empty initial application does not mean ignoring user configuration.
- **R16.** Preserve all existing demo data in an easily loadable form. Launching with `--demo` restores the existing demonstration content and simulated conversation behavior. Demo mode excludes real user agents. Agents created or edited in demo mode exist only for that run and do not modify real agent configuration.
- **R17.** Normal-mode Send does nothing in this MVP and does not need to be explicitly disabled. It must not execute a harness or generate a simulated reply. Actual agent execution is outside this scope.

## Domain and data

An agent is a named, user-configured definition of a harness, its interface driver, command, provider/model, default effort and optional system instructions. An agent definition is distinct from a running process or conversation. Selection loads the definition but does not start it.

The user configuration directory owns real definitions. Both agent selectors and Settings expose those definitions. Demo agents belong to the current demo run and are isolated from real definitions.

The saved definition is authoritative. Unsaved form edits remain separate until Save succeeds, and conflicts with external changes require the user's choice.

## Interfaces and dependencies

The affected user-facing interfaces are application startup through `--demo`, the existing Add an Agent entry point, the separate creation window, Settings navigation and agent editing, the Agents selector, the composer selector, and filesystem editing of `agent.yml`.

The existing configuration directory convention is retained. The startup command and system instructions are stored configuration for later execution work. This MVP requires no connection to OMP, ACP or a model provider.

## Boundaries

- Actual harness startup, ACP communication, model requests and sending system instructions to a running agent are excluded by the user's Q1 decision.
- OMP through ACP is the only required combination for this scope.
- Model and effort changes are made through agent settings; conversation-level overrides are excluded for now.
- Demo mode does not load real agents or persist changes to its agents across runs.
- This document scopes required behavior. UI design, architecture, implementation planning and implementation are separate work, as requested through the just-scope skill.

### Rejected ideas

- Disabling normal-mode Send with an explanation: the user chose to let it do nothing.
- Free-text effort values: the user chose the five fixed values in R2.
- Always selecting a newly created agent: selection happens automatically only for the first agent.
- Keeping predefined demo content in normal startup: all existing demo content is retained behind `--demo`.

## Acceptance criteria

- **AC1 (R1, R2).** Create Josh with the example values. Its directory contains `agent.yml` with every entered setting, and the definition remains available after restarting without `--demo`.
- **AC2 (R2, R4).** Saving rejects a missing required value or an effort outside the five allowed values. Empty system instructions are accepted. Saving never starts the configured command or contacts a provider.
- **AC3 (R3).** `Josh Smith` produces `josh-smith`; repeated punctuation or whitespace produces a single separator, and leading/trailing separators are removed. An empty or invalid result or existing destination prevents creation or rename without overwriting that destination.
- **AC4 (R5).** Add an Agent opens a separate Settings-style window containing every definition field and a bottom Save button.
- **AC5 (R6).** Successfully saving the first agent closes the creation window, adds it to both selectors and selects it. Saving a later agent closes the window and adds it to both selectors while preserving the current selection.
- **AC6 (R7).** Selecting Josh loads the saved command, harness, driver, provider/model, effort and instructions. The composer displays his saved model and effort without override controls. Saving changes to selected Josh updates those loaded and displayed values immediately.
- **AC7 (R8).** Removing or invalidating the selected definition clears selection and displays `Select an agent`; no other agent is automatically substituted. A user with no saved definitions sees no predefined agents in normal mode.
- **AC8 (R9).** Settings contains an Agents group with individual agents underneath. All fields can be edited and explicitly saved. Renaming updates the folder and agent's displayed name while preserving its other settings.
- **AC9 (R10).** Deletion requires confirmation. Cancelling keeps the agent intact; confirming removes its folder and all corresponding Settings and selector entries.
- **AC10 (R11).** Leaving an unsaved creation or editing form offers Save, Discard and Cancel, with the specified effects. Choosing Save cannot bypass validation.
- **AC11 (R12).** Adding, editing or removing a definition outside Adeline is reflected immediately without restart or manual refresh, including updates to the selected agent when applicable.
- **AC12 (R13).** Edit an agent in Settings without saving, then change its definition externally. The form retains the unsaved edits and asks whether to reload or overwrite before saving.
- **AC13 (R14).** Introduce an invalid or unreadable agent definition alongside a valid one. Adeline identifies the failing file and reason, keeps the valid agent available, and does not offer the invalid definition as a valid agent.
- **AC14 (R15).** Launch without `--demo` and with no user agents. No predefined workspace content appears across the app. After saving an agent, a subsequent normal launch loads that agent without introducing demo content.
- **AC15 (R16).** Launch with `--demo` and verify all preserved demonstration content and simulated conversation behavior are available. Real agents are absent. Demo agent creations and edits disappear on the next run and leave real definitions untouched.
- **AC16 (R17).** Activating Send in normal mode does nothing, starts no harness and adds no simulated response. The control need not be explicitly disabled.

## Decisions and rationale

- Q1: Definitions and selection are the MVP outcome; execution is excluded.
- Q2 and Q6: OMP/ACP is the required combination. All fields except instructions are required; validation does not execute the command. Effort uses the agreed five-value list.
- Q3 and Q7: Preserve the entire existing demo behind `--demo`; normal startup is free of predefined content. Demo agents are isolated and changes last only for the current run.
- Q4: Selecting an agent uses its exact saved configuration; the composer displays model and effort without overrides.
- Q5: Maintain agents under Settings, with explicit Save, editable names and corresponding folder renames, plus confirmed deletion.
- Q8: Normal Send does nothing; explicit disabling is unnecessary.
- Q9: External filesystem changes appear immediately.
- Q10: Reject folder collisions; identify invalid files while preserving access to valid agents.
- Q11: Preserve unsaved edits and resolve external-change conflicts through reload or overwrite.
- Q12: Creation always closes on successful Save, but only the first agent is automatically selected.
- Q13: A removed or invalid selected agent leaves an explicit unselected state.
- Q14: Unsaved edits receive Save, Discard and Cancel choices.
- Q15: Normalize folder names using lowercase and collapsed hyphens, rejecting invalid results and collisions.

## Open questions

None.
