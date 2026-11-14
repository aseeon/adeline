//! The Chats layout and its chat header view, command popovers (the menus),
//! modal dialogs (including quit, SSH prompt and upgrade), and UI helpers
//! shared across windows.
use super::*;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::switch::Switch;

type MenuGroup = (Option<&'static str>, Vec<(String, Action)>);

const MENU_ITEM: f32 = 0.925;
const MENU_HEADING: f32 = 0.8;

fn menu_item(
    label: &str,
    action: &Action,
    toggles: &[(&'static str, &'static str, bool, Action)],
    owner: &WeakEntity<Adeline>,
) -> CommandItem {
    let item = CommandItem::new().label(label.to_owned());
    if matches!(action, Action::AppSettings | Action::ConfigureModeSettings) {
        let label = label.to_owned();
        return item.child(move |_, cx| {
            row()
                .w_full()
                .gap_3()
                .text_size(rems(MENU_ITEM))
                .child(div().flex_1().child(label.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(conversation_ui::shortcut("Ctrl+,")),
                )
        });
    }
    let Some((label, _, checked, action)) = toggles.iter().find(|(name, _, _, _)| *name == label)
    else {
        let label = label.to_owned();
        return item.child(move |_, _| div().text_size(rems(MENU_ITEM)).child(label.clone()));
    };
    let (label, checked) = (*label, *checked);
    let action = action.clone();
    let owner = owner.clone();
    item.child(move |_, cx| {
        let owner = owner.clone();
        let action = action.clone();
        row()
            .w_full()
            .gap_3()
            .text_size(rems(MENU_ITEM))
            .child(div().flex_1().child(label))
            .child(
                Switch::new(label)
                    .small()
                    .color(cx.theme().info)
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
}

fn surface_fade(surface: Hsla, angle: f32) -> Div {
    div()
        .absolute()
        .h(rems(chat::HEADER_FADE))
        .bg(linear_gradient(
            angle,
            linear_color_stop(surface.opacity(0.9), 0.),
            linear_color_stop(surface.alpha(0.), 1.),
        ))
}

fn searchable(menu: &str) -> bool {
    !matches!(menu, "app" | "files" | "mode-settings")
}

pub(super) struct ChatHeader(pub WeakEntity<Adeline>);
impl Render for ChatHeader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |app, cx| {
                div()
                    .size_full()
                    .child(app.chat_header(cx))
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Adeline {
    fn chat_header(&self, cx: &Context<Self>) -> Div {
        let title = self.selected.map_or_else(
            || "New chat".to_owned(),
            |ix| self.workspace().threads[ix].title.clone(),
        );
        row()
            .h(panes::dp(chat::PANE_HEADER))
            .px(panes::dp(10.))
            .gap(panes::dp(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.chat_chooser(&title, cx)),
            )
            .children(self.selected.and_then(|_| self.todo_button(cx)))
    }

    pub(super) fn chats(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let thread = self.selected.map(|ix| &self.workspace().threads[ix]);
        let surface = chat::chat_surface(self.section, cx);
        let scrollbar = theme::SCROLLBAR_TRACK;
        let composer_height = std::rc::Rc::clone(&self.composer_height);
        let transcript = self.transcript.clone();
        let owner = cx.weak_entity();
        let measure = chat::size_probe(move |size, cx| {
            if (composer_height.get() - size.height).abs() > px(0.5) {
                composer_height.set(size.height);
                transcript.update(cx, |_, cx| cx.notify());
                let _ = owner.update(cx, |_, cx| cx.notify());
            }
        });
        let header = (self.section == Section::Docs).then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right(scrollbar)
                .pl(scrollbar)
                .bg(surface)
                .rounded_tl(panes::dp(14.))
                .child(chat::region(
                    &self.chat_header,
                    StyleRefinement::default()
                        .w_full()
                        .h(panes::dp(chat::PANE_HEADER)),
                    window,
                ))
        });
        let header_height = if header.is_some() {
            panes::dp(chat::PANE_HEADER)
        } else {
            px(0.)
        };
        let fade = surface_fade(surface, 180.)
            .top(header_height)
            .left_0()
            .right(scrollbar);
        div()
            .id("conversation-panel")
            .test_support()
            .role(Role::Group)
            .aria_label("Conversation panel")
            .relative()
            .size_full()
            .min_w_0()
            .child(self.transcript.clone())
            .child(fade)
            .children(header)
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right(scrollbar)
                    .pl(scrollbar)
                    .bg(surface)
                    .when(self.section != Section::Chats, |region| {
                        region.rounded_bl(panes::dp(14.)).pb(panes::dp(18.))
                    })
                    .child(surface_fade(surface, 0.).bottom_full().left_0().right_0())
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
                    .child(panes::glide(
                        self.composer_region.clone(),
                        self.section == Section::Chats,
                        &self.composer_glide,
                    ))
                    .child(measure),
            )
            .into_any_element()
    }

    fn command_entries(&self, menu: &'static str) -> Vec<MenuGroup> {
        if menu == "mode-settings" {
            let mut groups: Vec<MenuGroup> = self
                .mode_options(self.section)
                .into_iter()
                .map(|(heading, options)| {
                    let entries = options
                        .into_iter()
                        .map(|(label, _, _, action)| (label.into(), action))
                        .collect();
                    (heading, entries)
                })
                .collect();
            let all = if self.section == Section::Docs {
                "All docs settings"
            } else {
                "All chat settings"
            };
            groups.push((None, vec![(all.into(), Action::ConfigureModeSettings)]));
            return groups;
        }
        let mut entries: Vec<(String, Action)> = Vec::new();
        match menu {
            "agents" | "agent" => {
                entries.extend(self.agent_catalog().entries.iter().map(|entry| {
                    (
                        entry.definition.name.clone(),
                        Action::Agent(entry.id.clone()),
                    )
                }));
                entries.push(("Add an agent".into(), Action::AddAgent));
                entries.push(("Agent settings".into(), Action::AgentSettings));
                if menu == "agent" && self.demo_mode {
                    entries.extend([
                        ("Standard speed".into(), Action::Speed(0)),
                        ("Fast speed".into(), Action::Speed(1)),
                    ]);
                }
            }
            "files" => {
                entries.extend([
                    ("Add a file".into(), Action::AddFile),
                    ("Add a directory".into(), Action::AddDirectory),
                ]);
            }
            _ => {
                entries.push(("Add an agent".into(), Action::AddAgent));
                if !self.agent_catalog().entries.is_empty() {
                    entries.push(("Edit an agent".into(), Action::EditAgent));
                }
                if !self.demo_mode {
                    entries.push(("Stop all agents".into(), Action::StopAll));
                }
                let mut settings = Vec::new();
                if self.has_open_project() {
                    settings.push(("Project settings".into(), Action::Settings));
                }
                settings.push(("General settings".into(), Action::AppSettings));
                return vec![
                    (Some("Agents"), entries),
                    (Some("Settings"), settings),
                    (
                        Some("Adeline"),
                        vec![
                            ("About Adeline".into(), Action::About),
                            ("Quit Adeline".into(), Action::QuitApp),
                        ],
                    ),
                ];
            }
        }
        vec![(None, entries)]
    }

    pub(super) fn refocus_after_menu(&self, window: &mut Window, cx: &mut App) {
        let menu_focused = self
            .command_popup
            .as_ref()
            .is_some_and(|state| state.focus_handle(cx).is_focused(window));
        if menu_focused || window.focused(cx).is_none() {
            window.focus(&self.composer.focus_handle(cx), cx);
        }
    }

    pub(super) fn menu_popover(
        &self,
        menu: &'static str,
        trigger: Button,
        anchor: Anchor,
        state: Option<&Entity<CommandState>>,
        cx: &App,
    ) -> component::popover::Popover {
        component::popover::Popover::new(menu)
            .anchor(anchor)
            .trigger(trigger)
            .open(self.menu == Some(menu))
            .p_0()
            .shadow(menu_shadow(cx))
            .when_some(state, |popover, state| {
                popover.track_focus(&state.focus_handle(cx))
            })
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
        let toggles: Vec<_> = if menu == "mode-settings" {
            self.mode_options(self.section)
                .into_iter()
                .flat_map(|(_, options)| options)
                .collect()
        } else {
            Vec::new()
        };
        let state = self.command_popup.clone();
        let errors = if matches!(menu, "agent" | "agents") {
            self.agent_catalog().errors.join("\n")
        } else {
            String::new()
        };
        self.menu_popover(menu, trigger, anchor, state.as_ref(), cx)
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if *open {
                        app.act(Action::open_menu(menu), window, cx);
                    } else if app.menu == Some(menu) {
                        app.menu = None;
                        app.refocus_after_menu(window, cx);
                        app.header_region.update(cx, |_, cx| cx.notify());
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
                        let command = Command::new(state)
                            .bordered(false)
                            .searchable(searchable(menu))
                            .max_h(rems(24.))
                            .placeholder("Search");
                        let command = entries.iter().enumerate().fold(
                            command,
                            |command, (ix, (heading, group))| {
                                let heading = heading.map(|heading| {
                                    CommandItem::new().label(heading).disabled(true).child(
                                        move |_, _| {
                                            div().text_size(rems(MENU_HEADING)).child(heading)
                                        },
                                    )
                                });
                                let items = CommandGroup::new().items(heading.into_iter().chain(
                                    group.iter().map(|(label, action)| {
                                        menu_item(label, action, &toggles, &owner)
                                    }),
                                ));
                                if ix > 0 { command.separator() } else { command }.group(items)
                            },
                        );
                        column.child(command.on_confirm(move |path, window, cx| {
                            if let Some((_, action)) =
                                entries.get(path.section).and_then(|(heading, group)| {
                                    group.get(path.row.checked_sub(usize::from(heading.is_some()))?)
                                })
                            {
                                if !action.keeps_menu_open() {
                                    popover.update(cx, |state, cx| state.dismiss(window, cx));
                                }
                                let _ = owner.update(cx, |app, cx| {
                                    app.act(action.clone(), window, cx);
                                });
                            }
                        }))
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
        RenderOnce::render(
            Command::new(&state).searchable(searchable(menu)),
            window,
            cx,
        );
        window.focus(&state.focus_handle(cx), cx);
        self.command_popup = Some(state);
        self.menu = Some(menu);
        self.header_region.update(cx, |_, cx| cx.notify());
        self.composer_region.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn open_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modals_opened += 1;
        let opened = self.modals_opened;
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
                opened,
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
                            Some("doc-new") => Some(Action::Docs(docs::Act::Create)),
                            Some("doc-rename") => Some(Action::Docs(docs::Act::ConfirmRename)),
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
                        match app.modal.take() {
                            Some("prompt") => client::answer_prompt(None, cx),
                            Some("doc-unsaved") => app.docs_cancel(),
                            _ => {}
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
        if matches!(self.modal, Some("doc-new" | "doc-rename")) {
            self.docs_focus_path(window, cx);
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
        dialog_content(format!("Upgrade {name}?"))
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
                .test_support()
                .role(Role::Group)
                .aria_label(title.clone())
                .gap_3()
                .child(dialog_title(title))
                .child(
                    div()
                        .id("ssh-prompt-text")
                        .test_support()
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

// Kit keeps a closed dialog drawn while it fades out. A newer modal may be open by
// then, so a stale dialog draws nothing in place of the newer modal's content.
struct ModalContent {
    owner: WeakEntity<Adeline>,
    opened: usize,
    _subscription: Subscription,
}
impl Render for ModalContent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let opened = self.opened;
        self.owner
            .update(cx, |app, cx| match app.modal {
                _ if app.modals_opened != opened => div().into_any_element(),
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
                Some(modal) if modal.starts_with("doc-") => app.docs_dialog(cx),
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

pub(super) fn dialog_content(
    label: impl Into<SharedString>,
) -> impl IntoElement + ParentElement + Styled {
    col()
        .id("dialog-content")
        .test_support()
        .role(Role::Group)
        .aria_label(label.into())
}

pub(super) fn dialog_title(title: impl Into<SharedString>) -> Div {
    div()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .child(title.into())
}

pub(super) fn menu_shadow(cx: &App) -> Vec<BoxShadow> {
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
