# WASM plugin hosts in native Rust desktop and editor apps (as of Oct 2026)

## Zed: how its extension system works, what extensions contribute, and the UI question

### Takeaway
Zed embeds Wasmtime in-process and runs Rust extensions compiled to `wasm32-wasip2` components against versioned WIT worlds (`since_v0.0.1` through at least `since_v0.8.0`). Users narrow what extensions can do through `granted_extension_capabilities` (process exec, file download, npm install). Extensions cannot render UI. Zed has deprecated the agent-server and slash-command extension kinds in favor of the ACP Registry, and plans the same for MCP servers with the MCP registry. "Customize Zed's UI" is still listed only as a future plan.

### Cited Findings
- Extensions run as Wasm components in Wasmtime. On install the host builds a `wasmtime::Component` from `extension.wasm`, creates a store and instantiates it. The interface is defined in WIT: the guest uses `wit_bindgen`, the host uses `wasmtime::component::bindgen!` on the same WIT files. (Blog post dated Oct 21, 2024, so it is older.) — [Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)
- The API is versioned. WIT files live in directories like `since_v0.2.0`, the compiled extension records the API version it targets, and the host picks the matching bindings. Stable and Preview builds don't allow the newest (unreleased) API version, while Dev and Nightly builds do. — [Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)
- The current WIT directories in `crates/extension_api/wit` are: since_v0.0.1, v0.0.4, v0.0.6, v0.1.0, v0.2.0, v0.3.0, v0.4.0, v0.5.0, v0.6.0, v0.8.0 (the listing may be truncated). — [zed/crates/extension_api/wit](https://github.com/zed-industries/zed/tree/main/crates/extension_api/wit)
- Host-to-extension calls are async (`async: true` in the bindings), so an extension can do blocking work such as downloading a language server without blocking the host. Calls go through a channel and run in a task on Zed's background executor. The runtime is embedded in the Zed process. — [Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)
- Why Wasm: Zed first considered embedding V8 through Deno but wanted extensions written in Rust. The Component Model only recently became usable enough. Users never compile anything, because the registry pipeline compiles each extension ahead of time into an archive of Wasm plus Scheme query files. — [Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)
- What the 2024 post said extensions can't do: no "modifying the UI to create new panels," no arbitrary HTTP requests, no free-form filesystem access. A 15-year-old developer built a Discord presence extension by writing a fake language server. — [Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)
- Current docs list the extension types as languages, debuggers, themes, icon themes, snippets and MCP servers. The manifest is `extension.toml` (`id`, `name`, `version`, `schema_version`, `authors`, `description`, `repository`) plus optional `languages/`, `themes/` and `snippets/` dirs. Rust code is needed only for language server, context server and debugger extensions, and it compiles to `wasm32-wasip2`. — [Zed docs: Developing Extensions](https://zed.dev/docs/extensions/developing-extensions)
- Capabilities: the `granted_extension_capabilities` setting holds `{kind, ...}` entries. Kinds are `process:exec` (command + args globs), `download_file` (host + path globs) and `npm:install` (package). A call without the capability returns an error. Setting the list to `[]` revokes everything and "will likely make many extensions non-functional." — [Zed docs: Capabilities](https://zed.dev/docs/extensions/capabilities)
- "Agent server and slash command extensions have been deprecated and submissions will no longer be accepted." Agent servers should go to the ACP Registry. "MCP server extensions will be deprecated in favor of the MCP registry in the future" (tracked in issue #59351). — [Zed docs: Publishing prerequisites](https://zed.dev/docs/extensions/publishing/prerequisites)
- ACP agent-server extensions were deprecated as of Zed v1.5.0 in favor of the ACP Registry, and installed extension agents are migrated automatically. — [Zed docs: Agent Server Extensions](https://zed.dev/docs/extensions/agent-servers); [Zed docs: External Agents](https://zed.dev/docs/ai/external-agents)
- Zed extension pages say: "In the future, we plan on increasing the extension surface to allow you to customize Zed's UI and more." — [zed.dev extension listing FAQ (e.g. ical)](https://zed.dev/extensions/ical)

### Inferences
- Zed's direction is to shrink Wasm extensions toward in-process, data-and-hook contributions (grammars, LSP/DAP launch logic, themes). Out-of-process protocol servers (ACP agents, MCP servers) move to registries of external processes instead of Wasm wrappers. For Adeline this suggests that agent and tool integrations belong in ACP/MCP processes, and Wasm (if used at all) is for small hooks.
- No UI from Wasm even in Zed, the GPUI reference app. There is no public GPUI-over-WIT design to copy.
- Capabilities are user-granted allowlists checked at the host-function boundary, not a declared-in-manifest-then-prompt model.

### Gaps
- I found no GitHub discussion or blog post with Zed's detailed reasoning against UI extensions, or a design for them, beyond "haven't gotten to it" (2024) and the FAQ's future-tense line.
- The default value of `granted_extension_capabilities`, and when the capability system shipped, weren't found.
- Whether Zed has moved extension execution to a separate process (2026) wasn't confirmed. The only source says in-process on the background executor.

## Extism vs. wasmtime component model + WIT, WASI 0.2/0.3 status, wit-bindgen tradeoffs

### Takeaway
WASI 0.3 was ratified on June 11, 2026. It moves async into the Component Model (`async func`, `stream<T>`, `future<T>`), and Wasmtime 46 enables it by default. Rust guest tooling was still described as "in progress" at ratification. Extism is a simpler bytes-in/bytes-out plugin framework with a manifest-based sandbox (allowed hosts and paths, memory caps, timeout) and host SDKs for many languages. I found no source confirming Component Model support in Extism.

### Cited Findings
- WASI 0.3.0 was ratified on June 11, 2026. "async is now native to WebAssembly Components." The `wasi:io` pollables and streams move into the canonical ABI as `stream<T>`/`future<T>`, one host event loop is shared across components, and `async func` replaces the 0.2 start/finish/subscribe pattern. The `wasi:http` `proxy` world is replaced by `service`/`middleware`. — [Bytecode Alliance: WASI 0.3](https://bytecodealliance.org/articles/WASI-0.3); [wasi.dev P3 release](https://wasi.dev/releases/wasi-p3)
- Wasmtime 45 runs the WASI 0.3 release candidate. "Wasmtime 46 will ship WASI 0.3.0 with Component Model Async enabled by default." Guest toolchains (Rust via wit-bindgen, Go, JS, Python) are "in progress." — [Bytecode Alliance: WASI 0.3](https://bytecodealliance.org/articles/WASI-0.3)
- WASI 0.3 is "a purely additive change," and 0.2 keeps working in hosts. — [Component Model book: migrating to P3](https://component-model.bytecodealliance.org/design/migrating-to-p3.html)
- Docs conflict on the minimum Wasmtime version: wasi.dev says 0.3 support is available from Wasmtime 43+, while the BA article refers to 45 and 46. Third-party blogs giving a February 2026 or late-2025 date conflict with the official June 11, 2026 date. — [wasi.dev P3](https://wasi.dev/releases/wasi-p3); [BA article](https://bytecodealliance.org/articles/WASI-0.3)
- Wasmtime gives a component no access to filesystem or env by default, and access must be granted explicitly. This is capability-based by construction. — [Component Model book: Wasmtime](https://component-model.bytecodealliance.org/running-components/wasmtime.html)
- Wasmtime documents an official "plugins as WASIp2 components" pattern (calculator example). — [docs.wasmtime.dev: wasip2 plugins](https://docs.wasmtime.dev/wasip2-plugins.html)
- Extism host functions are injected as Wasm imports. Parameters are core Wasm types (I32/I64/F32/F64/V128), and data crosses as memory offsets. UserData must be `Send + Sync` in Rust when used with plugin pools. PDKs: Rust, Go, Haskell, AssemblyScript, C, Zig, JS. Host SDKs exist for ~14 languages. — [Extism: Host Functions](https://extism.org/docs/concepts/host-functions)
- The Extism manifest has `timeout_ms`, `memory.max_pages`, an HTTP response size cap, `allowed_hosts` (allowlist for Extism's HTTP functions) and `allowed_paths` (host-to-guest path mapping). This comes from the OCaml binding, so field names may differ elsewhere. — [extism-manifest (OCaml)](https://ocaml.org/p/extism-manifest/latest/doc/Extism_manifest/index.html)
- Data point on runtime choice: Next.js/SWC replaced Wasmer with Wasmtime for plugins, citing Wasmer build complexity from conflicting feature flags and Wasmtime's Bytecode Alliance maintenance, performance and footprint. — [next.js commit via semanticdiff](https://app.semanticdiff.com/gh/vercel/next.js/commit/a24462d9e5244f78679383e6aa41e107dd9520ac)
- Interpreter vs JIT: wasmi loosely mirrors the Wasmtime API "to act as drop-in replacement." Users include Zellij and Typst. — [wasmi docs.rs](https://docs.rs/crate/wasmi/2.0.0). An HN commenter (not a benchmark) put Cranelift at roughly 1.5 to 1.9x slower than native and interpreters at ~10x. — [HN thread](https://hn.nuxt.dev/item/49521031)

### Inferences
- Component Model + WIT gives typed, versionable interfaces. Zed's `since_vX` directories show the cost: the host keeps every old WIT world and its bindings forever. Extism avoids WIT but pushes serialization (JSON/protobuf) and schema versioning onto you.
- With WASI 0.3 ratified but Rust guest tooling just landing, a 2026 Adeline host would likely target WASIp2 (sync guest, host-side async like Zed's `async: true`) and add p3 later, since the change is additive.
- Wasmtime gives epoch/fuel interruption and per-store memory limits. wasmi gives a smaller, faster-to-start, slower-running engine. Either way the plugin runs in the host process unless you add a process boundary yourself.

### Gaps
- Extism's Component Model status and the exact Wasmtime version underneath weren't verified from primary docs.
- I found no quantitative resource figures (memory per instance, instantiate latency) for Wasmtime components vs Extism.
- wit-bindgen's release that supports p3 async guests wasn't confirmed.

## Lapce, Helix, Zellij (and others)

### Takeaway
Lapce runs WASI plugins in Wasmtime inside its separate `lapce-proxy` process (the same process that does LSP and file I/O, and that can run remotely). It speaks LSP-style JSON-RPC over in-memory WASI stdin/stdout pipes, with two threads per plugin and broad permissions. Zellij plugins do render UI: `render(rows, cols)` prints ANSI bytes to stdout, plus serialized themed components. They run in the Zellij server on wasmi, use protobuf events, and have 14 user-granted permission types. Helix still has no merged plugin system. The Steel (Scheme) PR #8675 is a draft, and as of March 2026 it was usable only via forks.

### Cited Findings
- Lapce: plugins use `wasmtime::Engine`/`Module`/`Linker` with `wasmtime_wasi` and `wasi_experimental_http_wasmtime`. Host-to-plugin messages are JSON-RPC (LSP types) written to the plugin's WASI stdin, followed by a call to the exported `handle_rpc`. Plugin-to-host messages go through the host import `lapce.host_handle_rpc`, and stderr goes through `host_handle_stderr`. The code lives in `lapce_proxy::plugin::wasi`. Each plugin gets two threads (a Wasm loop and an RPC loop). The only preopened dir is the plugin's own volt dir mapped to `/`. It uses `.inherit_env()`, and HTTP is set to `allowed_hosts: ["insecure:allow-all"]`. — [lapce-proxy/src/plugin/wasi.rs](https://raw.githubusercontent.com/lapce/lapce/master/lapce-proxy/src/plugin/wasi.rs)
- Lapce plugins can be written in any language that compiles to WASI (C, Rust, AssemblyScript), and Lapce has built-in remote development modeled on VS Code Remote. — [Lapce README](https://github.com/paricbat/lapce/blob/master/README.md)
- Zellij: "Zellij itself builds its UI from plugins." A plugin is "a first class citizen in the workspace, just like a terminal pane," and can render UI, react to state, and control Zellij. Rust is the only officially supported plugin language. — [Zellij docs: Plugins](https://zellij.dev/documentation/plugins)
- Zellij rendering: "When a plugin's render function prints to `STDOUT`, Zellij treats the printed bytes as utf-8 ANSI." The previous output is cleared on each render. Built-in themed components (Table, Ribbon, Nested List, Text) are serialized to stdout as a private DCS escape (`ESC Pz ... ESC \`) with coordinates, selected/opaque flags and color indices. — [Zellij docs: UI rendering](https://zellij.dev/documentation/plugin-ui-rendering.html)
- The Zellij plugin trait has `load(config)`, `update(Event) -> bool` (true = re-render), `pipe(PipeMessage) -> bool` and `render(rows, cols)`. `ZellijWorker::on_message` runs background work without blocking rendering. Config, events, pipe and worker messages cross the boundary as protobuf. — [zellij-tile/src/lib.rs](https://raw.githubusercontent.com/zellij-org/zellij/main/zellij-tile/src/lib.rs); [Zellij docs: Plugin API](https://zellij.dev/documentation/plugin-api.html)
- Zellij permissions (requested through `request_permission`, the user grants them): ReadApplicationState, ChangeApplicationState, OpenFiles, RunCommands, OpenTerminalsOrPlugins, WriteToStdin, Reconfigure, FullHdAccess, StartWebServer, InterceptInput, ReadPaneContents, RunActionsAsUser, WriteToClipboard, ReadSessionEnvironmentVariables. Plugins can see the folder Zellij was opened in by default, and FullHdAccess widens that. — [Zellij docs: Permissions](https://zellij.dev/documentation/plugin-api-permissions.html); [Plugin API](https://zellij.dev/documentation/plugin-api.html)
- The Zellij server depends on `wasmi` and `wasmi_wasi` 1.1.0 (an interpreter), not wasmtime or wasmer. — [zellij-server/Cargo.toml](https://github.com/zellij-org/zellij/blob/main/zellij-server/Cargo.toml)
- Helix: PR #8675 adds Steel as an optional plugin system and depends on the event-system PR #8021. It is shown as Draft. — [helix PR #8675](https://github.com/helix-editor/helix/pull/8675). As of March 2026 the plugin system "hasn't been merged into mainline," and users use a fork. — [ArchWiki mirror, Helix (2026-04 snapshot)](https://wikipedia.jakami.de/content/archlinux_en_all_maxi_2026-04/Helix). The `steelix` fork is packaged in Nix (version dated 2026-05-21). — [mynixos steelix](https://mynixos.com/nixpkgs/package/steelix). The Steel author says Steel "has not yet been accepted as the language of choice, it is a just a good candidate." — [steel discussion #119](https://de.github.com/mattwparas/steel/discussions/119)

### Inferences
- Zellij is the one example that renders plugin UI. It works because the UI medium is a byte stream (ANSI plus a small declarative component vocabulary), so the host keeps layout and theming. The GPUI analogue would be plugins emitting a declarative view tree (a small component set) that the UI process renders. It would not be plugin access to GPUI.
- Lapce is closest to Adeline's split. Plugins live in the backend/proxy process, never in the UI process, and speak a JSON-RPC protocol. Adeline's engine is the natural host, and plugin effects would reach the UI only through the existing engine protocol.

### Gaps
- I didn't find the Helix maintainers' stated reasons for choosing Scheme over WASM (the long-running plugin discussion wasn't fetched).
- Zellij's runtime history (wasmer, then wasmtime, then wasmi) and its reasons weren't sourced. Only the current `wasmi` dependency is confirmed.
- I didn't research Bevy or other GPU apps.
- Lapce's Cargo versions and plugin manifest (`volt.toml`) fields weren't fetched.

## Cross-cutting patterns: security, manifests, versioning, hot reload, isolation, where plugins run

### Takeaway
All of the hosts above run Wasm inside a native process and rely on the Wasm sandbox plus host-function-level checks, not OS process isolation. The difference is which process. Zed runs extensions in the UI app (on a background executor). Lapce runs them in its backend proxy process. Zellij runs them in its server process. Capability models are allowlists checked at host calls, either user-configured (Zed) or user-prompted (Zellij), and Lapce is permissive.

### Cited Findings
- Where plugin code runs:
  - Zed: in-process, as async tasks on the background executor, not the UI thread. — [Zed Decoded](https://zed.dev/blog/zed-decoded-extensions)
  - Lapce: in the `lapce-proxy` process, with two OS threads per plugin. — [wasi.rs](https://raw.githubusercontent.com/lapce/lapce/master/lapce-proxy/src/plugin/wasi.rs)
  - Zellij: in the server (`zellij-server` depends on wasmi), with optional workers for heavy work so rendering isn't blocked. — [Cargo.toml](https://github.com/zellij-org/zellij/blob/main/zellij-server/Cargo.toml); [Plugin API](https://zellij.dev/documentation/plugin-api.html)
- Capability models:
  - Zed: user setting with glob allowlists per kind (exec, download, npm), returning errors on denial. — [Zed Capabilities](https://zed.dev/docs/extensions/capabilities)
  - Zellij: runtime `request_permission` with 14 permission kinds. — [Zellij Permissions](https://zellij.dev/documentation/plugin-api-permissions.html)
  - Lapce: only the plugin's own directory is preopened, but env is inherited and HTTP is allowed to all hosts. — [wasi.rs](https://raw.githubusercontent.com/lapce/lapce/master/lapce-proxy/src/plugin/wasi.rs)
  - Extism: `allowed_hosts`/`allowed_paths`/memory/timeout in the manifest. — [extism-manifest](https://ocaml.org/p/extism-manifest/latest/doc/Extism_manifest/index.html)
- Manifest and versioning: Zed uses `extension.toml` with `schema_version`, and the binary records the targeted WIT API version. The host keeps every `since_vX` world. — [Zed docs](https://zed.dev/docs/extensions/developing-extensions); [Zed Decoded](https://zed.dev/blog/zed-decoded-extensions)
- Hot reload and dev loop: Zed's "Install Dev Extension" overrides the published version, and logs go to Zed.log or `zed --foreground`. — [Zed docs](https://zed.dev/docs/extensions/developing-extensions). Zellij can load plugins from a local or remote `.wasm` with `--skip-plugin-cache` to force a recompile. — [Zellij Plugin](https://zellij.dev/documentation/zellij-plugin.html)
- Distribution: Zed builds extensions centrally from a registry repo of git submodules (which the team said may not scale). — [Zed Decoded](https://zed.dev/blog/zed-decoded-extensions)

### Inferences
- A Wasm trap is contained by the runtime, so a buggy plugin doesn't crash the host. Runaway CPU or memory still needs epoch, fuel or memory limits (Wasmtime) or `timeout_ms`/`max_pages` (Extism). None of the sources showed an app running plugins in a dedicated OS process for crash isolation. For Adeline, the engine process is already that boundary relative to the UI.
- The lowest-maintenance design consistent with these hosts: plugins in the engine process, a small versioned WIT world (or an Extism bytes ABI), user-granted capability allowlists modeled on Zed's, no direct UI. If UI contributions are wanted, use Zellij's approach of a declarative, host-rendered component vocabulary sent over the existing engine-to-UI protocol, which would need a PROTOCOL bump.

### Gaps
- No source found for per-plugin process isolation in any of these editors, or for true hot reload (swapping a running instance without restart) in Zed or Lapce.
- How hosts handle state migration across WIT versions, beyond keeping old bindings, wasn't documented.
