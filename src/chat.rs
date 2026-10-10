//! Independently invalidated chat regions. The shell remains the shared
//! data/action coordinator; list state and input invalidation live in these views.
use super::*;
use crate::Action;
use crate::chat_render::{LabelPlacement, section_label};
use crate::prepared::{Criteria, Group, Outcome};
use gpui_kit::component::{
    ActiveTheme as _,
    input::{InputEvent, TextareaState},
    scroll::Scrollbar,
};
use std::sync::Arc;

/// Heights of the chat list's items in rems. Rows and section labels have fixed
/// heights so the list can place its stacked section labels by arithmetic.
pub(super) const ROW_HEIGHT: f32 = 1.75;
/// A section label includes the gap above it that sets it off from the chats before.
pub(super) const SECTION_HEIGHT: f32 = 2.125;
pub(super) const LABEL_GAP: f32 = 0.375;
/// Room after the last chat.
const END_HEIGHT: f32 = 0.75;
/// Below this list width, in rems, the search and the empty list shorten their text.
const COMPACT_WIDTH: f32 = 17.;

/// One entry of the virtualized chat list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Item {
    /// The label of the section at this position in `Outcome::groups`.
    Section(usize),
    /// A chat, by index into the workspace's threads.
    Chat(usize),
    /// Closes the list with its chat count.
    End,
    /// Space after the end so the last section can scroll up into the top stack.
    Tail,
}

/// Resolved item geometry, rebuilt every frame from the item heights.
#[derive(Default)]
struct Layout {
    /// Top of every item plus the total height, in pixels.
    tops: Vec<Pixels>,
    /// Item index of every section label.
    sections: Vec<usize>,
    section: Pixels,
    viewport: Pixels,
}

impl Layout {
    fn scroll_offset(&self, top: ListOffset) -> Pixels {
        self.tops[top.item_ix.min(self.tops.len() - 1)] + top.offset_in_item
    }

    fn offset_at(&self, y: Pixels) -> ListOffset {
        let y = y.max(px(0.));
        let item_ix = self
            .tops
            .partition_point(|&top| top <= y)
            .saturating_sub(1)
            .min(self.tops.len().saturating_sub(2));
        ListOffset {
            item_ix,
            offset_in_item: y - self.tops[item_ix],
        }
    }

    /// The section an item belongs to.
    fn section_of(&self, item: usize) -> usize {
        self.sections
            .partition_point(|&start| start <= item)
            .saturating_sub(1)
    }

    /// Which labels sit in the top stack at a scroll offset. A label sticks once
    /// its natural place reaches its slot, like CSS sticky positioning with a top
    /// of slot times the label height.
    fn stacks(&self, scroll: Pixels) -> Stacks {
        let mut top = 0;
        let mut current = 0;
        for (k, &ix) in self.sections.iter().enumerate() {
            let natural = self.tops[ix] - scroll;
            let slot = self.section * k as f32;
            if scroll > px(0.) && natural <= slot + px(0.5) {
                top = k + 1;
            }
            if scroll + px(1.) >= self.tops[ix] - slot {
                current = k;
            }
        }
        Stacks {
            top: 0..top,
            current,
        }
    }

    /// The scroll offset that puts a section's first chat right under the top stack.
    fn jump_target(&self, section: usize) -> Option<Pixels> {
        let ix = *self.sections.get(section)?;
        Some(self.tops[ix] - self.section * section as f32)
    }

    /// The scroll offset that brings an item into view below the top stack, if it
    /// is covered or out of view.
    fn reveal_target(&self, item: usize, scroll: Pixels) -> Option<Pixels> {
        let section = self.section_of(item);
        let covered_top = self.section * (section + 1) as f32;
        let (row_top, row_bottom) = (self.tops[item], self.tops[item + 1]);
        if row_top - scroll < covered_top {
            Some(row_top - covered_top)
        } else if row_bottom - scroll > self.viewport {
            Some(row_bottom - self.viewport)
        } else {
            None
        }
    }
}

/// Where each section label is drawn at the current scroll position.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Stacks {
    /// Sections stacked under the controls, from the first.
    pub top: std::ops::Range<usize>,
    /// The section being read: the last one that reached its place in the top stack.
    pub current: usize,
}

/// What `ChatList::hover_collapsed` records while the pointer is on the
/// collapsed list's search; chat ids never start with a NUL.
pub(super) const COLLAPSED_SEARCH: &str = "\0search";

