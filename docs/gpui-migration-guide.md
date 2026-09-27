# GPUI 0.2.2 to gpui-pre 0.3.6: Adeline migration guide

Research date: 2026-09-27. The migration is implemented. Sections 1–8 retain the original investigation and implementation guidance; section 9 records the implementation checks and their limits.

## 1. Recommendation and scope

Use `gpui-pre =0.3.6` under the existing `gpui` dependency name, add the matching platform crate, and preserve Adeline's current entity/view architecture. The immediate source changes are application startup, focus and focus traversal, two scrollbar offset accesses, and the shared shadow constructor. The larger risks are platform packaging, text/input behavior, virtual-list scrolling, and preserving cached rendering.

After that compatibility work, the strongest opportunities are:

1. Use list remeasurement for streamed messages and font changes; evaluate built-in tail-follow against Adeline's existing scroll policy.
2. Add accessible roles, names, values, and keyboard focus treatment to the custom controls.
3. Pair the existing render-isolation counters with GPUI's new frame and input-latency profiler.
4. Consider native notifications for background conversation completion and permission requests as separate product work.

The [confirmed migration scope](scope-gpui-pre-0.3.6.md) remains the acceptance contract. It requires compatibility with GPUI Kit 0.6.6 and gpui-whiteboard 0.5.1, preservation of working workflows/data, and Windows/macOS/Linux support including X11 and Wayland. It does **not** require adopting Kit controls or implementing a whiteboard. Optional recommendations below do not expand that scope.

### Evidence and limits

This guide compares the actual published archives, rather than assuming that current Zed `main` or similarly numbered GPUI packages match the target:

| Source | Identity |
| --- | --- |
| Adeline baseline | `Cargo.toml` and `Cargo.lock` both select `gpui 0.2.2` |
| Published `gpui 0.2.2` | `.cargo_vcs_info.json` records Zed `69e2130295c2649963eb639fc70b4f2ee8ea1624`, with `dirty: true`; the archive is authoritative over a clean checkout of that commit |
| Published `gpui-pre 0.3.6` | Package metadata identifies a snapshot of Zed `bcf6582ce3500df93a8a39366640173e6786cea6`; metadata still calls the upstream crate version `0.2.2` |
| Platform and ecosystem | Exact `gpui-pre-platform 0.3.6`, `gpui-kit 0.6.6`, and `gpui-whiteboard 0.5.1` published manifests/sources |

`gpui-pre 0.3.6` is a differently named snapshot distribution. Do not describe this as an official `gpui 0.3.6` release or infer a complete compatibility promise from its version number. The target includes publication-specific changes, for example vendoring GPUI shader inputs into the Apple package. [Package metadata][new-manifest] and [Apple build source][apple-build] establish these details.

Observed during this investigation:

- The downloaded old archive's SHA-256 matched Adeline's lockfile checksum. Core/platform/Kit/whiteboard target archives matched the independently resolved registry checksums.
- An isolated `cargo generate-lockfile --manifest-path <temporary-probe>/Cargo.toml` succeeded under the repository's Rust 1.98.1 toolchain. The probe included the proposed core/platform dependencies, Kit with default features, and whiteboard. It resolved 853 packages, including exactly one GPUI core, `gpui-pre 0.3.6`, with no `gpui` package. Kit/base/component resolved to 0.6.6 and whiteboard to 0.5.1.
- No application source, dependency declaration, or application lockfile was changed. No migrated application was compiled or launched, and no platform visual or screen-reader behavior was exercised.

The API differences below are source-verified. Their predicted compiler effects are **[INFERENCE]**, not captured compiler diagnostics. Runtime risks and expected benefits require the checks in section 8. This is an Adeline-focused migration investigation, not an exhaustive changelog of every upstream internal change.

## 2. Dependency and application startup changes

### Replace the package, retain the import name

Replace the current `gpui = "=0.2.2"` with:

```toml
[dependencies]
gpui = { package = "gpui-pre", version = "=0.3.6" }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.6", features = ["font-kit", "wayland", "x11"] }
```

The published library is still named `gpui`, so `use gpui::{prelude::*, *};` and qualified `gpui::...` references can stay. Keep the exact pins and regenerate/commit `Cargo.lock` during implementation. Do not retain the old package alongside the new one: entities, contexts, windows, and traits from distinct GPUI packages are different Rust types. [Old manifest][old-manifest], [target manifest][new-manifest], [platform manifest][platform-manifest].

### Platform features now need explicit wiring

The old crate bundled native backends. The target separates the core from the platform facade and OS packages. The facade has `default = []`; enabling similarly named features on core alone does not select the facade's desktop backends.

