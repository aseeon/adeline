//! The chat list: search, sections with stacked labels, rows, the collapsed
//! rail and the sidebar around it.
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

    fn chat_line(&self, i: usize, selected: bool, cx: &Context<Self>) -> Div {
        let thread = &self.workspace().threads[i];
        self.chat_title(i, selected, cx)
            .child(row_status(thread, updated(thread), cx))
    }

    fn chat_title(&self, i: usize, selected: bool, cx: &Context<Self>) -> Div {
        let thread = &self.workspace().threads[i];
        let theme = cx.theme();
        let done = thread.status.closed();
        let live = thread.status.live();
        // Unread, waiting and running chats read bright; the rest recede.
        let title_color = if selected || thread.unread() || live {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        row()
            .h(rems(ROW_HEIGHT))
            .px_2()
            .gap_2p5()
            .rounded(theme.radius)
            .text_sm()
            .child(agent_mark(self.agent_icon(&thread.provider, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(title_color)
                    .when(done, |title| {
                        title.line_through().text_decoration_color(theme.border)
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
        let theme = cx.theme();
        let selected = self.selected == Some(i);
        let id = SharedString::from(format!(
            "chat:{}:{}",
            self.workspace().config.id,
            self.workspace().threads[i].id
        ));
        div()
            .w_full()
            .px_1p5()
            .child(
                self.chat_title(i, selected, cx)
                    .child(self.chat_row_end(i, &id, cx))
                    .id(id.clone())
                    .group(id)
                    .role(Role::ListBoxOption)
                    .aria_selected(selected)
                    .aria_label(self.chat_description(i))
                    .w_full()
                    .when(selected, |row| row.bg(theme.sidebar_primary))
                    .when(!selected, |row| {
                        row.hover(|style| style.bg(theme.secondary))
                    })
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.act(Action::Chat(i), window, cx)),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn collapsed_search(
        &self,
        open: bool,
        matches: Option<usize>,
        list: WeakEntity<ChatList>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let key: Arc<str> = Arc::from(COLLAPSED_SEARCH);
        let trigger = collapsed_button("chat-rail-search", "Search chats", "search")
            .selected(matches.is_some())
            .on_click(cx.listener(|app, _, window, cx| app.focus_chat_search(window, cx)));
        let flyout = row()
            .id("chat-rail-search-flyout")
            .absolute()
            .top(rems(-0.25))
            .left(rems(-0.25))
            .w(rems(20.))
            .h_8()
            .pl_2()
            .pr_3()
            .gap_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.primary)
            .bg(theme.background)
            .on_hover({
                let key = Arc::clone(&key);
                let list = list.clone();
                move |hovered, _, cx| {
                    if !*hovered {
                        let _ = list.update(cx, |list, cx| list.hover_collapsed(&key, false, cx));
                    }
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|app, _, window, cx| app.focus_chat_search(window, cx)),
            )
            .child(icon("search").size(rems(1.)).text_color(theme.primary))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.query)
                        .aria_label("Search chats")
                        .cleanable(true)
                        .appearance(false),
                ),
            )
            .when_some(matches, |flyout, matches| {
                flyout.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(matches.to_string()),
                )
            });
        div()
            .id("chat-rail-search-slot")
            .relative()
            .flex_shrink_0()
            .on_hover(move |hovered, _, cx| {
                if *hovered {
                    let _ = list.update(cx, |list, cx| list.hover_collapsed(&key, true, cx));
                }
            })
            .child(trigger)
            .when(open, |slot| slot.child(deferred(flyout).with_priority(1)))
            .into_any_element()
    }

    pub(crate) fn collapsed_chat(
        &self,
        i: usize,
        hovered: bool,
        focus: &FocusHandle,
        keyboard_focus: bool,
        list: WeakEntity<ChatList>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let thread = &self.workspace().threads[i];
        let theme = cx.theme();
        let selected = self.selected == Some(i);
        let key: Arc<str> = Arc::from(thread.id.as_str());
        let id = format!("{}:{}", self.workspace().config.id, thread.id);
        let fill = if selected {
            theme.sidebar_primary
        } else {
            theme.secondary
        };
        let open = move |app: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            app.act(Action::Chat(i), window, cx);
        };
        let tile = row()
            .id(SharedString::from(format!("chat-rail:{id}")))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .aria_label(self.chat_description(i))
            .track_focus(focus)
            .flex_shrink_0()
            .size(rems(ROW_HEIGHT))
            .px_2()
            .rounded(theme.radius)
            .when(selected, |tile| tile.bg(fill))
            .when(!selected, |tile| tile.hover(|style| style.bg(fill)))
            .on_hover({
                let list = list.clone();
                let key = Arc::clone(&key);
                move |hovered, _, cx| {
                    if *hovered {
                        let _ = list.update(cx, |list, cx| list.hover_collapsed(&key, true, cx));
                    }
                }
            })
            .on_click(cx.listener(move |app, _, window, cx| open(app, window, cx)))
            .on_key_down(cx.listener(move |app, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    open(app, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(agent_mark(self.agent_icon(&thread.provider, cx)));
        // The flyout is the list row, laid exactly over the icon so the icon
        // does not move, and drawn above the chats beside the rail.
        let flyout = self
            .chat_line(i, selected, cx)
            .id(SharedString::from(format!("chat-rail-flyout:{id}")))
            .absolute()
            .top_0()
            .left_0()
            .w(rems(20.))
            .bg(fill)
            .when(keyboard_focus, |row| {
                row.border_1().border_color(theme.ring)
            })
            .on_hover(move |hovered, _, cx| {
                if !*hovered {
                    let _ = list.update(cx, |list, cx| list.hover_collapsed(&key, false, cx));
                }
            })
            .on_click(cx.listener(move |app, _, window, cx| open(app, window, cx)));
        div()
            .relative()
            .flex_shrink_0()
            .size(rems(ROW_HEIGHT))
            .child(tile)
            .when(hovered || keyboard_focus, |slot| {
                slot.child(deferred(flyout).with_priority(1))
            })
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
                                    .text_xs()
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
        let theme = cx.theme();
        let searching = self.chat_search_open
            || search_focused
            || !self.query.read(cx).value().is_empty()
            || self.agent_filter.is_some();
        // The frame belongs to the list rather than the input, so the agent button
        // shares the frame's inset instead of the input's fixed text padding. Like
        // every search field, focus shows as a primary border rather than a glow.
        let search = row()
            .id("chat-search-field")
            .w_full()
            .h_8()
            .pl_3()
            // Matches the 1 px border so the agent button is inset evenly on every side.
            .pr(px(1.))
            .rounded(theme.radius)
            .border_1()
            .border_color(if search_focused {
                theme.primary
            } else {
                theme.input
            })
            .bg(theme.background)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|app, _, window, cx| {
                    window.focus(&app.query.focus_handle(cx), cx);
                }),
            )
            .child(icon("search").size(rems(1.)).text_color(if searching {
                theme.primary
            } else {
                theme.muted_foreground
            }))
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
            .h(rems(2.75))
            .flex_shrink_0()
            .pl(rems(0.875))
            .pr_2()
            .gap_0p5()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Chats"),
            )
            .child(
                Button::new("chat-search-toggle")
                    .ghost()
                    .small()
                    .icon(Icon::default().path("search.svg"))
                    .selected(searching)
                    .accessibility_label("Search chats")
                    .tooltip("Search chats")
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.toggle_chat_search(window, cx);
                    })),
            )
            .child(self.icon_button(
                "new-chat",
                "New chat",
                Icon::default().path("new-chat.svg"),
                Action::NewChat,
                cx,
            ));
        col()
            .w_full()
            .min_w_0()
            .h_full()
            .child(header)
            .when(searching, |panel| {
                panel.child(div().w_full().px_2().pb_2().child(search))
            })
            .child(list)
    }
}

