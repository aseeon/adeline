//! The Chats layout, command popovers (the menus), modal dialogs (including
//! quit, SSH prompt and upgrade), and UI helpers shared across windows.
use super::*;
use crate::conversation::ThreadStatus;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::switch::Switch;

impl Adeline {
    pub(super) fn chats(&self, cx: &Context<Self>) -> AnyElement {
        let thread = self.selected.map(|ix| &self.workspace().threads[ix]);
        let title = thread.map_or_else(|| "New chat".to_owned(), |thread| thread.title.clone());
        // Status, message count and age live in the conversation list; the
        // header adds only what the list can't show.
        let header = chat_render::chat_column()
            .h(rems(chat_render::CHAT_HEADER_HEIGHT))
            .flex()
            .items_center()
            .gap(rems(0.375))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .children(thread.and_then(|_| self.todo_button(cx)))
            .when_some(thread, |header, thread| {
                header.child(chat_render::context_meter(thread.context, cx).mr_2())
            })
            .when_some(thread, |header, thread| {
                let done = thread.status.closed();
                let archived = thread.status == ThreadStatus::Archived;
                let complete = if done {
                    "Mark as incomplete"
                } else {
                    "Mark as complete"
                };
                header
                    .child(
                        Button::new("complete-chat")
                            .small()
                            .outline()
                            .icon(Icon::default().path("check.svg"))
                            .selected(done)
                            .accessibility_label(complete)
                            .tooltip(complete)
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.act(Action::Complete, window, cx);
                            })),
                    )
                    .child(
                        Button::new("archive-chat")
                            .small()
                            .outline()
                            .icon(Icon::default().path("archive.svg"))
                            .disabled(archived)
                            .accessibility_label(if archived { "Archived" } else { "Archive" })
                            .tooltip(if archived { "Archived" } else { "Archive" })
                            .on_click(cx.listener(|app, _, window, cx| {
                                if let Some(ix) = app.selected {
                                    app.act(Action::ArchiveChat(ix), window, cx);
                                }
                            })),
                    )
            });
        let theme = cx.theme();
        // The transcript fills the pane so its scrollbar runs the full height. The
        // header and composer float over it and stop short of the scrollbar;
        // matching left padding keeps their columns centered on the messages.
        let scrollbar = theme::SCROLLBAR_TRACK;
        let composer_height = std::rc::Rc::clone(&self.composer_height);
        let transcript = self.transcript.clone();
        // The activity panel's stats island matches the composer's height.
        let owner = cx.weak_entity();
        let measure = chat::size_probe(move |size, cx| {
            if (composer_height.get() - size.height).abs() > px(0.5) {
                composer_height.set(size.height);
                transcript.update(cx, |_, cx| cx.notify());
                let _ = owner.update(cx, |_, cx| cx.notify());
            }
        });
        // Messages fade out just below the header's edge instead of meeting a line.
        let fade = div()
            .absolute()
            .top(rems(chat_render::CHAT_HEADER_HEIGHT))
            .left_0()
            .right(scrollbar)
            .h(rems(chat_render::HEADER_FADE))
            .bg(linear_gradient(
                180.,
                linear_color_stop(theme.background.alpha(0.9), 0.),
                linear_color_stop(theme.background.alpha(0.), 1.),
            ));
        div()
            .id("conversation-panel")
            .role(Role::Group)
            .aria_label("Conversation panel")
            .relative()
            .size_full()
            .min_w_0()
            // Not cached: a replayed transcript skips registering its text for
            // selection, and Kit then drops the selection for a frame.
            .child(self.transcript.clone())
            .child(fade)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right(scrollbar)
                    .pl(scrollbar)
                    .bg(theme.background)
                    .child(header),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right(scrollbar)
                    .pl(scrollbar)
                    .bg(theme.background)
                    // Messages fade out just above the composer, mirroring the header.
                    .child(
                        div()
                            .absolute()
                            .bottom_full()
                            .left_0()
                            .right_0()
                            .h(rems(chat_render::HEADER_FADE))
                            .bg(linear_gradient(
                                0.,
                                linear_color_stop(theme.background.alpha(0.9), 0.),
                                linear_color_stop(theme.background.alpha(0.), 1.),
                            )),
                    )
                    // Back to the latest message, 0.5rem above the composer.
                    .when(
                        thread.is_some() && !self.transcript.read(cx).following(),
                        |region| {
                            region.child(
                                div()
                                    .absolute()
                                    .bottom_full()
                                    .left_0()
                                    .right_0()
                                    .pb_2()
                                    .flex()
                                    .justify_center()
                                    .child(activity::jump_to_latest("chat-jump", cx)),
                            )
                        },
                    )
                    .child(self.composer_region.clone())
                    .child(measure),
            )
            .into_any_element()
    }

    fn command_entries(&self, menu: &'static str) -> Vec<(String, Action)> {
        let mut entries: Vec<(String, Action)> = Vec::new();
        match menu {
            "agents" | "agent" => {
                entries.extend(self.agent_catalog().entries.iter().map(|entry| {
                    (
                        entry.definition.name.clone(),
                        Action::Agent(entry.id.clone()),
                    )
                }));
                entries.push(("Add an agent…".into(), Action::AddAgent));
                entries.push(("Agent settings…".into(), Action::AgentSettings));
                if menu == "agent" && self.demo_mode {
                    entries.extend([
                        ("Standard speed".into(), Action::Speed(0)),
                        ("Fast speed".into(), Action::Speed(1)),
                    ]);
                }
            }
            "files" => {
                entries.extend([
                    ("Add a file…".into(), Action::AddFile),
                    ("Add a directory…".into(), Action::AddDirectory),
                ]);
            }
            "mode-settings" => {
                entries.extend(
                    self.mode_options(Section::Chats)
                        .into_iter()
                        .map(|(label, _, _, action)| (label.into(), action)),
                );
                entries.push(("Full chat settings…".into(), Action::ConfigureModeSettings));
            }
            _ => {
                entries.push(("Add an agent…".into(), Action::AddAgent));
                if self.has_open_project() {
                    entries.push(("Project settings…".into(), Action::Settings));
                }
                if !self.demo_mode {
                    entries.push(("Stop all agents".into(), Action::StopAll));
                }
                entries.extend([
                    ("Settings…".into(), Action::AppSettings),
                    ("About Adeline…".into(), Action::About),
                    ("Quit Adeline".into(), Action::QuitApp),
                ]);
            }
        }
        entries
    }

    pub(super) fn refocus_after_menu(&self, window: &mut Window, cx: &mut App) {
        // A closed menu leaves focus on its search box, which is gone. The
        // message box takes it, so shortcuts keep working.
        let menu_focused = self
            .command_popup
            .as_ref()
            .is_some_and(|state| state.focus_handle(cx).is_focused(window));
        if menu_focused || window.focused(cx).is_none() {
            window.focus(&self.composer.focus_handle(cx), cx);
        }
    }

    pub(super) fn command_popover(
        &self,
        menu: &'static str,
        trigger: Button,
        anchor: Anchor,
        cx: &Context<Self>,
    ) -> component::popover::Popover {
        let owner = cx.weak_entity();
        let content_owner = owner.clone();
        let entries = self.command_entries(menu);
        let toggles = if menu == "mode-settings" {
            self.mode_options(Section::Chats)
        } else {
            Vec::new()
        };
        let state = self.command_popup.clone();
        let errors = if matches!(menu, "agent" | "agents") {
            self.agent_catalog().errors.join("\n")
        } else {
            String::new()
        };
        component::popover::Popover::new(menu)
            .anchor(anchor)
            .trigger(trigger)
            .open(self.menu == Some(menu))
            .p_0()
            .shadow(menu_shadow(cx))
            .when_some(state.as_ref(), |popover, state| {
                popover.track_focus(&state.focus_handle(cx))
            })
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if *open {
                        let action = match menu {
                            "agents" => Action::Agents,
                            "agent" => Action::AgentMenu,
                            "mode-settings" => Action::ModeSettings,
                            "files" => Action::InsertFiles,
                            _ => Action::AppMenu,
                        };
                        app.act(action, window, cx);
                    } else if app.menu == Some(menu) {
                        app.menu = None;
                        app.refocus_after_menu(window, cx);
                        app.header_region.update(cx, |_, cx| cx.notify());
                        app.control_pane.update(cx, |_, cx| cx.notify());
                        app.composer_region.update(cx, |_, cx| cx.notify());
                        cx.notify();
                    }
                });
            })
            .content(move |_, _, cx| {
                let owner = content_owner.clone();
                let entries = entries.clone();
                let popover = cx.entity();
                col()
                    .w(rems(22.))
                    .when(!errors.is_empty(), |column| {
                        column.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(errors.clone()),
                        )
                    })
                    .when_some(state.as_ref(), |column, state| {
                        column.child(
                            Command::new(state)
                                .bordered(false)
                                .searchable(menu != "files")
                                .placeholder("Search")
                                .items(entries.iter().map(|(label, _)| {
                                    let item = CommandItem::new().label(label.clone());
                                    let Some((label, _, checked, action)) =
                                        toggles.iter().find(|(name, _, _, _)| *name == label)
                                    else {
                                        return item;
                                    };
                                    let (label, checked) = (*label, *checked);
                                    let action = action.clone();
                                    let owner = owner.clone();
                                    item.child(move |_, _| {
                                        let owner = owner.clone();
                                        let action = action.clone();
                                        row()
                                            .w_full()
                                            .gap_3()
                                            .child(div().flex_1().child(label))
                                            .child(
                                                Switch::new(label)
                                                    .checked(checked)
                                                    .accessibility_label(label)
                                                    .on_change(move |_, window, cx| {
                                                        cx.stop_propagation();
                                                        let _ = owner.update(cx, |app, cx| {
                                                            app.act(action.clone(), window, cx);
                                                        });
                                                    }),
                                            )
                                    })
                                }))
                                .on_confirm(move |path, window, cx| {
                                    if let Some((_, action)) = entries.get(path.row) {
                                        if !matches!(
                                            action,
                                            Action::ShowCompleted
                                                | Action::ShowArchived
                                                | Action::HideToolCalls
                                                | Action::ShowThinking
                                                | Action::SubmitOnEnter
                                        ) {
                                            popover
                                                .update(cx, |state, cx| state.dismiss(window, cx));
                                        }
                                        let _ = owner.update(cx, |app, cx| {
                                            app.act(action.clone(), window, cx);
                                        });
                                    }
                                }),
                        )
                    })
            })
    }

    pub(super) fn open_commands(
        &mut self,
        menu: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if menu == "projects" {
            self.open_projects_menu(window, cx);
            return;
        }
        if menu == "machines" {
            self.menu = Some(menu);
            self.header_region.update(cx, |_, cx| cx.notify());
            cx.notify();
            return;
        }
        let state = cx.new(|cx| CommandState::new(window, cx));
        window.focus(&state.focus_handle(cx), cx);
        self.command_popup = Some(state);
        self.menu = Some(menu);
        self.header_region.update(cx, |_, cx| cx.notify());
        self.control_pane.update(cx, |_, cx| cx.notify());
        self.composer_region.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn open_modal(&self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity();
        let weak = owner.downgrade();
        let content = cx.new(|cx| {
            let subscription = cx.observe_in(&owner, window, |_, owner, window, cx| {
                if owner.read(cx).modal.is_none() {
                    window.close_dialog(cx);
                }
                cx.notify();
            });
            ModalContent {
                owner: weak.clone(),
                _subscription: subscription,
            }
        });
        let width = px(match self.modal {
            Some("about") => 320.,
            Some("browser" | "prompt") => 520.,
            _ => 400.,
        });
        window.open_dialog(cx, move |dialog, _, cx| {
            let owner = weak.clone();
            let confirm_owner = weak.clone();
            styled_dialog(dialog, cx)
                .w(width)
                .child(content.clone())
                .on_ok(move |_, window, cx| {
                    let _ = confirm_owner.update(cx, |app, cx| {
                        let action = match app.modal {
                            Some("add-project") => Some(Action::SaveProject),
                            Some("settings") => Some(Action::SaveSettings),
                            Some("rename-project") => Some(Action::SaveRename),
                            Some("prompt") => Some(Action::AnswerPrompt(true)),
                            _ => None,
                        };
                        if let Some(action) = action {
                            app.act(action, window, cx);
                        }
                    });
                    false
                })
                .on_close(move |_, _, cx| {
                    let _ = owner.update(cx, |app, cx| {
                        // Closing an SSH prompt cancels it. The next prompt, or one
                        // that came while another dialog was open, shows.
                        if app.modal.take() == Some("prompt") {
                            client::answer_prompt(None, cx);
                        }
                        if client::prompt(cx).is_some() {
                            app.show_prompt(cx);
                        }
                        cx.notify();
                    });
                })
        });
        if matches!(
            self.modal,
            Some("add-project" | "settings" | "rename-project")
        ) {
            window.focus(&self.name_input.focus_handle(cx), cx);
        }
        if self.modal == Some("prompt") {
            window.focus(&self.prompt_input.focus_handle(cx), cx);
        }
    }

    fn upgrade_dialog(&self, cx: &Context<Self>) -> AnyElement {
        let machine = self.upgrade_machine.clone().unwrap_or_default();
        let name = machines::name(&machine);
        let detail = match client::state(&machine, cx) {
            Some(client::State::UpgradeNeeded { version, active }) => format!(
                "{name} runs Adeline {version} with {active} active conversation{}. Upgrading restarts its engine with Adeline {}, which stops them.",
                if *active == 1 { "" } else { "s" },
                env!("CARGO_PKG_VERSION"),
            ),
            _ => format!(
                "{name} runs an older Adeline. Upgrading restarts its engine, which stops its agents."
            ),
        };
        let content = col()
            .gap_3()
            .text_sm()
            .child(dialog_title(format!("Upgrade {name}?")))
            .child(div().text_color(cx.theme().muted_foreground).child(detail))
            .child(
                dialog_footer()
                    .child(self.button("upgrade-later", "Later", Action::Close, cx))
                    .child(
                        self.button(
                            "upgrade-now",
                            "Upgrade now (stops them)",
                            Action::ConfirmUpgrade,
                            cx,
                        )
                        .danger(),
                    ),
            );
        col()
            .id("dialog-content")
            .role(Role::Group)
            .aria_label(format!("Upgrade {name}?"))
            .child(content)
            .into_any_element()
    }

    fn prompt_dialog(&self, cx: &Context<Self>) -> AnyElement {
        let group = client::prompt(cx).map(|(machine, text, host_key)| {
            let title = if host_key {
                format!("Trust {}?", machines::name(&machine))
            } else {
                format!("Sign in to {}", machines::name(&machine))
            };
            col()
                .id("ssh-prompt")
                .role(Role::Group)
                .aria_label(title.clone())
                .gap_3()
                .child(dialog_title(title))
                .child(
                    div()
                        .id("ssh-prompt-text")
                        .role(Role::Status)
                        .aria_label(text.clone())
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(text),
                )
                .when(!host_key, |column| {
                    column.child(Input::new(&self.prompt_input).aria_label("SSH answer"))
                })
                .child(
                    dialog_footer()
                        .child(self.button(
                            "prompt-cancel",
                            if host_key { "Reject" } else { "Cancel" },
                            Action::AnswerPrompt(false),
                            cx,
                        ))
                        .child(
                            self.button(
                                "prompt-ok",
                                if host_key { "Accept" } else { "Continue" },
                                Action::AnswerPrompt(true),
                                cx,
                            )
                            .primary(),
                        ),
                )
        });
        col().gap_3().text_sm().children(group).into_any_element()
    }

    fn quit_dialog(&self, cx: &Context<Self>) -> AnyElement {
        let stopping = self.runtime.stopping_all;
        col()
            .gap_3()
            .text_sm()
            .child(dialog_title(if stopping { "Stopping agents…" } else { "Agents are still working" }))
            .child(div().text_color(cx.theme().muted_foreground).child(if stopping {
                "Adeline closes once every agent has exited. Agents still running after 5 seconds are stopped."
            } else {
                "Stop all cancels running turns and closes every agent. Finish in background keeps them running after Adeline closes, and their replies are here when you reopen it."
            }))
            .when(!stopping, |column| column.child(dialog_footer()
                .child(self.button("quit-cancel", "Cancel", Action::Close, cx))
                .child(self.button("quit-background", "Finish in background", Action::FinishInBackground, cx))
                .child(self.button("quit-stop-all", "Stop all", Action::QuitStopAll, cx).danger())))
            .into_any_element()
    }
}