| Target | Required facade selection | Reason |
| --- | --- | --- |
| Windows | No extra platform feature | Win32 and DirectWrite are selected by target configuration |
| macOS | `font-kit` | The target README warns that without it the fallback text system lays out text but renders no glyphs |
| Linux | `wayland`, `x11` | Preserve both supported display backends; either feature brings the corresponding renderer/text support |

The combined feature list above is the target README's cross-platform recommendation. Keep core's default Windows manifest support. Sources: [target README][new-readme], [platform feature wiring][platform-manifest].

### Replace the constructor

In `src/main.rs::main`:

```rust
// Before
Application::new()

// After
gpui_platform::application()
```

Keep the existing `.with_assets(Assets).run(...)` chain and initialization order. `Application::new()` is absent from the target core; `Application::with_platform(Rc<dyn Platform>)` is the lower-level constructor, and the facade supplies the native implementation. No custom platform abstraction is needed. `with_assets`, the `run` callback, and the used `open_window` contract remain available. [Old application][old-app], [target application][new-app], [platform factory][platform-source].

### Kit and whiteboard compatibility

- Kit 0.6.6 pins both core and platform to `=0.3.6`. Its desktop platform dependency also enables `runtime_shaders`, in addition to `font-kit`, X11, and Wayland.
- Whiteboard 0.5.1 declares `gpui = { package = "gpui-pre", version = "0.3" }`. Adeline's exact pin constrains that compatible requirement to 0.3.6.
- The successful resolver probe establishes one compatible dependency graph. It does not prove that Kit/whiteboard views compile inside Adeline or that their initialization and rendering work.
- When either library is actually adopted, check feature unification and compile a real embedding path. Kit's `runtime_shaders` feature changes the macOS shader path even if Adeline does not explicitly enable it. Keep unused component dependencies out of production until integration needs them.

Sources: [Kit manifest][kit-manifest], [whiteboard manifest][whiteboard-manifest], [whiteboard version guidance][whiteboard-readme].

## 3. Backward incompatibilities affecting Adeline

### Required edits

| Change | Before | Target | Adeline impact |
| --- | --- | --- | --- |
| Application construction | `Application::new()` | `gpui_platform::application()` | `src/main.rs::main` |
| Focus | `window.focus(&handle)` | `window.focus(&handle, cx)` | `main.rs`, `input.rs`, `interaction.rs`, `settings.rs`, `collaboration_modes.rs` |
| Traversal | `focus_next()`, `focus_prev()` | `focus_next(cx)`, `focus_prev(cx)` | Main shell, settings and agent-creation window action callbacks |
| Panel scroll maximum | `ScrollHandle::max_offset() -> Size<Pixels>` | Returns `Point<Pixels>` | `src/scrollbar.rs::Target::metrics`: `.height` becomes `.y` |
| Virtual-list scroll maximum | `ListState::max_offset_for_scrollbar() -> Size<Pixels>` | Returns `Point<Pixels>` | Same shared scrollbar metrics function |
| Shadow literal | Four fields | Additional `inset: bool` | `src/theme.rs::shadow`: add `inset: false` |

Sources: [old window][old-window] / [target window][new-window], [old Div/scroll handle][old-div] / [target Div/scroll handle][new-div], [old list][old-list] / [target list][new-list], [old style][old-style] / [target style][new-style].

#### Focus: pass through the existing context

For the main shell's action handlers, change the discarded callback argument into a named context:

```rust
.on_action(cx.listener(|_, _: &NextFocus, w, cx| w.focus_next(cx)))
.on_action(cx.listener(|_, _: &PreviousFocus, w, cx| w.focus_prev(cx)))
```

Apply the same principle to every direct focus call, including search, modal/dialog entry and dismissal, input clearing, settings controls, initial startup, and collaboration-mode forms. `FocusHandle::focus` also takes `cx` in the target if that convenience API is used later. Do not confuse these methods with the `.focus(...)` style builder, which is a different API.

The target focus implementation updates a focus generation and clears pending keystrokes through the app context. Preserve the current focus destinations and recheck configured multi-stroke bindings. `Focusable::focus_handle`, `cx.focus_handle`, `track_focus`, and `FocusHandle::is_focused` retain the relevant contracts.

#### Scrollbars: change the component, preserve the sign convention

The existing vertical maximum reads become:

```rust
h.max_offset().y
h.max_offset_for_scrollbar().y
```

Keep the existing negative current-offset handling. A `Size` becoming a `Point` does not by itself justify reversing scroll direction. `set_offset(Point)`, `set_offset_from_scrollbar(Point)`, and `scroll_px_offset_for_scrollbar()` remain compatible with the current call shapes.

#### Shadows: preserve the outer shadow

Add `inset: false` to `theme::shadow`'s existing literal. The target also offers `BoxShadow::new(...).blur_radius(...).spread_radius(...)` and `.inset()`, but no constructor rewrite is required to retain the existing appearance.

