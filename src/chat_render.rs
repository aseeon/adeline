use super::*;
use crate::chat::{self, ROW_HEIGHT, SECTION_HEIGHT};
use crate::prepared::{Group, Outcome};
use gpui_kit::component::plot::shape::{Arc as ArcShape, ArcData};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Side, Sizable as _, ThemeStyled as _,
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariants as _},
    input::Textarea,
    menu::{DropdownMenu as _, PopupMenuItem},
    message::{Message, MessageAlignment, MessageContent, MessageHeader},
    tab::{Tab, TabBar},
    text::TextView,
};
use std::f32::consts::TAU;
use std::fmt::Write as _;
use std::sync::Arc;

/// The filter each chat tab selects: all chats, needing input, unread.
const TAB_FILTERS: [usize; 3] = [0, 1, 3];
/// Width of the agent and status lanes that frame every chat row, in rems.
const LANE: f32 = 2.25;
/// The running indicator turns once per this many milliseconds.
const RUNNING_TURN_MS: u64 = 2400;

/// The dotted line that ties the agent lane of the list together.
#[derive(Clone, Copy)]
pub(super) struct Rail {
    /// Top of the item in list content coordinates, so dots stay on one pitch.
    pub top: Pixels,
    /// The first item draws from its center down.
    pub starts: bool,
    /// The end marker draws down to its center.
    pub ends: bool,
}

/// Where a section label is drawn.
#[derive(Clone, Copy)]
pub(super) enum LabelPlacement {
    /// In the list, above its chats.
    Inline(Rail),
    /// Stacked under the controls; the last one draws the edge to the chats.
    Top { last: bool },
    /// Stacked at the bottom; the first one draws the edge to the chats.
    Bottom { first: bool },
}

/// The icon asset for an agent, by the name stored on the chat.
fn agent_icon_asset(agent: &str) -> &'static str {
    let agent = agent.to_lowercase();
    if agent.contains("claude") {
        "claude"
    } else if agent.contains("codex") || agent.contains("openai") {
        "codex"
    } else {
        "robot"
    }
}

/// The agent's icon and its color: Claude in the accent, others in text colors.
fn agent_icon(agent: &str, cx: &App) -> (&'static str, Hsla) {
    let asset = agent_icon_asset(agent);
    let color = match asset {
        "claude" => cx.theme().primary,
        "codex" => cx.theme().sidebar_primary_foreground,
        _ => cx.theme().muted_foreground,
    };
    (asset, color)
}

/// Paints the rail's dots down `x` from `from` to `to`. `offset` is how far
/// `from` sits below the top of the whole rail, so separately painted pieces
/// share one pitch.
fn paint_dots(x: Pixels, from: Pixels, to: Pixels, offset: Pixels, window: &mut Window, cx: &App) {
    let rem = window.rem_size();
    let dot = rem * 0.125;
    let pitch = rem * 0.375;
    let phase = offset % pitch;
    let mut y = from
        + if phase > px(0.) {
            pitch - phase
        } else {
            px(0.)
        };
    let color = cx.theme().input;
    while y + dot <= to {
        window.paint_quad(
            fill(Bounds::new(point(x - dot / 2., y), size(dot, dot)), color).corner_radii(dot / 2.),
        );
        y += pitch;
    }
}

fn rail_canvas(rail: Rail) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, cx| {
            let center = bounds.center();
            let from = if rail.starts { center.y } else { bounds.top() };
            let to = if rail.ends { center.y } else { bounds.bottom() };
            // Align every item's dots to one pitch measured from the top of the list.
            paint_dots(
                center.x,
                from,
                to,
                rail.top + (from - bounds.top()),
                window,
                cx,
            );
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .left(rems(0.5))
    .w(rems(LANE))
}

/// Height of one item of the collapsed list: a tile and the gap below it, in rems.
const COLLAPSED_ITEM: f32 = LANE + 0.5;
/// Width of the collapsed list: a tile with half a gap on either side, in rems.
pub(super) const COLLAPSED_WIDTH: f32 = LANE + 0.5;

