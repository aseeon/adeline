//! The composer: the message input, its option menus and the execution
//! settings under it.
use super::*;

impl Adeline {
    pub(crate) fn composer_view(&self, width_rems: f32, cx: &Context<Self>) -> Div {
        // The controls sit in the chat column, inside its side padding.
        let fit = Fit::for_width(width_rems.min(CHAT_COLUMN) - 4.);
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
        let options = if bound.is_some() {
            self.conversation_options()
        } else {
            Vec::new()
        };
        let running = self.current_live().is_some_and(|live| live.running);
        let selections = bound
            .as_ref()
            .map(|definition| definition.selections.clone());
        let no_agents =
            bound.is_none() && self.agent_catalog().entries.is_empty() && !self.demo_mode;
        let (agent_path, agent_color) = self.agent_icon(&agent_name, cx);
        let processing = self.conversation_processing();
        let can_send = !self.attachments.is_empty()
            || self
                .composer
                .read(cx)
                .text()
                .chars()
                .any(|character| !character.is_whitespace());
        let steering = self
            .current_live()
            .is_some_and(|live| live.features.steering);
        // A new chat shows its agent's defaults, read-only until it starts.
        let defaults = (bound.is_none())
            .then(|| selected.map(|d| (d.model.clone(), d.effort.clone(), d.mode.clone())))
            .flatten();
        let menu = |category, max_chars| {
            self.option_menu(
                category,
                &options,
                selections.as_ref(),
                running,
                processing,
                max_chars,
                cx,
            )
        };
        let (model, effort, more, mode) = if let Some((model, effort, mode)) = defaults {
            let fixed = |id, heading, value: String| {
                (!value.is_empty())
                    .then(|| execution_setting(id, heading, value, 16, cx).into_any_element())
            };
            (
                fixed("chat-model", "Model", model_label(&model)),
                fixed("chat-effort", "Effort", effort_label(&effort)),
                None,
                fixed("chat-mode", "Mode", effort_label(&mode)),
            )
        } else {
            (
                menu(conversation::Category::Model, fit.model),
                menu(conversation::Category::Effort, fit.effort),
                menu(conversation::Category::Other, 0),
                menu(conversation::Category::Mode, fit.mode),
            )
        };
        let send_tooltip = if config::with(|s| s.modes.chats.submit_on_enter) {
            "Send (Enter)".to_owned()
        } else {
            format!("Send ({})", conversation_ui::shortcut("Ctrl+Enter"))
        };
        let send_keys = row()
            .gap_0p5()
            .when(!config::with(|s| s.modes.chats.submit_on_enter), |keys| {
                let modifier = if cfg!(target_os = "macos") {
                    "command.svg"
                } else {
                    "control.svg"
                };
                keys.child(Icon::default().path(modifier).size_4())
            })
            .child(Icon::default().path("arrow-elbow-down-left.svg").size_4());
        let send = if no_agents {
            // Sending needs an agent, so adding one is the main action.
            Button::new("send-chat-message")
                .small()
                .primary()
                .label("Add an agent…")
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::AddAgent, window, cx)))
                .into_any_element()
        } else if processing {
            let owner = cx.weak_entity();
            let send_now = if steering {
                format!(
                    "Send now ({})",
                    conversation_ui::shortcut("Ctrl+Shift+Enter")
                )
            } else {
                format!(
                    "Send now: stops the turn, then sends ({})",
                    conversation_ui::shortcut("Ctrl+Shift+Enter")
                )
            };
            row()
                .gap_0p5()
                .child(
                    Button::new("send-chat-message")
                        .small()
                        .ghost()
                        .child(send_keys)
                        .accessibility_label("Queue")
                        .tooltip("Queue: sends when this turn ends")
                        .disabled(!can_send)
                        .on_click(
                            cx.listener(|app, _, window, cx| app.act(Action::Send, window, cx)),
                        ),
                )
                .child(
                    Button::new("send-chat-options")
                        .small()
                        .ghost()
                        .icon(Icon::default().path("chevron.svg").size_3())
                        .accessibility_label("Send options")
                        .tooltip("Send options")
                        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                            let (queue, now) = (owner.clone(), owner.clone());
                            menu.item(PopupMenuItem::new("Queue").on_click(move |_, window, cx| {
                                let _ =
                                    queue.update(cx, |app, cx| app.act(Action::Send, window, cx));
                            }))
                            .item(
                                PopupMenuItem::new(send_now.clone()).on_click(
                                    move |_, window, cx| {
                                        let _ = now.update(cx, |app, cx| {
                                            app.act(Action::SendNow, window, cx);
                                        });
                                    },
                                ),
                            )
                        }),
                )
                .child(
                    Button::new("stop-chat-turn")
                        .small()
                        .ghost()
                        .icon(Icon::default().path("stop.svg"))
                        .accessibility_label("Stop")
                        .tooltip(format!("Stop ({})", conversation_ui::shortcut("Ctrl+.")))
                        .on_click(
                            cx.listener(|app, _, window, cx| app.act(Action::Stop, window, cx)),
                        ),
                )
                .into_any_element()
        } else {
            // The icons spell the send shortcut: Enter, or Ctrl+Enter (Cmd on
            // macOS). Both are content, not the button's icon, so the padding
            // and the Enter key stay put when the shortcut changes.
            Button::new("send-chat-message")
                .small()
                .ghost()
                .child(send_keys)
                .accessibility_label("Send")
                .tooltip(send_tooltip)
                .disabled(!can_send)
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::Send, window, cx)))
                .into_any_element()
        };
        let owner = cx.weak_entity();
        let textarea = Textarea::new(&self.composer)
            .aria_label("Message")
            .appearance(false)
            .flex_1()
            .min_w_0()
            .on_paste(move |item, _, cx| {
                owner
                    .update(cx, |app, cx| app.paste(item, cx))
                    .unwrap_or(false)
            });
        let chips = (!self.attachments.is_empty() || self.attachment_error.is_some()).then(|| {
            col()
                .w_full()
                .gap_1()
                .mb_2()
                .when(!self.attachments.is_empty(), |column| {
                    column.child(self.attachment_chips(&self.attachments, None, cx))
                })
                .when_some(self.attachment_error.clone(), |column, error| {
                    column.child(div().text_sm().text_color(theme.danger).child(error))
                })
        });
        let island = col()
            .id("composer-island")
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
            .on_drop(cx.listener(|app, paths: &ExternalPaths, _, cx| {
                for path in paths.paths() {
                    app.attach_local(path.clone(), cx);
                }
            }))
            .capture_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified()
                    || !app.composer.focus_handle(cx).is_focused(window)
                {
                    return;
                }
                if app.composer_key(&event.keystroke.key, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .children(chips)
            .child(
                row()
                    .w_full()
                    .min_w_0()
                    .items_start()
                    .gap_2()
                    .child(textarea)
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
                                .tooltip(format!(
                                    "Attach files… ({})",
                                    conversation_ui::shortcut("Ctrl+Shift+A")
                                )),
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
                            // Clip rather than draw under the mode button.
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
                            .children(model)
                            .children(effort)
                            .children(more),
                    )
                    .children(mode),
            );
        let slash = self.slash_list(cx).map(|list| {
            div()
                .absolute()
                .bottom_full()
                .left_0()
                .right_0()
                .pb_1()
                .child(list)
        });
        col()
            .w_full()
            .flex_shrink_0()
            .min_w_0()
            .bg(theme.background)
            .pb_4()
            .child(chat_column().child(div().relative().child(island).children(slash)))
    }

    fn option_menu(
        &self,
        category: conversation::Category,
        options: &[conversation::SessionOption],
        selections: Option<&conversation::Selections>,
        running: bool,
        processing: bool,
        max_chars: usize,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        use conversation::Category;
        let menu = option_menu_name(category);
        let (id, heading, keys) = match category {
            Category::Model => ("chat-model", "Model", "Alt+M"),
            Category::Effort => ("chat-effort", "Effort", "Alt+E"),
            Category::Mode => ("chat-mode", "Mode", "Alt+O"),
            Category::Other => ("chat-more-options", "More options", "Alt+P"),
        };
        let shown: Vec<conversation::SessionOption> = match category {
            Category::Other => options
                .iter()
                .filter(|option| option.category == Category::Other)
                .cloned()
                .collect(),
            category => conversation::option(options, category)
                .cloned()
                .into_iter()
                .collect(),
        };
        if shown.is_empty() {
            return None;
        }
        // The running agent knows the current value; a stopped one shows the
        // choice saved for its next start.
        let current = |option: &conversation::SessionOption| {
            selections
                .and_then(|selections| selections.get(option))
                .filter(|_| !running)
                .map_or_else(|| option.current(), str::to_owned)
        };
        let shortcut = conversation_ui::shortcut(keys);
        let trigger = if category == Category::Other {
            Button::new(id)
                .ghost()
                .small()
                .flex_shrink_0()
                .icon(Icon::default().path("more.svg"))
                .accessibility_label(heading)
        } else {
            let option = &shown[0];
            let value = current(option);
            let name = option.name_of(&value);
            let label: SharedString = if name == value && category == Category::Mode {
                effort_label(&value).into()
            } else {
                name.to_owned().into()
            };
            Button::new(id)
                .ghost()
                .small()
                .flex_shrink_0()
                .label(shorten(&label, max_chars))
                .dropdown_caret(true)
                .accessibility_label(format!("{heading}: {label}"))
        }
        .disabled(processing)
        .tooltip(if processing {
            format!("{heading}. Switch after this turn finishes.")
        } else {
            format!("{heading} ({shortcut})")
        });
        if processing {
            return Some(trigger.into_any_element());
        }
        // The choice lists are built only while the menu is open.
        let currents: Vec<String> = shown.iter().map(current).collect();
        let owner = cx.weak_entity();
        let content_owner = owner.clone();
        let state = self.command_popup.clone();
        let cache_note = matches!(category, Category::Model | Category::Effort);
        Some(
            component::popover::Popover::new(menu)
                .anchor(if category == Category::Mode {
                    Anchor::BottomRight
                } else {
                    Anchor::BottomLeft
                })
                .trigger(trigger)
                .open(self.menu == Some(menu))
                .p_0()
                .shadow(views::menu_shadow(cx))
                .when_some(state.as_ref(), |popover, state| {
                    popover.track_focus(&state.focus_handle(cx))
                })
                .on_open_change(move |open, window, cx| {
                    let _ = owner.update(cx, |app, cx| {
                        if *open {
                            app.open_commands(menu, window, cx);
                        } else if app.menu == Some(menu) {
                            app.menu = None;
                            app.refocus_after_menu(window, cx);
                            app.composer_region.update(cx, |_, cx| cx.notify());
                            cx.notify();
                        }
                    });
                })
                .content(move |_, _, cx| {
                    let popover = cx.entity();
                    let owner = content_owner.clone();
                    let (groups, ids) = option_groups(category, &shown, &currents);
                    let values: Vec<Vec<String>> = groups
                        .iter()
                        .map(|(_, items)| items.iter().map(|item| item.0.clone()).collect())
                        .collect();
                    col().w(rems(24.)).when_some(state.as_ref(), |column, state| {
                        let mut command = Command::new(state)
                            .bordered(false)
                            .placeholder(format!("Search {}", heading.to_lowercase()))
                            .when(cache_note, |command| {
                                command.header(|_, _, cx| {
                                    div()
                                        .px_3()
                                        .py_2()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            "Switching may invalidate the prompt cache and cost more on the next turn.",
                                        )
                                })
                            });
                        for (group, items) in &groups {
                            let mut entry = CommandGroup::new();
                            if !group.is_empty() {
                                entry = entry.label(group.clone());
                            }
                            command = command.group(entry.items(items.iter().map(
                                |(value, name, description, checked)| {
                                    let item = CommandItem::new()
                                        .label(name.clone())
                                        .keywords([value.clone()])
                                        .checked(*checked);
                                    if description.is_empty() {
                                        return item;
                                    }
                                    let (name, description) = (name.clone(), description.clone());
                                    item.child(move |_, cx| {
                                        col()
                                            .child(name.clone())
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(description.clone()),
                                            )
                                    })
                                },
                            )));
                        }
                        column.child(command.on_confirm(move |path, window, cx| {
                            let (Some(value), Some(option)) = (
                                values.get(path.section).and_then(|items| items.get(path.row)).cloned(),
                                ids.get(path.section).cloned(),
                            ) else {
                                return;
                            };
                            popover.update(cx, |state, cx| state.dismiss(window, cx));
                            let _ = owner.update(cx, |app, cx| {
                                app.menu = None;
                                app.act(
                                    Action::SetOption {
                                        category,
                                        option,
                                        value,
                                    },
                                    window,
                                    cx,
                                );
                            });
                        }))
                    })
                })
                .into_any_element(),
        )
    }
}

