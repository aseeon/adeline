//! The chat list: the project switcher, search, sections with stacked labels,
//! rows and the sidebar around them. Each row is a cached `ChatCard` view, and
//! the list notifies every card whenever it is notified itself.
use super::*;

impl Adeline {
    pub(crate) fn chat_criteria(&self, cx: &App) -> Criteria {
        Criteria {
            query: self.query(cx),
            completed: self.show_completed,
            archived: self.show_archived,
            agent: self.agent_filter.clone(),
        }
    }

    pub(crate) fn search_sidebar(&self, cx: &mut Context<Self>) {
        let criteria = self.chat_criteria(cx);
        self.chat_list.update(cx, |list, cx| {
            // Programmatic input resets can emit the same InputEvent::Change.
            if list.criteria != criteria {
                list.search(criteria, true, cx);
            }
        });
    }

    pub(crate) fn sync_sidebar(&self, cx: &mut Context<Self>) {
        let criteria = self.chat_criteria(cx);
        self.chat_list
            .update(cx, |list, cx| list.sync(self, criteria, cx));
    }

    fn chat_title(&self, i: usize, selected: bool, cx: &Context<Self>) -> Div {
        let thread = &self.workspace().threads[i];
        let isl = theme::islands(cx);
        let done = thread.status.closed();
        let live = thread.status.live();
        // Unread, waiting and running chats read bright; the rest recede.
        let bright = selected || thread.unread() || live;
        row()
            .h(dp(ROW_HEIGHT))
            .px(dp(10.))
            .gap(dp(10.))
            .rounded(dp(8.))
            .text_size(dp(theme::T_BODY))
            .child(agent_mark(self.agent_icon(&thread.provider, cx)).size(dp(16.)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if bright { isl.fg } else { isl.fg2 })
                    .when(thread.unread(), |title| {
                        title.font_weight(FontWeight::MEDIUM)
                    })
                    .when(done, |title| {
                        title.line_through().text_decoration_color(isl.fg3)
                    })
                    .child(highlighted_title(thread.title.trim(), &self.query(cx), cx)),
            )
    }

    fn chat_row_end(&self, i: usize, group: &SharedString, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[i];
        let status = row_status(thread, updated(thread), cx);
        if thread.status != ThreadStatus::Completed {
            return status;
        }
        div()
            .relative()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .min_w(rems(1.5))
            .child(
                div()
                    .group_hover(group.clone(), |style| style.invisible())
                    .child(status),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(rems(-0.25))
                    .flex()
                    .items_center()
                    .invisible()
                    .group_hover(group.clone(), |style| style.visible())
                    .child(
                        self.icon_button(
                            SharedString::from(format!("archive-chat:{}", thread.id)),
                            "Archive",
                            Icon::default().path("archive.svg"),
                            Action::ArchiveChat(i),
                            cx,
                        )
                        .xsmall(),
                    ),
            )
            .into_any_element()
    }

    fn chat_description(&self, i: usize) -> String {
        let thread = &self.workspace().threads[i];
        let mut description = format!(
            "{}, {}, {} messages",
            thread.title.trim(),
            self.agent_name(&thread.provider),
            thread.messages.len()
        );
        if let Some(updated) = updated(thread) {
            write!(description, ", updated {updated}").unwrap();
        }
        write!(description, ", {}", status_name(thread)).unwrap();
        if thread.unread() {
            description.push_str(", unread");
        }
        description
    }