/// One chat tile of the collapsed list, with its retained focus handle.
#[expect(
    clippy::too_many_arguments,
    reason = "the pinned tile and the virtualized rows share this"
)]
fn collapsed_tile(
    owner: &WeakEntity<Adeline>,
    list: &WeakEntity<ChatList>,
    project: &str,
    id: &str,
    row: usize,
    hovered: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let focus = window
        .use_keyed_state(
            SharedString::from(format!("chat-rail-focus:{project}:{id}")),
            cx,
            |_, cx| cx.focus_handle().tab_stop(true),
        )
        .read(cx)
        .clone();
    let keyboard_focus = focus.is_focused(window) && window.last_input_was_keyboard();
    owner
        .update(cx, |app, cx| {
            app.collapsed_chat(row, hovered, &focus, keyboard_focus, list.clone(), cx)
        })
        .unwrap_or_else(|_| div().into_any_element())
}

pub(super) struct ChatList {
    owner: WeakEntity<Adeline>,
    state: ListState,
    typography: config::Typography,
    records: prepared::SearchCatalog,
    generation: prepared::Generation,
    pending: Option<Task<()>>,
    searching: bool,
    search_bytes: usize,
    project: usize,
    selected: Option<usize>,
    criteria: Criteria,
    outcome: Outcome,
    items: Arc<[Item]>,
    keys: Vec<Arc<str>>,
    tail: Pixels,
    layout: Layout,
    /// Width of the list panel at the last paint; narrow panels drop details.
    width: Pixels,
    /// The search placeholder last set, shortened with the tabs.
    placeholder: &'static str,
    /// The collapsed list's chat under the pointer, by thread id.
    collapsed_hover: Option<Arc<str>>,
    /// Scroll position of the collapsed list's chats.
    collapsed_scroll: UniformListScrollHandle,
    /// Sections whose chats are hidden under their label.
    folded: Vec<Group>,
}

