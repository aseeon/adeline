# Build modes as crates, extensions later

Adeline's built-in modes (Docs, Workflows, Services, Group chats, Issues, Whiteboard) should **not** be built as WASM extensions. They should be **first-party Rust crates written against a small internal mode contract**, the model Zed and VS Code's workbench contributions use. The parts of that contract that cross features should be **data-only registries**: a typed `Card` schema that the engine-side mode emits and the UI draws, and an opener keyed by an `adeline://<mode>/...` URI. That way a later third-party extension API (WASM in the engine, or a webview lane) can feed the same registries without rewriting the modes. The evidence is consistent across apps. No shipping GPUI app, Zed included, lets WASM draw UI ([Zed docs: Extensions](https://zed.dev/docs/extensions)). Every chat platform that puts third-party content into a transcript uses a host-rendered card vocabulary with a stable entity ref and an "open the detail view" action ([Slack Work Objects](https://docs.slack.dev/messaging/work-objects-overview)). And apps that put core features on the public plugin model end up adding a private tier anyway ([JetBrains Internal API](https://plugins.jetbrains.com/docs/intellij/api-internal.html)). For Adeline in particular, cards must come from the **engine**, not the UI. The engine is what sees tool calls, runs headless, and runs on remote machines over SSH. So a `Card` becomes part of `protocol::Delta`, and `PROTOCOL` goes from 4 to 5. The recommended path costs one protocol type, one generic card renderer, one "mode with chat sidebar" shell layout, and the Docs crate. Everything extension-related can wait until a real third party asks for it.

## Every peer keeps built-in features native and extensions narrow

The direct question has a clear answer from precedent. **Zed**, the closest analogue (Rust, GPUI), builds its agent panel, terminal, git and channel notes as crates. Each has an `init(cx)` that registers actions and implements the workspace `Panel`/`Item` traits ([assistant_panel.rs](https://git.secluded.site/zed/blob/v0.181.3-pre/crates/assistant/src/assistant_panel.rs)). Its WASM extensions, which run in Wasmtime against versioned WIT worlds, can add only languages, themes, icon themes, snippets, debuggers and MCP servers. They have **"no support for modifying the UI to create new panels"** ([Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)). Zed also deprecated its agent-server and slash-command extension kinds in favor of the ACP registry ([Zed docs: Publishing](https://zed.dev/docs/extensions/publishing/prerequisites)). So the WASM surface is shrinking toward small hooks, and process-shaped integrations move to external protocols. **VS Code** keeps search, git UI, debug and chat in `vs/workbench/contrib`, which uses "an internal (as opposed to real extension API) mechanism". Each contribution has one entry file and exposes one common API that other contributions may use, but never reach past ([VS Code wiki](https://github.com/microsoft/vscode/wiki/Source-Code-Organization)). Even its first-party extensions need a privileged "proposed API" lane to move faster than the public API freezes ([Using Proposed API](https://code.visualstudio.com/api/advanced-topics/using-proposed-api)).

The one strong counter-example is **Raycast**. It ported built-in GitHub and Linear features to its public extension API and says those ports "informed what we need to build and expose through the API" ([Raycast blog](https://raycast.com/blog/how-raycast-api-extensions-work)). That worked because Raycast's UI is a handful of declarative components (List, Detail, Form). A Markdown editor or a whiteboard does not fit that vocabulary. Atom, where everything was an in-process package with full DOM access, was sunset in 2022 ([TechCrunch](https://techcrunch.com/2022/06/08/github-sunsets-atom-the-software-dev-environment-it-launched-in-2011)). Its founder's next editor is Zed, with bounded WASM extensions ([Changelog #531](https://changelog.com/podcast/531)). No primary source states this as a lesson learned, but the direction of travel is unambiguous.

The local peers point the same way, and they also show what to avoid. Zeron, Waku and Codeg have no plugins. They hardcode modes as closed enums (`Route`, `RightSurface` in Zeron `crates/ui/src/shell.rs`; `RightPanelSurface` in Waku `src/app.rs`; a route union in Codeg `src/components/workbench/workbench-content.tsx`). Their good ideas are worth taking: Zeron's transcript emits link events to a handler instead of opening views itself (`markdown/render.rs`). Waku routes every transcript link through one function (`src/app/right_panel.rs`). Codeg keeps the conversation mounted under full-page routes and decides where a file opens with one pure function (`resolve-link-action.ts`). Their shared cost is that **mode-specific cards live in the core transcript**: Zeron's `transcript.rs` is about 16k lines and `shell.rs` about 18k, and Codeg's transcript renderer is a 3,300-line if-chain. Ghostex has a real extension system ([Ghostex](https://github.com/maddada/Ghostex)), with manifest, permissions and HTML in CEF webviews across five placements. It still has no extension point for chat-message cards. In other words, none of the five peers solves the exact problem Adeline has. The card registry described below is the piece they all lack.

**Microsoft's Pragmatic Rust Guidelines** (`.claude/skills/ms-rust`) settle the remaining options. M-ISOLATE-DLL-STATE rules out native Rust dylib plugins, because `String`, `Vec`, `TypeId` and statics such as tokio and log cannot be shared safely across a DLL boundary. That leaves WASM or a process boundary for third parties. M-FFI-TRANSLATES says the interop layer should only translate, so WIT bindings belong in a thin adapter over a native Rust mode API and not inside the modes. M-DONT-LEAK-TYPES keeps wasmtime and GPUI types out of that API. M-DI-HIERARCHY ranks concrete types above generics above `dyn` traits. That argues against a `trait Mode` while every mode is first-party and known at compile time: a closed enum and a `match` are enough. M-SMALLER-CRATES supports one crate per mode.

## Cards are data the engine emits and the UI draws

Adeline already has most of the detection input. Every tool call becomes a `data::Activity` with a normalized `tool: ToolKind` and `paths: Vec<String>` (`crates/adeline_core/src/data.rs:150`, `crates/adeline_core/src/conversation.rs:216`). So "an Edit that created `plan.md`" is a filter on existing data. What's missing is a way for a mode to turn that into something the transcript shows without the transcript importing the mode. Today `tool_icon`/`tool_steps` in `adeline_ui/src/chat.rs` are a hardcoded `match`, which is exactly the Zeron and Codeg trajectory.

The industry answer has one shape everywhere. The owner registers a match key (domain, file glob, URI scheme). The host detects matches in content it owns. The owner returns a **typed, host-rendered card** and never arbitrary UI. Clicking the card deep-links back to the owner. Slack's `card` block is the template: title and subtitle up to 150 characters, body up to 200, and **at most 3 buttons** ([Slack card block](https://docs.slack.dev/reference/block-kit/blocks/card-block)). Work Objects add the half Adeline needs. A compact unfurl sits in the channel, and a **flexpane opens on the right when the card is clicked**, keyed by a stable `external_ref` ([Slack Work Objects](https://docs.slack.dev/messaging/work-objects-overview)). That is "Docs card opens Docs mode with the chat beside it", with the sides swapped. VS Code's chat reaches the same result with typed `reference`, `anchor` and `filetree` parts that open through the host's own resolver ([Chat Participant API](https://code.visualstudio.com/api/extension-guides/ai/chat)). Zed routes `zed://` links through a single parser into typed variants handled by different crates ([Zed #44910](https://git.secluded.site/zed/commit/829b1b5661dbcf976489c3d8021acdb8f4ed6af9)).

Two Adeline-specific constraints decide where card code runs. First, the engine owns tool calls and runs on remote machines, and the headless build has no GPUI. So **detection must run in the engine**: it can check that the `.md` file exists under the project root, and it is the only process that can reach the issue tracker from a remote box. Cards then travel over the protocol like any other conversation state. They persist, replay on reconnect, and work in a second client. Second, cards are built from agent output, so they are untrusted input. Slack advises considering disabling unfurls on LLM-generated messages because of prompt-injection exfiltration ([Slack unfurling](https://docs.slack.dev/messaging/unfurling-links-in-messages)). VS Code renders chat links only from trusted domains and needs explicit allowlists for command links ([VS Code chat](https://code.visualstudio.com/api/extension-guides/ai/chat)). For Adeline that means a card exists only for an entity the engine has verified: a file inside the project, or an issue on a tracker the user configured. Card actions come from a closed enum and never run commands carried in the payload.

## Three candidate architectures, ranked

| | 1. Mode crates + data-only card/opener registry | 3. Native modes + webview/MCP-Apps extensions | 2. Everything is a WASM extension |
|---|---|---|---|
| Rank | **Recommended** | Second, as a later add-on to 1 | Last |
| Built-in modes | Rust crates, native GPUI views | Same as 1 | WASM components behind a public WIT |
| Third parties | Later: WASM in engine feeding the same registries | HTML in embedded webview, MCP Apps bridge | Same lane as built-ins |
| Where mode logic runs | Engine (detection, data) + UI (view) | Same; extension UI in UI process | Engine (WASM) + host-drawn declarative UI |
| Card renderer | One generic GPUI card | Generic card; webview for extension views | One generic GPUI card |
| New dependencies now | None | Webview (wry/CEF) when the lane opens | wasmtime, wit-bindgen, versioned WIT forever |
| Up-front cost | Low | Low now, high when the lane opens | High |
| Resembles | Zed, VS Code contrib, Slack Work Objects | Ghostex, VS Code + MCP Apps | Raycast, Zellij |

### 1. First-party mode crates with a data-only card and opener registry

**Shape.** `adeline_core` gains `card.rs`: `Card`, `EntityRef` and `CardAction`. These are plain serde types next to `protocol.rs`. Each mode is one crate, for example `adeline_docs`. Its engine half has no GPUI dependency: a function from `&Activity` (or reply text) plus the project to `Option<Card>`, and any engine `Command` handlers the mode needs, such as reading and writing a doc. Its view half sits behind a `ui` Cargo feature, so the headless engine compiles the crate without GPUI and `adeline_ui` enables the feature. The engine keeps a closed `enum Mode { Docs, Issues, … }` and calls each enabled mode's detector with a `match`. The UI keeps one router that maps `adeline://<mode>/…` to that mode's view. The transcript renders every card with one generic component and never names a mode. The shell gains one layout, `Surface::Mode { entity, chat }`: the mode's view fills the main area and the existing chat view moves into a sidebar. This is a one-time shell change that every mode reuses, the way Codeg keeps the conversation mounted under full-page routes.

**Docs flow.** The agent finishes an Edit whose `paths` include `notes/plan.md`. The engine's ACP worker updates the `Activity` as it does today, then calls `docs::card(&activity, &project)`. That function checks `ToolKind::Edit`, a `.md` extension, a completed status and that the file exists under the root. It returns `Card { entity: "adeline://docs/notes/plan.md", icon: Doc, title: "plan.md", subtitle: first heading, actions: [Open] }`. The engine stores the card on the message and sends `Delta::Card { project_id, chat_id, card }`. A card with an existing `entity` replaces the old one in place, the same upsert Slack does on `external_ref`. `apply_delta` stores it. The transcript draws it after the activity that produced it (reusing `Activity::after_text` placement), keyboard-focusable and with a shortcut, as AGENTS.md requires. Activating `Open` dispatches the URI to the router, which sets `Surface::Mode { entity, chat }`. The Docs view loads the file **through an engine Command and not the local filesystem**, because on a remote machine the file lives where the engine is. The chat stays live in the sidebar, so the user can ask the agent to revise the doc while reading it.

**Issues flow.** Detection has two triggers. A turn-end scan of reply text finds issue URLs for trackers the user configured. An Execute or MCP activity whose output names an issue (`gh issue create`, a Linear tool) is the other. The engine-side Issues mode fetches state with credentials stored on the engine machine and emits `Card { entity: "adeline://issues/github/acme/app/42", icon: Issue, title, subtitle: "acme/app#42", facts: [State: Open, Assignee: …], actions: [Open, OpenExternal(url)] }`. Later ticket updates in the same chat produce a card with the same `entity`, so the card updates instead of piling up. Click-through is identical to Docs. Only the view differs.

**Enable and disable.** A per-mode flag lives in engine settings. A disabled mode's detector does not run, its nav entry hides, and an existing card whose mode is now disabled falls back to a plain link. No Cargo feature per mode is needed unless binary size ever matters. **Demo mode** works for free: `assets/workspace.json` holds sample cards, and the demo branches in `runtime_ui.rs` answer the Docs read command from fixture text.

**Third parties later.** An extension host crate (`adeline_ext`) runs WASM components in the **engine**. This is the Lapce and Zellij placement: Lapce runs plugins in its backend proxy process, Zellij in its server ([lapce wasi.rs](https://raw.githubusercontent.com/lapce/lapce/master/lapce-proxy/src/plugin/wasi.rs), [zellij-server Cargo.toml](https://github.com/zellij-org/zellij/blob/main/zellij-server/Cargo.toml)). It uses a small WIT world: `on-activity(activity) -> list<card>` plus a declared URI authority. The adapter translates WIT records into `adeline_core::Card` and nothing more (M-FFI-TRANSLATES). The router's fallback arm sends unknown authorities to the extension's view, which is either a declarative view tree in the Zellij style or option 3's webview. Capabilities follow Zed's user-granted allowlists checked at host calls ([Zed Capabilities](https://zed.dev/docs/extensions/capabilities)). Only at that point does `Mode` gain an `Extension(ExtensionMode)` variant wrapping the `dyn` behind a named struct (M-DI-HIERARCHY). None of this needs a change to Docs or Issues.

**Pros.** Native performance, theming, keyboard and accessibility for every built-in. Zero new dependencies now. The transcript stays mode-agnostic, which avoids the 16k-line Zeron transcript. It works headless and remote. The card schema stays small and gets exercised by first-party modes before any third party depends on it. **Cons.** The internal contract is not proven by an external user until extensions ship, so the WIT API will need some adjustment when it does. Discipline is required to stop modes from reaching into the transcript directly. **Cost.** Lowest. Phase 1 is roughly one core module, one Delta variant, one card component, one shell layout and the Docs crate.

### 3. Native built-in modes plus webview/MCP-Apps extensions

**Shape.** Built-in modes as in option 1, but the third-party lane is HTML. Extensions ship web bundles rendered in an embedded webview (CEF as Ghostex does, or wry with WebView2/WKWebView) with a bridge SDK, or they reuse **MCP Apps**: stable since 2026-01-26, `ui://` HTML resources in a sandboxed iframe talking JSON-RPC over postMessage, with display modes `inline`, `fullscreen` and `pip` ([MCP Apps spec](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx)). Claude, VS Code and Microsoft 365 Copilot already host it ([MCP Apps overview](https://modelcontextprotocol.io/extensions/apps/overview); [VS Code blog](https://code.visualstudio.com/blogs/2026/01/26/mcp-apps-support)).

**Flows.** Docs and Issues behave exactly as in option 1. A third-party card works differently. An MCP tool carrying `_meta.ui.resourceUri` would get a native summary card in the transcript, and its `Open` action would request the `fullscreen` display mode, which renders the HTML in the mode area with the chat in the sidebar. **Pros.** Unlimited expressiveness for third parties. It plugs into an ecosystem agents already use, so an MCP server's UI works in Adeline with no Adeline-specific SDK. **Cons.** Embedding a webview in GPUI is a large cross-platform project that the notes found no GPUI precedent for. It runs only in the UI process, so a remote engine must ship HTML to the client. It needs CSP, sandbox origins and theme-variable plumbing to look native. Raycast explicitly rejected the "mini-app in a frame" feel ([Raycast blog](https://raycast.com/blog/how-raycast-api-extensions-work)). **Cost.** Low until the lane opens, then high. It ranks second because it is compatible with option 1 rather than an alternative to it. Treat it as the likely shape of Adeline's eventual third-party UI lane, and render webviews only in the full mode view, never inline, so the transcript stays native.

### 2. Everything is an extension: modes as WASM components

**Shape.** An engine-hosted wasmtime runtime. Docs, Issues and the other modes ship as WASM components against a public, versioned WIT world. Their UI is a declarative vocabulary the host draws: list, detail, form, markdown, the Raycast or Zellij approach ([Zellij UI rendering](https://zellij.dev/documentation/plugin-ui-rendering.html)).

**Flows.** Card detection is the same as option 1 but runs inside the Docs component. The full Docs view must be expressed in the vocabulary. A Markdown editor with live preview, a whiteboard canvas or a group-chat composer either cannot be expressed, or becomes a host-native "editor" primitive. At that point the built-in has moved back into the host and only the overhead remains. **Pros.** The API gets dogfooded from day one, as at Raycast. There is one uniform model, and third parties get the same power as first-party code. **Cons.** Every internal need becomes public, frozen surface. Zed keeps every `since_vX` WIT world and its bindings forever ([zed extension_api/wit](https://github.com/zed-industries/zed/tree/main/crates/extension_api/wit)). VS Code and JetBrains needed proposed-API and `@Internal` escape hatches for exactly this pressure. The tooling is in motion: WASI 0.3 was ratified on 2026-06-11, with Rust guest tooling still "in progress" ([Bytecode Alliance](https://bytecodealliance.org/articles/WASI-0.3)). It also adds a serialization boundary between a mode and its own view. **Cost.** Highest: a new runtime dependency, an API design phase before the first mode ships, and permanent version maintenance. It ranks last because it pays all of this before any third party exists. Zed, the closest peer, chose not to.

## A minimal type sketch

The contract is a few plain types plus two `match` statements. No traits are needed while every mode is first-party.

```rust
// adeline_core/src/card.rs
// "adeline://docs/notes/plan.md", "adeline://issues/github/acme/app/42"
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityRef(String);
impl EntityRef {
    pub fn mode(&self) -> &str { /* authority after adeline:// */ }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub entity: EntityRef,        // stable: a new card with the same ref replaces the old one
    pub icon: CardIcon,           // Doc, Issue, Workflow, Service, Board
    pub title: String,            // capped at 150 chars
    pub subtitle: String,
    pub facts: Vec<Fact>,         // capped at 4
    pub actions: Vec<CardAction>, // capped at 3
}
pub struct Fact { pub label: String, pub value: String }
pub enum CardAction { Open, OpenExternal(String), Copy(String) }

// protocol.rs: Delta::Card { project_id, chat_id, card }; PROTOCOL 4 -> 5,
// shape recorded in crates/adeline_core/tests/protocol.rs.

// adeline_engine: detectors, one match, no registry object
pub enum Mode { Docs, Issues }
fn card(mode: Mode, a: &Activity, p: &Project) -> Option<Card> {
    match mode {
        Mode::Docs => adeline_docs::card(a, p),
        Mode::Issues => adeline_issues::card(a, p),
    }
}

// adeline_ui: one opener
fn open(entity: &EntityRef, chat: ChatId, cx: &mut App) {
    match entity.mode() {
        "docs" => show_mode(adeline_docs::view(entity, cx), chat, cx),
        "issues" => show_mode(adeline_issues::view(entity, cx), chat, cx),
        _ => open_as_link(entity, cx),
    }
}
```

The caps live in constructors or are enforced in the renderer, so the transcript stays scannable on narrow widths, as every chat platform's limits do. `EntityRef` doubles as an external deep link later (`adeline://docs/...` from a terminal or another app), the way Zed's `open_listener` handles `zed://`.

## Ship Docs first and defer everything extension-shaped

Phase 1 delivers Docs end to end. It adds `card.rs` and `Delta::Card` with the `PROTOCOL` bump and its pinned shape test, the generic card component with a keyboard shortcut to open the focused card (added to `docs/shortcuts.md`), and the `Surface::Mode { entity, chat }` shell layout with the chat in a sidebar. It also adds an `adeline_docs` crate with the `.md` detector and a read-only Markdown view loaded through an engine command, demo fixtures, and a per-mode enable flag. Phase 1 also adds `ui_check.py` coverage: the card appears in `--demo`, and opening it shows Docs with the chat sidebar. Phase 2 makes Docs editable and adds Issues. Issues brings text scanning, tracker credentials on the engine side, and in-place card upserts on ticket updates. These are the second consumer that will show whether `Card` needs anything beyond facts and actions. Later modes reuse the same layout and card. Defer the WASM host, any `trait Mode`, webviews and MCP Apps rendering, extension manifests and capability prompts, and an inline post-back action (`Invoke { action_id, payload }`). Each should arrive only when a concrete third party or mode needs it. One open question deserves an early check: whether ACP adds its own UI or card metadata. If it does, the engine-side detector for agent-supplied cards becomes a translation of ACP `_meta` into the same `Card`.

## Conclusion

The real decision is not "extension or built-in" but **where the seam goes**. Peers that hardcode modes put the seam nowhere and pay for it in giant transcript and shell files. Peers that make everything an extension put it at a public ABI and pay in frozen API and lost UI fidelity. Adeline's engine/client split already provides a well-placed seam: the protocol. Making cards and entity URIs protocol data owned by engine-side modes gets the decoupling of an extension system now, keeps native GPUI views for the modes that need them, and makes the later extension API mostly an adapter over types the first-party modes have already exercised.

What remains uncertain is the third-party UI lane. WASM with a declarative vocabulary suits small integrations. Webviews and MCP Apps suit rich ones and are already the AI-chat standard. Neither choice is forced yet, and with option 1 neither choice requires rewriting the modes.
