//! Project-specific collaboration demos and their local interactions.
use super::*;

pub(super) struct Group {
    title: String,
    topic: String,
    members: Vec<usize>,
    messages: Vec<(String, String)>,
}
pub(super) struct Ticket {
    title: String,
    description: String,
    status: usize,
    priority: usize,
    assignee: usize,
}
#[derive(Clone)]
pub(super) struct Mark {
    points: Vec<(f32, f32)>,
    tool: usize,
    label: String,
    author: String,
}
#[derive(Default)]
pub(super) struct ProjectCollaboration {
    groups: Vec<Group>,
    tickets: Vec<Ticket>,
    marks: Vec<Mark>,
    group: usize,
    issue: usize,
    filter: usize,
    tool: usize,
    prefix: String,
    board_title: String,
    seed_marks: usize,
}
const STATUSES: [&str; 4] = ["Backlog", "In progress", "In review", "Done"];
const TOOLS: [(&str, &str); 3] = [
    ("Pen", "pen"),
    ("Rectangle", "square"),
    ("Text note", "file"),
];
impl ProjectCollaboration {
    pub fn seed(project: &Workspace) -> Self {
        let (prefix, subject, topics, tasks, notes) = match project.config.id.as_str() {
            "demo-skills" => (
                "SKL",
                "Skill publishing",
                ["Library planning", "Review guidelines", "First release"],
                [
                    "Define the shared skill folder",
                    "Validate skill frontmatter",
                    "Write the review checklist",
                    "Test installation in a clean project",
                    "Add examples to each skill",
                    "Preview changes before publishing",
                    "Document skill ownership",
                    "Keep local edits during updates",
                ],
                [
                    "Draft a skill",
                    "Review with the team",
                    "Publish to library",
                    "One source of truth",
                    "Try it in a clean project",
                    "Owner: Codex",
                ],
            ),
            "demo-relay" => (
                "RLY",
                "Reliable message delivery",
                ["Delivery design", "Incident room", "Release readiness"],
                [
                    "Set the retry budget",
                    "Reconnect without losing messages",
                    "Review token expiry handling",
                    "Exercise the slow-client path",
                    "Expose queue depth",
                    "Add delivery receipts",
                    "Document reconnect behavior",
                    "Limit duplicate delivery",
                ],
                [
                    "Client sends",
                    "Relay queues",
                    "Agent receives",
                    "Acknowledge every message",
                    "Retry with backoff",
                    "Owner: Claude Code",
                ],
            ),
            _ => (
                "ADE",
                "Workspace flow",
                [
                    "Workspace design",
                    "Preview reliability",
                    "Release checklist",
                ],
                [
                    "Restore the last open workspace",
                    "Resize both side panels",
                    "Review keyboard focus states",
                    "Test the narrow window layout",
                    "Keep document edits in order",
                    "Search long conversations",
                    "Polish empty states",
                    "Remember panel preferences",
                ],
                [
                    "Open a project",
                    "Work with agents",
                    "Review the result",
                    "Keep context close",
                    "Make room for the work",
                    "Owner: Codex",
                ],
            ),
        };
        let groups = topics.iter().enumerate().map(|(i, title)| Group {
            title: (*title).into(), topic: format!("{} • {}", project.config.name, subject), members: vec![0, 1, 2 + i % 2],
            messages: vec![
                ("You".into(), format!("Let's use this group for {}. What should we tackle first?", title.to_lowercase())),
                ("Claude Code".into(), format!("My first suggestion is this ticket: {}. It gives us a clear path to check before expanding the scope.", tasks[i].to_lowercase())),
                ("Codex".into(), format!("Agreed. I can pick up this ticket: {}. I'll add the edge cases to the issue board.", tasks[i + 3].to_lowercase())),
                ("You".into(), "Keep the first pass small. Put the open questions on the whiteboard so we can review them together.".into()),
                (AGENTS[2 + i % 2].into(), format!("I've added a draft flow for {}. The handoff between steps is the part I'd like us to review.", subject.to_lowercase())),
            ],
        }).collect();
        let tickets = tasks.iter().enumerate().map(|(i, title)| Ticket {
            title: (*title).into(), description: format!("{} for {}.\n\nAcceptance criteria\n• Cover the main flow and an interrupted attempt.\n• Keep the behavior consistent with the project conventions.\n• Include a short example for the team to review.", title, project.config.name),
            status: i % 4, priority: i % 3, assignee: i % 4,
        }).collect();
        let mut marks = Vec::new();
        for (i, label) in notes.iter().enumerate() {
            marks.push(Mark {
                points: vec![(
                    0.07 + (i % 3) as f32 * 0.31,
                    if i < 3 { 0.17 } else { 0.57 },
                )],
                tool: 2,
                label: (*label).into(),
                author: if i % 2 == 0 { "Codex" } else { "Claude Code" }.into(),
            });
        }
        for x in [0.25, 0.56] {
            marks.push(Mark {
                points: vec![
                    (x, 0.3),
                    (x + 0.1, 0.3),
                    (x + 0.075, 0.275),
                    (x + 0.1, 0.3),
                    (x + 0.075, 0.325),
                ],
                tool: 0,
                label: String::new(),
                author: "Codex".into(),
            });
        }
        let seed_marks = marks.len();
        Self {
            groups,
            tickets,
            marks,
            group: 0,
            issue: 1,
            filter: 0,
            tool: 0,
            prefix: prefix.into(),
            board_title: subject.into(),
            seed_marks,
        }
    }
}
impl Adeline {
    fn collaboration_state(&self) -> &ProjectCollaboration {
        &self.collaboration[self.project]
    }
    pub(super) fn collaboration_action(
        &mut self,
        action: &Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.collaboration[self.project];
        match *action {
            Action::Group(i) => state.group = i,
            Action::SendGroup => {
                let message = self.group_input.read(cx).content.trim().to_owned();
                if !message.is_empty() && !state.groups.is_empty() {
                    state.groups[state.group]
                        .messages
                        .push(("You".into(), message));
                    self.group_input.update(cx, |v, cx| v.set("", cx));
                    self.main_scroll[Section::Groupchats as usize]
                        .handle
                        .set_offset(point(px(0.), px(-100000.)));
                }
            }
            Action::NewGroup => {
                state.groups.push(Group {
                    title: format!("Planning group {}", state.groups.len() + 1),
                    topic: "A place to plan the next piece of work".into(),
                    members: if self.demo_mode { vec![0, 1] } else { vec![] },
                    messages: if self.demo_mode {
                        vec![("You".into(), "Let's plan our next steps here.".into())]
                    } else {
                        vec![]
                    },
                });
                state.group = state.groups.len() - 1;
                self.query.update(cx, |v, cx| v.set("", cx));
                window.focus(&self.group_input.focus_handle(cx));
            }
            Action::Issue(i) => {
                state.issue = i;
                self.side_panel_open[Section::Issues as usize] = true;
            }
            Action::IssueFilter(i) => state.filter = i,
            Action::IssueScroll(forward) => {
                let scroll = &self.main_scroll[Section::Issues as usize].handle;
                let offset = scroll.offset();
                scroll.set_offset(point(
                    (offset.x + px(if forward { -251. } else { 251. })).min(px(0.)),
                    offset.y,
                ));
            }
            Action::IssueStatus(i) => state.tickets[state.issue].status = i,
            Action::NewIssue => {
                state.tickets.push(Ticket { title: format!("Follow-up from team review {}", state.tickets.len() + 1), description: "Capture the next improvement and agree on its acceptance criteria with the team.".into(), status: 0, priority: 1, assignee: 1 });
                state.issue = state.tickets.len() - 1;
                state.filter = 0;
                self.query.update(cx, |v, cx| v.set("", cx));
                self.side_panel_open[Section::Issues as usize] = true;
            }
            Action::BoardTool(i) => {
                state.tool = i;
                if i == 2 {
                    window.focus(&self.board_input.focus_handle(cx));
                }
            }
            Action::BoardUndo if state.marks.len() > state.seed_marks => {
                state.marks.pop();
            }
            _ => {}
        }
    }
    pub(super) fn collaboration_left(&self, cx: &Context<Self>) -> AnyElement {
        let state = self.collaboration_state();
        let mut content = col().gap_2().p_3();
        match self.section {
            Section::Groupchats => {
                let query = self.query.read(cx).content.to_lowercase();
                let mut count = 0;
                for (i, group) in state
                    .groups
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| g.title.to_lowercase().contains(&query))
                {
                    count += 1;
                    content = content.child(
                        self.button(("group", i), "", Action::Group(i), cx)
                            .h_auto()
                            .p_3()
                            .items_start()
                            .flex_col()
                            .gap_2()
                            .bg(rgb(theme::sidebar()))
                            .text_color(rgb(theme::sidebar_foreground()))
                            .when(i == state.group, |d| {
                                d.bg(rgb(theme::sidebar_primary()))
                                    .text_color(rgb(theme::sidebar_primary_foreground()))
                            })
                            .when(i != state.group, |d| {
                                d.hover(|s| {
                                    s.bg(rgb(theme::muted()))
                                        .text_color(rgb(theme::muted_foreground()))
                                })
                            })
                            .child(
                                row().w_full().gap_2().child(icon("group")).child(
                                    div()
                                        .child(group.title.clone())
                                        .text_size(config::text_pixels(14.))
                                        .font_weight(FontWeight::SEMIBOLD),
                                ),
                            )
                            .child(
                                div()
                                    .child(short(
                                        &group
                                            .messages
                                            .last()
                                            .map(|m| format!("{}: {}", m.0, m.1))
                                            .unwrap_or_default(),
                                        70,
                                    ))
                                    .text_size(config::text_pixels(12.)),
                            )
                            .child(
                                div()
                                    .child(format!("{} members", group.members.len() + 1))
                                    .text_size(config::text_pixels(11.)),
                            ),
                    );
                }
                if count == 0 {
                    content = content.child(
                        text(
                            "No groups match your search.",
                            13.,
                            theme::muted_foreground(),
                        )
                        .p_3(),
                    );
                }
            }
            Section::Issues => {
                content = content.child(text("Views", 12., theme::muted_foreground()).p_2());
                for (i, label) in [
                    "All tickets",
                    "High priority",
                    if self.demo_mode {
                        "Assigned to Codex"
                    } else {
                        "Assigned"
                    },
                    "Open tickets",
                ]
                .iter()
                .enumerate()
                {
                    content = content.child(
                        self.button(("issue-filter", i), *label, Action::IssueFilter(i), cx)
                            .when(state.filter == i, |d| d.bg(rgb(theme::sidebar_accent()))),
                    );
                }
                content = content.child(
                    text("Project progress", 12., theme::muted_foreground())
                        .mt_6()
                        .p_2(),
                );
                for (i, status) in STATUSES.iter().enumerate() {
                    content = content.child(
                        row()
                            .justify_between()
                            .p_2()
                            .child(text(*status, 13., theme::foreground()))
                            .child(count_chip(
                                state
                                    .tickets
                                    .iter()
                                    .filter(|t| t.status == i)
                                    .count()
                                    .to_string(),
                            )),
                    );
                }
            }
            Section::Whiteboard => {
                content =
                    content.child(text("Drawing tools", 12., theme::muted_foreground()).p_2());
                for (i, (label, glyph)) in TOOLS.iter().enumerate() {
                    content = content.child(
                        self.button(("board-tool", i), "", Action::BoardTool(i), cx)
                            .gap_3()
                            .child(icon(glyph))
                            .child(*label)
                            .when(state.tool == i, |d| d.bg(rgb(theme::sidebar_accent()))),
                    );
                }
                content = content.child(div().mt_4().p_2().border_1().border_color(rgb(theme::border())).rounded_md().child(self.board_input.clone()))
                    .child(text("Choose Text note, type above, then click to place it. Drag on the canvas to draw.", 12., theme::muted_foreground()).p_2())
                    .child(self.button("undo-mark", "Undo my last mark", Action::BoardUndo, cx).mt_3().child(icon("arrow-counter-clockwise")))
                    .child(text("Team sketch", 12., theme::muted_foreground()).mt_6().p_2())
                    .child(text(state.board_title.clone(), 15., theme::foreground()).p_2());
            }
            _ => {}
        }
        let header = if self.section == Section::Whiteboard {
            row()
                .h(px(77.))
                .px_5()
                .gap_3()
                .child(icon("whiteboard").size(px(24.)))
                .child(text("Whiteboard", 20., theme::foreground()))
        } else {
            self.mode_sidebar_header(cx)
        };
        col()
            .size_full()
            .child(header)
            .child(
                div().flex_1().min_h_0().child(
                    self.left_scroll[self.section as usize].wrap("collaboration-left", content),
                ),
            )
            .into_any_element()
    }
    pub(super) fn collaboration_members(&self) -> AnyElement {
        let state = self.collaboration_state();
        if !self.demo_mode {
            return text("No participants", 13., theme::muted_foreground())
                .p_4()
                .into_any_element();
        }
        let members = if self.section == Section::Groupchats {
            state
                .groups
                .get(state.group)
                .map(|group| group.members.clone())
                .unwrap_or_default()
        } else {
            vec![0, 1]
        };
        let mut content = col().p_4().gap_5().child(text(
            format!("{} participants", members.len() + 1),
            12.,
            theme::muted_foreground(),
        ));
        for member in std::iter::once(None).chain(members.into_iter().map(Some)) {
            let (name, glyph, role) = member.map_or(("You", "user", "Project owner"), |i| {
                (
                    AGENTS[i],
                    ["claude", "codex", "grok", "sparkle"][i],
                    "Agent • Demo participant",
                )
            });
            content = content.child(
                row().gap_3().child(icon(glyph).size(px(26.))).child(
                    col()
                        .gap_1()
                        .child(text(name, 14., theme::foreground()))
                        .child(text(role, 11., theme::muted_foreground())),
                ),
            );
        }
        content.child(div().mt_5().border_t_1().border_color(rgb(theme::border())).pt_4().child(text("Demo workspace", 13., theme::foreground())).child(text("Messages and drawings stay in this session. Agent contributions are sample content.", 12., theme::muted_foreground()).mt_2())).into_any_element()
    }
    pub(super) fn issue_details(&self, cx: &Context<Self>) -> AnyElement {
        let state = self.collaboration_state();
        let Some(ticket) = state.tickets.get(state.issue) else {
            return text("No issue selected", 13., theme::muted_foreground())
                .p_4()
                .into_any_element();
        };
        let mut content = col()
            .p_4()
            .gap_4()
            .child(text(
                format!("{}-{}", state.prefix, 101 + state.issue),
                12.,
                theme::muted_foreground(),
            ))
            .child(text(ticket.title.clone(), 20., theme::foreground()))
            .child(text("Status", 12., theme::muted_foreground()));
        for (i, status) in STATUSES.iter().enumerate() {
            content = content.child(
                self.button(("ticket-status", i), *status, Action::IssueStatus(i), cx)
                    .when(ticket.status == i, |d| {
                        d.bg(rgb(theme::sidebar_accent())).child(icon("check"))
                    }),
            );
        }
        content
            .child(text(
                format!(
                    "Assignee   {}",
                    if self.demo_mode {
                        AGENTS[ticket.assignee]
                    } else {
                        "Unassigned"
                    }
                ),
                13.,
                theme::foreground(),
            ))
            .child(text(
                format!("Priority   {}", ["High", "Medium", "Low"][ticket.priority]),
                13.,
                theme::foreground(),
            ))
            .child(
                text(ticket.description.clone(), 13., theme::foreground())
                    .pt_3()
                    .border_t_1()
                    .border_color(rgb(theme::border())),
            )
            .child(text("Activity", 12., theme::muted_foreground()).mt_4())
            .child(text(
                if self.demo_mode {
                    "Codex added acceptance criteria.\nYou added this to the team review."
                } else {
                    "No activity"
                },
                12.,
                theme::muted_foreground(),
            ))
            .into_any_element()
    }
    pub(super) fn collaboration_body(&self, cx: &Context<Self>) -> AnyElement {
        match self.section {
            Section::Groupchats => self.group_body(cx),
            Section::Issues => self.issues_body(cx),
            _ => self.whiteboard_body(cx),
        }
    }
    fn collaboration_header(&self, title: String, subtitle: String) -> Div {
        col()
            .h(px(77.))
            .flex_shrink_0()
            .px_5()
            .justify_center()
            .gap_1()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .child(text(title, 20., theme::foreground()).truncate())
            .child(text(subtitle, 12., theme::muted_foreground()).truncate())
    }
    fn group_body(&self, cx: &Context<Self>) -> AnyElement {
        let state = self.collaboration_state();
        let Some(group) = state.groups.get(state.group) else {
            return text("No group selected", 13., theme::muted_foreground())
                .p_5()
                .into_any_element();
        };
        let mut messages = col()
            .p_5()
            .gap_5()
            .child(text("Today", 11., theme::muted_foreground()).text_center());
        for (i, (author, message)) in group.messages.iter().enumerate() {
            let glyph = match author.as_str() {
                "You" => "user",
                "Claude Code" => "claude",
                "Grok Build" => "grok",
                "Antigravity" => "sparkle",
                _ => "codex",
            };
            messages = messages.child(
                col()
                    .gap_2()
                    .when(author == "You", |d| d.pl_6())
                    .child(
                        row()
                            .gap_2()
                            .child(icon(glyph).size(px(23.)))
                            .child(text(author.clone(), 12., theme::foreground()))
                            .child(text(
                                format!("10:{:02}", 12 + i % 48),
                                10.,
                                theme::muted_foreground(),
                            )),
                    )
                    .child(
                        text(message.clone(), 14., theme::foreground())
                            .p_4()
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .rounded(px(18.))
                            .bg(rgb(if author == "You" {
                                theme::card()
                            } else {
                                theme::background()
                            })),
                    ),
            );
        }
        col()
            .size_full()
            .child(self.collaboration_header(group.title.clone(), group.topic.clone()))
            .child(div().flex_1().min_h_0().child(
                self.main_scroll[Section::Groupchats as usize].wrap("group-messages", messages),
            ))
            .child(
                col()
                    .flex_shrink_0()
                    .p_4()
                    .gap_3()
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .bg(rgb(theme::sidebar()))
                    .child(self.group_input.clone())
                    .child(
                        row()
                            .justify_between()
                            .child(text(
                                "Everyone in this group can see your message",
                                11.,
                                theme::muted_foreground(),
                            ))
                            .child(
                                self.button("send-group", "Send", Action::SendGroup, cx)
                                    .child(icon("send")),
                            ),
                    ),
            )
            .into_any_element()
    }
    fn issues_body(&self, cx: &Context<Self>) -> AnyElement {
        let state = self.collaboration_state();
        let query = self.query.read(cx).content.to_lowercase();
        let visible = |t: &Ticket| {
            t.title.to_lowercase().contains(&query)
                && match state.filter {
                    1 => t.priority == 0,
                    2 => t.assignee == 1,
                    3 => t.status != 3,
                    _ => true,
                }
        };
        let mut columns = row().items_start().gap_4().p_5().w(px(1028.)).min_h_full();
        for (status, label) in STATUSES.iter().enumerate() {
            let tickets: Vec<_> = state
                .tickets
                .iter()
                .enumerate()
                .filter(|(_, t)| t.status == status && visible(t))
                .collect();
            let mut column = col().w(px(235.)).flex_shrink_0().gap_3().child(
                row()
                    .gap_2()
                    .pb_2()
                    .child(div().size(px(7.)).rounded_full().bg(rgb([
                        theme::muted_foreground(),
                        theme::primary(),
                        theme::chart_4(),
                        theme::chart_3(),
                    ][status])))
                    .child(text(*label, 13., theme::foreground()).font_weight(FontWeight::SEMIBOLD))
                    .child(count_chip(tickets.len().to_string())),
            );
            if tickets.is_empty() {
                column = column.child(
                    text("No tickets", 12., theme::muted_foreground())
                        .p_4()
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .rounded_md(),
                );
            }
            for (i, ticket) in tickets {
                column = column.child(
                    self.button(("ticket", i), "", Action::Issue(i), cx)
                        .h_auto()
                        .w_full()
                        .p_4()
                        .items_start()
                        .flex_col()
                        .gap_3()
                        .bg(rgb(theme::card()))
                        .border_1()
                        .border_color(rgb(if i == state.issue {
                            theme::primary()
                        } else {
                            theme::border()
                        }))
                        .rounded(px(9.))
                        .child(text(
                            format!("{}-{}", state.prefix, 101 + i),
                            11.,
                            theme::muted_foreground(),
                        ))
                        .child(
                            text(ticket.title.clone(), 14., theme::foreground())
                                .font_weight(FontWeight::MEDIUM),
                        )
                        .child(
                            row()
                                .w_full()
                                .justify_between()
                                .child(badge(
                                    ["High", "Medium", "Low"][ticket.priority],
                                    theme::muted(),
                                    if ticket.priority == 0 {
                                        theme::primary()
                                    } else {
                                        theme::muted_foreground()
                                    },
                                ))
                                .child(text(
                                    if self.demo_mode {
                                        AGENTS[ticket.assignee]
                                    } else {
                                        "Unassigned"
                                    },
                                    11.,
                                    theme::muted_foreground(),
                                )),
                        ),
                );
            }
            columns = columns.child(column);
        }
        col()
            .size_full()
            .child(self.collaboration_header(
                format!("{} issues", self.workspace().config.name),
                format!(
                    "{} tickets • Team board",
                    state.tickets.iter().filter(|t| visible(t)).count()
                ),
            ))
            .child(
                div()
                    .id("issue-board")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .track_scroll(&self.main_scroll[Section::Issues as usize].handle)
                    .child(columns),
            )
            .child(
                row()
                    .h(px(40.))
                    .flex_shrink_0()
                    .px_4()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .child(text("Team board", 11., theme::muted_foreground()))
                    .child(div().flex_1())
                    .child(self.button(
                        "earlier-columns",
                        "← Previous columns",
                        Action::IssueScroll(false),
                        cx,
                    ))
                    .child(self.button(
                        "later-columns",
                        "Next columns →",
                        Action::IssueScroll(true),
                        cx,
                    )),
            )
            .into_any_element()
    }
    fn whiteboard_body(&self, cx: &Context<Self>) -> AnyElement {
        let state = self.collaboration_state();
        let marks = state.marks.clone();
        let owner = cx.entity().downgrade();
        let mut board = div()
            .id("drawing-board")
            .relative()
            .size_full()
            .overflow_hidden()
            .cursor(CursorStyle::Crosshair)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = owner.update(cx, |app, _| app.board_bounds = bounds);
                    },
                    move |bounds, (), window, _| {
                        let w = f32::from(bounds.size.width);
                        let h = f32::from(bounds.size.height);
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "canvas sizes are non-negative; whole-pixel steps are intended"
                        )]
                        for x in (0..w as usize).step_by(24) {
                            for y in (0..h as usize).step_by(24) {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        bounds.origin + point(px(x as f32), px(y as f32)),
                                        size(px(1.), px(1.)),
                                    ),
                                    rgb(theme::border()),
                                ));
                            }
                        }
                        for mark in marks.iter().filter(|m| m.tool != 2) {
                            let mut path = PathBuilder::stroke(px(2.));
                            let convert =
                                |p: (f32, f32)| bounds.origin + point(px(p.0 * w), px(p.1 * h));
                            let Some(first) = mark.points.first().copied() else {
                                continue;
                            };
                            path.move_to(convert(first));
                            if mark.tool == 1 {
                                let end = mark.points.last().copied().unwrap_or(first);
                                for p in [(end.0, first.1), end, (first.0, end.1), first] {
                                    path.line_to(convert(p));
                                }
                            } else {
                                for p in mark.points.iter().skip(1) {
                                    path.line_to(convert(*p));
                                }
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, rgb(theme::chart_4()));
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, e: &MouseDownEvent, _, cx| {
                    let bounds = s.board_bounds;
                    if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
                        return;
                    }
                    let p = (
                        (f32::from(e.position.x - bounds.left()) / f32::from(bounds.size.width))
                            .clamp(0., 0.95),
                        (f32::from(e.position.y - bounds.top()) / f32::from(bounds.size.height))
                            .clamp(0., 0.95),
                    );
                    let label = s.board_input.read(cx).content.trim().to_owned();
                    let state = &mut s.collaboration[s.project];
                    if state.tool == 2 && label.is_empty() {
                        return;
                    }
                    let p = if state.tool == 2 {
                        (p.0.min(0.74), p.1.min(0.75))
                    } else {
                        p
                    };
                    state.marks.push(Mark {
                        points: vec![p, p],
                        tool: state.tool,
                        label,
                        author: "You".into(),
                    });
                    s.drawing = state.tool != 2;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|s, e: &MouseMoveEvent, _, cx| {
                if !s.drawing || e.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let bounds = s.board_bounds;
                let p = (
                    (f32::from(e.position.x - bounds.left()) / f32::from(bounds.size.width))
                        .clamp(0., 1.),
                    (f32::from(e.position.y - bounds.top()) / f32::from(bounds.size.height))
                        .clamp(0., 1.),
                );
                if let Some(mark) = s.collaboration[s.project].marks.last_mut() {
                    if mark.tool == 1 {
                        mark.points[1] = p;
                    } else {
                        mark.points.push(p);
                    }
                }
                cx.notify();
            }));
        for (i, mark) in state.marks.iter().enumerate().filter(|(_, m)| m.tool == 2) {
            let p = mark.points[0];
            board = board.child(
                col()
                    .absolute()
                    .left(relative(p.0))
                    .top(relative(p.1))
                    .w(relative(0.24))
                    .p_3()
                    .gap_3()
                    .rounded(px(5.))
                    .bg(rgb(if i % 2 == 0 {
                        theme::secondary()
                    } else {
                        theme::muted()
                    }))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .child(text(mark.label.clone(), 14., theme::foreground()))
                    .child(text(mark.author.clone(), 10., theme::muted_foreground())),
            );
        }
        col()
            .size_full()
            .child(self.collaboration_header(
                state.board_title.clone(),
                format!(
                    "{} • {} selected",
                    self.workspace().config.name,
                    TOOLS[state.tool].0
                ),
            ))
            .child(div().flex_1().min_h_0().child(board))
            .child(
                row()
                    .h(px(36.))
                    .px_4()
                    .justify_between()
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .child(text(
                        if self.demo_mode {
                            "Shared team sketch • Demo"
                        } else {
                            "Shared team sketch"
                        },
                        11.,
                        theme::muted_foreground(),
                    ))
                    .child(text(
                        format!("{} marks", state.marks.len()),
                        11.,
                        theme::muted_foreground(),
                    )),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{AGENTS, ProjectCollaboration};
    use crate::data;
    #[test]
    fn each_demo_has_distinct_collaboration_content() {
        let (projects, _) = data::load();
        let states: Vec<_> = projects.iter().map(ProjectCollaboration::seed).collect();
        for state in &states {
            assert_eq!(state.groups.len(), 3);
            assert!(state.groups.iter().all(|g| g.members.len() >= 2
                && g.messages.len() >= 4
                && g.members.iter().all(|i| *i < AGENTS.len())));
            for status in 0..4 {
                assert!(state.tickets.iter().any(|t| t.status == status));
            }
            assert!(state.marks.iter().any(|m| m.tool == 2));
        }
        for (i, a) in states.iter().enumerate() {
            for b in states.iter().skip(i + 1) {
                assert_ne!(a.prefix, b.prefix);
                assert_ne!(a.groups[0].title, b.groups[0].title);
            }
        }
    }
}
