//! The transcript: messages and their actions (Reply, Copy, Retry, Fork), fork
//! markers, the closing row and decision rows.
//!
//! Each row is a cached `Row` view that keeps its last measured size (see
//! `chat.rs`).
//!
//! A reply keeps its own text view, which gets only the newly streamed tail
//! (`Thread::stream` only appends), so the Kit re-parses the last block, not the
//! whole reply. Its key includes `created_at`: a reply replaced at the same
//! index starts a new view.
use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

impl Adeline {
    pub(crate) fn resync_transcript(&self, cx: &mut Context<Self>) {
        self.transcript
            .update(cx, |view, cx| view.sync(self, false, cx));
    }

    pub(crate) fn welcome(&self) -> Stateful<Div> {
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

    pub(crate) fn message_row(
        &self,
        index: usize,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let thread = &self.workspace().threads[index];
        let message = &thread.messages[i];
        let reply = (message.role != MessageRole::User && !self.demo_mode)
            .then(|| reply_text(&thread.id, i, message, window, cx));
        let panel = if self.section == Section::Docs {
            panes::DOCS_CHAT
        } else {
            panes::CHATS_MAIN
        };
        let selectable = panes::selectable_in(panel, cx);
        let theme = cx.theme();
        let user = message.role == MessageRole::User;
        // A reply that so far only thought shows as the Thinking row instead.
        let thinking = config::with(|s| s.modes.chats.show_thinking);
        if !user && message.text.is_empty() && (message.thought.is_empty() || !thinking) {
            return div()
                .id(SharedString::from(format!(
                    "chat-message:{}:{i}",
                    thread.id
                )))
                .into_any_element();
        }
        let time = message_time(&message.created_at);
        let isl = theme::islands(cx);
        // Prose: 15 px, at 1.55 in a bubble and 1.65 in replies.
        let leading = relative(if user { 1.55 } else { 1.65 });
        let mut body = col()
            .w_full()
            .min_w_0()
            .gap(dp(10.))
            .text_size(dp(theme::T_PROSE))
            .line_height(leading);
        if user || self.demo_mode {
            for (n, paragraph) in message.text.split("\n\n").enumerate() {
                let quote = unquote(paragraph);
                let text = quote.clone().unwrap_or_else(|| paragraph.to_owned());
                body = body.child(
                    div()
                        .min_w_0()
                        .when(quote.is_some(), |div| {
                            div.border_l(px(3.))
                                .border_color(theme.muted_foreground.alpha(0.5))
                                .pl_3()
                                .text_color(theme.muted_foreground)
                        })
                        .map(|div| {
                            if selectable {
                                div.child(SelectableText::new(("paragraph", n), text))
                            } else {
                                div.child(text)
                            }
                        }),
                );
            }
        } else {
            // Compact text with inline code in the accent on the muted fill; the
            // default fill is nearly the background and hides code spans. Links
            // take `chart_2` through the theme's link color.
            body = body.child(
                TextView::new(&reply.expect("a reply outside demo mode"))
                    .selectable(selectable)
                    .style(
                        TextViewStyle::default()
                            .paragraph_gap(rems(0.625))
                            .inline_code(HighlightStyle {
                                color: Some(theme.primary),
                                background_color: Some(isl.chip),
                                ..Default::default()
                            }),
                    )
                    .code_block_actions(|block, _, _| {
                        let code = block.code();
                        div()
                            .invisible()
                            .group_hover("reply", |style| style.visible())
                            .child(
                                Button::new("copy-code")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::default().path("copy.svg"))
                                    .accessibility_label("Copy code")
                                    .tooltip("Copy")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            code.to_string(),
                                        ));
                                    }),
                            )
                    })
                    .text_size(dp(theme::T_PROSE))
                    .line_height(leading)
                    .w_full()
                    .min_w_0(),
            );
        }
        if !message.attachments.is_empty() {
            body = body.child(self.attachment_chips(&message.attachments, Some(i), cx));
        }
        for path in &message.images {
            let asset = image_asset(path);
            // Reserve height before decoding so a virtual row keeps its scroll anchor.
            let bytes = embedded(asset).expect("bundled message image");
            let (width, height) = image_size(bytes);
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
        let copy = |id: String| {
            Button::new(SharedString::from(id))
                .ghost()
                .xsmall()
                .text_color(isl.fg3)
                .icon(Icon::default().path("copy.svg"))
                .accessibility_label("Copy")
                .tooltip("Copy")
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.act(Action::CopyMessage(i), window, cx);
                }))
        };
        let group = SharedString::from(format!("message:{}:{i}", thread.id));
        let content = if user {
            col()
                .group(group.clone())
                .items_end()
                .child(
                    div()
                        .max_w(relative(0.78))
                        .px(dp(16.))
                        .py(dp(10.))
                        .rounded(dp(14.))
                        .bg(isl.bubble)
                        .text_color(isl.fg)
                        .child(body),
                )
                .child(
                    row()
                        .h(dp(28.))
                        .gap(dp(4.))
                        .text_size(dp(theme::T_CAPTION))
                        .text_color(isl.fg3)
                        .child(
                            div()
                                .invisible()
                                .group_hover(group, |style| style.visible())
                                .child(copy(format!("copy-prompt:{}:{i}", thread.id))),
                        )
                        .children(time),
                )
                .into_any_element()
        } else {
            let name = self.agent_name(&thread.provider);
            // The reply being written gets its closing row when the turn ends.
            let writing = self.conversation_processing()
                && thread
                    .turn_of(thread.messages.len())
                    .is_some_and(|last| i > last);
            let meta = row()
                .h(dp(24.))
                .gap(dp(6.))
                .text_size(dp(theme::T_CAPTION))
                .text_color(isl.fg3)
                .child(agent_mark(self.agent_icon(&thread.provider, cx)).size(dp(13.)))
                .child(name)
                .children(time.map(|time| format!("· {time}")));
            col()
                .w_full()
                .min_w_0()
                .children(
                    self.thought_block(thread, i, cx)
                        .map(|thought| div().mb(dp(6.)).child(thought)),
                )
                .when(!message.text.is_empty(), |content| {
                    content.child(
                        div()
                            .group("reply")
                            .max_w(dp(640.))
                            .text_color(isl.fg)
                            .child(body),
                    )
                })
                .child(meta.mt(dp(4.)))
                .when(thread.ends_turn(i) && !writing, |content| {
                    content
                        .children(self.turn_cards(thread, i, cx))
                        .child(self.closing_row(thread, i, cx))
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
                    .pb(dp(if user { 16. } else { 28. }))
                    .when_some(thread.fork.as_ref().filter(|_| i == 0), |column, fork| {
                        column.child(self.fork_marker(fork, cx))
                    })
                    // Notes from the session's start, such as skipped MCP servers.
                    .when(i == 0, |column| {
                        column.children(thread.activity.iter().filter(|a| a.kind == "note").map(
                            |note| {
                                div()
                                    .pb_3()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(note.title.clone())
                            },
                        ))
                    })
                    .child(content),
            )
            .into_any_element()
    }

    fn fork_marker(&self, fork: &Fork, cx: &Context<Self>) -> impl IntoElement {
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
            .test_support()
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
                .text_color(theme.muted_foreground)
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

    pub(crate) fn decision_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
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
        } else if thread.status == ThreadStatus::Blocked {
            div().text_sm().child("Needs your input").into_any_element()
        } else {
            div().into_any_element()
        }
    }
}

