Status: Implemented in v0.1.0. This scope is history. Where it and the code differ, the code is right.

# Agent onboarding and live model/effort selection

## Purpose and context

Adding or editing an agent in Adeline should take as little effort as possible. Today the user types the harness, driver, command, provider/model and effort-parameter name by hand and has to guess values the harness already knows. This scope replaces that with harness discovery, server-populated model and effort pickers, and model/effort switching inside a conversation when the harness allows it.

The user is the person running Adeline locally, who adds agents and chats with them.

**Before:** Agents → Add an Agent shows free-text Name, Harness, Driver, Command and Provider/model fields, an Arguments list, five fixed effort levels, and an "Effort parameter name" dropdown. A wrong model string surfaces only when a conversation starts.

**After:** The user types a name and picks a harness from a list showing which ones are installed. Adeline starts the harness in the background and fills the Model and Effort pickers with what that harness actually offers. The user picks a default model and effort, a permission mode, and optional system instructions, then saves. Command and Arguments appear only for a custom harness.

This scope supersedes, for agent definitions, the Command/Arguments form and legacy migration (R2, R3), the bans on changing model and effort mid-conversation (R5) and the agent-name sentence wording (R6) in `docs/archive/scope-acp-agent-driver.md`. All other parts of that scope remain in force.

Repository evidence inspected on 2026-10-02:

- `src/agent_form.rs`, `FIELDS`, `AgentForm::new` and `AgentForm::fields`: free-text Name, Harness, Driver, Command and Provider/model fields; ordered Arguments list; `RadioGroup` over `agents::EFFORTS`; `Select` over `EffortParameterName::ALL`; permission `RadioGroup`; plain `Textarea` for system instructions.
- `src/agents.rs`, `AgentDefinition` and `AgentDefinition::validate`: `driver` must equal `"ACP"`; effort must be one of Low, Medium, High, Extra High, Max; `load` migrates legacy combined commands. Definitions live at `~/.config/adeline/agents/<normalized-name>/agent.yml`, and `normalize_name` defines folder naming.
- `src/acp.rs`, `Worker::configure` and the `Request::Model`/`Request::Effort` handling: `configOptions` returned by `session/new` and `session/set_config_option` are used only to validate the saved model and effort; effort labels are mapped to fixed values (`"Extra High"` → `xhigh`). `harness == "OMP"` enables the appended system guidance.
- `src/chat_render.rs`, `Adeline::composer_view` and `execution_setting`: the composer's Model and Effort buttons show only the current value and an "Agent settings…" item; no switching.
- `src/storage.rs`: conversation snapshots save harness, command, arguments, model, effort and `effort_parameter_name`.
- `README.md`, Agents section: documents the current YAML format, including `driver: ACP` and `effort_parameter_name`.
- ACP registry: `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json` (source `https://github.com/agentclientprotocol/registry`). On 2026-10-02 it listed 41 agents. Each entry has an id, name, version, description, repository/website, icon, and a distribution that is either `npx` (package plus arguments) or `binary` (per-platform archive, `cmd` and `args`). OMP is not listed.
- Found on the developer's machine: `omp.exe` (`%LOCALAPPDATA%\omp`), `codex-acp`, `codex`, `gemini`, `opencode` (npm global bin), `claude.exe` (native CLI, no ACP adapter).

## Requirements

### Harness catalog and detection