### Broader API breaks to know about

These differences matter when porting surrounding code or examples, but do not require a wholesale Adeline rewrite:

| API | Difference | Current application consequence |
| --- | --- | --- |
| `AsyncApp::update`, `refresh`, `subscribe`, global accessors | Previously fallible app access now returns direct values; access after app drop can panic | No direct affected calls found. Do not mechanically remove error handling from weak-entity updates |
| `Entity::update` / `read_with` with generic contexts | Return changes from context-dependent `C::Result<R>` to `R` | Inspect strong-entity operations if adding async integrations |
| `WeakEntity::update` | Still returns `Result<R>` | Retain dropped-view handling in chat search, document preparation, runtime events, and demo tasks |
| `AnyView::cached(style)` | Returns `ViewElement<AnyView>` rather than `AnyView` | Existing `.child(...)` chains remain compatible; a variable explicitly typed as `AnyView` would need adjustment |
| `Task` | Reexported from the scheduler package | Stored/awaited/detached tasks remain usable; dropping a task still cancels it |
| `Task::detach_and_log_err` | Moves to extension trait `TaskExt`; error bounds now include `Display` and `Debug` | No current callsite; import the trait if adopting the method |
| Executor helpers | Old `spawn_labeled`, `TaskLabel`, and `block` surface replaced by priority APIs and `block_on` | No current callsite; do not introduce blocking waits on the UI thread |
| Low-level shadow painting | `Window::paint_shadows` replaced by separate drop/inset shadow APIs | Adeline uses styled shadows, so no direct painter migration |

Sources: [old async context][old-async] / [target async context][new-async], [old entities][old-entity] / [target entities][new-entity], [old views][old-view] / [target views][new-view], [old executor][old-executor] / [target executor][new-executor], [scheduler task contract][scheduler-task], [target window][new-window].

## 4. Behavior changes that compilation will not catch

### Virtual lists and custom scrollbar interaction

The new list implementation changes scrollbar bookkeeping during content growth. Its maximum can be frozen against the height captured at drag start; its pixel-offset calculation also differs from the old implementation. Adeline's custom scrollbar consumes both values. Test dragging while an assistant response or service log grows, rather than validating only an idle list. [Old list][old-list], [target list][new-list].

List wheel-handler registration also changes: the target registers the list handler before children so reverse bubble dispatch gives child handlers a chance to stop propagation first. Exercise nested scrolling in popups and content views.

`HitboxId::is_hovered` now suppresses hover in keyboard modality unless pointer capture is active. `src/scrollbar.rs` calls it directly, so hover coloring after keyboard input can differ without any compile error. The target adds pointer capture with automatic release on mouse-up. Adeline already has drag state and window-level mouse listeners; adopt capture only if it simplifies a demonstrated drag problem. [Target window][new-window].

### Text input and IME

`EntityInputHandler` keeps its old required methods and adds defaulted hooks: `paste`, `set_selected_text_range`, `text_length_utf16`, `accepts_text_input`, `text_input_configuration`, and `text_input_editable_range`. The current UTF-16/UTF-8 conversion and platform input registration in `src/input.rs` can stay. [Old input trait][old-input], [target input trait][new-input].

Two additions deserve specific checks:

- **Platform paste:** Adeline's existing Paste action replaces newlines with spaces. The new trait's default platform-paste hook inserts plain text unchanged. If a backend invokes that hook, override it to preserve the same single-line rule. This is a source-visible alternate path, not an observed regression on the supported desktop platforms.
- **Character input preference:** new input-handler preferences and key-dispatch handling can prefer accepted text over bindings for events marked `prefer_character_input`. Exercise IME composition, non-US layouts, dead keys, emoji/non-BMP characters, selection replacement, and configurable shortcuts together.

`TextRun` retains its existing fields and adds `Default`. `TextStyle`, `shape_line`, the custom `Element` drawing contract, and the input-handler registration pattern do not require replacing Adeline's editor. Both releases already contain a custom input example. Single-line shaping still requires text without embedded newlines. [Target text system][new-text], [target input][new-input], [target element contract][new-element].

### Layout and typography

The layout dependency changes from Taffy `=0.9.0` to `0.13.0`. This is reason to visually check flex minimum sizes, clipping, wrapping, popup bounds, and virtual-row heights, not evidence of any particular layout regression. [Old manifest][old-manifest], [target manifest][new-manifest].

Keep Adeline's existing configured font inheritance and bundled-font registration before opening windows. `add_fonts` and `all_font_names` retain their signatures; the target additionally sorts/deduplicates font names and clears font caches on registration. The existing catalogue normalization remains compatible. No GPUI-driven conversion of saved font settings is indicated by these APIs.

Do not remove the explicit virtual-document width workaround in `src/content_views.rs` merely because the framework changed. Both list versions still measure child height with `AvailableSpace::MinContent`; prove changed behavior before removing it.

