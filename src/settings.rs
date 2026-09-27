use super::*;

const GROUPS: [&str; 4] = ["General", "Modes", "Licenses", "Agents"];
const SUBGROUPS: [&[&str]; 4] = [
    &["Features", "Appearance", "Keymap"],
    &[
        "Chats",
        "Docs",
        "Workflows",
        "Services",
        "Groupchats",
        "Issues",
        "Whiteboard",
    ],
    &[
        "Phosphor Icons",
        "Lobe Icons · Mono",
        "GPUI",
        "Chivo & Chivo Mono",
    ],
    &[],
];
const THEME: [(&str, &str); 5] = [
    ("Theme", "Claude Plus lightos colors appearance dropdown"),
    ("Interface font size", "text size smaller larger pixels"),
    ("Code font", "installed system fonts Chivo Mono dropdown"),
    ("Code font size", "code text size smaller larger pixels"),
    (
        "Interface font",
        "installed system fonts Chivo Mono dropdown",
    ),
];
const KEYMAP: [(&str, &str); 7] = [
    ("Open settings", "Ctrl/Cmd+,"),
    ("New chat", "Ctrl/Cmd+N"),
    ("Focus search", "Ctrl/Cmd+F"),
    ("Send message", "Ctrl/Cmd+Enter"),
    ("Close dialog or popup", "Escape"),
    ("Move between controls", "Tab / Shift+Tab"),
    ("Activate focused control", "Enter / Space"),
];
fn keymap_rows() -> Vec<(&'static str, String)> {
    let keys = config::current().general.keymap;
    vec![
        ("Open settings", keys.open_settings.join(" / ")),
        ("New chat", keys.new_chat.join(" / ")),
        ("Focus search", keys.focus_search.join(" / ")),
        ("Send message", keys.send_message.join(" / ")),
        (
            "Close dialog or popup",
            keys.close_dialog_or_popup.join(" / "),
        ),
        ("Next control", keys.next_control.join(" / ")),
        ("Previous control", keys.previous_control.join(" / ")),
        (
            "Activate focused control (built in)",
            "Enter / Space".into(),
        ),
    ]
}
const MODES: [(Section, &str); 7] = [
    (Section::Chats, "Chats"),
    (Section::Docs, "Docs"),
    (Section::Workflows, "Workflows"),
    (Section::Services, "Services"),
    (Section::Groupchats, "Groupchats"),
    (Section::Issues, "Issues"),
    (Section::Whiteboard, "Whiteboard"),
];
type SettingOption = (&'static str, &'static str, bool, Action);
const MACHINE_SELECTOR: [&str; 4] = [
    "General",
    "Features",
    "Machine selector",
    "Show the machine selector in the top bar.",
];

impl Adeline {
    pub(super) fn mode_options(&self, section: Section) -> Vec<SettingOption> {
        let mut options = match section {
            Section::Groupchats | Section::Issues | Section::Whiteboard => vec![],
            Section::Chats => vec![
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
            ],
            Section::Docs => vec![
                (
                    "Show raw Markdown",
                    "View the source of the selected document.",
                    self.raw,
                    Action::Raw,
                ),
                (
                    "Show archived documents",
                    "Include archived documents in the file list.",
                    self.archived,
                    Action::Archive,
                ),
            ],
            Section::Workflows => vec![(
                "Only scheduled workflows",
                "Limit the workflow list to scheduled workflows.",
                self.collection == "Scheduled",
                Action::Collection(
                    if self.collection == "Scheduled" {
                        "All"
                    } else {
                        "Scheduled"
                    }
                    .into(),
                ),
            )],
            Section::Services => vec![
                (
                    "Wrap output lines",
                    "Keep long output lines within the panel width.",
                    self.wrap,
                    Action::Wrap,
                ),
                (
                    "Follow latest output",
                    "Keep the latest service output in view.",
                    self.follow,
                    Action::Follow,
                ),
            ],
        };
        options.push((
            "Show left panel",
            "Display this mode's navigation panel.",
            self.left_panel_open[section as usize],
            Action::LeftPanel(section),
        ));
        options.push((
            match section {
                Section::Chats => "Show agent activity",
                Section::Groupchats | Section::Whiteboard => "Show members",
                Section::Issues => "Show ticket details",
                Section::Docs => "Show document details",
                Section::Workflows => "Show workflow details",
                Section::Services => "Show service details",
            },
            "Display this mode's right panel.",
            self.side_panel_open[section as usize],
            Action::RightPanel(section),
        ));
        options
    }
}