- **R1.** The harness list is built from the ACP registry plus OMP, which Adeline supports as a built-in entry that behaves like a registry entry. A Custom option is always available and listed last.
- **R2.** When Add Agent opens, Adeline fetches a fresh registry in the background without blocking the form. If the fetch fails, it uses the last successfully fetched copy cached on disk. If no cached copy exists, it uses a copy shipped with Adeline. A successful fetch replaces the cache. No registry status line is shown.
- **R3.** A harness counts as installed when its executable is on PATH or in a known default install location for that harness (for example OMP under `%LOCALAPPDATA%\omp`). Locally installed binaries and packages are always used. Adeline never runs agents through `npx` or another on-demand package runner, and never downloads or installs a harness.
- **R4.** For a registry entry, Adeline launches the locally installed executable with the arguments the registry specifies for that agent. For example, an installed `gemini` runs as `gemini --acp`, and `opencode` as `opencode acp`.
- **R5.** The harness picker in the agent form lists every registry entry and OMP with its registry icon and name. A green dot marks installed harnesses and a red dot marks the rest. Installed harnesses are sorted first, each group by name, followed by Custom.
- **R6.** An uninstalled harness can be highlighted but not used. The form shows "Not installed" with a link to the agent's website or repository from the registry. Model, Effort and Save stay unavailable until an installed harness or Custom is selected.
- **R7.** Detection runs in the background at app start, when Add Agent or Edit Agent opens, and when the composer's agent picker opens. It never blocks the UI, and the dots update as results arrive.

### Agent form

- **R8.** Add Agent and Edit Agent use the same form with these fields, in this order:
  1. Name: required, empty for a new agent, nothing prefilled.
  2. Harness: the picker from R5.
  3. Command and Arguments: shown only when Custom is selected (R12).
  4. Model: required, chosen from the harness's offered models (R10). Absent when the harness offers no model option.
  5. Effort: required, chosen from the harness's offered effort levels (R11).
  6. Default permission mode: Ask or Allow everything, defaulting to Ask.
  7. System instructions and the append/overwrite toggle: shown only when R14 applies.
- **R9.** Driver is never shown. ACP is the only driver. The Harness free-text field and the Effort parameter name field are removed.
- **R10.** Selecting an installed harness (or entering a Custom command) starts it in the background in a temporary folder. Adeline runs the ACP handshake and opens a session to read the offered model and effort options, without sending a prompt, then stops the process. The form shows a loading state meanwhile. The Model picker is a searchable dropdown. It groups models by the server's own groups; when the server sends none and IDs look like `provider/model`, it groups by the provider prefix. Each item shows the server's display name with its ID as secondary text.
- **R11.** The Effort picker lists the effort options the harness offers for the selected model, with the server's labels. Changing the model refreshes this list from the options returned after that model is set. A previously selected effort stays selected if it is still offered; otherwise the user must choose again. If the harness offers no effort option for the selected model, the Effort field is absent and not required. Likewise, if the harness offers no model option, the Model field is absent, not required, and the harness uses its own default.
- **R12.** Choosing Custom reveals a required Command field (one executable name or path) and an ordered Arguments list. Arguments are literal strings that can be added, removed and reordered, as today. Probing (R10) uses that command and those arguments.
- **R13.** Adeline identifies a harness by the agent name it reports during the ACP handshake. A Custom agent whose reported identity matches a known harness, such as OMP, gets that harness's built-in features (for example instruction support). Custom agents show the reported name as their harness label, for example "Custom: oh-my-pi".
- **R14.** The System instructions field and its append/overwrite toggle appear only for harnesses with a mechanism Adeline knows for delivering system instructions (OMP; others, such as Claude Agent ACP, only once their mechanism is verified). Append adds the instructions to the harness's default system guidance. Overwrite replaces it. For other harnesses the field is hidden, and the form explains that this harness does not accept system instructions.
- **R15.** The System instructions field renders its content as Markdown when not focused. Clicking or focusing it switches to raw text editing.
- **R16.** For every harness covered by R14, in both append and overwrite modes, the system guidance Adeline sends begins with `You are an agent named <name>, running inside Adeline ADE (Agentic Development Environment)`, followed by any custom instructions. For harnesses without instruction support, nothing is sent and nothing is injected into user messages.

### Probe failures and editing

