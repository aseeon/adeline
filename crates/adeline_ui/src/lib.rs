//! The desktop UI: the `Adeline` root view, its `Action`s and the window it opens.
//! Every UI event is an `Action`. The headless build leaves this crate out.
//!
//! A frame redraws the views notified since the last one and the views above
//! them. Most regions are cached views (`chat::region`), and the chat list and
//! the transcript cache each row instead (see `chat.rs`). They read `Adeline`'s
//! state, so `notify_cached_views` notifies them whenever `Adeline` is notified.
//! GPUI leaves a replayed view out of the accessibility tree, so while
//! assistive technology is connected nothing is cached.
mod activity;
mod agent_form;
mod chat;
mod client;
#[cfg(feature = "control")]
pub mod control;
mod conversation_ui;
mod docs;
mod fonts;
#[cfg(feature = "profiler")]
mod frame_log;
mod interaction;
mod mcp_ui;
mod menu_bar;
mod panes;
mod project_bar;
mod project_ui;
mod runtime_ui;
mod settings;
mod theme;
mod themed_icon;
mod titlebar;
mod ui_state;
mod views;
use adeline_assets::embedded;
use adeline_core::{
    agents, config, conversation, data, harness, install, ipc, machines, prepared, profiles,
    protocol, recency,
};
use config::Section;
use data::*;
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::{
    ActiveTheme, Icon, Root, Selectable, Sizable, TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState, TextareaState},
};
use gpui_kit::{prelude::*, *};
use std::borrow::Cow;

