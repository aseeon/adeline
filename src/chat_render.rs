use super::*;
impl Adeline {
    pub(super) fn chat_card(&self, i: usize, cx: &Context<Self>) -> AnyElement {
        let t = &self.workspace().threads[i];
        let status = match t.status.as_str() {
            "working" => "Processing",
            "completed" => "Completed",
            "blocked" => "Attention",
            "archived" => "Archived",
            _ => "Active",
        };
        let card = col()
            .id(SharedString::from(t.id.clone()))
            .p(px(14.))
            .gap(px(10.))
            .rounded(px(6.))
            .cursor_pointer()
            .bg(rgb(theme::sidebar()))
            .text_color(rgb(theme::sidebar_foreground()))
            .when(self.selected == Some(i), |d| {
                d.bg(rgb(theme::sidebar_primary()))
                    .text_color(rgb(theme::sidebar_primary_foreground()))
            })
            .when(self.selected != Some(i), |d| {
                d.hover(|s| {
                    s.bg(rgb(theme::muted()))
                        .text_color(rgb(theme::muted_foreground()))
                })
            })
            .on_click(cx.listener(move |s, _, w, cx| s.act(Action::Chat(i), w, cx)))
            .child(
                div()
                    .child(t.title.trim().to_string())
                    .text_size(config::text_pixels(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(config::text_pixels(21.))
                    .max_h(px(63.))
                    .overflow_hidden(),
            )
            .child(
                row()
                    .w_full()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        row()
                            .flex_1()
                            .min_w_0()
                            .gap(px(8.))
                            .child(
                                div()
                                    .child(if self.demo_mode {
                                        provider(&t.provider).to_owned()
                                    } else {
                                        t.provider.clone()
                                    })
                                    .text_size(config::text_pixels(12.))
                                    .min_w_0()
                                    .truncate(),
                            )
                            .child(
                                row()
                                    .flex_shrink_0()
                                    .gap(px(4.))
                                    .child(icon("chat").size(px(12.)))
                                    .child(
                                        div()
                                            .child(t.messages.len().to_string())
                                            .text_size(config::text_pixels(12.)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .child(status)
                            .text_size(config::text_pixels(12.))
                            .flex_shrink_0()
                            .when(t.status == "blocked", |d| {
                                d.font_weight(FontWeight::SEMIBOLD)
                                    .px(px(7.))
                                    .py(px(3.))
                                    .rounded(px(4.))
                                    .bg(rgb(theme::primary()))
                                    .text_color(rgb(theme::primary_foreground()))
                            }),
                    ),
            );
        col().w_full().px_3().pb_1().child(card).into_any_element()
    }
    pub(super) fn chat_sidebar(
        &self,
        list: AnyElement,
        counts: [usize; 4],
        cx: &Context<Self>,
    ) -> Div {
        let mut filters = row().w_full().px_3().gap(px(6.)).pt(px(18.)).pb(px(14.));
        for (i, name) in ["All", "Attention", "Processing"].iter().enumerate() {
            let count = counts[i];
            let active = i == self.filter;
            let needs_you = i == 1;
            let color = if needs_you && count > 0 {
                theme::primary()
            } else if active {
                theme::card_foreground()
            } else {
                theme::sidebar_foreground()
            };
            let count_label = if needs_you {
                text(count.to_string(), 10., theme::primary_foreground())
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(config::text_pixels(15.))
                    .px(px(4.))
                    .py(px(1.))
                    .rounded(px(5.))
                    .bg(rgb(theme::primary()))
            } else {
                div()
                    .child(count.to_string())
                    .text_size(config::text_pixels(10.))
                    .font_weight(FontWeight::SEMIBOLD)
            };
            filters = filters.child(
                self.button(("filter", i), "", Action::Filter(i), cx)
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .relative()
                    .gap(px(5.))
                    .px(px(6.))
                    .h(px(30.))
                    .rounded(px(6.))
                    .text_size(config::text_pixels(11.))
                    .font_weight(if needs_you {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(rgb(color))
                    .bg(rgb(if active {
                        theme::card()
                    } else {
                        theme::sidebar()
                    }))
                    .when(!active, |d| {
                        d.hover(move |s| {
                            s.bg(rgb(theme::card()))
                                .text_color(rgb(if needs_you && count > 0 {
                                    theme::primary()
                                } else {
                                    theme::card_foreground()
                                }))
                        })
                    })
                    .when(self.sidebar_width < 320., |d| {
                        d.text_size(config::text_pixels(10.)).gap(px(3.)).px(px(3.))
                    })
                    .when(i == 2 && self.sidebar_width >= 320., |d| {
                        d.child(icon("working").size(px(10.)))
                    })
                    .child(*name)
                    .child(count_label)
                    .when(active, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom(px(-5.))
                                .left(px(8.))
                                .right(px(8.))
                                .h(px(2.))
                                .rounded_full()
                                .bg(rgb(theme::primary())),
                        )
                    }),
            );
        }
        col()
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .child(self.mode_sidebar_header(cx))
            .child(filters)
            .child(list)
    }
    pub(super) fn welcome(&self) -> Div {
        col()
            .flex_1()
            .min_h_0()
            .justify_center()
            .items_center()
            .text_center()
            .p_6()
            .gap_3()
            .child(icon("chat-illustration").size(px(96.)))
            .child(text("What are you working on?", 28., theme::foreground()).mt_2())
            .child(text(
                "Let's cross something off your list.",
                14.,
                theme::muted_foreground(),
            ))
    }

    pub(super) fn message_row(&self, index: usize, i: usize, _cx: &Context<Self>) -> AnyElement {
        let t = &self.workspace().threads[index];
        let m = &t.messages[i];
        let user = m.role == "user";
        let time = if m.created_at.len() > 15 {
            format!(
                "{}:{} PM",
                m.created_at[11..13].parse::<u32>().unwrap_or(14) + 2 - 12,
                &m.created_at[14..16]
            )
        } else {
            "Now".into()
        };
        let avatar = if user {
            icon("user").size(px(27.)).into_any_element()
        } else if t.provider == "claude" {
            icon("claude").size(px(27.)).into_any_element()
        } else {
            icon("codex").size(px(27.)).into_any_element()
        };
        let mut bubble = col()
            .px(px(19.))
            .py(px(17.))
            .gap(px(12.))
            .rounded(px(22.))
            .when(user, |d| {
                d.ml(relative(0.2))
                    .rounded_tr(px(6.))
                    .border_1()
                    .border_color(rgb(theme::border()))
            })
            .bg(rgb(if user {
                theme::card()
            } else {
                theme::background()
            }));
        if user || self.demo_mode {
            for paragraph in m.text.split("\n\n") {
                bubble = bubble.child(
                    text(paragraph.to_owned(), 14., theme::foreground())
                        .line_height(config::text_pixels(26.)),
                );
            }
        } else {
            bubble = bubble.child(formatted_response(&m.text));
        }
        for path in &m.images {
            let asset = image_asset(path);
            // Reserve the image's final height before asynchronous decoding, so
            // virtual rows do not acquire stale heights when an image loads.
            let bytes = embedded(asset).expect("bundled message image");
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
            bubble = bubble.child(
                img(ImageSource::Resource(Resource::Embedded(asset.into())))
                    .w_full()
                    .max_w(px(760.))
                    .map(|mut image| {
                        image.style().aspect_ratio = Some(width as f32 / height as f32);
                        image
                    })
                    .rounded_lg()
                    .object_fit(ObjectFit::Contain),
            );
        }
        col()
            .id(("message", i))
            .w_full()
            .px(relative(0.075))
            .pb_6()
            .gap_2()
            .child(
                row()
                    .when(user, |d| d.justify_end())
                    .gap_2()
                    .child(avatar)
                    .child(text(
                        if user {
                            "You".to_owned()
                        } else if self.demo_mode {
                            provider(&t.provider).to_owned()
                        } else {
                            t.provider.clone()
                        },
                        13.,
                        theme::foreground(),
                    ))
                    .child(text(time, 10., theme::muted_foreground())),
            )
            .child(bubble)
            .into_any_element()
    }

    pub(super) fn decision_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let t = &self.workspace().threads[index];
        if let Some(d) = self
            .workspace()
            .decisions
            .iter()
            .find(|d| d.thread_id == t.id)
        {
            let mut decision = col()
                .p_5()
                .gap_2()
                .bg(rgb(theme::card()))
                .border_1()
                .border_color(rgb(theme::border()))
                .rounded(px(18.))
                .child(
                    row()
                        .gap(px(12.))
                        .child(
                            div()
                                .size(px(34.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .rounded(px(11.))
                                .bg(rgb(theme::sidebar_accent()))
                                .child(
                                    icon("chat").size(px(20.)).text_color(rgb(theme::primary())),
                                ),
                        )
                        .child(
                            text(d.title.clone(), 18., theme::foreground())
                                .font_weight(FontWeight::BOLD),
                        ),
                )
                .child(
                    text(d.body.clone(), 14., theme::secondary_foreground())
                        .line_height(config::text_pixels(26.))
                        .mt(px(8.))
                        .mb(px(12.)),
                );
            for (i, option) in d.options.iter().enumerate() {
                let chosen = d.selected == Some(i);
                decision = decision.child(
                    self.button(("decision", i), "", Action::Decision(i), cx)
                        .h(px(48.))
                        .px(px(12.))
                        .gap(px(12.))
                        .rounded(px(10.))
                        .text_size(config::text_pixels(14.))
                        .border_1()
                        .border_color(rgb(if chosen {
                            theme::primary()
                        } else {
                            theme::border()
                        }))
                        .bg(rgb(if chosen {
                            theme::muted()
                        } else {
                            theme::card()
                        }))
                        .child(
                            div()
                                .size(px(17.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(if chosen {
                                    theme::foreground()
                                } else {
                                    theme::muted_foreground()
                                }))
                                .when(chosen, |d| {
                                    d.child(
                                        div()
                                            .size(px(9.))
                                            .rounded_full()
                                            .bg(rgb(theme::foreground())),
                                    )
                                }),
                        )
                        .child(option.clone()),
                );
            }
            decision.into_any_element()
        } else if t.status == "blocked" {
            text("Needs your input", 13., theme::muted_foreground()).into_any_element()
        } else {
            div().into_any_element()
        }
    }
    pub(super) fn composer_view(&self, cx: &Context<Self>) -> Div {
        let bound = self.bound_definition();
        let selected = self.selected_definition();
        let processing = self.conversation_processing();
        let codex = self.demo_mode
            && self.selected_agent.as_deref()
                == self
                    .agent_catalog
                    .entries
                    .get(1)
                    .map(|entry| entry.id.as_str());
        col()
            .w_full()
            .flex_shrink_0()
            .bg(rgb(theme::input()))
            .text_color(rgb(theme::foreground()))
            .border_t_1()
            .border_color(rgb(theme::border()))
            .overflow_hidden()
            .child(
                row()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(self.composer.clone())),
            )
            .child(
                row()
                    .h(px(50.))
                    .bg(rgb(theme::sidebar()))
                    .text_color(rgb(theme::sidebar_foreground()))
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .px_2()
                    .gap_2()
                    .child(self.ib("attach", "plus", Action::InsertFiles, cx))
                    .child(
                        self.button("agent-picker", "", Action::AgentMenu, cx)
                            .h(px(42.))
                            .px_2()
                            .child(if codex {
                                icon("codex").size(px(18.)).into_any_element()
                            } else if self.demo_mode && selected.is_some() {
                                icon("claude").size(px(22.)).into_any_element()
                            } else {
                                icon("sparkle").size(px(18.)).into_any_element()
                            })
                            .child(
                                col()
                                    .child(
                                        text(
                                            bound
                                                .as_ref()
                                                .map(|agent| agent.name.as_str())
                                                .or_else(|| {
                                                    selected.map(|agent| agent.name.as_str())
                                                })
                                                .unwrap_or("Select an agent")
                                                .to_owned(),
                                            12.,
                                            theme::sidebar_foreground(),
                                        )
                                        .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .child(text(
                                        bound
                                            .as_ref()
                                            .map(|agent| {
                                                format!("{} {}", agent.model, agent.effort)
                                            })
                                            .or_else(|| {
                                                selected.map(|agent| {
                                                    format!("{} {}", agent.model, agent.effort)
                                                })
                                            })
                                            .unwrap_or_default(),
                                        11.,
                                        theme::sidebar_foreground(),
                                    )),
                            )
                            .child(icon("chevron")),
                    )
                    .child(div().flex_1())
                    .child({
                        let empty = !processing && self.composer.read(cx).content.trim().is_empty();
                        self.button(
                            "send",
                            "",
                            if processing {
                                Action::Stop
                            } else {
                                Action::Send
                            },
                            cx,
                        )
                        .w(px(34.))
                        .px_0()
                        .justify_center()
                        .bg(rgb(if empty {
                            theme::sidebar()
                        } else {
                            theme::primary()
                        }))
                        .hover(|s| {
                            s.bg(rgb(if empty {
                                theme::secondary()
                            } else {
                                theme::primary()
                            }))
                        })
                        .focus(|s| {
                            s.bg(rgb(if empty {
                                theme::sidebar_accent()
                            } else {
                                theme::primary()
                            }))
                        })
                        .child(if processing {
                            div()
                                .size(px(12.))
                                .bg(rgb(theme::primary_foreground()))
                                .into_any_element()
                        } else {
                            icon("send")
                                .text_color(rgb(if empty {
                                    theme::muted_foreground()
                                } else {
                                    theme::primary_foreground()
                                }))
                                .into_any_element()
                        })
                    }),
            )
    }
}

fn formatted_response(source: &str) -> Div {
    let mut content = col().gap_2();
    let mut fenced = false;
    for line in source.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            content = content.child(
                text(line.to_owned(), 13., theme::foreground())
                    .font_family(config::code_font())
                    .bg(rgb(theme::muted()))
                    .px_3()
                    .py_1(),
            );
            continue;
        }
        let heading = line.chars().take_while(|&c| c == '#').count();
        let display = if heading > 0 {
            line[heading..].trim()
        } else {
            line
        };
        let (display, highlights) = inline_response(display);
        content = content.child(
            div()
                .text_size(config::text_pixels(if heading > 0 { 18. } else { 14. }))
                .text_color(rgb(theme::foreground()))
                .child(StyledText::new(display).with_highlights(highlights))
                .line_height(config::text_pixels(26.))
                .when(heading > 0, |d| d.font_weight(FontWeight::SEMIBOLD))
                .when(line.starts_with("- ") || line.starts_with("* "), |d| {
                    d.pl_3()
                }),
        );
    }
    content
}

fn inline_response(source: &str) -> (String, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
    let mut text = String::with_capacity(source.len());
    let mut highlights = Vec::new();
    let mut rest = source;
    while !rest.is_empty() {
        let marker = if rest.starts_with("**") {
            "**"
        } else if rest.starts_with('`') {
            "`"
        } else if rest.starts_with('*') {
            "*"
        } else {
            ""
        };
        if !marker.is_empty()
            && let Some(end) = rest[marker.len()..].find(marker).filter(|&end| end > 0)
        {
            let start = text.len();
            text.push_str(&rest[marker.len()..marker.len() + end]);
            let style = match marker {
                "**" => HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
                "*" => HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                },
                _ => HighlightStyle {
                    background_color: Some(rgb(theme::muted()).into()),
                    ..Default::default()
                },
            };
            highlights.push((start..text.len(), style));
            rest = &rest[end + marker.len() * 2..];
        } else {
            let character = rest.chars().next().expect("nonempty text");
            text.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    (text, highlights)
}

#[cfg(test)]
mod response_tests {
    use super::inline_response;

    #[test]
    fn streaming_markers_and_unicode_preserve_visible_content() {
        assert_eq!(inline_response("A **part").0, "A **part");
        let (text, styles) = inline_response("A **café** and `x*y`.");
        assert_eq!(text, "A café and x*y.");
        assert_eq!(&text[styles[0].0.clone()], "café");
        assert_eq!(&text[styles[1].0.clone()], "x*y");
    }
}