- **R17.** When probing fails, the form shows the error and a Retry action. If the failure is missing authentication, it lists the harness's advertised ACP login methods with instructions to authenticate through the harness outside Adeline. As a fallback, the user can type the model and effort as free text and save. Free-text values are validated when a conversation starts, as today.
- **R18.** Opening Edit on an existing agent probes its harness again. If the saved model or effort is no longer offered, the form keeps the saved value, marks it "not offered by harness", and blocks Save until an offered value is chosen or the R17 fallback applies.
- **R19.** Other ways in: the composer's agent picker includes "Add an Agent…", and when no agents exist, adding an agent is the main action wherever Send would otherwise be blocked for lack of an agent. The Agents → Add an Agent entry stays.

### Agent definition format

- **R20.** Agent definitions use a new format, still stored one per agent at `~/.config/adeline/agents/<normalized-name>/agent.yml` with the existing name normalization and collision rules. Every file includes a `version` field, currently `1`.
- **R21.** A definition records: version, name, harness identity (registry ID, built-in OMP, or Custom), default model (when the harness offers one), default effort (when the harness offers one), default permission mode, system instructions, and append/overwrite mode. Command and arguments are recorded only for Custom agents. Registry and OMP agents store no executable path; the installed executable is located again each time a conversation starts, so upgrades and moves keep working.
- **R22.** The new format is not backward compatible. Agent files with no `version` field or a version older than the oldest supported one are silently ignored: they are not listed, not reported as errors, and not modified or deleted.

### Conversations and switching

- **R23.** A new conversation saves the resolved command, arguments, model and effort it actually started with. Later edits to the agent definition do not change existing conversations.
- **R24.** When the running harness exposes a model option, the composer's Model menu lists its live options, using the R10 grouping and search. Likewise for the Effort menu when it exposes an effort option. A selection is applied to the running session and saved to that conversation only. It never changes the agent's saved default.
- **R25.** Model and effort switching is disabled while a turn is processing. When the harness exposes no switchable option, the menu shows the current value read-only, as today.
- **R26.** The Model and Effort menus show a short inline warning that switching may invalidate the prompt cache and cost more on the next turn. No confirmation dialog appears.
- **R27.** For a conversation whose agent is not running, the menus offer the options saved from its last session, and the choice is applied when the agent next starts. If that value is no longer offered at start, the existing configuration error applies.
- **R28.** Conversations saved in the old snapshot format stay readable and can still be completed or archived. Sending is blocked with: "This conversation was created by an older Adeline version; start a new chat."

### Availability indicators

- **R29.** A green or red installed dot appears in the agent form's harness picker, the composer's agent picker and the Settings → Agents list. An agent whose harness is not installed (for example, it was uninstalled after saving) shows a red dot, and Send reports that the harness is not installed instead of starting.

### Constraints and quality requirements

- **R30.** Registry fetching, detection and probing never block the UI. A slow or failing network or harness affects only the loading and error states described above.
- **R31.** Before reporting implementation complete, run the repository's lint, checks and tests, and rebuild with `cargo build --release --locked` as required by `AGENTS.md`. Update `README.md`'s Agents section to describe the new format.

## Boundaries

- No `npx`, `uvx` or other on-demand runners; no downloading, installing or updating harnesses.
- No embedded login. Authentication stays outside Adeline (driver-scope R27 still applies).
- No migration of old agent definitions or old conversation snapshots.
- Switching agents mid-conversation remains excluded. Only model and effort become switchable, and only when the harness allows it.
- The working directory, permission mode behavior, retries, transcripts and lifecycle from `docs/archive/scope-acp-agent-driver.md` are unchanged.
- Demo mode stays isolated and must not fetch the registry into real config, probe real harnesses on the user's behalf for saving, or write real definitions.

### Rejected ideas

- Scanning PATH for arbitrary executables and probing them for ACP.
- Listing npx-launchable agents as available.
- Two separate Provider and Model pickers (replaced by one searchable, grouped picker).
- A confirmation dialog for every model/effort switch.
- Letting a mid-conversation switch change the agent's default.
- Sending the name sentence as a user message for harnesses without instruction support.
- Prefilling the agent name with the harness name.
- A registry status line in the form.
- Backward compatibility or automatic migration of old agent files; reporting or deleting them.
- A Harness name field for Custom agents (identity comes from the handshake).
- An "Advanced" section with Command/Arguments for every harness.

