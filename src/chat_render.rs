//! Drawing Chats: chat list rows and sidebar, transcript messages and their
//! actions (Reply, Copy, Retry, Fork), decision rows and the composer.
use super::*;
use crate::chat::{self, LABEL_GAP, ROW_HEIGHT, SECTION_HEIGHT};
use crate::prepared::{Group, Outcome};
use gpui_kit::base::SelectableText;
use gpui_kit::component::plot::shape::{Arc as ArcShape, ArcData};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Side, Sizable as _,
    button::{Button, ButtonVariants as _},
    command::{Command, CommandGroup, CommandItem},
    input::Textarea,
    menu::{DropdownMenu as _, PopupMenuItem},
    spinner::Spinner,
    text::{TextView, TextViewStyle},
    tooltip::Tooltip,
};
use std::f32::consts::TAU;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

/// The running indicator turns once per this many milliseconds.
const RUNNING_TURN_MS: u64 = 2400;

/// Where a section label is drawn.
#[derive(Clone, Copy)]
pub(super) enum LabelPlacement {
    /// In the list, above its chats.
    Inline,
    /// Stacked under the controls; the last one draws the edge to the chats.
    Top { last: bool },
}

/// Width of the collapsed list in rems: the width of the mode rail beside it.
pub(super) const COLLAPSED_WIDTH: f32 = 2.75;

/// One item of the collapsed list: a chat's agent icon in a square the height
/// of a list row, centered in the rail.
pub(super) fn collapsed_item(tile: AnyElement) -> impl IntoElement {
    div()
        .w_full()
        .h(rems(ROW_HEIGHT))
        .flex()
        .justify_center()
        .child(tile)
}