pub(crate) struct Transcript {
    owner: WeakEntity<Adeline>,
    state: ListState,
    rows: Rc<RefCell<Vec<Entity<Row>>>>,
    width: Rc<Cell<Pixels>>,
    typography: fonts::Typography,
    thread: Option<(usize, String)>,
    selected: Option<usize>,
    messages: usize,
    footer_focus: FocusHandle,
    composer_height: Rc<Cell<Pixels>>,
    left_to_end: Pixels,
    following: bool,
}

impl Transcript {
    pub fn new(
        owner: WeakEntity<Adeline>,
        composer_height: Rc<Cell<Pixels>>,
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
                let _ = view.update(cx, |transcript, cx| {
                    transcript.resume_near_end(near);
                    transcript.note_following(cx);
                });
            });
        });
        Self {
            owner,
            state,
            rows: Rc::default(),
            width: Rc::default(),
            typography: fonts::typography(),
            thread: None,
            selected: None,
            messages: 0,
            footer_focus: cx.focus_handle(),
            composer_height,
            left_to_end: px(0.),
            following: true,
        }
    }

    pub fn following(&self) -> bool {
        self.state.is_following_tail()
    }

    pub fn follow(&mut self, cx: &mut Context<Self>) {
        self.state.set_follow_mode(FollowMode::Tail);
        self.following = true;
        cx.notify();
    }

    pub fn reveal(&mut self, message: usize, cx: &mut Context<Self>) {
        self.state.scroll_to(ListOffset {
            item_ix: message,
            offset_in_item: px(0.),
        });
        self.following = self.state.is_following_tail();
        cx.notify();
    }

    pub fn invalidate(&mut self, cx: &mut Context<Self>) {
        for row in self.rows.borrow().iter() {
            row.update(cx, |row, _| row.fresh = true);
        }
        cx.notify();
    }

    // Doc cards change under earlier turns, so every row redraws and remeasures.
    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.state.remeasure();
        self.invalidate(cx);
    }

    fn note_following(&mut self, cx: &mut Context<Self>) {
        let following = self.state.is_following_tail();
        if following != self.following {
            self.following = following;
            let _ = self.owner.update(cx, |_, cx| cx.notify());
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
                || t.status == ThreadStatus::Blocked
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
        let mut rows = self.rows.borrow_mut();
        if changed_thread {
            rows.clear();
        }
        rows.truncate(count);
        for row in rows.iter().skip(start) {
            row.update(cx, |row, _| row.fresh = true);
        }
        let transcript = cx.weak_entity();
        while rows.len() < count {
            let row = Row::new(transcript.clone(), rows.len());
            rows.push(cx.new(|_| row));
        }
        drop(rows);
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

/// A message bubble lit from above: a faint top-to-bottom gradient and a 1px
/// highlight along the top edge. Light themes light it with the background,
/// since their foreground is dark.
/// The text of a paragraph quoted with `>` on every line, without the markers.
fn unquote(paragraph: &str) -> Option<String> {
    let lines = paragraph
        .lines()
        .map(|line| {
            line.strip_prefix('>')
                .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(lines.join("\n"))
}

fn reply_text(
    thread: &str,
    i: usize,
    message: &Message,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TextViewState> {
    let key = SharedString::from(format!("reply:{thread}:{i}:{}", message.created_at));
    let reply = window.use_keyed_state(key, cx, |_, cx| Reply {
        fed: message.text.len(),
        view: cx.new(|cx| TextViewState::markdown(&message.text, cx)),
    });
    reply.update(cx, |reply, cx| {
        let text = message.text.as_str();
        match text.get(reply.fed..) {
            Some("") => {}
            Some(tail) => reply.view.update(cx, |view, cx| view.push_str(tail, cx)),
            None => reply.view.update(cx, |view, cx| view.set_text(text, cx)),
        }
        reply.fed = text.len();
        reply.view.clone()
    })
}

struct Reply {
    view: Entity<TextViewState>,
    fed: usize,
}

impl Render for Transcript {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = fonts::typography();
        if self.typography != typography {
            self.typography = typography;
            self.state.remeasure();
            for row in self.rows.borrow().iter() {
                row.update(cx, |row, _| row.fresh = true);
            }
        }
        // The header and composer float over the transcript so its scrollbar runs
        // the full height; the messages start and end clear of both.
        let gap = window.rem_size() * 0.5;
        let docs = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).section == Section::Docs);
        let top = if docs {
            window.rem_size() * CHAT_HEADER_HEIGHT + gap
        } else {
            gap
        };
        let bottom = self.composer_height.get() + gap;
        if self.selected.is_none() {
            return self
                .owner
                .update(cx, |app, _cx| app.welcome().size_full().into_any_element())
                .unwrap_or_else(|_| div().into_any_element());
        }
        let rows = Rc::clone(&self.rows);
        let width = Rc::clone(&self.width);
        let list = list(self.state.clone(), move |ix, window, cx| {
            let Some(row) = rows.borrow().get(ix).cloned() else {
                return div().into_any_element();
            };
            let (fresh, size) = {
                let row = row.read(cx);
                (row.fresh, row.size.get())
            };
            match size {
                Some(size)
                    if !fresh
                        && !window.is_a11y_active()
                        && (size.width - width.get()).abs() < px(0.5) =>
                {
                    row.cached(StyleRefinement::default().w_full().h(size.height))
                        .into_any_element()
                }
                _ => {
                    row.update(cx, |row, _| row.fresh = false);
                    row.into_any_element()
                }
            }
        })
        .size_full()
        .pt(top)
        .pb(bottom);
        let width = Rc::clone(&self.width);
        div()
            .relative()
            .size_full()
            .min_h_0()
            // Prepaints before the list, so its rows see this frame's width.
            .child(
                canvas(
                    move |bounds, _, _| width.set(bounds.size.width),
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(list)
            .child(Scrollbar::vertical(&self.state))
            .into_any_element()
    }
}

pub(crate) struct Row {
    transcript: WeakEntity<Transcript>,
    index: usize,
    size: Rc<Cell<Option<Size<Pixels>>>>,
    fresh: bool,
}

impl Row {
    fn new(transcript: WeakEntity<Transcript>, index: usize) -> Self {
        Self {
            transcript,
            index,
            size: Rc::default(),
            fresh: true,
        }
    }
}

impl Render for Row {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(transcript) = self.transcript.upgrade() else {
            return div().into_any_element();
        };
        let (owner, selected, messages, footer_focus) = {
            let transcript = transcript.read(cx);
            (
                transcript.owner.clone(),
                transcript.selected,
                transcript.messages,
                transcript.footer_focus.clone(),
            )
        };
        let Some(selected) = selected else {
            return div().into_any_element();
        };
        let row = self.index;
        let content = owner
            .update(cx, |app, cx| {
                if row < messages {
                    app.message_row(selected, row, window, cx)
                } else {
                    // Wrapped like message rows so the column centers in the row.
                    div()
                        .w_full()
                        .min_w_0()
                        .track_focus(&footer_focus)
                        .child(chat_column().flex().flex_col().child(if app.demo_mode {
                            col()
                                .child(app.decision_row(selected, cx))
                                .child(app.runtime_footer(selected, cx))
                                .into_any_element()
                        } else {
                            app.runtime_footer(selected, cx)
                        }))
                        .into_any_element()
                }
            })
            .unwrap_or_else(|_| div().into_any_element());
        let size = Rc::clone(&self.size);
        div()
            .relative()
            .w_full()
            .child(content)
            .child(size_probe(move |measured, _| size.set(Some(measured))))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::unquote;

    #[test]
    fn a_paragraph_quoted_on_every_line_unquotes() {
        assert_eq!(
            unquote("> Navigation\n>\n> Read it."),
            Some("Navigation\n\nRead it.".into())
        );
        assert_eq!(unquote("> quoted\nplain"), None);
        assert_eq!(unquote("plain"), None);
    }
}