## Domain and data

- **Registry entry:** an agent listed in the ACP registry, describing its id, name, icon, links and launch command and arguments. Fetched copies are cached on disk; a bundled copy is the last fallback.
- **Harness:** a registry entry, built-in OMP, or a Custom command. Its installed state comes from detection and is not saved.
- **Harness identity:** the name the agent reports during the ACP handshake. It decides which built-in features (instruction delivery) apply.
- **Agent definition:** a versioned, user-named configuration owned by the user. It holds defaults; it does not store executable paths except for Custom.
- **Conversation execution settings:** the resolved command, arguments, model and effort a conversation started with. Model and effort can then change by switching (R24). Neither direction changes the agent definition.
- **Probe:** a short-lived, prompt-free harness session used only to read the offered options.

## Interfaces and dependencies

- ACP registry CDN (read-only fetch); local filesystem cache; a registry copy bundled with the app.
- The local PATH and per-harness known install locations.
- ACP `initialize` (agent identity, auth methods), `session/new` and `session/set_config_option` (config options, their groups and categories).
- Affected UI: Add/Edit Agent form, Settings → Agents list, composer agent picker, composer Model and Effort menus, and the empty state when no agents exist.

## Failure and edge cases

- **R32.** If the registry fetch fails and both the cache and the bundled copy are missing or unreadable, the harness picker still offers OMP and Custom.
- **R33.** If a Custom command does not exist or does not speak ACP, the probe error says so and R17's Retry and manual fallback apply.
- **R34.** If the user changes the harness while a probe is running, results from the earlier probe are discarded, and its process is stopped.
- **R35.** Probe processes are always stopped, including when the form is closed or the probe fails.

## Acceptance criteria

