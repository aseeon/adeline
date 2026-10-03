//! The agent activity panel: what the open chat's agent did, a line per step,
//! with the chat's numbers in an island level with the composer.
use super::*;
use crate::chat_render::{CHAT_HEADER_HEIGHT, agent_mark, message_time, tokens, tool_icon};
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    scroll::Scrollbar,
    spinner::Spinner,
    tooltip::Tooltip,
};
use std::cell::RefCell;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ActivityPanel {
    /// Rows opened to show their details.
    expanded: HashSet<String>,
    /// The transcript message under the hovered row, lit in the chat.
    pub(super) lit: Option<usize>,
    /// The row that lit it. Moving between rows enters the next one before
    /// leaving the last, so only this row's leaving puts the light out.
    hovered: Option<String>,
    scroll: ScrollHandle,
    /// The chat, row count and following state the list last scrolled for.
    synced: RefCell<(String, usize, bool)>,
}

impl ActivityPanel {
    /// Forgets the open chat's rows when another chat opens.
    pub(super) fn reset(&mut self) {
        self.expanded.clear();
        self.lit = None;
        self.hovered = None;
    }
}

/// One line of the panel, in the order things happened.
#[derive(Debug, PartialEq)]
enum Entry<'a> {
    /// A turn starts at its prompt, the message at this index.
    Turn {
        number: usize,
        message: usize,
    },
    Prompt(usize),
    /// Reply text written between tool calls.
    Reply {
        message: usize,
        text: &'a str,
    },
    /// A tool call or an error, by its index in the chat's activity.
    Call(usize),
}

fn push_reply<'a>(
    entries: &mut Vec<Entry<'a>>,
    thread: &'a Thread,
    message: usize,
    from: usize,
    to: usize,
) {
    let text = &thread.messages[message].text;
    if let Some(part) = text
        .get(from.min(text.len())..to.min(text.len()))
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        entries.push(Entry::Reply {
            message,
            text: part,
        });
    }
}

/// The chat as the panel lists it: each turn's prompt, then its reply split
/// around the tool calls made while it was written.
fn timeline(thread: &Thread) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    let prompts: Vec<usize> = (0..thread.messages.len())
        .filter(|&i| thread.messages[i].role == "user")
        .collect();
    let first = prompts.first().copied().unwrap_or(thread.messages.len());
    for message in (0..first).filter(|&i| thread.messages[i].role == "assistant") {
        push_reply(&mut entries, thread, message, 0, usize::MAX);
    }
    for (n, &prompt) in prompts.iter().enumerate() {
        let end = prompts.get(n + 1).copied().unwrap_or(thread.messages.len());
        entries.push(Entry::Turn {
            number: n + 1,
            message: prompt,
        });
        entries.push(Entry::Prompt(prompt));
        let mut replies = (prompt + 1..end).filter(|&i| thread.messages[i].role == "assistant");
        let reply = replies.next();
        let mut written = 0;
        for (ix, call) in thread.activity.iter().enumerate() {
            if call.turn != Some(prompt) {
                continue;
            }
            if let Some(reply) = reply
                && call.kind.starts_with("tool:")
            {
                push_reply(&mut entries, thread, reply, written, call.after_text);
                written = written.max(call.after_text);
            }
            entries.push(Entry::Call(ix));
        }
        if let Some(reply) = reply {
            push_reply(&mut entries, thread, reply, written, usize::MAX);
        }
        for reply in replies {
            push_reply(&mut entries, thread, reply, 0, usize::MAX);
        }
    }
    // Calls from before turns were recorded go last, where the running work is.
    entries.extend(
        thread
            .activity
            .iter()
            .enumerate()
            .filter(|(_, call)| call.turn.is_none())
            .map(|(ix, _)| Entry::Call(ix)),
    );
    entries
}

/// `0.4s`, `41.2s`, `2m 04s`, `1h 05m`.
fn duration(ms: u64) -> String {
    if ms < 100 {
        "<0.1s".into()
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.)
    } else if ms < 3_600_000 {
        format!("{}m {:02}s", ms / 60_000, ms / 1000 % 60)
    } else {
        format!("{}h {:02}m", ms / 3_600_000, ms / 60_000 % 60)
    }
}

/// Local time to the second: `2:43:05 PM`.
fn clock(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%-I:%M:%S %p")
                .to_string()
        })
        .unwrap_or_default()
}

fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
}

