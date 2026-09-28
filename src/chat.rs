//! Independently invalidated chat regions. The shell remains the shared
//! data/action coordinator; list state and input invalidation live in these views.
use super::*;
use crate::Action;
use crate::chat_render::{LabelPlacement, Rail, RowContext, end_label, section_label};
use crate::prepared::{Criteria, Group, Outcome};
use gpui_kit::component::{
    ActiveTheme as _,
    input::{InputEvent, TextareaState},
    scroll::Scrollbar,
};
use std::sync::Arc;

/// Heights of the chat list's items in rems. Rows and section labels have fixed
/// heights so the list can place its stacked section labels by arithmetic.
pub(super) const ROW_HEIGHT: f32 = 3.25;
pub(super) const SECTION_HEIGHT: f32 = 2.;
const END_HEIGHT: f32 = 2.25;
/// Below these list widths, in rems, the scope tabs use short labels and the
/// section labels drop their new message counts.
const COMPACT_TABS_WIDTH: f32 = 17.;
const COMPACT_SECTIONS_WIDTH: f32 = 14.;

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

    /// Which labels sit in the top and bottom stacks at a scroll offset. A label
    /// sticks once its natural place reaches its slot, like CSS sticky positioning
    /// with a top of slot times the label height.
    fn stacks(&self, scroll: Pixels) -> Stacks {
        let n = self.sections.len();
        let mut top = 0;
        let mut bottom = n;
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
            if bottom == n
                && k >= top
                && self.viewport > px(0.)
                && natural > self.viewport - self.section * (n - k) as f32
            {
                bottom = k;
            }
        }
        Stacks {
            top: 0..top,
            bottom: bottom.max(top)..n,
            current,
        }
    }

    /// The scroll offset that puts a section's first chat right under the top stack.
    fn jump_target(&self, section: usize) -> Option<Pixels> {
        let ix = *self.sections.get(section)?;
        Some(self.tops[ix] - self.section * section as f32)
    }

    /// The scroll offset that brings an item clear of both stacks, if it is covered.
    fn reveal_target(&self, item: usize, scroll: Pixels) -> Option<Pixels> {
        let n = self.sections.len();
        let section = self.section_of(item);
        let covered_top = self.section * (section + 1) as f32;
        let covered_bottom = self.section * (n - 1 - section) as f32;
        let (row_top, row_bottom) = (self.tops[item], self.tops[item + 1]);
        if row_top - scroll < covered_top {
            Some(row_top - covered_top)
        } else if row_bottom - scroll > self.viewport - covered_bottom {
            Some(row_bottom - self.viewport + covered_bottom)
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
    /// Sections stacked at the bottom edge, up to the last.
    pub bottom: std::ops::Range<usize>,
    /// The section being read: the last one that reached its place in the top stack.
    pub current: usize,
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
        }
    }

    fn record(thread: &Thread) -> prepared::SearchRecord {
        prepared::SearchRecord {
            id: Arc::from(thread.id.as_str()),
            text: thread.search_text.clone(),
            completed: !thread.matches_status(0, false),
            blocked: thread.status == "blocked",
            working: thread.status == "working",
            unread: thread.unread(),
            agent: Arc::from(thread.provider.as_str()),
            activity: thread.last_activity(),
        }
    }

    pub fn sync(&mut self, app: &Adeline, criteria: Criteria, cx: &mut Context<Self>) {
        let records: prepared::SearchCatalog =
            app.workspace().threads.iter().map(Self::record).collect();
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
            let record = Self::record(thread);
            let unchanged = old.text.same_snapshot(&record.text)
                && old.completed == record.completed
                && old.blocked == record.blocked
                && old.working == record.working
                && old.unread == record.unread
                && old.activity == record.activity;
            if !unchanged {
                self.search_bytes = self.search_bytes - old.text.len() + record.text.len();
                self.records.replace(index, record);
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
        for (slot, (_, rows)) in outcome.groups.iter().enumerate() {
            items.push(Item::Section(slot));
            items.extend(rows.iter().map(|&row| Item::Chat(row)));
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

    /// Scroll so the first chat of a section sits right under the top stack.
    pub fn jump(&mut self, section: usize, cx: &mut Context<Self>) {
        if let Some(target) = self.layout.jump_target(section) {
            self.state.scroll_to(self.layout.offset_at(target));
            cx.notify();
        }
    }

    /// Keep a chat clear of both stacks after keyboard selection.
    fn reveal(&self, item: usize) {
        if let Some(target) = self.layout.reveal_target(item, self.scroll()) {
            self.state.scroll_to(self.layout.offset_at(target));
        }
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
        ui_metrics::record(ui_metrics::Region::Sidebar);
        self.measure(window);
        let stacks = self.layout.stacks(self.scroll());
        let this = cx.weak_entity();
        let measured = this.clone();
        let rem = window.rem_size();
        // Compact tabs share the bar evenly: each is capped at a third of the
        // room inside the list's side padding and the segmented bar's inset and
        // gaps, so the labels give up tab padding instead of clipping the last tab.
        let tab_cap = (self.width > px(0.) && self.width < rem * COMPACT_TABS_WIDTH)
            .then(|| (self.width - rem * 1.5 - px(12.)) / 3.);
        let compact_sections = self.width > px(0.) && self.width < rem * COMPACT_SECTIONS_WIDTH;
        if let Some(query) = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).query.clone())
        {
            let placeholder = if tab_cap.is_some() {
                "Search"
            } else {
                "Search chats"
            };
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
            // A narrow list lines the message up with the tabs and shortens the button.
            let compact = tab_cap.is_some();
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
            let tops: Arc<[Pixels]> = self.layout.tops.clone().into();
            let groups: Arc<[(Group, Vec<usize>)]> = self.outcome.groups.clone().into();
            let selected = self.selected;
            let tail = self.tail;
            let total = self.outcome.matches();
            let current = stacks.current;
            let section_view = {
                let records = self.records.clone();
                let this = this.clone();
                move |slot: usize, placement: LabelPlacement, cx: &mut App| {
                    let (group, rows) = &groups[slot];
                    let fresh = rows.iter().filter(|&&row| records[row].unread).count();
                    section_label(
                        *group,
                        rows.len(),
                        fresh,
                        slot == current,
                        compact_sections,
                        placement,
                        {
                            let this = this.clone();
                            move |_, _, cx: &mut App| {
                                let _ = this.update(cx, |list, cx| list.jump(slot, cx));
                            }
                        },
                        cx,
                    )
                }
            };
            let list_view = {
                let section_view = section_view.clone();
                list(self.state.clone(), move |ix, _, cx| {
                    let rail = Rail {
                        top: tops[ix],
                        starts: ix == 0,
                        ends: matches!(items[ix], Item::End),
                    };
                    match items[ix] {
                        Item::Section(slot) => section_view(slot, LabelPlacement::Inline(rail), cx),
                        Item::Chat(row) => {
                            ui_metrics::record(ui_metrics::Region::ChatRow);
                            let next = items.get(ix + 1).copied();
                            let context = RowContext {
                                rail,
                                separator: matches!(next, Some(Item::Chat(next)) if selected != Some(next)),
                            };
                            owner
                                .update(cx, |app, cx| app.chat_card(row, context, cx))
                                .unwrap_or_else(|_| div().into_any_element())
                        }
                        Item::End => end_label(total, rail, cx),
                        Item::Tail => div().h(tail).into_any_element(),
                    }
                })
                .size_full()
            };
            let top_stack =
                col()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .children(stacks.top.clone().map(|slot| {
                        section_view(
                            slot,
                            LabelPlacement::Top {
                                last: slot + 1 == stacks.top.end,
                            },
                            cx,
                        )
                    }));
            let bottom_stack =
                col()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .children(stacks.bottom.clone().map(|slot| {
                        section_view(
                            slot,
                            LabelPlacement::Bottom {
                                first: slot == stacks.bottom.start,
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
                .child(bottom_stack)
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
                app.chat_sidebar(rows, &self.outcome, focused, tab_cap, cx)
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element());
        div().relative().size_full().child(sidebar).child(
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
}

impl Transcript {
    pub fn new(owner: WeakEntity<Adeline>, cx: &mut Context<Self>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(250.));
        Self {
            owner,
            state,
            typography: config::typography(),
            thread: None,
            selected: None,
            messages: 0,
            footer_focus: cx.focus_handle(),
        }
    }

    pub fn sync(&mut self, app: &Adeline, scroll_to_end: bool, cx: &mut Context<Self>) {
        let thread = app.selected.map(|i| &app.workspace().threads[i]);
        let key = thread.map(|t| (app.project, t.id.clone()));
        let messages = thread.map_or(0, |t| t.messages.len());
        let footer = thread.is_some_and(|t| {
            !app.demo_mode
                || t.status == "blocked"
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
        if scroll_to_end {
            self.state.scroll_to(ListOffset {
                item_ix: count,
                offset_in_item: px(0.),
            });
        } else if !changed_thread {
            self.state.scroll_to(previous_top);
        }
        self.thread = key;
        self.selected = app.selected;
        self.messages = messages;
        cx.notify();
    }
}

impl Render for Transcript {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = config::typography();
        if self.typography != typography {
            self.typography = typography;
            let top = self.state.logical_scroll_top();
            let count = self.state.item_count();
            self.state.splice(0..count, count);
            self.state.scroll_to(top);
        }
        ui_metrics::record(ui_metrics::Region::Transcript);
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
            ui_metrics::record(ui_metrics::Region::MessageRow);
            owner
                .update(cx, |app, cx| {
                    if row < messages {
                        app.message_row(selected, row, cx)
                    } else {
                        col()
                            .w_full()
                            .track_focus(&footer_focus)
                            .px_5()
                            .child(if app.demo_mode {
                                app.decision_row(selected, cx)
                            } else {
                                app.runtime_footer(selected, cx)
                            })
                            .into_any_element()
                    }
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full()
        .py_2();
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
            _content_subscription: subscription,
        }
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Composer);
        self.owner
            .update(cx, |app, cx| app.composer_view(cx).into_any_element())
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(super) struct Header(pub WeakEntity<Adeline>);
impl Render for Header {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Header);
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
                | HideToolCalls
                | LeftPanel(_)
                | RightPanel(_)
                | ToggleLeftPanel
                | ToggleSidePanel
        ) {
            self.control_pane.update(cx, |_, cx| cx.notify());
        }
        // Data mutations notify their dependents explicitly. Opening overlays,
        // changing focus and changing unrelated sections do not invalidate them.
        match action {
            Project(_) => self.sync_sidebar(cx),
            Section(crate::Section::Chats)
            | NewChat
            | Filter(_)
            | AgentFilter(_)
            | ClearChatFilters
            | ShowCompleted => {
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
    fn labels_stack_under_the_controls_and_wait_at_the_bottom() {
        let layout = layout();
        // At the top, the first label is in place and the others wait at the bottom.
        assert_eq!(
            at(&layout, 0.),
            Stacks {
                top: 0..0,
                bottom: 1..3,
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
                bottom: 3..3,
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
    fn keyboard_reveal_keeps_rows_clear_of_both_stacks() {
        let layout = layout();
        let row_under_top_stack = layout.sections[1] + 1;
        let scroll = layout.tops[row_under_top_stack] - px(40.);
        let target = layout.reveal_target(row_under_top_stack, scroll).unwrap();
        assert_eq!(layout.tops[row_under_top_stack] - target, px(64.));
        let row_under_bottom_stack = layout.sections[0] + 7;
        let target = layout
            .reveal_target(row_under_bottom_stack, px(0.))
            .unwrap();
        let bottom_edge: Pixels = layout.tops[row_under_bottom_stack + 1] - target;
        assert_eq!(bottom_edge, px(400.) - px(32.) * 2.);
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
