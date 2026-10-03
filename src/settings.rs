use super::*;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, FocusableExt as _, Icon, IndexPath, Root, Selectable as _,
    Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonVariants},
    form::{Field, Form},
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
};
use gpui_kit::{AppContext as _, base::actions::Cancel};
use std::{cell::Cell, rc::Rc};

const GROUPS: [&str; 5] = ["General", "Modes", "Licenses", "Agents", "Engine"];
const SUBGROUPS: [&[&str]; 5] = [
    &["Features", "Appearance", "Keymap"],
    &["Chats"],
    &["Phosphor Icons", "GPUI", "Chivo & Chivo Mono"],
    &[],
    &["Engine"],
];
const KEEP_RUNNING: &str = "Keep conversation engine running";
const KEEP_RUNNING_DESCRIPTION: &str =
    "The engine never exits on its own, and idle agents keep running with no window open.";
const RETRY_LABEL: &str = "Automatic retry limit";
const RETRY_DESCRIPTION: &str =
    "Additional attempts after a temporary failure. Zero disables automatic retries.";
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
const THINKING_LABEL: &str = "Thinking animation";
const THINKING_DESCRIPTION: &str = "How the row under your message moves while the agent works.";

impl Adeline {
    pub(super) fn mode_options(&self, section: Section) -> Vec<SettingOption> {
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
                "Submit on Enter",
                "Enter sends the message and Shift+Enter starts a new line.",
                config::current().modes.chats.submit_on_enter,
                Action::SubmitOnEnter,
            ),
            (
                "Show left panel",
                "Display chat navigation.",
                self.left_panel_open[Section::Chats as usize],
                Action::ToggleLeftPanel,
            ),
            (
                "Show agent activity",
                "Display chat agent activity.",
                self.side_panel_open[Section::Chats as usize],
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

    fn diagnostics(&self, cx: &App) -> Vec<String> {
        self.entity
            .read_with(cx, |app, _| app.agent_catalog.errors.clone())
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
    open_at(Owner::new(owner, entity), None, cx);
}
/// Settings on an agent's page, or where they were if it no longer exists.
pub(super) fn open_agent_page(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    id: Option<String>,
    cx: &mut Context<Adeline>,
) {
    open_at(Owner::new(owner, entity), id.map(AfterAgent::Agent), cx);
}
pub(super) fn open_mode(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    mode: Section,
    cx: &mut Context<Adeline>,
) {
    open_at(
        Owner::new(owner, entity),
        Some(AfterAgent::OpenMode(mode)),
        cx,
    );
}
fn open_at(owner: Owner, target: Option<AfterAgent>, cx: &mut Context<Adeline>) {
    cx.defer(move |cx| {
        if let Some((root, settings)) = windows_of::<SettingsWindow>(cx)
            .into_iter()
            .find(|(_, view)| view.read(cx).owner.window.window_id() == owner.window.window_id())
        {
            let _ = cx.update_window(root.into(), |_, window, cx| {
                settings.update(cx, |settings, cx| {
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
                let view = cx.new(|cx| SettingsWindow::new(owner, mode, window, cx));
                if let Some(AfterAgent::Agent(id)) = target {
                    view.update(cx, |view, cx| view.show_agent(id, window, cx));
                }
                // Kit's root paints the background, which would hide the window blur.
                cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
            },
        );
        if let Err(error) = result {
            eprintln!("Could not open settings: {error}");
        }
    });
}
pub(super) fn open_agent(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    cx: &mut Context<Adeline>,
) {
    let owner = Owner::new(owner, entity);
    cx.defer(move |cx| {
        let bounds = Bounds::centered(None, size(px(760.), px(760.)), cx);
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
                let view = cx.new(|cx| AgentWindow::new(owner, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        if let Err(error) = result {
            eprintln!("Could not open agent editor: {error}");
        }
    });
}

pub(super) fn can_close_for(
    owner: WindowHandle<Root>,
    entity: &WeakEntity<Adeline>,
    cx: &mut App,
) -> bool {
    for (root, settings) in windows_of::<SettingsWindow>(cx) {
        if settings.read(cx).owner.window.window_id() == owner.window_id()
            && settings
                .read(cx)
                .agent_form
                .as_ref()
                .is_some_and(|form| form.read(cx).dirty(cx))
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
    cx.update_window(owner.into(), |_, window, cx| {
        entity
            .update(cx, |app, cx| {
                let opening = app.modal != Some("quit");
                let ready = app.request_quit(cx);
                if !ready && opening {
                    app.open_modal(window, cx);
                }
                ready
            })
            .unwrap_or(false)
    })
    .unwrap_or(false)
}
pub(super) fn request_close(owner: WindowHandle<Root>, entity: WeakEntity<Adeline>, cx: &mut App) {
    cx.defer(move |cx| {
        if can_close_for(owner, &entity, cx) {
            let _ = owner.update(cx, |_, window, _| window.remove_window());
        }
    });
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
) -> Result<String, String> {
    let form = form.read(cx);
    form.check(cx)?;
    let definition = form.values(cx);
    let original = form.id.clone();
    let expected = original.as_ref().map(|_| form.original.clone());
    let (demo, select_first) = owner
        .entity
        .read_with(cx, |app, _| {
            (
                app.demo_mode,
                original.is_none() && app.agent_catalog.entries.is_empty(),
            )
        })
        .map_err(|error| error.to_string())?;
    let saved = if demo {
        let original = original.clone();
        owner
            .entity
            .update(cx, move |app, _| {
                app.agent_catalog.save(
                    original.as_deref(),
                    definition,
                    expected.as_ref(),
                    overwrite,
                )
            })
            .map_err(|error| error.to_string())??
    } else {
        let command = protocol::Command::SaveAgent {
            original: original.clone(),
            definition,
            expected,
            overwrite,
        };
        client::request_blocking(command, cx)?
            .as_str()
            .map(str::to_owned)
            .ok_or("The conversation engine returned no agent ID.")?
    };
    owner
        .entity
        .update(cx, |app, cx| {
            app.agents_changed(original.as_deref(), Some(&saved), select_first, cx);
        })
        .map_err(|error| error.to_string())?;
    Ok(saved)
}

/// Deletes an agent through the engine, or locally in demo mode.
fn delete_agent(owner: &Owner, id: &str, cx: &mut App) -> Result<(), String> {
    let demo = owner
        .entity
        .read_with(cx, |app, _| app.demo_mode)
        .map_err(|error| error.to_string())?;
    if demo {
        owner
            .entity
            .update(cx, |app, _| app.agent_catalog.delete(id))
            .map_err(|error| error.to_string())??;
    } else {
        client::request_blocking(protocol::Command::DeleteAgent { id: id.to_owned() }, cx)?;
    }
    owner
        .entity
        .update(cx, |app, cx| app.agents_changed(Some(id), None, false, cx))
        .map_err(|error| error.to_string())
}

/// Multi-choice Kit Dialog, with Escape mapped to the last (Cancel) action.
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
            let selected = selected.clone();
            let decided = decided.clone();
            let button = Button::new(format!("choice-{index}"))
                .label(label)
                .small()
                .on_click(move |_, window, cx| {
                    decided.set(true);
                    window.close_dialog(cx);
                    selected(index, window, cx);
                });
            footer = footer.child(if label == "Delete" {
                button.danger()
            } else if index == 0 {
                button.primary()
            } else {
                button
            });
        }
        let on_ok = selected.clone();
        let ok_decided = decided.clone();
        let on_close = selected.clone();
        let close_decided = decided.clone();
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

struct AgentWindow {
    owner: Owner,
    form: Entity<agent_form::AgentForm>,
    focus: FocusHandle,
    pending: bool,
    _subscriptions: Vec<Subscription>,
}
impl AgentWindow {
    fn new(owner: Owner, window: &mut Window, cx: &mut Context<Self>) -> Self {
        client::ensure(cx);
        client::refresh_harnesses(true, cx);
        let form = cx.new(|cx| {
            agent_form::AgentForm::new(None, agents::AgentDefinition::default(), window, cx)
        });
        let focus = cx.focus_handle().tab_stop(true);
        let name = form.read(cx).name_focus(cx);
        window.focus(&name, cx);
        let subscriptions = vec![cx.observe(&form, |_, _, cx| cx.notify())];
        let view = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, cx| view.confirm_close(false, window, cx))
        });
        Self {
            owner,
            form,
            focus,
            pending: false,
            _subscriptions: subscriptions,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match persist_form(&self.owner, &self.form, false, cx) {
            Ok(_) => {
                window.remove_window();
                true
            }
            Err(error) => {
                self.form.update(cx, |form, cx| {
                    form.status = Some(error);
                    cx.notify();
                });
                false
            }
        }
    }
    fn confirm_close(
        &mut self,
        close_owner: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.form.read(cx).dirty(cx) {
            if close_owner {
                window.remove_window();
            }
            return true;
        }
        if self.pending {
            return false;
        }
        self.pending = true;
        let weak = cx.entity().downgrade();
        choose(
            window,
            cx,
            "Save changes to this agent?".into(),
            "Unsaved agent details will be lost if you discard them.",
            &["Save", "Discard", "Cancel"],
            move |choice, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.pending = false;
                    match choice {
                        0 => {
                            if view.save(window, cx) && close_owner {
                                request_close(view.owner.window, view.owner.entity.clone(), cx);
                            }
                        }
                        1 => {
                            let owner = view.owner.clone();
                            window.remove_window();
                            if close_owner {
                                request_close(owner.window, owner.entity, cx);
                            }
                        }
                        _ => {}
                    }
                });
            },
        );
        false
    }
}
impl Render for AgentWindow {
    #[cfg_attr(
        not(target_os = "windows"),
        expect(unused_variables, reason = "only the Windows titlebar reads window")
    )]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .p_6()
            .child(div().text_xl().child("Add an agent"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Configure an agent for this workspace. Saving does not start the command.",
                    ),
            )
            .child(self.form.clone())
            .child(diagnostics(self.owner.diagnostics(cx), cx));
        let shell = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground);
        #[cfg(target_os = "windows")]
        let shell = shell.child(titlebar::render("Add an agent · Adeline".into(), window));
        shell
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .id("agent-form-scroll")
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .p_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .when_some(self.form.read(cx).status.clone(), |row, status| {
                        row.child(error(status, cx).flex_1().min_w_0())
                    })
                    .child(Button::new("agent-create-cancel").label("Cancel").on_click(
                        cx.listener(|view, _, window, cx| {
                            if view.confirm_close(false, window, cx) {
                                window.remove_window();
                            }
                        }),
                    ))
                    .child(
                        Button::new("agent-create-save")
                            .primary()
                            .label("Save")
                            .disabled(self.pending || self.form.read(cx).blocked(cx))
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.save(window, cx);
                            })),
                    ),
            )
            .track_focus(&self.focus)
            .on_action(cx.listener(|view, _: &Cancel, window, cx| {
                if view.confirm_close(false, window, cx) {
                    window.remove_window();
                }
            }))
    }
}