impl ChatList {
    pub fn new(owner: WeakEntity<Adeline>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(180.));
        Self {
            owner,
            state,
            typography: config::typography(),
            records: Default::default(),
            generation: Default::default(),
            pending: None,
            searching: false,
            search_bytes: 0,
            project: usize::MAX,
            selected: None,
            criteria: Criteria {
                completed: true,
                ..Criteria::default()
            },
            outcome: Outcome::default(),
            items: Arc::from([]),
            keys: Vec::new(),
            tail: px(0.),
            layout: Layout::default(),
            width: px(0.),
            placeholder: "Search chats",
            collapsed_hover: None,
            collapsed_scroll: UniformListScrollHandle::new(),
            // Finished chats stay out of the way until asked for.
            folded: vec![Group::Completed],
        }
    }

    pub fn sync(&mut self, app: &Adeline, criteria: Criteria, cx: &mut Context<Self>) {
        let records: prepared::SearchCatalog = app
            .workspace()
            .threads
            .iter()
            .map(Thread::search_record)
            .collect();
        if self.project != app.project
            || self.records.len() != records.len()
            || self
                .records
                .iter()
                .zip(records.iter())
                .any(|(a, b)| a.id != b.id)
        {
            self.items = Arc::from([]);
            self.keys.clear();
            self.state.reset(0);
        }
        self.project = app.project;
        self.search_bytes = records.iter().map(|r| r.text.len()).sum();
        self.records = records;
        self.selected = app.selected;
        self.search(criteria, false, cx);
    }

    /// A model mutation identifies one stable thread; selection alone is view state.
    fn update_thread(&mut self, app: &Adeline, cx: &mut Context<Self>) {
        if self.project != app.project || self.records.len() != app.workspace().threads.len() {
            self.sync(app, self.criteria.clone(), cx);
            return;
        }
        self.selected = app.selected;
        if let Some(index) = app.selected {
            let thread = &app.workspace().threads[index];
            if self.records[index].id.as_ref() != thread.id {
                self.sync(app, self.criteria.clone(), cx);
                return;
            }
            let old = &self.records[index];
            let record = thread.search_record();
            let unchanged = Arc::ptr_eq(&old.text, &record.text)
                && old.completed == record.completed
                && old.archived == record.archived
                && old.blocked == record.blocked
                && old.processing == record.processing
                && old.unread == record.unread
                && old.activity == record.activity;
            if !unchanged {
                self.search_bytes = self.search_bytes - old.text.len() + record.text.len();
                self.records[index] = record;
                // Status and activity move chats between sections, so the
                // sections are rebuilt. Explicit actions skip the typing delay.
                self.search(self.criteria.clone(), false, cx);
                return;
            }
        }
        self.invalidate_selection();
        cx.notify();
    }

    pub fn search(&mut self, criteria: Criteria, debounce: bool, cx: &mut Context<Self>) {
        self.pending.take();
        self.criteria = criteria.clone();
        let ticket = self.generation.next();
        let records = self.records.clone();
        let now = recency::now();
        if criteria.query.is_empty() || (records.len() < 256 && self.search_bytes < 64 * 1024) {
            self.publish(prepared::search(records.iter(), &criteria, now), cx);
        } else {
            self.searching = true;
            let executor = cx.background_executor().clone();
            self.pending = Some(cx.spawn(async move |this, cx| {
                if debounce {
                    executor.timer(std::time::Duration::from_millis(30)).await;
                }
                let outcome = executor
                    .spawn(async move { prepared::search(records.iter(), &criteria, now) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.generation.accepts(ticket) {
                        this.publish(outcome, cx);
                    }
                });
            }));
            cx.notify();
        }
    }

    fn publish(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        let mut items = Vec::new();
        for (slot, (group, rows)) in outcome.groups.iter().enumerate() {
            items.push(Item::Section(slot));
            if !self.folded.contains(group) {
                items.extend(rows.iter().map(|&row| Item::Chat(row)));
            }
        }
        if !items.is_empty() {
            items.extend([Item::End, Item::Tail]);
        }
        let keys: Vec<Arc<str>> = items
            .iter()
            .map(|item| match item {
                Item::Section(slot) => Arc::from(format!("\0{}", outcome.groups[*slot].0.title())),
                Item::Chat(row) => self.records[*row].id.clone(),
                Item::End => Arc::from("\0end"),
                Item::Tail => Arc::from("\0tail"),
            })
            .collect();
        let (old, new) = prepared::changed_range(&self.keys, &keys);
        if !old.is_empty() || !new.is_empty() {
            self.state.splice(old, new.len());
        }
        // Counts and separators live inside rows that kept their keys.
        invalidate_range(&self.state, 0..self.state.item_count());
        self.outcome = outcome;
        self.items = items.into();
        self.keys = keys;
        self.searching = false;
        cx.notify();
    }

    /// Selection draws the row fill and hides the separator above it.
    fn invalidate_selection(&self) {
        invalidate_range(&self.state, 0..self.state.item_count());
    }

    fn measure(&mut self, window: &Window) {
        let rem = window.rem_size();
        let (row, section, end) = (rem * ROW_HEIGHT, rem * SECTION_HEIGHT, rem * END_HEIGHT);
        let mut tops = Vec::with_capacity(self.items.len() + 1);
        let mut sections = Vec::new();
        let mut y = px(0.);
        for (ix, item) in self.items.iter().enumerate() {
            tops.push(y);
            y += match item {
                Item::Section(_) => {
                    sections.push(ix);
                    section
                }
                Item::Chat(_) => row,
                Item::End => end,
                Item::Tail => self.tail,
            };
        }
        tops.push(y);
        self.layout = Layout {
            tops,
            sections,
            section,
            viewport: self.state.viewport_bounds().size.height,
        };
        // When the chats overflow, add room after the end so the last section can
        // reach its place in the top stack, as every earlier section can.
        let tail = match self.layout.sections.last() {
            Some(&last) if self.layout.viewport > px(0.) => {
                let content = y - self.tail;
                let reach = self.layout.tops[last]
                    - section * (self.layout.sections.len() - 1) as f32
                    + self.layout.viewport;
                if content > self.layout.viewport {
                    (reach - content).max(px(0.))
                } else {
                    px(0.)
                }
            }
            _ => px(0.),
        };
        if (tail - self.tail).abs() > px(0.5) {
            let delta = tail - self.tail;
            self.tail = tail;
            if let Some(last) = self.layout.tops.last_mut() {
                *last += delta;
            }
            let ix = self.items.len() - 1;
            invalidate_range(&self.state, ix..ix + 1);
        }
    }

    fn scroll(&self) -> Pixels {
        self.layout.scroll_offset(self.state.logical_scroll_top())
    }

    /// Show or hide a section's chats under its label.
    fn toggle_fold(&mut self, group: Group, cx: &mut Context<Self>) {
        if let Some(ix) = self.folded.iter().position(|&folded| folded == group) {
            self.folded.remove(ix);
        } else {
            self.folded.push(group);
        }
        self.publish(self.outcome.clone(), cx);
    }

    /// Scroll so the first chat of a section sits right under the top stack.
    pub fn jump(&mut self, section: usize, cx: &mut Context<Self>) {
        if let Some(target) = self.layout.jump_target(section) {
            self.state.scroll_to(self.layout.offset_at(target));
            cx.notify();
        }
    }

    /// Keep a chat clear of the top stack and in view after keyboard selection.
    fn reveal(&self, item: usize) {
        if let Some(target) = self.layout.reveal_target(item, self.scroll()) {
            self.state.scroll_to(self.layout.offset_at(target));
        }
    }

    /// A collapsed chat unfurls when the pointer enters its tile and folds when
    /// the pointer leaves its flyout.
    pub fn hover_collapsed(&mut self, id: &Arc<str>, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(id.clone())
        } else if self.collapsed_hover.as_ref() == Some(id) {
            None
        } else {
            return;
        };
        if self.collapsed_hover != next {
            self.collapsed_hover = next;
            cx.notify();
        }
    }

    /// The collapsed list: new chat and search above the agent tiles of the
    /// current and today's chats, without section labels.
    fn render_collapsed(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        // While a search is typed, the scrolling tiles are its results instead.
        let searching = !self.criteria.query.is_empty();
        let selected = self.selected.filter(|&row| row < self.records.len());
        let (rows, project, search_focused) = {
            let app = owner.read(cx);
            let rows: Vec<usize> = if searching {
                self.outcome
                    .groups
                    .iter()
                    .flat_map(|(_, rows)| rows.iter().copied())
                    .filter(|&row| Some(row) != selected)
                    .collect()
            } else {
                prepared::rail(
                    &self.records,
                    app.show_completed,
                    app.show_archived,
                    selected,
                    recency::now(),
                )
            };
            (
                rows,
                SharedString::from(app.workspace().config.id.clone()),
                app.query.focus_handle(cx).is_focused(window),
            )
        };
        let list = cx.weak_entity();
        // The open chat stays pinned under search while the others scroll.
        let pinned = selected.and_then(|row| {
            let id = self.records.get(row)?.id.clone();
            let hovered = self.collapsed_hover.as_ref() == Some(&id);
            Some(collapsed_tile(
                &self.owner,
                &list,
                &project,
                &id,
                row,
                hovered,
                window,
                cx,
            ))
        });
        let items: Arc<[(usize, Arc<str>)]> = rows
            .into_iter()
            .filter_map(|row| Some((row, self.records.get(row)?.id.clone())))
            .collect();
        let count = items.len();
        let chats = uniform_list("chat-rail-items", count, {
            let (owner, list, hover) = (
                self.owner.clone(),
                list.clone(),
                self.collapsed_hover.clone(),
            );
            move |range, window, cx| {
                range
                    .map(|ix| {
                        let (row, id) = &items[ix];
                        let hovered = hover.as_ref() == Some(id);
                        chat_render::collapsed_item(collapsed_tile(
                            &owner, &list, &project, id, *row, hovered, window, cx,
                        ))
                    })
                    .collect::<Vec<_>>()
            }
        })
        .track_scroll(&self.collapsed_scroll)
        .size_full();
        let search_open =
            search_focused || self.collapsed_hover.as_deref() == Some(COLLAPSED_SEARCH);
        let matches = searching.then(|| self.outcome.matches());
        let (new_chat, search) = owner.update(cx, |app, cx| {
            let new_chat = chat_render::collapsed_button("chat-rail-new", "New chat", "new-chat")
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::NewChat, window, cx)));
            let search = app.collapsed_search(search_open, matches, list.clone(), cx);
            (new_chat, search)
        });
        col()
            .id("chat-rail")
            .h_full()
            .w(rems(chat_render::COLLAPSED_WIDTH))
            // New, search, the open chat and the first scrolling chat sit one gap
            // apart, and one gap below the top.
            .pt_2()
            .gap_2()
            .items_center()
            .child(new_chat)
            .child(search)
            .child(
                col()
                    .id("chat-rail-list")
                    .role(Role::ListBox)
                    .aria_label(if searching { "Search results" } else { "Chats" })
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .children(pinned)
                    // No scrollbar: it would cover the tiles in a rail this narrow.
                    .child(div().flex_1().min_h_0().w_full().child(chats)),
            )
            .into_any_element()
    }

    /// The chat a navigation key moves to, as (item, thread index).
    fn navigation_target(&self, key: &str) -> Option<(usize, usize)> {
        let chats: Vec<(usize, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(ix, item)| match item {
                Item::Chat(row) => Some((ix, *row)),
                _ => None,
            })
            .collect();
        if chats.is_empty() {
            return None;
        }
        let current = self
            .selected
            .and_then(|selected| chats.iter().position(|&(_, row)| row == selected));
        let target = match key {
            "up" => current.unwrap_or(0).saturating_sub(1),
            "down" => current.map_or(0, |ix| (ix + 1).min(chats.len() - 1)),
            "home" => 0,
            "end" => chats.len() - 1,
            _ => return None,
        };
        Some(chats[target])
    }
}