/// A section label: a chevron, the title in capitals and the chat count.
/// Needs Input takes the accent; the section being read takes the foreground.
pub(super) fn section_label(
    group: Group,
    count: usize,
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
                .aria_label(format!("{verb} {}, {}", group.title(), chat_count(count)))
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
                .child(div().flex_shrink_0().child(count.to_string())),
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

fn chat_count(count: usize) -> String {
    format!("{count} {}", if count == 1 { "chat" } else { "chats" })
}

/// The title with the searched text highlighted where the title contains it.
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

/// A track with a quarter arc that has turned `turn` of the way around.
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

/// The right end of a list row: the time since the chat was active, or in its
/// place a small spinner while the agent runs or the accent dot when it waits.
fn row_status(thread: &Thread, updated: Option<String>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    match thread.status.as_str() {
        "blocked" => row()
            .flex_shrink_0()
            .size(rems(0.8125))
            .justify_center()
            .rounded_full()
            .bg(theme.primary.alpha(0.22))
            .child(div().size(rems(0.4375)).rounded_full().bg(theme.primary))
            .into_any_element(),
        "processing" => {
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

/// A command on the collapsed list, the same button as in the full list's header.
pub(super) fn collapsed_button(id: &'static str, label: &'static str, icon_name: &str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(Icon::default().path(format!("{icon_name}.svg")))
        .accessibility_label(label)
        .tooltip(label)
}

/// The chat's status in words, for accessible labels.
fn status_name(thread: &Thread) -> &'static str {
    match thread.status.as_str() {
        "processing" => "running",
        "completed" => "completed",
        "blocked" => "needs input",
        "archived" => "archived",
        _ => "idle",
    }
}

/// A list row's agent icon, also the whole of a chat in the collapsed list.
pub(super) fn agent_mark((path, color): (String, Hsla)) -> themed_icon::ThemedIcon {
    themed_icon::ThemedIcon::path(&path)
        .flex_shrink_0()
        .size(rems(0.8125))
        .text_color(color)
}

impl Adeline {
    /// The icon path and color of the agent named `agent`: its avatar in text
    /// colors, else the muted robot.
    pub(super) fn agent_icon(&self, agent: &str, cx: &App) -> (String, Hsla) {
        let path = self
            .agent_catalog
            .entries
            .iter()
            .find(|entry| entry.definition.name == agent)
            .map_or_else(
                || "robot.svg".into(),
                |entry| agents::avatar_path(&entry.id, &entry.definition.harness),
            );
        let color = if path == "robot.svg" {
            cx.theme().muted_foreground
        } else {
            cx.theme().sidebar_primary_foreground
        };
        (path, color)
    }

    fn agent_name(&self, agent: &str) -> String {
        if self.demo_mode {
            provider(agent).to_owned()
        } else {
            agent.to_owned()
        }
    }

    /// A chat's row: its agent's icon, the title, and the time or live status.
    /// The list and the collapsed list's flyout both draw it, then add their own
    /// identity, fill and pointer handling.
    fn chat_line(&self, i: usize, selected: bool, cx: &Context<Self>) -> Div {
        let thread = &self.workspace().threads[i];
        let activity = thread.last_activity();
        let updated = (activity > 0).then(|| recency::label(activity, recency::now()));
        self.chat_title(i, selected, cx)
            .child(row_status(thread, updated, cx))
    }

    /// A row's agent icon and title, without the status at its right end.
    fn chat_title(&self, i: usize, selected: bool, cx: &Context<Self>) -> Div {
        let thread = &self.workspace().threads[i];
        let theme = cx.theme();
        let done = matches!(thread.status.as_str(), "completed" | "archived");
        let live = matches!(thread.status.as_str(), "blocked" | "processing");
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

    /// A list row's right end. A completed chat trades its time for an Archive
    /// button while the pointer is over the row.
    fn chat_row_end(&self, i: usize, group: &SharedString, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[i];
        let activity = thread.last_activity();
        let updated = (activity > 0).then(|| recency::label(activity, recency::now()));
        let status = row_status(thread, updated, cx);
        if thread.status != "completed" {
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

    /// A chat's accessible description: title, agent, size, age and state.
    fn chat_description(&self, i: usize) -> String {
        let thread = &self.workspace().threads[i];
        let mut description = format!(
            "{}, {}, {} messages",
            thread.title.trim(),
            self.agent_name(&thread.provider),
            thread.messages.len()
        );
        let activity = thread.last_activity();
        if activity > 0 {
            let updated = recency::label(activity, recency::now());
            write!(description, ", updated {updated}").unwrap();
        }
        write!(description, ", {}", status_name(thread)).unwrap();
        if thread.unread() {
            description.push_str(", unread");
        }
        description
    }

    /// A chat in the list.
    pub(super) fn chat_card(&self, i: usize, cx: &Context<Self>) -> AnyElement {
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

    /// The collapsed list's search: the full list's search button, which unfurls
    /// over the chats into the full list's search field and the number of
    /// matches. It stays unfurled while the field has focus.
    pub(super) fn collapsed_search(
        &self,
        open: bool,
        matches: Option<usize>,
        list: WeakEntity<chat::ChatList>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let key: Arc<str> = Arc::from(chat::COLLAPSED_SEARCH);
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
                let key = key.clone();
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

    /// A chat in the collapsed list: its agent icon. While the pointer is on it,
    /// or keyboard focus, it unfurls toward the chats into the chat's row from
    /// the full list; the rail itself keeps its width.
    pub(super) fn collapsed_chat(
        &self,
        i: usize,
        hovered: bool,
        focus: &FocusHandle,
        keyboard_focus: bool,
        list: WeakEntity<chat::ChatList>,
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
                let key = key.clone();
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

    /// The agent control at the end of the search field: a robot that opens the
    /// agent menu, or the chosen agent's icon, which clears the choice.
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
                (agent.clone(), self.agent_name(agent), path, *count)
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
                    let agent = agent.clone();
                    menu = menu.item(
                        PopupMenuItem::element(counted(name.clone(), *count))
                            .icon(Icon::default().path(path.clone()))
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |app, cx| {
                                    app.act(Action::AgentFilter(Some(agent.clone())), window, cx);
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    pub(super) fn chat_sidebar(
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

    pub(super) fn welcome(&self) -> Stateful<Div> {
        col()
            .id("chat-welcome")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                col()
                    .w_full()
                    .min_h_full()
                    .flex_shrink_0()
                    .justify_center()
                    .items_center()
                    .text_center()
                    .p_3()
                    .gap_3()
                    .child(div().text_xl().child("Start a chat"))
                    .child(
                        div()
                            .text_sm()
                            .child("Choose an agent, then write a message."),
                    ),
            )
    }

    pub(super) fn message_row(&self, index: usize, i: usize, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let thread = &self.workspace().threads[index];
        let message = &thread.messages[i];
        let user = message.role == "user";
        let time = message_time(&message.created_at);
        let mut body = col().w_full().min_w_0().gap_2();
        if user || self.demo_mode {
            for (n, paragraph) in message.text.split("\n\n").enumerate() {
                body = body.child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .child(SelectableText::new(("paragraph", n), paragraph.to_owned())),
                );
            }
        } else {
            // Compact text with inline code in the accent on the muted fill; the
            // default fill is nearly the background and hides code spans. Links
            // take `chart_2` through the theme's link color.
            body = body.child(
                TextView::markdown("response", message.text.clone())
                    .style(
                        TextViewStyle::default()
                            .paragraph_gap(rems(0.5))
                            .inline_code(HighlightStyle {
                                color: Some(theme.primary),
                                background_color: Some(theme.muted),
                                ..Default::default()
                            }),
                    )
                    .text_sm()
                    .w_full()
                    .min_w_0(),
            );
        }
        for path in &message.images {
            let asset = image_asset(path);
            // Reserve height before decoding so a virtual row keeps its scroll anchor.
            let bytes = embedded(asset).expect("bundled message image");
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
            body = body.child(
                img(ImageSource::Resource(Resource::Embedded(asset.into())))
                    .w_full()
                    .max_w(rems(47.5))
                    .map(|mut image| {
                        image.style().aspect_ratio = Some(width as f32 / height as f32);
                        image
                    })
                    .rounded_lg()
                    .object_fit(ObjectFit::Contain),
            );
        }
        let content = if user {
            col()
                .items_end()
                .child(
                    row()
                        .h_6()
                        .gap_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("You")
                        .children(time),
                )
                .child(
                    div()
                        .max_w(relative(0.78))
                        .min_w_0()
                        .bg(theme.secondary)
                        .rounded(rems(0.875))
                        .px(rems(0.875))
                        .py(rems(0.5625))
                        .child(body),
                )
                .child(
                    Button::new(SharedString::from(format!("copy-prompt:{}:{i}", thread.id)))
                        .ghost()
                        .small()
                        .mt_1()
                        .icon(Icon::default().path("copy.svg"))
                        .accessibility_label("Copy")
                        .tooltip("Copy")
                        .on_click(cx.listener(move |app, _, window, cx| {
                            app.act(Action::CopyMessage(i), window, cx);
                        })),
                )
                .into_any_element()
        } else {
            let name = if self.demo_mode {
                provider(&thread.provider).to_owned()
            } else {
                thread.provider.clone()
            };
            // The reply being written gets its closing row when the turn ends.
            let writing = self.conversation_processing()
                && thread
                    .messages
                    .iter()
                    .rposition(|m| m.role == "user")
                    .is_some_and(|last| i > last);
            col()
                .w_full()
                .min_w_0()
                .child(
                    agent_header(self.agent_icon(&thread.provider, cx), name, cx).children(
                        time.map(|time| {
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(time)
                        }),
                    ),
                )
                .child(div().mt_1().child(body))
                .when(thread.ends_turn(i) && !writing, |content| {
                    content.child(self.closing_row(thread, i, cx))
                })
                .into_any_element()
        };
        div()
            .id(SharedString::from(format!(
                "chat-message:{}:{}:{i}",
                self.workspace().config.id,
                thread.id
            )))
            .w_full()
            .min_w_0()
            // The message under the hovered row of the activity panel.
            .when(self.activity.lit == Some(i), |row| {
                row.bg(theme.primary.alpha(0.08))
            })
            .child(
                chat_column()
                    .pb(rems(if user { 1.5 } else { 2. }))
                    .when_some(thread.fork.as_ref().filter(|_| i == 0), |column, fork| {
                        column.child(self.fork_marker(fork, cx))
                    })
                    .child(content),
            )
            .into_any_element()
    }

    /// "Forked from" its source, above a fork's copied history.
    fn fork_marker(&self, fork: &Fork, cx: &Context<Self>) -> Stateful<Div> {
        let source = self
            .workspace()
            .threads
            .iter()
            .position(|thread| thread.id == fork.id);
        let title = source.map_or_else(
            || fork.title.clone(),
            |ix| self.workspace().threads[ix].title.clone(),
        );
        let note = "Started from a text copy of the history.";
        let spoken = if fork.text_copy {
            format!("Forked from {title}. {note}")
        } else {
            format!("Forked from {title}")
        };
        let link = match source {
            Some(ix) => Button::new("fork-source")
                .link()
                .small()
                .label(title)
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.act(Action::Chat(ix), window, cx);
                }))
                .into_any_element(),
            None => div().child(title).into_any_element(),
        };
        col()
            .id("fork-marker")
            .role(Role::Group)
            .aria_label(spoken)
            .w_full()
            .pb_4()
            .gap_1()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(
                row()
                    .gap_1p5()
                    .child(icon("fork").size(rems(0.875)))
                    .child("Forked from")
                    .child(link),
            )
            .when(fork.text_copy, |marker| marker.child(note))
    }

    /// Reply, Copy and Retry for a finished turn's last reply, then what the turn did.
    fn closing_row(&self, thread: &Thread, i: usize, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let turn = thread.turn_of(i);
        let summary = turn
            .map(|turn| thread.turn_summary(turn))
            .filter(|summary| summary.tools > 0);
        let key = format!("summary:{}:{i}", thread.id);
        let expanded = summary.is_some() && self.runtime.expanded_tools.contains(&key);
        // A fork that hasn't sent yet has nothing of its own to retry.
        let retry = !self.demo_mode
            && i + 1 == thread.messages.len()
            && self
                .runtime
                .conversations
                .get(&thread.id)
                .is_some_and(|live| !live.last_prompt.is_empty());
        let action = |name: &str, asset: &str, label: &'static str, action: Action| {
            Button::new(SharedString::from(format!("{name}:{}:{i}", thread.id)))
                .ghost()
                .small()
                .icon(Icon::default().path(format!("{asset}.svg")))
                .accessibility_label(label)
                .tooltip(label)
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.act(action.clone(), window, cx);
                }))
        };
        let actions = row()
            // Cancel the button inset so the first icon sits on the text edge.
            .ml(rems(-0.3125))
            .gap(rems(0.125))
            .child(action("reply", "reply", "Reply", Action::ReplyTo(i)))
            .child(action("copy", "copy", "Copy", Action::CopyMessage(i)))
            .child(action("fork", "fork", "Fork", Action::Fork(i)))
            .when(retry, |actions| {
                actions.child(action(
                    "retry",
                    "arrow-counter-clockwise",
                    "Retry",
                    Action::RetryPrompt,
                ))
            });
        let mut content = row().w_full().min_w_0().gap_1().child(actions);
        if let Some(summary) = summary {
            let mut parts = Vec::new();
            if summary.read > 0 {
                parts.push(("file", summary.read, "file", "Read"));
            }
            if summary.edited > 0 {
                parts.push(("edit", summary.edited, "file", "edited"));
            }
            parts.push(("wrench", summary.tools, "tool", "used"));
            let spoken = parts
                .iter()
                .map(|(_, number, noun, verb)| format!("{verb} {}", count(*number, noun)))
                .collect::<Vec<_>>()
                .join(", ");
            let last = parts.len() - 1;
            let mut button = Button::new(SharedString::from(format!(
                "turn-summary:{}:{i}",
                thread.id
            )))
            .ghost()
            .small()
            .text_color(theme.muted_foreground)
            .accessibility_label(format!(
                "{spoken}. {}",
                if expanded { "Hide steps" } else { "Show steps" }
            ))
            .on_click(cx.listener(move |app, _, window, cx| {
                app.act(Action::ToggleTool(key.clone()), window, cx);
            }));
            for (ix, (asset, number, noun, _)) in parts.into_iter().enumerate() {
                button = button.child(
                    row()
                        .gap_1()
                        .when(ix < last, |part| part.mr_1p5())
                        .child(icon(asset).size(rems(0.8125)))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(number.to_string()),
                        )
                        .child(format!(
                            "{noun}{}{}",
                            if number == 1 { "" } else { "s" },
                            if ix < last { "," } else { "" }
                        )),
                );
            }
            content =
                content.child(button.child(
                    icon(if expanded { "chevron" } else { "caret-right" }).size(rems(0.625)),
                ));
        }
        col()
            .w_full()
            .mt_2()
            .child(content)
            .when_some(turn.filter(|_| expanded), |column, turn| {
                column.child(tool_steps(thread.turn_tools(turn).collect(), cx))
            })
    }

    pub(super) fn decision_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[index];
        if let Some(decision) = self
            .workspace()
            .decisions
            .iter()
            .find(|decision| decision.thread_id == thread.id)
        {
            let mut content = col()
                .w_full()
                .min_w_0()
                .p_4()
                .gap_3()
                .bg(cx.theme().group_box)
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(decision.title.clone()),
                )
                .child(div().text_sm().child(decision.body.clone()));
            for (i, option) in decision.options.iter().enumerate() {
                content = content.child(
                    Button::new(format!(
                        "chat-decision:{}:{}:{i}",
                        self.workspace().config.id,
                        thread.id
                    ))
                    .outline()
                    .w_full()
                    .selected(decision.selected == Some(i))
                    .label(option.clone())
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(Action::Decision(i), window, cx);
                    })),
                );
            }
            content.into_any_element()
        } else if thread.status == "blocked" {
            div().text_sm().child("Needs your input").into_any_element()
        } else {
            div().into_any_element()
        }
    }
    /// `width` is the composer's width in rems; narrower composers shorten labels.
    pub(super) fn composer_view(&self, width: f32, cx: &Context<Self>) -> Div {
        // The controls sit in the chat column, inside its side padding.
        let fit = Fit::for_width(width.min(CHAT_COLUMN) - 4.);
        let theme = cx.theme();
        let bound = self.bound_definition();
        let selected = self.selected_definition();
        let agent_name: SharedString = bound
            .as_ref()
            .map(|definition| definition.name.as_str())
            .or_else(|| selected.map(|definition| definition.name.as_str()))
            .unwrap_or("Select an agent")
            .to_owned()
            .into();
        let execution = bound
            .as_ref()
            .map(|definition| (definition.model.clone(), definition.effort.clone()))
            .or_else(|| {
                selected.map(|definition| (definition.model.clone(), definition.effort.clone()))
            });
        let options = if bound.is_some() {
            self.conversation_options()
        } else {
            Vec::new()
        };
        let no_agents = bound.is_none() && self.agent_catalog.entries.is_empty() && !self.demo_mode;
        let (agent_path, agent_color) = self.agent_icon(&agent_name, cx);
        let processing = self.conversation_processing();
        let can_send = self
            .composer
            .read(cx)
            .text()
            .chars()
            .any(|character| !character.is_whitespace());
        let permission = self.current_permission_mode().map(|mode| {
            let label = permission_label(mode);
            let owner = cx.weak_entity();
            let shown = if fit.short_permission && mode == agents::PermissionMode::Ask {
                "Approval"
            } else {
                label
            };
            Button::new("chat-permission-mode")
                .ghost()
                .small()
                .flex_shrink_0()
                .label(shown)
                .dropdown_caret(true)
                .tooltip(format!("Permissions: {label}"))
                .accessibility_label(format!("Permissions: {label}"))
                .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                    let mut menu = menu.check_side(Side::Right).label("Permissions");
                    for (ix, choice) in agents::PermissionMode::ALL.into_iter().enumerate() {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(permission_label(choice))
                                .checked(choice == mode)
                                .on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |app, cx| {
                                        app.act(Action::Permission(ix), window, cx);
                                    });
                                }),
                        );
                    }
                    menu
                })
        });
        let send = if no_agents {
            // Sending needs an agent, so adding one is the main action.
            Button::new("send-chat-message")
                .small()
                .primary()
                .label("Add an agent…")
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::AddAgent, window, cx)))
        } else if processing {
            Button::new("send-chat-message")
                .small()
                .ghost()
                .icon(Icon::default().path("stop.svg"))
                .accessibility_label("Stop")
                .tooltip("Stop")
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::Stop, window, cx)))
        } else {
            // The icons spell the send shortcut: Enter, or Shift+Enter. Both
            // are content, not the button's icon, so the padding and the Enter
            // key stay put when the shortcut changes.
            Button::new("send-chat-message")
                .small()
                .ghost()
                .child(
                    row()
                        .gap_0p5()
                        .when(!config::with(|s| s.modes.chats.submit_on_enter), |keys| {
                            keys.child(Icon::default().path("arrow-fat-up.svg").size_4())
                        })
                        .child(Icon::default().path("arrow-elbow-down-left.svg").size_4()),
                )
                .accessibility_label("Send")
                .tooltip("Send")
                .disabled(!can_send)
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::Send, window, cx)))
        };
        let island = col()
            .w_full()
            .min_w_0()
            .bg(theme.group_box)
            .border_1()
            .border_color(theme.border)
            .rounded(rems(0.875))
            .pt_3()
            .pb_2()
            .pl(rems(0.875))
            .pr(rems(0.625))
            .child(
                row()
                    .w_full()
                    .min_w_0()
                    .items_start()
                    .gap_2()
                    .child(
                        Textarea::new(&self.composer)
                            .aria_label("Message")
                            .appearance(false)
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(div().flex_shrink_0().child(send)),
            )
            .child(
                row()
                    .w_full()
                    .min_w_0()
                    .mt_2()
                    .gap(rems(0.125))
                    .child(
                        self.command_popover(
                            "files",
                            Button::new("attach-chat-files")
                                .ghost()
                                .small()
                                .flex_shrink_0()
                                .icon(Icon::default().path("plus.svg"))
                                .accessibility_label("Attach files…")
                                .tooltip("Attach files…"),
                            Anchor::BottomLeft,
                            cx,
                        ),
                    )
                    .child(div().w_px().h_4().mx_1p5().flex_shrink_0().bg(theme.border))
                    .child(
                        row()
                            .id("chat-agent-selection")
                            .role(Role::Group)
                            .aria_label("Agent selection")
                            .flex_1()
                            .min_w_0()
                            // Clip rather than draw under the permission button.
                            .overflow_hidden()
                            .gap_1()
                            .child(
                                self.command_popover(
                                    "agent",
                                    Button::new("chat-agent-picker")
                                        .ghost()
                                        .small()
                                        .icon(
                                            Icon::default()
                                                .path(agent_path)
                                                .text_color(agent_color),
                                        )
                                        .label(shorten(&agent_name, fit.agent))
                                        .tooltip(agent_name)
                                        .dropdown_caret(true),
                                    Anchor::BottomLeft,
                                    cx,
                                ),
                            )
                            .when_some(execution, |selection, (model, effort)| {
                                selection
                                    .children(self.setting_menu(
                                        harness::Kind::Model,
                                        model,
                                        &options,
                                        processing,
                                        fit.model,
                                        cx,
                                    ))
                                    .children(self.setting_menu(
                                        harness::Kind::Effort,
                                        effort,
                                        &options,
                                        processing,
                                        fit.effort,
                                        cx,
                                    ))
                            }),
                    )
                    .children(permission),
            );
        col()
            .w_full()
            .flex_shrink_0()
            .min_w_0()
            .bg(theme.background)
            .pb_4()
            .child(chat_column().child(island))
    }
}

