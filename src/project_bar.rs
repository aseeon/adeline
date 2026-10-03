//! The unified title bar and the mode rail down the window's left edge.
//!
//! Open projects are full-height cells separated by quiet dividers. Each cell
//! shows the project's letter mark, its name and its attention count; the
//! count gives way to a close button while the pointer is over the cell.
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
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::tooltip::Tooltip;
use std::cell::RefCell;
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

/// How much of a project tab shows. As the strip runs out of room, inactive
/// tabs step down one at a time: full name, three letters, the mark alone,
/// then into the overflow menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TabSize {
    Full,
    Short,
    Letter,
    Hidden,
}

/// A tab's fixed parts in rem, matching `project_cell`: padding, mark, gap
/// and count slot. A name adds its own gap and is capped in width.
const TAB_CHROME: f32 = 0.625 + 1.125 + 0.5 + 1.5 + 0.375;
const TAB_NAME_GAP: f32 = 0.5;
const TAB_NAME_MAX: f32 = 11.25;
/// Widths of the new-project and overflow cells, in rem.
const NEW_TAB_WIDTH: f32 = 2.25;
const OVERFLOW_WIDTH: f32 = 4.25;

/// An open tab, as the fit sees it.
pub(super) struct FitTab {
    id: String,
    name: String,
    active: bool,
    busy: bool,
}

/// A tab's width at full, short and letter size.
struct Measured {
    widths: [Pixels; 3],
    active: bool,
    busy: bool,
}

/// The size of each open tab, measured each frame from the strip's width and
/// applied on the next.
#[derive(Default)]
pub(super) struct TabFit {
    sizes: RefCell<Vec<(String, TabSize)>>,
}

impl TabFit {
    fn size(&self, id: &str) -> TabSize {
        self.sizes
            .borrow()
            .iter()
            .find(|(tab, _)| tab == id)
            .map_or(TabSize::Full, |(_, size)| *size)
    }

    fn measure(&self, tabs: &[FitTab], available: Pixels, window: &mut Window) {
        if available <= Pixels::ZERO {
            return;
        }
        let rem = window.rem_size();
        let font_size = rems(0.875).to_pixels(rem);
        let style = window.text_style();
        let chrome = rems(TAB_CHROME).to_pixels(rem) + px(1.);
        let named = |label: String| {
            let run = style.to_run(label.len());
            let text = window
                .text_system()
                .shape_line(label.into(), font_size, &[run], None)
                .width;
            chrome + rems(TAB_NAME_GAP).to_pixels(rem) + text.min(rems(TAB_NAME_MAX).to_pixels(rem))
        };
        let measured: Vec<Measured> = tabs
            .iter()
            .map(|tab| Measured {
                widths: [
                    named(tab.name.clone()),
                    named(short_name(&tab.name)),
                    chrome,
                ],
                active: tab.active,
                busy: tab.busy,
            })
            .collect();
        let room = available - rems(NEW_TAB_WIDTH).to_pixels(rem) - px(1.);
        let more = rems(OVERFLOW_WIDTH).to_pixels(rem) + px(1.);
        let sizes: Vec<_> = tabs
            .iter()
            .map(|tab| tab.id.clone())
            .zip(fit_tabs(&measured, room, more))
            .collect();
        if *self.sizes.borrow() != sizes {
            *self.sizes.borrow_mut() = sizes;
            window.refresh();
        }
    }
}

/// Steps tabs down until they fit `room`. Each step reaches every quiet tab
/// before any busy one, rightmost first, and never the active tab. `more` is
/// the overflow cell, counted once a tab is hidden.
fn fit_tabs(tabs: &[Measured], room: Pixels, more: Pixels) -> Vec<TabSize> {
    let fits = |sizes: &[TabSize]| {
        let mut total = Pixels::ZERO;
        for (tab, size) in tabs.iter().zip(sizes) {
            total += match size {
                TabSize::Hidden => continue,
                size => tab.widths[*size as usize],
            };
        }
        if sizes.contains(&TabSize::Hidden) {
            total += more;
        }
        total <= room
    };
    let mut sizes = vec![TabSize::Full; tabs.len()];
    for step in [TabSize::Short, TabSize::Letter, TabSize::Hidden] {
        for busy in [false, true] {
            for ix in (0..tabs.len()).rev() {
                if fits(&sizes) {
                    return sizes;
                }
                if !tabs[ix].active && tabs[ix].busy == busy {
                    sizes[ix] = step;
                }
            }
        }
    }
    sizes
}

