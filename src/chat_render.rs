use super::*;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariants as _},
    input::Textarea,
    message::{Message, MessageAlignment, MessageContent, MessageHeader},
    text::TextView,
};
impl Adeline {
    pub(super) fn chat_card(&self, i: usize, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[i];
        let status = match thread.status.as_str() {
            "working" => "Processing",
            "completed" => "Completed",
            "blocked" => "Attention",
            "archived" => "Archived",
            _ => "Active",
        };
        let name = if self.demo_mode {
            provider(&thread.provider)
        } else {
            thread.provider.as_str()
        };
        div()
            .w_full()
            .px_3()
            .pb_1()
            .child(
                Button::new(format!("chat:{}:{}", self.workspace().config.id, thread.id))
                    .ghost()
                    .selected(self.selected == Some(i))
                    .accessibility_label(format!("Open chat: {}, {status}", thread.title))
                    .w_full()
                    .min_w_0()
                    .h_auto()
                    .py_2()
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.act(Action::Chat(i), window, cx)),
                    )
                    .child(
                        col()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .text_sm()
                                    .whitespace_normal()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(thread.title.trim().to_owned()),
                            )
                            .child(
                                row()
                                    .w_full()
                                    .min_w_0()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .text_xs()
                                            .child(name.to_owned()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .child(format!("{} messages", thread.messages.len())),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .when(thread.status == "blocked", |view| {
                                                view.font_weight(FontWeight::SEMIBOLD)
                                            })
                                            .child(status),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn chat_sidebar(
        &self,
        list: AnyElement,
        counts: [usize; 4],
        cx: &Context<Self>,
    ) -> Div {
        let mut filters = row()
            .id("chat-status-filters")
            .w_full()
            .min_w_0()
            .px_3()
            .gap_1()
            .py_3()
            .flex_wrap();
        for (i, name) in ["All", "Attention", "Processing"].iter().enumerate() {
            filters = filters.child(
                Button::new(("chat-filter", i))
                    .ghost()
                    .small()
                    .selected(self.filter == i)
                    .label(format!("{name} {}", counts[i]))
                    .accessibility_label(format!("Show {name} chats, {} total", counts[i]))
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(Action::Filter(i), window, cx);
                    }))
                    .flex_shrink_0(),
            );
        }
        col()
            .w_full()
            .min_w_0()
            .h_full()
            .child(self.mode_sidebar_header(cx))
            .child(filters)
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
                                Button::new("chat-agent-picker")
                                    .ghost()
                                    .small()
                                    .w_full()
                                    .label(agent_name.clone())
                                    .tooltip(agent_name)
                                    .dropdown_caret(true)
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        app.act(Action::AgentMenu, window, cx);
                                    })),
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
