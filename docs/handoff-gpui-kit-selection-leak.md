Status: Draft

# Handoff: report gpui-kit's selection leak upstream, then drop Adeline's workaround

A text selection dragged in one panel can spread into another panel's text. Adeline works around it in `crates/adeline_ui/src/panes.rs` (v0.2.2). The real fix belongs in gpui-kit. Below is the issue to file at https://github.com/longbridge/gpui-kit/issues, then what to remove from Adeline once a gpui-kit release fixes it.

## The issue to file

**Title:** Window text selection jumps into other panes when a drag leaves all text

**gpui-kit version:** 0.7.0. The rule described below is unchanged on `main` as of 2026-10-14 (`crates/base/src/text_selection.rs`, `WindowSelectionState::endpoint`).

**What happens.** A window has two side-by-side panes, each with selectable text: a `TextView` on the left and a list of chat messages on the right. Start a drag on text in the left pane, then move the pointer onto something that is not text: the pane's padding, a toolbar, or a third pane with no selectable text. The selection grows into messages in the right pane that the pointer never touched.

**Steps.**
1. Lay out two panes side by side, each holding one or more selectable `TextView`s or `SelectableText`s. Give the right pane several short text elements stacked down its height.
2. Press on text in the middle of the left pane.
3. Drag up and to the left, out of the text and over blank space or a pane with no selectable text.
4. Text in the right pane at about the pointer's height is highlighted, and it is included when copying.

**Why.** While a drag runs, `endpoint()` resolves the pointer to a participant. When no participant's hitbox is hovered, it falls back to the participant with the greatest `bounds.top()` that is still above the pointer, and failing that to the topmost participant. This search covers every participant in the window. It doesn't consider the participant the drag started in, or which pane the pointer is in. A short message in another pane often has a top edge nearer the pointer than the tall `TextView` the drag started in. That message becomes the cursor endpoint, and `participates()` then selects everything between the two endpoints in document order.

**Why the app can't fix it.**
- `TextSelectionScopeId` would split the panes into separate groups. But gpui-component's `Root` plugin calls `TextSelection::activate_scope` on every render with the active dialog's or sheet's scope, otherwise the default one. A scope the app activates is replaced on the next frame, and changing scope clears the selection.
- No public API lets the app move the cursor endpoint or ask which participant is under the pointer.
- `TextView` has no option to keep its selection inside itself; `selectable(false)` is the only alternative.

**Expected.** When the pointer isn't over any participant, the selection continues in the participant the drag started in, as a selection inside one native text view does. At least, the fallback should never pick a participant that the pointer isn't horizontally over. A container that bounds selection, as proposed in #2110 (like Flutter's `SelectionArea`), would also solve it.

**Suggested change.** In `endpoint()`, when `hit` is `None`, restrict `predecessor` and `first` to participants whose bounds contain the pointer's x. If none qualify, use the anchor's participant and clamp the point to its bounds.

## Adeline's workaround

`panes::panel_area` wraps the contents of every panel in Chats and Docs mode. A left-button press inside a panel records it as the panel the drag started in (the `DragPanel` global). Until the button comes up, `panes::selectable_in` returns false for the other panels. The text renderers draw that text as plain, unselectable text:

- the transcript (`chat/transcript.rs`): replies' `TextView` and the `SelectableText` paragraphs of user and demo messages
- the doc Viewer (`docs/viewer.rs`): `markdown()` and the nested `TextView`s of alert and task blocks

With no participants outside the start panel, gpui-kit's fallback has nowhere else to go. Starting and ending a drag redraws the transcript's cached rows (`Transcript::refresh_all`). `docs_demo` in `crates/adeline_ui/tests/ui.rs` checks the leak with a conversation beside a doc.

## Once gpui-kit fixes it

1. Bump gpui-kit and confirm `docs_demo` passes with the workaround still in place.
2. Remove `DragPanel`, `selectable_in`, `set_drag_panel` and the panel ID constants from `panes.rs`. Pass `content` through `panel_area`'s callers directly, or drop `panel_area`.
3. Return the transcript and the Viewer to always-selectable text: `TextView` with `.selectable(true)` and `SelectableText` paragraphs.
4. Run the leak check again; it should still pass.
