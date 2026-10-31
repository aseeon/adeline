//! The mode rail on the left island and the project switcher at the top of
//! the chat list.
//!
//! The switcher card shows the open project, and Ctrl+P or a click opens the
//! projects menu: a searchable list of every project, where open projects can
//! be closed and closed ones deleted, with a short window to undo the delete.
//! The other open projects sit under the card, each with an orange count of
//! chats that need the user or a green dot while an agent works. Right-clicking
//! a project offers rename and close or delete.
use super::*;
use adeline_core::conversation::ThreadStatus;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use panes::dp;
use std::time::Duration;

mod machine_menu;
mod menu;

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

const RAIL: [(Section, &str, &str); 7] = [
    (Section::Chats, "Chats", "chat.svg"),
    (Section::Docs, "Docs", "file.svg"),
    (Section::Workflows, "Workflows", "workflow.svg"),
    (Section::Services, "Services", "service.svg"),
    (Section::Groupchats, "Groupchats", "group.svg"),
    (Section::Issues, "Issues", "flag.svg"),
    (Section::Whiteboard, "Whiteboard", "whiteboard.svg"),
];

/// The modes the rail shows, in its order; Ctrl+1 to Ctrl+9 pick them.
pub(super) fn rail_sections() -> Vec<Section> {
    let features = config::current().general.features;
    RAIL.iter()
        .map(|(section, _, _)| *section)
        .filter(|section| features.enabled(*section))
        .collect()
}

pub(super) fn menu_anchor(id: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .size(px(1.))
        .opacity(0.)
        .tab_stop(false)
}

pub(super) fn mode_name(section: Section) -> &'static str {
    RAIL.iter()
        .find(|(mode, _, _)| *mode == section)
        .map_or("Chats", |(_, name, _)| name)
}

impl Adeline {
    /// Switches to the next or previous open project, wrapping at the ends.
    pub(super) fn step_project(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open: Vec<usize> = (0..self.projects.len())
            .filter(|&ix| self.open_projects[ix])
            .collect();
        let Some(at) = open.iter().position(|&ix| ix == self.project) else {
            return;
        };
        let next = (if forward { at + 1 } else { at + open.len() - 1 }) % open.len();
        self.act(Action::Project(open[next]), window, cx);
    }