    pub(crate) fn chat_card(&self, i: usize, cx: &Context<Self>) -> AnyElement {
        let isl = theme::islands(cx);
        let selected = self.selected == Some(i);
        let thread = &self.workspace().threads[i];
        let id = SharedString::from(format!("chat:{}:{}", self.workspace().config.id, thread.id));
        let done = thread.status.closed();
        let archived = thread.status == ThreadStatus::Archived;
        let owner = cx.weak_entity();
        div()
            .w_full()
            .px(dp(8.))
            .child(
                self.chat_title(i, selected, cx)
                    .child(self.chat_row_end(i, &id, cx))
                    .id(id.clone())
                    .test_support()
                    .group(id)
                    .role(Role::ListBoxOption)
                    .aria_selected(selected)
                    .aria_label(self.chat_description(i))
                    .w_full()
                    .when(selected, |row| row.bg(isl.sel))
                    .when(!selected, |row| row.hover(|style| style.bg(isl.hover)))
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.act(Action::Chat(i), window, cx)),
                    )
                    .context_menu(move |menu, _, _| {
                        let (complete, archive) = (owner.clone(), owner.clone());
                        menu.item(
                            PopupMenuItem::new(if done {
                                "Mark as incomplete"
                            } else {
                                "Mark as complete"
                            })
                            .on_click(move |_, window, cx| {
                                let _ = complete.update(cx, |app, cx| {
                                    app.act(Action::Chat(i), window, cx);
                                    app.act(Action::Complete, window, cx);
                                });
                            }),
                        )
                        .item(
                            PopupMenuItem::new("Archive").disabled(archived).on_click(
                                move |_, window, cx| {
                                    let _ = archive.update(cx, |app, cx| {
                                        app.act(Action::ArchiveChat(i), window, cx);
                                    });
                                },
                            ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn agent_filter_button(&self, outcome: &Outcome, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let button = |id: &'static str| {
            Button::new(id)
                .ghost()
                .flex_shrink_0()
                .size(rems(1.75))
                .p_0()
        };
        if let Some(agent) = &self.agent_filter {
            let (path, color) = self.agent_icon(agent, cx);
            let name = self.agent_name(agent);
            return button("chat-agent-filter-clear")
                .bg(theme.sidebar_primary)
                .border_1()
                .border_color(theme.input)
                .accessibility_label(format!("Showing {name} only. Show all agents"))
                .tooltip(format!("Showing {name} only. Click to show all agents"))
                .child(
                    themed_icon::ThemedIcon::path(&path)
                        .size(rems(1.))
                        .text_color(color),
                )
                .on_click(cx.listener(|app, _, window, cx| {
                    app.act(Action::AgentFilter(None), window, cx);
                }))
                .into_any_element();
        }
        let owner = cx.weak_entity();
        let agents: Arc<[(Arc<str>, String, String, usize)]> = outcome
            .agents
            .iter()
            .map(|(agent, count)| {
                let (path, _) = self.agent_icon(agent, cx);
                (Arc::clone(agent), self.agent_name(agent), path, *count)
            })
            .collect();
        let total: usize = agents.iter().map(|(_, _, _, count)| count).sum();
        button("chat-agent-filter")
            .accessibility_label("Filter by agent")
            .tooltip("Filter by agent")
            .child(
                icon("robot")
                    .size(rems(1.1875))
                    .text_color(theme.muted_foreground),
            )
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let counted = |label: String, count: usize| {
                    move |_: &mut Window, cx: &mut App| {
                        row()
                            .w_full()
                            .gap_3()
                            .child(div().flex_1().child(label.clone()))
                            .child(
                                div()
                                    .text_size(dp(theme::T_CAPTION))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(count.to_string()),
                            )
                    }
                };
                let mut menu = menu.check_side(Side::Right).label("Agent").item(
                    PopupMenuItem::element(counted("All agents".into(), total)).checked(true),
                );
                for (agent, name, path, count) in agents.iter() {
                    let owner = owner.clone();
                    let agent = Arc::clone(agent);
                    menu = menu.item(
                        PopupMenuItem::element(counted(name.clone(), *count))
                            .icon(Icon::default().path(path.clone()))
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |app, cx| {
                                    app.act(
                                        Action::AgentFilter(Some(Arc::clone(&agent))),
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    pub(crate) fn chat_sidebar(
        &self,
        list: AnyElement,
        outcome: &Outcome,
        search_focused: bool,
        cx: &Context<Self>,
    ) -> Div {
        let isl = theme::islands(cx);
        let searching = self.chat_search_open
            || search_focused
            || !self.query.read(cx).value().is_empty()
            || self.agent_filter.is_some();
        // The frame belongs to the list rather than the input, so the agent button
        // shares the frame's inset instead of the input's fixed text padding.
        // Focus shows as the stronger ring, as on the composer.
        let search = row()
            .id("chat-search-field")
            .w_full()
            .h(dp(32.))
            .pl(dp(10.))
            // Matches the 1 px ring so the agent button is inset evenly on every side.
            .pr(px(1.))
            .rounded(dp(8.))
            .border_1()
            .border_color(if search_focused {
                isl.ring_strong
            } else {
                isl.ring
            })
            .bg(isl.chip)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|app, _, window, cx| {
                    window.focus(&app.query.focus_handle(cx), cx);
                }),
            )
            .child(icon("search").size(dp(14.)).text_color(isl.fg3))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.query)
                        .aria_label("Search chats")
                        .cleanable(true)
                        .appearance(false),
                ),
            )
            .child(self.agent_filter_button(outcome, cx));
        let header = row()
            .h(dp(44.))
            .flex_shrink_0()
            .pl(dp(18.))
            .pr(dp(10.))
            .gap(dp(6.))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(dp(theme::T_BODY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(isl.fg)
                    .child("Chats"),
            )
            .child(
                self.command_popover(
                    "mode-settings",
                    panes::icon_button(
                        "chat-settings",
                        Icon::default()
                            .path("more.svg")
                            .rotate(Radians(std::f32::consts::FRAC_PI_2)),
                        cx,
                    )
                    .accessibility_label("Chat settings")
                    .tooltip("Chat settings"),
                    Anchor::TopLeft,
                    cx,
                ),
            )
            .child(div().flex_1())
            .when(!machines::remotes().is_empty(), |header| {
                header.child(self.machines_button(cx))
            })
            .child(
                panes::icon_button("chat-search-toggle", Icon::default().path("search.svg"), cx)
                    .selected(searching)
                    .accessibility_label("Search chats")
                    .tooltip(format!(
                        "Search chats ({})",
                        conversation_ui::shortcut("Ctrl+F")
                    ))
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.toggle_chat_search(window, cx);
                    })),
            )
            .child(
                panes::icon_button("new-chat", Icon::default().path("new-chat.svg"), cx)
                    .accessibility_label("New chat")
                    .tooltip(format!(
                        "New chat ({})",
                        conversation_ui::shortcut("Ctrl+N")
                    ))
                    .on_click(cx.listener(|app, _, window, cx| {
                        cx.stop_propagation();
                        app.act(Action::NewChat, window, cx);
                    })),
            );
        col()
            .w_full()
            .min_w_0()
            .h_full()
            .child(self.project_switcher(cx))
            .child(header)
            .when(searching, |panel| {
                panel.child(div().w_full().px(dp(8.)).pb(dp(8.)).child(search))
            })
            .child(list)
    }
}

pub(crate) const ROW_HEIGHT: f32 = 36.;

pub(crate) const SECTION_HEIGHT: f32 = 36.;

pub(crate) const LABEL_GAP: f32 = 10.;

const END_HEIGHT: f32 = 12.;

const COMPACT_WIDTH: f32 = 17.;

// Smaller searches run inline, where they finish well within a frame; larger
// ones run in the background so typing never stalls.
const INLINE_SEARCH_ROWS: usize = 256;

const INLINE_SEARCH_BYTES: usize = 64 * 1024;

// Background searches wait this long for the next keystroke.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(30);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Item {
    Section(usize),
    Chat(usize),
    End,
    Tail,
}

