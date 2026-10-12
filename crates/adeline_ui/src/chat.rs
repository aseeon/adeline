//! Chats: the chat list and sidebar, the transcript and the composer, each an
//! independently invalidated view, plus the parts other surfaces draw with.
//! The shell remains the shared data/action coordinator.
//!
//! Sizes are in rems. Chat rows and section labels (which include the gap above
//! them) have fixed heights, so the list places its stacked labels by arithmetic.
//! A label sticks once its natural place reaches its slot, like CSS sticky with a
//! top of slot times the label height; the current section is the last to stick.
//! `COLLAPSED_SEARCH` is NUL-prefixed because chat ids never are. `Transcript`'s
//! `follow` and `reveal` leave redrawing the panel to the caller. The collapsed
//! list is as wide as the mode rail beside it.
//!
//! Looping animations run at `ANIMATION_FPS`, and `spinner` is the Kit's
//! `Spinner` with that cap: no view is cached yet, so every animation frame
//! redraws the whole window (`docs/handoff-view-caching.md`).
use super::*;
use adeline_core::conversation::{MessageRole, ThreadStatus};
use adeline_core::prepared::{Criteria, Group, Outcome};
use gpui_kit::base::SelectableText;
use gpui_kit::component::plot::shape::{Arc as ArcShape, ArcData};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Selectable as _, Side, Sizable as _,
    button::ButtonVariants as _,
    command::{Command, CommandGroup, CommandItem},
    input::Textarea,
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::Scrollbar,
    text::{TextView, TextViewState, TextViewStyle},
    tooltip::Tooltip,
};
use std::f32::consts::TAU;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

mod composer;
mod list;
mod transcript;
pub(super) use composer::Composer;
pub(super) use list::ChatList;
pub(super) use transcript::Transcript;

pub(super) fn size_probe(
    on_size: impl FnOnce(Size<Pixels>, &mut App) + 'static,
) -> impl IntoElement {
    canvas(
        move |bounds, _, cx| on_size(bounds.size, cx),
        |_, (), _, _| {},
    )
    .absolute()
    .size_full()
}

pub(super) fn region<V: Render>(view: &Entity<V>, window: &Window) -> AnyElement {
    if window.is_a11y_active() {
        view.clone().into_any_element()
    } else {
        view.clone()
            .cached(StyleRefinement::default().size_full())
            .into_any_element()
    }
}

fn invalidate_range(state: &ListState, range: std::ops::Range<usize>) {
    let top = state.logical_scroll_top();
    state.splice(range.clone(), range.len());
    state.scroll_to(top);
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
    pub(super) fn sync_regions(&self, action: &Action, cx: &mut Context<Self>) {
        use super::Action::*;
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
            Section(config::Section::Chats)
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

pub(super) fn agent_mark((path, color): (String, Hsla)) -> themed_icon::ThemedIcon {
    themed_icon::ThemedIcon::path(&path)
        .flex_shrink_0()
        .size(rems(0.8125))
        .text_color(color)
}

impl Adeline {
    pub(super) fn agent_icon(&self, agent: &str, cx: &App) -> (String, Hsla) {
        let path = self
            .agent_catalog()
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

    pub(super) fn agent_name(&self, agent: &str) -> String {
        if self.demo_mode {
            provider(agent).to_owned()
        } else {
            agent.to_owned()
        }
    }
}

pub(super) fn option_menu_name(category: conversation::Category) -> &'static str {
    match category {
        conversation::Category::Model => "model",
        conversation::Category::Effort => "effort",
        conversation::Category::Mode => "mode",
        conversation::Category::Other => "more",
    }
}

pub(super) const CHAT_HEADER_HEIGHT: f32 = 3.;
pub(super) const HEADER_FADE: f32 = 1.75;

pub(super) const CHAT_COLUMN: f32 = 46.;

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

pub(super) const ANIMATION_FPS: f32 = 30.;

pub(super) fn spinner() -> AnyElement {
    Icon::new(IconName::Loader)
        .xsmall()
        .with_animation(
            "spinner",
            Animation::new(Duration::from_millis(800))
                .repeat()
                .with_easing(ease_in_out)
                .with_max_fps(ANIMATION_FPS),
            |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
}

const BRAILLE_FRAMES: [u8; 10] = [0x0B, 0x19, 0x39, 0x38, 0x3C, 0x34, 0x26, 0x27, 0x07, 0x0F];
const THINKING_WORDS: [&str; 4] = ["Thinking…", "Reasoning…", "Pondering…", "Working it out…"];

pub(super) fn thinking_label(progress: Option<String>, cx: &App) -> AnyElement {
    let muted: Hsla = rgb(theme::muted_foreground()).into();
    let accent = cx.theme().primary;
    let size = fonts::text_pixels(13.);
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
                            Animation::new(Duration::from_secs(8))
                                .repeat()
                                .with_max_fps(ANIMATION_FPS),
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
                .child(
                    row().gap(dot).ml(dot).with_animation(
                        "thinking-dots",
                        Animation::new(Duration::from_millis(1200))
                            .repeat()
                            .with_max_fps(ANIMATION_FPS),
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
                    ),
                )
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
                            // Bits 0-2 are the left column top to bottom, 3-5
                            // the right. The bundled fonts have no braille glyphs.
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
        // Accent paints into the text from the left, then starts over. A
        // label in Words mode gets this too, so it still shows work going on.
        config::ThinkingAnimation::Fill | config::ThinkingAnimation::Words => {
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
                                .with_easing(ease_in_out)
                                .with_max_fps(ANIMATION_FPS),
                            |fill, delta| fill.w(relative(delta)),
                        ),
                )
                .into_any_element()
        }
    }
}

pub(super) fn chat_column() -> Div {
    div().w_full().max_w(rems(CHAT_COLUMN)).mx_auto().px_8()
}

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

pub(super) fn tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.).replace(".0M", "M")
    } else if count >= 1_000 {
        format!("{}k", (count + 500) / 1_000)
    } else {
        count.to_string()
    }
}

pub(super) fn count(number: usize, noun: &str) -> String {
    format!("{number} {noun}{}", if number == 1 { "" } else { "s" })
}

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

pub(super) fn tool_icon(tool: conversation::ToolKind) -> &'static str {
    use conversation::ToolKind;
    match tool {
        ToolKind::Read => "file",
        ToolKind::Edit | ToolKind::Delete | ToolKind::Move => "edit",
        ToolKind::Execute => "code",
        ToolKind::Search => "search",
        ToolKind::Fetch => "link",
        ToolKind::Think => "sparkle",
        _ => "wrench",
    }
}

fn step_title(call: &Activity) -> String {
    call.title
        .strip_suffix(" (completed)")
        .unwrap_or(&call.title)
        .to_owned()
}

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
                    spinner()
                } else {
                    icon(tool_icon(call.tool))
                        .size(rems(0.75))
                        .into_any_element()
                })
                .child(div().min_w_0().truncate().child(step_title(call)))
        }))
}