/// The first three letters and an ellipsis, or the whole name when that
/// would be no shorter.
fn short_name(name: &str) -> String {
    if name.chars().count() <= 4 {
        return name.to_owned();
    }
    format!("{}…", name.chars().take(3).collect::<String>())
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
            match self.tab_fit.size(&project.config.id) {
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
                    id: project.config.id.clone(),
                    name: project.config.name.clone(),
                    active: ix == self.project,
                    busy: project.busy(),
                }
            })
            .collect();
        let tab_fit = self.tab_fit.clone();
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

    /// The project tabs, the machine selector and the Projects menu. On macOS,
    /// where the window controls sit at the left, the strip draws the line
    /// under the title bar itself.
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
        let id = project.config.id.clone();
        let name = project.config.name.clone();
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
        let label = if attention == 0 {
            name.clone()
        } else {
            format!("{name} ({attention})")
        };
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
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(rems(1.5))
                    .child(
                        count_badge(attention, bar, cx)
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
            .context_menu(move |menu, _, _| project_actions(menu, &owner, ix, true, &id))
    }

    /// The "+N" cell holding the tabs that don't fit, and its menu. A dot on
    /// it means a hidden project needs the user.
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
                    project.config.name.clone(),
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
                cell.child(
                    div()
                        .absolute()
                        .top(rems(0.5))
                        .right(rems(0.5))
                        .size(rems(0.375))
                        .rounded_full()
                        .bg(theme.primary),
                )
            })
    }

    /// The mode rail: the width and color of the app-icon cell above it, from
    /// the title bar to the bottom of the window. Each enabled mode is an icon
    /// named by its tooltip; the chosen one is filled and marked on its left
    /// edge, and Chats carries a dot while a chat waits on the user.
    pub(super) fn mode_rail(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let waiting = self
            .workspace()
            .threads
            .iter()
            .any(|thread| thread.status == "blocked");
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

    fn menu_row(&self, ix: usize, now: i64, owner: &WeakEntity<Self>, cx: &App) -> AnyElement {
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
        let menu_owner = owner.clone();
        let menu_id = id.clone();
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
            .when(highlighted, |row| row.bg(theme.accent))
            .hover(|style| style.bg(theme.accent))
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
            .context_menu(move |menu, _, _| project_actions(menu, &menu_owner, ix, open, &menu_id))
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
        self.add_project(name, &directory.to_string_lossy(), false, window, cx);
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

/// The right-click menu of a project tab or projects-menu row.
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

/// A project's attention count. Zero stays visible but quiet, so every tab
/// keeps the same shape; anything more fills with the accent.
fn count_badge(count: usize, bar: &theme::BarColors, cx: &App) -> Div {
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
            } else {
                badge.bg(theme.primary).text_color(theme.primary_foreground)
            }
        })
        .child(count.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Measured, TabSize, fit_tabs, short_name};
    use gpui_kit::px;

    fn tab(full: f32, short: f32, active: bool, busy: bool) -> Measured {
        Measured {
            widths: [px(full), px(short), px(40.)],
            active,
            busy,
        }
    }

    #[test]
    fn tabs_step_down_quiet_first_and_never_the_active_one() {
        use TabSize::*;
        // Active, busy, quiet, quiet.
        let tabs = [
            tab(120., 70., true, false),
            tab(120., 70., false, true),
            tab(120., 70., false, false),
            tab(120., 70., false, false),
        ];
        let fit = |room: f32| fit_tabs(&tabs, px(room), px(50.));
        assert_eq!(fit(480.), [Full, Full, Full, Full]);
        assert_eq!(fit(430.), [Full, Full, Full, Short]);
        assert_eq!(fit(380.), [Full, Full, Short, Short]);
        assert_eq!(fit(330.), [Full, Short, Short, Short]);
        assert_eq!(fit(300.), [Full, Short, Short, Letter]);
        assert_eq!(fit(270.), [Full, Short, Letter, Letter]);
        assert_eq!(fit(240.), [Full, Letter, Letter, Letter]);
        // The overflow cell is wider than one letter tab, so two go at once.
        assert_eq!(fit(230.), [Full, Letter, Hidden, Hidden]);
        assert_eq!(fit(210.), [Full, Letter, Hidden, Hidden]);
        assert_eq!(fit(10.), [Full, Hidden, Hidden, Hidden]);
    }

    #[test]
    fn short_names_keep_three_letters() {
        assert_eq!(short_name("techdemos"), "tec…");
        assert_eq!(short_name("Usage Tool"), "Usa…");
        assert_eq!(short_name("docs"), "docs");
    }
}