- **AC1 (R1, R2, R32).** With network access, opening Add Agent shows registry harnesses plus OMP and Custom without waiting for the fetch, and a fresh registry is cached. Offline with a cache, the cached list is used; offline without a cache, the bundled list is used; with none available, OMP and Custom still appear.
- **AC2 (R3, R4, R5, R7, R29).** On a machine with `omp.exe`, `codex-acp`, `gemini` and `opencode` installed, those show green dots and are sorted first; Claude Agent ACP shows red. Running a Gemini agent launches the local `gemini --acp`, never `npx`. Dots in the form, composer picker and Settings → Agents update after background detection without freezing the UI.
- **AC3 (R6).** Selecting an uninstalled harness shows "Not installed" with its website or repository link; Model, Effort and Save are unavailable.
- **AC4 (R8, R9).** The form shows Name (empty), Harness, Model, Effort, Default permission mode (Ask), and system instructions when supported. Driver, the Harness text field and Effort parameter name never appear. Saving without a name is rejected.
- **AC5 (R10, R11).** Selecting OMP shows a loading state and then a searchable Model dropdown grouped by provider, with display names and IDs. Typing filters the list. Selecting a model refreshes Effort from that model's options and keeps a still-offered effort selected. A harness with no effort option has no Effort field and can be saved without one; a harness with no model option likewise has no Model field and saves without a model. No prompt is sent during probing.
- **AC6 (R12, R13).** Choosing Custom shows Command and an ordered Arguments list. A Custom agent using `omp.exe` with `acp` is identified as OMP by its handshake and gets OMP's instruction support, labeled "Custom: <reported name>".
- **AC7 (R14, R15).** For OMP, the System instructions field and append/overwrite toggle are visible, and unfocused content renders as Markdown (headings, lists, code). For a harness without known instruction support, the field is hidden with an explanation.
- **AC8 (R16).** In both append and overwrite modes, OMP receives guidance starting with `You are an agent named Josh, running inside Adeline ADE (Agentic Development Environment)` followed by the custom instructions. Overwrite mode replaces OMP's default guidance; append keeps it. A harness without instruction support receives no name sentence in any message.
- **AC9 (R17, R33).** With a harness that is not logged in, the probe failure lists its advertised login methods with instructions and a Retry button. Retry after logging in populates the pickers. Free-text model and effort entry allows saving when probing cannot succeed.
- **AC10 (R18).** Editing an agent whose saved model is no longer offered shows it marked "not offered by harness" and blocks Save until an offered model is chosen or the fallback applies.
- **AC11 (R19).** The composer's agent picker offers "Add an Agent…". With no agents, adding an agent is the main action where Send would be blocked.
- **AC12 (R20, R21).** A saved registry agent's `agent.yml` contains `version: 1`, name, harness identity, model, effort, permission mode, instructions and instruction mode, with no command or path. A Custom agent's file also contains command and arguments.
- **AC13 (R22).** Old-format `agent.yml` files (no `version`, or an older version) do not appear in any list, produce no error message, and are left byte-for-byte unchanged.
- **AC14 (R23, R24, R26).** In a conversation with a harness that exposes model and effort options, the composer menus list the live options with search and grouping, and show the cache warning inline. Switching applies to the next turn, persists after restart, and leaves the agent's defaults unchanged.
- **AC15 (R25, R27).** While a turn is processing, the switch menus are disabled. A harness without switchable options shows the current value read-only. In a completed conversation, a switch from the saved options is applied when the agent next starts.
- **AC16 (R28).** An old-format conversation opens with readable history and can be completed or archived. Send shows the "created by an older Adeline version" message and starts no agent.
- **AC17 (R29).** After a harness is uninstalled, its agents show red dots, and Send reports that the harness is not installed without starting anything.
- **AC18 (R30, R34, R35).** Changing harness mid-probe discards the earlier results and stops its process. Closing the form stops any running probe. No probe process outlives the form.
- **AC19 (R31).** Lint, checks, tests and `cargo build --release --locked` pass, and `README.md` describes the new definition format.

## Decisions and rationale

- **Q1:** The user picks a detected harness or adds a custom ACP one, then must pick a default model and effort from server data.
- **Q2, Q13:** Model and effort switching is per conversation, only when the harness allows it, with an inline cache warning, and never changes agent defaults.
- **Q3, Q9:** Use the official ACP registry with fetch → cache → bundled fallbacks; OMP is built in.
- **Q4, Q14:** Probe on selection in the background; offer Retry and a free-text fallback; on Edit, flag saved values that are no longer offered.
- **Q5:** Drop the fixed effort list and the effort parameter name; keep an Effort picker fed by the server.
- **Q6:** Markdown renders when unfocused and turns into raw text on focus.
- **Q7, Q15, Q21:** Driver is hidden. The definition format is new and versioned (`version: 1`); old files are silently ignored. This supersedes the earlier preference for backward compatibility.
- **Q8, Q8b, Q19, Q24, Q25:** Only installed harnesses run, never via npx. Every registry entry is listed with green/red dots in three places, and detection runs in the background.
- **Q10, Q11, Q11b, Q11c, Q20:** Instructions and the append/overwrite toggle exist only for harnesses with a known mechanism. Identity comes from the handshake. The new name sentence leads in both modes and is never sent to harnesses without instruction support.
- **Q12:** One searchable model picker grouped by provider.
- **Q16:** Login stays outside Adeline.
- **Q17, Q18:** Add an Agent is also reachable from the composer and the no-agents state; there is no registry status line.
- **Q22:** Old conversations can be read, completed and archived, but not continued.
- **Q23:** Registry and OMP agents store no paths; conversations save the resolved launch settings.
- **Q26:** A harness with no model option hides Model and saves without one, matching the effort rule.

## Open questions

None. The user confirmed the complete scope and acceptance criteria on 2026-10-02.