impl Render for ChatList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = config::typography();
        if self.typography != typography {
            self.typography = typography;
            let top = self.state.logical_scroll_top();
            let count = self.state.item_count();
            self.state.splice(0..count, count);
            self.state.scroll_to(top);
        }
        if self
            .owner
            .upgrade()
            .is_some_and(|owner| !owner.read(cx).left_panel_is_open())
        {
            return self.render_collapsed(window, cx);
        }
        self.collapsed_hover = None;
        self.measure(window);
        let stacks = self.layout.stacks(self.scroll());
        let this = cx.weak_entity();
        let measured = this.clone();
        let rem = window.rem_size();
        // Compact tabs share the bar evenly: each is capped at a third of the
        // room inside the list's side padding and the segmented bar's inset and
        // gaps, so the labels give up tab padding instead of clipping the last tab.
        let compact = self.width > px(0.) && self.width < rem * COMPACT_WIDTH;
        if let Some(query) = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).query.clone())
        {
            let placeholder = if compact { "Search" } else { "Search chats" };
            if self.placeholder != placeholder {
                self.placeholder = placeholder;
                query.update(cx, |query, cx| {
                    query.set_placeholder(placeholder, window, cx);
                });
            }
        }
        let list_focus = window
            .use_keyed_state("chat-list-keyboard", cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let filtered = !self.criteria.query.is_empty()
            || self.criteria.filter != 0
            || self.criteria.agent.is_some();
        let rows = if self.items.is_empty() {
            let owner = self.owner.clone();
            // A narrow list tightens the message and shortens the button.
            col()
                .flex_1()
                .when(compact, |empty| empty.px_3().py_5())
                .when(!compact, |empty| empty.p_5())
                .gap_3()
                .items_start()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if self.searching {
                    "Searching…".to_owned()
                } else if !self.criteria.query.is_empty() {
                    format!("No chats match “{}”.", self.criteria.query)
                } else if filtered {
                    "No chats match these filters.".to_owned()
                } else {
                    "No chats yet.".to_owned()
                })
                .when(filtered && !self.searching, |empty| {
                    empty.child(
                        Button::new("clear-chat-filters")
                            .outline()
                            .small()
                            .max_w_full()
                            .label(if compact {
                                "Clear filters"
                            } else {
                                "Clear search and filters"
                            })
                            .accessibility_label("Clear search and filters")
                            .when(compact, |button| button.tooltip("Clear search and filters"))
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |app, cx| {
                                    app.act(Action::ClearChatFilters, window, cx);
                                });
                            }),
                    )
                })
                .into_any_element()
        } else {
            let owner = self.owner.clone();
            let items = self.items.clone();
            let groups: Arc<[(Group, Vec<usize>)]> = self.outcome.groups.clone().into();
            let folded: Arc<[Group]> = self.folded.clone().into();
            let selected = self.selected;
            let tail = self.tail;
            let current = stacks.current;
            let section_view = {
                let this = this.clone();
                move |slot: usize, placement: LabelPlacement, cx: &mut App| {
                    let (group, rows) = &groups[slot];
                    let group = *group;
                    section_label(
                        group,
                        rows.len(),
                        folded.contains(&group),
                        slot == current,
                        placement,
                        {
                            let this = this.clone();
                            // A label in the list folds its section; a pinned one
                            // scrolls to it.
                            move |_, _, cx: &mut App| {
                                let _ = this.update(cx, |list, cx| match placement {
                                    LabelPlacement::Inline => list.toggle_fold(group, cx),
                                    LabelPlacement::Top { .. } => list.jump(slot, cx),
                                });
                            }
                        },
                        cx,
                    )
                }
            };
            let list_view = {
                let section_view = section_view.clone();
                list(self.state.clone(), move |ix, _, cx| match items[ix] {
                    Item::Section(slot) => section_view(slot, LabelPlacement::Inline, cx),
                    Item::Chat(row) => owner
                        .update(cx, |app, cx| app.chat_card(row, cx))
                        .unwrap_or_else(|_| div().into_any_element()),
                    Item::End => div().h(rems(END_HEIGHT)).into_any_element(),
                    Item::Tail => div().h(tail).into_any_element(),
                })
                .size_full()
                .pr(theme::SCROLLBAR_TRACK)
            };
            let top_stack = col()
                .absolute()
                .top_0()
                .left_0()
                .right(theme::SCROLLBAR_TRACK)
                .children(stacks.top.clone().map(|slot| {
                    section_view(
                        slot,
                        LabelPlacement::Top {
                            last: slot + 1 == stacks.top.end,
                        },
                        cx,
                    )
                }));
            div()
                .id("chat-list-viewport")
                .role(Role::ListBox)
                .aria_label("Chats")
                .relative()
                .flex_1()
                .min_h_0()
                .track_focus(&list_focus)
                .on_key_down({
                    let owner = self.owner.clone();
                    move |event, window, cx| {
                        if event.keystroke.modifiers.modified() {
                            return;
                        }
                        // Selecting runs the shell's action, which updates this list again,
                        // so the target is read first and the action runs outside the list.
                        let Some((_, row)) = this.upgrade().and_then(|list| {
                            let list = list.read(cx);
                            let target = list.navigation_target(&event.keystroke.key)?;
                            list.reveal(target.0);
                            Some(target)
                        }) else {
                            return;
                        };
                        list_focus.focus(window, cx);
                        if selected != Some(row) {
                            let _ =
                                owner.update(cx, |app, cx| app.act(Action::Chat(row), window, cx));
                        }
                        window.refresh();
                        cx.stop_propagation();
                    }
                })
                .child(list_view)
                .child(top_stack)
                .child(Scrollbar::vertical(&self.state))
                .into_any_element()
        };
        let focused = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).query.focus_handle(cx).is_focused(window));
        let sidebar = self
            .owner
            .update(cx, |app, cx| {
                app.chat_sidebar(rows, &self.outcome, focused, cx)
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element());
        div()
            .relative()
            .size_full()
            .child(sidebar)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = measured.update(cx, |list, cx| {
                            if (list.width - bounds.size.width).abs() > px(0.5) {
                                list.width = bounds.size.width;
                                cx.notify();
                            }
                        });
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }
}

