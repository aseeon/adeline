//! The Settings and agent windows: state, navigation, search, and closing with
//! unsaved changes. Each page lives in a child module. A remote machine shows
//! only its Agents and Engine; the rest is this client's. Demo mode adds
//! General › Features, whose toggles are still stubs.
use super::*;
use gpui_kit::AppContext as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, FocusableExt as _, IndexPath, Selectable as _,
    Sizable as _, WindowExt as _,
    button::Button,
    form::{Field, Form},
    input::Input,
    scroll::ScrollableElement as _,
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
};
use std::{cell::Cell, rc::Rc};
use views::{card, text_input};

mod agent_editor;
mod agent_window;
mod appearance;
mod engine;
mod general;
mod licenses;
mod machine_list;
mod search;
use agent_window::AgentWindow;
use search::{agent_matches, engine_matches, general_matches, matches_query};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    General,
    Modes,
    Licenses,
    Agents,
    Engine,
}
impl Group {
    const NAVIGATION: [Self; 5] = [
        Self::General,
        Self::Modes,
        Self::Agents,
        Self::Engine,
        Self::Licenses,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Modes => "Modes",
            Self::Licenses => "Licenses",
            Self::Agents => "Agents",
            Self::Engine => "Engine",
        }
    }

    fn subgroups(self) -> &'static [&'static str] {
        match self {
            Self::General => &["Features", "Appearance", "Keymap"],
            Self::Modes => &["Chats", "Docs"],
            Self::Licenses => &["Phosphor Icons", "GPUI", "Chivo & Chivo Mono"],
            Self::Agents => &["MCP servers"],
            Self::Engine => &["Engine"],
        }
    }

    fn local_only(self) -> bool {
        matches!(self, Self::General | Self::Modes | Self::Licenses)
    }

    fn of_mode(mode: Option<Section>) -> Self {
        if matches!(mode, Some(Section::Chats | Section::Docs)) {
            Self::Modes
        } else {
            Self::General
        }
    }
}
const KEEP_RUNNING: &str = "Keep conversation engine running";
const KEEP_RUNNING_DESCRIPTION: &str =
    "The engine never exits on its own, and idle agents keep running with no window open.";
const RETRY_LABEL: &str = "Automatic retry limit";
const RETRY_DESCRIPTION: &str =
    "Additional attempts after a temporary failure. Zero disables automatic retries.";
const ENGINE_STATUS: [&str; 8] = [
    "Status",
    "PID",
    "Version",
    "Build",
    "Uptime",
    "Clients",
    "Stop engine",
    "Start engine",
];
const MODES: [(Section, &str); 7] = [
    (Section::Chats, "Chats"),
    (Section::Docs, "Docs"),
    (Section::Workflows, "Workflows"),
    (Section::Services, "Services"),
    (Section::Groupchats, "Groupchats"),
    (Section::Issues, "Issues"),
    (Section::Whiteboard, "Whiteboard"),
];
const KEYMAP: [&str; 8] = [
    "Open settings",
    "New chat",
    "Focus search",
    "Send message",
    "Close dialog or popup",
    "Next control",
    "Previous control",
    "Activate focused control (built in)",
];
type SettingOption = (&'static str, &'static str, bool, Action);
const SILENCE_LABEL: &str = "Silence notice after (minutes)";
const SILENCE_DESCRIPTION: &str = "0 turns the notice off.";
const MCP_DESCRIPTION: &str = "Every agent on this machine gets these MCP servers when a session starts, before its own. A server whose transport an agent doesn't support is skipped with a note in the chat.";
const THINKING_LABEL: &str = "Thinking animation";
const THINKING_DESCRIPTION: &str = "How the row under your message moves while the agent works.";

impl Adeline {
    pub(super) fn mode_options(&self, section: Section) -> Vec<SettingOption> {
        if section == Section::Docs {
            let docs = config::current().modes.docs;
            return vec![
                (
                    "Respect .gitignore",
                    "Leave out files that .gitignore ignores and files in hidden folders.",
                    docs.respect_gitignore,
                    Action::Docs(docs::Act::Gitignore),
                ),
                (
                    "Show left panel",
                    "Display the docs tree.",
                    docs.show_left_panel,
                    Action::Docs(docs::Act::LeftPanel),
                ),
                (
                    "Show conversation panel",
                    "Display a conversation beside the doc.",
                    docs.show_conversation_panel,
                    Action::Docs(docs::Act::ChatPanel),
                ),
                (
                    "Open files in the Editor",
                    "Open docs in the Editor instead of the Viewer.",
                    docs.open_in == config::DocView::Editor,
                    Action::Docs(docs::Act::OpenIn),
                ),
                (
                    "Load remote images",
                    "Show images from the web in the Viewer.",
                    docs.load_remote_images,
                    Action::Docs(docs::Act::RemoteImages),
                ),
            ];
        }
        if section != Section::Chats {
            return Vec::new();
        }
        vec![
            (
                "Show completed chats",
                "Include completed conversations in the chat list.",
                self.show_completed,
                Action::ShowCompleted,
            ),
            (
                "Show archived chats",
                "Include archived conversations in the chat list.",
                self.show_archived,
                Action::ShowArchived,
            ),
            (
                "Hide tool calls",
                "Hide tool calls and results in chats. Permission requests stay visible.",
                config::current().modes.chats.hide_tool_calls,
                Action::HideToolCalls,
            ),
            (
                "Show thinking",
                "Stream the agent's reasoning into the chat.",
                config::current().modes.chats.show_thinking,
                Action::ShowThinking,
            ),
            (
                "Submit on Enter",
                "Enter sends the message and Shift+Enter starts a new line.",
                config::current().modes.chats.submit_on_enter,
                Action::SubmitOnEnter,
            ),
            (
                "Show left panel",
                "Display chat navigation.",
                self.left_panel_open,
                Action::ToggleLeftPanel,
            ),
            (
                "Show agent activity",
                "Display chat agent activity.",
                self.side_panel_open,
                Action::ToggleSidePanel,
            ),
        ]
    }
}

