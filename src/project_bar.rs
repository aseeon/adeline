//! The unified title bar and the modes bar beneath it.
//!
//! Open projects are full-height cells separated by quiet dividers. Each cell
//! shows the project's letter mark, its name and its attention count; the
//! count gives way to a close button while the pointer is over the cell.
//! Inactive names fold away, leaving the mark and count, only when the strip
//! cannot fit them. The Projects cell opens a searchable menu of every
//! project, where open projects can be closed and closed ones deleted, with a
//! short window to undo the delete.
use super::*;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use std::cell::Cell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

/// How long a deleted project can be restored before its data is removed.
const UNDO_WINDOW: Duration = Duration::from_secs(6);
/// Height of a projects-menu row, in rem.
const MENU_ROW: f32 = 2.5;
/// Rows the menu shows before scrolling; the half row signals that more follow.
const MENU_ROWS: f32 = 5.5;

/// A deep drop shadow that lifts the projects menu off the dark workspace.
///
/// Replaces the popover's default shadow, so it redraws its hairline ring too.
fn menu_shadow(cx: &App) -> Vec<BoxShadow> {
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

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ProjectSort {
    /// Most recently opened first.
    #[default]
    Recent,
    /// Alphabetical by name.
    Name,
}

/// A deleted project, hidden from the menu until the undo window closes.
pub(super) struct PendingRemoval {
    id: String,
    /// Matches the timer that commits this removal, so an undone removal's
    /// timer finds nothing to do.
    generation: u64,
}

/// Whether inactive tabs fold to their letter mark, measured each frame from
/// the strip's scroll bounds. The width at full names is remembered so the
/// strip unfolds once the window has room again.
#[derive(Default)]
pub(super) struct TabFit {
    compact: Cell<bool>,
    full_width: Cell<Pixels>,
    /// The tab contents `full_width` was measured for.
    signature: Cell<u64>,
}

impl TabFit {
    pub(super) fn compact(&self) -> bool {
        self.compact.get()
    }

    fn measure(&self, strip: &ScrollHandle, signature: u64, window: &mut Window) {
        let available = strip.bounds().size.width;
        if available <= Pixels::ZERO {
            return;
        }
        if self.compact.get() {
            // New tabs or counts invalidate the remembered width: unfold for a
            // frame to measure again.
            if self.signature.get() != signature || self.full_width.get() <= available {
                self.compact.set(false);
                window.refresh();
            }
        } else {
            let overflow = strip.max_offset().x;
            self.full_width.set(available + overflow);
            self.signature.set(signature);
            // Rounding can leave a sub-pixel overflow at an exact fit.
            if overflow > px(1.) {
                self.compact.set(true);
                window.refresh();
            }
        }
    }
}

impl Adeline {
    pub(super) fn header(&self, window: &Window, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let bar = theme::bar_colors(theme);
        let compact = self.tab_fit.compact();
        let mut signature = DefaultHasher::new();
        f32::from(window.rem_size()).to_bits().hash(&mut signature);
        let mut tabs = row().h_full().flex_shrink_0();
        for (ix, project) in self
            .projects
            .iter()
            .enumerate()
            .filter(|(ix, _)| self.open_projects[*ix])
        {
            (
                &project.config.id,
                &project.config.name,
                project.attention_count(),
                ix == self.project,
            )
                .hash(&mut signature);
            tabs = tabs.child(self.project_cell(ix, project, compact, &bar, cx));
        }
        tabs = tabs.child(
            row()
                .id("new-project")
                .role(Role::Button)
                .aria_label("New project…")
                .h_full()
                .px(rems(0.625))
                .flex_shrink_0()
                .occlude()
                .border_r_1()
                .border_color(bar.divider)
                .text_color(theme.muted_foreground)
                .hover(|style| style.bg(bar.hover).text_color(theme.foreground))
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::AddProject, window, cx)))
                .child(Icon::default().path("plus.svg").small()),
        );
        // The strip scrolls when even folded tabs overflow. The canvas after it
        // prepaints once the strip's bounds are current and decides whether
        // names fold.
        let fit = self.tab_fit.clone();
        let strip_handle = self.tab_scroll.clone();
        let signature = signature.finish();
        let measure = canvas(
            move |_, window, _| fit.measure(&strip_handle, signature, window),
            |_, (), _, _| {},
        )
        .absolute()
        .size_0();
        let projects = Button::new("project-menu")
            .ghost()
            .icon(Icon::default().path("folder.svg"))
            .label("Projects")
            .dropdown_caret(true)
            .h_full()
            .rounded_none()
            .px(rems(0.875))
            .text_color(theme.muted_foreground);
        let toolbar = row()
            .id("project-toolbar")
            .size_full()
            .child(
                row()
                    .h_full()
                    .w(rems(2.75))
                    .flex_shrink_0()
                    .justify_center()
                    .border_r_1()
                    .border_color(bar.divider)
                    .child(titlebar::app_icon(window)),
            )
            .child(
                div()
                    .id("projects")
                    .role(Role::TabList)
                    .aria_label("Projects")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    .child(tabs),
            )
            .child(measure)
            .when(
                config::current().general.features.machine_selector,
                |toolbar| {
                    toolbar.child(
                        div()
                            .h_full()
                            .occlude()
                            .border_l_1()
                            .border_color(bar.divider)
                            .child(
                                self.button("machines", "Machines", Action::Machines, cx)
                                    .ghost()
                                    .h_full()
                                    .rounded_none()
                                    .px(rems(0.875)),
                            ),
                    )
                },
            )
            .child(
                // Flex so the popover's trigger wrapper stretches to the bar's height.
                div()
                    .flex()
                    .h_full()
                    .flex_shrink_0()
                    .occlude()
                    .border_l_1()
                    .border_r_1()
                    .border_color(bar.divider)
                    .child(self.projects_menu(projects, cx)),
            );
        let top = TitleBar::new()
            .h(titlebar::MAIN_HEIGHT)
            .when(!cfg!(target_os = "macos"), |bar| bar.pl_0())
            .border_b_0()
            .bg(theme.title_bar)
            .child(toolbar);
        col()
            .flex_shrink_0()
            .child(top)
            .when(self.has_open_project(), |header| {
                header.child(self.modes(cx))
            })
    }

    fn project_cell(
        &self,
        ix: usize,
        project: &Workspace,
        compact: bool,
        bar: &theme::BarColors,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let id = project.config.id.clone();
        let name = project.config.name.clone();
        let active = ix == self.project;
        let attention = project.attention_count();
        let group = SharedString::from(format!("project-tab-{id}"));
        let (fill, letter) =
            theme::project_mark(theme::project_tint(self.project_tints[ix]), active, theme);
        let show_name = !compact || active || self.hovered_tab.as_deref() == Some(id.as_str());
        let label = if attention == 0 {
            name.clone()
        } else {
            format!("{name} ({attention})")
        };
        let hover_id = id.clone();
        row()
            .id(SharedString::from(format!("project-{id}")))
            .group(group.clone())
            .role(Role::Tab)
            .aria_label(label)
            .aria_selected(active)
            .h_full()
            .flex_shrink_0()
            .gap_2()
            .pl(rems(0.625))
            .pr(rems(0.375))
            .text_sm()
            .occlude()
            .border_r_1()
            .border_color(bar.divider)
            .map(|cell| {
                if active {
                    cell.bg(theme.background).text_color(theme.foreground)
                } else {
                    cell.text_color(theme.muted_foreground)
                        .hover(|style| style.bg(bar.hover).text_color(theme.foreground))
                }
            })
            // Folded names unfold under the pointer; unfolded ones need no tracking.
            .on_hover(cx.listener(move |app, hovered: &bool, _, cx| {
                let next = if *hovered && app.tab_fit.compact() {
                    Some(hover_id.clone())
                } else if app.hovered_tab.as_ref() == Some(&hover_id) {
                    None
                } else {
                    return;
                };
                if app.hovered_tab != next {
                    app.hovered_tab = next;
                    app.header_region.update(cx, |_, cx| cx.notify());
                }
            }))
            .on_click(cx.listener(move |app, _, window, cx| {
                app.act(Action::Project(ix), window, cx);
            }))
            .child(letter_mark(&name, fill, letter, rems(1.125), cx))
            .when(show_name, |cell| {
                cell.child(div().max_w(rems(11.25)).truncate().child(name))
            })
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(rems(1.5))
                    .child(
                        count_badge(attention, active, bar, cx)
                            .group_hover(group.clone(), |style| style.invisible()),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .invisible()
                            .group_hover(group, |style| style.visible())
                            .child(self.icon_button(
                                SharedString::from(format!("close-{id}")),
                                "Close project tab",
                                Icon::default().path("close.svg"),
                                Action::CloseProject(ix),
                                cx,
                            )),
                    ),
            )
    }

    fn modes(&self, cx: &Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let mut modes = row()
            .id("modes")
            .role(Role::TabList)
            .aria_label("Modes")
            .overflow_x_scroll()
            .gap(rems(0.125))
            .px_2()
            .py(rems(0.375))
            .bg(theme.background)
            .border_b_1()
            .border_color(theme.border);
        for (section, name, icon_path) in [
            (Section::Chats, "Chats", "chat.svg"),
            (Section::Docs, "Docs", "file.svg"),
            (Section::Workflows, "Workflows", "workflow.svg"),
            (Section::Services, "Services", "service.svg"),
            (Section::Groupchats, "Groupchats", "group.svg"),
            (Section::Issues, "Issues", "flag.svg"),
            (Section::Whiteboard, "Whiteboard", "whiteboard.svg"),
        ] {
            if config::current().general.features.enabled(section) {
                let active = self.section == section;
                modes = modes.child(
                    self.button(name, name, Action::Section(section), cx)
                        .icon(Icon::default().path(icon_path))
                        .ghost()
                        .selected(active)
                        .toggled(active)
                        .when(active, |button| button.text_color(theme.foreground))
                        .when(!active, |button| button.text_color(theme.muted_foreground)),
                );
            }
        }
        modes
    }

    fn projects_menu(&self, trigger: Button, cx: &Context<Self>) -> Popover {
        let owner = cx.weak_entity();
        let content_owner = owner.clone();
        Popover::new("projects")
            .anchor(Anchor::TopRight)
            .trigger(trigger)
            .open(self.menu == Some("projects"))
            .track_focus(&self.project_search.focus_handle(cx))
            .p_0()
            .shadow(menu_shadow(cx))
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if *open {
                        app.act(Action::Projects, window, cx);
                    } else if app.menu == Some("projects") {
                        app.menu = None;
                        app.header_region.update(cx, |_, cx| cx.notify());
                        cx.notify();
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
        let bar = theme::bar_colors(theme);
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
            .max_h(rems(MENU_ROW * MENU_ROWS))
            .overflow_y_scroll()
            .track_scroll(&self.project_list_scroll)
            .px_1()
            .pt_1()
            .pb_2()
            // Room for the last row to scroll clear of the fade.
            .when(overflows, |list| list.pb(rems(1.25)))
            .children(
                listed
                    .iter()
                    .map(|&ix| self.menu_row(ix, now, owner, &bar, cx)),
            )
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
        col()
            .w(rems(22.))
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

    fn menu_row(
        &self,
        ix: usize,
        now: i64,
        owner: &WeakEntity<Self>,
        bar: &theme::BarColors,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let project = &self.projects[ix];
        let id = &project.config.id;
        let open = self.open_projects[ix];
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
        row()
            .id(SharedString::from(format!("project-menu-{id}")))
            .group(group.clone())
            .role(Role::ListBoxOption)
            .aria_label(project.config.name.clone())
            .aria_selected(highlighted)
            .h(rems(MENU_ROW))
            .flex_shrink_0()
            .px_2()
            .gap_3()
            .rounded(theme.radius)
            .when(highlighted, |row| row.bg(bar.menu_hover))
            .hover(|style| style.bg(bar.menu_hover))
            .on_click(move |_, window, cx| {
                let _ = select_owner.update(cx, |app, cx| {
                    app.menu = None;
                    app.act(Action::Project(ix), window, cx);
                });
            })
            .child(letter_mark(
                &project.config.name,
                fill,
                letter,
                rems(1.5),
                cx,
            ))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .truncate()
                            .child(project.config.name.clone()),
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
            .into_any_element()
    }

    /// Projects the menu lists for `query`, in the chosen order. A project
    /// waiting out its undo window is already gone from the user's view.
    fn menu_projects(&self, query: &str) -> Vec<usize> {
        let pending = self.pending_removal.as_ref().map(|p| p.id.as_str());
        let mut listed: Vec<usize> = (0..self.projects.len())
            .filter(|&ix| Some(self.projects[ix].config.id.as_str()) != pending)
            .filter(|&ix| {
                query.is_empty() || self.projects[ix].config.name.to_lowercase().contains(query)
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

    /// Keyboard control of the open menu. Returns whether the key was used,
    /// so everything else still reaches the search field.
    fn menu_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let query = self.project_search.read(cx).value().trim().to_lowercase();
        let listed = self.menu_projects(&query);
        if listed.is_empty() {
            return false;
        }
        let position = self.project_highlight.as_ref().and_then(|id| {
            listed
                .iter()
                .position(|&ix| &self.projects[ix].config.id == id)
        });
        match key {
            "down" | "up" => {
                let next = match (key, position) {
                    ("down", Some(at)) => (at + 1) % listed.len(),
                    ("down", None) => 0,
                    (_, Some(at)) => (at + listed.len() - 1) % listed.len(),
                    (_, None) => listed.len() - 1,
                };
                self.project_highlight = Some(self.projects[listed[next]].config.id.clone());
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
                    Action::RemoveClosedProject(self.projects[ix].config.id.clone())
                };
                self.act(action, window, cx);
                true
            }
            _ => false,
        }
    }

    pub(super) fn open_projects_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_search
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.project_highlight = None;
        self.menu = Some("projects");
        window.focus(&self.project_search.focus_handle(cx), cx);
        self.header_region.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Typing moves the highlight to the best match, so Enter opens it.
    pub(super) fn search_projects(&mut self, cx: &mut Context<Self>) {
        let query = self.project_search.read(cx).value().trim().to_lowercase();
        self.project_highlight = if query.is_empty() {
            None
        } else {
            self.menu_projects(&query)
                .first()
                .map(|&ix| self.projects[ix].config.id.clone())
        };
        self.project_list_scroll.scroll_to_item(0);
        self.header_region.update(cx, |_, cx| cx.notify());
    }

    /// Hides a closed project and deletes it once the undo window passes.
    pub(super) fn remove_closed_project(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.projects.iter().position(|p| p.config.id == id) else {
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

    pub(super) fn undo_project_removal(
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

    /// Deletes the pending project for real, through the same path as the
    /// settings dialog: agents stop first, and the dialog appears only if they
    /// are still stopping or storage fails.
    pub(super) fn commit_pending_removal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_removal.take() else {
            return;
        };
        window.remove_notification1::<PendingRemoval>(SharedString::from(pending.id.clone()), cx);
        let Some(ix) = self.projects.iter().position(|p| p.config.id == pending.id) else {
            return;
        };
        if self.open_projects[ix] {
            return;
        }
        if self.demo_mode {
            self.remove_project_at(ix, cx);
            return;
        }
        self.delete_project = Some(ix);
        self.modal = Some("delete-project");
        self.confirm_project_delete(cx);
    }

    /// Opens the project for `directory`, creating one named after the folder
    /// when none uses it yet.
    pub(super) fn open_folder(
        &mut self,
        directory: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self
            .projects
            .iter()
            .position(|p| p.config.directory == *directory)
        {
            self.act(Action::Project(ix), window, cx);
            return;
        }
        let name = directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Err(error) = self.add_project(name, &directory.to_string_lossy(), window, cx) {
            window.push_notification(error, cx);
        }
    }
}

/// A click handler that runs `action` on the shell, for elements built outside
/// its render context.
fn owner_action(
    owner: &WeakEntity<Adeline>,
    action: Action,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let owner = owner.clone();
    move |_, window, cx| {
        cx.stop_propagation();
        let _ = owner.update(cx, |app, cx| app.act(action.clone(), window, cx));
    }
}

fn letter_mark(name: &str, fill: Hsla, letter: Hsla, size: Rems, cx: &App) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(size)
        .rounded(cx.theme().radius)
        .bg(fill)
        .text_color(letter)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .child(
            name.chars()
                .next()
                .map(|first| first.to_uppercase().collect::<String>())
                .unwrap_or_default(),
        )
}

/// A project's attention count. Zero stays visible but quiet, so every tab
/// keeps the same shape.
fn count_badge(count: usize, active: bool, bar: &theme::BarColors, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .justify_center()
        .min_w(rems(1.))
        .h(rems(1.))
        .px_1()
        .rounded_full()
        .text_xs()
        .map(|badge| {
            if count == 0 {
                badge
                    .border_1()
                    .border_color(bar.divider)
                    .text_color(theme.muted_foreground)
                    .opacity(0.7)
            } else if active {
                badge.bg(theme.sidebar_primary).text_color(theme.foreground)
            } else {
                badge.bg(theme.secondary).text_color(theme.muted_foreground)
            }
        })
        .child(count.to_string())
}