#[derive(Clone)]
enum AfterAgent {
    CloseSettings,
    CloseOwner,
    Child(usize, usize),
    OpenMode(Section),
    Agent(String),
    Delete(String),
    Search(String),
}
struct SettingsWindow {
    owner: Owner,
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
    /// Stop engine is waiting for the engine's agents to exit.
    engine_stopping: bool,
    thinking_picker: Entity<SelectState<SearchableVec<String>>>,
    thinking_error: Option<String>,
    theme_picker: Entity<SelectState<SearchableVec<theme::ThemeChoice>>>,
    font_pickers: [Entity<SelectState<SearchableVec<String>>>; 2],
    theme_status: Option<String>,
    group: usize,
    subgroup: Option<usize>,
    search_page: Option<(usize, Option<usize>)>,
    /// Sidebar groups folded to their header.
    folded: [bool; 5],
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}
fn text_input(
    value: String,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut input = InputState::new(window, cx).placeholder(placeholder);
        input.set_value(value, window, cx);
        input
    })
}
fn selected_font_size(which: usize) -> u16 {
    if which == 0 {
        config::font_size()
    } else {
        config::code_font_size()
    }
}
fn font_label(which: usize) -> &'static str {
    if which == 0 {
        "Interface font"
    } else {
        "Code font"
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
fn keymap_rows() -> Vec<String> {
    let keys = config::current().general.keymap;
    vec![
        keys.open_settings.join(" / "),
        keys.new_chat.join(" / "),
        keys.focus_search.join(" / "),
        keys.send_message.join(" / "),
        keys.close_dialog_or_popup.join(" / "),
        keys.next_control.join(" / "),
        keys.previous_control.join(" / "),
        "Enter / Space".into(),
    ]
}
fn error(message: impl Into<String>, cx: &App) -> Div {
    div()
        .text_sm()
        .text_color(cx.theme().danger)
        .child(message.into())
}
fn diagnostics(errors: Vec<String>, cx: &App) -> Div {
    let mut list = div().flex().flex_col().gap_2();
    for message in errors {
        list = list.child(error(message, cx));
    }
    list
}

impl SettingsWindow {
    fn new(
        owner: Owner,
        mode: Option<Section>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        let focus = cx.focus_handle().tab_stop(true);
        fonts::refresh(cx);
        let font_sizes = std::array::from_fn(|which| {
            text_input(selected_font_size(which).to_string(), "14", window, cx)
        });
        let retry_limit = text_input(
            client::connection(cx).settings.retry_limit.to_string(),
            "5",
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
            theme::discover().unwrap_or_else(|error| (Vec::new(), vec![error]));
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
                        this.font_size_errors[which] =
                            Some("Enter a whole number from 10 to 24.".into());
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
                match value {
                    Ok(limit) if limit != client::connection(cx).settings.retry_limit => {
                        let settings = protocol::EngineSettings {
                            retry_limit: limit,
                            ..client::connection(cx).settings.clone()
                        };
                        this.save_engine_settings(settings, cx);
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
                .err();
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
                if this
                    .agent_form
                    .as_ref()
                    .is_some_and(|form| form.read(cx).dirty(cx))
                {
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
                            Some(message)
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
                if view
                    .agent_form
                    .as_ref()
                    .is_some_and(|form| form.read(cx).dirty(cx))
                {
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
            engine_stopping: false,
            thinking_picker,
            thinking_error: None,
            theme_picker,
            font_pickers,
            theme_status,
            group: usize::from(mode == Some(Section::Chats)),
            subgroup: Some(0),
            search_page: None,
            folded: std::array::from_fn(|group| group != usize::from(mode == Some(Section::Chats))),
            focus,
            _subscriptions: subscriptions,
        }
    }

    fn show_agent(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let definition = self
            .owner
            .entity
            .read_with(cx, |app, _| {
                app.agent_catalog
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| entry.definition.clone())
            })
            .ok()
            .flatten();
        self.agent_page = Some(id.clone());
        self.group = 3;
        self.subgroup = None;
        self.folded[3] = false;
        self.agent_status = None;
        self.search_page = Some((3, None));
        self.agent_subscriptions.clear();
        client::ensure(cx);
        client::refresh_harnesses(false, cx);
        self.agent_form = definition.map(|definition| {
            cx.new(|cx| agent_form::AgentForm::new(Some(id), definition, window, cx))
        });
        if let Some(form) = &self.agent_form {
            self.agent_subscriptions
                .push(cx.observe(form, |_, _, cx| cx.notify()));
        }
        cx.notify();
    }

    fn sync_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.agent_form.clone() else {
            return;
        };
        let form_id = form.read(cx).id.clone();
        let current = self
            .owner
            .entity
            .read_with(cx, |app, _| {
                app.agent_catalog
                    .entries
                    .iter()
                    .find(|entry| Some(&entry.id) == form_id.as_ref())
                    .map(|entry| entry.definition.clone())
            })
            .ok()
            .flatten();
        if current.as_ref() == Some(&form.read(cx).original) {
            form.update(cx, |form, _| form.external_changed = false);
        } else if form.read(cx).dirty(cx) {
            form.update(cx, |form, _| form.external_changed = true);
        } else if let Some(definition) = current {
            form.update(cx, |form, cx| form.reload(definition, window, cx));
        } else {
            self.agent_form = None;
            self.agent_subscriptions.clear();
            self.agent_page = None;
        }
        cx.notify();
    }

    fn after_agent(&mut self, after: AfterAgent, window: &mut Window, cx: &mut Context<Self>) {
        match after {
            AfterAgent::CloseSettings => window.remove_window(),
            AfterAgent::CloseOwner => {
                self.agent_form = None;
                self.agent_subscriptions.clear();
                request_close(self.owner.window, self.owner.entity.clone(), cx);
            }
            AfterAgent::Child(group, child) => {
                self.group = group;
                self.folded[group] = false;
                self.subgroup = Some(child);
                self.agent_page = None;
                self.agent_form = None;
                self.agent_subscriptions.clear();
                self.search_page = Some((group, Some(child)));
            }
            AfterAgent::OpenMode(mode) => {
                self.last_query.clear();
                self.query
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.after_agent(
                    AfterAgent::Child(usize::from(mode == Section::Chats), 0),
                    window,
                    cx,
                );
            }
            AfterAgent::Agent(id) => self.show_agent(id, window, cx),
            AfterAgent::Delete(id) => self.confirm_delete_after_leaving(id, window, cx),
            AfterAgent::Search(query) => {
                self.last_query.clone_from(&query);
                self.agent_page = None;
                self.agent_form = None;
                self.agent_subscriptions.clear();
                self.search_page = None;
                self.query
                    .update(cx, |input, cx| input.set_value(query, window, cx));
            }
        }
        cx.notify();
    }

    fn save_agent(
        &mut self,
        after: Option<AfterAgent>,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.agent_form.clone() else {
            return;
        };
        let form_id = form.read(cx).id.clone();
        let current = self
            .owner
            .entity
            .read_with(cx, |app, _| {
                app.agent_catalog
                    .entries
                    .iter()
                    .find(|entry| Some(&entry.id) == form_id.as_ref())
                    .map(|entry| entry.definition.clone())
            })
            .ok()
            .flatten();
        if !overwrite
            && (form.read(cx).external_changed || current.as_ref() != Some(&form.read(cx).original))
        {
            if self.pending {
                return;
            }
            self.pending = true;
            let view = cx.entity().downgrade();
            choose(
                window,
                cx,
                "This agent changed outside Adeline.".into(),
                "Reload the external definition and lose your unsaved edits, or overwrite it with your edits.",
                &["Reload", "Overwrite", "Cancel"],
                move |choice, window, cx| {
                    let after = after.clone();
                    let _ = view.update(cx, |view, cx| {
                        view.pending = false;
                        match choice {
                            0 => {
                                // The engine watches agent files, so the list is current.
                                if let Some(form) = view.agent_form.clone() {
                                    let form_id = form.read(cx).id.clone();
                                    let definition = view
                                        .owner
                                        .entity
                                        .read_with(cx, |app, _| {
                                            app.agent_catalog
                                                .entries
                                                .iter()
                                                .find(|entry| Some(&entry.id) == form_id.as_ref())
                                                .map(|entry| entry.definition.clone())
                                        })
                                        .ok()
                                        .flatten();
                                    if let Some(definition) = definition {
                                        form.update(cx, |form, cx| {
                                            form.reload(definition, window, cx);
                                        });
                                    } else {
                                        view.agent_form = None;
                                        view.agent_page = None;
                                        view.agent_subscriptions.clear();
                                    }
                                }
                                if let Some(after) = after {
                                    view.after_agent(after, window, cx);
                                }
                            }
                            1 => view.save_agent(after, true, window, cx),
                            _ => {}
                        }
                    });
                },
            );
            return;
        }
        match persist_form(&self.owner, &form, overwrite, cx) {
            Ok(saved) => {
                form.update(cx, |form, cx| {
                    let definition = form.values(cx);
                    form.id = Some(saved.clone());
                    form.reload(definition, window, cx);
                });
                let after = after.map(|next| match next {
                    AfterAgent::Delete(_) => AfterAgent::Delete(saved.clone()),
                    other => other,
                });
                self.agent_page = Some(saved);
                if let Some(after) = after {
                    self.after_agent(after, window, cx);
                }
                cx.notify();
            }
            Err(message) => {
                let conflict = message.contains("changed outside this form");
                form.update(cx, |form, cx| {
                    form.external_changed = conflict;
                    form.status = Some(message);
                    cx.notify();
                });
                cx.notify();
            }
        }
    }

    fn leave(&mut self, after: AfterAgent, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if !self
            .agent_form
            .as_ref()
            .is_some_and(|form| form.read(cx).dirty(cx))
        {
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

    fn confirm_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if self
            .agent_form
            .as_ref()
            .is_some_and(|form| form.read(cx).dirty(cx))
        {
            self.leave(AfterAgent::Delete(id), window, cx);
        } else {
            self.confirm_delete_after_leaving(id, window, cx);
        }
    }
    fn confirm_delete_after_leaving(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending {
            return;
        }
        self.pending = true;
        let name = self
            .agent_form
            .as_ref()
            .map_or_else(|| id.clone(), |form| form.read(cx).original.name.clone());
        let view = cx.entity().downgrade();
        choose(
            window,
            cx,
            format!("Delete {name}?"),
            "The agent's folder and saved definition will be removed.",
            &["Delete", "Cancel"],
            move |choice, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.pending = false;
                    if choice != 0 {
                        return;
                    }
                    let owner = view.owner.clone();
                    let result = delete_agent(&owner, &id, cx);
                    match result {
                        Ok(()) => {
                            view.agent_page = None;
                            view.agent_form = None;
                            view.agent_subscriptions.clear();
                            view.agent_status = None;
                        }
                        Err(message) => view.agent_status = Some(message),
                    }
                    cx.notify();
                });
            },
        );
    }

    fn save_font_size(&mut self, which: usize, size: u16, cx: &mut Context<Self>) {
        if !(config::MIN_FONT_SIZE..=config::MAX_FONT_SIZE).contains(&size) {
            self.font_size_errors[which] = Some("Enter a whole number from 10 to 24.".into());
        } else if size == selected_font_size(which) {
            self.font_size_errors[which] = None;
        } else {
            match config::update(|settings| {
                if which == 0 {
                    settings.general.appearance.font_size = size;
                } else {
                    settings.general.appearance.code_font_size = size;
                }
            }) {
                Ok(()) => {
                    self.font_size_errors[which] = None;
                    theme::apply(cx);
                }
                Err(message) => self.font_size_errors[which] = Some(message),
            }
        }
        cx.notify();
    }
    fn choose_font(&mut self, which: usize, family: &str, cx: &mut Context<Self>) {
        match config::update(|settings| {
            if which == 0 {
                settings.general.appearance.interface_font = family.into();
            } else {
                settings.general.appearance.code_font = family.into();
            }
        }) {
            Ok(()) => {
                self.theme_status = None;
                theme::apply(cx);
            }
            Err(message) => self.theme_status = Some(message),
        }
        cx.notify();
    }
    fn refresh_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match theme::discover() {
            Ok((choices, errors)) => {
                let saved = config::current().general.appearance.theme;
                self.theme_picker.update(cx, |picker, cx| {
                    picker.set_items(SearchableVec::new(choices), window, cx);
                    picker.set_selected_value(&saved, window, cx);
                });
                self.theme_status = (!errors.is_empty())
                    .then(|| errors.join("\n"))
                    .or_else(theme::load_error);
            }
            Err(message) => self.theme_status = Some(message),
        }
        cx.notify();
    }

    fn refresh_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        fonts::refresh(cx);
        for (which, picker) in self.font_pickers.iter().enumerate() {
            let mut names = fonts::families();
            let saved = saved_font(which);
            if !names.iter().any(|name| name.eq_ignore_ascii_case(&saved)) && !saved.is_empty() {
                names.insert(0, saved.clone());
            }
            picker.update(cx, |picker, cx| {
                picker.set_items(SearchableVec::new(names), window, cx);
                picker.set_selected_value(&saved, window, cx);
            });
        }
        theme::apply(cx);
        cx.notify();
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
            .child(
                Switch::new(id)
                    .checked(checked)
                    .accessibility_label(label)
                    .on_change(move |_, window, cx| owner.act(action.clone(), window, cx)),
            )
    }
    fn retry_limit_row(&self, cx: &Context<Self>) -> Div {
        div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Field::new()
                    .label(RETRY_LABEL)
                    .description(RETRY_DESCRIPTION)
                    .child(
                        div()
                            .w_24()
                            .child(Input::new(&self.retry_limit).aria_label(RETRY_LABEL)),
                    ),
            )
            .when_some(self.retry_limit_error.clone(), |row, message| {
                row.child(error(message, cx))
            })
    }
    fn save_engine_settings(&mut self, settings: protocol::EngineSettings, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        client::request(
            protocol::Command::SetSettings { settings },
            Box::new(move |result, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.retry_limit_error = result.err();
                    cx.notify();
                });
            }),
            cx,
        );
    }

    fn stop_engine(&mut self, cx: &mut Context<Self>) {
        self.engine_stopping = true;
        let view = cx.entity().downgrade();
        client::request(
            protocol::Command::Shutdown,
            Box::new(move |_, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.engine_stopping = false;
                    cx.notify();
                });
            }),
            cx,
        );
        cx.notify();
    }

    /// Settings › Engine: the engine's own settings, its status, and Stop or Start.
    fn engine_settings(&self, query: &str, cx: &Context<Self>) -> Div {
        let connection = client::connection(cx);
        let connected = connection.state == client::State::Connected;
        let mut page = div().w_full().flex().flex_col().gap_3();
        page = page
            .child(div().text_lg().child("Engine"))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(
                "The conversation engine runs agents in the background, so work can finish after Adeline closes. These settings apply immediately, including to work already running.",
            ));
        let mut rows = Vec::new();
        if matches_query(
            query,
            &["Engine", KEEP_RUNNING, KEEP_RUNNING_DESCRIPTION, "daemon"],
        ) {
            let view = cx.entity().downgrade();
            let checked = connection.settings.keep_running;
            rows.push(
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
                            .child(KEEP_RUNNING)
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(KEEP_RUNNING_DESCRIPTION),
                            ),
                    )
                    .child(
                        Switch::new("engine-keep-running")
                            .checked(checked)
                            .disabled(!connected)
                            .accessibility_label(KEEP_RUNNING)
                            .on_change(move |_, _, cx| {
                                let settings = protocol::EngineSettings {
                                    keep_running: !checked,
                                    ..client::connection(cx).settings.clone()
                                };
                                let _ = view
                                    .update(cx, |this, cx| this.save_engine_settings(settings, cx));
                            }),
                    ),
            );
        }
        if matches_query(query, &["Engine", RETRY_LABEL, RETRY_DESCRIPTION]) {
            rows.push(self.retry_limit_row(cx));
        }
        if let Some(card) = card(rows, cx) {
            page = page.child(card);
        }
        if matches_query(
            query,
            &[
                "Engine",
                "Status",
                "PID",
                "Version",
                "Uptime",
                "Clients",
                "Stop engine",
                "Start engine",
            ],
        ) {
            let status = &connection.status;
            let state = match &connection.state {
                client::State::Connected => "Running".to_owned(),
                client::State::Connecting | client::State::Starting => "Starting…".to_owned(),
                client::State::Stopped { unexpected: true } => "Stopped unexpectedly".to_owned(),
                client::State::Stopped { .. } => "Stopped".to_owned(),
                client::State::Unavailable(reason) => format!("Unavailable: {reason}"),
                client::State::Mismatch(_) | client::State::Waiting => {
                    "An older engine is running".to_owned()
                }
                client::State::Demo => "Not used in demo mode".to_owned(),
            };
            let uptime = status.uptime_secs + connection.status_at.elapsed().as_secs();
            let mut fields = vec![("Status", state)];
            if connected {
                fields.extend([
                    ("PID", status.pid.to_string()),
                    ("Version", status.version.clone()),
                    ("Protocol", status.protocol.to_string()),
                    (
                        "Daemon mode",
                        if status.daemon { "On" } else { "Off" }.to_owned(),
                    ),
                    (
                        "Uptime",
                        format!("{}h {}m {}s", uptime / 3600, uptime / 60 % 60, uptime % 60),
                    ),
                    ("Connected clients", status.clients.to_string()),
                    (
                        "Active conversations",
                        status.conversations.len().to_string(),
                    ),
                ]);
                for conversation in &status.conversations {
                    fields.push((
                        "",
                        format!(
                            "{} / {} · {}",
                            conversation.project, conversation.title, conversation.state
                        ),
                    ));
                }
                fields.push(("Logs", status.log.clone()));
            }
            let rows = fields
                .into_iter()
                .map(|(label, value)| {
                    div()
                        .flex()
                        .justify_between()
                        .gap_4()
                        .px_4()
                        .py_2()
                        .child(label)
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(value),
                        )
                })
                .collect();
            page = page
                .child(caps("Status", cx).mt_2())
                .children(card(rows, cx));
            let button = if connected {
                Button::new("engine-stop")
                    .danger()
                    .label(if self.engine_stopping {
                        "Stopping engine…"
                    } else {
                        "Stop engine"
                    })
                    .disabled(self.engine_stopping)
                    .on_click(cx.listener(|this, _, _, cx| this.stop_engine(cx)))
            } else {
                Button::new("engine-start")
                    .primary()
                    .label("Start engine")
                    .disabled(matches!(
                        connection.state,
                        client::State::Demo | client::State::Connecting | client::State::Starting
                    ))
                    .on_click(cx.listener(|_, _, _, cx| client::connect(true, cx)))
            };
            page = page.child(div().flex().justify_end().mt_2().child(button.small()));
        }
        page
    }

    fn thinking_animation_row(&self, cx: &Context<Self>) -> Div {
        div()
            .w_full()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Field::new()
                    .label(THINKING_LABEL)
                    .description(THINKING_DESCRIPTION)
                    .child(div().w_64().child(
                        Select::new(&self.thinking_picker).accessibility_label(THINKING_LABEL),
                    )),
            )
            .when_some(self.thinking_error.clone(), |row, message| {
                row.child(error(message, cx))
            })
    }
    fn appearance_settings(&self, cx: &Context<Self>) -> Div {
        let mut page = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                Form::new().child(
                    Field::new()
                        .label("Theme")
                        .description("Themes are loaded from the Adeline themes folder.")
                        .child(
                            Select::new(&self.theme_picker)
                                .accessibility_label("Theme")
                                .search_placeholder("Search themes"),
                        ),
                ),
            )
            .child(
                Button::new("refresh-themes")
                    .small()
                    .ghost()
                    .label("Refresh themes")
                    .on_click(cx.listener(|this, _, window, cx| this.refresh_themes(window, cx))),
            );
        if let Some(status) = &self.theme_status {
            page = page.child(error(status.clone(), cx));
        }
        for which in 0..2 {
            let fallback = if which == 0 {
                fonts::DEFAULT
            } else {
                fonts::CODE_DEFAULT
            };
            let saved = saved_font(which);
            let resolved = if which == 0 {
                config::font()
            } else {
                config::code_font()
            };
            page = page.child(
                Form::new().child(
                    Field::new()
                        .label(font_label(which))
                        .description(format!(
                            "{fallback} is used if the selected font is unavailable."
                        ))
                        .child(
                            Select::new(&self.font_pickers[which])
                                .accessibility_label(font_label(which))
                                .search_placeholder("Search installed fonts"),
                        ),
                ),
            );
            if saved != resolved {
                page = page.child(error(
                    format!("{saved} is unavailable. Using {resolved}."),
                    cx,
                ));
            }
            let size_label = if which == 0 {
                "Interface font size"
            } else {
                "Code font size"
            };
            page = page.child(
                Form::new().child(
                    Field::new()
                        .label(size_label)
                        .description(
                            "A whole number from 10 to 24 pixels. Changes save immediately.",
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new(format!("decrease-font-size-{which}"))
                                        .label("Smaller")
                                        .accessibility_label(format!("Decrease {size_label}"))
                                        .disabled(
                                            selected_font_size(which) <= config::MIN_FONT_SIZE,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.step_font(which, -1, window, cx);
                                        })),
                                )
                                .child(div().w_16().child(
                                    Input::new(&self.font_sizes[which]).aria_label(size_label),
                                ))
                                .child(
                                    Button::new(format!("increase-font-size-{which}"))
                                        .label("Larger")
                                        .accessibility_label(format!("Increase {size_label}"))
                                        .disabled(
                                            selected_font_size(which) >= config::MAX_FONT_SIZE,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.step_font(which, 1, window, cx);
                                        })),
                                ),
                        ),
                ),
            );
            if let Some(message) = &self.font_size_errors[which] {
                page = page.child(error(message.clone(), cx));
            }
        }
        page.child(
            Button::new("refresh-fonts")
                .small()
                .ghost()
                .label("Refresh installed fonts")
                .on_click(cx.listener(|this, _, window, cx| this.refresh_fonts(window, cx))),
        )
    }
    fn step_font(&mut self, which: usize, step: i16, window: &mut Window, cx: &mut Context<Self>) {
        let next = selected_font_size(which)
            .saturating_add_signed(step)
            .clamp(config::MIN_FONT_SIZE, config::MAX_FONT_SIZE);
        self.save_font_size(which, next, cx);
        self.font_sizes[which].update(cx, |input, cx| {
            input.set_value(selected_font_size(which).to_string(), window, cx);
        });
    }
}