#[derive(Default)]
struct Layout {
    // The top of every item, then the total height.
    tops: Vec<Pixels>,
    sections: Vec<usize>,
    section: Pixels,
    viewport: Pixels,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Stacks {
    pub top: std::ops::Range<usize>,
    pub current: usize,
}

pub(crate) struct ChatList {
    owner: WeakEntity<Adeline>,
    state: ListState,
    typography: fonts::Typography,
    records: Vec<prepared::SearchRecord>,
    generation: prepared::Generation,
    pending: Option<Task<()>>,
    searching: bool,
    search_bytes: usize,
    project: usize,
    pub(super) selected: Option<usize>,
    criteria: Criteria,
    outcome: Outcome,
    items: Arc<[Item]>,
    keys: Vec<Arc<str>>,
    tail: Pixels,
    layout: Layout,
    width: Pixels,
    placeholder: &'static str,
    folded: Vec<Group>,
    cards: std::rc::Rc<std::cell::RefCell<Vec<Entity<ChatCard>>>>,
    _redraw_cards: Subscription,
}

pub(crate) struct ChatCard {
    owner: WeakEntity<Adeline>,
    row: usize,
}

impl Render for ChatCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = self.row;
        self.owner
            .update(cx, |app, cx| app.chat_card(row, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

const RUNNING_TURN_MS: u64 = 2400;

#[derive(Clone, Copy)]
pub(crate) enum LabelPlacement {
    Inline,
    Top { last: bool },
}

pub(crate) fn section_label(
    group: Group,
    chats: usize,
    folded: bool,
    current: bool,
    placement: LabelPlacement,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let id = match placement {
        LabelPlacement::Inline => "inline",
        LabelPlacement::Top { .. } => "top",
    };
    // Inline labels fold their section; pinned ones scroll to it.
    let verb = match (placement, folded) {
        (LabelPlacement::Inline, true) => "Show",
        (LabelPlacement::Inline, false) => "Hide",
        _ => "Go to",
    };
    let isl = theme::islands(cx);
    let color = if group == Group::NeedsInput {
        isl.accent
    } else if current {
        isl.fg2
    } else {
        isl.fg3
    };
    div()
        .w_full()
        .flex_shrink_0()
        .px(dp(8.))
        .pt(dp(LABEL_GAP))
        .bg(isl.island)
        .child(
            row()
                .id(SharedString::from(format!(
                    "chat-section:{id}:{}",
                    group.title()
                )))
                .role(Role::Button)
                .aria_label(format!(
                    "{verb} {}, {}",
                    group.title(),
                    count(chats, "chat")
                ))
                .h(dp(SECTION_HEIGHT - LABEL_GAP))
                .px(dp(10.))
                .gap(dp(6.))
                .rounded(dp(8.))
                .text_size(dp(theme::T_MICRO))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .hover(|style| style.text_color(isl.fg))
                .on_click(on_click)
                .when(folded, |label| {
                    label.child(Icon::default().path("caret-right.svg").size(dp(10.)))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(spaced(&group.title().to_uppercase())),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(FontWeight::NORMAL)
                        .child(chats.to_string()),
                ),
        )
        // The pinned stack's edge sits inside the label's side padding. The stack
        // stops short of the scrollbar on the right, so the line gives up the
        // same width on the left and sits evenly in the panel.
        .when(
            matches!(placement, LabelPlacement::Top { last: true }),
            |label| label.child(div().h(px(1.)).ml(theme::SCROLLBAR_TRACK).bg(isl.line)),
        )
        .into_any_element()
}

// GPUI has no letter spacing, so uppercase labels get hair spaces between
// letters for the design's 0.08em tracking.
fn spaced(label: &str) -> String {
    let mut spaced = String::with_capacity(label.len() * 4);
    for (ix, letter) in label.chars().enumerate() {
        if ix > 0 {
            spaced.push('\u{200A}');
        }
        spaced.push(letter);
    }
    spaced
}

fn highlighted_title(title: &str, query: &str, cx: &App) -> StyledText {
    let text = StyledText::new(title.to_owned());
    if query.is_empty() {
        return text;
    }
    let lower = title.to_lowercase();
    // Lowercasing can change byte lengths outside ASCII; skip the mark then.
    match lower.find(query) {
        Some(start) if lower.len() == title.len() => text.with_highlights([(
            start..start + query.len(),
            HighlightStyle {
                background_color: Some(cx.theme().primary.alpha(0.28)),
                color: Some(cx.theme().sidebar_primary_foreground),
                ..Default::default()
            },
        )]),
        _ => text,
    }
}

fn running_ring(turn: f32, track: Hsla, arc: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let outer = f32::from(bounds.size.width.min(bounds.size.height)) / 2.;
            let inner = outer - f32::from(window.rem_size() * 0.125);
            let shape = ArcShape::new().inner_radius(inner).outer_radius(outer);
            let start = turn * TAU;
            for (index, (from, to, color)) in [(0., TAU, track), (start, start + TAU / 4., arc)]
                .into_iter()
                .enumerate()
            {
                let segment = ArcData::new(&(), index, 1., from, to);
                shape.paint(&segment, color, &bounds, window);
            }
        },
    )
    .size_full()
}

fn updated(thread: &Thread) -> Option<String> {
    let activity = thread.last_activity();
    (activity > 0).then(|| recency::label(activity, recency::now()))
}

fn row_status(thread: &Thread, updated: Option<String>, cx: &App) -> AnyElement {
    let isl = theme::islands(cx);
    match thread.status {
        ThreadStatus::Blocked => div()
            .flex_shrink_0()
            .size(dp(7.))
            .rounded_full()
            .bg(isl.accent)
            .into_any_element(),
        ThreadStatus::Processing => {
            let (track, arc) = (isl.fg3.alpha(0.35), isl.fg2);
            div()
                .flex_shrink_0()
                .size(dp(12.))
                .with_animation(
                    "chat-row-running",
                    Animation::new(Duration::from_millis(RUNNING_TURN_MS))
                        .repeat_synced()
                        .with_max_fps(ANIMATION_FPS),
                    move |ring, turn| ring.child(running_ring(turn, track, arc)),
                )
                .into_any_element()
        }
        _ => div()
            .flex_shrink_0()
            .text_size(dp(theme::T_CAPTION))
            .text_color(isl.fg3)
            .children(updated)
            .into_any_element(),
    }
}

pub(crate) fn status_name(thread: &Thread) -> &'static str {
    match thread.status {
        ThreadStatus::Processing => "running",
        ThreadStatus::Completed => "completed",
        ThreadStatus::Blocked => "needs input",
        ThreadStatus::Archived => "archived",
        ThreadStatus::Idle => "idle",
    }
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

    fn section_of(&self, item: usize) -> usize {
        self.sections
            .partition_point(|&start| start <= item)
            .saturating_sub(1)
    }

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

    fn jump_target(&self, section: usize) -> Option<Pixels> {
        let ix = *self.sections.get(section)?;
        Some(self.tops[ix] - self.section * section as f32)
    }

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

impl ChatList {
    pub fn new(owner: WeakEntity<Adeline>, cx: &mut Context<Self>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(180.));
        Self {
            cards: std::rc::Rc::default(),
            _redraw_cards: cx.observe_self(|list, cx| {
                for card in list.cards.borrow().iter() {
                    card.update(cx, |_, cx| cx.notify());
                }
            }),
            owner,
            state,
            typography: fonts::typography(),
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
            // Finished chats stay out of the way until asked for.
            folded: vec![Group::Completed],
        }
    }

    pub fn sync(&mut self, app: &Adeline, criteria: Criteria, cx: &mut Context<Self>) {
        let records: Vec<prepared::SearchRecord> = app
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
        let mut cards = self.cards.borrow_mut();
        cards.truncate(records.len());
        while cards.len() < records.len() {
            let card = ChatCard {
                owner: self.owner.clone(),
                row: cards.len(),
            };
            cards.push(cx.new(|_| card));
        }
        drop(cards);
        self.search_bytes = records.iter().map(|r| r.text.len()).sum();
        self.records = records;
        self.selected = app.selected;
        self.search(criteria, false, cx);
    }

    pub(super) fn update_thread(&mut self, app: &Adeline, cx: &mut Context<Self>) {
        if self.project != app.project || self.records.len() != app.workspace().threads.len() {
            self.sync(app, self.criteria.clone(), cx);
            return;
        }
        self.selected = app.selected;
        if let Some(index) = app.selected
            && self.update_record(app, index, cx)
        {
            return;
        }
        // Selection draws the row fill and hides the separator above it.
        invalidate_range(&self.state, 0..self.state.item_count());
        cx.notify();
    }

    pub(crate) fn update_streamed(&mut self, app: &Adeline, id: &str, cx: &mut Context<Self>) {
        if self.project != app.project || self.records.len() != app.workspace().threads.len() {
            self.sync(app, self.criteria.clone(), cx);
        } else if let Some(index) = app.workspace().threads.iter().position(|t| t.id == id) {
            self.update_record(app, index, cx);
        }
    }

    fn update_record(&mut self, app: &Adeline, index: usize, cx: &mut Context<Self>) -> bool {
        let thread = &app.workspace().threads[index];
        if self.records[index].id.as_ref() != thread.id {
            self.sync(app, self.criteria.clone(), cx);
            return true;
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
        if unchanged {
            return false;
        }
        self.search_bytes = self.search_bytes - old.text.len() + record.text.len();
        self.records[index] = record;
        // Status and activity move chats between sections, so the
        // sections are rebuilt. Explicit actions skip the typing delay.
        self.search(self.criteria.clone(), false, cx);
        true
    }

    pub(crate) fn first_result(&self) -> Option<usize> {
        self.outcome
            .groups
            .iter()
            .flat_map(|(_, rows)| rows)
            .next()
            .copied()
    }

    pub fn search(&mut self, criteria: Criteria, debounce: bool, cx: &mut Context<Self>) {
        self.pending.take();
        self.criteria = criteria.clone();
        let ticket = self.generation.issue();
        let records = self.records.clone();
        let now = recency::now();
        if criteria.query.is_empty()
            || (records.len() < INLINE_SEARCH_ROWS && self.search_bytes < INLINE_SEARCH_BYTES)
        {
            self.publish(prepared::search(records.iter(), &criteria, now), cx);
        } else {
            self.searching = true;
            let executor = cx.background_executor().clone();
            self.pending = Some(cx.spawn(async move |this, cx| {
                if debounce {
                    executor.timer(SEARCH_DEBOUNCE).await;
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
                Item::Chat(row) => Arc::clone(&self.records[*row].id),
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

    fn measure(&mut self) {
        let (row, section, end) = (dp(ROW_HEIGHT), dp(SECTION_HEIGHT), dp(END_HEIGHT));
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

    fn toggle_fold(&mut self, group: Group, cx: &mut Context<Self>) {
        if let Some(ix) = self.folded.iter().position(|&folded| folded == group) {
            self.folded.remove(ix);
        } else {
            self.folded.push(group);
        }
        self.publish(self.outcome.clone(), cx);
    }

    pub fn jump(&mut self, section: usize, cx: &mut Context<Self>) {
        if let Some(target) = self.layout.jump_target(section) {
            self.state.scroll_to(self.layout.offset_at(target));
            cx.notify();
        }
    }

    fn reveal(&self, item: usize) {
        if let Some(target) = self.layout.reveal_target(item, self.scroll()) {
            self.state.scroll_to(self.layout.offset_at(target));
        }
    }

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
        let typography = fonts::typography();
        if self.typography != typography {
            self.typography = typography;
            let top = self.state.logical_scroll_top();
            let count = self.state.item_count();
            self.state.splice(0..count, count);
            self.state.scroll_to(top);
        }
        self.measure();
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
        let filtered = !self.criteria.query.is_empty() || self.criteria.agent.is_some();
        let rows = if self.items.is_empty() {
            let owner = self.owner.clone();
            // A narrow list tightens the message and shortens the button.
            col()
                .flex_1()
                .when(compact, |empty| empty.px_3().py_5())
                .when(!compact, |empty| empty.p_5())
                .gap_3()
                .items_start()
                .text_size(dp(theme::T_UI))
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
            let items = Arc::clone(&self.items);
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
                let cards = std::rc::Rc::clone(&self.cards);
                list(self.state.clone(), move |ix, window, cx| match items[ix] {
                    Item::Section(slot) => section_view(slot, LabelPlacement::Inline, cx),
                    Item::Chat(row) => match cards.borrow().get(row) {
                        Some(card) => region(
                            card,
                            StyleRefinement::default().w_full().h(dp(ROW_HEIGHT)),
                            window,
                        ),
                        None => owner
                            .update(cx, |app, cx| app.chat_card(row, cx))
                            .unwrap_or_else(|_| div().into_any_element()),
                    },
                    Item::End => div().h(dp(END_HEIGHT)).into_any_element(),
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
            .child(size_probe(move |size, cx| {
                let _ = measured.update(cx, |list, cx| {
                    if (list.width - size.width).abs() > px(0.5) {
                        list.width = size.width;
                        cx.notify();
                    }
                });
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Layout, Stacks, invalidate_range};
    use gpui_kit::{ListAlignment, ListOffset, ListState, Pixels, px};

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