pub(crate) struct Composer {
    owner: WeakEntity<Adeline>,
    width: Pixels,
    _content_subscription: Subscription,
}

type OptionItem = (String, String, String, bool);

fn option_groups(
    category: conversation::Category,
    shown: &[conversation::SessionOption],
    currents: &[String],
) -> (Vec<(String, Vec<OptionItem>)>, Vec<String>) {
    use conversation::{Category, OptionKind};
    let mut groups: Vec<(String, Vec<OptionItem>)> = Vec::new();
    for (option, value) in shown.iter().zip(currents) {
        let mut items = Vec::new();
        match &option.kind {
            OptionKind::Select { choices, .. } => {
                for choice in choices {
                    items.push((
                        choice.value.clone(),
                        choice.name.clone(),
                        choice.description.clone(),
                        choice.value == *value,
                    ));
                }
            }
            OptionKind::Boolean { current } => {
                for (on, name) in [(true, "On"), (false, "Off")] {
                    items.push((
                        on.to_string(),
                        name.to_owned(),
                        String::new(),
                        *current == on,
                    ));
                }
            }
        }
        if category == Category::Model {
            // Models group by the agent's groups or provider prefix.
            let OptionKind::Select { choices, .. } = &option.kind else {
                continue;
            };
            for (choice, item) in choices.iter().zip(items) {
                match groups.last_mut() {
                    Some((group, list)) if *group == choice.group => list.push(item),
                    _ => groups.push((choice.group.clone(), vec![item])),
                }
            }
        } else {
            let title = if category == Category::Other {
                option.name.clone()
            } else {
                String::new()
            };
            groups.push((title, items));
        }
    }
    let ids = if category == Category::Model {
        vec![shown[0].id.clone(); groups.len()]
    } else {
        shown.iter().map(|option| option.id.clone()).collect()
    };
    (groups, ids)
}

