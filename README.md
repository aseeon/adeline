![Adeline](assets/adeline-banner.png)

![Adeline application screenshot](assets/adeline-screenshot.png)

# Adeline

A native workspace for projects and agent conversations, built with GPUI Kit.

## Agents

Normal startup loads saved agents, projects, and conversation history without starting an agent. **Agents → Add an Agent**, or **Add an agent…** in the composer's agent picker, opens a separate creation window. With no agents, the composer offers adding one in place of Send. Edit, rename, or delete saved agents under **Settings → Agents**.

### Harnesses

The harness list is the [ACP registry](https://github.com/agentclientprotocol/registry) plus OMP, which Adeline supports as a built-in entry, and Custom, which is always last. Opening Add Agent refreshes the registry in the background. Without network access, Adeline uses the last copy cached under `~/.config/adeline/cache/`, then the copy shipped with Adeline.

Adeline only runs harnesses that are already installed. A harness counts as installed when its executable is on `PATH` or in a known default location such as `%LOCALAPPDATA%\omp`. Adeline starts it with the arguments the registry gives, so Gemini CLI runs as `gemini --acp` and OpenCode as `opencode acp`. It never uses `npx`, `uvx`, or another on-demand runner, and never downloads or installs a harness. Detection runs in the background at startup and when the agent form or the composer's agent picker opens. A green dot marks installed harnesses and a red dot marks the rest, in the form, the composer's agent picker, and Settings → Agents.

### The agent form

The form has Name, Harness, Model, Effort, Default permission mode (Ask or Allow everything), and, where supported, System instructions. Choosing an installed harness starts it in the background in a temporary folder. Adeline runs the ACP handshake and opens a session to read its model and effort options without sending a prompt, then stops the process. The Model picker is searchable and grouped by the harness's groups, or by the `provider/` prefix of model IDs. Changing the model refreshes Effort, because some harnesses, such as Codex, offer effort only per model. A harness without a model or effort option hides that field and uses its own default.

If probing fails, the form shows the error and a Retry button. When the harness needs a login, it lists the harness's login methods; sign in through the harness outside Adeline. You can also type the model and effort as free text and save; the values are checked when a conversation starts. Editing an agent probes again and marks a saved model or effort the harness no longer offers as "not offered by harness". Save stays blocked until you choose an offered value.

Custom shows Command (one executable name or path) and an ordered Arguments list of literal values. Adeline identifies the harness by the name it reports in the handshake, so a Custom `omp.exe acp` gets OMP's instruction support and appears as "Custom: omp".

System instructions appear only for harnesses with a known way to receive them; today that is OMP. Append adds them to the harness's default guidance; Overwrite replaces it. Either way the guidance begins with `You are an agent named <name>, running inside Adeline ADE (Agentic Development Environment)`, followed by your instructions. Other harnesses receive no instructions and no name sentence. The field shows its Markdown rendered until you click it.

### Definition files

Each agent has one definition at `~/.config/adeline/agents/<normalized-name>/agent.yml`, including on Windows. For example, Josh is stored as:

```yaml
version: 1
name: Josh
harness: omp
model: openai-codex/gpt-6-luna
effort: high
permission_mode: Ask
system_instructions: You are a helpful coding assistant.
instructions_mode: Append
```

`harness` is a registry ID, `omp`, or `custom`. Registry and OMP agents store no executable path; Adeline locates the installed executable each time a conversation starts. Only Custom agents store `command`, `arguments`, and the handshake `identity`. `model` and `effort` are the harness's own values and are omitted when it offers no such option. Names become lowercase folder names with punctuation and spaces collapsed into hyphens; invalid names and existing destinations are rejected.

The format is not backward compatible. Files without `version`, or with an older version, are silently ignored: they are not listed, reported, changed, or deleted.

Filesystem changes are watched automatically. Invalid definitions report their filename and error while valid agents remain available. Unsaved forms offer Save, Discard, and Cancel when leaving or closing. If an external edit conflicts with a dirty Settings form, Save offers Reload, Overwrite, or Cancel.

### Conversations and switching

New Chat opens a draft. First Send creates the conversation and starts its agent. One available agent is selected automatically; with several, choose one. Each conversation saves the resolved command, arguments, model, effort, instructions, and working directory it started with, so later agent edits affect only new conversations. If the harness was uninstalled, Send reports that it is not installed and starts nothing.

When the harness offers model or effort options, the composer's Model and Effort menus list them with search and grouping, plus a note that switching may invalidate the prompt cache and cost more on the next turn. A switch applies to the running session and is saved to that conversation only; it never changes the agent's default. Switching is disabled while a turn is processing. For a stopped agent, the menus offer the options from its last session and apply the choice when the agent next starts. Harnesses without these options show the current value read-only.

Conversations from older Adeline versions stay readable and can be completed or archived. Sending in them is blocked with "This conversation was created by an older Adeline version; start a new chat."

### Projects and conversations

Create a project with a name and an existing absolute working directory. Its `~/.config/adeline/projects/<normalized-name>/project.yml` contains only `name` and `directory`. Project settings can rename the project and its configuration folder. Changing the working directory requires every conversation to be completed or archived; existing conversations keep their saved directory.

Each conversation has a generated folder under the project's `conversations/`, containing `conversation.yml` and timestamped `transcript.jsonl`. The log retains raw ACP traffic, visible messages, tool activity, permission decisions, errors, and lifecycle events. Saved history is readable without a running harness. External project and conversation edits load at startup.

Responses stream with formatted text and expandable tool results. Settings → Modes → Chats controls tool visibility and the automatic retry limit (five additional attempts by default; zero disables retries). Thinking is shown instead of reasoning text. Stop cancels the current turn and retries while preserving partial output and normally keeping the process available for another turn. Switching chats or closing a project tab leaves its agents running.

Each conversation has its own mutable Ask / Allow reads / Allow everything permission mode. Allow reads approves read and search tool calls automatically and asks for everything else. Ask shows only the choices and remembered-grant scope offered by the harness, including one-time denial, never permanent denial. Grants are not broadened or re-created by replaying the transcript.

Complete and Archive preserve history and gracefully close that conversation's process. Sending again restores the saved session when supported. Application exit and confirmed project deletion also stop agents gracefully. If shutdown stalls, Force Stop is an explicit choice. Deleting a project removes its saved Adeline data, never its working directory.

Temporary failures retry within the configured limit. Recovery after partial work restores the session and asks for continuation rather than replaying the original request. If restoration fails, starting a replacement session with saved messages requires your choice. Missing configuration, authentication, and denied permissions do not automatically retry. For authentication errors, authenticate using the harness outside Adeline, then use Retry. Transcript write failures cancel processing and block new prompts until Retry storage succeeds.

OMP 18.3.2 was exercised with real responses, appended name/custom guidance, model and effort selection, follow-up context, process restart and session restoration, cancellation with partial text, and graceful shutdown. Harness probing was checked against OMP 18.4.8, Codex ACP 1.13.1, OpenCode 1.18.32, and Gemini CLI 0.42.0.

### Demo mode

```sh
cargo run --release --locked -- --demo
```

Or launch `Adeline.exe --demo`. Demo mode restores the bundled workspace and simulated chat replies. It excludes real agents, saved projects, and conversation execution. Demo project, agent, and chat changes never modify the user's definitions or conversation history. Appearance and feature preferences still use `settings.yml`.

Docs, Workflows, Services, Groupchats, Issues, and Whiteboard are empty destinations in both normal and demo mode. Their feature switches remain under Settings → General → Features. Existing content and preferences for those modes are left intact and ignored; unrelated settings changes preserve their stored values. New settings files contain their feature switches but no obsolete content or panel preferences.

The workspace keeps its projects bar, modes bar, resizable conversation/transcript/activity regions, and bottom controls. Chat settings also offers keyboard-operated panel width adjustments and reset. Searchable Kit command dialogs provide project, agent, machine, and conversation commands. Escape closes the current command dialog and restores the previous keyboard focus.


## Platform setup

Adeline pins `gpui-kit =0.6.6` as its UI dependency. Kit selects the matching `gpui-pre 0.3.6` core and platform backends, including macOS `font-kit` and Linux X11 and Wayland. Windows uses Kit's title bar with Adeline's scaled icon; macOS and Linux retain native decorations.

### Windows

Install Rust using the MSVC toolchain and Visual Studio Build Tools with **Desktop development with C++**, including a Windows SDK. A DirectX-capable graphics driver is required. This is the platform built and visually tested during implementation.

### macOS

Install Rust and the full Xcode application with its command-line tools. GPUI builds Metal shaders with Xcode:

```sh
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
cargo run --release --locked
```

### Linux

Install Rust and the native build dependencies. On Ubuntu / Debian, `scripts/linux` installs them (CI uses the same script):

```sh
./scripts/linux
cargo run --release --locked
```

The platform facade enables both X11 and Wayland through GPUI's WGPU renderer. Run inside a graphical session with a working Vulkan driver. Consult [gpui-pre's documentation](https://docs.rs/gpui-pre/0.3.6/gpui/) and [Zed's Linux build guide](https://github.com/zed-industries/zed/blob/main/docs/src/development/linux.md) for platform prerequisites.

The app icon uses the close-up portrait in `assets/adeline.close.up.svg`; the full portrait is kept in `assets/adeline.svg`. Each build renders the 24×24 title-bar icon at pixel sizes matching Windows display scaling (100%–500%) and embeds a multi-size Windows icon for the executable and taskbar. To update the app icon, replace the close-up SVG and rebuild with `scripts/package.ps1`.

## Interface fonts

Settings → General → Appearance has separate searchable dropdowns for Interface font and Code font. Both list installed font families. Chivo is the default interface font; Chivo Mono is the default code font. Both are embedded in the executable. If a saved font is missing, its setting falls back to the corresponding bundled default while retaining the saved preference. Existing explicit font choices are preserved; legacy `System` preferences use the bundled default.

Interface font size and Code font size each accept whole numbers from 10 to 24 px, with Smaller and Larger buttons for one-pixel adjustments. Both default to 14 px and save immediately. Code typography applies to Markdown code and tool output independently of interface text. Interface size scales controls and spacing as well as text; font changes remeasure virtual rows.

Both fonts come from Google Fonts ([Chivo](https://github.com/google/fonts/tree/main/ofl/chivo), [Chivo Mono](https://github.com/google/fonts/tree/main/ofl/chivomono)), under the SIL Open Font License 1.1. Their copyrights and licenses are included in `assets/fonts/` and displayed in Settings → Licenses.

## Validation

The checks follow [Zed's setup](https://github.com/zed-industries/zed). `rust-toolchain.toml` pins the Rust version, so rustup installs the right toolchain the first time you run `cargo`.

CI runs Clippy, tests, and release builds on Windows, macOS, and Linux. Keep platform-specific render code warning-free on all three; Kit title bars are used only on Windows.

```sh
cargo fmt --all -- --check
./scripts/clippy
./scripts/check-todos
cargo nextest run --locked --all-features
cargo build --release --locked
```

`--release` builds skip LTO and build incrementally, so local rebuilds are fast. Shipped binaries use the `dist` profile, which adds thin LTO: `scripts/package.ps1` and CI run `cargo build --profile dist --locked`, which writes to `target/dist/`.

On Windows, use `scripts\clippy.ps1`. Running `cargo test --locked` works too if nextest isn't installed.

The [framework migration record](docs/gpui-migration-guide.md#9-implementation-and-verification) describes the earlier GPUI migration and its platform verification limits. GPUI Kit is now the production UI dependency; gpui-whiteboard is not included.

`scripts/clippy` is the type checker and linter, the Rust counterpart of basedpyright. The lint levels are in `[lints]` in `Cargo.toml`: the default clippy groups plus `pedantic`, and Zed's denies (debug and unfinished-work macros, redundant clones, disallowed methods). To silence a lint, use `#[expect(lint, reason = "...")]` rather than `#[allow]`. `expect` fails once the lint stops firing, like basedpyright's unnecessary-ignore check. `clippy.toml` bans calls that block the UI thread. Warnings are errors in both the script and CI.

If they are installed, `scripts/clippy` also runs these tools:

- [`cargo-shear`](https://github.com/Boshen/cargo-shear) finds unused dependencies.
- [`typos`](https://github.com/crate-ci/typos) spell-checks the code, using `.config/typos.toml`.

To install them:

```sh
cargo install --locked cargo-shear typos-cli cargo-nextest
```

The tests cover saved project/agent/conversation behavior, Chats demo data, search and status filters, theme safeguards, and preservation or omission of ignored legacy mode preferences.

On Windows, `scripts/check-ui-layout.ps1 -ProcessId <PID>` checks a running Chats window with a project open and no dialog. Composer controls must fit the conversation panel; visible panels and bottom controls must fit the window. Repeat at the minimum window size with both side panels visible and interface size 24, and at normal dimensions.

`.github/workflows/ci.yml` runs on pushes to `main`, pull requests, and manual dispatches. It follows the layout of Zed's `run_tests` workflow. First comes a style job (rustfmt, TODO check, typos). Then clippy and nextest run on Windows, macOS, and Linux, alongside a dependency job (cargo-shear, lockfile, dependency review). Release builds run last and upload an executable for each platform, and a `tests_pass` job gives branch protection a single check to require. CI copies `.cargo/ci-config.toml` so that compiler warnings fail the build.

Theme selection accepts YAML filenames within the themes folder. Path separators and Windows drive prefixes are rejected on every platform.

## Source map

| File | Responsibility |
| --- | --- |
| `src/main.rs` | Kit initialization, window overlay layers, shared shell and controls |
| `src/settings.rs` | Settings and agent windows, search, appearance, feature switches, license notices |
| `src/agents.rs` | Agent definitions, YAML persistence, validation, discovery, demo catalog |
| `src/agent_form.rs` | Shared creation and editing fields |
| `src/acp.rs` | ACP workers, protocol negotiation, permissions, cancellation and recovery |
| `src/storage.rs` | Durable projects, conversation snapshots and transcript replay |
| `src/runtime_ui.rs` | Runtime events, conversation actions and persistence integration |
| `src/project_ui.rs` | Project settings, validation and confirmed deletion |
| `src/views.rs` | Chat header/activity, searchable command dialogs, project dialog hosting |
| `src/chat.rs` | Chat entities, cache invalidation, virtual chat list with stacked section labels |
| `src/chat_render.rs` | Chat rows, section labels, search and filter controls, messages, Markdown and multiline composer |
| `src/prepared.rs` | Search snapshots, chat sections and filter counts, request generations |
| `src/recency.rs` | Chat timestamps, time sections and short time labels |
| `src/interaction.rs` | UI action routing and isolated demo actions |
| `src/data.rs` | Workspace projections, demo data and filtering |
| `assets/` | Bundled workspace data, artwork, and SVGs |
| `build.rs` | Embed assets for portable executable builds |

Kit `MessageScroller` in 0.6.6 does not expose focusable-row registration. Chats retains GPUI's variable-height `ListState` so a focused permission decision stays mounted during scrolling. Kit's uniform-height `List` and premeasured `VirtualList` do not cover these rows either. The row controls, Markdown, composer, and scrollbars use Kit.
