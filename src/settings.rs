use super::*;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, FocusableExt as _, IndexPath, Root, Selectable as _,
    Sizable as _, WindowExt as _,
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

const GROUPS: [&str; 4] = ["General", "Modes", "Licenses", "Agents"];
const SUBGROUPS: [&[&str]; 4] = [
    &["Features", "Appearance", "Keymap"],
    &["Chats"],
    &[
        "Phosphor Icons",
        "Lobe Icons · Mono",
        "GPUI",
        "Chivo & Chivo Mono",
    ],
    &[],
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

impl Adeline {
    pub(super) fn mode_options(&self, section: Section) -> Vec<SettingOption> {
        if section != Section::Chats {
            return Vec::new();
        }
        vec![
            (
                "Show idle chats",
                "Include completed conversations in the chat list.",
                self.show_completed,
                Action::ShowCompleted,
            ),
            (
                "Hide tool calls",
                "Hide tool calls and results in chats. Permission requests stay visible.",
                config::current().modes.chats.hide_tool_calls,
                Action::HideToolCalls,
            ),
            (
                "Show left panel",
                "Display chat navigation.",
                self.left_panel_open[Section::Chats as usize],
                Action::LeftPanel(Section::Chats),
            ),
            (
                "Show agent activity",
                "Display chat agent activity.",
                self.side_panel_open[Section::Chats as usize],
                Action::RightPanel(Section::Chats),
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

fn settings_windows(cx: &App) -> Vec<(WindowHandle<Root>, Entity<SettingsWindow>)> {
    cx.windows()
        .into_iter()
        .filter_map(|handle| {
            let root = handle.downcast::<Root>()?;
            let view = root
                .read(cx)
                .ok()?
                .view()
                .clone()
                .downcast::<SettingsWindow>()
                .ok()?;
            Some((root, view))
        })
        .collect()
}
fn agent_windows(cx: &App) -> Vec<(WindowHandle<Root>, Entity<AgentWindow>)> {
    cx.windows()
        .into_iter()
        .filter_map(|handle| {
            let root = handle.downcast::<Root>()?;
            let view = root
                .read(cx)
                .ok()?
                .view()
                .clone()
                .downcast::<AgentWindow>()
                .ok()?;
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
pub(super) fn open_mode(
    owner: WindowHandle<Root>,
    entity: WeakEntity<Adeline>,
    mode: Section,
    cx: &mut Context<Adeline>,
) {
    open_at(Owner::new(owner, entity), Some(mode), cx);
}
fn open_at(owner: Owner, mode: Option<Section>, cx: &mut Context<Adeline>) {
    cx.defer(move |cx| {
        if let Some((root, settings)) = settings_windows(cx)
            .into_iter()
            .find(|(_, view)| view.read(cx).owner.window.window_id() == owner.window.window_id())
        {
            let _ = cx.update_window(root.into(), |_, window, cx| {
                settings.update(cx, |settings, cx| {
                    if let Some(mode) = mode {
                        settings.leave(AfterAgent::OpenMode(mode), window, cx);
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
                titlebar: Some(TitlebarOptions {
                    title: Some("Settings · Adeline".into()),
                    appears_transparent: cfg!(target_os = "windows"),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| SettingsWindow::new(owner, mode, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
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
    for (root, settings) in settings_windows(cx) {
        if settings.read(cx).owner.window.window_id() == owner.window_id()
            && settings
                .read(cx)
                .agent_form
                .as_ref()
                .is_some_and(|form| form.dirty(cx))
        {
            let _ = cx.update_window(root.into(), |_, window, cx| {
                settings.update(cx, |view, cx| {
                    view.leave(AfterAgent::CloseOwner, window, cx);
                });
            });
            return false;
        }
    }
    for (root, creation) in agent_windows(cx) {
        if creation.read(cx).owner.window.window_id() == owner.window_id()
            && creation.read(cx).form.dirty(cx)
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
                let opening_shutdown = app.modal != Some("shutdown");
                let ready = app.request_runtime_exit(cx);
                if !ready && opening_shutdown {
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
    for (root, view) in settings_windows(cx) {
        if view.read(cx).owner.window.window_id() == owner.window_id() {
            let _ = root.update(cx, |_, window, _| window.remove_window());
        }
    }
    for (root, view) in agent_windows(cx) {
        if view.read(cx).owner.window.window_id() == owner.window_id() {
            let _ = root.update(cx, |_, window, _| window.remove_window());
        }
    }
}

fn persist_form(
    owner: &Owner,
    form: &agent_form::AgentForm,
    overwrite: bool,
    cx: &mut App,
) -> Result<String, String> {
    let definition = form.values(cx);
    let original = form.id.clone();
    let expected = original.as_ref().map(|_| form.original.clone());
    owner
        .entity
        .update(cx, move |app, cx| {
            let select_first = original.is_none() && app.agent_catalog.entries.is_empty();
            let saved = app.agent_catalog.save(
                original.as_deref(),
                definition,
                expected.as_ref(),
                overwrite,
            )?;
            app.agents_changed(original.as_deref(), Some(&saved), select_first, cx);
            Ok(saved)
        })
        .map_err(|error| error.to_string())?
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
    window.open_dialog(cx, move |dialog, _, _| {
        let mut footer = div().flex().justify_end().gap_2();
        for (index, &label) in labels.iter().enumerate() {
            let selected = selected.clone();
            let decided = decided.clone();
            let button = Button::new(format!("choice-{index}"))
                .label(label)
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
        dialog
            .title(title.clone())
            .child(description)
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
    form: agent_form::AgentForm,
    focus: FocusHandle,
    pending: bool,
    _subscriptions: Vec<Subscription>,
}
impl AgentWindow {
    fn new(owner: Owner, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = agent_form::AgentForm::new(
            None,
            agents::AgentDefinition {
                harness: "OMP".into(),
                driver: "ACP".into(),
                effort: "Medium".into(),
                ..Default::default()
            },
            window,
            cx,
        );
        let focus = cx.focus_handle().tab_stop(true);
        window.focus(&form.inputs[0].read(cx).focus_handle(cx), cx);
        let mut subscriptions = Vec::new();
        for input in &form.inputs {
            subscriptions.push(cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.form.status = None;
                    cx.notify();
                }
            }));
        }
        subscriptions.push(
            cx.subscribe(&form.instructions, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.form.status = None;
                    cx.notify();
                }
            }),
        );
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
                self.form.status = Some(error);
                cx.notify();
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
        if !self.form.dirty(cx) {
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
            .child(self.form.fields())
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
                    .when_some(self.form.status.clone(), |row, status| {
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
                            .disabled(self.pending)
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
            .children(window_layers(window, cx).into_iter().flatten())
    }
}

#[derive(Clone)]
enum AfterAgent {
    CloseSettings,
    CloseOwner,
    Group(usize),
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
    agent_form: Option<agent_form::AgentForm>,
    agent_subscriptions: Vec<Subscription>,
    agent_status: Option<String>,
    pending: bool,
    font_sizes: [Entity<InputState>; 2],
    font_size_errors: [Option<String>; 2],
    retry_limit: Entity<InputState>,
    retry_limit_error: Option<String>,
    theme_picker: Entity<SelectState<SearchableVec<theme::ThemeChoice>>>,
    font_pickers: [Entity<SelectState<SearchableVec<String>>>; 2],
    theme_status: Option<String>,
    group: usize,
    subgroup: Option<usize>,
    search_page: Option<(usize, Option<usize>)>,
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
            config::current().modes.chats.retry_limit.to_string(),
            "5",
            window,
            cx,
        );
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
                this.retry_limit_error = match value {
                    Ok(limit) => {
                        config::update(|settings| settings.modes.chats.retry_limit = limit).err()
                    }
                    Err(_) => {
                        Some("Enter a non-negative whole number. Zero disables retries.".into())
                    }
                };
                cx.notify();
            }),
        );
        subscriptions.push(cx.subscribe_in(
            &query,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let requested = input.read(cx).value().to_string();
                if requested == this.last_query {
                    return;
                }
                if this.agent_form.as_ref().is_some_and(|form| form.dirty(cx)) {
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
                if view.agent_form.as_ref().is_some_and(|form| form.dirty(cx)) {
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
            theme_picker,
            font_pickers,
            theme_status,
            group: usize::from(mode == Some(Section::Chats)),
            subgroup: Some(0),
            search_page: None,
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
        self.agent_status = None;
        self.search_page = Some((3, None));
        self.agent_subscriptions.clear();
        self.agent_form = definition
            .map(|definition| agent_form::AgentForm::new(Some(id), definition, window, cx));
        if let Some(form) = &self.agent_form {
            for input in &form.inputs {
                self.agent_subscriptions.push(cx.subscribe(
                    input,
                    |this, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            if let Some(form) = this.agent_form.as_mut() {
                                form.status = None;
                            }
                            cx.notify();
                        }
                    },
                ));
            }
            self.agent_subscriptions.push(cx.subscribe(
                &form.instructions,
                |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        if let Some(form) = this.agent_form.as_mut() {
                            form.status = None;
                        }
                        cx.notify();
                    }
                },
            ));
        }
        cx.notify();
    }

    fn sync_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.agent_form.as_mut() else {
            return;
        };
        let current = self
            .owner
            .entity
            .read_with(cx, |app, _| {
                app.agent_catalog
                    .entries
                    .iter()
                    .find(|entry| Some(&entry.id) == form.id.as_ref())
                    .map(|entry| entry.definition.clone())
            })
            .ok()
            .flatten();
        if current.as_ref() == Some(&form.original) {
            form.external_changed = false;
        } else if form.dirty(cx) {
            form.external_changed = true;
        } else if let Some(definition) = current {
            form.reload(definition, window, cx);
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
            AfterAgent::Group(group) => {
                self.group = group;
                self.agent_page = None;
                self.agent_form = None;
                self.agent_subscriptions.clear();
                self.subgroup = None;
                self.search_page = Some((group, None));
            }
            AfterAgent::Child(group, child) => {
                self.group = group;
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
        let Some(form) = self.agent_form.as_ref() else {
            return;
        };
        let current = self
            .owner
            .entity
            .read_with(cx, |app, _| {
                app.agent_catalog
                    .entries
                    .iter()
                    .find(|entry| Some(&entry.id) == form.id.as_ref())
                    .map(|entry| entry.definition.clone())
            })
            .ok()
            .flatten();
        if !overwrite && (form.external_changed || current.as_ref() != Some(&form.original)) {
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
                                let _ = view
                                    .owner
                                    .entity
                                    .update(cx, |app, cx| app.refresh_agents(cx));
                                if let Some(form) = view.agent_form.as_mut() {
                                    let definition = view
                                        .owner
                                        .entity
                                        .read_with(cx, |app, _| {
                                            app.agent_catalog
                                                .entries
                                                .iter()
                                                .find(|entry| Some(&entry.id) == form.id.as_ref())
                                                .map(|entry| entry.definition.clone())
                                        })
                                        .ok()
                                        .flatten();
                                    if let Some(definition) = definition {
                                        form.reload(definition, window, cx);
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
        match persist_form(&self.owner, form, overwrite, cx) {
            Ok(saved) => {
                if let Some(form) = &mut self.agent_form {
                    let definition = form.values(cx);
                    form.id = Some(saved.clone());
                    form.reload(definition, window, cx);
                }
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
                if conflict {
                    let _ = self
                        .owner
                        .entity
                        .update(cx, |app, cx| app.refresh_agents(cx));
                }
                if let Some(form) = self.agent_form.as_mut() {
                    form.external_changed = conflict;
                    form.status = Some(message);
                }
                cx.notify();
            }
        }
    }

    fn leave(&mut self, after: AfterAgent, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if !self.agent_form.as_ref().is_some_and(|form| form.dirty(cx)) {
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
        if self.agent_form.as_ref().is_some_and(|form| form.dirty(cx)) {
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
            .map_or_else(|| id.clone(), |form| form.original.name.clone());
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
                    let result = view
                        .owner
                        .entity
                        .update(cx, |app, cx| {
                            app.agent_catalog.delete(&id)?;
                            app.agents_changed(Some(&id), None, false, cx);
                            Ok::<_, String>(())
                        })
                        .map_err(|error| error.to_string())
                        .and_then(|result| result);
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
            .py_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(label)
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .child(
                Switch::new(id)
                    .checked(checked)
                    .accessibility_label(label)
                    .on_change(move |_, window, cx| owner.act(action.clone(), window, cx)),
            )
    }
    fn retry_limit_row(&self, cx: &Context<Self>) -> Div {
        div().w_full().py_4().flex().flex_col().gap_2().border_b_1().border_color(cx.theme().border)
            .child(Field::new().label("Automatic retry limit")
                .description("Additional attempts after a temporary failure. Zero disables automatic retries.")
                .child(div().w_24().child(Input::new(&self.retry_limit).aria_label("Automatic retry limit"))))
            .when_some(self.retry_limit_error.clone(), |row, message| row.child(error(message, cx)))
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
fn navigation_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .accessibility_label(label.clone())
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
                            .map(|entry| (entry.id.clone(), entry.definition.name.clone()))
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
            navigation = navigation.child(
                navigation_button(
                    format!("settings-group-{group}"),
                    name,
                    !searching && self.group == group && self.subgroup.is_none(),
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.leave(AfterAgent::Group(group), window, cx);
                })),
            );
            for (child, title) in matching {
                navigation = navigation.child(
                    div().pl_4().child(
                        navigation_button(
                            format!("settings-{group}-{child}"),
                            *title,
                            self.group == group && self.subgroup == Some(child),
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.leave(AfterAgent::Child(group, child), window, cx);
                            },
                        )),
                    ),
                );
            }
            for (id, title) in matching_agents {
                let selected = self.agent_page.as_ref() == Some(&id);
                navigation = navigation.child(
                    div().pl_4().child(
                        navigation_button(format!("settings-agent-{id}"), title, selected, cx)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.agent_page.as_deref() != Some(&id) {
                                    this.leave(AfterAgent::Agent(id.clone()), window, cx);
                                }
                            })),
                    ),
                );
            }
        }
        let sidebar = div()
            .w_64()
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(Input::new(&self.query).aria_label("Search settings"))
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
            self.agent_form.as_ref().unwrap().inputs[0]
                .read(cx)
                .value()
                .to_string()
        } else {
            self.subgroup
                .map_or(GROUPS[self.group], |child| SUBGROUPS[self.group][child])
                .to_string()
        };
        let mut content = div()
            .w_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child(heading));
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
                if matches_query(
                    &query,
                    &["General", "Features", "Machine selector", "top bar"],
                ) {
                    content = content.child(self.setting_row(
                        "machine-selector",
                        "Machine selector",
                        "Show the machine selector in the top bar.",
                        features.machine_selector,
                        Action::ToggleMachineSelector,
                        cx,
                    ));
                }
                for (section, name) in MODES.into_iter().skip(1) {
                    if matches_query(
                        &query,
                        &["General", "Features", name, "Enable mode in the main view"],
                    ) {
                        content = content.child(self.setting_row(
                            format!("feature-{name}"),
                            name,
                            "Enable this mode in the main view.",
                            features.enabled(section),
                            Action::ToggleMode(section),
                            cx,
                        ));
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
                for (label, value) in KEYMAP.into_iter().zip(keymap_rows()) {
                    if !matches_query(&query, &["General", "Keymap", label, &value]) {
                        continue;
                    }
                    content = content.child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_4()
                            .py_3()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(label)
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(value),
                            ),
                    );
                }
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
                let retry_matches = matches_query(
                    &query,
                    &[
                        "Modes",
                        "Chats",
                        "Automatic retry limit",
                        "Additional attempts after a temporary failure",
                    ],
                );
                if !options.is_empty() || retry_matches {
                    found = true;
                    if searching || self.subgroup.is_none() {
                        content = content.child(div().text_lg().child("Chats"));
                    }
                    for (index, (label, description, checked, action)) in options {
                        content = content.child(self.setting_row(
                            format!("chats-setting-{index}"),
                            label,
                            description,
                            checked,
                            action,
                            cx,
                        ));
                    }
                    if retry_matches {
                        content = content.child(self.retry_limit_row(cx));
                    }
                }
            }
        }
        if self.group == 2 && !searching {
            for (index, (name, description, license)) in [
                (
                    "Phosphor Icons",
                    "Primary interface icons · MIT · Copyright (c) 2020-2024 Phosphor Icons",
                    include_str!("../assets/PHOSPHOR-LICENSE.txt"),
                ),
                (
                    "Lobe Icons · Mono",
                    "Provider and tool brand icons · MIT · Copyright (c) 2023 LobeHub",
                    include_str!("../assets/LOBE-LICENSE.txt"),
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
                if let Some(form) = &self.agent_form {
                    if self.group == 3 && (!searching || self.search_page.is_some()) {
                        content = content.child(form.fields());
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
                                        .disabled(self.pending)
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
        let shell = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground);
        #[cfg(target_os = "windows")]
        let shell = shell.child(titlebar::render("Settings · Adeline".into(), window));
        shell
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_stretch()
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
                            .overflow_y_scrollbar()
                            .id("settings-content")
                            .child(content),
                    ),
            )
            .children(window_layers(window, cx).into_iter().flatten())
    }
}