/// At most `lines` lines and `chars` characters of `text`, marked when cut.
fn excerpt(text: &str, lines: usize, chars: usize) -> String {
    let kept: Vec<&str> = text.trim().lines().take(lines).collect();
    let mut kept = kept.join("\n");
    let mut cut = kept.len() < text.trim().len();
    if kept.chars().count() > chars {
        kept = kept.chars().take(chars).collect();
        cut = true;
    }
    if cut {
        kept.push('…');
    }
    kept
}

/// About four characters a token.
fn token_estimate(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// What became of a tool call: `Completed in 41.2s`, `Failed after 3.0s`.
fn outcome(call: &Activity) -> String {
    if call.kind == "error" {
        return "Error".into();
    }
    if call.running {
        return "Running".into();
    }
    let failed = call.status == "failed";
    match call.duration_ms() {
        Some(ms) if failed => format!("Failed after {}", duration(ms)),
        Some(ms) => format!("Completed in {}", duration(ms)),
        None if failed => "Failed".into(),
        None => "Completed".into(),
    }
}

/// What a row shows when hovered.
#[derive(Clone)]
struct Card {
    text: String,
    meta: String,
    preview: Option<String>,
}

/// What a row shows when opened.
struct Detail {
    facts: Vec<(&'static str, String)>,
    text: Option<String>,
    output: Option<String>,
    /// The message "Show in chat" scrolls to, and whether to open its turn's steps.
    message: Option<(usize, bool)>,
    copy: Option<(&'static str, String)>,
}

/// The round button over the chat and over the activity list that goes back
/// to the latest message. Both sit 0.5rem above the composer's top edge, which
/// the stats island shares, so they line up.
pub(super) fn jump_to_latest(id: &'static str, cx: &Context<Adeline>) -> impl IntoElement {
    let theme = cx.theme();
    // Clicks stop here rather than reaching the row beneath.
    div().occlude().child(
        Button::new(id)
            .icon(Icon::default().path("caret-double-down.svg"))
            .accessibility_label("Jump to latest")
            .tooltip("Jump to latest")
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .on_click(cx.listener(|app, _, _, cx| {
                app.transcript.update(cx, |view, cx| view.follow(cx));
                app.activity.scroll.scroll_to_bottom();
                app.activity.lit = None;
                cx.notify();
            })),
    )
}

impl Adeline {
    pub(super) fn activity_panel(&self, cx: &Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let header = row()
            .flex_none()
            .h(rems(CHAT_HEADER_HEIGHT))
            .px_3()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .child("Agent activity");
        let panel = col()
            .id("agent-activity-panel")
            .role(Role::Group)
            .aria_label("Agent activity panel")
            .relative()
            .size_full()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(header);
        let Some(thread) = self.selected.map(|ix| &self.workspace().threads[ix]) else {
            return panel.child(
                div()
                    .px_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Start a chat to see agent activity."),
            );
        };
        let entries = timeline(thread);
        let following = self.transcript.read(cx).following();
        // The list sticks to its end while the chat does: on new rows, another
        // chat, or the chat returning to its end.
        {
            let mut synced = self.activity.synced.borrow_mut();
            let current = (thread.id.clone(), entries.len(), following);
            if following && *synced != current {
                self.activity.scroll.scroll_to_bottom();
            }
            *synced = current;
        }
        let list = if entries.is_empty() {
            col().child(
                div()
                    .px_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No activity yet."),
            )
        } else {
            col()
                .w_full()
                .pl_1p5()
                // Room for the scrollbar, so it never covers a row.
                .pr(theme::SCROLLBAR_TRACK)
                .pb_2()
                .children(
                    entries
                        .iter()
                        .map(|entry| self.activity_row(thread, entry, cx)),
                )
        };
        // The island takes the composer's height and bottom inset, so the two line up.
        let composer = self.composer_height.get();
        let island_height = if composer > px(0.) {
            DefiniteLength::from(composer)
        } else {
            rems(6.5).into()
        };
        let jump = (!following && !entries.is_empty()).then(|| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(island_height)
                .pb_2()
                .flex()
                .justify_center()
                .child(jump_to_latest("activity-jump", cx))
        });
        panel
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("activity-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.activity.scroll)
                            .child(list),
                    )
                    .child(Scrollbar::vertical(&self.activity.scroll)),
            )
            .child(
                div()
                    .flex_none()
                    .h(island_height)
                    .px_2()
                    .pb_4()
                    .child(self.activity_stats(thread, cx)),
            )
            .children(jump)
    }

    /// Six numbers about the chat, in an island like the composer's.
    fn activity_stats(&self, thread: &Thread, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let calls = || {
            thread
                .activity
                .iter()
                .filter(|call| call.kind.starts_with("tool:"))
        };
        let timed: Vec<u64> = calls().filter_map(Activity::duration_ms).collect();
        let finished = |status: &str| calls().filter(|call| call.status == status).count();
        let failed = finished("failed");
        let active = thread.timing.active_ms();
        let dash = || "–".to_owned();
        let cells = [
            (
                "Active time",
                if active > 0 { duration(active) } else { dash() },
                "Time from each prompt to the agent's last reply or tool call",
            ),
            (
                "Tool time",
                if timed.is_empty() {
                    dash()
                } else {
                    duration(timed.iter().sum())
                },
                "Time the tool calls took, added up",
            ),
            (
                "Succeeded",
                finished("completed").to_string(),
                "Tool calls that completed",
            ),
            ("Failed", failed.to_string(), "Tool calls that failed"),
            (
                "Context",
                thread.context.map_or_else(dash, |(used, size)| {
                    format!("{} of {}", tokens(used), tokens(size))
                }),
                "Tokens in the agent's context, of its window",
            ),
            (
                "Reply speed",
                thread
                    .timing
                    .tokens_per_second()
                    .map_or_else(dash, |speed| format!("≈{speed:.0} tok/s")),
                "Estimated from how fast reply text arrives, at about four characters a token",
            ),
        ];
        let cell = |(label, value, help): (&'static str, String, &'static str)| {
            let alert = label == "Failed" && failed > 0;
            col()
                .id(label)
                .flex_1()
                .min_w_0()
                .tooltip(move |window, cx| Tooltip::new(help).build(window, cx))
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .text_color(if alert {
                            theme.danger
                        } else {
                            theme.foreground
                        })
                        .child(value),
                )
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
        };
        let mut cells = cells.into_iter().map(cell);
        col()
            .size_full()
            .justify_center()
            .gap_2()
            .px_3()
            .bg(theme.group_box)
            .border_1()
            .border_color(theme.border)
            .rounded(rems(0.875))
            .child(row().gap_2().children(cells.by_ref().take(3)))
            .child(row().gap_2().children(cells))
    }

    fn activity_row(&self, thread: &Thread, entry: &Entry<'_>, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let message_icon = |path: &str| icon(path).size(rems(0.75)).into_any_element();
        let (key, glyph, label, color, lit, card, detail) = match *entry {
            Entry::Turn { number, message } => {
                return row()
                    .h_7()
                    .pt_2()
                    .px_2()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.sidebar_foreground)
                            .child(format!("Turn {number}")),
                    )
                    .child(div().flex_1())
                    .children(message_time(&thread.messages[message].created_at))
                    .into_any_element();
            }
            Entry::Prompt(message) => {
                let text = &thread.messages[message].text;
                let sent = message_time(&thread.messages[message].created_at);
                (
                    format!("{}:prompt:{message}", thread.id),
                    message_icon("user"),
                    first_line(text).to_owned(),
                    theme.foreground,
                    message,
                    Card {
                        text: excerpt(text, 6, 280),
                        meta: sent
                            .clone()
                            .map_or_else(|| "You".into(), |at| format!("You, {at}")),
                        preview: None,
                    },
                    Detail {
                        facts: [
                            sent.map(|at| ("Sent", at)),
                            Some(("Length", format!("≈{} tokens", token_estimate(text)))),
                        ]
                        .into_iter()
                        .flatten()
                        .collect(),
                        text: Some(excerpt(text, 40, 2000)),
                        output: None,
                        message: Some((message, false)),
                        copy: Some(("Copy", text.clone())),
                    },
                )
            }
            Entry::Reply { message, text } => {
                let start =
                    text.as_ptr() as usize - thread.messages[message].text.as_ptr() as usize;
                (
                    format!("{}:reply:{message}:{start}", thread.id),
                    agent_mark(self.agent_icon(&thread.provider, cx))
                        .size(rems(0.75))
                        .into_any_element(),
                    first_line(text).to_owned(),
                    theme.sidebar_foreground,
                    message,
                    Card {
                        text: excerpt(text, 6, 280),
                        meta: format!(
                            "{}, ≈{} tokens",
                            if self.demo_mode {
                                provider(&thread.provider)
                            } else {
                                &thread.provider
                            },
                            token_estimate(text)
                        ),
                        preview: None,
                    },
                    Detail {
                        facts: vec![("Length", format!("≈{} tokens", token_estimate(text)))],
                        text: Some(excerpt(text, 40, 2000)),
                        output: None,
                        message: Some((message, false)),
                        copy: Some(("Copy", text.to_owned())),
                    },
                )
            }
            Entry::Call(ix) => {
                let call = &thread.activity[ix];
                let error = call.kind == "error";
                let failed = error || call.status == "failed";
                // A turn's steps show under its last reply once the turn ends.
                let closing = call.turn.and_then(|prompt| {
                    (prompt + 1..thread.messages.len())
                        .take_while(|&i| thread.messages[i].role != "user")
                        .last()
                        .filter(|&i| thread.ends_turn(i))
                });
                let glyph = if call.running {
                    Spinner::new().xsmall().into_any_element()
                } else {
                    icon(if error { "flag" } else { tool_icon(&call.tool) })
                        .size(rems(0.75))
                        .when(failed, |glyph| glyph.text_color(theme.danger))
                        .into_any_element()
                };
                let outcome = outcome(call);
                let mut facts = vec![("Status", outcome.clone())];
                if call.started > 0 {
                    facts.push(("Started", clock(call.started)));
                }
                if !call.paths.is_empty() {
                    facts.push(("Files", call.paths.join("\n")));
                }
                let output =
                    (!call.detail.trim().is_empty()).then(|| excerpt(&call.detail, 14, 1600));
                (
                    format!("{}:{ix}", thread.id),
                    glyph,
                    first_line(call.name()).to_owned(),
                    if call.running {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    },
                    closing.or(call.turn).unwrap_or(0),
                    Card {
                        text: excerpt(call.name(), 4, 280),
                        meta: if call.started > 0 && !error {
                            format!("{outcome}, started {}", clock(call.started))
                        } else {
                            outcome
                        },
                        preview: (!call.detail.trim().is_empty())
                            .then(|| excerpt(&call.detail, 3, 240)),
                    },
                    Detail {
                        facts,
                        text: error.then(|| call.title.clone()),
                        output,
                        message: closing
                            .map(|closing| (closing, true))
                            .or(call.turn.map(|prompt| (prompt, false))),
                        copy: (!call.detail.trim().is_empty())
                            .then(|| ("Copy output", call.detail.clone())),
                    },
                )
            }
        };
        let expanded = self.activity.expanded.contains(&key);
        let toggle = key.clone();
        let hover_key = key.clone();
        let mut button = Button::new(SharedString::from(format!("activity:{key}")))
            .ghost()
            .small()
            .w_full()
            .selected(expanded)
            .accessibility_label(label.clone())
            .child(
                row()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .text_color(color)
                    .child(
                        div()
                            .w_4()
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .child(glyph),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label)),
            )
            .on_click(cx.listener(move |app, _, _, cx| {
                if !app.activity.expanded.remove(&toggle) {
                    app.activity.expanded.insert(toggle.clone());
                }
                cx.notify();
            }))
            .on_hover(cx.listener(move |app, hovered: &bool, _, cx| {
                app.light_message(*hovered, &hover_key, lit, cx);
            }));
        let mono = theme.mono_font_family.clone();
        button.interactivity().tooltip(move |window, cx| {
            let card = card.clone();
            let mono = mono.clone();
            Tooltip::element(move |_, cx| {
                let theme = cx.theme();
                col()
                    .max_w(rems(20.))
                    .py_1()
                    .gap_1()
                    .whitespace_normal()
                    .child(
                        div()
                            .text_color(theme.popover_foreground)
                            .child(card.text.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(card.meta.clone()),
                    )
                    .when_some(card.preview.clone(), |card, preview| {
                        card.child(
                            div()
                                .font_family(mono.clone())
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(preview),
                        )
                    })
            })
            .build(window, cx)
        });
        col()
            .w_full()
            .child(button)
            .when(expanded, |column| {
                column.child(self.activity_detail(&key, detail, cx))
            })
            .into_any_element()
    }

    fn activity_detail(&self, key: &str, detail: Detail, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let fact = |(name, value): (&'static str, String)| {
            row()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .w(rems(3.5))
                        .flex_shrink_0()
                        .text_color(theme.muted_foreground)
                        .child(name),
                )
                .child(div().flex_1().min_w_0().child(value))
        };
        let show = detail.message.map(|(message, steps)| {
            Button::new(SharedString::from(format!("activity-show:{key}")))
                .outline()
                .xsmall()
                .label("Show in chat")
                .on_click(cx.listener(move |app, _, _, cx| app.show_in_chat(message, steps, cx)))
        });
        let copy = detail.copy.map(|(label, text)| {
            Button::new(SharedString::from(format!("activity-copy:{key}")))
                .ghost()
                .xsmall()
                .label(label)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                })
        });
        col()
            .ml(rems(1.75))
            .mr_1()
            .mt_0p5()
            .mb_2()
            .p_2()
            .gap_1()
            .rounded(theme.radius)
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .text_xs()
            .when_some(detail.text, |column, text| {
                column.child(
                    div()
                        .pb_1()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(text),
                )
            })
            .children(detail.facts.into_iter().map(fact))
            .when_some(detail.output, |column, output| {
                column.child(
                    div()
                        .mt_1()
                        .p_2()
                        .rounded(theme.radius)
                        .bg(theme.muted)
                        .font_family(theme.mono_font_family.clone())
                        .text_color(theme.muted_foreground)
                        .child(output),
                )
            })
            .child(row().mt_1().gap_1().children(show).children(copy))
    }

    /// Lights the transcript message under a hovered row, and puts it out again.
    fn light_message(&mut self, hovered: bool, row: &str, message: usize, cx: &mut Context<Self>) {
        let lit = if hovered {
            self.activity.hovered = Some(row.to_owned());
            Some(message)
        } else if self.activity.hovered.as_deref() == Some(row) {
            self.activity.hovered = None;
            None
        } else {
            return;
        };
        if lit != self.activity.lit {
            self.activity.lit = lit;
            self.transcript.update(cx, |_, cx| cx.notify());
        }
    }

    /// Scrolls the chat to a message; for a tool call, opens its turn's steps.
    fn show_in_chat(&mut self, message: usize, steps: bool, cx: &mut Context<Self>) {
        if steps && let Some(thread) = self.selected.map(|ix| &self.workspace().threads[ix]) {
            self.runtime
                .expanded_tools
                .insert(format!("summary:{}:{message}", thread.id));
            self.transcript
                .update(cx, |view, cx| view.sync(self, false, cx));
        }
        self.activity.lit = Some(message);
        self.transcript
            .update(cx, |view, cx| view.reveal(message, cx));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, duration, excerpt, timeline};
    use crate::data::{Activity, Message, Thread};

    fn message(role: &str, text: &str) -> Message {
        Message {
            role: role.into(),
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn replies_split_around_the_tool_calls_made_while_writing() {
        let mut thread = Thread {
            messages: vec![message("user", "Fix it")],
            ..Default::default()
        };
        thread.push_message(message("assistant", "Looking first."));
        thread.apply_tool("a", "Read a.rs", "completed", "", "read", &[], 10);
        thread.messages[1].text.push_str(" Found it.");
        thread.apply_tool("b", "Edit a.rs", "in_progress", "", "edit", &[], 20);
        thread.messages[1].text.push_str(" Done.");
        thread.activity.push(Activity {
            kind: "error".into(),
            title: "Lost".into(),
            ..Default::default()
        });
        let entries = timeline(&thread);
        assert_eq!(
            entries,
            [
                Entry::Turn {
                    number: 1,
                    message: 0
                },
                Entry::Prompt(0),
                Entry::Reply {
                    message: 1,
                    text: "Looking first."
                },
                Entry::Call(0),
                Entry::Reply {
                    message: 1,
                    text: "Found it."
                },
                Entry::Call(1),
                Entry::Reply {
                    message: 1,
                    text: "Done."
                },
                Entry::Call(2),
            ]
        );
        assert_eq!(thread.activity[0].duration_ms(), Some(0));
        assert!(thread.activity[1].running && thread.activity[1].finished == 0);
        assert_eq!(thread.activity[1].name(), "Edit a.rs");
    }

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(duration(40), "<0.1s");
        assert_eq!(duration(41_200), "41.2s");
        assert_eq!(duration(124_300), "2m 04s");
        assert_eq!(duration(3_900_000), "1h 05m");
        assert_eq!(excerpt("a\nb\nc", 2, 99), "a\nb…");
    }
}