pub(super) struct Transcript {
    owner: WeakEntity<Adeline>,
    state: ListState,
    typography: config::Typography,
    thread: Option<(usize, String)>,
    selected: Option<usize>,
    messages: usize,
    footer_focus: FocusHandle,
    /// Height of the composer floating over the end of the transcript.
    composer_height: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// Distance from the end at the last wheel scroll.
    left_to_end: Pixels,
    /// Following the end at the last scroll; the activity panel follows with it.
    following: bool,
}

impl Transcript {
    pub fn new(
        owner: WeakEntity<Adeline>,
        composer_height: std::rc::Rc<std::cell::Cell<Pixels>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(250.));
        // Stick to the end while the reader is at the bottom; scrolling up
        // pauses this and scrolling back down resumes it.
        state.set_follow_mode(FollowMode::Tail);
        // Gpui only resumes within 1px of the end; resume once a downward wheel
        // scroll brings the last line close to the composer. Deferred because
        // the list is still borrowed while its handler runs.
        let view = cx.weak_entity();
        state.set_scroll_handler(move |_, window, cx| {
            let view = view.clone();
            let near = window.rem_size() * 3.;
            cx.defer(move |cx| {
                view.update(cx, |transcript, cx| {
                    transcript.resume_near_end(near);
                    transcript.note_following(cx);
                })
                .ok();
            });
        });
        Self {
            owner,
            state,
            typography: config::typography(),
            thread: None,
            selected: None,
            messages: 0,
            footer_focus: cx.focus_handle(),
            composer_height,
            left_to_end: px(0.),
            following: true,
        }
    }

    /// Whether the transcript sticks to its end as messages arrive.
    pub fn following(&self) -> bool {
        self.state.is_following_tail()
    }

    /// Scrolls to the end and follows it again. The caller redraws the panel.
    pub fn follow(&mut self, cx: &mut Context<Self>) {
        self.state.set_follow_mode(FollowMode::Tail);
        self.following = true;
        cx.notify();
    }

    /// Scrolls a message into view, which stops following the end. The caller
    /// redraws the panel.
    pub fn reveal(&mut self, message: usize, cx: &mut Context<Self>) {
        self.state.scroll_to(ListOffset {
            item_ix: message,
            offset_in_item: px(0.),
        });
        self.following = self.state.is_following_tail();
        cx.notify();
    }

    /// Tells the activity panel when scrolling starts or stops following.
    fn note_following(&mut self, cx: &mut Context<Self>) {
        let following = self.state.is_following_tail();
        if following != self.following {
            self.following = following;
            self.owner.update(cx, |_, cx| cx.notify()).ok();
        }
    }

    fn resume_near_end(&mut self, near: Pixels) {
        let left =
            self.state.max_offset_for_scrollbar().y + self.state.scroll_px_offset_for_scrollbar().y;
        // Only while moving down, so a small scroll up near the end can leave it.
        if !self.state.is_following_tail() && left <= near && left < self.left_to_end {
            self.state.set_follow_mode(FollowMode::Tail);
        }
        self.left_to_end = left;
    }

    pub fn sync(&mut self, app: &Adeline, scroll_to_end: bool, cx: &mut Context<Self>) {
        let thread = app.selected.map(|i| &app.workspace().threads[i]);
        let key = thread.map(|t| (app.project, t.id.clone()));
        let messages = thread.map_or(0, |t| t.messages.len());
        let footer = thread.is_some_and(|t| {
            !app.demo_mode
                || t.status == "blocked"
                || app.runtime.conversations.contains_key(&t.id)
                || app
                    .workspace()
                    .decisions
                    .iter()
                    .any(|d| d.thread_id == t.id)
        });
        let count = messages + usize::from(footer);
        let previous_top = self.state.logical_scroll_top();
        let changed_thread = key != self.thread;
        if changed_thread {
            self.state.reset(0);
        }
        // Streaming can change the final message without appending another row.
        let start = if changed_thread {
            0
        } else if app.demo_mode {
            self.messages.min(messages)
        } else {
            self.messages.saturating_sub(1).min(messages)
        };
        if start != count || start != self.state.item_count() {
            self.state.splice_focusable(
                start..self.state.item_count(),
                (start..count).map(|i| (i == messages).then(|| self.footer_focus.clone())),
            );
        }
        if scroll_to_end || changed_thread {
            self.state.set_follow_mode(FollowMode::Tail);
        } else if !self.state.is_following_tail() {
            self.state.scroll_to(previous_top);
        }
        self.thread = key;
        self.selected = app.selected;
        self.messages = messages;
        cx.notify();
    }
}