## 5. Best practices to retain or update

### Keep the existing state and rendering boundaries

Adeline already separates the header, sidebar, transcript, composer, documents, and logs into entity-backed regions. `chat.rs::sync_regions` controls their notifications; `document_render.rs` prepares documents off-thread; runtime event delivery updates the owning entity. The target preserves the relevant `Render`, `AssetSource`, `Context::spawn`, `spawn_in`, `listener`, `observe`, `subscribe`, and `notify` contracts. No new state-management layer is needed. [Context source][new-context], [element source][new-element], [asset source][new-assets].

Cached views still depend on bounds, content mask, inherited text style, entity dirtiness, and window refresh state. A refreshed cached parent still forces nested cached children through rendering. The existing warning in `content_views.rs` therefore remains applicable. Keep heavy sibling boundaries and explicit notifications rather than adding nested caches everywhere. [Old cache implementation][old-view], [target cache implementation][new-view].

The new `Entity<T>::cached(style)` can express a typed cached view directly. Prefer it when touching a relevant callsite, but replacing every `AnyView::from(entity).cached(...)` is optional and has no measured performance benefit here.

### Keep cancellation and stale-result protection

Dropping a task still cancels it. Keep `ChatList.pending.take()`, request generations, stored document tasks, weak-handle updates, and runtime generation checks. Scheduler replacement does not make stale search/document results safe to apply. Keep blocking filesystem/process work outside GPUI foreground callbacks.

The target offers background priority selection and foreground `spawn_when_idle(timeout, future)`. Use these only for work that can safely wait. Agent event delivery, permission decisions, durable transcript writes, and shutdown must not depend on eventual idle time. [Executor source][new-executor], [scheduler task][scheduler-task].

### Make accessibility explicit

The target introduces AccessKit integration and default accessibility hooks on `Element`, plus Div roles and ARIA properties. The published accessibility guide requires an ID and role for an element to contribute a node. Adeline's custom `TextElement` returns no ID, and its input parent has no explicit accessible role/name/value.

**[INFERENCE from source]** the existing custom field will not become a fully described accessible text input merely by changing dependencies. This was not tested with a screen reader.

Recommended adoption order:

1. Give actual input/control nodes stable IDs and suitable roles. Add real labels, current input values, placeholders, and selected/disabled state where applicable.
2. Cover icon-only buttons, project/chat navigation, settings controls, dialogs, permission choices, and validation errors. A visible icon or tooltip is not sufficient evidence of an accessible name.
3. Use keyboard focus styling deliberately. New `.focus_visible(...)` supports a keyboard-only focus treatment; retain an obvious editing focus indicator on text fields.
4. For the custom-drawn editor, implement the guide's text-run/selection semantics if richer screen-reader cursor behavior is required. Labels alone do not establish full text editing accessibility.
5. Verify navigation, names, state, editing, and dialogs with the platform accessibility tools and a screen reader. Do not claim accessibility compliance from source annotations alone.

Sources: [target accessibility guide][a11y], [Div accessibility/focus API][new-div], [custom Element hooks][new-element]. This is recommended follow-on work; the confirmed preservation scope does not require a control-library replacement.

## 6. New capabilities worth using

| Priority | Capability | Concrete Adeline use | Adoption boundary |
| --- | --- | --- | --- |
| High | `ListState::remeasure_items(range)` | Recompute the growing assistant message's height in `chat.rs::Transcript::sync` without treating it as a different row | Retain `splice_focusable` for actual insertion/removal or focus-handle changes |
| High | `ListState::remeasure()` | Invalidate unchanged row identities after font/size changes in chat, document, and log views | It preserves a proportional in-row anchor; compare against the current logical-top policy |
| High | `scroll_to_end`, `FollowMode::Tail`, `is_following_tail`, `pause_following_tail` | Streaming transcript and service-log follow behavior, plus a possible jump-to-latest control | Adopt only after deciding whether current follow policy should change |
| High | AccessKit roles/properties and `focus_visible` | Accessible custom controls and visible keyboard navigation | Requires control semantics and runtime verification, not just enabling a feature |
| Medium | `profiler`, frame-duration/input-latency snapshots, debug overlay | Diagnose typing, streaming, and large-document stalls alongside `ui_metrics.rs` | Opt-in diagnostics; existing render-isolation counters answer a different question |
| Medium | `TextSystem::prewarm_fonts` | Warm selected interface/code fonts after registration when first-use shaping stalls are measured | Documentation says it may be expensive; run on a background executor, not synchronously at every render |
| Product-dependent | System notifications and app identity | Notify for a background conversation finishing or needing permission; open the relevant conversation on activation | Delivery may be unavailable or a no-op. Keep in-app state authoritative and route actions through existing permission logic |
| Conditional | Async clipboard and text-input configuration | Future platform-driven paste, software keyboard hints, or non-desktop targets | Current desktop input does not need a speculative rewrite |