#[derive(Clone)]
struct Owner {
    window: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
}
impl Owner {
    fn new(window: WindowHandle<Root>, entity: WeakEntity<Adeline>) -> Self {
        Self { window, entity }
    }

    fn act(&self, action: Action, window: &mut Window, cx: &mut App) {
        // Preference-save feedback belongs in the window that initiated the change.
        let _ = self
            .entity
            .update(cx, |app, cx| app.act(action, window, cx));
    }

    fn diagnostics(&self, machine: &str, cx: &App) -> Vec<String> {
        self.entity
            .read_with(cx, |app, _| app.catalog(machine).errors.clone())
            .unwrap_or_default()
    }
}

fn windows_of<V: 'static>(cx: &App) -> Vec<(WindowHandle<Root>, Entity<V>)> {
    cx.windows()
        .into_iter()
        .filter_map(|handle| {
            let root = handle.downcast::<Root>()?;
            let view = root.read(cx).ok()?.view().clone().downcast::<V>().ok()?;
            Some((root, view))
        })
        .collect()
}

pub(super) fn open(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    cx: &mut Context<Adeline>,
) {
    open_at(Owner::new(owner, entity), None, None, cx);
}
pub(super) fn open_machines(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    cx: &mut Context<Adeline>,
) {
    open_at(
        Owner::new(owner, entity),
        None,
        Some(AfterAgent::Machines),
        cx,
    );
}
pub(super) fn open_agent_page(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    machine: String,
    id: Option<String>,
    cx: &mut Context<Adeline>,
) {
    open_at(
        Owner::new(owner, entity),
        Some(machine),
        id.map(AfterAgent::Agent),
        cx,
    );
}
pub(super) fn open_mode(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    mode: Section,
    cx: &mut Context<Adeline>,
) {
    open_at(
        Owner::new(owner, entity),
        Some(machines::LOCAL.to_owned()),
        Some(AfterAgent::OpenMode(mode)),
        cx,
    );
}
fn open_at(
    owner: Owner,
    machine: Option<String>,
    target: Option<AfterAgent>,
    cx: &mut Context<Adeline>,
) {
    cx.defer(move |cx| {
        if let Some((root, settings)) = windows_of::<SettingsWindow>(cx)
            .into_iter()
            .find(|(_, view)| view.read(cx).owner.window.window_id() == owner.window.window_id())
        {
            let _ = cx.update_window(root.into(), |_, window, cx| {
                settings.update(cx, |settings, cx| {
                    let dirty = settings.agent_dirty(cx);
                    if let Some(machine) = machine.filter(|m| *m != settings.machine && !dirty) {
                        settings.set_machine(machine, window, cx);
                    }
                    let shown = matches!(&target, Some(AfterAgent::Agent(id))
                        if settings.agent_page.as_ref() == Some(id));
                    if let Some(target) = target.filter(|_| !shown) {
                        settings.leave(target, window, cx);
                    }
                });
                window.activate_window();
            });
            return;
        }
        let bounds = Bounds::centered(None, size(px(960.), px(760.)), cx);
        let owner_window = owner.window;
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(760.), px(500.))),
                ..titlebar::main_window_options()
            },
            |window, cx| {
                window.set_window_title("Settings · Adeline");
                let mode = match target {
                    Some(AfterAgent::OpenMode(mode)) => Some(mode),
                    _ => None,
                };
                let machine = machine.unwrap_or_else(|| machines::LOCAL.to_owned());
                let view = cx.new(|cx| SettingsWindow::new(owner, machine, mode, window, cx));
                match target {
                    Some(AfterAgent::Agent(id)) => {
                        view.update(cx, |view, cx| view.show_agent(id, window, cx));
                    }
                    Some(AfterAgent::Machines) => {
                        view.update(cx, |view, cx| {
                            view.after_agent(AfterAgent::Machines, window, cx);
                        });
                    }
                    _ => {}
                }
                // Kit's root paints the background, which would hide the window blur.
                cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
            },
        );
        if let Err(error) = result {
            notify_failure(
                owner_window,
                format!("Could not open settings: {error:#}"),
                cx,
            );
        }
    });
}
pub(super) fn open_agent(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    machine: String,
    cx: &mut Context<Adeline>,
) {
    let owner = Owner::new(owner, entity);
    cx.defer(move |cx| {
        let bounds = Bounds::centered(None, size(px(760.), px(760.)), cx);
        let owner_window = owner.window;
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(620.), px(500.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Add an agent · Adeline".into()),
                    appears_transparent: cfg!(target_os = "windows"),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| AgentWindow::new(owner, machine, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        if let Err(error) = result {
            notify_failure(
                owner_window,
                format!("Could not open agent editor: {error:#}"),
                cx,
            );
        }
    });
}

fn notify_failure(owner: WindowHandle<Root>, message: String, cx: &mut App) {
    let _ = cx.update_window(owner.into(), |_, window, cx| {
        window.push_notification(message, cx);
    });
}

pub(super) fn can_close(owner: WindowHandle<Root>, cx: &mut App) -> bool {
    for (root, settings) in windows_of::<SettingsWindow>(cx) {
        if settings.read(cx).owner.window.window_id() == owner.window_id()
            && settings.read(cx).agent_dirty(cx)
        {
            let _ = cx.update_window(root.into(), |_, window, cx| {
                settings.update(cx, |view, cx| {
                    view.leave(AfterAgent::CloseOwner, window, cx);
                });
            });
            return false;
        }
    }
    for (root, creation) in windows_of::<AgentWindow>(cx) {
        if creation.read(cx).owner.window.window_id() == owner.window_id()
            && creation.read(cx).form.read(cx).dirty(cx)
        {
            let _ = cx.update_window(root.into(), |_, window, cx| {
                creation.update(cx, |view, cx| view.confirm_close(true, window, cx))
            });
            return false;
        }
    }
    true
}
pub(super) fn close_for(owner: WindowHandle<Root>, cx: &mut App) {
    for (root, view) in windows_of::<SettingsWindow>(cx) {
        if view.read(cx).owner.window.window_id() == owner.window_id() {
            let _ = root.update(cx, |_, window, _| window.remove_window());
        }
    }
    for (root, view) in windows_of::<AgentWindow>(cx) {
        if view.read(cx).owner.window.window_id() == owner.window_id() {
            let _ = root.update(cx, |_, window, _| window.remove_window());
        }
    }
}

fn persist_form(
    owner: &Owner,
    form: &Entity<agent_form::AgentForm>,
    overwrite: bool,
    cx: &mut App,
    done: impl FnOnce(Result<String>, &mut App) + 'static,
) {
    let form = form.read(cx);
    if let Err(error) = form.check(cx) {
        return cx.defer(move |cx| done(Err(error), cx));
    }
    let definition = form.values(cx);
    let original = form.id.clone();
    let machine = form.machine.clone();
    let expected = original.as_ref().map(|_| form.original.clone());
    Adeline::save_agent(
        &owner.entity,
        &machine,
        original,
        definition,
        expected,
        overwrite,
        cx,
        done,
    );
}

fn choose(
    window: &mut Window,
    cx: &mut App,
    title: String,
    description: &'static str,
    labels: &'static [&'static str],
    selected: impl Fn(usize, &mut Window, &mut App) + 'static,
) {
    let selected: Rc<dyn Fn(usize, &mut Window, &mut App)> = Rc::new(selected);
    let decided = Rc::new(Cell::new(false));
    window.open_dialog(cx, move |dialog, _, cx| {
        let mut footer = div().flex().justify_end().gap_2();
        for (index, &label) in labels.iter().enumerate() {
            let selected = Rc::clone(&selected);
            let decided = Rc::clone(&decided);
            let button = Button::new(format!("choice-{index}"))
                .label(label)
                .small()
                .on_click(move |_, window, cx| {
                    decided.set(true);
                    window.close_dialog(cx);
                    selected(index, window, cx);
                });
            footer = footer.child(if matches!(label, "Delete" | "Remove") {
                button.danger()
            } else if index == 0 {
                button.primary()
            } else {
                button
            });
        }
        let on_ok = Rc::clone(&selected);
        let ok_decided = Rc::clone(&decided);
        let on_close = Rc::clone(&selected);
        let close_decided = Rc::clone(&decided);
        views::styled_dialog(dialog, cx)
            .title(views::dialog_title(title.clone()))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
            .footer(footer)
            .overlay_closable(false)
            .close_button(false)
            .on_ok(move |_, window, cx| {
                if !ok_decided.replace(true) {
                    window.close_dialog(cx);
                    on_ok(0, window, cx);
                }
                false
            })
            .on_close(move |_, window, cx| {
                if !close_decided.replace(true) {
                    on_close(labels.len() - 1, window, cx);
                }
            })
    });
}

#[derive(Clone)]
enum AfterAgent {
    Machine(String),
    Machines,
    CloseSettings,
    CloseOwner,
    Child(Group, usize),
    OpenMode(Section),
    Agent(String),
    Delete(String),
    Search(String),
}
struct SettingsWindow {
    owner: Owner,
    machine: String,
    machine_picker: Entity<SelectState<SearchableVec<String>>>,
    machines_page: bool,
    editing: Option<String>,
    machine_name: Entity<InputState>,
    machine_destination: Entity<InputState>,
    machine_error: Option<String>,
    query: Entity<InputState>,
    last_query: String,
    agent_page: Option<String>,
    agent_form: Option<Entity<agent_form::AgentForm>>,
    agent_subscriptions: Vec<Subscription>,
    agent_status: Option<String>,
    pending: bool,
    font_sizes: [Entity<InputState>; 2],
    font_size_errors: [Option<String>; 2],
    retry_limit: Entity<InputState>,
    retry_limit_error: Option<String>,
    silence_minutes: Entity<InputState>,
    silence_error: Option<String>,
    mcp_error: Option<String>,
    engine_error: Option<String>,
    engine_stopping: bool,
    thinking_picker: Entity<SelectState<SearchableVec<String>>>,
    thinking_error: Option<String>,
    theme_picker: Entity<SelectState<SearchableVec<theme::ThemeChoice>>>,
    font_pickers: [Entity<SelectState<SearchableVec<String>>>; 2],
    theme_status: Option<String>,
    demo: bool,
    group: Group,
    subgroup: Option<usize>,
    search_page: Option<(Group, Option<usize>)>,
    folded: [bool; 5],
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}
fn font_size_error() -> String {
    format!(
        "Enter a whole number from {} to {}.",
        config::MIN_FONT_SIZE,
        config::MAX_FONT_SIZE
    )
}
fn selected_font_size(which: usize) -> u16 {
    if which == 0 {
        config::font_size()
    } else {
        config::code_font_size()
    }
}
fn saved_font(which: usize) -> String {
    let appearance = config::current().general.appearance;
    if which == 0 {
        appearance.interface_font
    } else {
        appearance.code_font
    }
}
fn error(message: impl Into<String>, cx: &App) -> Div {
    div()
        .text_sm()
        .text_color(cx.theme().danger)
        .child(message.into())
}
fn switch_row(label: &'static str, description: &'static str, switch: Switch, cx: &App) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(label)
                .when(!description.is_empty(), |text| {
                    text.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    )
                }),
        )
        .child(switch)
}
fn field_row(field: Field, message: Option<String>, cx: &App) -> Div {
    div()
        .w_full()
        .px_4()
        .py_3()
        .flex()
        .flex_col()
        .gap_2()
        .child(field)
        .when_some(message, |row, message| row.child(error(message, cx)))
}
fn diagnostics(errors: Vec<String>, cx: &App) -> Div {
    let mut list = div().flex().flex_col().gap_2();
    for message in errors {
        list = list.child(error(message, cx));
    }
    list
}