fn matches_query(query: &str, parts: &[&str]) -> bool {
    query
        .split_whitespace()
        .all(|word| parts.iter().any(|part| part.to_lowercase().contains(word)))
}
fn general_matches(child: usize, query: &str) -> bool {
    match child {
        0 => {
            matches_query(
                query,
                &[
                    "General",
                    "Features",
                    "Machine selector",
                    "Show the machine selector in the top bar.",
                ],
            ) || MODES.iter().any(|(_, name)| {
                matches_query(
                    query,
                    &["General", "Features", name, "Enable mode in the main view"],
                )
            })
        }
        1 => [
            "Theme",
            "Interface font",
            "Code font",
            "Interface font size",
            "Code font size",
        ]
        .iter()
        .any(|label| matches_query(query, &["General", "Appearance", label])),
        2 => KEYMAP
            .iter()
            .any(|label| matches_query(query, &["General", "Keymap", label])),
        _ => false,
    }
}
fn engine_matches(query: &str) -> bool {
    [
        KEEP_RUNNING,
        RETRY_LABEL,
        "Status",
        "PID",
        "Version",
        "Uptime",
        "Clients",
        "Stop engine",
        "Start engine",
        "daemon",
    ]
    .iter()
    .any(|label| matches_query(query, &["Engine", label]))
}
/// Small capitals in the code font, like the chat list's section labels.
fn caps(text: &str, cx: &App) -> Div {
    div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_uppercase())
}
/// Rows on one rounded surface, split by hairlines. Empty when no rows match a search.
fn card(rows: Vec<Div>, cx: &App) -> Option<Div> {
    (!rows.is_empty()).then(|| {
        let theme = cx.theme();
        col()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.foreground.alpha(0.025))
            .children(rows.into_iter().enumerate().map(|(ix, row)| {
                row.when(ix > 0, |row| row.border_t_1().border_color(theme.border))
            }))
    })
}
/// A sidebar group: chevron, name in capitals and its page count. Clicking folds it.
fn group_header(
    group: usize,
    count: usize,
    folded: bool,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let name = GROUPS[group];
    row()
        .id(SharedString::from(format!("settings-group-{group}")))
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
            this.folded[group] = !this.folded[group];
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
        for (group, name) in GROUPS.into_iter().enumerate() {
            if group == 2 && searching {
                continue;
            }
            let matching: Vec<_> = SUBGROUPS[group]
                .iter()
                .enumerate()
                .filter(|(child, title)| {
                    if group == 0 {
                        general_matches(*child, &query)
                    } else {
                        matches_query(&query, &[name, title])
                    }
                })
                .collect();
            let matching_agents: Vec<_> = if group == 3 {
                self.owner
                    .entity
                    .read_with(cx, |app, _| {
                        app.agent_catalog
                            .entries
                            .iter()
                            .filter(|entry| {
                                matches_query(
                                    &query,
                                    &[
                                        "Agents",
                                        &entry.definition.name,
                                        &entry.definition.model,
                                        &entry.definition.command,
                                    ],
                                )
                            })
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
                && !(group == 3 && matches_query(&query, &["Agents"]))
            {
                continue;
            }
            // Searching shows every match, whatever is folded.
            let folded = self.folded[group] && !searching;
            navigation = navigation.child(group_header(
                group,
                matching.len() + matching_agents.len(),
                folded,
                cx,
            ));
            if folded {
                continue;
            }
            let icon = match group {
                1 => "chat.svg",
                2 => "file.svg",
                4 => "devices.svg",
                _ => "robot.svg",
            };
            for (child, title) in matching {
                let icon = if group == 0 {
                    ["settings.svg", "pen.svg", "code.svg"][child]
                } else {
                    icon
                };
                navigation = navigation.child(
                    navigation_button(
                        format!("settings-{group}-{child}"),
                        *title,
                        icon,
                        self.group == group && self.subgroup == Some(child),
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.leave(AfterAgent::Child(group, child), window, cx);
                    })),
                );
            }
            for (id, title, harness) in matching_agents {
                let selected = self.agent_page.as_ref() == Some(&id);
                let installed = cx.global::<harness::Catalog>().is_installed(&harness);
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
            );
        let show = |group, child| {
            if searching {
                self.search_page
                    .is_none_or(|(g, c)| g == group && c.is_none_or(|c| c == child))
            } else {
                self.group == group && self.subgroup.is_none_or(|c| c == child)
            }
        };
        let heading = if searching {
            "Search results".to_string()
        } else if self.group == 3 && self.agent_form.is_some() {
            self.agent_form.as_ref().unwrap().read(cx).name(cx)
        } else {
            self.subgroup
                .map_or(GROUPS[self.group], |child| SUBGROUPS[self.group][child])
                .to_string()
        };
        // The title bar names the page, so the content starts with its settings.
        let crumbs = if searching || heading == GROUPS[self.group] {
            vec![heading]
        } else {
            vec![GROUPS[self.group].to_string(), heading]
        };
        let mut content = div().w_full().p_6().pt_4().flex().flex_col().gap_3();
        let mut found = false;
        if let Some(owner) = self.owner.entity.upgrade() {
            let app = owner.read(cx);
            if show(0, 0) && general_matches(0, &query) {
                found = true;
                if searching || self.subgroup.is_none() {
                    content = content.child(div().text_lg().child("Features"));
                }
                content = content.child(div().text_sm().text_color(cx.theme().muted_foreground)
                    .child("Choose which features are available in the main view. Chats is always enabled."));
                let features = config::current().general.features;
                let mut top_bar = Vec::new();
                if matches_query(
                    &query,
                    &["General", "Features", "Machine selector", "top bar"],
                ) {
                    top_bar.push(self.setting_row(
                        "machine-selector",
                        "Machine selector",
                        "Show the machine selector in the top bar.",
                        features.machine_selector,
                        Action::ToggleMachineSelector,
                        cx,
                    ));
                }
                let mut modes = Vec::new();
                for (section, name) in MODES.into_iter().skip(1) {
                    if matches_query(
                        &query,
                        &["General", "Features", name, "Enable mode in the main view"],
                    ) {
                        modes.push(self.setting_row(
                            format!("feature-{name}"),
                            name,
                            "",
                            features.enabled(section),
                            Action::ToggleMode(section),
                            cx,
                        ));
                    }
                }
                for (label, rows) in [("Top bar", top_bar), ("Modes", modes)] {
                    if let Some(card) = card(rows, cx) {
                        content = content.child(caps(label, cx).mt_2()).child(card);
                    }
                }
            }
            if show(0, 1) && general_matches(1, &query) {
                found = true;
                content = content.child(self.appearance_settings(cx));
            }
            if show(0, 2) && general_matches(2, &query) {
                found = true;
                if searching || self.subgroup.is_none() {
                    content = content.child(div().text_lg().child("Keymap"));
                }
                content = content.child(div().text_sm().text_color(cx.theme().muted_foreground)
                    .child("Edit shortcuts in settings.yml, then restart Adeline. Enter and Space activate focused controls."));
                let mut keys = Vec::new();
                for (label, value) in KEYMAP.into_iter().zip(keymap_rows()) {
                    if !matches_query(&query, &["General", "Keymap", label, &value]) {
                        continue;
                    }
                    keys.push(
                        div()
                            .flex()
                            .justify_between()
                            .gap_4()
                            .px_4()
                            .py_3()
                            .child(label)
                            .child(
                                div()
                                    .text_sm()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(cx.theme().muted_foreground)
                                    .child(value),
                            ),
                    );
                }
                content = content.children(card(keys, cx));
            }
            if show(1, 0) {
                let options: Vec<_> = app
                    .mode_options(Section::Chats)
                    .into_iter()
                    .enumerate()
                    .filter(|(_, option)| {
                        matches_query(&query, &["Modes", "Chats", option.0, option.1])
                    })
                    .collect();
                let thinking_matches = matches_query(
                    &query,
                    &["Modes", "Chats", THINKING_LABEL, THINKING_DESCRIPTION],
                );
                if !options.is_empty() || thinking_matches {
                    found = true;
                    if searching || self.subgroup.is_none() {
                        content = content.child(div().text_lg().child("Chats"));
                    }
                    let mut rows: Vec<Div> = options
                        .into_iter()
                        .map(|(index, (label, description, checked, action))| {
                            self.setting_row(
                                format!("chats-setting-{index}"),
                                label,
                                description,
                                checked,
                                action,
                                cx,
                            )
                        })
                        .collect();
                    if thinking_matches {
                        rows.push(self.thinking_animation_row(cx));
                    }
                    content = content.children(card(rows, cx));
                }
            }
        }
        if show(4, 0) && engine_matches(&query) {
            found = true;
            content = content.child(self.engine_settings(&query, cx));
        }
        if self.group == 2 && !searching {
            for (index, (name, description, license)) in [
                (
                    "Phosphor Icons",
                    "Primary interface icons · MIT · Copyright (c) 2020-2024 Phosphor Icons",
                    include_str!("../assets/PHOSPHOR-LICENSE.txt"),
                ),
                (
                    "GPUI",
                    "Native interface toolkit · Apache-2.0 · Zed Industries",
                    include_str!("../assets/GPUI-LICENSE.txt"),
                ),
                (
                    "Chivo & Chivo Mono",
                    "Bundled interface and code fonts · SIL Open Font License 1.1",
                    fonts::LICENSE,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                if self.subgroup.is_some_and(|child| child != index) {
                    continue;
                }
                found = true;
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_lg().child(name))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        )
                        .child(div().p_4().bg(cx.theme().muted).child(license)),
                );
            }
        }
        if self.group == 3 || searching {
            let matching_agent = self
                .owner
                .entity
                .read_with(cx, |app, _| {
                    app.agent_catalog.entries.iter().any(|entry| {
                        matches_query(
                            &query,
                            &[
                                "Agents",
                                &entry.definition.name,
                                &entry.definition.model,
                                &entry.definition.command,
                            ],
                        )
                    })
                })
                .unwrap_or(false);
            if !searching || matching_agent || matches_query(&query, &["Agents"]) {
                found = true;
                if let Some(form) = self.agent_form.clone() {
                    if self.group == 3 && (!searching || self.search_page.is_some()) {
                        let blocked = form.read(cx).blocked(cx);
                        content = content.child(form.clone());
                        let form = form.read(cx);
                        if form.external_changed {
                            content = content.child(error("This agent changed outside Adeline. Your edits are kept. Save to choose Reload or Overwrite.", cx));
                        }
                        if let Some(message) = &form.status {
                            content = content.child(error(message.clone(), cx));
                        }
                        content = content.child(
                            div()
                                .w_full()
                                .flex()
                                .justify_between()
                                .gap_2()
                                .mt_4()
                                .child(
                                    Button::new("settings-delete-agent")
                                        .danger()
                                        .label("Delete agent…")
                                        .disabled(self.pending)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if let Some(id) = this.agent_page.clone() {
                                                this.confirm_delete(id, window, cx);
                                            }
                                        })),
                                )
                                .child(
                                    Button::new("settings-save-agent")
                                        .primary()
                                        .label("Save")
                                        .disabled(self.pending || blocked)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.save_agent(None, false, window, cx);
                                        })),
                                ),
                        );
                    }
                } else {
                    content = content.child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("Select an agent in the sidebar to edit its settings."),
                    );
                }
                if let Some(message) = &self.agent_status {
                    content = content.child(error(message.clone(), cx));
                }
                content = content.child(diagnostics(self.owner.diagnostics(cx), cx));
            }
        }
        if !found {
            content = content.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("No settings found. Try another search."),
            );
        }
        if !searching
            && (self.group == 1 || (self.group == 0 && self.subgroup.is_none_or(|i| i == 0)))
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