pub(super) fn open(owner: WindowHandle<Adeline>, cx: &mut Context<Adeline>) {
    open_at(owner, None, cx);
}
pub(super) fn open_agent(owner: WindowHandle<Adeline>, cx: &mut Context<Adeline>) {
    let entity = cx.entity();
    cx.defer(move |cx| {
        let bounds = Bounds::centered(None, size(px(760.), px(760.)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(620.), px(500.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Add an Agent · Adeline".into()),
                    appears_transparent: cfg!(target_os = "windows"),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| AgentWindow::new(owner, &entity, window, cx)),
        ) {
            eprintln!("Could not open agent editor: {error}");
        }
    });
}

pub(super) fn can_close_for(owner: WindowHandle<Adeline>, cx: &mut App) -> bool {
    for handle in cx.windows() {
        if let Some(settings) = handle.downcast::<SettingsWindow>()
            && settings.read(cx).is_ok_and(|view| {
                view.owner.window_id() == owner.window_id()
                    && view.agent_form.as_ref().is_some_and(|form| form.dirty(cx))
            })
        {
            let _ = settings.update(cx, |view, window, cx| {
                view.leave(AfterAgent::CloseOwner, window, cx);
            });
            return false;
        }
        if let Some(creation) = handle.downcast::<AgentWindow>()
            && creation.read(cx).is_ok_and(|view| {
                view.owner.window_id() == owner.window_id() && view.form.dirty(cx)
            })
        {
            let _ = creation.update(cx, |view, window, cx| view.confirm_close(true, window, cx));
            return false;
        }
    }
    owner
        .update(cx, |app, _, cx| app.request_runtime_exit(cx))
        .unwrap_or(false)
}

pub(super) fn request_close(owner: WindowHandle<Adeline>, cx: &mut App) {
    cx.defer(move |cx| {
        if can_close_for(owner, cx) {
            let _ = owner.update(cx, |_, window, _| window.remove_window());
        }
    });
}

fn persist_form(
    owner: WindowHandle<Adeline>,
    form: &agent_form::AgentForm,
    overwrite: bool,
    cx: &mut App,
) -> Result<String, String> {
    let definition = form.values(cx);
    let original = form.id.clone();
    let expected = original.as_ref().map(|_| form.original.clone());
    owner
        .update(cx, move |app, _, cx| {
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

#[derive(Clone)]
enum AfterAgent {
    CloseSettings,
    CloseOwner,
    Group(usize),
    Child(usize, usize),
    Agent(String),
    Delete(String),
    Search(String),
}

fn agent_diagnostics(owner: WindowHandle<Adeline>, cx: &App) -> Vec<String> {
    owner
        .read(cx)
        .map_or_else(|_| Vec::new(), |app| app.agent_catalog.errors.clone())
}

fn diagnostics(errors: Vec<String>) -> Div {
    let mut section = col().gap_2();
    for error in errors {
        section = section
            .child(text(error, 12., theme::destructive()).line_height(config::text_pixels(19.)));
    }
    section
}

pub(super) fn close_for(owner: WindowHandle<Adeline>, cx: &mut App) {
    for handle in cx.windows() {
        if let Some(settings) = handle.downcast::<SettingsWindow>() {
            let _ = settings.update(cx, |settings, window, _| {
                if settings.owner.window_id() == owner.window_id() {
                    window.remove_window();
                }
            });
        }
        if let Some(creation) = handle.downcast::<AgentWindow>() {
            let _ = creation.update(cx, |creation, window, _| {
                if creation.owner.window_id() == owner.window_id() {
                    window.remove_window();
                }
            });
        }
    }
}
pub(super) fn open_mode(owner: WindowHandle<Adeline>, mode: Section, cx: &mut Context<Adeline>) {
    open_at(owner, Some(mode), cx);
}
fn open_at(owner: WindowHandle<Adeline>, mode: Option<Section>, cx: &mut Context<Adeline>) {
    // Defer until the workspace is off the entity stack, as in Zed's settings window.
    let entity = cx.entity();
    cx.defer(move |cx| {
        if let Some(existing) = cx
            .windows()
            .into_iter()
            .find_map(|w| w.downcast::<SettingsWindow>())
        {
            let _ = existing.update(cx, |settings, window, cx| {
                if let Some(mode) = mode {
                    settings.leave(AfterAgent::Child(1, mode as usize), window, cx);
                }
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
                cx.new(|cx| {
                    let mut settings = SettingsWindow::new(owner, &entity, window, cx);
                    if let Some(mode) = mode {
                        settings.select_mode(mode, cx);
                    }
                    settings
                })
            },
        );
        if let Err(error) = result {
            eprintln!("Could not open settings: {error}");
        }
    });
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
struct AgentWindow {
    owner: WindowHandle<Adeline>,
    form: agent_form::AgentForm,
    scroll: scrollbar::PanelScroll,
    focus: FocusHandle,
    pending: bool,
    _subscriptions: Vec<Subscription>,
}

impl AgentWindow {
    fn new(
        owner: WindowHandle<Adeline>,
        entity: &Entity<Adeline>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = agent_form::AgentForm::new(
            None,
            agents::AgentDefinition {
                harness: "OMP".into(),
                driver: "ACP".into(),
                effort: "Medium".into(),
                ..Default::default()
            },
            cx,
        );
        let focus = cx.focus_handle();
        window.focus(&form.inputs[0].read(cx).focus_handle(cx), cx);
        let mut subscriptions = vec![cx.observe(entity, |_, _, cx| cx.notify())];
        let mut inputs = form.inputs.to_vec();
        inputs.extend(form.argument_inputs());
        for input in &inputs {
            subscriptions.push(
                cx.subscribe(input, |this, _, _: &input::ContentChanged, cx| {
                    this.form.status = None;
                    cx.notify();
                }),
            );
        }
        let view = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, cx| view.confirm_close(false, window, cx))
        });
        Self {
            owner,
            form,
            scroll: Default::default(),
            focus,
            pending: false,
            _subscriptions: subscriptions,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match persist_form(self.owner, &self.form, false, cx) {
            Ok(_) => {
                let definition = self.form.values(cx);
                self.form.reload(definition, cx);
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
        let response = window.prompt(
            PromptLevel::Warning,
            "Save changes to this agent?",
            Some("Unsaved agent details will be lost if you discard them."),
            &[
                PromptButton::ok("Save"),
                PromptButton::new("Discard"),
                PromptButton::cancel("Cancel"),
            ],
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            let choice = response.await.unwrap_or(2);
            let _ = view.update_in(cx, |view, window, cx| {
                view.pending = false;
                match choice {
                    0 => {
                        if view.save(window, cx) && close_owner {
                            request_close(view.owner, cx);
                        }
                    }
                    1 => {
                        let owner = view.owner;
                        window.remove_window();
                        if close_owner {
                            request_close(owner, cx);
                        }
                    }
                    _ => {}
                }
            });
        })
        .detach();
        false
    }
}

impl Render for AgentWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(not(target_os = "windows"))]
        let _ = window;
        let mut content = col()
            .w_full()
            .gap_5()
            .p_8()
            .child(text("Add an Agent", 22., theme::foreground()))
            .child(text(
                "Configure an agent for this workspace. Saving does not start the command.",
                12.,
                theme::muted_foreground(),
            ))
            .child(self.form.fields(cx));
        if let Some(status) = &self.form.status {
            content = content.child(text(status.clone(), 12., theme::destructive()));
        }
        content = content.child(diagnostics(agent_diagnostics(self.owner, cx)));
        let shell = col()
            .size_full()
            .font_family(config::font())
            .bg(rgb(theme::background()))
            .text_color(rgb(theme::foreground()));
        #[cfg(target_os = "windows")]
        let shell = shell.child(titlebar::render("Add an Agent · Adeline".into(), window));
        shell.child(
            col()
                .flex_1()
                .min_h_0()
                .track_focus(&self.focus)
                .on_action(cx.listener(|view, _: &Dismiss, window, cx| {
                    if view.confirm_close(false, window, cx) {
                        window.remove_window();
                    }
                }))
                .on_action(cx.listener(|_, _: &NextFocus, window, cx| window.focus_next(cx)))
                .on_action(cx.listener(|_, _: &PreviousFocus, window, cx| window.focus_prev(cx)))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.scroll.wrap("agent-creation", content)),
                )
                .child(
                    row()
                        .justify_end()
                        .gap_3()
                        .px_8()
                        .py_4()
                        .border_t_1()
                        .border_color(rgb(theme::border()))
                        .child(
                            row()
                                .id("agent-create-cancel")
                                .focusable()
                                .tab_stop(true)
                                .cursor_pointer()
                                .px_5()
                                .py_2()
                                .rounded(px(5.))
                                .bg(rgb(theme::secondary()))
                                .focus(|s| s.border_1().border_color(rgb(theme::ring())))
                                .child(text("Cancel", 13., theme::secondary_foreground()))
                                .on_click(cx.listener(|view, _, window, cx| {
                                    if view.confirm_close(false, window, cx) {
                                        window.remove_window();
                                    }
                                })),
                        )
                        .child(
                            row()
                                .id("agent-create-save")
                                .focusable()
                                .tab_stop(true)
                                .cursor_pointer()
                                .px_5()
                                .py_2()
                                .rounded(px(5.))
                                .bg(rgb(theme::primary()))
                                .focus(|s| s.border_1().border_color(rgb(theme::ring())))
                                .child(text("Save", 13., theme::primary_foreground()))
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.save(window, cx);
                                })),
                        ),
                ),
        )
    }
}