/// One item of the collapsed list: its tile over the rail, which runs from the
/// center of the first tile to the center of the last. `ix` and `count` place
/// the item in the list so every item's dots share one pitch.
pub(super) fn collapsed_item(ix: usize, count: usize, tile: AnyElement) -> impl IntoElement {
    div()
        .relative()
        .w_full()
        .h(rems(COLLAPSED_ITEM))
        .flex()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, cx| {
                    let rem = window.rem_size();
                    let center = bounds.top() + rem * (LANE / 2.);
                    let from = if ix == 0 { center } else { bounds.top() };
                    let to = if ix + 1 == count {
                        center
                    } else {
                        bounds.bottom()
                    };
                    let top = rem * (COLLAPSED_ITEM * ix as f32);
                    paint_dots(
                        bounds.center().x,
                        from,
                        to,
                        top + (from - bounds.top()),
                        window,
                        cx,
                    );
                },
            )
            .absolute()
            .size_full(),
        )
        .child(tile)
}

/// A section label: pip on the rail, title, new messages and the chat count.
/// A compact label drops the new messages and tightens its gaps so the title
/// and count fit a narrow list.
pub(super) fn section_label(
    group: Group,
    count: usize,
    current: bool,
    compact: bool,
    placement: LabelPlacement,
    on_jump: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let live = group == Group::Current;
    let pip_color = if live {
        theme.primary
    } else {
        theme.foreground
    };
    let id = match placement {
        LabelPlacement::Inline(_) => "inline",
        LabelPlacement::Top { .. } => "top",
        LabelPlacement::Bottom { .. } => "bottom",
    };
    div()
        .w_full()
        .flex_shrink_0()
        .px_2()
        .bg(theme.sidebar)
        .child(
            row()
                .id(SharedString::from(format!(
                    "chat-section:{id}:{}",
                    group.title()
                )))
                .role(Role::Button)
                .aria_label(format!("Go to {}, {}", group.title(), chat_count(count)))
                .relative()
                .h(rems(SECTION_HEIGHT))
                .px_2()
                .when(compact, |label| label.gap_1p5())
                .when(!compact, |label| label.gap_3())
                .text_xs()
                .text_color(if current {
                    theme.sidebar_primary_foreground
                } else {
                    theme.muted_foreground
                })
                .border_color(theme.border)
                .hover(|style| style.bg(theme.secondary))
                .on_click(on_jump)
                .map(|label| match placement {
                    // U2: an edge only where a pinned stack meets the chats.
                    LabelPlacement::Inline(rail) => label.child(rail_canvas(rail)),
                    LabelPlacement::Top { last } => label.when(last, |label| label.border_b_1()),
                    LabelPlacement::Bottom { first } => {
                        label.when(first, |label| label.border_t_1())
                    }
                })
                .child(
                    row().w(rems(LANE)).flex_shrink_0().justify_center().child(
                        div()
                            .size(rems(0.625))
                            .rounded_full()
                            .border_2()
                            .map(|pip| {
                                if current {
                                    pip.bg(pip_color).border_color(pip_color)
                                } else {
                                    pip.bg(theme.sidebar).border_color(if live {
                                        theme.primary
                                    } else {
                                        theme.input
                                    })
                                }
                            }),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(group.title()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .when(!compact, |count| count.pr_1())
                        .child(count.to_string()),
                ),
        )
        .into_any_element()
}

fn chat_count(count: usize) -> String {
    format!("{count} {}", if count == 1 { "chat" } else { "chats" })
}

/// Closes the list: the rail ends on this pip, beside the chat count.
pub(super) fn end_label(count: usize, rail: Rail, cx: &App) -> AnyElement {
    div()
        .w_full()
        .px_2()
        .child(
            row()
                .relative()
                .h(rems(2.25))
                .px_2()
                .gap_3()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(rail_canvas(rail))
                .child(
                    row()
                        .w(rems(LANE))
                        .justify_center()
                        .child(div().size(rems(0.375)).rounded_full().bg(cx.theme().input)),
                )
                .child(chat_count(count)),
        )
        .into_any_element()
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
                let segment = ArcData {
                    data: &(),
                    index,
                    value: 1.,
                    start_angle: from,
                    end_angle: to,
                    pad_angle: 0.,
                };
                shape.paint(&segment, color, None, None, &bounds, window);
            }
        },
    )
    .size_full()
}

