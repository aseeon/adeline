//! The projects menu: listing, sorting and searching projects, removing closed
//! ones with undo, and opening a folder.
use super::*;
use gpui_kit::base::input;

impl Adeline {
    pub(crate) fn projects_menu(&self, cx: &Context<Self>) -> Popover {
        let owner = cx.weak_entity();
        let content_owner = owner.clone();
        Popover::new("projects")
            .anchor(Anchor::TopCenter)
            .trigger(menu_anchor("projects-anchor"))
            .open(self.menu == Some("projects"))
            .track_focus(&self.project_search.focus_handle(cx))
            .p_0()
            .shadow(views::menu_shadow(cx))
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if *open {
                        app.act(Action::Projects, window, cx);
                    } else {
                        app.close_menu("projects", cx);
                    }
                });
            })
            .content(move |_, window, cx| {
                content_owner.upgrade().map_or_else(
                    || div().into_any_element(),
                    |app| {
                        app.read(cx)
                            .projects_menu_content(&content_owner, window, cx)
                    },
                )
            })
    }

    fn projects_menu_content(
        &self,
        owner: &WeakEntity<Self>,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let query = self.project_search.read(cx).value().trim().to_lowercase();
        let listed = self.menu_projects(&query);
        let (sort_icon, sort_label) = match self.project_sort {
            ProjectSort::Recent => ("clock.svg", "Recent"),
            ProjectSort::Name => ("sort.svg", "A–Z"),
        };
        let sort = Button::new("project-sort")
            .ghost()
            .xsmall()
            .icon(Icon::default().path(sort_icon))
            .label(sort_label)
            .text_color(theme.muted_foreground)
            .accessibility_label(format!("Order: {sort_label}. Change order"))
            .tooltip("Change order")
            .on_click(owner_action(owner, Action::ToggleProjectSort));
        let now = recency::now();
        let overflows = listed.len() as f32 > MENU_ROWS;
        let list = div()
            .id("project-menu-list")
            .role(Role::ListBox)
            .aria_label("Projects")
            .max_h(rems(MENU_ROW_REM * MENU_ROWS))
            .overflow_y_scroll()
            .track_scroll(&self.project_list_scroll)
            .px_1()
            .pt_1()
            .pb_2()
            // Room for the last row to scroll clear of the fade.
            .when(overflows, |list| list.pb(rems(1.25)))
            .children(listed.iter().map(|&ix| self.menu_row(ix, now, owner, cx)))
            .when(listed.is_empty(), |list| {
                list.child(
                    div()
                        .px_2()
                        .py_3()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if query.is_empty() {
                            "No projects yet".to_owned()
                        } else {
                            format!("No projects match \"{query}\"")
                        }),
                )
            });
        let key_owner = owner.clone();
        let delete_owner = owner.clone();
        col()
            .w(dp(580.))
            // The search field binds Delete, and bindings run before key listeners.
            .capture_action(move |_: &input::Delete, window, cx| {
                let handled = delete_owner
                    .update(cx, |app, cx| app.menu_key("delete", window, cx))
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            })
            .capture_key_down(move |event, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let key = event.keystroke.key.clone();
                let handled = key_owner
                    .update(cx, |app, cx| app.menu_key(&key, window, cx))
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            })
            .child(
                div().p_1().child(search_field(
                    Input::new(&self.project_search)
                        .small()
                        .prefix(
                            Icon::default()
                                .path("search.svg")
                                .small()
                                .text_color(theme.muted_foreground),
                        )
                        .suffix(sort),
                    &self.project_search,
                    window,
                    cx,
                )),
            )
            .child(
                div()
                    .relative()
                    .child(list)
                    // The half-shown row fades out, so the list reads as continuing.
                    .when(overflows, |area| {
                        area.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(rems(1.5))
                                .bg(linear_gradient(
                                    180.,
                                    linear_color_stop(theme.popover.opacity(0.), 0.),
                                    linear_color_stop(theme.popover, 1.),
                                )),
                        )
                    }),
            )
            .child(
                row()
                    .gap_1()
                    .p_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("menu-new-project")
                            .ghost()
                            .small()
                            .flex_1()
                            .icon(Icon::default().path("plus.svg"))
                            .label("New project")
                            .on_click(owner_action(owner, Action::AddProject)),
                    )
                    .child(
                        Button::new("menu-open-folder")
                            .ghost()
                            .small()
                            .flex_1()
                            .icon(Icon::default().path("folder.svg"))
                            .label("Open folder")
                            .on_click(owner_action(owner, Action::OpenFolder)),
                    ),
            )
            .into_any_element()
    }

    fn menu_row(&self, ix: usize, now: i64, owner: &WeakEntity<Self>, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let project = &self.projects[ix];
        let id = &project.key();
        let open = self.open_projects[ix];
        let offline = !client::usable(&project.machine, cx);
        let machine = (machines::checked().len() > 1).then(|| machines::name(&project.machine));
        let highlighted = self.project_highlight.as_ref() == Some(id);
        let group = SharedString::from(format!("project-row-{id}"));
        let (fill, letter) =
            theme::project_mark(theme::project_tint(self.project_tints[ix]), false, theme);
        // At rest the trailing slot says where the project stands; under the
        // pointer or keyboard highlight it offers the action: close if open,
        // delete if not.
        let status = if open {
            div()
                .px_1()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .child("Open")
                .into_any_element()
        } else {
            div()
                .child(
                    project
                        .config
                        .opened_at
                        .map(|at| recency::label(at, now))
                        .unwrap_or_default(),
                )
                .into_any_element()
        };
        let action = if open {
            Button::new(SharedString::from(format!("menu-close-{id}")))
                .ghost()
                .xsmall()
                .icon(Icon::default().path("close.svg"))
                .accessibility_label(format!("Close {}", project.config.name))
                .tooltip("Close tab")
                .on_click(owner_action(owner, Action::CloseProject(ix)))
        } else {
            Button::new(SharedString::from(format!("menu-delete-{id}")))
                .ghost()
                .xsmall()
                .icon(Icon::default().path("trash.svg"))
                .accessibility_label(format!("Delete {}", project.config.name))
                .tooltip("Delete project")
                .on_click(owner_action(owner, Action::RemoveClosedProject(id.clone())))
        };
        let select_owner = owner.clone();
        let menu_owner = owner.clone();
        let menu_id = id.clone();
        row()
            .id(SharedString::from(format!("project-menu-{id}")))
            .test_support()
            .group(group.clone())
            .role(Role::ListBoxOption)
            .aria_label(match &machine {
                Some(machine) => format!("{} on {machine}", project.config.name),
                None => project.config.name.clone(),
            })
            .aria_selected(highlighted)
            .h(rems(MENU_ROW_REM))
            .flex_shrink_0()
            .px_2()
            .gap_3()
            .rounded(theme.radius)
            .when(highlighted, |row| row.bg(theme.accent))
            .hover(|style| style.bg(theme.accent))
            .on_click(move |_, window, cx| {
                let _ = select_owner.update(cx, |app, cx| {
                    app.menu = None;
                    app.act(Action::Project(ix), window, cx);
                });
            })
            .child(letter_mark(&project.config.name, fill, letter, dp(24.)))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        row()
                            .gap_2()
                            .min_w_0()
                            .text_sm()
                            .child(div().truncate().child(project.config.name.clone()))
                            .children(machine.map(|machine| {
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(if offline {
                                        format!("{machine} · disconnected")
                                    } else {
                                        machine
                                    })
                            })),
                    )
                    .when(!project.config.directory.as_os_str().is_empty(), |text| {
                        text.child(
                            div()
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(project.config.directory.display().to_string()),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .min_w(rems(1.75))
                    .h(rems(1.75))
                    .flex()
                    .items_center()
                    .justify_end()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .when(highlighted, |status| status.invisible())
                            .group_hover(group.clone(), |style| style.invisible())
                            .child(status),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .flex()
                            .items_center()
                            .when(!highlighted, |slot| slot.invisible())
                            .group_hover(group, |style| style.visible())
                            .child(action),
                    ),
            )
            .context_menu(move |menu, _, _| project_actions(menu, &menu_owner, open, &menu_id))
            .into_any_element()
    }

    fn menu_projects(&self, query: &str) -> Vec<usize> {
        // A project waiting out its undo window is already gone from the user's view.
        let pending = self.pending_removal.as_ref().map(|p| p.id.as_str());
        let mut listed: Vec<usize> = (0..self.projects.len())
            .filter(|&ix| Some(self.projects[ix].key().as_str()) != pending)
            .filter(|&ix| {
                let project = &self.projects[ix];
                let haystack = format!(
                    "{} {}",
                    project.config.name,
                    machines::name(&project.machine)
                )
                .to_lowercase();
                query.split_whitespace().all(|term| haystack.contains(term))
            })
            .collect();
        let name = |ix: usize| self.projects[ix].config.name.to_lowercase();
        match self.project_sort {
            ProjectSort::Recent => listed.sort_by(|&a, &b| {
                let opened = |ix: usize| self.projects[ix].config.opened_at;
                opened(b)
                    .cmp(&opened(a))
                    .then_with(|| name(a).cmp(&name(b)))
            }),
            ProjectSort::Name => listed.sort_by_key(|&ix| name(ix)),
        }
        listed
    }

    fn menu_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // False lets the key reach the search field.
        let query = self.project_search.read(cx).value().trim().to_lowercase();
        let listed = self.menu_projects(&query);
        if listed.is_empty() {
            return false;
        }
        let position = self
            .project_highlight
            .as_ref()
            .and_then(|id| listed.iter().position(|&ix| &self.projects[ix].key() == id));
        match key {
            "down" | "up" => {
                let next = match (key, position) {
                    ("down", Some(at)) => (at + 1) % listed.len(),
                    ("down", None) => 0,
                    (_, Some(at)) => (at + listed.len() - 1) % listed.len(),
                    (_, None) => listed.len() - 1,
                };
                self.project_highlight = Some(self.projects[listed[next]].key());
                self.project_list_scroll.scroll_to_item(next);
                self.header_region.update(cx, |_, cx| cx.notify());
                true
            }
            "enter" if position.is_some() || !query.is_empty() => {
                self.menu = None;
                self.act(Action::Project(listed[position.unwrap_or(0)]), window, cx);
                true
            }
            // With search text, Delete edits the text instead.
            "delete" if query.is_empty() => {
                let Some(at) = position else {
                    return false;
                };
                let ix = listed[at];
                let action = if self.open_projects[ix] {
                    Action::CloseProject(ix)
                } else {
                    Action::RemoveClosedProject(self.projects[ix].key())
                };
                self.act(action, window, cx);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn open_projects_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_search
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.project_highlight = None;
        self.menu = Some("projects");
        window.focus(&self.project_search.focus_handle(cx), cx);
        self.header_region.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(crate) fn search_projects(&mut self, cx: &mut Context<Self>) {
        let query = self.project_search.read(cx).value().trim().to_lowercase();
        self.project_highlight = if query.is_empty() {
            None
        } else {
            self.menu_projects(&query)
                .first()
                .map(|&ix| self.projects[ix].key())
        };
        self.project_list_scroll.scroll_to_item(0);
        self.header_region.update(cx, |_, cx| cx.notify());
    }

    pub(crate) fn remove_closed_project(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.projects.iter().position(|p| p.key() == id) else {
            return;
        };
        if self.open_projects[ix] {
            return;
        }
        // One removal waits at a time; an earlier one is committed now.
        self.commit_pending_removal(window, cx);
        self.removal_generation += 1;
        let generation = self.removal_generation;
        self.pending_removal = Some(PendingRemoval {
            id: id.to_owned(),
            generation,
        });
        self.project_highlight = None;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(UNDO_WINDOW).await;
            let _ = this.update_in(cx, |app, window, cx| {
                if app
                    .pending_removal
                    .as_ref()
                    .is_some_and(|pending| pending.generation == generation)
                {
                    app.commit_pending_removal(window, cx);
                }
            });
        })
        .detach();
        let owner = cx.weak_entity();
        let undo_id = id.to_owned();
        window.push_notification(
            Notification::new()
                .id1::<PendingRemoval>(SharedString::from(id.to_owned()))
                .message(format!("Deleted \"{}\"", self.projects[ix].config.name))
                // Clear of the projects menu, which opens at the top right.
                .placement(Anchor::BottomRight)
                .action(move |_, _, _| {
                    Button::new("undo-project-removal")
                        .label("Undo")
                        .small()
                        .on_click(owner_action(
                            &owner,
                            Action::UndoProjectRemoval(undo_id.clone()),
                        ))
                }),
            cx,
        );
    }

    pub(crate) fn undo_project_removal(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_removal.as_ref().is_some_and(|p| p.id == id) {
            self.pending_removal = None;
            window.remove_notification1::<PendingRemoval>(SharedString::from(id.to_owned()), cx);
            self.header_region.update(cx, |_, cx| cx.notify());
        }
    }

    pub(crate) fn commit_pending_removal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The settings dialog's path: agents stop first, and the dialog appears
        // only if they are still stopping or storage fails.
        let Some(pending) = self.pending_removal.take() else {
            return;
        };
        window.remove_notification1::<PendingRemoval>(SharedString::from(pending.id.clone()), cx);
        let Some(ix) = self.projects.iter().position(|p| p.key() == pending.id) else {
            return;
        };
        if self.open_projects[ix] {
            return;
        }
        if self.demo_mode {
            self.remove_project_at(ix, cx);
            return;
        }
        self.delete_project = Some(pending.id);
        self.modal = Some("delete-project");
        self.confirm_project_delete(cx);
    }

    pub(crate) fn open_folder(
        &mut self,
        machine: &str,
        directory: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self
            .projects
            .iter()
            .position(|p| p.machine == machine && p.config.directory == *directory)
        {
            self.act(Action::Project(ix), window, cx);
            return;
        }
        let path = directory.to_string_lossy().into_owned();
        let name = project_ui::last_name(&path);
        machine.clone_into(&mut self.project_machine);
        self.add_project(name, &path, false, window, cx);
    }

    pub(super) fn close_menu(&mut self, menu: &str, cx: &mut Context<Self>) {
        if self.menu == Some(menu) {
            self.menu = None;
            self.header_region.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }
}

const UNDO_WINDOW: Duration = Duration::from_secs(6);

const MENU_ROW_REM: f32 = 2.5;

// The half row signals that more follow.
const MENU_ROWS: f32 = 5.5;