struct ModalContent {
    owner: WeakEntity<Adeline>,
    _subscription: Subscription,
}
impl Render for ModalContent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |app, cx| match app.modal {
                Some("about") => col()
                    .text_sm()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("About"),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Adeline"),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(concat!("Version ", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        div()
                            .pt_3()
                            .child("A native workspace for projects and agent conversations."),
                    )
                    .when(app.demo_mode, |column| {
                        column.child(
                            div()
                                .pt_2()
                                .text_color(cx.theme().muted_foreground)
                                .child("Demo changes reset when Adeline restarts."),
                        )
                    })
                    .into_any_element(),
                Some("upgrade") => app.upgrade_dialog(cx),
                Some("prompt") => app.prompt_dialog(cx),
                Some("quit") => app.quit_dialog(cx),
                _ => app.project_modal(cx),
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(super) fn styled_dialog(dialog: Dialog, cx: &App) -> Dialog {
    dialog
        .w(px(400.))
        .close_button(false)
        .bg(cx.theme().popover)
}

pub(super) fn dialog_footer() -> Div {
    row().justify_end().gap_2().pt_1()
}

pub(super) fn dialog_title(title: impl Into<SharedString>) -> Div {
    div()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .child(title.into())
}

pub(super) fn menu_shadow(cx: &App) -> Vec<BoxShadow> {
    // Replaces the popover's default shadow, so it redraws its hairline ring too.
    let ring = cx.theme().foreground.alpha(0.1);
    let ink = |a| hsla(0., 0., 0., a);
    vec![
        BoxShadow::new(px(0.), px(0.), ring)
            .blur_radius(px(0.))
            .spread_radius(px(1.)),
        BoxShadow::new(px(0.), px(4.), ink(0.25))
            .blur_radius(px(6.))
            .spread_radius(px(-2.)),
        BoxShadow::new(px(0.), px(16.), ink(0.45))
            .blur_radius(px(24.))
            .spread_radius(px(-4.)),
    ]
}

pub(super) fn text_input(
    value: &str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut input = InputState::new(window, cx).placeholder(placeholder);
        input.set_value(value.to_owned(), window, cx);
        input
    })
}

pub(super) fn card(rows: Vec<Div>, cx: &App) -> Option<Div> {
    (!rows.is_empty()).then(|| {
        let theme = cx.theme();
        col()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.foreground.alpha(0.025))
            .children(rows.into_iter().enumerate().map(|(ix, row)| {
                row.when(ix > 0, |row| row.border_t_1().border_color(theme.border))
            }))
    })
}