/// Needs input, running, done or idle, in a circle the size of the agent tile.
fn status_badge(thread: &Thread, selected: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    // The selected row's fill would swallow the circle; the list color keeps it distinct.
    let inside = if selected {
        theme.sidebar
    } else {
        theme.transparent
    };
    let circle = row()
        .relative()
        .flex_shrink_0()
        .size(rems(LANE))
        .justify_center()
        .rounded_full();
    match thread.status.as_str() {
        "blocked" => circle
            .bg(theme.primary)
            .child(
                icon("flag")
                    .size(rems(0.9375))
                    .text_color(theme.primary_foreground),
            )
            .into_any_element(),
        "working" => {
            let (track, arc) = (theme.border, theme.foreground);
            circle.bg(inside).child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .with_animation(
                        "chat-running",
                        Animation::new(std::time::Duration::from_millis(RUNNING_TURN_MS))
                            .repeat_synced(),
                        move |ring, turn| ring.child(running_ring(turn, track, arc)),
                    ),
            )
        }
        .child(div().size(rems(0.5)).rounded_full().bg(theme.foreground))
        .into_any_element(),
        "completed" | "archived" => circle
            .bg(inside)
            .border_1()
            .border_color(theme.border)
            .child(
                icon("check")
                    .size(rems(0.9375))
                    .text_color(theme.muted_foreground),
            )
            .into_any_element(),
        _ => circle
            .bg(inside)
            .border_1()
            .border_color(theme.border)
            .into_any_element(),
    }
}

/// A command square on the collapsed list, the size of an agent tile.
pub(super) fn collapsed_button(id: &'static str, label: &'static str, icon_name: &str) -> Button {
    Button::new(id)
        .outline()
        .size(rems(LANE))
        .p_0()
        .accessibility_label(label)
        .tooltip(label)
        .child(icon(icon_name).size(rems(1.)))
}

/// The chat's status in words, for accessible labels.
fn status_name(thread: &Thread) -> &'static str {
    match thread.status.as_str() {
        "working" => "running",
        "completed" => "completed",
        "blocked" => "needs input",
        "archived" => "archived",
        _ => "idle",
    }
}

/// The agent tile shared by list rows and the collapsed list.
fn agent_tile(agent: &str, fill: Hsla, cx: &App) -> Div {
    let (asset, color) = agent_icon(agent, cx);
    row()
        .flex_shrink_0()
        .size(rems(LANE))
        .justify_center()
        .rounded(cx.theme().radius)
        .bg(fill)
        .child(icon(asset).size(rems(1.0625)).text_color(color))
}

impl Adeline {
    fn agent_name(&self, agent: &str) -> String {
        if self.demo_mode {
            provider(agent).to_owned()
        } else {
            agent.to_owned()
        }
    }

