//! The unified title bar and the mode rail down the window's left edge.
//!
//! Open projects are full-height cells separated by quiet dividers. Each cell
//! shows the project's letter mark and its name, an accent dot when the
//! project needs the user, and a close button while the pointer is over it.
//! The active tab always shows its full name. When the strip runs out of room,
//! inactive names shorten to three letters, then to the mark alone, and the
//! tabs that still don't fit move into a "+N" menu. Projects that need the user
//! or are still working are the last to shrink. The Projects cell opens a searchable menu of every
//! project, where open projects can be closed and closed ones deleted, with a
//! short window to undo the delete. Right-clicking a tab or a menu row offers
//! the same: rename, and close or delete.
//!
//! The mode rail continues the title bar's app-icon cell down to the bottom of
//! the window: one icon per enabled mode, and the main menu at the foot.
use super::*;
use adeline_core::conversation::ThreadStatus;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::tooltip::Tooltip;
use std::cell::RefCell;
use std::time::Duration;

mod machine_menu;
mod menu;
mod tabs;
pub(super) use tabs::TabFit;
use tabs::{ATTENTION_DOT, FitTab, NEW_TAB_WIDTH, OVERFLOW_WIDTH, TAB_NAME_MAX, short_name};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ProjectSort {
    #[default]
    Recent,
    Name,
}