#[derive(Debug)]
pub struct Assets;
// The harnesses of the machine whose project is open.
#[derive(Clone, Default)]
struct Harnesses(harness::Catalog);
impl Global for Harnesses {}
impl std::ops::Deref for Harnesses {
    type Target = harness::Catalog;
    fn deref(&self) -> &harness::Catalog {
        &self.0
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match embedded(path) {
            Some(bytes) => Ok(Some(Cow::Borrowed(bytes))),
            None => match harness::icon(path) {
                Some(bytes) => Ok(Some(Cow::Owned(bytes))),
                None => assets::Assets.load(path),
            },
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
fn text(s: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(fonts::text_pixels(size))
        .text_color(rgb(color))
        .child(s.into())
}
fn search_field(input: Input, state: &Entity<InputState>, window: &Window, cx: &App) -> Input {
    use gpui_kit::base::FocusableExt as _;
    let focused = state.focus_handle(cx).contains_focused(window, cx);
    input
        .focus_ring(false)
        .when(focused, |input| input.border_color(cx.theme().primary))
}
fn icon(name: &str) -> themed_icon::ThemedIcon {
    themed_icon::ThemedIcon::new(name)
        .size(px(16.))
        .flex_shrink_0()
}
fn provider(s: &str) -> &'static str {
    if s == "claude" {
        "Claude Code"
    } else {
        "Codex"
    }
}
#[derive(Clone)]
enum Action {
    Project(usize),
    CloseProject(usize),
    RemoveClosedProject(String),
    UndoProjectRemoval(String),
    RenameProject(usize),
    SaveRename,
    ToggleProjectSort,
    OpenFolder,
    AddProject,
    SaveProject,
    DeleteProject,
    ConfirmDeleteProject,
    ForceDeleteProject,
    HideToolCalls,
    SubmitOnEnter,
    AddAgent,
    Section(Section),
    Chat(usize),
    NewChat,
    AgentFilter(Option<std::sync::Arc<str>>),
    ClearChatFilters,
    ShowCompleted,
    ShowArchived,
    ToggleSidePanel,
    ToggleLeftPanel,
    Complete,
    Projects,
    AppMenu,
    Machines,
    Agents,
    Machine(String),
    MachineRetry(String),
    ManageMachines,
    MachineUpgrade(String),
    ConfirmUpgrade,
    // True accepts or submits, false cancels.
    AnswerPrompt(bool),
    FolderMachine(String),
    ProjectMachine(String),
    BrowseTo(std::path::PathBuf),
    // None picks the shown folder.
    BrowsePick(Option<String>),
    About,
    QuitApp,
    Settings,
    AppSettings,
    AgentSettings,
    ModeSettings,
    ConfigureModeSettings,
    Close,
    SaveSettings,
    AgentMenu,
    Agent(String),
    Speed(usize),
    ToggleMode(Section),
    // Queued during a turn.
    Send,
    // Sends the queue when the composer is empty.
    SendNow,
    OptionMenu(conversation::Category),
    SetOption {
        category: conversation::Category,
        option: String,
        value: String,
    },
    EditQueued(u64),
    EditLastQueued,
    RemoveQueued(u64),
    SendQueuedNow(u64),
    AttachFile,
    RemoveAttachment(usize),
    DismissError(String),
    // Message, attachment.
    PreviewImage(usize, usize),
    ToggleThought(String),
    ShowThinking,
    TodoList,
    TrafficView,
    PanelTab(panes::SideTab),
    CopyTraffic,
    Login(String),
    Restart,
    Stop,
    ForceStop,
    StopAll,
    QuitStopAll,
    FinishInBackground,
    StartEngine,
    EngineRetry,
    EngineWait,
    EngineStopOld,
    RetryPrompt,
    RetryStorage,
    ReplaceSession,
    PermissionResponse(String),
    ToggleTool(String),
    ReplyTo(usize),
    CopyMessage(usize),
    Fork(usize),
    ArchiveChat(usize),
    Decision(usize),
    InsertFiles,
    AddFile,
    AddDirectory,
    Tint(usize),
    Docs(docs::Act),
    OpenCard {
        route: adeline_core::card::EntityRef,
        chat: String,
    },
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
        PreviousFocus,
        SendNow,
        StopTurn,
        RestartAgent,
        ModelMenu,
        EffortMenu,
        ModeMenu,
        MoreMenu,
        AttachFile,
        TrafficView,
        TodoList,
        NextProject,
        PreviousProject,
        CloseProjectTab,
        OpenFolder,
        MachinesMenu,
        FocusMessage,
        DismissErrors,
        ForkChat,
        ForkSource,
        CopyCode,
        ToggleChatList,
        ToggleSidePane,
        ProjectPalette
    ]
);
pub struct Adeline {
    main_window: WindowHandle<Root>,
    projects: Vec<Workspace>,
    open_projects: Vec<bool>,
    empty_workspace: Workspace,
    demo_mode: bool,
    // `--verbose`: shows debugging views, such as the ACP traffic tab.
    verbose: bool,
    catalogs: std::collections::HashMap<String, agents::AgentCatalog>,
    empty_catalog: agents::AgentCatalog,
    loaded_machines: std::collections::HashSet<String>,
    project_machine: String,
    browser: Option<project_ui::Browser>,
    browser_return: Option<&'static str>,
    upgrade_machine: Option<String>,
    prompt_input: Entity<InputState>,
    selected_agent: Option<String>,
    runtime: runtime_ui::Runtime,
    project_directory_input: Entity<InputState>,
    project_error: Option<String>,
    delete_project: Option<String>,
    rename_project: Option<String>,
    project_search: Entity<InputState>,
    project_sort: project_bar::ProjectSort,
    project_highlight: Option<String>,
    project_list_scroll: ScrollHandle,
    pending_removal: Option<project_bar::PendingRemoval>,
    removal_generation: u64,
    project: usize,
    section: Section,
    selected: Option<usize>,
    agent_filter: Option<std::sync::Arc<str>>,
    chat_search_open: bool,
    // The composer floats over the end of the transcript.
    composer_height: std::rc::Rc<std::cell::Cell<Pixels>>,
    show_completed: bool,
    show_archived: bool,
    side_panel_open: bool,
    activity: activity::ActivityPanel,
    query: Entity<InputState>,
    composer: Entity<TextareaState>,
    name_input: Entity<InputState>,
    modal: Option<&'static str>,
    menu: Option<&'static str>,
    command_popup: Option<Entity<component::command::CommandState>>,
    speed: usize,
    attachments: Vec<conversation::Attachment>,
    attachment_error: Option<String>,
    // Chat, its latest turn, message.
    // ponytail: in memory only, so a restart shows a dismissed error of the latest turn again.
    dismissed_errors: std::collections::HashSet<(String, Option<usize>, String)>,
    slash: Option<usize>,
    // Closed with Escape for the text typed so far.
    slash_dismissed: bool,
    side_tab: panes::SideTab,
    // Newest last.
    traffic: Vec<conversation::TrafficEntry>,
    traffic_scroll: ScrollHandle,
    open_thoughts: std::collections::HashSet<String>,
    logged_in: std::collections::HashSet<String>,
    pictures: std::cell::RefCell<std::collections::HashMap<String, std::sync::Arc<Image>>>,
    left_panel_open: bool,
    selected_tint: usize,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    project_tints: Vec<usize>,
    chat_list: Entity<chat::ChatList>,
    transcript: Entity<chat::Transcript>,
    composer_region: Entity<chat::Composer>,
    header_region: Entity<chat::Header>,
    side_panel: Entity<conversation_ui::SidePanel>,
    chat_header: Entity<views::ChatHeader>,
    docs: docs::Docs,
}
impl std::fmt::Debug for Adeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Adeline").finish_non_exhaustive()
    }
}
impl Adeline {
    fn new(
        demo_mode: bool,
        verbose: bool,
        snapshot: Option<protocol::Snapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut loaded_machines = std::collections::HashSet::new();
        if snapshot.is_some() {
            loaded_machines.insert(machines::LOCAL.to_owned());
        }
        let snapshot = snapshot.unwrap_or_default();
        let checked = machines::checked();
        let mut catalogs = std::collections::HashMap::new();
        let projects = if demo_mode {
            let mut projects = load();
            projects.retain(|project| checked.contains(&project.machine));
            for machine in &checked {
                catalogs.insert(machine.clone(), agents::AgentCatalog::new(true));
            }
            projects
        } else {
            let mut projects = snapshot.projects;
            for project in &mut projects {
                project.machine = machines::LOCAL.into();
            }
            ui_state::apply(machines::LOCAL, &mut projects);
            let mut catalog = agents::AgentCatalog::remote();
            catalog.entries = snapshot.agents;
            catalog.errors = snapshot.agent_errors;
            catalogs.insert(machines::LOCAL.to_owned(), catalog);
            projects
        };
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search chats"));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(config::current().modes.chats.submit_on_enter)
                .placeholder("Ask your agent to do anything")
        });
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Project name"));
        let project_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search projects"));
        let mut subscriptions =
            vec![
                cx.subscribe_in(
                    &query,
                    window,
                    |app, _, event: &InputEvent, window, cx| match event {
                        InputEvent::Change => app.search_sidebar(cx),
                        // The search icon lights up while the field is focused; an empty
                        // search folds away once focus leaves it.
                        InputEvent::Focus | InputEvent::Blur => {
                            if matches!(event, InputEvent::Blur)
                                && app.query.read(cx).value().is_empty()
                            {
                                app.chat_search_open = false;
                            }
                            app.chat_list.update(cx, |_, cx| cx.notify());
                        }
                        // Enter opens the first chat the search lists.
                        InputEvent::PressEnter { .. } => {
                            if let Some(ix) = app.chat_list.read(cx).first_result() {
                                app.act(Action::Chat(ix), window, cx);
                            }
                        }
                    },
                ),
            ];
        subscriptions.push(
            cx.subscribe(
                &project_search,
                |app, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => app.search_projects(cx),
                    // The field's border marks focus.
                    InputEvent::Focus | InputEvent::Blur => {
                        app.header_region.update(cx, |_, cx| cx.notify());
                    }
                    InputEvent::PressEnter { .. } => {}
                },
            ),
        );
        subscriptions.push(cx.subscribe_in(
            &composer,
            window,
            |app, _, event: &InputEvent, window, cx| {
                // With submit on Enter, the textarea reports a plain Enter
                // instead of inserting a line.
                if matches!(
                    event,
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false
                    }
                ) && config::current().modes.chats.submit_on_enter
                {
                    app.act(Action::Send, window, cx);
                }
            },
        ));
        let owner = cx.weak_entity();
        let chat_list = cx.new(|cx| chat::ChatList::new(owner.clone(), cx));
        let composer_height = std::rc::Rc::new(std::cell::Cell::new(px(0.)));
        let transcript = cx.new(|cx| {
            chat::Transcript::new(owner.clone(), std::rc::Rc::clone(&composer_height), cx)
        });
        let composer_region = cx.new(|cx| chat::Composer::new(owner.clone(), &composer, cx));
        let header_region = cx.new(|_| chat::Header(owner.clone()));
        let side_panel = cx.new(|_| conversation_ui::SidePanel(owner.clone()));
        let chat_header = cx.new(|_| views::ChatHeader(owner.clone()));
        subscriptions.push(cx.observe_self(Self::notify_cached_views));
        // A focused element that disappears, such as a closed menu's search box,
        // leaves keys dispatching above the "Adeline" context, so shortcuts stop.
        subscriptions.push(cx.on_focus_lost(window, |app, window, cx| {
            if app.has_open_project() && app.section == Section::Chats {
                window.focus(&app.composer.focus_handle(cx), cx);
            } else {
                window.focus(&app.focus, cx);
            }
        }));
        let docs = docs::Docs::new(window, cx);
        subscriptions.push(
            cx.subscribe(&docs.editor, |app, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    app.docs_edited(cx);
                }
            }),
        );
        subscriptions.push(
            cx.subscribe(&docs.search, |app, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    app.docs_refresh_rows(cx);
                }
            }),
        );
        let mut app = Self {
            main_window: window
                .window_handle()
                .downcast::<Root>()
                .expect("Kit root window"),
            empty_workspace: Workspace::default(),
            open_projects: vec![true; projects.len()],
            project_tints: (0..projects.len()).map(default_tint).collect(),
            demo_mode,
            verbose,
            catalogs,
            empty_catalog: agents::AgentCatalog::remote(),
            loaded_machines,
            project_machine: machines::LOCAL.into(),
            browser: None,
            browser_return: None,
            upgrade_machine: None,
            prompt_input: cx.new(|cx| InputState::new(window, cx).masked(true)),
            selected_agent: None,
            runtime: runtime_ui::Runtime {
                conversations: if demo_mode {
                    demo_live()
                } else {
                    snapshot.live
                },
                ..Default::default()
            },
            project_directory_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("Existing working directory")),
            project_error: None,
            delete_project: None,
            rename_project: None,
            project_search,
            project_sort: project_bar::ProjectSort::default(),
            project_highlight: None,
            project_list_scroll: ScrollHandle::new(),
            pending_removal: None,
            removal_generation: 0,
            projects,
            project: 0,
            section: Section::Chats,
            selected: None,
            agent_filter: None,
            chat_search_open: false,
            composer_height,
            show_completed: true,
            show_archived: false,
            side_panel_open: false,
            activity: activity::ActivityPanel::default(),
            query,
            composer,
            name_input,
            modal: None,
            menu: None,
            command_popup: None,
            speed: 0,
            attachments: Vec::new(),
            attachment_error: None,
            dismissed_errors: std::collections::HashSet::new(),
            slash: None,
            slash_dismissed: false,
            side_tab: panes::SideTab::Activity,
            traffic: Vec::new(),
            traffic_scroll: ScrollHandle::new(),
            open_thoughts: std::collections::HashSet::new(),
            logged_in: std::collections::HashSet::new(),
            pictures: std::cell::RefCell::default(),
            left_panel_open: true,
            selected_tint: 3,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            chat_list,
            transcript,
            composer_region,
            header_region,
            side_panel,
            chat_header,
            docs,
        };
        app.load_settings();
        if demo_mode || app.agent_catalog().entries.len() == 1 {
            app.selected_agent = app
                .agent_catalog()
                .entries
                .first()
                .map(|entry| entry.id.clone());
        }
        if !snapshot.errors.is_empty() {
            app.notify_toast(&snapshot.errors.join("\n"), cx);
        }
        if let Some(error) = theme::load_error() {
            app.notify_toast(&error, cx);
        }
        app.sync_sidebar(cx);
        app.transcript
            .update(cx, |view, cx| view.sync(&app, false, cx));
        app.defer_with_window(cx, |app, window, cx| app.open_last_active_chat(window, cx));
        // A quiet turn's silence notice appears on time without new deltas.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(15))
                    .await;
                let alive = this.update(cx, |app, cx| {
                    if app
                        .current_live()
                        .is_some_and(|live| live.quiet_since.is_some())
                    {
                        app.transcript.update(cx, chat::Transcript::invalidate);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        app
    }
    fn current_machine(&self) -> String {
        if self.has_open_project() {
            self.workspace().machine.clone()
        } else {
            machines::checked()
                .into_iter()
                .next()
                .unwrap_or_else(|| machines::LOCAL.into())
        }
    }
    fn catalog(&self, machine: &str) -> &agents::AgentCatalog {
        self.catalogs.get(machine).unwrap_or(&self.empty_catalog)
    }
    fn catalog_mut(&mut self, machine: &str) -> &mut agents::AgentCatalog {
        self.catalogs
            .entry(machine.to_owned())
            .or_insert_with(agents::AgentCatalog::remote)
    }
    fn agent_catalog(&self) -> &agents::AgentCatalog {
        self.catalog(&self.current_machine())
    }
    fn selected_definition(&self) -> Option<&agents::AgentDefinition> {
        let id = self.selected_agent.as_ref()?;
        self.agent_catalog()
            .entries
            .iter()
            .find(|entry| &entry.id == id)
            .map(|entry| &entry.definition)
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
        cx.notify();
        cx.refresh_windows();
    }

    fn notify_cached_views(&mut self, cx: &mut Context<Self>) {
        self.transcript.update(cx, chat::Transcript::invalidate);
        self.chat_list.update(cx, |_, cx| cx.notify());
        self.side_panel.update(cx, |_, cx| cx.notify());
        self.chat_header.update(cx, |_, cx| cx.notify());
    }

    fn has_open_project(&self) -> bool {
        self.open_projects
            .get(self.project)
            .copied()
            .unwrap_or(false)
    }
    // Read by the UI tests.
    pub fn open_chat(&self) -> Option<&str> {
        self.selected
            .and_then(|i| self.workspace().threads.get(i))
            .map(|thread| thread.title.as_str())
    }
    pub fn chat_option(&self, category: conversation::Category) -> Option<String> {
        let live = self.runtime.conversations.get(&self.current_id()?)?;
        conversation::option(&live.options, category).map(conversation::SessionOption::current)
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
            .on_click(cx.listener(move |app, _, window, cx| {
                cx.stop_propagation();
                app.act(action.clone(), window, cx);
            }))
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
            .on_click(cx.listener(move |app, _, window, cx| {
                cx.stop_propagation();
                app.act(action.clone(), window, cx);
            }))
    }
    fn focus_chat_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_search_open = true;
        window.focus(&self.query.focus_handle(cx), cx);
        self.chat_list.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
    fn toggle_chat_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_search_open || !self.query.read(cx).value().is_empty() {
            self.chat_search_open = false;
            self.act(Action::ClearChatFilters, window, cx);
            window.focus(&self.focus, cx);
            self.chat_list.update(cx, |_, cx| cx.notify());
        } else {
            self.focus_chat_search(window, cx);
        }
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
}
impl Render for Adeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if let Some(screen) = self.engine_screen(cx) {
            screen
        } else if !self.has_open_project() {
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
            self.chats(window, cx)
        } else if self.section == Section::Docs {
            self.docs_load_images(cx);
            self.docs_panels(window, cx)
        } else {
            div().size_full().into_any_element()
        };
        col()
            .relative()
            .size_full()
            .bg(theme::islands(cx).bg)
            .font_family(cx.theme().font_family.clone())
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
                if app.has_open_project() && app.section == Section::Docs {
                    window.focus(&app.docs.search.focus_handle(cx), cx);
                } else if app.has_open_project() && app.section == Section::Chats {
                    app.focus_chat_search(window, cx);
                } else {
                    app.act(Action::Projects, window, cx);
                }
            }))
            .on_action(
                cx.listener(|app, _: &SendMessage, window, cx| app.act(Action::Send, window, cx)),
            )
            .on_action(
                cx.listener(|app, _: &SendNow, window, cx| app.act(Action::SendNow, window, cx)),
            )
            .on_action(
                cx.listener(|app, _: &StopTurn, window, cx| app.act(Action::Stop, window, cx)),
            )
            .on_action(cx.listener(|app, _: &RestartAgent, window, cx| {
                app.act(Action::Restart, window, cx);
            }))
            .on_action(cx.listener(|app, _: &ModelMenu, window, cx| {
                app.act(
                    Action::OptionMenu(conversation::Category::Model),
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|app, _: &EffortMenu, window, cx| {
                app.act(
                    Action::OptionMenu(conversation::Category::Effort),
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|app, _: &ModeMenu, window, cx| {
                app.act(Action::OptionMenu(conversation::Category::Mode), window, cx);
            }))
            .on_action(cx.listener(|app, _: &MoreMenu, window, cx| {
                app.act(
                    Action::OptionMenu(conversation::Category::Other),
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|app, _: &AttachFile, window, cx| {
                app.act(Action::AttachFile, window, cx);
            }))
            .on_action(cx.listener(|app, _: &TrafficView, window, cx| {
                app.act(Action::TrafficView, window, cx);
            }))
            .on_action(
                cx.listener(|app, _: &TodoList, window, cx| app.act(Action::TodoList, window, cx)),
            )
            .on_action(cx.listener(|app, _: &NextProject, window, cx| {
                app.step_project(true, window, cx);
            }))
            .on_action(cx.listener(|app, _: &PreviousProject, window, cx| {
                app.step_project(false, window, cx);
            }))
            .on_action(cx.listener(|app, _: &CloseProjectTab, window, cx| {
                if app.has_open_project() {
                    app.act(Action::CloseProject(app.project), window, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &OpenFolder, window, cx| {
                app.act(Action::OpenFolder, window, cx);
            }))
            .on_action(cx.listener(|app, _: &MachinesMenu, window, cx| {
                if !machines::remotes().is_empty() {
                    app.act(Action::Machines, window, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &FocusMessage, window, cx| {
                window.focus(&app.composer.focus_handle(cx), cx);
            }))
            .on_action(
                cx.listener(|app, _: &DismissErrors, window, cx| app.dismiss_errors(window, cx)),
            )
            .on_action(cx.listener(|app, _: &ForkChat, window, cx| {
                let thread = app.selected.map(|ix| &app.workspace().threads[ix]);
                if let Some(reply) =
                    thread.and_then(|t| (0..t.messages.len()).rfind(|&i| t.ends_turn(i)))
                {
                    app.act(Action::Fork(reply), window, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &CopyCode, _, cx| {
                let thread = app.selected.map(|ix| &app.workspace().threads[ix]);
                let code = thread.and_then(|t| {
                    t.messages
                        .iter()
                        .rev()
                        .find(|m| m.role != conversation::MessageRole::User)
                        .and_then(|m| last_code_block(&m.text))
                });
                if let Some(code) = code {
                    cx.write_to_clipboard(ClipboardItem::new_string(code));
                }
            }))
            .on_action(cx.listener(|app, _: &ForkSource, window, cx| {
                let threads = &app.workspace().threads;
                let source = app
                    .selected
                    .and_then(|ix| threads[ix].fork.as_ref())
                    .and_then(|fork| threads.iter().position(|t| t.id == fork.id));
                if let Some(ix) = source {
                    app.act(Action::Chat(ix), window, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &ToggleChatList, window, cx| {
                app.act(Action::ToggleLeftPanel, window, cx);
            }))
            .on_action(cx.listener(|app, _: &ToggleSidePane, window, cx| {
                app.act(Action::ToggleSidePanel, window, cx);
            }))
            .on_action(cx.listener(|app, _: &ProjectPalette, window, cx| {
                if app.has_open_project() && app.section == Section::Docs {
                    window.focus(&app.docs.search.focus_handle(cx), cx);
                } else {
                    app.act(Action::Projects, window, cx);
                }
            }))
            .on_action(cx.listener(|_, _: &NextFocus, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &PreviousFocus, window, cx| window.focus_prev(cx)))
            .on_action(cx.listener(|app, action: &docs::SwitchMode, window, cx| {
                if let Some(section) = project_bar::rail_sections().get(action.index) {
                    app.act(Action::Section(*section), window, cx);
                }
            }))
            .when(
                self.section == Section::Docs && self.has_open_project(),
                |content| self.docs_actions(content, cx),
            )
            .child(self.header_region.clone())
            .children(self.engine_banner(cx))
            .child(self.islands(body, window, cx))
            .child(self.menu_overlays(cx))
    }
}

pub fn open(demo_mode: bool, verbose: bool, control: bool) {
    #[cfg(feature = "profiler")]
    frame_log::start();
    application().with_assets(Assets).run(move |cx: &mut App| {
        // These pixels describe native window geometry, not control spacing.
        let bounds = Bounds::centered(None, size(px(1440.), px(940.)), cx);
        // `--hidden` (control builds) keeps the window off screen for `adeline ui`.
        let hidden = control && std::env::args().any(|arg| arg == "--hidden");
        let adeline = start(
            demo_mode,
            verbose,
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.), px(600.))),
                show: !hidden,
                focus: !hidden,
                ..titlebar::main_window_options()
            },
            cx,
        );
        // A hidden window keeps the platform's default placement until it shows.
        if hidden {
            let window = adeline.read(cx).main_window;
            let _ = window.update(cx, |_, window, _| window.resize(bounds.size));
        }
        #[cfg(feature = "control")]
        if control && let Err(error) = control::serve(demo_mode, hidden, cx) {
            tracing::event!(
                name: "control.serve.failed",
                tracing::Level::WARN,
                error.message = format!("{error:#}"),
                "{{error.message}}"
            );
        }
        #[cfg(not(feature = "control"))]
        let _ = control;
        cx.activate(true);
    });
}

// Everything `open` sets up inside the platform application, so tests run it on GPUI's test platform.
pub fn start(
    demo_mode: bool,
    verbose: bool,
    options: WindowOptions,
    cx: &mut App,
) -> Entity<Adeline> {
    machines::init(demo_mode);
    init(cx);
    fonts::init(cx);
    config::init();
    theme::init();
    theme::apply(cx);
    cx.set_global(Harnesses(harness::Catalog {
        harnesses: if demo_mode {
            harness::load()
        } else {
            Vec::new()
        },
        detected: demo_mode,
        demo: demo_mode,
        ..Default::default()
    }));
    let mut initial = (!demo_mode && machines::is_checked(machines::LOCAL))
        .then(client::connect_existing)
        .flatten();
    let snapshot = initial.as_mut().and_then(|initial| initial.snapshot.take());
    client::init(demo_mode, initial, snapshot.as_ref(), cx);
    bind_keys(cx);
    // Outside the main view (dialogs, Settings). Deferred, because the window is
    // still busy with the key press and can't be updated until it's done.
    cx.on_action(|_: &NextFocus, cx| {
        cx.defer(|cx| {
            if let Some(handle) = cx.active_window() {
                let _ = cx.update_window(handle, |_, window, cx| window.focus_next(cx));
            }
        });
    });
    cx.on_action(|_: &PreviousFocus, cx| {
        cx.defer(|cx| {
            if let Some(handle) = cx.active_window() {
                let _ = cx.update_window(handle, |_, window, cx| window.focus_prev(cx));
            }
        });
    });
    let mut adeline = None;
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| Adeline::new(demo_mode, verbose, snapshot, window, cx));
        adeline = Some(view.clone());
        client::set_owner(view.downgrade(), cx);
        let handle = window
            .window_handle()
            .downcast::<Root>()
            .expect("Kit root window");
        let owner = view.downgrade();
        let closing_owner = owner.clone();
        window.on_window_should_close(cx, move |_, cx| {
            interaction::request_close(handle, closing_owner.clone(), cx);
            false
        });
        menu_bar::init(handle, owner, demo_mode, cx);
        cx.on_window_closed(move |cx, closed| {
            if closed == handle.window_id() {
                settings::close_for(handle, cx);
            }
        })
        .detach();
        view.update(cx, |app, cx| window.focus(&app.focus, cx));
        // Kit's root paints the background, which would hide the window blur.
        cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
    })
    .expect("open Adeline window");
    adeline.expect("Adeline view")
}

// A new project's tab color, by its place in the bar.
// ponytail: only ``` fences, add ~~~ if agents start using them.
fn last_code_block(text: &str) -> Option<String> {
    let mut block = None;
    let mut open: Option<Vec<&str>> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match open.take() {
                Some(lines) => block = Some(lines.join("\n")),
                None => open = Some(Vec::new()),
            }
        } else if let Some(lines) = &mut open {
            lines.push(line);
        }
    }
    block
}

fn default_tint(ix: usize) -> usize {
    [0, 2, 3][ix.min(2)]
}

fn bind_keys(cx: &mut App) {
    let keys = config::current().general.keymap;
    let mut bindings = Vec::new();
    macro_rules! bind {
        ($keys:expr, $action:expr) => {
            for key in &$keys {
                match KeyBinding::load(
                    key,
                    Box::new($action),
                    None,
                    false,
                    None,
                    &gpui_kit::DummyKeyboardMapper,
                ) {
                    Ok(binding) => bindings.push(binding),
                    Err(error) => config::set_error(format!("Invalid shortcut {key}: {error}")),
                }
            }
        };
    }
    bind!(keys.open_settings, OpenSettings);
    bind!(keys.new_chat, NewThread);
    bind!(keys.focus_search, Search);
    bind!(keys.send_message, SendMessage);
    bind!(keys.close_dialog_or_popup, Cancel);
    bind!(keys.next_control, NextFocus);
    bind!(keys.previous_control, PreviousFocus);
    bindings.push(KeyBinding::new("cmd-q", Quit, None));
    // The conversation controls' shortcuts (docs/shortcuts.md), Ctrl and Cmd alike.
    for modifier in ["ctrl", "cmd"] {
        let key = |rest: &str| format!("{modifier}-{rest}");
        bindings.extend([
            KeyBinding::new(&key("shift-enter"), SendNow, None),
            KeyBinding::new(&key("."), StopTurn, None),
            KeyBinding::new(&key("shift-r"), RestartAgent, None),
            KeyBinding::new(&key("shift-a"), AttachFile, None),
            KeyBinding::new(&key("shift-l"), TrafficView, None),
            KeyBinding::new(&key("shift-t"), TodoList, None),
            KeyBinding::new(&key("w"), CloseProjectTab, None),
            KeyBinding::new(&key("o"), OpenFolder, None),
            KeyBinding::new(&key("l"), FocusMessage, None),
            KeyBinding::new(&key("shift-d"), DismissErrors, None),
            KeyBinding::new(&key("shift-f"), ForkChat, None),
            KeyBinding::new(&key("shift-g"), ForkSource, None),
            KeyBinding::new(&key("shift-c"), CopyCode, None),
            KeyBinding::new(&key("b"), ToggleChatList, None),
            KeyBinding::new(&key("shift-b"), ToggleSidePane, None),
            KeyBinding::new(&key("p"), ProjectPalette, None),
        ]);
    }
    // Cmd+Tab is the macOS app switcher, so tabs switch with Ctrl alone.
    bindings.extend([
        KeyBinding::new("ctrl-tab", NextProject, None),
        KeyBinding::new("ctrl-shift-tab", PreviousProject, None),
    ]);
    // The option menus and the machines menu. Ctrl+Shift+M, E and O are global
    // hotkeys of common Windows tools (AMD Adrenalin among them), so Windows and
    // Linux use Alt.
    let menu = |letter: &str| {
        if cfg!(target_os = "macos") {
            format!("cmd-shift-{letter}")
        } else {
            format!("alt-{letter}")
        }
    };
    bindings.extend([
        KeyBinding::new(&menu("m"), ModelMenu, None),
        KeyBinding::new(&menu("e"), EffortMenu, None),
        KeyBinding::new(&menu("o"), ModeMenu, None),
        KeyBinding::new(&menu("p"), MoreMenu, None),
        KeyBinding::new(&menu("h"), MachinesMenu, None),
    ]);
    bindings.extend(docs::key_bindings());
    cx.bind_keys(bindings);
}