### Tail-follow is useful, but it is a policy change

Today, `runtime_ui.rs` appends streamed chunks to the existing assistant message. `Transcript::sync` invalidates a changing final row with a splice and restores a logical scroll position; sending a message can request an end offset. The new list APIs distinguish those operations more directly:

- `remeasure_items(ix..ix + 1)` preserves an absolute in-row anchor while the same row grows.
- `remeasure()` preserves a proportional in-row anchor across a full remeasurement.
- `scroll_to_end()` anchors after the last item so layout can find the bottom of a last row that continues growing.
- Tail mode keeps following at the end, pauses on upward user scrolling, and resumes when the user returns to the end. Programmatic scrolling and custom scrollbar movement interact with this state.

Keep normal mode for an initial behavior-preserving port if necessary. If adopting Tail mode, test new-message insertion, growth within one message, tool expansion, history reading, returning to the bottom, and dragging during growth. Do not use uniform-height optimization for variable-height chat/document rows. [Target list implementation and API documentation][new-list].

### Profiling should supplement existing checks

The target's `profiler` feature gates `Window::frame_duration_snapshot`, `input_latency_snapshot`, and debug-frame-overlay controls. A possible explicit feature mapping when adopting diagnostics is:

```toml
[features]
ui-profiling = ["gpui/profiler"]
```

Adeline's current feature is `ui-profiling = []`. Wiring it to GPUI enables framework instrumentation; it does not automatically display the overlay or consume snapshots. Keep the existing counters and isolation scripts, then add only the diagnostic UI needed to investigate an actual stall. No frame-time or throughput improvement was measured during this investigation. [Target window profiler APIs][new-window], [feature manifest][new-manifest].

### Notifications, clipboard, and motion

`App::set_app_identity` should run early, before opening windows or posting notifications, if native identity/notifications are adopted. `show_system_notification` is explicitly best-effort; tags can replace previous notifications where supported, and the response handler is replaced by later registrations. Use stable conversation routing and never treat an OS notification as the sole place a permission request exists.

`read_from_clipboard_async` is preferred in code that can await, especially on asynchronous/permission-gated platforms. Preserve single-line paste normalization and avoid applying a delayed paste to a different field/selection without a deliberate policy. The existing synchronous desktop action remains available.

The target adds `reduce_motion`/`set_reduce_motion` for nonessential animation policy. If used for spinners or later animations, wire it to an actual preference/platform policy; do not assume the existence of the getter proves automatic OS-setting synchronization. [Target application APIs][new-app].

## 7. Platform and packaging impact

| Platform | Verified source change | What Adeline should do |
| --- | --- | --- |
| Windows | D3D11/DirectX rendering remains, but implementation and release HLSL compilation move to `gpui-pre-windows`; shader compiler lookup now includes the newest SDK registry entry | Retain MSVC/Windows SDK prerequisites and exercise a release build, not only `cargo check` |
| macOS | Metal rendering/build code moves behind the macOS/Apple packages | Retain Xcode setup; include facade `font-kit`; verify the selected shader feature path |
| Linux | Old X11/Wayland backends used Blade; target Linux features enable `gpui_wgpu` | Keep both display features and native-library setup; test actual X11 and Wayland sessions and representative GPU drivers |

Sources: [old platform build][old-build], [target Windows build][windows-build], [old Linux renderer][old-linux], [target Linux manifest][linux-manifest], [Apple build][apple-build].

### macOS shader features have a concrete build effect

The published `gpui-pre-apple 0.3.6/build.rs` still generates shader bindings. Without `runtime_shaders`, it invokes `xcrun -sdk macosx metal` and `xcrun -sdk macosx metallib`. With the feature, it emits a stitched Metal source file instead. This resolves the misleading impression that the much smaller new core build script removes the shader toolchain requirement. Kit enables the runtime-shader feature through the platform facade. Validate both the intended production feature graph and app launch; do not relax the documented Xcode prerequisite solely because core no longer compiles Metal itself.

### Preserve native window behavior

Adeline's used `WindowOptions`, `TitlebarOptions`, and `WindowControlArea` APIs remain available. Window option literals already use defaults, so additional fields do not require filling out every option.

