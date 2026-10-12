Status: Draft

# Handoff: cache the main window's views

The goal is a window that draws a frame within the 4.17 ms budget of a 240 Hz display, and runs its animations at full refresh rate. Today every change redraws the whole window. This doc collects what profiling found and the steps to fix it. Measure with `scripts/engine-check/perf_check.py` (see `scripts/engine-check/README.md`).

## What happens today

No view in the main window uses `.cached(...)`. In GPUI, a child view is re-rendered, laid out and painted whenever its parent is, unless the parent wraps it in `.cached(style)`. So a hover, a keystroke, a streamed chunk or an animation frame rebuilds the whole tree.

When a view is notified, GPUI marks it and every view above it dirty (`Window::mark_view_dirty`). Dirty views re-render. A cached view that isn't dirty reuses last frame's layout and paint.

Two docs claim otherwise and need correcting with this work: `docs/architecture.md` ("The chat regions are separate GPUI views, so typing or scrolling one doesn't redraw the others") and the module comment of `crates/adeline_ui/src/chat.rs` ("each an independently invalidated view").

## Measurements (v0.1.15, 240 Hz display, demo mode)

- A frame costs 5–5.5 ms whatever changed. Handing it to the GPU costs about 0.4 ms. The rest is CPU work.
- The CPU profile of a frame: about 40% layout (taffy, and the transcript list measuring its visible rows), about 15% prepaint, about 25% paint (text about 8%). Adeline's own `render` code is 5–10%.
- With animations off, a hover or a keystroke costs 3.3–4.3 ms.
- Caching only the transcript cut typing from 4.2 to 3.3 ms and hover from 4.5 to 3.6 ms.
- Caching only the chat list changed nothing while chats run: its running rings keep it dirty.

## Steps

1. **Cache the transcript.** In `views.rs`, wrap `self.transcript` in `.cached(StyleRefinement::default().size_full())`. It was left uncached on purpose: a comment there says a replayed transcript skips registering its text for selection, and the Kit then drops the selection for a frame. Fix that first. Either the Kit should re-register selectable text when it reuses a cached view, or the transcript must re-render while a selection exists. Check the Kit's `SelectableText`/`TextView` selection registry in gpui-base.
2. **Cache the other big regions.** The chat list (`panes.rs`, both places), the activity side panel, the project bar and the header. A cached view needs a layout that doesn't depend on its content, such as `size_full()` in a sized parent. The composer's height follows its text, so it either stays uncached or gets a measured fixed height.
3. **Move animations into their own small views.** Each running ring (`chat/list.rs`, `row_status`), spinner (`chat::spinner`) and thinking animation (`chat::thinking_label`) becomes a tiny view. An animation frame then notifies only that view and the views above it. Every view above it must be cheap: the root `Adeline` re-renders on every tick, and its cached children are reused. An animation inside the transcript dirties the transcript. So for the tool-step spinner and the thinking row, either make each transcript row its own cached view, or move the running indicators out of the transcript.
4. **Lift the frame cap.** `chat::ANIMATION_FPS` (30) caps looping animations because every tick redraws the window. Once a tick costs well under 1 ms, remove the cap. Measure the idle scenario with chats running first.
5. **Notify less while streaming.** Every chunk calls `refresh_runtime_views` (`runtime_ui.rs`): it re-syncs the sidebar, which rebuilds a search record per thread and re-runs the chat search, and it notifies the header, composer, control pane and root. Only the transcript and the streaming chat's row need to change per chunk.

## A related cost: one long reply is one row

While a reply streams, a frame's draw time grows with the reply's length: 2.5 ms at the start, 5.7 ms after about 10 KB. Since v0.1.16 the reply's text view gets only the new text (`chat/transcript.rs`), so the Kit no longer re-parses the whole reply per chunk. But a reply is one row of the transcript list, and the Kit's `TextView` builds and lays out every block of it every frame, including blocks scrolled out of view. Caching doesn't help, because the streaming row changes every chunk. Options:

- Split a reply into several list rows at block boundaries, so the list virtualizes them. Splitting must respect markdown (a blank line inside a code block or a list is not a boundary), and selection across rows needs checking.
- Ask GPUI Kit for virtualization inside a non-scrollable `TextView`.

## Done when

`perf_check.py` shows, on a 240 Hz display: draw p95 under 4 ms for hover, typing and scroll; animations at the display's rate with chats running; and stream draw times that stay flat as the reply grows.
