#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod acp;
mod agent_form;
mod agents;
mod chat;
mod chat_render;
mod config;
mod data;
mod fonts;
mod interaction;
mod panes;
mod prepared;
mod project_ui;
mod recency;
mod runtime_ui;
mod settings;
mod storage;
mod themed_icon;
#[cfg(target_os = "windows")]
mod titlebar;
mod ui_metrics;
mod views;
use data::*;
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::{
    ActiveTheme, Icon, Root, Selectable, Sizable, TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState, TextareaState},
};
use gpui_kit::{prelude::*, *};
use std::borrow::Cow;
mod theme;

struct Machine {
    name: &'static str,
    kind: &'static str,
}
const MACHINES: [Machine; 3] = [
    Machine {
        name: "Nexus",
        kind: "Local machine",
    },
    Machine {
        name: "Matrix",
        kind: "Remote machine",
    },
    Machine {
        name: "Vortex",
        kind: "Remote machine",
    },
];
include!(concat!(env!("OUT_DIR"), "/assets.rs"));
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match embedded(path) {
            Some(bytes) => Ok(Some(Cow::Borrowed(bytes))),
            None => assets::Assets.load(path),
        }
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        assets::Assets.list(path)
    }
}
fn row() -> Div {
    div().flex().items_center()
}
fn col() -> Div {
    div().flex().flex_col()
}
// Published Kit 0.6.6 owns overlay state in Root; window views mount its layers.
fn window_layers(window: &mut Window, cx: &mut App) -> [Option<AnyElement>; 3] {
    [
        Root::render_sheet_layer(window, cx).map(IntoElement::into_any_element),
        Root::render_dialog_layer(window, cx).map(IntoElement::into_any_element),
        Root::render_notification_layer(window, cx).map(IntoElement::into_any_element),
    ]
}
fn text(s: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(config::text_pixels(size))
        .text_color(rgb(color))
        .child(s.into())
}
fn icon(name: &str) -> themed_icon::ThemedIcon {
    themed_icon::ThemedIcon::new(name)
        .size(px(16.))
        .flex_shrink_0()
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
    Project(usize),
    CloseProject(usize),
    AddProject,
    SaveProject,
    DeleteProject,
    ConfirmDeleteProject,
    ForceDeleteProject,
    HideToolCalls,
    AddAgent,
    Section(Section),
    Chat(usize),
    NewChat,
    Filter(usize),
    /// Show one agent's chats, or every agent's with `None`.
    AgentFilter(Option<std::sync::Arc<str>>),
    ClearChatFilters,
    ShowCompleted,
    ToggleSidePanel,
    ToggleLeftPanel,
    ResizePanel(usize, f32),
    ResetPanels,
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
    Agent(String),
    Speed(usize),
    Permission(usize),
    ToggleMode(Section),
    ToggleMachineSelector,
    Send,
    Stop,
    ForceStop,
    ForceStopAll,
    RetryPrompt,
    RetryStorage,
    ReplaceSession,
    PermissionResponse(String),
    ToggleTool(String),
    ArchiveChat,
    Decision(usize),
    InsertFiles,
    AddFile,
    AddDirectory,
    Tint(usize),
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
            Self::InsertFiles => Some("files"),
            _ => None,
        }
    }
}
actions!(
    adeline,
    [
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
    main_window: WindowHandle<Root>,
    projects: Vec<Workspace>,
    open_projects: Vec<bool>,
    empty_workspace: Workspace,
    demo_mode: bool,
    agent_catalog: agents::AgentCatalog,
    selected_agent: Option<String>,
    agent_watcher: Option<notify::RecommendedWatcher>,
    project_store: std::sync::Arc<std::sync::Mutex<storage::ProjectStore>>,
    runtime: runtime_ui::Runtime,
    project_directory_input: Entity<InputState>,
    project_error: Option<String>,
    delete_project: Option<usize>,
    project: usize,
    machine: usize,
    section: Section,
    selected: Option<usize>,
    filter: usize,
    agent_filter: Option<std::sync::Arc<str>>,
    show_completed: bool,
    side_panel_open: [bool; 7],
    right_panel_width: f32,
    panel_state: Entity<component::ResizableState>,
    expanded_event: Option<usize>,
    query: Entity<InputState>,
    composer: Entity<TextareaState>,
    name_input: Entity<InputState>,
    modal: Option<&'static str>,
    menu: Option<&'static str>,
    command_popup: Option<Entity<component::command::CommandState>>,
    speed: usize,
    permission: usize,
    left_panel_open: [bool; 7],
    sidebar_width: f32,
    selected_tint: usize,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    project_tints: Vec<usize>,
    chat_list: Entity<chat::ChatList>,
    transcript: Entity<chat::Transcript>,
    composer_region: Entity<chat::Composer>,
    header_region: Entity<chat::Header>,
    control_pane: Entity<panes::ControlPane>,
}
impl Adeline {
    fn new(demo_mode: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = if demo_mode {
            storage::ProjectStore::empty()
        } else {
            storage::ProjectStore::new()
        };
        let projects = if demo_mode {
            load()
        } else {
            store.to_workspaces()
        };
        #[cfg(feature = "ui-profiling")]
        let projects = if demo_mode {
            ui_metrics::stress_fixture(projects)
        } else {
            projects
        };
        let mut projects = projects;
        for project in &mut projects {
            project.rebuild_counts();
        }
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search chats"));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 4)
                .placeholder("Ask your agent to do anything")
        });
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Project name"));
        let subscriptions = vec![cx.subscribe(&query, |app, _, event: &InputEvent, cx| {
            match event {
                InputEvent::Change => app.search_sidebar(cx),
                // The search icon lights up while the field is focused.
                InputEvent::Focus | InputEvent::Blur => {
                    app.chat_list.update(cx, |_, cx| cx.notify());
                }
                InputEvent::PressEnter { .. } => {}
            }
        })];
        let owner = cx.weak_entity();
        let chat_list = cx.new(|_| chat::ChatList::new(owner.clone()));
        let transcript = cx.new(|cx| chat::Transcript::new(owner.clone(), cx));
        let composer_region = cx.new(|cx| chat::Composer::new(owner.clone(), &composer, cx));
        let control_pane = cx.new(|_| panes::ControlPane::new(owner.clone()));
        let header_region = cx.new(|_| chat::Header(owner.clone()));
        let mut app = Self {
            main_window: window
                .window_handle()
                .downcast::<Root>()
                .expect("Kit root window"),
            right_panel_width: 302.,
            panel_state: cx.new(|_| component::ResizableState::default()),
            empty_workspace: Workspace::default(),
            open_projects: vec![true; projects.len()],
            project_tints: (0..projects.len()).map(|i| [0, 2, 3][i.min(2)]).collect(),
            demo_mode,
            agent_catalog: agents::AgentCatalog::new(demo_mode),
            selected_agent: None,
            agent_watcher: None,
            project_store: std::sync::Arc::new(std::sync::Mutex::new(store)),
            runtime: runtime_ui::Runtime::default(),
            project_directory_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("Existing working directory")),
            project_error: None,
            delete_project: None,
            projects,
            project: 0,
            machine: 0,
            section: Section::Chats,
            selected: None,
            filter: 0,
            agent_filter: None,
            show_completed: true,
            side_panel_open: [false, false, false, false, true, true, true],
            expanded_event: None,
            query,
            composer,
            name_input,
            modal: None,
            menu: None,
            command_popup: None,
            speed: 0,
            permission: 2,
            left_panel_open: [true, false, false, true, true, true, true],
            sidebar_width: 360.,
            selected_tint: 3,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            chat_list,
            transcript,
            composer_region,
            header_region,
            control_pane,
        };
        app.load_settings();
        if demo_mode || app.agent_catalog.entries.len() == 1 {
            app.selected_agent = app
                .agent_catalog
                .entries
                .first()
                .map(|entry| entry.id.clone());
        }
        app.watch_agents(cx);
        app.watch_runtime(cx);
        if let Some(error) = theme::load_error() {
            app.notify_toast(&error, cx);
        }
        app.sync_sidebar(cx);
        app.transcript
            .update(cx, |view, cx| view.sync(&app, false, cx));
        app
    }
    fn selected_definition(&self) -> Option<&agents::AgentDefinition> {
        let id = self.selected_agent.as_ref()?;
        self.agent_catalog
            .entries
            .iter()
            .find(|entry| &entry.id == id)
            .map(|entry| &entry.definition)
    }

    fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        if self.agent_catalog.refresh() {
            self.agents_changed(None, None, false, cx);
        }
    }

    fn agents_changed(
        &mut self,
        previous_id: Option<&str>,
        saved_id: Option<&str>,
        select_first: bool,
        cx: &mut Context<Self>,
    ) {
        if select_first || (previous_id.is_some() && self.selected_agent.as_deref() == previous_id)
        {
            self.selected_agent = saved_id.map(str::to_owned);
        }
        if self.selected_definition().is_none() {
            self.selected_agent = None;
        }
        self.composer_region.update(cx, |_, cx| cx.notify());
        self.header_region.update(cx, |_, cx| cx.notify());
        self.control_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
        cx.refresh_windows();
    }

    fn watch_agents(&mut self, cx: &mut Context<Self>) {
        use notify::Watcher;
        if self.demo_mode {
            return;
        }
        let Ok(root) = config::directory() else {
            return;
        };
        let (sender, receiver) = async_channel::unbounded();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let _ = sender.try_send(event);
        })
        .and_then(|mut watcher| {
            watcher.watch(&root, notify::RecursiveMode::Recursive)?;
            Ok(watcher)
        });
        match watcher {
            Ok(watcher) => self.agent_watcher = Some(watcher),
            Err(error) => {
                self.agent_catalog.errors.push(format!(
                    "{}: cannot watch agent definitions: {error}",
                    root.display()
                ));
                return;
            }
        }
        cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                let relevant = match &event {
                    Ok(event) => {
                        !matches!(event.kind, notify::EventKind::Access(_))
                            && event
                                .paths
                                .iter()
                                .any(|path| path.starts_with(root.join("agents")))
                    }
                    Err(_) => true,
                };
                if !relevant {
                    continue;
                }
                if this
                    .update(cx, |app, cx| {
                        app.refresh_agents(cx);
                        if let Err(error) = event {
                            app.agent_catalog
                                .errors
                                .push(format!("{}: agent watcher: {error}", root.display()));
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
    fn has_open_project(&self) -> bool {
        self.open_projects
            .get(self.project)
            .copied()
            .unwrap_or(false)
    }
    fn workspace(&self) -> &Workspace {
        if !self.has_open_project() {
            return &self.empty_workspace;
        }
        self.projects
            .get(self.project)
            .unwrap_or(&self.empty_workspace)
    }
    fn query(&self, cx: &App) -> String {
        self.query.read(cx).value().to_lowercase()
    }
    fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        action: Action,
        cx: &Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .small()
            .on_click(cx.listener(move |app, _, window, cx| app.act(action.clone(), window, cx)))
    }
    fn icon_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        icon: Icon,
        action: Action,
        cx: &Context<Self>,
    ) -> Button {
        let label = label.into();
        Button::new(id)
            .icon(icon)
            .accessibility_label(label.clone())
            .tooltip(label)
            .small()
            .ghost()
            .on_click(cx.listener(move |app, _, window, cx| app.act(action.clone(), window, cx)))
    }
    fn notify_toast(&self, value: &str, cx: &mut Context<Self>) {
        let handle = self.main_window;
        let value = value.to_owned();
        cx.defer(move |cx| {
            let _ = cx.update_window(handle.into(), |_, window, cx| {
                window.push_notification(value.clone(), cx);
            });
        });
    }
    fn header(&self, window: &Window, cx: &Context<Self>) -> Div {
        let mut projects = row()
            .id("projects")
            .role(Role::TabList)
            .aria_label("Projects")
            .overflow_x_scroll()
            .min_w_0()
            .items_end()
            // Reach through the bar's bottom padding so the active tab covers the border line.
            .mb(px(-1.))
            .gap_1();
        for (ix, project) in self
            .projects
            .iter()
            .enumerate()
            .filter(|(ix, _)| self.open_projects[*ix])
        {
            let attention = project.attention_count();
            let active = ix == self.project;
            let name = if attention == 0 {
                project.config.name.clone()
            } else {
                format!("{} ({attention})", project.config.name)
            };
            projects = projects.child(
                project_tab(active, cx)
                    .pl_3()
                    .pr_1()
                    .child(
                        row()
                            .id(SharedString::from(format!("project-{}", project.config.id)))
                            .role(Role::Tab)
                            .aria_label(name.clone())
                            .aria_selected(active)
                            .h_full()
                            .gap_2()
                            .cursor_pointer()
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.act(Action::Project(ix), window, cx);
                            }))
                            .child(
                                div()
                                    .size_2()
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(rgb(theme::project_colors()[self.project_tints[ix]])),
                            )
                            .child(div().max_w(px(180.)).truncate().child(name)),
                    )
                    .child(self.icon_button(
                        SharedString::from(format!("close-{}", project.config.id)),
                        "Close project tab",
                        Icon::default().path("close.svg"),
                        Action::CloseProject(ix),
                        cx,
                    )),
            );
        }
        projects = projects.child(
            project_tab(false, cx)
                .id("new-project")
                .role(Role::Button)
                .aria_label("New project…")
                .px_2()
                .cursor_pointer()
                .on_click(cx.listener(|app, _, window, cx| app.act(Action::AddProject, window, cx)))
                .child(Icon::default().path("plus.svg").small()),
        );
        // The project toolbar is the window's title bar. Its empty space drags the window, so
        // everything clickable in it occludes the drag area beneath.
        let toolbar = row()
            .id("project-toolbar")
            .size_full()
            .items_end()
            .gap_2()
            .child(
                row()
                    .h(px(32.))
                    .flex_shrink_0()
                    .child(titlebar::app_icon(window)),
            )
            .child(projects)
            .child(div().flex_1())
            .child(
                row()
                    .self_center()
                    .flex_shrink_0()
                    .gap_2()
                    .pr_2()
                    .occlude()
                    .when(config::current().general.features.machine_selector, |row| {
                        row.child(self.button("machines", "Machines", Action::Machines, cx))
                    })
                    .child(
                        self.command_popover(
                            "projects",
                            Button::new("project-menu")
                                .icon(Icon::default().path("folder.svg"))
                                .label("Projects")
                                .small(),
                            Anchor::TopRight,
                            cx,
                        ),
                    ),
            );
        let top = div()
            .relative()
            .bg(cx.theme().title_bar)
            // Painted before the tabs so the active tab covers it and opens into the modes bar.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(cx.theme().border),
            )
            .child(
                TitleBar::new()
                    .h(titlebar::MAIN_HEIGHT)
                    .when(!cfg!(target_os = "macos"), |bar| bar.pl_2())
                    // Keeps the window buttons' hover fill above the border line.
                    .pb(px(1.))
                    .border_b_0()
                    .bg(transparent_black())
                    .child(toolbar),
            );
        let mut modes = row()
            .id("modes")
            .role(Role::TabList)
            .aria_label("Modes")
            .overflow_x_scroll()
            .gap_1()
            .px_2()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border);
        for (section, name, icon_path) in [
            (Section::Chats, "Chats", "chat.svg"),
            (Section::Docs, "Docs", "file.svg"),
            (Section::Workflows, "Workflows", "workflow.svg"),
            (Section::Services, "Services", "service.svg"),
            (Section::Groupchats, "Groupchats", "group.svg"),
            (Section::Issues, "Issues", "flag.svg"),
            (Section::Whiteboard, "Whiteboard", "whiteboard.svg"),
        ] {
            if self.has_open_project() && config::current().general.features.enabled(section) {
                let label = if section == Section::Chats {
                    let (_, unread) = self.workspace().notifications();
                    let attention = unread + self.workspace().counts[1];
                    if attention == 0 {
                        name.to_owned()
                    } else {
                        format!("{name} ({attention})")
                    }
                } else {
                    name.to_owned()
                };
                let active = self.section == section;
                // An underline marks the active mode; a selection fill would blur
                // the shared surface with the active project tab.
                modes = modes.child(
                    col()
                        .pt_1()
                        .child(
                            self.button(name, label, Action::Section(section), cx)
                                .icon(Icon::default().path(icon_path))
                                .ghost()
                                .toggled(active)
                                .when(active, |button| button.text_color(cx.theme().foreground)),
                        )
                        .child(
                            div()
                                .mx_2()
                                .mt_1()
                                .h(px(2.))
                                .rounded_full()
                                .when(active, |line| line.bg(cx.theme().foreground)),
                        ),
                );
            }
        }
        col()
            .flex_shrink_0()
            .child(top)
            .when(self.has_open_project(), |header| header.child(modes))
    }
}
// The active project tab shares the modes bar background so the two read as one surface.
fn project_tab(active: bool, cx: &App) -> Div {
    let tab = row()
        .flex_shrink_0()
        .h(px(32.))
        .gap_2()
        .text_sm()
        .occlude()
        .rounded_t(cx.theme().radius)
        .border_t_1()
        .border_l_1()
        .border_r_1()
        .border_color(cx.theme().border);
    if active {
        tab.bg(cx.theme().background)
            .text_color(cx.theme().foreground)
    } else {
        tab.bg(cx.theme().muted)
            .border_b_1()
            .text_color(cx.theme().muted_foreground)
            .hover(|style| style.text_color(cx.theme().foreground))
    }
}
impl Render for Adeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Shell);
        let body = if !self.has_open_project() {
            col()
                .size_full()
                .items_center()
                .justify_center()
                .gap_4()
                .p_6()
                .child(div().text_lg().child("Open a project to start a chat"))
                .child(
                    row()
                        .gap_2()
                        .child(self.button("empty-create", "New project…", Action::AddProject, cx))
                        .child(self.button("empty-open", "Open project…", Action::Projects, cx)),
                )
                .into_any_element()
        } else if self.section == Section::Chats {
            self.workspace_panels(cx)
        } else {
            div().size_full().into_any_element()
        };
        let content = col()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus)
            .key_context("Adeline")
            .on_action(
                cx.listener(|app, _: &Cancel, window, cx| app.act(Action::Close, window, cx)),
            )
            .on_action(cx.listener(|app, _: &OpenSettings, window, cx| {
                app.act(Action::AppSettings, window, cx);
            }))
            .on_action(
                cx.listener(|app, _: &NewThread, window, cx| app.act(Action::NewChat, window, cx)),
            )
            .on_action(cx.listener(|app, _: &Search, window, cx| {
                if app.has_open_project() && app.section == Section::Chats {
                    app.left_panel_open[0] = true;
                    window.focus(&app.query.focus_handle(cx), cx);
                    cx.notify();
                } else {
                    app.act(Action::Projects, window, cx);
                }
            }))
            .on_action(
                cx.listener(|app, _: &SendMessage, window, cx| app.act(Action::Send, window, cx)),
            )
            .on_action(cx.listener(|_, _: &NextFocus, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &PreviousFocus, window, cx| window.focus_prev(cx)))
            .child(self.header_region.clone())
            .child(div().flex_1().min_h_0().child(body))
            .child(self.control_pane.clone());
        col()
            .size_full()
            .bg(cx.theme().background)
            .child(content)
            .children(window_layers(window, cx).into_iter().flatten())
    }
}