- Preserve the native `WindowControlArea` implementation in `src/titlebar.rs` for main, settings, and agent-creation windows. Do not replace native caption hit regions with ordinary click handlers.
- Target Windows hit testing now respects resizable/minimizable flags for Max/Min regions. The existing default flags remain true. **[INFERENCE]** the current native caption behavior should remain, but Snap Layouts, drag, double-click, maximize, minimize, and close need a real Windows run.
- `app_owns_titlebar_drag` is a new macOS-specific option. It is not needed for Adeline's Windows-only custom chrome; leave its default false.
- The target adds an inactive-window frame interval default of 33,333 microseconds. Check inactive-window updates, especially streaming and settings, without assuming a particular visible frame rate.
- Keep main-window close interception, graceful agent shutdown, and auxiliary-window cleanup. Constructor migration does not replace these application responsibilities.

Sources: [target platform/window options][new-platform-types], [target Windows hit testing][windows-events], [old Windows hit testing][old-windows-events].

Update `README.md`'s pinned framework link and explanation of Linux features when the migration actually lands. Keep `scripts/linux`, the three-platform CI matrix, the app's separate asset/icon build, and bundled font/license handling. Driver support under the new Linux renderer and macOS/Windows visual behavior were not verified here.

## 8. Implementation sequence and acceptance checks

### A. Make the mechanical port

1. Record the working baseline and preserve representative saved projects, agents, settings, and transcripts for migration checks. Do not use live user data for destructive scenarios.
2. Replace the dependency and add the platform facade with explicit features; regenerate the application lockfile.
3. Change startup, all focus/traversal callsites, the two scrollbar maximum reads, and the shadow literal.
4. Compile before adopting new list policy, accessibility behavior, or component libraries. Keep API migration and optional product changes distinguishable.
5. Inspect `cargo tree -i gpui-pre`, `cargo tree -i gpui-pre-platform`, and `cargo tree -d`. Other duplicate libraries are not automatically bugs; the key invariant is one GPUI type family at the selected version.
6. For the scope's Kit/whiteboard compatibility criterion, compile a separate real embedding check using their actual view types with the same GPUI entities/windows. Resolver success alone is insufficient. Avoid unused production dependencies added only to demonstrate a compatible manifest.

Expected directly edited application files are `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/input.rs`, `src/interaction.rs`, `src/settings.rs`, `src/collaboration_modes.rs`, and `src/scrollbar.rs`, `src/theme.rs`. `chat.rs`, `content_views.rs`, and profiling code need changes only if the corresponding optional API adoption is included. Update `README.md` after the new dependency and platform setup are real.

### B. Run the repository gates and rebuild

The existing documented gates remain required for application changes:

```sh
cargo fmt --all -- --check
./scripts/clippy
./scripts/check-todos
cargo nextest run --locked --all-features
cargo build --release --locked
```

On Windows use `scripts\clippy.ps1`; `cargo test --locked` is the documented fallback when nextest is unavailable. Keep CI's Windows, macOS, and Ubuntu checks/tests/release builds, plus dependency/lockfile checks. A green check build does not exercise Windows release shader compilation. A release build does not prove desktop behavior.

### C. Exercise the migrated native application

| Scenario | Required observation |
| --- | --- |
| Main/settings/agent windows | Correct fonts/icons, theme, minimum size, clipping, popup placement, and close handling |
| Windows titlebars | Caption drag and double-click, native Min/Max/Close, Snap Layouts, activation changes, display scaling |
| macOS and Linux | Native decorations; readable bundled/system fonts; launch and input in both Linux display backends |
| Focus and input | Tab/Shift-Tab order, search focus, modal focus restoration, clear/refocus, configured shortcuts, IME, emoji, dead keys, clipboard normalization |
| Streaming transcript | One response grows without overlap; end position and reading-history behavior remain intentional; custom scrollbar works during growth |
| Typography | Interface/code font and size changes remeasure rows, preserve usable scroll position, survive restart, and retain unavailable-font preferences |
| Real projects/agents | Create/edit/rename/reopen work; validation errors remain visible; editing an agent does not change saved conversation execution configuration |
| Real chat lifecycle | Send/stream/follow-up/switch/reopen history; tool results; offered permission choices; Stop preserves partial output; retry/recovery and graceful shutdown retain their behavior |
| Persistence failures | Malformed settings are not overwritten; transcript failure blocks unsafe progress; interrupted turns and storage recovery remain available |
| Optional new features | Screen-reader behavior, notifications, Tail mode, or profiling are tested separately if adopted |

Run normal startup against isolated saved fixtures for persistence and real-agent scenarios. `--demo` cannot prove those workflows: it excludes saved projects and real agent execution.

### D. Preserve rendering isolation

Use the existing `ui-profiling` build and actual native interactions:

```sh
cargo run --release --locked --features ui-profiling -- --demo
```

`src/ui_metrics.rs` writes counters to `artifacts/ui-profile.csv`; optional stress row-count files are `artifacts/ui-stress-rows.txt` and `artifacts/ui-stress-content-rows.txt`, capped at 10,000 rows. Capture before/after CSV snapshots within the same running process, after initial rendering settles:

- Type in the composer and run `scripts/check-ui-isolation.ps1 -Before <before.csv> -After <after.csv>`. It requires composer activity without header/sidebar/transcript/row renders.
- Trigger an unrelated shell update while document/log content stays unchanged and run `scripts/check-content-isolation.ps1 -Before <before.csv> -After <after.csv> -Region document` or `log`.
- Resize and change fonts with large variable-height content, then inspect row wrapping and scroll anchoring. The isolation counters do not prove correct geometry.
- Remove temporary stress controls after the check. Use the GPUI profiler to diagnose latency if adopted; do not replace these behavioral checks with a render-count benchmark alone.

Completion means the [confirmed scope's acceptance criteria](scope-gpui-pre-0.3.6.md#acceptance-criteria) have evidence. The original investigation established dependency resolution only; the implementation evidence follows.

## 9. Implementation and verification

Implemented on 2026-09-27, with exact core/platform pins and a regenerated application lockfile. The application retains its existing storage formats, runtime ownership, rendering regions, and scrolling policy.

Compilation found four additional changes beyond section 3: `ShapedLine::paint` now needs alignment and optional alignment width; SVG text refinements are no longer optional; popup anchors use `Anchor`; and `on_window_closed` supplies the closed window ID. Startup focus uses an entity update to avoid borrowing the application immutably and mutably at once. The new platform-paste hook and the existing Paste action share the existing newline normalization.

### Build and compatibility evidence

- `cargo fmt --all -- --check`, `scripts/clippy.ps1` (including installed dependency/spelling checks), `bash scripts/check-todos`, and `cargo nextest run --locked --all-features` passed. All 68 tests passed.
- `cargo build --release --locked` succeeded on Windows, including the new Windows platform/shader build. The rebuilt executable was launched for the checks below.
- A separate temporary host compiled with exact GPUI Kit 0.6.6 and gpui-whiteboard 0.5.1 dependencies. It constructed a `WhiteboardView` through Adeline's `gpui::App` context, assigned it to `gpui_kit::Entity<WhiteboardView>`, rendered it alongside a Kit `Button`, and wrapped the host in Kit's `Root` using the same window/context. Production dependencies remain limited to the framework.
- Application and probe dependency trees each resolved one GPUI core, `gpui-pre 0.3.6`, with no old `gpui` package. Target-filtered Cargo metadata retained macOS `font-kit` and both Linux display backends with `gpui_wgpu`.

### Observed Windows workflows

Native pointer/keyboard automation exercised the release app against an isolated copy of pre-migration settings, themes, agents, projects, and transcripts. The original user files were untouched.

- Existing saved history opened with the retained Chivo / Chivo Mono fonts and Claude Plus theme. Project creation rejected a nonexistent directory, accepted an existing directory, opened the project, and survived restart.
- Project settings rejected a conflicting name and an active conversation's directory change. Renaming preserved the new conversation history and execution directory.
- Agent creation and editing saved successfully. An existing conversation retained Low effort after its agent definition changed to Medium; a new conversation used Medium. Previously saved agents remained available.
- OMP 18.3.4 returned real responses and remembered an earlier number across follow-ups and a full application restart. Tool activity rendered, and a real tool wrote the expected marker in the isolated working directory.
- A growing response rendered and scrolled; Stop retained partial output and persisted `turn_cancelled`. Conversation switching, completion, archive, and reopening history worked.
- Ask / Allow everything selection persisted across restart. Missing harness authentication produced a visible error with Retry. A harness configured with `always-ask` denied a write without offering an ACP permission request; the application retained the failure and did not create the file.
- Theme and interface/code font-size changes applied and survived restart. Multiline clipboard paste became one line and preserved a non-BMP emoji. Search and settings shortcuts, popup focus, and button traversal were exercised.
- Malformed settings produced a visible save error and remained byte-identical. An exclusive lock on the isolated transcript produced a visible storage error and Retry storage. Releasing the lock and retrying saved history and exposed the existing interrupted-turn Retry path; that action resumed streamed output.
- Native caption double-click maximize/restore, minimize, dragging, and close worked. Closing the main window also closed its settings window and shut down the real chat process. A later shutdown after storage recovery stalled, displayed Force Stop, and closed after that explicit action.
- Hash comparison found every copied pre-migration project, agent, and transcript byte-identical after these checks. Only settings changed, through the exercised preference controls.
- The `ui-profiling` release build passed `check-ui-isolation.ps1` while typing in the composer and `check-content-isolation.ps1` for document/log views while filtering their sidebars with stable focus. Opening the application menu changed focus and rebuilt the document once; both old and new `Window::focus` call `refresh`, which bypasses caches. No cache policy was changed to suppress that refresh.

One later launch found two test files under the repository's `target` directory renamed with `[conflicted]` suffixes; Adeline reported the missing conversation snapshot. The user confirmed that pCloud, which synchronizes this directory with cloud storage, renamed those files. A final launch using a fresh saved-data copy under the OS temporary directory opened the old history correctly and closed normally. Keep persistence-failure fixtures outside synchronized directories.

### Verification limits

macOS, X11, and Wayland runtime behavior was not observed on this Windows workstation; WSL is not installed. Their feature selection and existing CI matrix are retained, but this record does not claim a non-Windows build or launch. The ecosystem host was compiled, not visually exercised. Native IME/dead-key input, Snap Layouts, other display scales, and a harness-offered permission-choice dialog were not exercised; existing permission/recovery regression tests passed. Text fields still use the pre-existing non-tab-stop focus handles in both framework versions; this migration does not change that keyboard policy.

## Source index

All external citations pin the release used in the comparison. Adeline file/symbol references describe the repository as inspected on the research date.

- Framework packages: [GPUI 0.2.2][old-manifest], [gpui-pre 0.3.6][new-manifest], [platform facade 0.3.6][platform-manifest].
- Target guidance: [platform setup][new-readme], [accessibility][a11y], [virtual lists][new-list], [task lifecycle][scheduler-task].
- Ecosystem constraints: [GPUI Kit 0.6.6][kit-manifest], [gpui-whiteboard 0.5.1][whiteboard-manifest].
- Native build and rendering: [Windows shader build][windows-build], [Apple shader build][apple-build], [Linux backend features][linux-manifest].

[old-manifest]: https://docs.rs/crate/gpui/0.2.2/source/Cargo.toml
[new-manifest]: https://docs.rs/crate/gpui-pre/0.3.6/source/Cargo.toml
[new-readme]: https://docs.rs/crate/gpui-pre/0.3.6/source/README.md
[platform-manifest]: https://docs.rs/crate/gpui-pre-platform/0.3.6/source/Cargo.toml
[platform-source]: https://docs.rs/crate/gpui-pre-platform/0.3.6/source/src/gpui_platform.rs
[kit-manifest]: https://docs.rs/crate/gpui-kit/0.6.6/source/Cargo.toml
[whiteboard-manifest]: https://docs.rs/crate/gpui-whiteboard/0.5.1/source/Cargo.toml
[whiteboard-readme]: https://docs.rs/crate/gpui-whiteboard/0.5.1/source/README.md
[old-app]: https://docs.rs/crate/gpui/0.2.2/source/src/app.rs
[new-app]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/app.rs
[old-window]: https://docs.rs/crate/gpui/0.2.2/source/src/window.rs
[new-window]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/window.rs
[old-div]: https://docs.rs/crate/gpui/0.2.2/source/src/elements/div.rs
[new-div]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/elements/div.rs
[old-list]: https://docs.rs/crate/gpui/0.2.2/source/src/elements/list.rs
[new-list]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/elements/list.rs
[old-style]: https://docs.rs/crate/gpui/0.2.2/source/src/style.rs
[new-style]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/style.rs
[old-async]: https://docs.rs/crate/gpui/0.2.2/source/src/app/async_context.rs
[new-async]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/app/async_context.rs
[old-entity]: https://docs.rs/crate/gpui/0.2.2/source/src/app/entity_map.rs
[new-entity]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/app/entity_map.rs
[old-view]: https://docs.rs/crate/gpui/0.2.2/source/src/view.rs
[new-view]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/view.rs
[old-executor]: https://docs.rs/crate/gpui/0.2.2/source/src/executor.rs
[new-executor]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/executor.rs
[scheduler-task]: https://docs.rs/gpui-pre-scheduler/0.3.6/scheduler/struct.Task.html
[old-input]: https://docs.rs/crate/gpui/0.2.2/source/src/input.rs
[new-input]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/input.rs
[new-text]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/text_system.rs
[new-element]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/element.rs
[new-context]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/app/context.rs
[new-assets]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/assets.rs
[a11y]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/_accessibility.rs
[old-build]: https://docs.rs/crate/gpui/0.2.2/source/build.rs
[windows-build]: https://docs.rs/crate/gpui-pre-windows/0.3.6/source/build.rs
[old-linux]: https://docs.rs/crate/gpui/0.2.2/source/src/platform/linux/x11/window.rs
[linux-manifest]: https://docs.rs/crate/gpui-pre-linux/0.3.6/source/Cargo.toml
[apple-build]: https://docs.rs/crate/gpui-pre-apple/0.3.6/source/build.rs
[new-platform-types]: https://docs.rs/crate/gpui-pre/0.3.6/source/src/platform.rs
[windows-events]: https://docs.rs/crate/gpui-pre-windows/0.3.6/source/src/events.rs
[old-windows-events]: https://docs.rs/crate/gpui/0.2.2/source/src/platform/windows/events.rs
