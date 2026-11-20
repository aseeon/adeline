# UI drawing and state

The UI is GPUI views drawn from `Adeline`'s state. Every UI event is an `Action`, which `Adeline::act` dispatches. The headless build leaves the UI crate out.

## Cached views

GPUI redraws a notified view and every view above it, and a cached view that redraws also redraws every view inside it. A frame redraws the views notified since the last frame and the views above them.

- Most regions are cached views (`chat::region`). They read `Adeline`'s state, so `notify_cached_views` notifies them whenever `Adeline` is notified.
- The chat list and the transcript are drawn uncached, and each of their rows is a cached view. A hover or an animation then redraws only its own row. The chat list notifies every `ChatCard` whenever the list itself is notified.
- GPUI leaves a replayed view out of the accessibility tree, so while assistive technology is connected nothing is cached.
- A streamed chunk of reply or thought redraws only the transcript and the side panel. The chat list changes only when the chunk starts a new message. Every other change goes through `refresh_runtime_views`, which redraws all the regions.
- `Transcript`'s `follow` and `reveal` leave redrawing the panel to the caller.

## Rows and sizes

- Sizes are in rems.
- Chat rows and section labels, which include the gap above them, have fixed heights. The list places its stacked labels by arithmetic. A label sticks once its natural place reaches its slot, like CSS `sticky` with a top of the slot times the label height. The current section is the last one to stick.
- Transcript rows keep the height they last measured at the list's current width, and measure again after `Transcript::sync` or `invalidate`.
- A reply keeps its own text view, which gets only the newly streamed tail (`Thread::stream` only appends). The Kit then parses only the last block again, not the whole reply. The view's key includes `created_at`, so a reply replaced at the same index starts a new view.
- Looping animations run at `ANIMATION_FPS`, and `spinner` is the Kit's `Spinner` with that cap.

## Dialogs

The Kit keeps a closed dialog drawn while it fades out. A newer modal may be open by then, so a stale dialog draws nothing in place of the newer modal's content (`ModalContent` in `views.rs`).

## Window chrome

- The main window draws its own title bar on every platform, because the project tabs share it. Windows and Linux put the Kit's caption buttons at its right end. macOS keeps its traffic lights at the left, with the app icon beside them.
- Both rails use `rail` and `rail_button`, so they look and behave the same.
- The macOS menu bar code runs on every platform. Other platforms store the menus and never show them, so the app's own menus stay the same everywhere.
- Menu items dispatch GPUI actions, which reach the focused element first. App-wide handlers in `menu_bar.rs` catch the ones no element handled, such as when focus is in another window.
- `menu_bar::init` runs after `bind_keys`, because macOS shows each item's shortcut from the keymap.

## Themes and icons

- Borders and hovers are white ink in dark themes and black ink in light ones.
- Preferences stay separate from workspace data.
- `theme::apply` runs after the Kit's initialization and after every appearance change.
- GPUI SVGs need a color of their own. `themed_icon.rs` resolves the inherited text color at paint time, so a parent's hover and selection styles reach the icon without freezing a palette value.

## Chat search

`prepared.rs` prepares chat search apart from rendering, so it can run on a worker thread.

- `Group` is in display order, and each group lists its chats newest first.
- The agent counts ignore the agent filter itself.
- Only the latest `Generation` ticket may publish a result.

## Other UI state

- The agent form gets model, effort and mode from the agent through a background probe. The last probe's lists stay while a model change probes again. Under the harness picker, the form installs, updates and logs in supported agents.
- The engine deletes a project everywhere once its agents stop. Closing the stopping dialog keeps the project, but agents already asked to stop keep stopping.
- `ui_state.rs` keeps local projects under the bare IDs that older versions wrote. `migrated` records that the first local snapshot's times, kept by older project files, were adopted.
- Profiling builds write frame times in Unix milliseconds, so a driver script can slice the log by scenario (`frame_log.rs`).