pub(super) struct PendingRemoval {
    id: String,
    // Matches the timer that commits this removal, so an undone removal's
    // timer finds nothing to do.
    generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TabSize {
    Full,
    Short,
    Letter,
    Hidden,
}

impl Adeline {
    pub(super) fn header(&self, window: &Window, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let bar = theme::bar_colors(theme);
        let open: Vec<usize> = (0..self.projects.len())
            .filter(|&ix| self.open_projects[ix])
            .collect();
        let mut tabs = row().h_full().flex_shrink_0();
        let mut hidden = Vec::new();
        for &ix in &open {
            let project = &self.projects[ix];
            match self.tab_fit.size(&project.key()) {
                TabSize::Hidden if ix != self.project => hidden.push(ix),
                size => tabs = tabs.child(self.project_cell(ix, project, size, &bar, cx)),
            }
        }
        if !hidden.is_empty() {
            tabs = tabs.child(self.overflow_menu(&hidden, &bar, cx));
        }
        tabs = tabs.child(
            row()
                .id("new-project")
                .role(Role::Button)
                .aria_label("New project…")
                .h_full()
                .w(rems(NEW_TAB_WIDTH))
                .justify_center()
                .flex_shrink_0()
                .occlude()
                .border_r_1()
                .border_color(bar.divider)
                .text_color(theme.muted_foreground)
                .hover(|style| style.bg(bar.hover).text_color(theme.foreground))
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::AddProject, window, cx)))
                .child(Icon::default().path("plus.svg").small()),
        );
        // Prepaints once the strip's bounds are current and sizes the tabs for
        // the next frame.
        let fit: Vec<FitTab> = open
            .iter()
            .map(|&ix| {
                let project = &self.projects[ix];
                FitTab {
                    id: project.key(),
                    name: self.tab_name(project),
                    active: ix == self.project,
                    busy: project.busy(),
                    dot: project.attention_count() > 0,
                }
            })
            .collect();
        let tab_fit = std::rc::Rc::clone(&self.tab_fit);
        let measure = canvas(
            move |bounds, window, _| tab_fit.measure(&fit, bounds.size.width, window),
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        let projects = Button::new("project-menu")
            .ghost()
            .icon(Icon::default().path("folder.svg"))
            .label("Projects")
            .dropdown_caret(true)
            .h_full()
            .rounded_none()
            .px(rems(0.875))
            .text_color(theme.muted_foreground);
        // Absolute, so the tabs add nothing to the min width of Kit's bar row,
        // which never shrinks: in flow, many tabs pushed the window controls
        // off-screen and the strip never folded or scrolled.
        let toolbar = row()
            .id("project-toolbar")
            .absolute()
            .inset_0()
            .child(
                row()
                    .h_full()
                    .w(rems(2.75))
                    .flex_shrink_0()
                    .justify_center()
                    .child(titlebar::app_icon(window)),
            )
            .child(self.project_strip(tabs, projects, measure, cx));
        let top = TitleBar::new()
            .h(titlebar::MAIN_HEIGHT)
            .when(!cfg!(target_os = "macos"), |bar| bar.pl_0())
            .border_b_0()
            .bg(theme.title_bar.alpha(titlebar::GLASS))
            .child(toolbar);
        // Windows and Linux: the line under the project tabs runs on under Kit's
        // window controls. It is drawn after the title bar so their hover fill
        // stops above it. It starts past the app icon, which has no left padding
        // on these platforms.
        let line = (!cfg!(target_os = "macos")).then(|| {
            div()
                .absolute()
                .left(rems(2.75))
                .right_0()
                .bottom_0()
                .h(px(1.))
                .bg(theme.border)
        });
        col()
            .relative()
            .flex_shrink_0()
            .child(titlebar::without_text_selection(top))
            .children(line)
    }

    fn project_strip(
        &self,
        tabs: Div,
        projects: Button,
        measure: Canvas<()>,
        cx: &Context<Self>,
    ) -> Div {
        let bar = theme::bar_colors(cx.theme());
        let line = cfg!(target_os = "macos").then(|| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(1.))
                .bg(cx.theme().border)
        });
        row()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .id("projects")
                    .role(Role::TabList)
                    .aria_label("Projects")
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(tabs)
                    .child(measure),
            )
            .when(!machines::remotes().is_empty(), |toolbar| {
                toolbar.child(
                    div()
                        .flex()
                        .h_full()
                        .occlude()
                        .border_l_1()
                        .border_color(bar.divider)
                        .child(self.machine_selector(cx)),
                )
            })
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
            )
            .children(line)
    }

    fn project_cell(
        &self,
        ix: usize,
        project: &Workspace,
        size: TabSize,
        bar: &theme::BarColors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let id = project.key();
        let name = self.tab_name(project);
        let offline = !client::usable(&project.machine, cx);
        let active = ix == self.project;
        let attention = project.attention_count();
        let group = SharedString::from(format!("project-tab-{id}"));
        let (fill, letter) =
            theme::project_mark(theme::project_tint(self.project_tints[ix]), active, theme);
        let shown = match size {
            _ if active => Some(name.clone()),
            TabSize::Full => Some(name.clone()),
            TabSize::Short => Some(short_name(&name)),
            TabSize::Letter | TabSize::Hidden => None,
        };
        let tooltip = (shown.as_ref() != Some(&name)).then(|| name.clone());
        let mut label = if attention == 0 {
            name.clone()
        } else {
            format!("{name} ({attention})")
        };
        if offline {
            label.push_str(", disconnected");
        }
        let owner = cx.weak_entity();
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
            .when(offline, |cell| cell.opacity(0.6))
            .when_some(tooltip, |cell, name| {
                cell.tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
            })
            .on_click(cx.listener(move |app, _, window, cx| {
                app.act(Action::Project(ix), window, cx);
            }))
            .child(letter_mark(&name, fill, letter, rems(1.125), cx))
            .when_some(shown, |cell, shown| {
                cell.child(div().max_w(rems(TAB_NAME_MAX)).truncate().child(shown))
            })
            .when(attention > 0, |cell| cell.child(attention_dot(cx)))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(rems(1.5))
                    .invisible()
                    .group_hover(group, |style| style.visible())
                    .child(self.icon_button(
                        SharedString::from(format!("close-{id}")),
                        "Close project tab",
                        Icon::default().path("close.svg"),
                        Action::CloseProject(ix),
                        cx,
                    )),
            )
            .context_menu(move |menu, _, _| project_actions(menu, &owner, ix, true, &id))
    }

    fn overflow_menu(&self, hidden: &[usize], bar: &theme::BarColors, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let now = recency::now();
        let rows: Vec<_> = hidden
            .iter()
            .map(|&ix| {
                let project = &self.projects[ix];
                let (fill, letter) =
                    theme::project_mark(theme::project_tint(self.project_tints[ix]), false, theme);
                let opened = project
                    .config
                    .opened_at
                    .map(|at| recency::label(at, now))
                    .unwrap_or_default();
                (
                    ix,
                    self.tab_name(project),
                    fill,
                    letter,
                    project.attention_count(),
                    opened,
                )
            })
            .collect();
        let waiting = rows.iter().any(|row| row.4 > 0);
        let count = rows.len();
        let owner = cx.weak_entity();
        let trigger = Button::new("project-overflow")
            .ghost()
            .label(format!("+{count}"))
            .dropdown_caret(true)
            .accessibility_label(format!("{count} more projects"))
            .tooltip("More projects")
            .h_full()
            .w(rems(OVERFLOW_WIDTH))
            .px_2()
            .rounded_none()
            .text_color(theme.muted_foreground)
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _, _| {
                for (ix, name, fill, letter, attention, opened) in rows.iter().cloned() {
                    let owner = owner.clone();
                    let item = PopupMenuItem::element(move |_, cx| {
                        let theme = cx.theme();
                        row()
                            .w_full()
                            .gap_3()
                            .child(letter_mark(&name, fill, letter, rems(1.5), cx))
                            .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                            .child(div().text_xs().map(|status| {
                                if attention > 0 {
                                    status
                                        .text_color(theme.primary)
                                        .child(attention.to_string())
                                } else {
                                    status
                                        .text_color(theme.muted_foreground)
                                        .child(opened.clone())
                                }
                            }))
                    })
                    .on_click(move |_, window, cx| {
                        let _ =
                            owner.update(cx, |app, cx| app.act(Action::Project(ix), window, cx));
                    });
                    menu = menu.item(item);
                }
                menu.min_w(px(240.))
            });
        div()
            .relative()
            .flex()
            .h_full()
            .flex_shrink_0()
            .occlude()
            .border_r_1()
            .border_color(bar.divider)
            .child(trigger)
            .when(waiting, |cell| {
                cell.child(attention_dot(cx).absolute().top(rems(0.5)).right(rems(0.5)))
            })
    }

    pub(super) fn mode_rail(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let waiting = self
            .workspace()
            .threads
            .iter()
            .any(|thread| thread.status == ThreadStatus::Blocked);
        let mut modes = col()
            .id("modes")
            .role(Role::TabList)
            .aria_label("Modes")
            .items_center()
            .gap_1();
        if self.has_open_project() {
            for (section, name, icon_path) in [
                (Section::Chats, "Chats", "chat.svg"),
                (Section::Docs, "Docs", "file.svg"),
                (Section::Workflows, "Workflows", "workflow.svg"),
                (Section::Services, "Services", "service.svg"),
                (Section::Groupchats, "Groupchats", "group.svg"),
                (Section::Issues, "Issues", "flag.svg"),
                (Section::Whiteboard, "Whiteboard", "whiteboard.svg"),
            ] {
                if !config::current().general.features.enabled(section) {
                    continue;
                }
                let active = self.section == section;
                let dot = section == Section::Chats && waiting;
                let label = if dot {
                    format!("{name}, a chat needs you")
                } else {
                    name.to_owned()
                };
                modes = modes.child(
                    div()
                        .relative()
                        .child(
                            Button::new(name)
                                .ghost()
                                .icon(Icon::default().path(icon_path))
                                .size(rems(2.))
                                .p_0()
                                .selected(active)
                                .toggled(active)
                                .accessibility_label(label)
                                .tooltip(name)
                                .text_color(if active {
                                    theme.foreground
                                } else {
                                    theme.muted_foreground
                                })
                                .on_click(cx.listener(move |app, _, window, cx| {
                                    app.act(Action::Section(section), window, cx);
                                })),
                        )
                        .when(active, |mode| {
                            mode.child(
                                div()
                                    .absolute()
                                    .left(rems(-0.375))
                                    .top(rems(0.5))
                                    .bottom(rems(0.5))
                                    .w(px(2.))
                                    .rounded_full()
                                    .bg(theme.foreground),
                            )
                        })
                        .when(dot, |mode| {
                            mode.child(
                                div()
                                    .absolute()
                                    .top(rems(0.1875))
                                    .right(rems(0.1875))
                                    .size(rems(0.6875))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(theme.title_bar)
                                    .bg(theme.primary),
                            )
                        }),
                );
            }
        }
        col()
            .h_full()
            .w(rems(2.75))
            .flex_shrink_0()
            .items_center()
            .p_2()
            .bg(theme.title_bar.alpha(titlebar::GLASS))
            .child(modes)
            .child(div().flex_1())
            .child(
                self.command_popover(
                    "app",
                    Button::new("app-menu")
                        .icon(Icon::default().path("menu.svg"))
                        .accessibility_label("Main menu")
                        .tooltip("Main menu")
                        .small()
                        .ghost(),
                    Anchor::BottomLeft,
                    cx,
                ),
            )
    }

    pub(super) fn tab_name(&self, project: &Workspace) -> String {
        if machines::checked().len() > 1 {
            format!(
                "{} · {}",
                project.config.name,
                machines::name(&project.machine)
            )
        } else {
            project.config.name.clone()
        }
    }
}

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

fn project_actions(
    menu: PopupMenu,
    owner: &WeakEntity<Adeline>,
    ix: usize,
    open: bool,
    id: &str,
) -> PopupMenu {
    let rename = PopupMenuItem::new("Rename project…")
        .on_click(owner_action(owner, Action::RenameProject(ix)));
    let last = if open {
        PopupMenuItem::new("Close project").on_click(owner_action(owner, Action::CloseProject(ix)))
    } else {
        PopupMenuItem::new("Delete project").on_click(owner_action(
            owner,
            Action::RemoveClosedProject(id.to_owned()),
        ))
    };
    menu.item(rename).item(last)
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

fn attention_dot(cx: &App) -> Div {
    div()
        .flex_shrink_0()
        .size(rems(ATTENTION_DOT))
        .rounded_full()
        .bg(cx.theme().primary)
}