const ADD_MACHINE: &str = "Add machine…";
const MANAGE_MACHINES: &str = "Manage machines…";

fn machine_labels() -> Vec<String> {
    let mut labels: Vec<String> = machines::all().iter().map(|m| machines::name(m)).collect();
    labels.extend([ADD_MACHINE.to_owned(), MANAGE_MACHINES.to_owned()]);
    labels
}

fn engine_settings_of(machine: &str, cx: &App) -> protocol::EngineSettings {
    client::get(machine, cx)
        .map(|c| c.settings.clone())
        .unwrap_or_default()
}

impl SettingsWindow {
    fn new(
        owner: Owner,
        machine: String,
        mode: Option<Section>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let demo = owner
            .entity
            .read_with(cx, |app, _| app.demo_mode)
            .unwrap_or(false);
        client::focus(&machine, cx);
        let machine_picker = cx.new(|cx| {
            let mut picker =
                SelectState::new(SearchableVec::new(machine_labels()), None, window, cx);
            picker.set_selected_value(&machines::name(&machine), window, cx);
            picker
        });
        let machine_name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let machine_destination = cx.new(|cx| {
            InputState::new(window, cx).placeholder("user@host, user@host:port or a Host alias")
        });
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        let focus = cx.focus_handle().tab_stop(true);
        fonts::refresh(cx);
        let font_sizes = std::array::from_fn(|which| {
            text_input(
                &selected_font_size(which).to_string(),
                if which == 0 { "15" } else { "14" },
                window,
                cx,
            )
        });
        let retry_limit = text_input(
            &engine_settings_of(&machine, cx).retry_limit.to_string(),
            "5",
            window,
            cx,
        );
        let silence_minutes = text_input(
            &config::current()
                .modes
                .chats
                .silence_notice_minutes
                .to_string(),
            "10",
            window,
            cx,
        );
        let thinking = config::current().modes.chats.thinking_animation;
        let thinking_picker = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    config::ThinkingAnimation::ALL
                        .map(|choice| choice.label().to_owned())
                        .to_vec(),
                ),
                config::ThinkingAnimation::ALL
                    .iter()
                    .position(|choice| *choice == thinking)
                    .map(|index| IndexPath::default().row(index)),
                window,
                cx,
            )
        });
        let (theme_choices, errors) =
            theme::discover().unwrap_or_else(|error| (Vec::new(), vec![format!("{error:#}")]));
        let selected_theme = config::current().general.appearance.theme;
        let selected_theme = theme_choices
            .iter()
            .position(|choice| choice.file == selected_theme)
            .map(|index| IndexPath::default().row(index));
        let theme_picker = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(theme_choices),
                selected_theme,
                window,
                cx,
            )
            .searchable(true)
        });
        let font_pickers = std::array::from_fn(|which| {
            let mut names = fonts::families();
            let saved = saved_font(which);
            if !names.iter().any(|name| name.eq_ignore_ascii_case(&saved)) && !saved.is_empty() {
                names.insert(0, saved.clone());
            }
            let selected = names
                .iter()
                .position(|name| *name == saved)
                .map(|index| IndexPath::default().row(index));
            cx.new(|cx| {
                SelectState::new(SearchableVec::new(names), selected, window, cx).searchable(true)
            })
        });
        let mut subscriptions = Vec::new();
        for (which, size_input) in font_sizes.iter().enumerate() {
            subscriptions.push(cx.subscribe(
                size_input,
                move |this, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let value = input.read(cx).value();
                    if let Ok(size) = value.trim().parse::<u16>() {
                        this.save_font_size(which, size, cx);
                    } else {
                        this.font_size_errors[which] = Some(font_size_error());
                        cx.notify();
                    }
                },
            ));
        }
        subscriptions.push(
            cx.subscribe(&retry_limit, |this, input, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = input.read(cx).value().trim().parse::<usize>();
                let current = engine_settings_of(&this.machine, cx);
                match value {
                    Ok(limit) if limit != current.retry_limit => {
                        let settings = protocol::EngineSettings {
                            retry_limit: limit,
                            ..current
                        };
                        this.save_engine_settings(settings, |this| &mut this.retry_limit_error, cx);
                    }
                    Ok(_) => this.retry_limit_error = None,
                    Err(_) => {
                        this.retry_limit_error = Some(
                            "Enter a non-negative whole number. Zero disables retries.".into(),
                        );
                    }
                }
                cx.notify();
            }),
        );
        subscriptions.push(cx.subscribe(
            &silence_minutes,
            |this, input, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                this.silence_error = match input.read(cx).value().trim().parse::<u32>() {
                    Ok(minutes) => config::update(|settings| {
                        settings.modes.chats.silence_notice_minutes = minutes;
                    })
                    .err()
                    .map(|e| format!("{e:#}")),
                    Err(_) => {
                        Some("Enter a whole number of minutes. 0 turns the notice off.".into())
                    }
                };
                let _ = this.owner.entity.update(cx, |app, cx| {
                    app.transcript.update(cx, chat::Transcript::invalidate);
                });
                cx.notify();
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &machine_picker,
            window,
            |this, _, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                let next = match label.as_str() {
                    ADD_MACHINE | MANAGE_MACHINES => AfterAgent::Machines,
                    name => match machines::all()
                        .into_iter()
                        .find(|m| machines::name(m) == name)
                    {
                        Some(machine) => AfterAgent::Machine(machine),
                        None => return,
                    },
                };
                if label == ADD_MACHINE {
                    this.editing = None;
                }
                // The dropdown names the machine shown, not the page.
                let shown = machines::name(&this.machine);
                this.machine_picker.update(cx, |picker, cx| {
                    picker.set_selected_value(&shown, window, cx);
                });
                this.leave(next, window, cx);
            },
        ));
        subscriptions.push(cx.subscribe(
            &thinking_picker,
            |this, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                let Some(choice) = config::ThinkingAnimation::ALL
                    .into_iter()
                    .find(|choice| choice.label() == label)
                else {
                    return;
                };
                this.thinking_error = config::update(|settings| {
                    settings.modes.chats.thinking_animation = choice;
                })
                .err()
                .map(|e| format!("{e:#}"));
                // A running turn's row picks up the new animation on its next frame.
                let _ = this.owner.entity.update(cx, |_, cx| cx.notify());
                cx.notify();
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &query,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Focus | InputEvent::Blur) {
                    // The field's border marks focus.
                    cx.notify();
                    return;
                }
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let requested = input.read(cx).value().to_string();
                if requested == this.last_query {
                    return;
                }
                if this.agent_dirty(cx) {
                    input.update(cx, |input, cx| {
                        input.set_value(this.last_query.clone(), window, cx);
                    });
                    this.leave(AfterAgent::Search(requested), window, cx);
                } else {
                    this.after_agent(AfterAgent::Search(requested), window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &theme_picker,
            window,
            |this, _, event: &SelectEvent<SearchableVec<theme::ThemeChoice>>, window, cx| {
                if let SelectEvent::Confirm(Some(file)) = event {
                    this.theme_status = match theme::select(file, cx) {
                        Ok(()) => None,
                        Err(message) => {
                            let saved = config::current().general.appearance.theme;
                            this.theme_picker.update(cx, |picker, cx| {
                                picker.set_selected_value(&saved, window, cx);
                            });
                            Some(format!("{message:#}"))
                        }
                    };
                    cx.notify();
                }
            },
        ));
        for (which, picker) in font_pickers.iter().enumerate() {
            subscriptions.push(cx.subscribe_in(
                picker,
                window,
                move |this, _, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                    if let SelectEvent::Confirm(Some(name)) = event {
                        this.choose_font(which, name, cx);
                        if saved_font(which) != *name {
                            this.font_pickers[which].update(cx, |picker, cx| {
                                picker.set_selected_value(&saved_font(which), window, cx);
                            });
                        }
                    }
                },
            ));
        }
        subscriptions.push(
            cx.observe_window_activation(window, |this, window, cx| this.sync_agent(window, cx)),
        );
        let view = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, cx| {
                if view.agent_dirty(cx) {
                    view.leave(AfterAgent::CloseSettings, window, cx);
                    false
                } else {
                    true
                }
            })
        });
        window.focus(&query.read(cx).focus_handle(cx), cx);
        let mut notices = errors;
        if let Some(message) = theme::load_error() {
            notices.insert(0, message);
        }
        let theme_status = (!notices.is_empty()).then(|| notices.join("\n"));
        Self {
            owner,
            machine,
            machine_picker,
            machines_page: false,
            editing: None,
            machine_name,
            machine_destination,
            machine_error: None,
            query,
            last_query: String::new(),
            agent_page: None,
            agent_form: None,
            agent_subscriptions: Vec::new(),
            agent_status: None,
            pending: false,
            font_sizes,
            font_size_errors: [None, None],
            retry_limit,
            retry_limit_error: None,
            silence_minutes,
            silence_error: None,
            mcp_error: None,
            engine_error: None,
            engine_stopping: false,
            thinking_picker,
            thinking_error: None,
            theme_picker,
            font_pickers,
            theme_status,
            demo,
            group: Group::of_mode(mode),
            subgroup: Some(usize::from(!demo && mode != Some(Section::Chats))),
            search_page: None,
            folded: std::array::from_fn(|group| group != Group::of_mode(mode) as usize),
            focus,
            _subscriptions: subscriptions,
        }
    }

    fn set_machine(&mut self, machine: String, window: &mut Window, cx: &mut Context<Self>) {
        client::focus(&machine, cx);
        client::ensure(&machine, cx);
        client::refresh_harnesses(&machine, false, cx);
        let name = machines::name(&machine);
        self.machine_picker.update(cx, |picker, cx| {
            picker.set_items(SearchableVec::new(machine_labels()), window, cx);
            picker.set_selected_value(&name, window, cx);
        });
        let limit = engine_settings_of(&machine, cx).retry_limit.to_string();
        self.retry_limit
            .update(cx, |input, cx| input.set_value(limit, window, cx));
        self.retry_limit_error = None;
        self.engine_error = None;
        self.mcp_error = None;
        self.close_agent();
        self.agent_status = None;
        self.machines_page = false;
        if machine != machines::LOCAL && self.group.local_only() {
            self.group = Group::Engine;
            self.subgroup = Some(0);
            self.folded[Group::Engine as usize] = false;
        }
        self.machine = machine;
        cx.notify();
    }

    fn after_agent(&mut self, after: AfterAgent, window: &mut Window, cx: &mut Context<Self>) {
        match after {
            AfterAgent::Machine(machine) => self.set_machine(machine, window, cx),
            AfterAgent::Machines => {
                self.close_agent();
                self.machines_page = true;
                self.machine_error = None;
                self.fill_machine_inputs(window, cx);
            }
            AfterAgent::CloseSettings => window.remove_window(),
            AfterAgent::CloseOwner => {
                self.agent_form = None;
                self.agent_subscriptions.clear();
                interaction::request_close(self.owner.window, self.owner.entity.clone(), cx);
            }
            AfterAgent::Child(group, child) => {
                self.machines_page = false;
                self.group = group;
                self.folded[group as usize] = false;
                self.subgroup = Some(child);
                self.close_agent();
                self.search_page = Some((group, Some(child)));
            }
            AfterAgent::OpenMode(mode) => {
                self.last_query.clear();
                self.query
                    .update(cx, |input, cx| input.set_value("", window, cx));
                let child = usize::from(mode == Section::Docs);
                self.after_agent(
                    AfterAgent::Child(Group::of_mode(Some(mode)), child),
                    window,
                    cx,
                );
            }
            AfterAgent::Agent(id) => self.show_agent(id, window, cx),
            AfterAgent::Delete(id) => self.confirm_delete_after_leaving(id, window, cx),
            AfterAgent::Search(query) => {
                self.last_query.clone_from(&query);
                self.close_agent();
                self.search_page = None;
                self.query
                    .update(cx, |input, cx| input.set_value(query, window, cx));
            }
        }
        cx.notify();
    }

    fn leave(&mut self, after: AfterAgent, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if !self.agent_dirty(cx) {
            self.after_agent(after, window, cx);
            return;
        }
        self.pending = true;
        let view = cx.entity().downgrade();
        choose(
            window,
            cx,
            "Save changes to this agent?".into(),
            "Your edits will be lost if you discard them.",
            &["Save", "Discard", "Cancel"],
            move |choice, window, cx| {
                let after = after.clone();
                let _ = view.update(cx, |view, cx| {
                    view.pending = false;
                    match choice {
                        0 => view.save_agent(Some(after), false, window, cx),
                        1 => view.after_agent(after, window, cx),
                        _ => {}
                    }
                });
            },
        );
    }

    fn setting_row(
        &self,
        id: impl Into<ElementId>,
        label: &'static str,
        description: &'static str,
        checked: bool,
        action: Action,
        cx: &Context<Self>,
    ) -> Div {
        let owner = self.owner.clone();
        switch_row(
            label,
            description,
            Switch::new(id)
                .checked(checked)
                .accessibility_label(label)
                .on_change(move |_, window, cx| owner.act(action.clone(), window, cx)),
            cx,
        )
    }
}