struct SettingsWindow {
    owner: WindowHandle<Adeline>,
    query: Entity<TextInput>,
    agent_page: Option<String>,
    agent_form: Option<agent_form::AgentForm>,
    agent_subscriptions: Vec<Subscription>,
    last_query: String,
    agent_status: Option<String>,
    pending: bool,
    font_query: Entity<TextInput>,
    font_size_inputs: [Entity<TextInput>; 2],
    retry_limit_input: Entity<TextInput>,
    retry_limit_error: Option<String>,
    font_size_errors: [Option<String>; 2],
    font_dropdown: Option<usize>,
    font_trigger_bounds: [std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>; 2],
    font_choices: Vec<String>,
    font_list_scroll: scrollbar::PanelScroll,
    theme_status: Option<String>,
    theme_dropdown: bool,
    theme_trigger_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    theme_choices: Vec<theme::ThemeChoice>,
    theme_list_scroll: scrollbar::PanelScroll,
    group: usize,
    subgroup: Option<usize>,
    search_page: Option<(usize, Option<usize>)>,
    expanded: [bool; 4],
    nav_scroll: scrollbar::PanelScroll,
    scroll: scrollbar::PanelScroll,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}
impl SettingsWindow {
    fn show_agent(&mut self, id: String, cx: &mut Context<Self>) {
        let definition = self.owner.read(cx).ok().and_then(|app| {
            app.agent_catalog
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| entry.definition.clone())
        });
        self.agent_page = Some(id.clone());
        self.group = 3;
        self.subgroup = None;
        self.agent_status = None;
        self.search_page = Some((3, None));
        self.expanded[3] = true;
        self.scroll.handle.set_offset(point(px(0.), px(0.)));
        self.agent_subscriptions.clear();
        self.agent_form =
            definition.map(|definition| agent_form::AgentForm::new(Some(id), definition, cx));
        if let Some(form) = &self.agent_form {
            let mut inputs = form.inputs.to_vec();
            inputs.extend(form.argument_inputs());
            for input in &inputs {
                self.agent_subscriptions.push(cx.subscribe(
                    input,
                    |this, _, _: &input::ContentChanged, cx| {
                        if let Some(form) = this.agent_form.as_mut() {
                            form.status = None;
                        }
                        cx.notify();
                    },
                ));
            }
        }
        cx.notify();
    }

    fn sync_agent(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.agent_form.as_mut() else {
            cx.notify();
            return;
        };
        let current = self.owner.read(cx).ok().and_then(|app| {
            app.agent_catalog
                .entries
                .iter()
                .find(|entry| Some(&entry.id) == form.id.as_ref())
                .map(|entry| entry.definition.clone())
        });
        if current.as_ref() == Some(&form.original) {
            form.external_changed = false;
        } else if form.dirty(cx) {
            form.external_changed = true;
        } else if let Some(definition) = current {
            form.reload(definition, cx);
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
                request_close(self.owner, cx);
            }
            AfterAgent::Group(group) => {
                self.group = group;
                self.agent_page = None;
                self.agent_form = None;
                self.agent_subscriptions.clear();
                self.subgroup = None;
                self.search_page = Some((group, None));
                self.expanded[group] = !self.expanded[group];
            }
            AfterAgent::Child(group, child) => {
                self.group = group;
                self.subgroup = Some(child);
                self.agent_page = None;
                self.agent_form = None;
                self.agent_subscriptions.clear();
                self.search_page = Some((group, Some(child)));
            }
            AfterAgent::Agent(id) => self.show_agent(id, cx),
            AfterAgent::Delete(id) => self.confirm_delete_after_leaving(id, window, cx),
            AfterAgent::Search(query) => {
                self.last_query.clone_from(&query);
                self.agent_page = None;
                self.agent_form = None;
                self.agent_subscriptions.clear();
                self.search_page = None;
                self.query.update(cx, |input, cx| input.set(query, cx));
                self.nav_scroll.handle.set_offset(point(px(0.), px(0.)));
            }
        }
        self.scroll.handle.set_offset(point(px(0.), px(0.)));
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
        let current = self.owner.read(cx).ok().and_then(|app| {
            app.agent_catalog
                .entries
                .iter()
                .find(|entry| Some(&entry.id) == form.id.as_ref())
                .map(|entry| entry.definition.clone())
        });
        if !overwrite && (form.external_changed || current.as_ref() != Some(&form.original)) {
            if self.pending {
                return;
            }
            self.pending = true;
            let response = window.prompt(
                PromptLevel::Warning,
                "This agent changed outside Adeline.",
                Some("Reload the external definition and lose your unsaved edits, or overwrite it with your edits."),
                &[PromptButton::ok("Reload"), PromptButton::new("Overwrite"), PromptButton::cancel("Cancel")], cx,
            );
            cx.spawn_in(window, async move |view, cx| {
                let choice = response.await.unwrap_or(2);
                let _ = view.update_in(cx, |view, window, cx| {
                    view.pending = false;
                    match choice {
                        0 => {
                            let _ = view.owner.update(cx, |app, _, cx| app.refresh_agents(cx));
                            if let Some(form) = view.agent_form.as_mut() {
                                let definition = view.owner.read(cx).ok().and_then(|app| {
                                    app.agent_catalog
                                        .entries
                                        .iter()
                                        .find(|entry| Some(&entry.id) == form.id.as_ref())
                                        .map(|entry| entry.definition.clone())
                                });
                                if let Some(definition) = definition {
                                    form.reload(definition, cx);
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
            })
            .detach();
            return;
        }
        match persist_form(self.owner, form, overwrite, cx) {
            Ok(saved) => {
                if let Some(form) = &mut self.agent_form {
                    let definition = form.values(cx);
                    form.id = Some(saved.clone());
                    form.reload(definition, cx);
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
            Err(error) => {
                let conflict = error.contains("changed outside this form");
                if conflict {
                    let _ = self.owner.update(cx, |app, _, cx| app.refresh_agents(cx));
                }
                if let Some(form) = &mut self.agent_form {
                    form.external_changed = conflict;
                    form.status = Some(error);
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
        let response = window.prompt(
            PromptLevel::Warning,
            "Save changes to this agent?",
            Some("Your edits will be lost if you discard them."),
            &[
                PromptButton::ok("Save"),
                PromptButton::new("Discard"),
                PromptButton::cancel("Cancel"),
            ],
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            let choice = response.await.unwrap_or(2);
            let _ = view.update_in(cx, |view, window, cx| {
                view.pending = false;
                match choice {
                    0 => view.save_agent(Some(after), false, window, cx),
                    1 => {
                        if let Some(form) = view.agent_form.as_mut() {
                            form.reload(form.original.clone(), cx);
                        }
                        view.after_agent(after, window, cx);
                    }
                    _ => {}
                }
            });
        })
        .detach();
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
        let response = window.prompt(
            PromptLevel::Warning,
            &format!("Delete {name}?"),
            Some("The agent's folder and saved definition will be removed."),
            &[PromptButton::ok("Delete"), PromptButton::cancel("Cancel")],
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            let choice = response.await.unwrap_or(1);
            let _ = view.update_in(cx, |view, _, cx| {
                view.pending = false;
                if choice != 0 {
                    return;
                }
                let result = view
                    .owner
                    .update(cx, |app, _, cx| {
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
                    Err(error) => view.agent_status = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save_font_size(&mut self, which: usize, size: u16, cx: &mut Context<Self>) {
        if !(config::MIN_FONT_SIZE..=config::MAX_FONT_SIZE).contains(&size) {
            self.font_size_errors[which] = Some("Enter a whole number from 10 to 24.".into());
        } else if size == selected_font_size(which) {
            self.font_size_errors[which] = None;
        } else {
            match config::update(|s| {
                if which == 0 {
                    s.general.appearance.font_size = size;
                } else {
                    s.general.appearance.code_font_size = size;
                }
            }) {
                Ok(()) => {
                    self.font_size_errors[which] = None;
                    cx.refresh_windows();
                }
                Err(error) => self.font_size_errors[which] = Some(error),
            }
        }
        cx.notify();
    }

    fn font_size_picker(&self, which: usize, cx: &Context<Self>) -> Div {
        let button = |id, label, increase| {
            row()
                .id((id, which))
                .focusable()
                .tab_stop(true)
                .cursor_pointer()
                .justify_center()
                .w(px(36.))
                .py_2()
                .rounded(px(3.))
                .bg(rgb(theme::secondary()))
                .border_1()
                .border_color(rgb(theme::border()))
                .focus(|s| s.border_color(rgb(theme::ring())))
                .child(text(label, 16., theme::secondary_foreground()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let size = if increase {
                        selected_font_size(which) + 1
                    } else {
                        selected_font_size(which) - 1
                    }
                    .clamp(config::MIN_FONT_SIZE, config::MAX_FONT_SIZE);
                    this.save_font_size(which, size, cx);
                    let saved = selected_font_size(which);
                    this.font_size_inputs[which]
                        .update(cx, |input, cx| input.set(saved.to_string(), cx));
                }))
        };
        col()
            .gap_2()
            .child(text(
                if which == 0 {
                    "Interface font size"
                } else {
                    "Code font size"
                },
                13.,
                theme::foreground(),
            ))
            .child(
                row()
                    .gap_2()
                    .child(button("decrease-font-size", "−", false))
                    .child(
                        div()
                            .w(px(64.))
                            .px_2()
                            .py_2()
                            .bg(rgb(theme::input()))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .rounded(px(3.))
                            .child(self.font_size_inputs[which].clone()),
                    )
                    .child(button("increase-font-size", "+", true))
                    .child(text("px", 12., theme::muted_foreground())),
            )
            .when_some(self.font_size_errors[which].clone(), |d, error| {
                d.child(text(error, 12., theme::destructive()))
            })
    }

    fn choose_font(
        &mut self,
        which: usize,
        family: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match config::update(|s| {
            if which == 0 {
                s.general.appearance.interface_font = family.into();
            } else {
                s.general.appearance.code_font = family.into();
            }
        }) {
            Ok(()) => {
                self.font_dropdown = None;
                self.theme_status = Some(format!("{} saved.", font_label(which)));
                window.focus(&self.focus, cx);
                cx.refresh_windows();
            }
            Err(error) => self.theme_status = Some(error),
        }
        cx.notify();
    }

    fn font_picker(&self, which: usize, cx: &Context<Self>) -> Div {
        let selected = if which == 0 {
            config::font()
        } else {
            config::code_font()
        };
        let bounds = self.font_trigger_bounds[which].clone();
        let label = if matches!(selected.as_str(), fonts::DEFAULT | fonts::CODE_DEFAULT) {
            format!("{selected} (built in)")
        } else {
            selected.clone()
        };
        let mut picker = col().relative().w_full().child(
            row()
                .id(("font-dropdown", which))
                .relative()
                .focusable()
                .tab_stop(true)
                .cursor_pointer()
                .px_3()
                .py_2()
                .gap_3()
                .rounded(px(3.))
                .border_1()
                .border_color(rgb(theme::border()))
                .bg(rgb(theme::secondary()))
                .focus(|s| s.border_color(rgb(theme::ring())))
                .child(text(label, 13., theme::secondary_foreground()).flex_1())
                .child(icon("chevron"))
                .child(
                    canvas(move |area, _, _| bounds.set(area), |_, (), _, _| {})
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.theme_dropdown = false;
                    this.font_dropdown = if this.font_dropdown == Some(which) {
                        None
                    } else {
                        Some(which)
                    };
                    if this.font_dropdown.is_some() {
                        fonts::refresh(cx);
                        this.font_choices = fonts::families();
                        this.font_query.update(cx, |input, cx| input.set("", cx));
                        this.font_list_scroll
                            .handle
                            .set_offset(point(px(0.), px(0.)));
                        window.focus(&this.font_query.read(cx).focus_handle(cx), cx);
                    }
                    cx.notify();
                })),
        );
        if self.font_dropdown == Some(which) {
            let query = self.font_query.read(cx).content.to_lowercase();
            let mut options = col();
            let mut count = 0;
            for (i, family) in self.font_choices.iter().enumerate() {
                if !family.to_lowercase().contains(query.trim()) {
                    continue;
                }
                count += 1;
                let family = family.clone();
                let label = if matches!(family.as_str(), fonts::DEFAULT | fonts::CODE_DEFAULT) {
                    format!("{family} (built in)")
                } else {
                    family.clone()
                };
                options = options.child(
                    row()
                        .id(("interface-font-option", i))
                        .focusable()
                        .tab_stop(true)
                        .cursor_pointer()
                        .px_3()
                        .py_2()
                        .rounded(px(3.))
                        .when(family == selected, |d| d.bg(rgb(theme::secondary())))
                        .hover(|s| s.bg(rgb(theme::secondary())))
                        .focus(|s| s.border_1().border_color(rgb(theme::ring())))
                        .child(text(label, 13., theme::foreground()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_font(which, &family, window, cx);
                        })),
                );
            }
            if count == 0 {
                options =
                    options.child(text("No matching fonts.", 13., theme::muted_foreground()).p_3());
            }
            picker = picker.child(
                deferred(
                    anchored()
                        .position(self.font_trigger_bounds[which].get().bottom_left())
                        .snap_to_window()
                        .child(
                            menu_surface()
                                .id("interface-font-popup")
                                .w(self.font_trigger_bounds[which].get().size.width)
                                .on_mouse_down_out(cx.listener(
                                    move |this, event: &MouseDownEvent, _, cx| {
                                        if !this.font_trigger_bounds[which]
                                            .get()
                                            .contains(&event.position)
                                        {
                                            this.font_dropdown = None;
                                            cx.notify();
                                        }
                                    },
                                ))
                                .child(
                                    col()
                                        .gap_2()
                                        .child(
                                            div()
                                                .p_2()
                                                .bg(rgb(theme::input()))
                                                .child(self.font_query.clone()),
                                        )
                                        .child(
                                            div()
                                                .h(px((count.max(1) as f32 * 38.).min(230.)))
                                                .child(
                                                    self.font_list_scroll
                                                        .wrap("interface-font-options", options),
                                                ),
                                        ),
                                ),
                        ),
                )
                .with_priority(2),
            );
        }
        col()
            .gap_2()
            .child(text(font_label(which), 13., theme::foreground()))
            .child(picker)
            .child(text(
                format!(
                    "{} is built in and is used if your selected font is unavailable.",
                    if which == 0 {
                        fonts::DEFAULT
                    } else {
                        fonts::CODE_DEFAULT
                    }
                ),
                12.,
                theme::muted_foreground(),
            ))
    }

    fn appearance_settings(&self, cx: &Context<Self>) -> Div {
        let trigger_bounds = self.theme_trigger_bounds.clone();
        let mut modes = col().relative().child(
            row()
                .id("theme-dropdown")
                .relative()
                .focusable()
                .tab_stop(true)
                .cursor_pointer()
                .px_4()
                .py_2()
                .gap_3()
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(theme::border()))
                .bg(rgb(theme::secondary()))
                .focus(|s| s.border_color(rgb(theme::ring())))
                .child(text(
                    theme::active_theme().name,
                    13.,
                    theme::secondary_foreground(),
                ))
                .child(icon("chevron"))
                .child(
                    canvas(
                        move |bounds, _, _| trigger_bounds.set(bounds),
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.font_dropdown = None;
                    this.theme_dropdown = !this.theme_dropdown;
                    if this.theme_dropdown {
                        match theme::discover() {
                            Ok((choices, errors)) => {
                                this.theme_choices = choices;
                                this.theme_status = (!errors.is_empty()).then(|| errors.join("\n"));
                            }
                            Err(error) => this.theme_status = Some(error),
                        }
                    }
                    cx.notify();
                })),
        );
        if self.theme_dropdown {
            let mut choices = col().gap_1();
            for (i, choice) in self.theme_choices.iter().enumerate() {
                let file = choice.file.clone();
                choices = choices.child(
                    row()
                        .id(("theme-choice", i))
                        .focusable()
                        .tab_stop(true)
                        .cursor_pointer()
                        .px_4()
                        .py_2()
                        .rounded(px(3.))
                        .bg(rgb(theme::sidebar()))
                        .hover(|s| s.bg(rgb(theme::secondary())))
                        .focus(|s| s.border_1().border_color(rgb(theme::ring())))
                        .child(text(choice.name.clone(), 13., theme::sidebar_foreground()))
                        .child(
                            div()
                                .ml_3()
                                .px_2()
                                .py(px(2.))
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(theme::border()))
                                .bg(rgb(theme::muted()))
                                .child(text(
                                    choice.brightness.label(),
                                    11.,
                                    theme::muted_foreground(),
                                )),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            match theme::select(&file, cx) {
                                Ok(()) => {
                                    this.theme_dropdown = false;
                                    this.theme_status = Some("Theme selected and saved.".into());
                                }
                                Err(error) => this.theme_status = Some(error),
                            }
                            cx.notify();
                        })),
                );
            }
            if self.theme_choices.is_empty() {
                choices = choices.child(text(
                    "No valid themes found in ~/.config/adeline/themes.",
                    13.,
                    theme::muted_foreground(),
                ));
            }
            modes = modes.child(
                deferred(
                    menu_surface()
                        .id("theme-popup")
                        .absolute()
                        .top_full()
                        .mt_1()
                        .left_0()
                        .w_full()
                        .h(px(
                            (self.theme_choices.len().max(1) as f32 * 42. + 10.).min(230.)
                        ))
                        .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            if !this.theme_trigger_bounds.get().contains(&event.position) {
                                this.theme_dropdown = false;
                                cx.notify();
                            }
                        }))
                        .child(self.theme_list_scroll.wrap("theme-options", choices)),
                )
                .with_priority(1),
            );
        }
        col()
            .w_full()
            .gap_3()
            .mt_4()
            .child(text("Appearance", 18., theme::foreground()))
            .child(
                col()
                    .gap_2()
                    .child(text("Theme", 13., theme::foreground()))
                    .child(modes),
            )
            .when_some(self.theme_status.clone(), |d, status| {
                d.child(text(status, 12., theme::foreground()))
            })
            .child(text(
                "Themes are loaded from ~/.config/adeline/themes.",
                12.,
                theme::muted_foreground(),
            ))
            .child(self.font_picker(0, cx))
            .child(self.font_size_picker(0, cx))
            .child(self.font_picker(1, cx))
            .child(self.font_size_picker(1, cx))
    }
    fn select_mode(&mut self, mode: Section, cx: &mut Context<Self>) {
        self.group = 1;
        self.subgroup = Some(mode as usize);
        self.expanded[1] = true;
        self.query.update(cx, |query, cx| query.set("", cx));
        self.scroll.handle.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn new(
        owner: WindowHandle<Adeline>,
        entity: &Entity<Adeline>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| TextInput::search("Search settings...", cx));
        let focus = cx.focus_handle();
        fonts::refresh(cx);
        let font_query = cx.new(|cx| TextInput::search("Search fonts...", cx));
        let font_size_inputs = std::array::from_fn(|which| {
            cx.new(|cx| {
                let mut input = TextInput::new("14", cx);
                input.set(selected_font_size(which).to_string(), cx);
                input
            })
        });
        let retry_limit_input = cx.new(|cx| {
            let mut input = TextInput::new("5", cx);
            input.set(config::current().modes.chats.retry_limit.to_string(), cx);
            input
        });
        window.focus(&focus, cx);
        let mut subscriptions = Vec::new();
        for (which, size_input) in font_size_inputs.iter().enumerate() {
            subscriptions.push(cx.subscribe(
                size_input,
                move |this, input, _: &input::ContentChanged, cx| {
                    if let Ok(size) = input.read(cx).content.trim().parse::<u16>() {
                        this.save_font_size(which, size, cx);
                    } else {
                        this.font_size_errors[which] =
                            Some("Enter a whole number from 10 to 24.".into());
                        cx.notify();
                    }
                },
            ));
        }
        subscriptions.push(cx.subscribe(
            &retry_limit_input,
            |this, input, _: &input::ContentChanged, cx| {
                let value = input.read(cx).content.trim().parse::<usize>();
                this.retry_limit_error = match value {
                    Ok(limit) => {
                        config::update(|settings| settings.modes.chats.retry_limit = limit).err()
                    }
                    Err(_) => {
                        Some("Enter a non-negative whole number. Zero disables retries.".into())
                    }
                };
                cx.notify();
            },
        ));
        subscriptions.extend([
            cx.subscribe_in(
                &query,
                window,
                |this, input, _: &input::ContentChanged, window, cx| {
                    let requested = input.read(cx).content.to_string();
                    if requested == this.last_query {
                        return;
                    }
                    if this.agent_form.as_ref().is_some_and(|form| form.dirty(cx)) {
                        input.update(cx, |input, cx| input.set(this.last_query.clone(), cx));
                        this.leave(AfterAgent::Search(requested), window, cx);
                        return;
                    }
                    this.last_query = requested;
                    this.search_page = None;
                    this.nav_scroll.handle.set_offset(point(px(0.), px(0.)));
                    this.agent_page = None;
                    this.agent_form = None;
                    this.agent_subscriptions.clear();
                    this.scroll.handle.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                },
            ),
            cx.subscribe(&font_query, |this, _, _: &input::ContentChanged, cx| {
                this.font_list_scroll
                    .handle
                    .set_offset(point(px(0.), px(0.)));
                cx.notify();
            }),
            cx.observe(entity, |this, _, cx| this.sync_agent(cx)),
            cx.observe_window_bounds(window, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |_, _, cx| cx.notify()),
        ]);
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
        Self {
            owner,
            query,
            font_query,
            font_size_inputs,
            retry_limit_input,
            retry_limit_error: None,
            font_size_errors: [None, None],
            font_dropdown: None,
            font_trigger_bounds: Default::default(),
            font_choices: fonts::families(),
            font_list_scroll: Default::default(),
            theme_status: theme::load_error(),
            theme_dropdown: false,
            theme_trigger_bounds: Default::default(),
            theme_choices: Vec::new(),
            theme_list_scroll: Default::default(),
            group: 0,
            subgroup: Some(0),
            search_page: None,
            expanded: [true, false, false, true],
            agent_page: None,
            agent_form: None,
            agent_subscriptions: Vec::new(),
            agent_status: None,
            last_query: String::new(),
            pending: false,
            nav_scroll: Default::default(),
            scroll: Default::default(),
            focus,
            _subscriptions: subscriptions,
        }
    }
    fn subgroup_matches(&self, group: usize, child: usize, query: &str, cx: &App) -> bool {
        if query.trim().is_empty() {
            return true;
        }
        match group {
            0 => general_matches(child, query),
            1 => {
                (child == Section::Chats as usize
                    && matches_query(
                        query,
                        &[
                            "Modes",
                            "Chats",
                            "Automatic retry limit",
                            "Additional attempts after a temporary failure. Zero disables automatic retries.",
                        ],
                    ))
                    || self.owner.read(cx).is_ok_and(|app| {
                        app.mode_options(MODES[child].0)
                            .iter()
                            .any(|o| matches_query(query, &["Modes", MODES[child].1, o.0, o.1]))
                    })
            }
            _ => false,
        }
    }
    fn setting_row(&self, id: usize, option: SettingOption, cx: &Context<Self>) -> Stateful<Div> {
        let (label, description, checked, action) = option;
        row()
            .id(("setting", id))
            .w_full()
            .py_5()
            .gap_6()
            .justify_between()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(text(label, 14., theme::foreground()))
                    .child(
                        text(description, 12., theme::muted_foreground())
                            .line_height(config::text_pixels(19.)),
                    ),
            )
            .child(
                div()
                    .id(("toggle", id))
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .w(px(34.))
                    .h(px(20.))
                    .rounded_full()
                    .flex_shrink_0()
                    .p(px(3.))
                    .bg(rgb(if checked {
                        theme::primary()
                    } else {
                        theme::border()
                    }))
                    .flex()
                    .items_center()
                    .when(checked, |d| d.justify_end())
                    .child(
                        div()
                            .size(px(14.))
                            .rounded_full()
                            .bg(rgb(theme::primary_foreground())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let _ = this
                            .owner
                            .update(cx, |app, window, cx| app.act(action.clone(), window, cx));
                        cx.notify();
                    })),
            )
    }
    fn retry_limit_row(&self) -> Div {
        col()
            .w_full()
            .py_5()
            .gap_2()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .child(text("Automatic retry limit", 14., theme::foreground()))
            .child(text(
                "Additional attempts after a temporary failure. Zero disables automatic retries.",
                12.,
                theme::muted_foreground(),
            ))
            .child(
                div()
                    .w(px(90.))
                    .p_2()
                    .bg(rgb(theme::input()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .rounded(px(3.))
                    .child(self.retry_limit_input.clone()),
            )
            .when_some(self.retry_limit_error.clone(), |d, error| {
                d.child(text(error, 12., theme::destructive()))
            })
    }
}
fn general_matches(child: usize, query: &str) -> bool {
    match child {
        0 => {
            matches_query(query, &MACHINE_SELECTOR)
                || MODES.iter().any(|(_, name)| {
                    matches_query(
                        query,
                        &[
                            "General",
                            "Features",
                            name,
                            "Enable mode in the main view",
                            "Chats is always enabled and cannot be disabled",
                        ],
                    )
                })
        }
        1 => THEME
            .iter()
            .any(|(label, value)| matches_query(query, &["General", "Appearance", label, value])),
        2 => KEYMAP
            .iter()
            .any(|(label, value)| matches_query(query, &["General", "Keymap", label, value])),
        _ => false,
    }
}
fn show_licenses(group: usize, searching: bool) -> bool {
    group == 2 && !searching
}
fn matches_query(query: &str, parts: &[&str]) -> bool {
    query
        .split_whitespace()
        .all(|word| parts.iter().any(|part| part.to_lowercase().contains(word)))
}
impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(not(target_os = "windows"))]
        let _ = window;
        let query = self.query.read(cx).content.to_lowercase();
        let searching = !query.trim().is_empty();
        let mut sidebar = col()
            .w(px(238.))
            .flex_shrink_0()
            .h_full()
            .p_3()
            .gap_2()
            .bg(rgb(theme::sidebar()))
            .border_r_1()
            .border_color(rgb(theme::border()))
            .child(
                row()
                    .gap_2()
                    .px_2()
                    .h(px(34.))
                    .mb_2()
                    .rounded(px(5.))
                    .bg(rgb(theme::input()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .child(icon("search"))
                    .child(div().flex_1().min_w_0().child(self.query.clone())),
            );
        let mut navigation = col().gap_1();
        for (i, name) in GROUPS.into_iter().enumerate() {
            // License names and text are never part of settings search.
            let matching: Vec<_> = SUBGROUPS[i]
                .iter()
                .enumerate()
                .filter(|(j, _)| self.subgroup_matches(i, *j, &query, cx))
                .collect();
            let matching_agents: Vec<_> = if i == 3 {
                self.owner.read(cx).ok().map_or_else(Vec::new, |app| {
                    app.agent_catalog
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| {
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
                        .map(|(j, entry)| (j, entry.id.clone(), entry.definition.name.clone()))
                        .collect()
                })
            } else {
                Vec::new()
            };
            if matching.is_empty()
                && (i != 3
                    || (searching
                        && matching_agents.is_empty()
                        && !matches_query(&query, &["Agents"])))
            {
                continue;
            }
            navigation = navigation.child(
                row()
                    .id(("group", i))
                    .focusable()
                    .tab_stop(true)
                    .h(px(32.))
                    .px_2()
                    .gap_2()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(theme::sidebar())))
                    .focus(|s| s.border_1().border_color(rgb(theme::sidebar_ring())))
                    .when(
                        !searching && self.group == i && self.subgroup.is_none(),
                        |d| d.bg(rgb(theme::sidebar())),
                    )
                    .child(icon("chevron").size(px(12.)).when(
                        !self.expanded[i] && !searching,
                        |d| {
                            d.with_transformation(Transformation::rotate(radians(
                                -std::f32::consts::FRAC_PI_2,
                            )))
                        },
                    ))
                    .child(text(name, 13., theme::foreground()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.leave(AfterAgent::Group(i), window, cx);
                    })),
            );
            if self.expanded[i] || searching {
                let mut children = col()
                    .ml(px(13.))
                    .pl_2()
                    .border_l_1()
                    .border_color(rgb(theme::border()));
                for (j, child) in matching {
                    children = children.child(
                        row()
                            .id(("subgroup", i * 10 + j))
                            .focusable()
                            .tab_stop(true)
                            .h(px(30.))
                            .px_2()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(theme::sidebar())))
                            .focus(|s| s.border_1().border_color(rgb(theme::sidebar_ring())))
                            .when(
                                self.group == i
                                    && self.subgroup == Some(j)
                                    && (!searching || self.search_page.is_some()),
                                |d| d.bg(rgb(theme::sidebar())),
                            )
                            .child(text(*child, 12., theme::muted_foreground()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.leave(AfterAgent::Child(i, j), window, cx);
                            })),
                    );
                }
                if i == 3 {
                    for (j, id, name) in matching_agents {
                        let selected =
                            self.agent_page.as_deref() == Some(id.as_str()) && self.group == 3;
                        children = children.child(
                            row()
                                .id(("settings-agent", j))
                                .focusable()
                                .tab_stop(true)
                                .cursor_pointer()
                                .h(px(30.))
                                .px_2()
                                .rounded(px(4.))
                                .hover(|s| s.bg(rgb(theme::sidebar())))
                                .focus(|s| s.border_1().border_color(rgb(theme::sidebar_ring())))
                                .when(selected, |d| d.bg(rgb(theme::sidebar())))
                                .child(text(name, 12., theme::muted_foreground()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if this.agent_page.as_deref() != Some(id.as_str()) {
                                        this.leave(AfterAgent::Agent(id.clone()), window, cx);
                                    }
                                })),
                        );
                    }
                }
                navigation = navigation.child(children);
            }
        }
        sidebar = sidebar
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.nav_scroll.wrap("settings-navigation", navigation)),
            )
            .child(
                text("Adeline preferences", 11., theme::muted_foreground())
                    .w_full()
                    .text_center()
                    .pt_3()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .whitespace_nowrap(),
            );
        let mut content = col().p_8().gap_3().child(text(
            if searching {
                "Search results".to_string()
            } else if self.group == 3 && self.agent_form.is_some() {
                self.agent_form.as_ref().unwrap().inputs[0]
                    .read(cx)
                    .content
                    .to_string()
            } else {
                self.subgroup
                    .map_or(GROUPS[self.group], |i| SUBGROUPS[self.group][i])
                    .to_string()
            },
            22.,
            theme::foreground(),
        ));
        let show_page = |group, child| {
            if searching {
                self.search_page
                    .is_none_or(|(g, c)| g == group && c.is_none_or(|c| c == child))
            } else {
                self.group == group && self.subgroup.is_none_or(|c| c == child)
            }
        };
        let mut found = false;
        if let Ok(app) = self.owner.read(cx) {
            if show_page(0, 0) && general_matches(0, &query) {
                found = true;
                if searching || self.subgroup.is_none() {
                    content = content.child(text("Features", 16., theme::foreground()).mt_4());
                }
                content = content.child(text(
                        "Choose which features are available in the main view. Chats is always enabled and cannot be disabled.",
                        12., theme::muted_foreground(),
                    ));
                let features = config::current().general.features;
                if matches_query(&query, &MACHINE_SELECTOR) {
                    content = content.child(self.setting_row(
                        0,
                        (
                            MACHINE_SELECTOR[2],
                            MACHINE_SELECTOR[3],
                            features.machine_selector,
                            Action::ToggleMachineSelector,
                        ),
                        cx,
                    ));
                }
                for (index, (section, name)) in MODES.into_iter().enumerate().skip(1) {
                    if matches_query(
                        &query,
                        &["General", "Features", name, "Enable mode in the main view"],
                    ) {
                        content = content.child(self.setting_row(
                            index,
                            (
                                name,
                                "Enable this mode in the main view.",
                                features.enabled(section),
                                Action::ToggleMode(section),
                            ),
                            cx,
                        ));
                    }
                }
            }
            if show_page(0, 1) && general_matches(1, &query) {
                found = true;
                content = content.child(self.appearance_settings(cx));
            }
            let keymap = keymap_rows();
            for (index, name, rows) in [(2, "Keymap", keymap.as_slice())] {
                if !show_page(0, index) {
                    continue;
                }
                let rows: Vec<_> = rows
                    .iter()
                    .filter(|(label, value)| {
                        matches_query(&query, &["General", name, label, value])
                    })
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                found = true;
                if searching || self.subgroup.is_none() {
                    content = content.child(text(name, 16., theme::foreground()).mt_4());
                }
                content = content.child(text(
                    "Edit shortcuts in settings.yml, then restart Adeline. Enter and Space activation is built into controls.",
                    12.,
                    theme::muted_foreground(),
                ));
                for (label, value) in rows {
                    content = content.child(
                        row()
                            .py_5()
                            .gap_6()
                            .justify_between()
                            .border_b_1()
                            .border_color(rgb(theme::border()))
                            .child(text(*label, 14., theme::foreground()))
                            .child(text(value.clone(), 12., theme::muted_foreground())),
                    );
                }
            }
            if self.group == 1 || searching {
                for (index, (section, name)) in MODES.into_iter().enumerate() {
                    if !show_page(1, index) {
                        continue;
                    }
                    let options: Vec<_> = app
                        .mode_options(section)
                        .into_iter()
                        .enumerate()
                        .filter(|(_, o)| matches_query(&query, &["Modes", name, o.0, o.1]))
                        .collect();
                    if !options.is_empty() {
                        found = true;
                        if searching || self.subgroup.is_none() {
                            content =
                                content.child(text(name, 16., theme::foreground()).mt_5().pb_2());
                        }
                        for (i, option) in options {
                            content =
                                content.child(self.setting_row(10 + index * 10 + i, option, cx));
                        }
                    }
                    if section == Section::Chats
                        && matches_query(
                            &query,
                            &[
                                "Modes",
                                "Chats",
                                "Automatic retry limit",
                                "Additional attempts after a temporary failure. Zero disables automatic retries.",
                            ],
                        )
                    {
                        found = true;
                        content = content.child(self.retry_limit_row());
                    }
                }
            }
        }
        if show_licenses(self.group, searching) {
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
                    "Native interface toolkit and adapted text input · Apache-2.0 · Zed Industries",
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
                if self.subgroup.is_some_and(|i| i != index) {
                    continue;
                }
                found = true;
                content = content.child(
                    col()
                        .mt_5()
                        .gap_3()
                        .child(text(name, 17., theme::foreground()))
                        .child(text(description, 12., theme::muted_foreground()))
                        .child(
                            text(license, 12., theme::foreground())
                                .line_height(config::text_pixels(20.))
                                .p_4()
                                .bg(rgb(theme::muted()))
                                .rounded(px(3.)),
                        ),
                );
            }
        }
        if self.group == 3 || searching {
            let matching_agent = self.owner.read(cx).is_ok_and(|app| {
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
            });
            if !searching || matching_agent || matches_query(&query, &["Agents"]) {
                found = true;
                if let Some(form) = &self.agent_form {
                    if self.group == 3 && (!searching || self.search_page.is_some()) {
                        content = content.child(form.fields(cx));
                        if form.external_changed {
                            content = content.child(text(
                                "This agent changed outside Adeline. Your edits are kept. Save to choose Reload or Overwrite.",
                                12., theme::destructive()
                            ));
                        }
                        if let Some(error) = &form.status {
                            content = content.child(text(error.clone(), 12., theme::destructive()));
                        }
                        content = content.child(
                            row()
                                .w_full()
                                .justify_between()
                                .mt_5()
                                .child(
                                    row()
                                        .id("settings-delete-agent")
                                        .focusable()
                                        .tab_stop(true)
                                        .cursor_pointer()
                                        .px_4()
                                        .py_2()
                                        .rounded(px(5.))
                                        .bg(rgb(theme::secondary()))
                                        .focus(|s| s.border_1().border_color(rgb(theme::ring())))
                                        .child(text("Delete agent", 13., theme::destructive()))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if let Some(id) = this.agent_page.clone() {
                                                this.confirm_delete(id, window, cx);
                                            }
                                        })),
                                )
                                .child(
                                    row()
                                        .id("settings-save-agent")
                                        .focusable()
                                        .tab_stop(true)
                                        .cursor_pointer()
                                        .px_5()
                                        .py_2()
                                        .rounded(px(5.))
                                        .bg(rgb(theme::primary()))
                                        .focus(|s| s.border_1().border_color(rgb(theme::ring())))
                                        .child(text("Save", 13., theme::primary_foreground()))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.save_agent(None, false, window, cx);
                                        })),
                                ),
                        );
                    }
                } else {
                    content = content.child(text(
                        "Select an agent in the sidebar to edit its settings.",
                        13.,
                        theme::muted_foreground(),
                    ));
                }
                if let Some(error) = &self.agent_status {
                    content = content.child(text(error.clone(), 12., theme::destructive()));
                }
                content = content.child(diagnostics(agent_diagnostics(self.owner, cx)));
            }
        }
        if !found {
            content = content.child(
                text(
                    "No settings found. Try another search.",
                    13.,
                    theme::muted_foreground(),
                )
                .mt_4(),
            );
        }
        if !searching
            && (self.group == 1 || (self.group == 0 && self.subgroup.is_none_or(|i| i == 0)))
        {
            content = content.child(
                text(
                    "Changes apply immediately and are saved to settings.yml.",
                    12.,
                    theme::muted_foreground(),
                )
                .mt_5(),
            );
        }
        let shell = col()
            .size_full()
            .font_family(config::font())
            .bg(rgb(theme::background()))
            .text_color(rgb(theme::foreground()));
        #[cfg(target_os = "windows")]
        let shell = shell.child(titlebar::render("Settings · Adeline".into(), window));
        // GPUI's focus handler prevents the default mouse-down behavior. Keep
        // it below the native caption so Windows can handle dragging and buttons.
        let content = row()
            .flex_1()
            .min_h_0()
            .items_start()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                if this.font_dropdown.is_some() {
                    this.font_dropdown = None;
                    window.focus(&this.focus, cx);
                    cx.notify();
                } else if this.theme_dropdown {
                    this.theme_dropdown = false;
                    cx.notify();
                } else {
                    this.leave(AfterAgent::CloseSettings, window, cx);
                }
            }))
            .on_action(cx.listener(|_, _: &NextFocus, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &PreviousFocus, window, cx| window.focus_prev(cx)))
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.scroll.wrap("settings-content", content)),
            );
        shell.child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::{general_matches, matches_query, show_licenses};
    #[test]
    fn navigation_matches_setting_descriptions_and_omits_unrelated_subgroups() {
        assert!(general_matches(0, "whiteboard"));
        assert!(general_matches(0, "machine selector"));
        assert!(general_matches(0, "top bar"));
        assert!(!general_matches(1, "model"));
        assert!(!general_matches(2, "model"));
        assert!(general_matches(2, "focus search"));
        assert!(!general_matches(0, "phosphor"));
    }
    #[test]
    fn licenses_are_excluded_even_when_search_starts_on_licenses() {
        for group in 0..3 {
            assert!(!show_licenses(group, true));
        }
        assert!(show_licenses(2, false));
    }
    #[test]
    fn search_matches_words_across_group_label_and_description() {
        assert!(matches_query(
            "docs raw",
            &["Modes", "Docs", "Show raw Markdown"]
        ));
        assert!(!matches_query(
            "docs wrap",
            &["Modes", "Services", "Wrap output lines"]
        ));
        assert!(matches_query("  ", &["General"]));
    }
}