impl Adeline {
    /// The composer's Model or Effort menu. With the harness's options it
    /// switches this conversation; without them it shows the value read-only.
    fn setting_menu(
        &self,
        kind: harness::Kind,
        value: String,
        options: &[serde_json::Value],
        processing: bool,
        max_chars: usize,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let (menu, id, heading) = match kind {
            harness::Kind::Model => ("model", "chat-model", "Model"),
            harness::Kind::Effort => ("effort", "chat-effort", "Effort"),
        };
        let Some(setting) = harness::setting(options, kind) else {
            if value.is_empty() {
                return None;
            }
            let label = match kind {
                harness::Kind::Model => model_label(&value),
                harness::Kind::Effort => effort_label(&value),
            };
            return Some(execution_setting(id, heading, label, max_chars, cx).into_any_element());
        };
        let current = if value.is_empty() {
            setting.current.clone()
        } else {
            value
        };
        let label: SharedString = setting.name_of(&current).to_owned().into();
        let trigger = Button::new(id)
            .ghost()
            .small()
            .flex_shrink_0()
            .label(shorten(&label, max_chars))
            .dropdown_caret(true)
            .disabled(processing)
            .tooltip(if processing {
                format!("{heading}: {label}. Switch after this turn finishes.")
            } else {
                format!("{heading}: {label}")
            })
            .accessibility_label(format!("{heading}: {label}"));
        if processing {
            return Some(trigger.into_any_element());
        }
        let mut groups: Vec<(String, Vec<harness::Choice>)> = Vec::new();
        for choice in setting.choices {
            match groups.last_mut() {
                Some((group, items)) if *group == choice.group => items.push(choice),
                _ => groups.push((choice.group.clone(), vec![choice])),
            }
        }
        let owner = cx.weak_entity();
        let content_owner = owner.clone();
        let state = self.command_popup.clone();
        Some(
            component::popover::Popover::new(menu)
                .anchor(Anchor::BottomLeft)
                .trigger(trigger)
                .open(self.menu == Some(menu))
                .p_0()
                .shadow(project_bar::menu_shadow(cx))
                .when_some(state.as_ref(), |popover, state| {
                    popover.track_focus(&state.focus_handle(cx))
                })
                .on_open_change(move |open, window, cx| {
                    let _ = owner.update(cx, |app, cx| {
                        if *open {
                            app.open_commands(menu, window, cx);
                        } else if app.menu == Some(menu) {
                            app.menu = None;
                            app.composer_region.update(cx, |_, cx| cx.notify());
                            cx.notify();
                        }
                    });
                })
                .content(move |_, _, cx| {
                    let popover = cx.entity();
                    let owner = content_owner.clone();
                    let values: Vec<Vec<String>> = groups
                        .iter()
                        .map(|(_, items)| items.iter().map(|c| c.value.clone()).collect())
                        .collect();
                    col().w(rems(24.)).when_some(state.as_ref(), |column, state| {
                        let mut command = Command::new(state)
                            .bordered(false)
                            .placeholder(format!("Search {}", heading.to_lowercase()))
                            .header(|_, _, cx| {
                                div()
                                    .px_3()
                                    .py_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "Switching may invalidate the prompt cache and cost more on the next turn.",
                                    )
                            });
                        for (group, items) in &groups {
                            let mut entry = CommandGroup::new();
                            if !group.is_empty() {
                                entry = entry.label(group.clone());
                            }
                            command = command.group(entry.items(items.iter().map(|choice| {
                                CommandItem::new()
                                    .label(choice.name.clone())
                                    .keywords([choice.value.clone()])
                                    .checked(choice.value == current)
                            })));
                        }
                        column.child(command.on_confirm(move |path, window, cx| {
                            let Some(value) = values
                                .get(path.section)
                                .and_then(|items| items.get(path.row))
                                .cloned()
                            else {
                                return;
                            };
                            popover.update(cx, |state, cx| state.dismiss(window, cx));
                            let _ = owner.update(cx, |app, cx| {
                                app.menu = None;
                                app.switch_setting(kind, value, cx);
                            });
                        }))
                    })
                })
                .into_any_element(),
        )
    }
}