    pub(super) fn mode_rail(&self, cx: &Context<Self>) -> Div {
        let isl = theme::islands(cx);
        let waiting = self.waiting_chat();
        let mut buttons = Vec::new();
        if self.has_open_project() {
            for (position, section) in rail_sections().into_iter().enumerate() {
                let Some((_, name, icon_path)) = RAIL.iter().find(|(mode, _, _)| *mode == section)
                else {
                    continue;
                };
                let active = self.section == section;
                let dot = section == Section::Chats && waiting;
                let label = if dot {
                    format!("{name}, a chat needs you")
                } else {
                    (*name).to_owned()
                };
                let tooltip = format!(
                    "{name} ({})",
                    conversation_ui::shortcut(&format!("Ctrl+{}", position + 1))
                );
                buttons.push(
                    div()
                        .relative()
                        .child(
                            panes::rail_button(
                                *name,
                                label,
                                tooltip,
                                Icon::default().path(*icon_path),
                                active,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |app, _, window, cx| {
                                    app.act(Action::Section(section), window, cx);
                                },
                            )),
                        )
                        .when(dot, |mode| {
                            mode.child(
                                div()
                                    .absolute()
                                    .top(dp(6.))
                                    .right(dp(6.))
                                    .size(dp(9.))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(isl.island)
                                    .bg(isl.accent),
                            )
                        })
                        .into_any_element(),
                );
            }
        }
        buttons.push(
            self.command_popover(
                "app",
                panes::rail_button(
                    "app-menu",
                    "Main menu",
                    format!(
                        "Settings and more ({})",
                        conversation_ui::shortcut("Ctrl+,")
                    ),
                    Icon::default().path("settings.svg"),
                    false,
                    cx,
                ),
                Anchor::TopLeft,
                cx,
            )
            .into_any_element(),
        );
        panes::rail("modes", "Modes", buttons, self.mode_rail_toggle(cx))
    }

    /// The projects and machines menus, hanging from the top center of the
    /// window where the design's palette opens.
    pub(super) fn menu_overlays(&self, cx: &Context<Self>) -> Div {
        div()
            .absolute()
            .top(dp(88.))
            .left(relative(0.5))
            .child(self.projects_menu(cx))
            .when(!machines::remotes().is_empty(), |anchor| {
                anchor.child(self.machine_selector(cx))
            })
    }

    /// The card for the open project, then the other open projects.
    pub(super) fn project_switcher(&self, cx: &Context<Self>) -> Div {
        let isl = theme::islands(cx);
        let others: Vec<usize> = (0..self.projects.len())
            .filter(|&ix| self.open_projects[ix] && ix != self.project)
            .collect();
        let project = self.workspace();
        let name = project.config.name.clone();
        let (fill, letter) = theme::project_mark(
            theme::project_tint(self.project_tints[self.project]),
            true,
            cx.theme(),
        );
        let card = Button::new("project-menu")
            .ghost()
            .w_full()
            .h(dp(52.))
            .px(dp(10.))
            .rounded(dp(12.))
            .bg(isl.chip)
            .accessibility_label(format!("Projects: {}", self.tab_name(project)))
            .on_click(cx.listener(|app, _, window, cx| app.act(Action::Projects, window, cx)))
            .child(
                row()
                    .w_full()
                    .min_w_0()
                    .gap(dp(10.))
                    .child(letter_mark(&name, fill, letter, dp(30.)))
                    .child(
                        col()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .child(
                                div()
                                    .max_w_full()
                                    .truncate()
                                    .text_size(dp(theme::T_BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(isl.fg)
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(dp(theme::T_CAPTION))
                                    .text_color(isl.fg3)
                                    .child(machines::name(&project.machine)),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(dp(theme::T_MICRO))
                            .text_color(isl.fg3)
                            .child(conversation_ui::shortcut("Ctrl+P").replace('+', " ")),
                    ),
            );
        let attention = project.attention_count();
        let owner = cx.weak_entity();
        let id = project.key();
        col()
            .w_full()
            .flex_shrink_0()
            .gap(dp(2.))
            .px(dp(8.))
            .pt(dp(8.))
            .child(
                div()
                    .id(SharedString::from(format!("project-{id}")))
                    .test_support()
                    .role(Role::Tab)
                    .aria_label(self.project_label(self.project, attention, cx))
                    .aria_selected(true)
                    .mb(dp(4.))
                    .child(card)
                    .context_menu(move |menu, _, _| project_actions(menu, &owner, true, &id)),
            )
            .children(others.into_iter().map(|ix| self.project_row(ix, cx)))
    }

    fn project_label(&self, ix: usize, attention: usize, cx: &App) -> String {
        let project = &self.projects[ix];
        let name = self.tab_name(project);
        let mut label = if attention == 0 {
            name
        } else {
            format!("{name} ({attention})")
        };
        if !client::usable(&project.machine, cx) {
            label.push_str(", disconnected");
        }
        label
    }

    fn project_row(&self, ix: usize, cx: &Context<Self>) -> impl IntoElement {
        let isl = theme::islands(cx);
        let project = &self.projects[ix];
        let id = project.key();
        let name = self.tab_name(project);
        let attention = project.attention_count();
        let running = project
            .threads
            .iter()
            .any(|thread| thread.status == ThreadStatus::Processing);
        let group = SharedString::from(format!("project-tab-{id}"));
        let (fill, letter) = theme::project_mark(
            theme::project_tint(self.project_tints[ix]),
            true,
            cx.theme(),
        );
        let status = if attention > 0 {
            div()
                .min_w(dp(18.))
                .h(dp(18.))
                .px(dp(5.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(isl.accent)
                .text_color(isl.send_fg)
                .text_size(dp(theme::T_MICRO))
                .font_weight(FontWeight::SEMIBOLD)
                .child(attention.to_string())
                .into_any_element()
        } else if running {
            div()
                .size(dp(7.))
                .rounded_full()
                .bg(isl.ok)
                .into_any_element()
        } else {
            div().into_any_element()
        };
        let owner = cx.weak_entity();
        row()
            .id(SharedString::from(format!("project-{id}")))
            .test_support()
            .group(group.clone())
            .role(Role::Tab)
            .aria_label(self.project_label(ix, attention, cx))
            .aria_selected(false)
            .h(dp(32.))
            .flex_shrink_0()
            .px(dp(10.))
            .gap(dp(10.))
            .rounded(dp(8.))
            .text_size(dp(theme::T_BODY))
            .text_color(isl.fg2)
            .hover(|style| style.bg(isl.hover).text_color(isl.fg))
            .when(!client::usable(&project.machine, cx), |row| {
                row.opacity(0.6)
            })
            .on_click(cx.listener(move |app, _, window, cx| {
                app.act(Action::Project(ix), window, cx);
            }))
            .child(letter_mark(&project.config.name, fill, letter, dp(20.)))
            .child(div().flex_1().min_w_0().truncate().child(name))
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .min_w(dp(22.))
                    .child(
                        div()
                            .group_hover(group.clone(), |style| style.invisible())
                            .child(status),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(dp(-4.))
                            .invisible()
                            .group_hover(group, |style| style.visible())
                            .child(
                                self.icon_button(
                                    SharedString::from(format!("close-{id}")),
                                    "Close project",
                                    Icon::default().path("close.svg"),
                                    Action::CloseProject(ix),
                                    cx,
                                )
                                .xsmall(),
                            ),
                    ),
            )
            .context_menu(move |menu, _, _| project_actions(menu, &owner, true, &id))
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

// The menu outlives the render that built it, so items find the project by key
// when clicked: the projects may have shifted since.
fn project_actions(
    menu: PopupMenu,
    owner: &WeakEntity<Adeline>,
    open: bool,
    id: &str,
) -> PopupMenu {
    let by_key = |make: fn(usize) -> Action| {
        let owner = owner.clone();
        let id = id.to_owned();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            let _ = owner.update(cx, |app, cx| {
                if let Some(ix) = app.project_index(Some(&id)) {
                    app.act(make(ix), window, cx);
                }
            });
        }
    };
    let rename = PopupMenuItem::new("Rename project…").on_click(by_key(Action::RenameProject));
    let last = if open {
        PopupMenuItem::new("Close project").on_click(by_key(Action::CloseProject))
    } else {
        PopupMenuItem::new("Delete project").on_click(owner_action(
            owner,
            Action::RemoveClosedProject(id.to_owned()),
        ))
    };
    menu.item(rename).item(last)
}

fn letter_mark(name: &str, fill: Hsla, letter: Hsla, size: Pixels) -> Div {
    // Up to two initials, one per word of the name.
    let initials: String = name
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    let small = size <= dp(22.);
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(size)
        .rounded(size * 0.25)
        .bg(fill)
        .text_color(letter)
        .text_size(dp(if small { 9. } else { theme::T_MICRO }))
        .font_weight(FontWeight::SEMIBOLD)
        .child(initials)
}
