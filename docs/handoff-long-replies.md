Status: Draft

# Handoff: draw a long reply as several transcript rows

The goal is a streamed reply whose frame time stays flat as it grows. In v0.1.20 a frame costs about 2 ms when a reply starts streaming and about 4.6 ms once it reaches 10 KB, past the 4.17 ms budget of a 240 Hz display. Measure frame times with the profiling build's `ADELINE_FRAME_LOG` while a long reply streams (see `scripts/engine-check/README.md`).

This picks up "A related cost: one long reply is one row" from the view caching work (`docs/archive/handoff-view-caching.md`). Read the "GPUI facts" section below before changing the transcript.

## What happens today

Each message is one row of the transcript list. The rows are cached views (`Row` in `chat/transcript.rs`) that keep their last measured size, and a row draws again only when it is notified or marked for measuring. While a reply streams:

- every chunk marks the reply's row for measuring (`Transcript::sync`), so the row draws uncached and the list measures it
- the reply's `TextView` builds and lays out every block of the reply each time the row draws, including blocks scrolled out of view
- the transcript follows the end, so each chunk scrolls the list, and every visible row draws again (a cached view's cache key includes its position)

Since v0.1.16 the reply's text view gets only the new text, so the Kit no longer parses the whole reply per chunk. The layout of every block is what still grows.

Measured on v0.1.20 (240 Hz display, `STREAM 1500`, about 6.5 bytes per chunk at 100 chunks per second):

| Time into the reply | Reply size | Draw p50 |
|---|---|---|
| 0-2 s | 0-1 KB | 2.08 ms |
| 6-8 s | 4-5 KB | 3.40 ms |
| 14-16 s | 9-10 KB | 4.58 ms |

## Options

1. **Split a reply into several list rows at block boundaries.** The list virtualizes rows, so blocks out of view are neither built nor laid out, and a chunk measures only the last block's row. Things to settle:
   - Boundaries must follow markdown. A blank line inside a fenced code block, a list or a table is not a boundary.
   - A streamed chunk appends to the last block, and can close it and start new ones. Only rows from the first changed block on need measuring (the `start` index in `Transcript::sync` does this for messages today).
   - Each block needs its own text view with its own key, or one text view must be shared across rows. `reply_text` keys a reply's view by thread, message index and `created_at`.
   - The Reply, Copy and Fork buttons, the hover-lit highlight from the activity panel and `Transcript::reveal` are per message. They need a message-to-rows map.
   - Selection across rows: the Kit orders selectable text by document order across participants. Check that a drag across two block rows selects and copies both, with the selection checks in `docs_demo` (`crates/adeline_ui/tests/ui.rs`) as a model.
2. **Ask GPUI Kit to virtualize a non-scrollable `TextView`,** so it lays out only the blocks inside the visible bounds. This keeps one row per message, and helps every `TextView` user.

Option 2 is less code in Adeline if the Kit takes it. Option 1 works without waiting on the Kit.

## GPUI facts

Learned while caching views in v0.1.17-v0.1.20, against gpui-pre 0.3.7 and Kit 0.7.0:

- A notified view draws again together with every view above it.
- A cached view that draws again also draws every view inside it, cached or not. Animated or often-changing parts therefore need uncached ancestors, which is why the transcript and the chat list are uncached and cache their rows.
- A cached view is laid out from its style, without measuring its content. Its size must come from its parent or from a known value. Transcript rows use the height they last measured at the list's current width. A probe that prepaints before the list records that width.
- A cached view's cache key includes its bounds, position included, so a row that moves draws again.
- A replayed view leaves the accessibility tree. While assistive technology is connected, nothing is cached (`chat::region`, and the row closure in `Transcript::render`).
- A replayed view's window control areas are not replayed, which is why Kit's title bar stays uncached.
- An animation with `with_max_fps` schedules its next frame on a timer. Windows timers tick every 15.6 ms, so a 30 fps cap runs at about 21 fps.

## Also open after view caching

- **The frame cap.** `chat::ANIMATION_FPS` still caps looping animations at 30 fps. Without the cap, animations ran at 240 fps with draw p95 at 3.7 ms, but the process used a whole CPU core for as long as any chat was running, against about 10-30% with the cap. Remove it once a frame with only an animation in it costs well under 1 ms.
- **Assistive technology.** With the accessibility tree on, nothing is cached, so a frame costs what it did in v0.1.16. Each region's animations now run on their own timers, so with chats running the idle window draws about 44 frames per second instead of 22, and its CPU use went from 15% to 33%. Coalescing the animation timers, or a lower cap while the tree is on, would bring it back.
- **The parts drawn every frame.** About 2 to 2.5 ms of every frame is the uncached frame around the cached regions: Kit's title bar, the composer, the control bar, the mode rail, the chat list's sidebar and the transcript's list. The composer is the largest candidate. It could be cached with a measured height, like transcript rows, and drawn uncached when its text changes.

## Done when

The frame log of a long streamed reply shows draw p50 within 0.5 ms of its first two seconds for the whole reply, and draw p95 under 4 ms.