/// Height of the chat header, in rems. It floats over the top of the transcript.
pub(super) const CHAT_HEADER_HEIGHT: f32 = 3.;
/// How far below the header the messages fade out as they scroll under it, in rems.
pub(super) const HEADER_FADE: f32 = 1.75;

/// Width of the chat column, in rems. The header, replies and composer share its edges.
pub(super) const CHAT_COLUMN: f32 = 46.;

/// The agent's icon and name that head its replies and its live progress.
pub(super) fn agent_header(
    agent_icon: (String, Hsla),
    name: impl Into<SharedString>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    row()
        .h_7()
        .gap_2()
        .child(
            div()
                .size_5()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius)
                .bg(theme.secondary)
                .child(agent_mark(agent_icon).size(rems(0.75))),
        )
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(name.into()),
        )
}

/// Braille frames ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ as dot masks: bits 0-2 are the left column top
/// to bottom, bits 3-5 the right. The bundled fonts have no braille glyphs, so
/// the cell is drawn from dots.
const BRAILLE_FRAMES: [u8; 10] = [0x0B, 0x19, 0x39, 0x38, 0x3C, 0x34, 0x26, 0x27, 0x07, 0x0F];
const THINKING_WORDS: [&str; 4] = ["Thinking…", "Reasoning…", "Pondering…", "Working it out…"];

