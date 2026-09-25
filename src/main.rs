#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod chat;
mod chat_render;
mod collaboration_modes;
mod config;
mod content_views;
mod data;
mod document_render;
#[expect(
    unused_imports,
    dead_code,
    reason = "adapted from GPUI's text input example; unused helpers kept for parity"
)]
mod input;
mod interaction;
mod panes;
mod prepared;
mod scrollbar;
mod settings;
mod themed_icon;
#[cfg(target_os = "windows")]
mod titlebar;
mod ui_metrics;
mod views;
use data::*;
use gpui::{prelude::*, *};
use input::TextInput;
use std::borrow::Cow;
mod theme;

struct Machine {
    name: &'static str,
    kind: &'static str,
    agents: &'static [usize],
}
const AGENTS: [&str; 4] = ["Claude Code", "Codex", "Grok Build", "Antigravity"];
const MACHINES: [Machine; 3] = [
    Machine {
        name: "Nexus",
        kind: "Local machine",
        agents: &[0, 1, 2, 3],
    },
    Machine {
        name: "Matrix",
        kind: "Remote machine",
        agents: &[0, 1, 2, 3],
    },
    Machine {
        name: "Vortex",
        kind: "Remote machine",
        agents: &[0, 1, 2, 3],
    },
];
include!(concat!(env!("OUT_DIR"), "/assets.rs"));
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(embedded(path).map(Cow::Borrowed))
    }
    fn list(&self, _: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
fn row() -> Div {
    div().flex().items_center()
}
fn col() -> Div {
    div().flex().flex_col()
}
fn menu_surface() -> Div {
    col()
        .occlude()
        .bg(rgb(theme::sidebar()))
        .text_color(rgb(theme::sidebar_foreground()))
        .border_1()
        .border_color(rgb(theme::border()))
        .rounded(px(4.))
        .p(px(6.))
}
fn text(s: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .child(s.into())
}
fn icon(name: &str) -> themed_icon::ThemedIcon {
    themed_icon::ThemedIcon::new(name)
        .size(px(16.))
        .flex_shrink_0()
}
fn icon_label(name: &str, label: impl Into<SharedString>, size: f32, color: u32) -> Div {
    row()
        .gap_2()
        .child(icon(name).size(px(size)).text_color(rgb(color)))
        .child(text(label, size, color))
}
fn badge(label: impl Into<SharedString>, bg: u32, fg: u32) -> Div {
    text(label, 11., fg)
        .px_2()
        .py_1()
        .rounded(px(5.))
        .bg(rgb(bg))
}
fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.into()
    }
}
fn provider(s: &str) -> &'static str {
    if s == "claude" {
        "Claude Code"
    } else {
        "Codex"
    }
}
fn count_chip(label: impl Into<SharedString>) -> Div {
    text(label, 10., theme::primary_foreground())
        .font_weight(FontWeight::SEMIBOLD)
        .h(px(17.))
        .min_w(px(17.))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .line_height(px(15.))
        .rounded(px(6.))
        .bg(rgb(theme::primary()))
}
fn checkbox(checked: bool) -> Div {
    div()
        .size(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded(px(3.))
        .border_1()
        .border_color(rgb(if checked {
            theme::primary()
        } else {
            theme::border()
        }))
        .when(checked, |d| {
            d.bg(rgb(theme::primary())).child(
                icon("check")
                    .size(px(11.))
                    .text_color(rgb(theme::primary_foreground())),
            )
        })
}
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Chats,
    Docs,
    Workflows,
    Services,
    Groupchats,
    Issues,
    Whiteboard,
}
#[derive(Clone)]
enum Action {
    Group(usize),
    SendGroup,
    NewGroup,
    Issue(usize),
    IssueFilter(usize),
    IssueScroll(bool),
    IssueStatus(usize),
    NewIssue,
    BoardTool(usize),
    BoardUndo,
    Project(usize),
    Section(Section),
    Chat(usize),
    NewChat,
    Filter(usize),
    ShowCompleted,
    ToggleSidePanel,
    ToggleLeftPanel,
    Event(usize),
    Complete,
    ChatMenu,
    Projects,
    AppMenu,
    Machines,
    Agents,
    Machine(usize),
    About,
    KeyboardShortcuts,
    QuitApp,
    Settings,
    AppSettings,
    LeftPanel(Section),
    RightPanel(Section),
    ModeSettings,
    ConfigureModeSettings,
    Close,
    SaveSettings,
    AgentMenu,
    Agent(usize),
    Model(usize),
    Effort(usize),
    Speed(usize),
    Permission(usize),
    ToggleMode(Section),
    Send,
    DocsHome,
    Document(usize),
    Raw,
    Archive,
    DocMenu,
    NewDoc,
    PinDoc,
    EditLine(usize),
    SaveLine,
    Format(&'static str),
    CheckLine(usize),
    Workflow(usize),
    Collection(String),
    RunWorkflow,
    Schedule,
    Service(usize),
    StopService,
    NewService,
    Wrap,
    Follow,
    CopyOutput,
    Decision(usize),
    InsertFiles,
    Tint(usize),
    Instructions,
    NewWorkflow,
    EditWorkflow,
    SaveWorkflow,
    NewCollection,
    SaveCollection,
    EditTitle,
    SaveTitle,
    ArchiveDoc,
}
impl Action {
    fn menu_target(&self) -> Option<&'static str> {
        match self {
            Self::AppMenu => Some("app"),
            Self::ModeSettings => Some("mode-settings"),
            Self::Machines => Some("machines"),
            Self::Projects => Some("projects"),
            Self::Agents => Some("agents"),
            Self::AgentMenu => Some("agent"),
            Self::ChatMenu => Some("chat"),
            Self::DocMenu => Some("document"),
            Self::InsertFiles => Some("files"),
            _ => None,
        }
    }
}
actions!(
    adeline,
    [
        Dismiss,
        NewThread,
        Search,
        OpenSettings,
        SendMessage,
        Quit,
        NextFocus,
        PreviousFocus
    ]
);
struct Adeline {
    projects: Vec<Workspace>,
    services: Vec<Service>,
    project: usize,
    machine: usize,
    machine_agents: [usize; 3],
    section: Section,
    selected: Option<usize>,
    filter: usize,
    show_completed: bool,
    side_panel_open: [bool; 7],
    left_scroll: [scrollbar::PanelScroll; 7],
    right_scroll: [scrollbar::PanelScroll; 7],
    main_scroll: [scrollbar::PanelScroll; 7],
    collaboration: Vec<collaboration_modes::ProjectCollaboration>,
    group_input: Entity<TextInput>,
    board_input: Entity<TextInput>,
    right_panel_width: f32,
    dragging_right: bool,
    board_bounds: Bounds<Pixels>,
    drawing: bool,
    editing_workflow: bool,
    expanded_event: Option<usize>,
    query: Entity<TextInput>,
    project_query: Entity<TextInput>,
    agent_query: Entity<TextInput>,
    composer: Entity<TextInput>,
    name_input: Entity<TextInput>,
    edit_input: Entity<TextInput>,
    modal: Option<&'static str>,
    menu: Option<&'static str>,
    menu_triggers:
        std::rc::Rc<std::cell::RefCell<std::collections::HashMap<&'static str, Bounds<Pixels>>>>,
    agent: usize,
    model: usize,
    effort: usize,
    speed: usize,
    permission: usize,
    document: Option<usize>,
    left_panel_open: [bool; 7],
    raw: bool,
    archived: bool,
    pinned: bool,
    edit_line: Option<usize>,
    workflow: Option<usize>,
    collection: String,
    service: Option<usize>,
    stopped: bool,
    wrap: bool,
    follow: bool,
    sidebar_width: f32,
    dragging: bool,
    selected_tint: usize,
    instructions: bool,
    focus: FocusHandle,
    toast: Option<String>,
    subscriptions: Vec<Subscription>,
    archived_docs: std::collections::HashSet<(usize, usize)>,
    project_tints: [usize; 3],
    chat_list: Entity<chat::ChatList>,
    transcript: Entity<chat::Transcript>,
    composer_region: Entity<chat::Composer>,
    header_region: Entity<chat::Header>,
    control_pane: Entity<panes::ControlPane>,
    files_region: Entity<content_views::Files>,
    files_home_region: Entity<content_views::DocsHome>,
    service_sidebar_region: Entity<content_views::ServiceSidebar>,
    document_region: Entity<content_views::DocumentView>,
    services_region: Entity<content_views::Services>,
    log_region: Entity<content_views::LogView>,
    document_tasks: std::collections::HashMap<(usize, usize), Task<()>>,
}
impl Adeline {
    fn new(cx: &mut Context<Self>) -> Self {
        let (projects, services) = load();
        #[cfg(feature = "ui-profiling")]
        let (projects, services) = ui_metrics::stress_content(projects, services);
        #[cfg(feature = "ui-profiling")]
        let projects = ui_metrics::stress_fixture(projects);
        let mut projects = projects;
        for project in &mut projects {
            project.rebuild_counts();
        }
        let query = cx.new(|cx| TextInput::search("Search…", cx));
        let project_query = cx.new(|cx| TextInput::search("Find a project…", cx));
        let agent_query = cx.new(|cx| TextInput::search("Find an agent…", cx));
        let composer = cx.new(|cx| TextInput::new("Ask your agent to do anything…", cx));
        let name_input = cx.new(|cx| TextInput::new("Project name", cx));
        let edit_input = cx.new(|cx| TextInput::new("Write here…", cx));
        let subscriptions = vec![
            cx.subscribe(&query, |app, _, _: &input::ContentChanged, cx| {
                match app.section {
                    Section::Chats => app.search_sidebar(cx),
                    Section::Docs => app.files_home_region.update(cx, |_, cx| cx.notify()),
                    Section::Services => app.service_sidebar_region.update(cx, |_, cx| cx.notify()),
                    Section::Workflows
                    | Section::Groupchats
                    | Section::Issues
                    | Section::Whiteboard => {}
                }
                cx.notify();
            }),
            cx.subscribe(&project_query, |_, _, _: &input::ContentChanged, cx| {
                cx.notify();
            }),
            cx.subscribe(&agent_query, |_, _, _: &input::ContentChanged, cx| {
                cx.notify();
            }),
        ];
        let owner = cx.weak_entity();
        let chat_list = cx.new(|_| chat::ChatList::new(owner.clone()));
        let transcript = cx.new(|cx| chat::Transcript::new(owner.clone(), cx));
        let composer_region = cx.new(|cx| chat::Composer::new(owner.clone(), &composer, cx));
        let control_pane = cx.new(|_| panes::ControlPane::new(owner.clone()));
        let header_region = cx.new(|_| chat::Header(owner.clone()));
        let files_region = cx.new(|_| content_views::Files(owner.clone()));
        let files_home_region = cx.new(|_| content_views::DocsHome(owner.clone()));
        let service_sidebar_region = cx.new(|_| content_views::ServiceSidebar(owner.clone()));
        let document_region = cx.new(|_| content_views::DocumentView::new(owner.clone()));
        let services_region = cx.new(|_| content_views::Services(owner.clone()));
        let log_region = cx.new(|_| content_views::LogView::new(owner));
        let mut app = Self {
            collaboration: projects
                .iter()
                .map(collaboration_modes::ProjectCollaboration::seed)
                .collect(),
            group_input: cx.new(|cx| TextInput::new("Message the group…", cx)),
            board_input: cx.new(|cx| TextInput::new("Type a note, then click the canvas…", cx)),
            right_panel_width: 302.,
            dragging_right: false,
            board_bounds: Bounds::default(),
            drawing: false,
            projects,
            services,
            project: 0,
            machine: 0,
            machine_agents: [0; 3],
            section: Section::Chats,
            selected: None,
            filter: 0,
            show_completed: true,
            side_panel_open: [false, false, false, false, true, true, true],
            left_scroll: std::array::from_fn(|_| Default::default()),
            right_scroll: std::array::from_fn(|_| Default::default()),
            main_scroll: std::array::from_fn(|_| Default::default()),
            editing_workflow: false,
            expanded_event: None,
            query,
            project_query,
            agent_query,
            composer,
            name_input,
            edit_input,
            modal: None,
            menu: None,
            menu_triggers: Default::default(),
            agent: 0,
            model: 0,
            effort: 2,
            speed: 0,
            permission: 2,
            document: None,
            left_panel_open: [true, false, false, true, true, true, true],
            raw: false,
            archived: false,
            pinned: false,
            edit_line: None,
            workflow: None,
            collection: "All".into(),
            service: None,
            stopped: false,
            wrap: true,
            follow: true,
            sidebar_width: 360.,
            dragging: false,
            selected_tint: 3,
            instructions: false,
            focus: cx.focus_handle(),
            toast: None,
            subscriptions,
            archived_docs: Default::default(),
            project_tints: [0, 2, 3],
            chat_list,
            transcript,
            composer_region,
            header_region,
            control_pane,
            document_tasks: Default::default(),
            files_region,
            files_home_region,
            service_sidebar_region,
            document_region,
            services_region,
            log_region,
        };
        app.load_settings();
        for project in 0..app.projects.len() {
            app.project = project;
            for index in 0..app.projects[project].docs.len() {
                let doc = &app.projects[project].docs[index];
                if doc.revision != doc.prepared_revision {
                    app.prepare_document(index, cx);
                }
            }
        }
        app.project = 0;
        app.sync_sidebar(cx);
        app.transcript
            .update(cx, |view, cx| view.sync(&app, false, cx));
        app
    }
    fn workspace(&self) -> &Workspace {
        &self.projects[self.project]
    }
    fn query(&self, cx: &App) -> String {
        self.query.read(cx).content.to_lowercase()
    }
    fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        action: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        // GPUI's on_click also handles Space and Enter for focused buttons.
        let label: SharedString = label.into();
        let state_styled = matches!(
            action,
            Action::Project(_) | Action::Section(_) | Action::Filter(_) | Action::Group(_)
        );
        let chrome_button = matches!(
            action,
            Action::AppMenu
                | Action::ToggleLeftPanel
                | Action::ToggleSidePanel
                | Action::ModeSettings
                | Action::Agents
                | Action::Projects
        );
        row()
            .id(id)
            .focusable()
            .tab_stop(true)
            .gap_2()
            .px_3()
            .h(px(34.))
            .rounded(px(8.))
            .cursor_pointer()
            .text_size(px(12.))
            .when_some(action.menu_target(), |d, target| {
                let triggers = self.menu_triggers.clone();
                d.relative().child(
                    canvas(
                        move |bounds, _, _| {
                            triggers.borrow_mut().insert(target, bounds);
                        },
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            // Icon-only buttons pass an empty label; skipping it keeps the
            // row gap from pushing their icons off centre.
            .when(!label.is_empty(), |d| d.child(label))
            // Chrome controls respond to hover without latching a click/focus
            // background or border when a menu or panel remains open.
            .when(chrome_button, |d| {
                d.hover(|s| {
                    s.bg(rgb(theme::popover()))
                        .text_color(rgb(theme::popover_foreground()))
                })
            })
            .when(!chrome_button && !state_styled, |d| {
                d.when(
                    !matches!(action, Action::Project(i) if i == self.project),
                    |d| d.hover(|s| s.bg(rgb(theme::secondary()))),
                )
                .when(!matches!(action, Action::Machines), |d| {
                    d.focus(|s| {
                        s.bg(rgb(theme::sidebar_accent()))
                            .border_1()
                            .border_color(rgb(theme::ring()))
                    })
                })
            })
            .on_click(cx.listener(move |s, _, w, cx| s.act(action.clone(), w, cx)))
    }
    fn ib(
        &self,
        id: &'static str,
        name: &str,
        action: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        self.button(id, "", action, cx)
            .w(px(34.))
            .px_0()
            .justify_center()
            .child(icon(name))
    }
    fn search_box(&self, _cx: &Context<Self>) -> Div {
        row()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .px_3()
            .h(px(39.))
            .bg(rgb(theme::input()))
            .border_1()
            .border_color(rgb(theme::border()))
            .rounded(px(10.))
            .child(icon("search").size(px(14.)))
            .child(div().flex_1().min_w_0().child(self.query.clone()))
    }
    fn notify_toast(&mut self, value: &str, cx: &mut Context<Self>) {
        self.toast = Some(value.into());
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(3))
                .await;
            let _ = this.update(cx, |s, cx| {
                s.toast = None;
                cx.notify();
            });
        })
        .detach();
    }
    fn project_icon(&self, i: usize, large: bool) -> Div {
        row()
            .w(px(24.))
            .h(px(26.))
            .justify_center()
            .flex_shrink_0()
            .child(
                icon(["project-circle", "project-triangle", "project-square"][i.min(2)])
                    .size(px(if large { 22. } else { 18. }))
                    .text_color(rgb(theme::project_colors()[self.project_tints[i]])),
            )
    }
    fn menu_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        action: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        self.button(id, label, action, cx)
            .w_full()
            .h(px(32.))
            .px(px(10.))
            .rounded(px(3.))
            .text_size(px(13.))
            .text_color(rgb(theme::sidebar_foreground()))
            .hover(|s| s.bg(rgb(theme::secondary())))
    }
    fn project_notifications(&self, (working, unread): (usize, usize)) -> Div {
        row()
            .gap_2()
            .flex_shrink_0()
            .when(working > 0, |d| {
                d.child(
                    row()
                        .gap(px(3.))
                        .child(icon("working").size(px(12.)))
                        .child(count_chip(working.to_string())),
                )
            })
            .when(unread > 0, |d| {
                d.child(
                    div()
                        .relative()
                        .w(px(26.))
                        .h(px(24.))
                        .child(
                            icon("chat")
                                .size(px(17.))
                                .absolute()
                                .left_0()
                                .bottom(px(2.)),
                        )
                        .child(
                            text(unread.to_string(), 9., theme::primary_foreground())
                                .font_weight(FontWeight::SEMIBOLD)
                                .absolute()
                                .top(px(-2.))
                                .right_0()
                                .min_w(px(14.))
                                .h(px(14.))
                                .px(px(2.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .line_height(px(12.))
                                .bg(rgb(theme::primary()))
                                .rounded_full(),
                        ),
                )
            })
    }
    fn header(&self, cx: &Context<Self>) -> Div {
        let mut tabs = row()
            .relative()
            .h(px(46.))
            .flex_shrink_0()
            .bg(rgb(theme::background()))
            .text_color(rgb(theme::foreground()))
            .px_4()
            .gap_2()
            // Paint the divider behind the tabs. The active tab's surface
            // covers its segment, joining it to the mode bar below.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(rgb(theme::border())),
            )
            .child(
                self.button("machines", "", Action::Machines, cx)
                    .flex_shrink_0()
                    .child(icon("devices"))
                    .child(MACHINES[self.machine].name)
                    .child(icon("chevron").size(px(12.))),
            );
        let mut project_tabs = row()
            .id("project-tabs-scroll")
            .h_full()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .gap_2();
        for i in 0..self.projects.len() {
            let active = i == self.project;
            let p = &self.projects[i];
            // The 18px SVG sits in a 24px box and each shape has its own
            // viewBox inset. Measure tab padding from the visible shape.
            let shape_inset = 3. + [3., 2., 4.][i.min(2)] * 18. / 24.;
            project_tabs = project_tabs.child(
                row()
                    .flex_shrink_0()
                    .h(px(40.))
                    .mt(px(6.))
                    .rounded(px(0.))
                    .child(
                        self.button(("project", i), "", Action::Project(i), cx)
                            .relative()
                            .rounded(px(0.))
                            .pl(px(11. - shape_inset))
                            .pr(px(11.))
                            // Reserve identical border space in every state.
                            .border_t_1()
                            .border_l_1()
                            .border_r_1()
                            .border_color(if active {
                                Hsla::from(rgb(theme::border()))
                            } else {
                                Hsla::from(rgba(0x00000000))
                            })
                            .bg(rgb(if active {
                                theme::sidebar()
                            } else {
                                theme::background()
                            }))
                            .text_color(rgb(if active {
                                theme::sidebar_foreground()
                            } else {
                                theme::foreground()
                            }))
                            .when(!active, |d| {
                                d.hover(|s| {
                                    s.bg(rgb(theme::popover()))
                                        .text_color(rgb(theme::popover_foreground()))
                                })
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(-1.))
                                        .right(px(-1.))
                                        .bottom_0()
                                        .h(px(1.))
                                        .bg(rgb(theme::border())),
                                )
                            })
                            .h_full()
                            .gap_2()
                            .child(self.project_icon(i, false).relative().when(
                                p.attention_count() > 0,
                                |d| {
                                    d.child(
                                        text(
                                            p.attention_count().to_string(),
                                            9.,
                                            theme::primary_foreground(),
                                        )
                                        .absolute()
                                        .top(px(-5.))
                                        .right(px(-5.))
                                        .min_w(px(14.))
                                        .h(px(14.))
                                        .px(px(2.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .line_height(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .bg(rgb(theme::primary()))
                                        .rounded_full(),
                                    )
                                },
                            ))
                            .child(
                                div()
                                    .child(p.config.name.clone())
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD),
                            ),
                    ),
            );
        }
        tabs = tabs
            .child(project_tabs)
            .child(
                self.button("machine-agents", "", Action::Agents, cx)
                    .flex_shrink_0()
                    .child(icon("sparkle"))
                    .child("Agents")
                    .child(icon("chevron").size(px(15.))),
            )
            .child(
                self.button("projects", "", Action::Projects, cx)
                    .flex_shrink_0()
                    .px(px(10.))
                    .rounded(px(9.))
                    .text_color(rgb(theme::foreground()))
                    .child(icon("folder").size(px(15.)))
                    .child("Projects")
                    .child(icon("chevron").size(px(15.))),
            );
        let mut nav = row()
            .bg(rgb(theme::sidebar()))
            .text_color(rgb(theme::sidebar_foreground()))
            .h(px(52.))
            .px_4()
            .gap_1()
            .border_b_1()
            .border_color(rgb(theme::border()));
        for (i, (s, name, ico)) in [
            (Section::Chats, "Chats", "chat"),
            (Section::Groupchats, "Groupchats", "group"),
            (Section::Issues, "Issues", "check-square"),
            (Section::Whiteboard, "Whiteboard", "whiteboard"),
            (Section::Docs, "Docs", "file"),
            (Section::Workflows, "Workflows", "workflow"),
            (Section::Services, "Services", "service"),
        ]
        .into_iter()
        .enumerate()
        {
            if !config::current().general.features.enabled(s) {
                continue;
            }
            let active = self.section == s;
            let color = if active {
                theme::card_foreground()
            } else {
                theme::sidebar_foreground()
            };
            let notification_count = match s {
                Section::Chats => self.workspace().notifications().1 + self.workspace().counts[1],
                Section::Services => self
                    .services
                    .iter()
                    .filter(|service| service.project_id == self.workspace().config.id)
                    .count(),
                _ => 0,
            };
            nav =
                nav.child(
                    self.button(("section", i), "", Action::Section(s), cx)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(color))
                        .bg(rgb(if active {
                            theme::card()
                        } else {
                            theme::sidebar()
                        }))
                        .when(!active, |d| {
                            d.hover(|s| {
                                s.bg(rgb(theme::popover()))
                                    .text_color(rgb(theme::popover_foreground()))
                            })
                        })
                        .child(div().relative().child(icon(ico)).when(
                            notification_count > 0,
                            |d| {
                                d.child(
                                    text(
                                        notification_count.to_string(),
                                        9.,
                                        theme::primary_foreground(),
                                    )
                                    .absolute()
                                    .top(px(-7.))
                                    .right(px(-7.))
                                    .min_w(px(14.))
                                    .h(px(14.))
                                    .px(px(2.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .line_height(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .bg(rgb(theme::primary()))
                                    .rounded_full(),
                                )
                            },
                        ))
                        .child(name),
                );
        }
        col().flex_shrink_0().child(tabs).child(nav)
    }
}
impl Render for Adeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Shell);
        let font: SharedString = config::font().into();
        let body = match self.section {
            Section::Chats => self.chats(cx),
            Section::Docs => self.files_region.clone().into_any_element(),
            Section::Workflows => self.workflows(cx),
            Section::Services => self.services_region.clone().into_any_element(),
            Section::Groupchats | Section::Issues | Section::Whiteboard => {
                self.collaboration_body(cx)
            }
        };
        let body = self.workspace_with_left_panel(body, cx);
        let body = row()
            .size_full()
            .items_start()
            .child(div().flex_1().min_w_0().h_full().child(body))
            .when(self.side_panel_is_open(), |d| {
                d.child(self.page_side_panel(cx))
            });
        let content = div()
            .w_full()
            .flex_1()
            .min_h_0()
            .relative()
            .font_family(font.clone())
            .text_size(px(14.))
            .line_height(relative(1.5))
            .text_color(rgb(theme::foreground()))
            .bg(rgb(theme::background()))
            .track_focus(&self.focus)
            .key_context("Adeline")
            .on_action(cx.listener(|s, _: &Dismiss, w, cx| s.act(Action::Close, w, cx)))
            .on_action(cx.listener(|s, _: &OpenSettings, w, cx| s.act(Action::AppSettings, w, cx)))
            .on_action(cx.listener(|s, _: &NewThread, w, cx| s.act(Action::NewChat, w, cx)))
            .on_action(cx.listener(|s, _: &Search, w, cx| {
                if !matches!(s.menu, Some("projects" | "agents"))
                    && matches!(s.section, Section::Chats | Section::Services)
                {
                    s.left_panel_open[s.section as usize] = true;
                    s.control_pane.update(cx, |_, cx| cx.notify());
                    cx.notify();
                }
                let input = if s.menu == Some("projects") {
                    &s.project_query
                } else if s.menu == Some("agents") {
                    &s.agent_query
                } else {
                    &s.query
                };
                w.focus(&input.focus_handle(cx));
            }))
            .on_action(cx.listener(|s, _: &SendMessage, w, cx| {
                s.act(
                    if s.section == Section::Groupchats {
                        Action::SendGroup
                    } else {
                        Action::Send
                    },
                    w,
                    cx,
                );
            }))
            .on_action(cx.listener(|_, _: &NextFocus, w, _| w.focus_next()))
            .on_action(cx.listener(|_, _: &PreviousFocus, w, _| w.focus_prev()))
            .on_mouse_move(cx.listener(|s, e: &MouseMoveEvent, w, cx| {
                if e.pressed_button != Some(MouseButton::Left) {
                    s.dragging = false;
                    s.dragging_right = false;
                    s.drawing = false;
                    return;
                }
                if s.dragging {
                    s.sidebar_width = f32::from(e.position.x).clamp(260., 520.);
                    cx.notify();
                }
                if s.dragging_right {
                    s.right_panel_width = (f32::from(w.viewport_size().width)
                        - f32::from(e.position.x))
                    .clamp(260., 520.);
                    s.log_region.update(cx, |_, cx| cx.notify());
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|s, _, _, _| {
                    s.dragging = false;
                    s.dragging_right = false;
                    s.drawing = false;
                }),
            )
            .child(
                col()
                    .size_full()
                    .child(
                        AnyView::from(self.header_region.clone()).cached(
                            StyleRefinement::default()
                                .w_full()
                                .h(px(98.))
                                .flex_shrink_0(),
                        ),
                    )
                    .child(div().flex_1().min_h_0().child(body))
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .border_t_1()
                            .border_color(rgb(theme::border()))
                            .child(self.control_pane.clone()),
                    ),
            )
            .when(self.menu.is_some(), |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(div().absolute().inset_0().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, event: &MouseDownEvent, w, cx| {
                                let on_trigger = s.menu.is_some_and(|menu| {
                                    s.menu_triggers
                                        .borrow()
                                        .get(menu)
                                        .is_some_and(|bounds| bounds.contains(&event.position))
                                });
                                // Its button handles toggling on click. Closing here
                                // on mouse-down would make that click reopen it.
                                if !on_trigger {
                                    s.act(Action::Close, w, cx);
                                }
                            }),
                        ))
                        .child(self.menu_view(window, cx)),
                )
            })
            .when(self.modal.is_some(), |d| d.child(self.modal_view(cx)))
            .when_some(self.toast.clone(), |d, s| {
                d.child(
                    div()
                        .absolute()
                        .bottom(px(120.))
                        .right(px(28.))
                        .px_5()
                        .py_3()
                        .bg(rgb(theme::foreground()))
                        .text_color(rgb(theme::primary_foreground()))
                        .rounded_lg()
                        .shadow(vec![theme::shadow(16.)])
                        .child(s),
                )
            });
        let shell = col()
            .size_full()
            .bg(rgb(theme::sidebar()))
            .font_family(font);
        #[cfg(target_os = "windows")]
        let shell = shell.child(titlebar::render(
            format!("{} · Adeline", self.workspace().config.name),
            window,
        ));
        shell.child(content)
    }
}
fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        config::init();
        theme::init();
        input::init(cx);
        config::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let bounds = Bounds::centered(None, size(px(1440.), px(940.)), cx);
        let main_window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(900.), px(620.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("ade-project".into()),
                        appears_transparent: cfg!(target_os = "windows"),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(Adeline::new);
                    #[cfg(target_os = "windows")]
                    view.update(cx, |app, cx| {
                        app.subscriptions
                            .push(cx.observe_window_bounds(window, |_, _, cx| cx.notify()));
                        app.subscriptions
                            .push(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
                    });
                    window.focus(&view.read(cx).focus);
                    view
                },
            )
            .expect("open Adeline window");
        // Observe actual window removal so native close controls and keyboard
        // shortcuts both dismiss settings belonging to this workspace.
        cx.on_window_closed(move |cx| {
            if !cx
                .windows()
                .iter()
                .any(|window| window.window_id() == main_window.window_id())
            {
                settings::close_for(main_window, cx);
            }
        })
        .detach();
        cx.activate(true);
    });
}