fn caps(text: &str, cx: &App) -> Div {
    div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_uppercase())
}
fn group_header(
    group: Group,
    count: usize,
    folded: bool,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> impl IntoElement {
    let id = SharedString::from(format!("settings-group-{}", group as usize));
    let focus = window
        .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle().tab_stop(true))
        .read(cx)
        .clone();
    let theme = cx.theme();
    let name = group.label();
    row()
        .id(id)
        .test_support()
        .track_focus(&focus)
        .tab_index(0)
        .role(Role::Button)
        .aria_label(format!("{} {name}", if folded { "Show" } else { "Hide" }))
        .mt_3()
        .mb_1()
        .h(rems(1.5))
        .px_2()
        .gap_1p5()
        .rounded(theme.radius)
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .text_color(theme.muted_foreground)
        .hover(|style| style.text_color(theme.foreground))
        .border_1()
        .border_color(theme.transparent)
        .focus_visible(|style| style.border_color(theme::sidebar_focus()))
        .child(
            Icon::default()
                .path("chevron.svg")
                .size(rems(0.625))
                .when(folded, |chevron| {
                    chevron.rotate(Radians(-std::f32::consts::FRAC_PI_2))
                }),
        )
        .child(div().flex_1().child(name.to_uppercase()))
        .child(count.to_string())
        .on_click(cx.listener(move |this, _, _, cx| {
            this.folded[group as usize] = !this.folded[group as usize];
            cx.notify();
        }))
}
fn navigation_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .accessibility_label(label.clone())
        .icon(Icon::default().path(icon))
        .child(div().w_full().truncate().text_left().child(label))
        .selected(selected)
        .focus_ring(false)
        .border_1()
        .border_color(cx.theme().transparent)
        .focus_visible(|style| style.border_color(theme::sidebar_focus()))
}
impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.query.read(cx).value().to_lowercase();
        let searching = !query.trim().is_empty();
        let mut navigation = div().flex().flex_col().gap_1();
        let remote = self.machine != machines::LOCAL;
        for group in Group::NAVIGATION {
            let name = group.label();
            if group == Group::Licenses && searching || remote && group.local_only() {
                continue;
            }
            let matching: Vec<_> = group
                .subgroups()
                .iter()
                .enumerate()
                .filter(|(child, title)| {
                    if group == Group::General {
                        general_matches(*child, &query)
                    } else {
                        matches_query(&query, &[name, title])
                    }
                })
                .collect();
            let matching_agents: Vec<_> = if group == Group::Agents {
                let machine = self.machine.clone();
                self.owner
                    .entity
                    .read_with(cx, |app, _| {
                        app.catalog(&machine)
                            .entries
                            .iter()
                            .filter(|entry| agent_matches(entry, &query))
                            .map(|entry| {
                                (
                                    entry.id.clone(),
                                    entry.definition.name.clone(),
                                    entry.definition.harness.clone(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            if searching
                && matching.is_empty()
                && matching_agents.is_empty()
                && !(group == Group::Agents && matches_query(&query, &["Agents"]))
            {
                continue;
            }
            let folded = self.folded[group as usize] && !searching;
            navigation = navigation.child(group_header(
                group,
                matching.len() + matching_agents.len(),
                folded,
                window,
                cx,
            ));
            if folded {
                continue;
            }
            for (child, title) in matching {
                let icon = match group {
                    Group::General => ["settings.svg", "pen.svg", "code.svg"][child],
                    Group::Modes => "chat.svg",
                    Group::Agents => "link.svg",
                    Group::Licenses => "file.svg",
                    Group::Engine => "devices.svg",
                };
                navigation = navigation.child(
                    navigation_button(
                        format!("settings-{}-{child}", group as usize),
                        *title,
                        icon,
                        !self.machines_page && self.group == group && self.subgroup == Some(child),
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.leave(AfterAgent::Child(group, child), window, cx);
                    })),
                );
            }
            for (id, title, harness) in matching_agents {
                let selected = !self.machines_page && self.agent_page.as_ref() == Some(&id);
                let installed = cx.global::<Harnesses>().is_installed(&harness);
                navigation = navigation.child(
                    navigation_button(
                        format!("settings-agent-{id}"),
                        title,
                        agents::avatar_path(&id, &harness),
                        selected,
                        cx,
                    )
                    .children(installed.map(agent_form::installed_dot))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.agent_page.as_deref() != Some(&id) {
                            this.leave(AfterAgent::Agent(id.clone()), window, cx);
                        }
                    })),
                );
            }
            if group == Group::Agents && !searching {
                navigation = navigation.child(
                    navigation_button("settings-add-agent", "Add agent", "plus.svg", false, cx)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let (window, machine) = (this.owner.window, this.machine.clone());
                            let _ = this.owner.entity.update(cx, |_, cx| {
                                open_agent(window, cx.weak_entity(), machine, cx);
                            });
                        })),
                );
            }
        }
        let sidebar_width = window.rem_size() * 15.;
        // The sidebar shows the window blur, like the main window's mode rail, and runs up
        // under the title bar.
        let sidebar = div()
            .w(sidebar_width)
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_2()
            .pb_2()
            .pt(titlebar::MAIN_HEIGHT + px(4.))
            .bg(cx.theme().title_bar.alpha(titlebar::GLASS))
            .child(search_field(
                Input::new(&self.query).aria_label("Search settings"),
                &self.query,
                window,
                cx,
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .id("settings-navigation")
                    .child(navigation),
            )
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Select::new(&self.machine_picker)
                            .icon(Icon::default().path("devices.svg"))
                            .accessibility_label("Machine")
                            .menu_width(rems(15.)),
                    ),
            );
        let show = |group: Group, child| {
            if self.machines_page || remote && group.local_only() {
                false
            } else if searching {
                self.search_page
                    .is_none_or(|(g, c)| g == group && c.is_none_or(|c| c == child))
            } else {
                self.group == group && self.subgroup.is_none_or(|c| c == child)
            }
        };
        let heading = if self.machines_page {
            "Machines".to_string()
        } else if searching {
            "Search results".to_string()
        } else if let Some(form) = self
            .agent_form
            .as_ref()
            .filter(|_| self.group == Group::Agents)
        {
            form.read(cx).name(cx)
        } else {
            self.subgroup
                .map_or(self.group.label(), |child| self.group.subgroups()[child])
                .to_string()
        };
        // The title bar names the page, so the content starts with its settings.
        let mut crumbs = if searching || self.machines_page || heading == self.group.label() {
            vec![heading]
        } else {
            vec![self.group.label().to_string(), heading]
        };
        if remote && !self.machines_page {
            crumbs.insert(0, machines::name(&self.machine));
        }
        let mut pages = Vec::new();
        if self.machines_page {
            pages.push(self.machines_settings(cx));
        }
        if let Some(owner) = self.owner.entity.upgrade() {
            let app = owner.read(cx);
            if show(Group::General, 0) && general_matches(0, &query) {
                pages.push(self.features_settings(&query, searching, cx));
            }
            if show(Group::General, 1) && general_matches(1, &query) {
                pages.push(self.appearance_settings(cx));
            }
            if show(Group::General, 2) && general_matches(2, &query) {
                pages.push(self.keymap_settings(&query, searching, cx));
            }
            if show(Group::Modes, 0) {
                pages.extend(self.chats_settings(app, &query, searching, cx));
            }
            if show(Group::Modes, 1) {
                pages.extend(self.docs_settings(app, &query, searching, cx));
            }
        }
        if show(Group::Engine, 0) && engine_matches(&query) {
            pages.push(self.engine_settings(&query, cx));
        }
        if self.group == Group::Licenses && !searching && !remote && !self.machines_page {
            pages.extend(self.licenses_settings(cx));
        }
        let mcp_page = !self.machines_page
            && self.agent_page.is_none()
            && if searching {
                matches_query(&query, &["Agents", "MCP servers", MCP_DESCRIPTION])
            } else {
                self.group == Group::Agents && self.subgroup == Some(0)
            };
        if mcp_page {
            pages.push(self.mcp_settings(cx));
        }
        if !self.machines_page && (searching || (self.group == Group::Agents && !mcp_page)) {
            pages.extend(self.agent_settings(&query, searching, cx));
        }
        let found = !pages.is_empty();
        let mut content = div()
            .w_full()
            .p_6()
            .pt_4()
            .flex()
            .flex_col()
            .gap_3()
            .children(pages);
        if !found {
            content = content.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("No settings found. Try another search."),
            );
        }
        if !searching
            && !self.machines_page
            && !remote
            && (self.group == Group::Modes
                || (self.group == Group::General && self.subgroup.is_none_or(|i| i == 0)))
        {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Changes apply immediately and are saved to settings.yml."),
            );
        }
        let theme = cx.theme();
        let mut breadcrumb = row().h_full().px_6().gap_1p5().text_sm().min_w_0();
        let last = crumbs.len() - 1;
        for (ix, crumb) in crumbs.into_iter().enumerate() {
            if ix > 0 {
                breadcrumb = breadcrumb.child(
                    Icon::default()
                        .path("caret-right.svg")
                        .size(rems(0.625))
                        .text_color(theme.muted_foreground),
                );
            }
            breadcrumb = breadcrumb.child(
                div()
                    .truncate()
                    .text_color(if ix == last {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(crumb),
            );
        }
        // One bar across the window, drawn over both panes without a line under it: the
        // sidebar's glass shows through on the left, the content's surface on the right.
        // macOS keeps Kit's left padding for the traffic lights.
        let lead = if cfg!(target_os = "macos") {
            sidebar_width - px(80.)
        } else {
            sidebar_width
        };
        // Kit wraps the bar in an unstyled div, and absolute boxes sit in their parent, so the
        // overlay is this wrapper rather than the bar itself.
        let title_bar = TitleBar::new()
            .h(titlebar::MAIN_HEIGHT)
            .when(!cfg!(target_os = "macos"), |bar| bar.pl_0())
            .border_b_0()
            .bg(transparent_black())
            .child(
                row()
                    .size_full()
                    .child(
                        row()
                            .h_full()
                            .w(lead)
                            .flex_shrink_0()
                            .gap_2()
                            .when(!cfg!(target_os = "macos"), |cell| {
                                cell.pl(rems(0.375)).child(titlebar::app_icon(window))
                            })
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Settings"),
                    )
                    .child(breadcrumb),
            );
        div()
            .relative()
            .size_full()
            .flex()
            .items_stretch()
            .text_color(theme.foreground)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                this.leave(AfterAgent::CloseSettings, window, cx);
            }))
            .on_action(cx.listener(|this, _: &MachinesMenu, window, cx| {
                let picker = this.machine_picker.focus_handle(cx);
                window.focus(&picker, cx);
            }))
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pt(titlebar::MAIN_HEIGHT)
                    .bg(theme.background)
                    .border_l_1()
                    .border_color(theme::bar_colors(theme).divider)
                    .child(
                        div()
                            .size_full()
                            .overflow_y_scrollbar()
                            .id("settings-content")
                            .child(content),
                    ),
            )
            .child(
                titlebar::without_text_selection(title_bar)
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0(),
            )
    }
}