/// The label after the agent's name while it works, animated as chosen in
/// settings. Progress the agent reports replaces the cycling words, which only
/// decorate.
pub(super) fn thinking_label(progress: Option<String>, cx: &App) -> AnyElement {
    let muted: Hsla = rgb(theme::muted_foreground()).into();
    let accent = cx.theme().primary;
    let size = config::text_pixels(13.);
    let base = div().text_size(size).text_color(muted).whitespace_nowrap();
    match config::with(|s| s.modes.chats.thinking_animation) {
        config::ThinkingAnimation::Words if progress.is_none() => {
            let line = size * 1.5;
            // One word shows at a time; each holds, then rolls up to the next.
            base.h(line)
                .overflow_hidden()
                .child(
                    div()
                        .relative()
                        .children(
                            THINKING_WORDS
                                .iter()
                                .chain(THINKING_WORDS.first())
                                .map(|word| div().h(line).flex().items_center().child(*word)),
                        )
                        .with_animation(
                            "thinking-words",
                            Animation::new(Duration::from_secs(8)).repeat(),
                            move |words, delta| {
                                let step = delta * THINKING_WORDS.len() as f32;
                                let slide = ((step.fract() - 0.875) / 0.125).clamp(0., 1.);
                                words.top(-line * (step.floor() + ease_in_out(slide)))
                            },
                        ),
                )
                .into_any_element()
        }
        config::ThinkingAnimation::Dots => {
            let label = progress.unwrap_or_else(|| "Thinking".into());
            let dot = size * 0.23;
            row()
                .child(base.child(label.trim_end_matches(['…', '.']).to_owned()))
                .child(row().gap(dot).ml(dot).with_animation(
                    "thinking-dots",
                    Animation::new(Duration::from_millis(1200)).repeat(),
                    move |dots, delta| {
                        dots.children((0..3).map(|i| {
                            // Each dot rises and brightens in its own
                            // part of the cycle, a beat after the last.
                            let phase = (delta - i as f32 * 0.125).rem_euclid(1.);
                            let lift = if phase < 0.6 {
                                (phase / 0.6 * std::f32::consts::PI).sin()
                            } else {
                                0.
                            };
                            div()
                                .size(dot)
                                .rounded_full()
                                .bg(muted)
                                .opacity(0.4 + 0.6 * lift)
                                .relative()
                                .top(-dot * 1.3 * lift)
                        }))
                    },
                ))
                .into_any_element()
        }
        config::ThinkingAnimation::Braille => {
            let dot = size * 0.2;
            row()
                .gap(size * 0.5)
                .child(
                    row().gap(dot * 0.6).with_animation(
                        "thinking-braille",
                        Animation::new(Duration::from_millis(800))
                            .repeat()
                            .with_max_fps(12.5),
                        move |cell, delta| {
                            let frames = BRAILLE_FRAMES.len();
                            let frame = (1..frames)
                                .filter(|&i| delta * frames as f32 >= i as f32)
                                .count();
                            let mask = BRAILLE_FRAMES[frame];
                            cell.children((0..2).map(|column| {
                                col().gap(dot * 0.6).children((0..3).map(move |dot_row| {
                                    let lit = mask >> (column * 3 + dot_row) & 1 == 1;
                                    div().size(dot).rounded_full().when(lit, |d| d.bg(accent))
                                }))
                            }))
                        },
                    ),
                )
                .child(base.child(progress.unwrap_or_else(|| "Thinking…".into())))
                .into_any_element()
        }
        // Accent paints into the text from the left, then starts over.
        config::ThinkingAnimation::Fill => {
            let label: SharedString = progress.unwrap_or_else(|| "Thinking…".into()).into();
            base.relative()
                .child(label.clone())
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .overflow_hidden()
                        .text_color(accent)
                        .child(div().whitespace_nowrap().child(label))
                        .with_animation(
                            "thinking-fill",
                            Animation::new(Duration::from_millis(2400))
                                .repeat()
                                .with_easing(ease_in_out),
                            |fill, delta| fill.w(relative(delta)),
                        ),
                )
                .into_any_element()
        }
        config::ThinkingAnimation::Words => {
            base.child(progress.unwrap_or_default()).into_any_element()
        }
    }
}