fn main() {
    let demo_mode = std::env::args().any(|arg| arg == "--demo");
    application().with_assets(Assets).run(move |cx: &mut App| {
        init(cx);
        fonts::init(cx);
        config::init();
        theme::init();
        theme::apply(cx);
        config::bind_keys(cx);
        cx.on_action(|_: &NextFocus, cx| {
            if let Some(handle) = cx.active_window() {
                let _ = cx.update_window(handle, |_, window, cx| window.focus_next(cx));
            }
        });
        cx.on_action(|_: &PreviousFocus, cx| {
            if let Some(handle) = cx.active_window() {
                let _ = cx.update_window(handle, |_, window, cx| window.focus_prev(cx));
            }
        });
        // These pixels describe native window geometry, not control spacing.
        let bounds = Bounds::centered(None, size(px(1440.), px(940.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.), px(600.))),
                ..titlebar::main_window_options()
            },
            move |window, cx| {
                let view = cx.new(|cx| Adeline::new(demo_mode, window, cx));
                let handle = window
                    .window_handle()
                    .downcast::<Root>()
                    .expect("Kit root window");
                let owner = view.downgrade();
                let closing_owner = owner.clone();
                window.on_window_should_close(cx, move |_, cx| {
                    settings::request_close(handle, closing_owner.clone(), cx);
                    false
                });
                let quit_owner = owner;
                cx.on_action(move |_: &Quit, cx| {
                    settings::request_close(handle, quit_owner.clone(), cx);
                });
                cx.on_window_closed(move |cx, closed| {
                    if closed == handle.window_id() {
                        settings::close_for(handle, cx);
                    }
                })
                .detach();
                view.update(cx, |app, cx| window.focus(&app.focus, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("open Adeline window");
        cx.activate(true);
    });
}