    pub(super) fn chat_card(&self, i: usize, rail: Rail, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[i];
        let theme = cx.theme();
        let selected = self.selected == Some(i);
        let unread = thread.unread();
        let status = status_name(thread);
        let agent = self.agent_name(&thread.provider);
        let activity = thread.last_activity();
        let updated = (activity > 0).then(|| recency::label(activity, recency::now()));
        let messages = thread.messages.len();
        let query = self.query(cx);
        let mut description = format!("{}, {agent}, {messages} messages", thread.title.trim());
        if let Some(updated) = &updated {
            write!(description, ", updated {updated}").unwrap();
        }
        write!(description, ", {status}").unwrap();
        if unread {
            description.push_str(", unread");
        }
        div()
            .w_full()
            .px_2()
            .child(
                row()
                    .id(SharedString::from(format!(
                        "chat:{}:{}",
                        self.workspace().config.id,
                        thread.id
                    )))
                    .group("chat-row")
                    .role(Role::ListBoxOption)
                    .aria_selected(selected)
                    .aria_label(description)
                    .relative()
                    .w_full()
                    .h(rems(ROW_HEIGHT))
                    .px_2()
                    .gap_3()
                    .rounded(theme.radius_lg)
                    .when(selected, |row| row.bg(theme.sidebar_primary))
                    .when(!selected, |row| {
                        row.hover(|style| style.bg(theme.secondary))
                    })
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.act(Action::Chat(i), window, cx)),
                    )
                    .child(rail_canvas(rail))
                    .child(agent_tile(
                        &thread.provider,
                        if selected {
                            theme.sidebar
                        } else {
                            theme.secondary
                        },
                        cx,
                    ))
                    .child(
                        col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .line_height(rems(1.25))
                                    .when(unread, |title| {
                                        title
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.sidebar_primary_foreground)
                                    })
                                    .when(!unread, |title| title.text_color(theme.foreground))
                                    .child(highlighted_title(thread.title.trim(), &query, cx)),
                            )
                            .child(
                                // The time wraps onto a clipped second line when it
                                // would run into the status circle.
                                row()
                                    .flex_wrap()
                                    .h(rems(1.))
                                    .overflow_hidden()
                                    .gap_3()
                                    .text_xs()
                                    .line_height(rems(1.))
                                    .whitespace_nowrap()
                                    .text_color(theme.muted_foreground)
                                    .child(
                                        row()
                                            .flex_shrink_0()
                                            .gap_1()
                                            .when(unread, |count| count.text_color(theme.primary))
                                            .child(
                                                icon(if unread { "chat-fill" } else { "chat" })
                                                    .size(rems(0.75)),
                                            )
                                            .child(messages.to_string()),
                                    )
                                    .when_some(updated, |meta, updated| {
                                        meta.child(
                                            row()
                                                .flex_shrink_0()
                                                .gap_1()
                                                .child(icon("clock").size(rems(0.75)))
                                                .child(updated),
                                        )
                                    }),
                            ),
                    )
                    .child(status_badge(thread, selected, cx)),
            )
            .into_any_element()
    }

    /// The collapsed list's search: a tile the size of an agent tile that
    /// unfurls, like a chat, into the search field and the number of matches.
    /// It stays unfurled while the field has focus.
    pub(super) fn collapsed_search(
        &self,
        open: bool,
        matches: Option<usize>,
        list: WeakEntity<chat::ChatList>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let key: Arc<str> = Arc::from(chat::COLLAPSED_SEARCH);
        let search_icon = |color: Hsla| icon("search").size(rems(1.)).text_color(color);
        let trigger = Button::new("chat-rail-search")
            .outline()
            .size(rems(LANE))
            .p_0()
            .accessibility_label("Search chats")
            .child(search_icon(if matches.is_some() {
                theme.primary
            } else {
                theme.foreground
            }))
            .on_click(cx.listener(|app, _, window, cx| app.focus_chat_search(window, cx)));
        // Same geometry as a chat's flyout: it covers the tile exactly.
        let inset = rems(0.25);
        let flyout = row()
            .id("chat-rail-search-flyout")
            .absolute()
            .top(-inset)
            .left(-inset)
            .w(rems(20.))
            .p(inset)
            .pr_2()
            .gap_2()
            .popover_style(cx)
            .rounded(theme.radius_lg)
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
            .child(
                row()
                    .flex_shrink_0()
                    .size(rems(LANE))
                    .justify_center()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.primary)
                    .child(search_icon(theme.primary)),
            )
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
            .size(rems(LANE))
            .on_hover(move |hovered, _, cx| {
                if *hovered {
                    let _ = list.update(cx, |list, cx| list.hover_collapsed(&key, true, cx));
                }
            })
            .child(trigger)
            .when(open, |slot| slot.child(deferred(flyout).with_priority(1)))
            .into_any_element()
    }

    /// A chat in the collapsed list: its agent tile. While the pointer is on it,
    /// or keyboard focus, the tile unfurls toward the chats into a flyout with
    /// the title, time and status; the rail itself keeps its width.
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
        let unread = thread.unread();
        let key: Arc<str> = Arc::from(thread.id.as_str());
        let id = format!("{}:{}", self.workspace().config.id, thread.id);
        let activity = thread.last_activity();
        let updated = (activity > 0).then(|| recency::label(activity, recency::now()));
        let mut description = format!(
            "{}, {}",
            thread.title.trim(),
            self.agent_name(&thread.provider)
        );
        if let Some(updated) = &updated {
            write!(description, ", updated {updated}").unwrap();
        }
        write!(description, ", {}", status_name(thread)).unwrap();
        if unread {
            description.push_str(", unread");
        }
        let fill = if selected {
            theme.sidebar_primary
        } else {
            theme.secondary
        };
        // Without a row fill around it, the open chat's tile also carries an outline.
        let outlined = |tile: Div| {
            tile.when(selected, |tile| {
                tile.border_1().border_color(theme.muted_foreground)
            })
        };
        let open = move |app: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            app.act(Action::Chat(i), window, cx);
        };
        let tile = outlined(agent_tile(&thread.provider, fill, cx))
            .id(SharedString::from(format!("chat-rail:{id}")))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .aria_label(description)
            .track_focus(focus)
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
            }));
        // The flyout covers the tile exactly and draws above the chats beside the rail.
        let inset = rems(0.25);
        let flyout = row()
            .id(SharedString::from(format!("chat-rail-flyout:{id}")))
            .absolute()
            .top(-inset)
            .left(-inset)
            .w(rems(20.))
            .p(inset)
            .pr_2()
            .gap_3()
            .popover_style(cx)
            .rounded(theme.radius_lg)
            .on_hover(move |hovered, _, cx| {
                if !*hovered {
                    let _ = list.update(cx, |list, cx| list.hover_collapsed(&key, false, cx));
                }
            })
            .on_click(cx.listener(move |app, _, window, cx| open(app, window, cx)))
            .child(
                outlined(agent_tile(&thread.provider, fill, cx)).when(keyboard_focus, |tile| {
                    tile.border_2().border_color(theme.ring)
                }),
            )
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .line_height(rems(1.25))
                            .when(unread, |title| title.font_weight(FontWeight::SEMIBOLD))
                            .child(thread.title.trim().to_owned()),
                    )
                    .when_some(updated, |text, updated| {
                        text.child(
                            row()
                                .gap_1()
                                .text_xs()
                                .line_height(rems(1.))
                                .text_color(theme.muted_foreground)
                                .child(icon("clock").size(rems(0.75)))
                                .child(updated),
                        )
                    }),
            )
            .child(status_badge(thread, false, cx));
        div()
            .relative()
            .flex_shrink_0()
            .size(rems(LANE))
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
            let (asset, color) = agent_icon(agent, cx);
            let name = self.agent_name(agent);
            return button("chat-agent-filter-clear")
                .bg(theme.sidebar_primary)
                .border_1()
                .border_color(theme.input)
                .accessibility_label(format!("Showing {name} only. Show all agents"))
                .tooltip(format!("Showing {name} only. Click to show all agents"))
                .child(icon(asset).size(rems(1.)).text_color(color))
                .on_click(cx.listener(|app, _, window, cx| {
                    app.act(Action::AgentFilter(None), window, cx);
                }))
                .into_any_element();
        }
        let owner = cx.weak_entity();
        let agents: Arc<[(Arc<str>, String, usize)]> = outcome
            .agents
            .iter()
            .map(|(agent, count)| (agent.clone(), self.agent_name(agent), *count))
            .collect();
        let total: usize = agents.iter().map(|(_, _, count)| count).sum();
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
                for (agent, name, count) in agents.iter() {
                    let owner = owner.clone();
                    let agent = agent.clone();
                    let asset = agent_icon_asset(&agent);
                    menu = menu.item(
                        PopupMenuItem::element(counted(name.clone(), *count))
                            .icon(Icon::default().path(format!("{asset}.svg")))
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
        tab_cap: Option<Pixels>,
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let searching = search_focused || !self.query.read(cx).value().is_empty();
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
        let owner = cx.weak_entity();
        let selected = TAB_FILTERS
            .iter()
            .position(|&filter| filter == self.filter)
            .unwrap_or(0);
        let tabs = TabBar::new("chat-scope")
            .segmented()
            .w_full()
            .selected_index(selected)
            .when_some(tab_cap, |tabs, cap| tabs.max_width(cap))
            .children(
                [("All", "All"), ("Attention", "At"), ("Unread", "Un")]
                    .into_iter()
                    .zip(outcome.scopes)
                    .enumerate()
                    .map(|(ix, ((label, short), count))| {
                        let active = ix == selected;
                        Tab::new()
                            .flex_1()
                            .aria_label(format!("{label}, {}", chat_count(count)))
                            .child(
                                row()
                                    .gap_1()
                                    .text_xs()
                                    .whitespace_nowrap()
                                    .child(
                                        div()
                                            .when(active, |label| {
                                                label
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme.sidebar_primary_foreground)
                                            })
                                            .when(!active, |label| {
                                                label.text_color(theme.muted_foreground)
                                            })
                                            .child(if tab_cap.is_some() { short } else { label }),
                                    )
                                    .child(
                                        div()
                                            .text_color(theme.muted_foreground)
                                            .child(count.to_string()),
                                    ),
                            )
                    }),
            )
            .on_click(move |ix, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    app.act(Action::Filter(TAB_FILTERS[*ix]), window, cx);
                });
            });
        col()
            .w_full()
            .min_w_0()
            .h_full()
            .child(self.mode_sidebar_header(search, cx))
            .child(div().w_full().px_3().pb_2().child(tabs))
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

    pub(super) fn message_row(&self, index: usize, i: usize, _cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[index];
        let message = &thread.messages[i];
        let user = message.role == "user";
        let time = if message.created_at.len() > 15 {
            format!(
                "{}:{} PM",
                message.created_at[11..13].parse::<u32>().unwrap_or(14) + 2 - 12,
                &message.created_at[14..16]
            )
        } else {
            "Now".into()
        };
        let avatar = if user {
            icon("user").size(rems(1.75)).into_any_element()
        } else if thread.provider == "claude" {
            icon("claude").size(rems(1.75)).into_any_element()
        } else {
            icon("codex").size(rems(1.75)).into_any_element()
        };
        let message_id = format!(
            "chat-message:{}:{}:{i}",
            self.workspace().config.id,
            thread.id
        );
        let mut bubble = Bubble::new()
            .with_variant(if user {
                BubbleVariant::Secondary
            } else {
                BubbleVariant::Ghost
            })
            .min_w_0();
        if user || self.demo_mode {
            for paragraph in message.text.split("\n\n") {
                bubble = bubble.child(div().min_w_0().text_sm().child(paragraph.to_owned()));
            }
        } else {
            bubble = bubble.child(
                TextView::markdown("response", message.text.clone())
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
            bubble = bubble.child(
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
        div()
            .id(SharedString::from(message_id))
            .w_full()
            .min_w_0()
            .pb_6()
            .px_5()
            .child(
                Message::new()
                    .alignment(if user {
                        MessageAlignment::End
                    } else {
                        MessageAlignment::Start
                    })
                    .avatar(avatar)
                    .header(
                        MessageHeader::new()
                            .child(if user {
                                "You".to_owned()
                            } else if self.demo_mode {
                                provider(&thread.provider).to_owned()
                            } else {
                                thread.provider.clone()
                            })
                            .child(time),
                    )
                    .content(MessageContent::new().bubble(bubble)),
            )
            .into_any_element()
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
    pub(super) fn composer_view(&self, cx: &Context<Self>) -> Div {
        let bound = self.bound_definition();
        let selected = self.selected_definition();
        let agent_name: SharedString = bound
            .as_ref()
            .map(|definition| definition.name.as_str())
            .or_else(|| selected.map(|definition| definition.name.as_str()))
            .unwrap_or("Select an agent")
            .to_owned()
            .into();
        let agent_details = bound
            .as_ref()
            .map(|definition| format!("{} {}", definition.model, definition.effort))
            .or_else(|| {
                selected.map(|definition| format!("{} {}", definition.model, definition.effort))
            });
        let processing = self.conversation_processing();
        let can_send = self
            .composer
            .read(cx)
            .text()
            .chars()
            .any(|character| !character.is_whitespace());
        col()
            .w_full()
            .flex_shrink_0()
            .min_w_0()
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div().w_full().px_3().py_2().child(
                    Textarea::new(&self.composer)
                        .aria_label("Message")
                        .w_full()
                        .min_w_0(),
                ),
            )
            .child(
                row()
                    .w_full()
                    .min_w_0()
                    .flex_wrap()
                    .items_start()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("attach-chat-files")
                            .ghost()
                            .small()
                            .accessibility_label("Attach files…")
                            .tooltip("Attach files…")
                            .child(icon("file").size(rems(1.)))
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.act(Action::InsertFiles, window, cx);
                            })),
                    )
                    .child(
                        col()
                            .id("chat-agent-selection")
                            .role(Role::Group)
                            .aria_label("Agent selection")
                            .flex_1()
                            .min_w(rems(4.))
                            .child(
                                self.command_popover(
                                    "agent",
                                    Button::new("chat-agent-picker")
                                        .ghost()
                                        .small()
                                        .w_full()
                                        .label(agent_name.clone())
                                        .tooltip(agent_name)
                                        .dropdown_caret(true),
                                    Anchor::BottomLeft,
                                    cx,
                                ),
                            )
                            .when_some(agent_details, |panel, details| {
                                panel.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(details),
                                )
                            }),
                    )
                    .child(
                        Button::new("send-chat-message")
                            .small()
                            .ml_auto()
                            .label(if processing { "Stop" } else { "Send" })
                            .when(processing, |button| button.danger())
                            .when(!processing, |button| button.primary().disabled(!can_send))
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.act(
                                    if processing {
                                        Action::Stop
                                    } else {
                                        Action::Send
                                    },
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
    }
}