/// The centered column the chat header, transcript and composer align to.
pub(super) fn chat_column() -> Div {
    div().w_full().max_w(rems(CHAT_COLUMN)).mx_auto().px_8()
}

/// How much of the agent's context the chat uses: `19%`, a bar, then the window
/// size. Dashes stand in until the agent reports its usage.
pub(super) fn context_meter(usage: Option<(u64, u64)>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let share = match usage {
        Some((used, size)) if size > 0 => (used as f32 / size as f32).clamp(0., 1.),
        _ => 0.,
    };
    let (percent, window, label) = match usage {
        Some((used, size)) => (
            format!("{}%", (share * 100.).round()),
            tokens(size),
            format!("Context: {} of {} tokens used", tokens(used), tokens(size)),
        ),
        None => (
            "–".to_owned(),
            "–".to_owned(),
            "Context: the agent hasn’t reported its usage yet".to_owned(),
        ),
    };
    row()
        .id("chat-context")
        .role(Role::ProgressIndicator)
        .aria_label(label.clone())
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .flex_shrink_0()
        .gap_2()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(percent)
        .child(
            div()
                .w_12()
                .h_1()
                .rounded_full()
                .bg(theme.secondary)
                .overflow_hidden()
                .child(div().h_full().w(relative(share)).bg(theme.muted_foreground)),
        )
        .child(window)
}