pub(crate) const ROW_HEIGHT: f32 = 1.75;

pub(crate) const SECTION_HEIGHT: f32 = 2.125;

pub(crate) const LABEL_GAP: f32 = 0.375;

const END_HEIGHT: f32 = 0.75;

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

pub(crate) const COLLAPSED_SEARCH: &str = "\0search";

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
    collapsed_hover: Option<Arc<str>>,
    collapsed_scroll: UniformListScrollHandle,
    folded: Vec<Group>,
}

const RUNNING_TURN_MS: u64 = 2400;

#[derive(Clone, Copy)]
pub(crate) enum LabelPlacement {
    Inline,
    Top { last: bool },
}

pub(crate) const COLLAPSED_WIDTH: f32 = 2.75;

pub(crate) fn collapsed_item(tile: AnyElement) -> impl IntoElement {
    div()
        .w_full()
        .h(rems(ROW_HEIGHT))
        .flex()
        .justify_center()
        .child(tile)
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
    let theme = cx.theme();
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
    let color = if group == Group::NeedsInput {
        theme.primary
    } else if current {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    div()
        .w_full()
        .flex_shrink_0()
        .px_1p5()
        .pt(rems(LABEL_GAP))
        .bg(theme.background)
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
                .h(rems(SECTION_HEIGHT - LABEL_GAP))
                .px_1p5()
                .gap_1p5()
                .rounded(theme.radius)
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .text_color(color)
                .hover(|style| style.text_color(theme.foreground))
                .on_click(on_click)
                .child(
                    Icon::default()
                        .path("chevron.svg")
                        .size(rems(0.625))
                        .when(folded, |chevron| {
                            chevron.rotate(Radians(-std::f32::consts::FRAC_PI_2))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(group.title().to_uppercase()),
                )
                .child(div().flex_shrink_0().child(chats.to_string())),
        )
        // The pinned stack's edge sits inside the label's side padding. The stack
        // stops short of the scrollbar on the right, so the line gives up the
        // same width on the left and sits evenly in the panel.
        .when(
            matches!(placement, LabelPlacement::Top { last: true }),
            |label| label.child(div().h(px(1.)).ml(theme::SCROLLBAR_TRACK).bg(theme.border)),
        )
        .into_any_element()
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
    let theme = cx.theme();
    match thread.status {
        ThreadStatus::Blocked => row()
            .flex_shrink_0()
            .size(rems(0.8125))
            .justify_center()
            .rounded_full()
            .bg(theme.primary.alpha(0.22))
            .child(div().size(rems(0.4375)).rounded_full().bg(theme.primary))
            .into_any_element(),
        ThreadStatus::Processing => {
            let (track, arc) = (theme.border, theme.foreground);
            div()
                .flex_shrink_0()
                .size(rems(0.625))
                .with_animation(
                    "chat-row-running",
                    Animation::new(Duration::from_millis(RUNNING_TURN_MS)).repeat_synced(),
                    move |ring, turn| ring.child(running_ring(turn, track, arc)),
                )
                .into_any_element()
        }
        _ => div()
            .flex_shrink_0()
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .text_color(theme.muted_foreground)
            .children(updated)
            .into_any_element(),
    }
}

pub(crate) fn collapsed_button(id: &'static str, label: &'static str, icon_name: &str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(Icon::default().path(format!("{icon_name}.svg")))
        .accessibility_label(label)
        .tooltip(label)
}

fn status_name(thread: &Thread) -> &'static str {
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
    pub fn new(owner: WeakEntity<Adeline>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(180.));
        Self {
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
            collapsed_hover: None,
            collapsed_scroll: UniformListScrollHandle::new(),
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
        // Selection draws the row fill and hides the separator above it.
        invalidate_range(&self.state, 0..self.state.item_count());
        cx.notify();
    }

    pub fn search(&mut self, criteria: Criteria, debounce: bool, cx: &mut Context<Self>) {
        self.pending.take();
        self.criteria = criteria.clone();
        let ticket = self.generation.next();
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

    pub fn hover_collapsed(&mut self, id: &Arc<str>, hovered: bool, cx: &mut Context<Self>) {
        // Entering a tile unfurls it; only leaving its own flyout folds it.
        let next = if hovered {
            Some(Arc::clone(id))
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
            let id = Arc::clone(&self.records.get(row)?.id);
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
            .filter_map(|row| Some((row, Arc::clone(&self.records.get(row)?.id))))
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
                        collapsed_item(collapsed_tile(
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
            let new_chat = collapsed_button("chat-rail-new", "New chat", "new-chat")
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::NewChat, window, cx)));
            let search = app.collapsed_search(search_open, matches, list.clone(), cx);
            (new_chat, search)
        });
        col()
            .id("chat-rail")
            .h_full()
            .w(rems(COLLAPSED_WIDTH))
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