fn model_label(model: &str) -> String {
    model.rsplit('/').next().unwrap_or(model).to_owned()
}

fn effort_label(effort: &str) -> String {
    let mut chars = effort.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

struct Fit {
    agent: usize,
    model: usize,
    effort: usize,
    mode: usize,
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let kept: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{}\u{2026}", kept.trim_end())
    }
}

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
            width: px(0.),
            _content_subscription: subscription,
        }
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Unmeasured counts as wide, so the first frame shows full labels.
        let width = if self.width > px(0.) {
            self.width / window.rem_size()
        } else {
            f32::MAX
        };
        let measured = cx.entity();
        self.owner
            .update(cx, |app, cx| {
                app.composer_view(width, cx)
                    .relative()
                    .child(size_probe(move |size, cx| {
                        measured.update(cx, |composer, cx| {
                            if (composer.width - size.width).abs() > px(0.5) {
                                composer.width = size.width;
                                cx.notify();
                            }
                        });
                    }))
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Fit {
    fn for_width(rems: f32) -> Self {
        let (agent, model, effort, mode) = if rems >= 40. {
            (usize::MAX, usize::MAX, usize::MAX, usize::MAX)
        } else if rems >= 36. {
            (usize::MAX, usize::MAX, usize::MAX, 12)
        } else if rems >= 33. {
            (usize::MAX, 16, usize::MAX, 10)
        } else if rems >= 30. {
            (12, 10, 6, 8)
        } else {
            // The narrowest window.
            (8, 7, 5, 6)
        };
        Self {
            agent,
            model,
            effort,
            mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tokens;

    #[test]
    fn token_counts_round_to_short_units() {
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(38_400), "38k");
        assert_eq!(tokens(200_000), "200k");
        assert_eq!(tokens(1_000_000), "1M");
        assert_eq!(tokens(1_500_000), "1.5M");
    }
}