/// A model name without its provider path: `anthropic/claude-opus-5` is `claude-opus-5`.
fn model_label(model: &str) -> String {
    model.rsplit('/').next().unwrap_or(model).to_owned()
}

/// An effort level as the composer shows it: `high` is `High`.
fn effort_label(effort: &str) -> String {
    let mut chars = effort.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// How much of each composer label fits, by composer width.
struct Fit {
    short_permission: bool,
    /// Most characters of the agent, model and effort labels.
    agent: usize,
    model: usize,
    effort: usize,
}

impl Fit {
    fn for_width(rems: f32) -> Self {
        let (short_permission, agent, model, effort) = if rems >= 40. {
            (false, usize::MAX, usize::MAX, usize::MAX)
        } else if rems >= 36. {
            (true, usize::MAX, usize::MAX, usize::MAX)
        } else if rems >= 33. {
            (true, usize::MAX, 16, usize::MAX)
        } else if rems >= 30. {
            (true, 12, 10, 6)
        } else {
            // The narrowest window.
            (true, 8, 7, 5)
        };
        Self {
            short_permission,
            agent,
            model,
            effort,
        }
    }
}

/// `text` cut to `max` characters, ending in an ellipsis when cut.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let kept: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{}\u{2026}", kept.trim_end())
    }
}

