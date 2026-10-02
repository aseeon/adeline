use super::*;
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
            .when_some(thread, |header, thread| {
                header.child(chat_render::context_meter(thread.context, cx).mr_2())
            })
            .when_some(thread, |header, thread| {
                let done = matches!(thread.status.as_str(), "completed" | "archived");
                let archived = thread.status == "archived";
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
        let composer_height = self.composer_height.clone();
        let transcript = self.transcript.clone();
        let measure = canvas(
            move |bounds, _, cx| {
                if (composer_height.get() - bounds.size.height).abs() > px(0.5) {
                    composer_height.set(bounds.size.height);
                    transcript.update(cx, |_, cx| cx.notify());
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full();
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
                    .child(self.composer_region.clone())
                    .child(measure),
            )
            .into_any_element()
    }

    pub(super) fn activity_content(&self, cx: &Context<Self>) -> Div {
        let mut panel = col().p_3().gap_3();
        if let Some(ix) = self.selected {
            let thread = &self.workspace().threads[ix];
            panel = panel.child(
                div()
                    .text_sm()
                    .child(format!("{} events", thread.activity.len())),
            );
            for (ix, event) in thread.activity.iter().enumerate() {
                panel = panel.child(
                    col()
                        .gap_2()
                        .child(
                            self.button(
                                SharedString::from(format!("activity-{}-{ix}", thread.id)),
                                event.title.clone(),
                                Action::Event(ix),
                                cx,
                            )
                            .ghost()
                            .w_full(),
                        )
                        .when(self.expanded_event == Some(ix), |column| {
                            column.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if event.detail.is_empty() {
                                        "Completed in the sample workspace.".to_owned()
                                    } else {
                                        event.detail.clone()
                                    }),
                            )
                        }),
                );
            }
        } else {
            panel = panel.child("Start a chat to see agent activity.");
        }
        panel
    }

    fn command_entries(&self, menu: &'static str) -> (&'static str, Vec<(String, Action)>) {
        let mut entries: Vec<(String, Action)> = Vec::new();
        let title = match menu {
            "projects" => {
                entries.extend(
                    self.projects
                        .iter()
                        .enumerate()
                        .map(|(ix, project)| (project.config.name.clone(), Action::Project(ix))),
                );
                entries.push(("New project…".into(), Action::AddProject));
                "Projects"
            }
            "agents" | "agent" => {
                entries.extend(self.agent_catalog.entries.iter().map(|entry| {
                    (
                        entry.definition.name.clone(),
                        Action::Agent(entry.id.clone()),
                    )
                }));
                entries.push(("Add an agent…".into(), Action::AddAgent));
                entries.push(("Agent settings…".into(), Action::AppSettings));
                if menu == "agent" && self.demo_mode {
                    entries.extend([
                        ("Standard speed".into(), Action::Speed(0)),
                        ("Fast speed".into(), Action::Speed(1)),
                    ]);
                }
                "Agents"
            }
            "machines" => {
                if !config::current().general.features.machine_selector {
                    return ("Machines", entries);
                }
                entries.extend(
                    MACHINES
                        .iter()
                        .enumerate()
                        .take(if self.demo_mode { MACHINES.len() } else { 1 })
                        .map(|(ix, machine)| {
                            (
                                if self.demo_mode {
                                    format!("{} ({})", machine.name, machine.kind)
                                } else {
                                    "Local machine".into()
                                },
                                Action::Machine(ix),
                            )
                        }),
                );
                "Machines"
            }
            "files" => {
                entries.extend([
                    ("Add a file…".into(), Action::AddFile),
                    ("Add a directory…".into(), Action::AddDirectory),
                ]);
                "Attach context"
            }
            "mode-settings" => {
                entries.extend(
                    self.mode_options(Section::Chats)
                        .into_iter()
                        .map(|(label, _, _, action)| (label.into(), action)),
                );
                entries.push(("Full chat settings…".into(), Action::ConfigureModeSettings));
                "Chat settings"
            }
            _ => {
                entries.push(("Add an agent…".into(), Action::AddAgent));
                if self.has_open_project() {
                    entries.push(("Project settings…".into(), Action::Settings));
                }
                entries.extend([
                    ("Settings…".into(), Action::AppSettings),
                    ("About Adeline…".into(), Action::About),
                    ("Quit Adeline".into(), Action::QuitApp),
                ]);
                "Adeline"
            }
        };
        (title, entries)
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
        let (_, entries) = self.command_entries(menu);
        let toggles = if menu == "mode-settings" {
            self.mode_options(Section::Chats)
        } else {
            Vec::new()
        };
        let state = self.command_popup.clone();
        let errors = if matches!(menu, "agent" | "agents") {
            self.agent_catalog.errors.join("\n")
        } else {
            String::new()
        };
        component::popover::Popover::new(menu)
            .anchor(anchor)
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
                        let action = match menu {
                            "agents" => Action::Agents,
                            "agent" => Action::AgentMenu,
                            "projects" => Action::Projects,
                            "mode-settings" => Action::ModeSettings,
                            "files" => Action::InsertFiles,
                            _ => Action::AppMenu,
                        };
                        app.act(action, window, cx);
                    } else if app.menu == Some(menu) {
                        app.menu = None;
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
        if matches!(
            menu,
            "agent" | "agents" | "app" | "mode-settings" | "model" | "effort" | "files"
        ) {
            let state = cx.new(|cx| CommandState::new(window, cx));
            window.focus(&state.focus_handle(cx), cx);
            self.command_popup = Some(state);
            self.menu = Some(menu);
            self.header_region.update(cx, |_, cx| cx.notify());
            self.control_pane.update(cx, |_, cx| cx.notify());
            self.composer_region.update(cx, |_, cx| cx.notify());
            cx.notify();
            return;
        }
        let (title, entries) = self.command_entries(menu);
        self.menu = Some(menu);
        let entries = std::rc::Rc::new(entries);
        let state = cx.new(|cx| CommandState::new(window, cx));
        let initial_focus = state.focus_handle(cx);
        let owner = cx.weak_entity();
        let closing_owner = owner.clone();
        let errors = if matches!(menu, "agent" | "agents") {
            self.agent_catalog.errors.join("\n")
        } else {
            String::new()
        };
        window.open_dialog(cx, move |dialog, _, cx| {
            let owner = owner.clone();
            let entries_for_action = entries.clone();
            let cancel_owner = closing_owner.clone();
            styled_dialog(dialog, cx)
                .title(dialog_title(title))
                .on_ok(|_, _, _| false)
                .child(
                    col()
                        .gap_2()
                        .capture_action(move |_: &Cancel, window, cx| {
                            window.close_dialog(cx);
                            let _ = cancel_owner.update(cx, |app, cx| {
                                app.menu = None;
                                cx.notify();
                            });
                            cx.stop_propagation();
                        })
                        .when(!errors.is_empty(), |column| column.child(errors.clone()))
                        .child(
                            Command::new(&state)
                                .placeholder("Search")
                                .items(
                                    entries
                                        .iter()
                                        .map(|(label, _)| CommandItem::new().label(label.clone())),
                                )
                                .on_confirm(move |path, window, cx| {
                                    if let Some((_, action)) = entries_for_action.get(path.row) {
                                        window.close_dialog(cx);
                                        let _ = owner.update(cx, |app, cx| {
                                            app.menu = None;
                                            app.act(action.clone(), window, cx);
                                        });
                                    }
                                }),
                        ),
                )
                .on_close({
                    let owner = closing_owner.clone();
                    move |_, _, cx| {
                        let _ = owner.update(cx, |app, cx| {
                            app.menu = None;
                            cx.notify();
                        });
                    }
                })
        });
        window.focus(&initial_focus, cx);
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
        let width = px(if self.modal == Some("about") { 320. } else { 400. });
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
                        app.modal = None;
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
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("About"))
                    .child(div().text_base().font_weight(FontWeight::MEDIUM).child("Adeline"))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(concat!("Version ", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(div().pt_3().child("A native workspace for projects and agent conversations."))
                    .when(app.demo_mode, |column| {
                        column.child(
                            div()
                                .pt_2()
                                .text_color(cx.theme().muted_foreground)
                                .child("Demo changes reset when Adeline restarts."),
                        )
                    })
                    .into_any_element(),
                _ => app.project_modal(cx),
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

/// Adeline's dialog surface: popover-colored like the app's menus, and closed
/// with Escape or Cancel rather than a corner button.
pub(super) fn styled_dialog(dialog: Dialog, cx: &App) -> Dialog {
    dialog.w(px(400.)).close_button(false).bg(cx.theme().popover)
}

/// A dialog heading sized to the app chrome rather than Kit's large title.
pub(super) fn dialog_title(title: impl Into<SharedString>) -> Div {
    div().text_sm().font_weight(FontWeight::MEDIUM).child(title.into())
}