impl Render for Transcript {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = config::typography();
        if self.typography != typography {
            self.typography = typography;
            self.state.remeasure();
        }
        // The header and composer float over the transcript so its scrollbar runs
        // the full height; the messages start and end clear of both.
        let gap = window.rem_size() * 0.5;
        let top = window.rem_size() * chat_render::CHAT_HEADER_HEIGHT + gap;
        let bottom = self.composer_height.get() + gap;
        let Some(selected) = self.selected else {
            return self
                .owner
                .update(cx, |app, _cx| app.welcome().size_full().into_any_element())
                .unwrap_or_else(|_| div().into_any_element());
        };
        let owner = self.owner.clone();
        let messages = self.messages;
        let footer_focus = self.footer_focus.clone();
        let rows = list(self.state.clone(), move |row, _, cx| {
            owner
                .update(cx, |app, cx| {
                    if row < messages {
                        app.message_row(selected, row, cx)
                    } else {
                        // Wrapped like message rows so the column centers in the row.
                        div()
                            .w_full()
                            .min_w_0()
                            .track_focus(&footer_focus)
                            .child(chat_render::chat_column().flex().flex_col().child(
                                if app.demo_mode {
                                    col()
                                        .child(app.decision_row(selected, cx))
                                        .child(app.runtime_footer(selected, cx))
                                        .into_any_element()
                                } else {
                                    app.runtime_footer(selected, cx)
                                },
                            ))
                            .into_any_element()
                    }
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full()
        .pt(top)
        .pb(bottom);
        div()
            .relative()
            .size_full()
            .min_h_0()
            .child(rows)
            .child(Scrollbar::vertical(&self.state))
            .into_any_element()
    }
}

fn invalidate_range(state: &ListState, range: std::ops::Range<usize>) {
    let top = state.logical_scroll_top();
    state.splice(range.clone(), range.len());
    state.scroll_to(top);
}

pub(super) struct Composer {
    owner: WeakEntity<Adeline>,
    /// Width at the last paint; a narrow composer shortens its labels.
    width: Pixels,
    _content_subscription: Subscription,
}

impl Composer {
    pub fn new(
        owner: WeakEntity<Adeline>,
        input: &Entity<TextareaState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            owner,
            width: px(0.),
            _content_subscription: subscription,
        }
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Unmeasured counts as wide, so the first frame shows full labels.
        let width = if self.width > px(0.) {
            self.width / window.rem_size()
        } else {
            f32::MAX
        };
        let measured = cx.entity();
        self.owner
            .update(cx, |app, cx| {
                app.composer_view(width, cx)
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                measured.update(cx, |composer, cx| {
                                    if (composer.width - bounds.size.width).abs() > px(0.5) {
                                        composer.width = bounds.size.width;
                                        cx.notify();
                                    }
                                });
                            },
                            |_, (), _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(super) struct Header(pub WeakEntity<Adeline>);
impl Render for Header {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |app, cx| {
                app.header(window, cx).w_full().into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Adeline {
    /// What the chat list should show for the current search and filters.
    pub(super) fn chat_criteria(&self, cx: &App) -> Criteria {
        Criteria {
            query: self.query(cx),
            filter: self.filter,
            completed: self.show_completed,
            archived: self.show_archived,
            agent: self.agent_filter.clone(),
        }
    }
    pub(super) fn search_sidebar(&self, cx: &mut Context<Self>) {
        let criteria = self.chat_criteria(cx);
        self.chat_list.update(cx, |list, cx| {
            // Programmatic input resets can emit the same InputEvent::Change.
            if list.criteria != criteria {
                list.search(criteria, true, cx);
            }
        });
    }
    pub(super) fn sync_sidebar(&self, cx: &mut Context<Self>) {
        let criteria = self.chat_criteria(cx);
        self.chat_list
            .update(cx, |list, cx| list.sync(self, criteria, cx));
    }

    pub(super) fn sync_regions(&self, action: &Action, cx: &mut Context<Self>) {
        use crate::Action::*;
        if matches!(
            action,
            Section(_)
                | Project(_)
                | NewChat
                | ShowCompleted
                | ShowArchived
                | HideToolCalls
                | SubmitOnEnter
                | ToggleLeftPanel
                | ToggleSidePanel
                | TrafficView
                | PanelTab(_)
        ) {
            self.control_pane.update(cx, |_, cx| cx.notify());
        }
        // Data mutations notify their dependents explicitly. Opening overlays,
        // changing focus and changing unrelated sections do not invalidate them.
        match action {
            Project(_) => self.sync_sidebar(cx),
            Section(crate::Section::Chats)
            | NewChat
            | AgentFilter(_)
            | ClearChatFilters
            | ShowCompleted
            | ShowArchived => {
                let criteria = self.chat_criteria(cx);
                self.chat_list.update(cx, |list, cx| {
                    list.selected = self.selected;
                    list.search(criteria, false, cx);
                });
            }
            Chat(_) | Complete | Send | Decision(_) => {
                self.chat_list
                    .update(cx, |list, cx| list.update_thread(self, cx));
            }
            _ => {}
        }
        if matches!(
            action,
            Project(_) | Chat(_) | NewChat | Complete | Send | Decision(_)
        ) {
            let end = matches!(action, Send);
            self.transcript
                .update(cx, |view, cx| view.sync(self, end, cx));
        }
        if matches!(
            action,
            Project(_) | Chat(_) | NewChat | Send | Complete | Agent(_) | Machine(_)
        ) {
            self.composer_region.update(cx, |_, cx| cx.notify());
        }
        if matches!(
            action,
            Project(_)
                | Machine(_)
                | Section(_)
                | Chat(_)
                | NewChat
                | Complete
                | Send
                | Decision(_)
                | SaveSettings
        ) {
            self.header_region.update(cx, |_, cx| cx.notify());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Layout, Stacks, invalidate_range};
    use gpui_kit::{ListAlignment, ListOffset, ListState, Pixels, px};

    /// Three sections of 10, 10 and 4 chats: labels 32 px, rows 52 px, a 400 px viewport.
    fn layout() -> Layout {
        let (label, row) = (px(32.), px(52.));
        let mut tops = Vec::new();
        let mut sections = Vec::new();
        let mut y = px(0.);
        for rows in [10, 10, 4] {
            sections.push(tops.len());
            tops.push(y);
            y += label;
            for _ in 0..rows {
                tops.push(y);
                y += row;
            }
        }
        tops.push(y);
        Layout {
            tops,
            sections,
            section: label,
            viewport: px(400.),
        }
    }

    fn at(layout: &Layout, scroll: f32) -> Stacks {
        layout.stacks(px(scroll))
    }

    #[test]
    fn labels_stack_under_the_controls() {
        let layout = layout();
        // At the top, every label is in its place in the list.
        assert_eq!(
            at(&layout, 0.),
            Stacks {
                top: 0..0,
                current: 0
            }
        );
        // The second label reaches its slot under the first and stays there.
        let second = f32::from(layout.tops[layout.sections[1]]) - 32.;
        assert_eq!(at(&layout, second - 1.).top, 0..1);
        assert_eq!(at(&layout, second).top, 0..2);
        assert_eq!(at(&layout, second).current, 1);
        assert_eq!(at(&layout, second + 300.).top, 0..2);
        // At the last section's jump target every label is stacked at the top.
        let last = f32::from(layout.jump_target(2).unwrap());
        assert_eq!(
            at(&layout, last),
            Stacks {
                top: 0..3,
                current: 2
            }
        );
    }

    #[test]
    fn jumping_puts_the_first_chat_right_under_the_stack() {
        let layout = layout();
        for section in 0..3 {
            let scroll = layout.jump_target(section).unwrap();
            let first_chat = layout.tops[layout.sections[section] + 1];
            assert_eq!(first_chat - scroll, px(32.) * (section + 1) as f32);
            assert_eq!(at(&layout, f32::from(scroll)).current, section);
        }
    }

    #[test]
    fn offsets_round_trip_through_items() {
        let layout = layout();
        for y in [0., 31., 32., 500., 1200.] {
            let offset = layout.offset_at(px(y));
            assert_eq!(layout.scroll_offset(offset), px(y));
        }
    }

    #[test]
    fn keyboard_reveal_keeps_rows_clear_of_the_stack_and_in_view() {
        let layout = layout();
        let row_under_top_stack = layout.sections[1] + 1;
        let scroll = layout.tops[row_under_top_stack] - px(40.);
        let target = layout.reveal_target(row_under_top_stack, scroll).unwrap();
        assert_eq!(layout.tops[row_under_top_stack] - target, px(64.));
        let row_below_view = layout.sections[0] + 8;
        let target = layout.reveal_target(row_below_view, px(0.)).unwrap();
        let bottom_edge: Pixels = layout.tops[row_below_view + 1] - target;
        assert_eq!(bottom_edge, px(400.));
        assert_eq!(layout.reveal_target(layout.sections[0] + 2, px(0.)), None);
    }

    #[test]
    fn remeasuring_variable_rows_preserves_the_scroll_anchor() {
        let list = ListState::new(1000, ListAlignment::Top, px(180.));
        list.scroll_to(ListOffset {
            item_ix: 600,
            offset_in_item: px(29.),
        });
        invalidate_range(&list, 600..601);
        assert_eq!(list.logical_scroll_top().item_ix, 600);
        assert_eq!(list.logical_scroll_top().offset_in_item, px(29.));
        assert_eq!(list.item_count(), 1000);
    }
}