/// A composer selector for a setting the agent owns. Its menu shows the
/// current value and where to change it.
fn execution_setting(
    id: &'static str,
    heading: &'static str,
    value: String,
    max_chars: usize,
    cx: &Context<Adeline>,
) -> impl IntoElement {
    let owner = cx.weak_entity();
    let current: SharedString = value.into();
    Button::new(id)
        .ghost()
        .small()
        .flex_shrink_0()
        .label(shorten(&current, max_chars))
        .tooltip(format!("{heading}: {current}"))
        .dropdown_caret(true)
        .accessibility_label(format!("{heading}: {current}"))
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
            let owner = owner.clone();
            menu.check_side(Side::Right)
                .label(heading)
                .item(PopupMenuItem::new(current.clone()).checked(true))
                .separator()
                .item(
                    PopupMenuItem::new("Agent settings…").on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |app, cx| {
                            app.act(Action::AgentSettings, window, cx);
                        });
                    }),
                )
        })
}

/// A token count as the header shows it: `950`, `38k`, `1.5M`.
pub(super) fn tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.).replace(".0M", "M")
    } else if count >= 1_000 {
        format!("{}k", (count + 500) / 1_000)
    } else {
        count.to_string()
    }
}

/// `1 file`, `4 files`.
fn count(number: usize, noun: &str) -> String {
    format!("{number} {noun}{}", if number == 1 { "" } else { "s" })
}

fn permission_label(mode: agents::PermissionMode) -> &'static str {
    match mode {
        agents::PermissionMode::Ask => "Ask for approval",
        agents::PermissionMode::AllowReads => "Allow reads",
        agents::PermissionMode::AllowEverything => "Allow everything",
    }
}

/// When a message was sent, in local time: `2:43 PM` today, else with its date.
pub(super) fn message_time(stamp: &str) -> Option<String> {
    use chrono::Datelike as _;
    let at = recency::parse(stamp)?;
    let local = chrono::DateTime::from_timestamp(at, 0)?.with_timezone(&chrono::Local);
    let today = chrono::Local::now().date_naive();
    let format = if local.date_naive() == today {
        "%-I:%M %p"
    } else if local.year() == today.year() {
        "%b %-d, %-I:%M %p"
    } else {
        "%b %-d %Y, %-I:%M %p"
    };
    Some(local.format(format).to_string())
}

/// The icon for a tool call, by its protocol kind.
pub(super) fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "read" => "file",
        "edit" | "delete" | "move" => "edit",
        "execute" => "code",
        "search" => "search",
        "fetch" => "link",
        "think" => "sparkle",
        _ => "wrench",
    }
}

/// A tool call's title without the status the runtime appends to it.
fn step_title(call: &Activity) -> String {
    call.title
        .strip_suffix(" (completed)")
        .unwrap_or(&call.title)
        .to_owned()
}

/// Tool calls as a compact list; running calls show a spinner.
pub(super) fn tool_steps(calls: Vec<&Activity>, cx: &App) -> Div {
    let theme = cx.theme();
    col()
        .w_full()
        .min_w_0()
        .py_1()
        .children(calls.into_iter().map(|call| {
            row()
                .h_6()
                .min_w_0()
                .gap_2()
                .text_sm()
                .text_color(if call.running {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .child(if call.running {
                    Spinner::new().xsmall().into_any_element()
                } else {
                    icon(tool_icon(&call.tool))
                        .size(rems(0.75))
                        .into_any_element()
                })
                .child(div().min_w_0().truncate().child(step_title(call)))
        }))
}

#[cfg(test)]
mod tests {
    use super::{effort_label, model_label, tokens};

    #[test]
    fn composer_labels_drop_provider_paths_and_capitalize_effort() {
        assert_eq!(model_label("anthropic/claude-opus-5"), "claude-opus-5");
        assert_eq!(model_label("openrouter/openai/gpt-6"), "gpt-6");
        assert_eq!(model_label("sonnet"), "sonnet");
        assert_eq!(effort_label("medium"), "Medium");
        assert_eq!(effort_label("High"), "High");
        assert_eq!(effort_label(""), "");
    }

    #[test]
    fn token_counts_round_to_short_units() {
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(38_400), "38k");
        assert_eq!(tokens(200_000), "200k");
        assert_eq!(tokens(1_000_000), "1M");
        assert_eq!(tokens(1_500_000), "1.5M");
    }
}
