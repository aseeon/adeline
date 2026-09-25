use super::*;

const GROUPS: [&str; 3] = ["General", "Modes", "Licenses"];
const SUBGROUPS: [&[&str]; 3] = [
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
    &["Phosphor Icons", "Lobe Icons · Mono", "GPUI"],
];
const THEME: [(&str, &str); 2] = [
    ("Appearance", "lightos colors customize reset"),
    ("Interface font", "System font"),
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

impl Adeline {
    pub(super) fn mode_options(&self, section: Section) -> Vec<SettingOption> {
        let mut options = match section {
            Section::Groupchats | Section::Issues | Section::Whiteboard => vec![],
            Section::Chats => vec![(
                "Show idle chats",
                "Include completed conversations in the chat list.",
                self.show_completed,
                Action::ShowCompleted,
            )],
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
pub(super) fn close_for(owner: WindowHandle<Adeline>, cx: &mut App) {
    for handle in cx.windows() {
        if let Some(settings) = handle.downcast::<SettingsWindow>() {
            let _ = settings.update(cx, |settings, window, _| {
                if settings.owner.window_id() == owner.window_id() {
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
                    settings.select_mode(mode, cx);
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

struct SettingsWindow {
    owner: WindowHandle<Adeline>,
    query: Entity<TextInput>,
    color_inputs: Vec<Entity<TextInput>>,
    font_input: Entity<TextInput>,
    theme_status: Option<String>,
    theme_dropdown: bool,
    theme_trigger_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    theme_choices: Vec<theme::ThemeChoice>,
    theme_list_scroll: scrollbar::PanelScroll,
    group: usize,
    subgroup: Option<usize>,
    search_page: Option<(usize, Option<usize>)>,
    expanded: [bool; 3],
    nav_scroll: scrollbar::PanelScroll,
    scroll: scrollbar::PanelScroll,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}
impl SettingsWindow {
    fn refresh_colors(&mut self, cx: &mut Context<Self>) {
        let colors = theme::current_colors();
        for (i, &(key, _, _)) in theme::ROLES.iter().enumerate() {
            self.color_inputs[i].update(cx, |input, cx| {
                input.set(format!("#{:06X}", colors[key]), cx);
            });
        }
    }
    fn change_theme(&mut self, reload: bool, cx: &mut Context<Self>) {
        let result = if reload {
            theme::select(&theme::active_file(), cx)
        } else {
            let mut selected = theme::active_theme();
            for (i, &(key, _, _)) in theme::ROLES.iter().enumerate() {
                selected.colors.insert(
                    key.into(),
                    self.color_inputs[i].read(cx).content.to_string(),
                );
            }
            theme::apply_colors(selected, cx)
        };
        match result {
            Ok(()) => {
                self.refresh_colors(cx);
                self.theme_status = Some(
                    if reload {
                        "Saved colors reloaded."
                    } else {
                        "Theme file saved. Changes apply to all windows."
                    }
                    .into(),
                );
            }
            Err(error) => self.theme_status = Some(error),
        }
        cx.notify();
    }
    fn theme_editor(&self, cx: &Context<Self>) -> Div {
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
                        .child(text(
                            format!("{} · {}", choice.name, choice.file),
                            13.,
                            theme::sidebar_foreground(),
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            match theme::select(&file, cx) {
                                Ok(()) => {
                                    this.refresh_colors(cx);
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
        let mut editor = col().w_full().gap_3().mt_4()
            .child(text("Appearance", 18., theme::foreground()))
            .child(modes).child(row().child(badge("Accent preview", theme::accent(), theme::accent_foreground())))
            .child(text("Edit colors below, then Apply colors. Apply saves to the selected YAML file. Selecting another theme discards unapplied edits.", 12., theme::muted_foreground()))
            .child(row().gap_3()
                .child(row().id("apply-theme").focusable().tab_stop(true).cursor_pointer().px_4().py_2().rounded(px(8.))
                    .bg(rgb(theme::primary())).border_1().border_color(rgb(theme::primary()))
                    .focus(|s| s.border_color(rgb(theme::foreground())))
                    .child(text("Apply colors", 13., theme::primary_foreground()))
                    .on_click(cx.listener(|this, _, _, cx| this.change_theme(false, cx))))
                .child(row().id("reset-theme").focusable().tab_stop(true).cursor_pointer().px_4().py_2().rounded(px(8.))
                    .bg(rgb(theme::secondary())).border_1().border_color(rgb(theme::border()))
                    .focus(|s| s.border_color(rgb(theme::ring())))
                    .child(text("Reload saved colors", 13., theme::secondary_foreground()))
                    .on_click(cx.listener(|this, _, _, cx| this.change_theme(true, cx)))))
            .when_some(self.theme_status.clone(), |d, status| d.child(text(status, 12., theme::foreground())))
            .child(text("Themes are loaded from ~/.config/adeline/themes.", 12., theme::muted_foreground()));
        editor = editor
            .child(
                row()
                    .gap_3()
                    .child(text("Interface font", 13., theme::foreground()))
                    .child(
                        div()
                            .w(px(180.))
                            .p_2()
                            .bg(rgb(theme::input()))
                            .rounded(px(3.))
                            .child(self.font_input.clone()),
                    )
                    .child(
                        row()
                            .id("save-interface-font")
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .px_3()
                            .py_2()
                            .bg(rgb(theme::secondary()))
                            .rounded(px(3.))
                            .focus(|s| s.border_1().border_color(rgb(theme::ring())))
                            .child(text("Save font", 13., theme::secondary_foreground()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let font = this.font_input.read(cx).content.trim().to_string();
                                if font.is_empty() {
                                    this.theme_status =
                                        Some("Enter System or a font family name.".into());
                                } else {
                                    match config::update(|s| {
                                        s.general.appearance.interface_font = font;
                                    }) {
                                        Ok(()) => {
                                            this.theme_status =
                                                Some("Interface font saved.".into());
                                            cx.refresh_windows();
                                        }
                                        Err(error) => this.theme_status = Some(error),
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(text(
                "Use System for the default font.",
                12.,
                theme::muted_foreground(),
            ));
        for (i, &(_, label, _)) in theme::ROLES.iter().enumerate() {
            let value = self.color_inputs[i].read(cx).content.as_ref();
            let preview = theme::parse_hex(value);
            editor = editor.child(
                row()
                    .w_full()
                    .gap_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(theme::border()))
                    .child(
                        div()
                            .size(px(24.))
                            .flex_shrink_0()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .bg(rgb(preview.unwrap_or(theme::background()))),
                    )
                    .child(text(label, 13., theme::foreground()).flex_1())
                    .child(
                        div()
                            .w(px(125.))
                            .flex_shrink_0()
                            .bg(rgb(theme::card()))
                            .text_color(rgb(theme::card_foreground()))
                            .border_1()
                            .border_color(rgb(if preview.is_some() {
                                theme::input()
                            } else {
                                theme::destructive()
                            }))
                            .rounded(px(3.))
                            .child(self.color_inputs[i].clone()),
                    ),
            );
        }
        editor
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
        let font_input = cx.new(|cx| {
            let mut input = TextInput::new("System", cx);
            input.set(config::current().general.appearance.interface_font, cx);
            input
        });
        window.focus(&focus);
        let mut subscriptions = vec![
            cx.subscribe(&query, |this, _, _: &input::ContentChanged, cx| {
                this.search_page = None;
                this.nav_scroll.handle.set_offset(point(px(0.), px(0.)));
                this.scroll.handle.set_offset(point(px(0.), px(0.)));
                cx.notify();
            }),
            cx.observe(entity, |_, _, cx| cx.notify()),
            cx.observe_window_bounds(window, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |_, _, cx| cx.notify()),
        ];
        let colors = theme::current_colors();
        let color_inputs = theme::ROLES
            .iter()
            .map(|&(key, _, _)| {
                let input = cx.new(|cx| {
                    let mut input = TextInput::new("#RRGGBB", cx);
                    input.set(format!("#{:06X}", colors[key]), cx);
                    input
                });
                subscriptions
                    .push(cx.subscribe(&input, |_, _, _: &input::ContentChanged, cx| cx.notify()));
                input
            })
            .collect();
        Self {
            owner,
            query,
            color_inputs,
            font_input,
            theme_status: theme::load_error(),
            theme_dropdown: false,
            theme_trigger_bounds: Default::default(),
            theme_choices: Vec::new(),
            theme_list_scroll: Default::default(),
            group: 0,
            subgroup: Some(0),
            search_page: None,
            expanded: [true, false, false],
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
            1 => self.owner.read(cx).is_ok_and(|app| {
                app.mode_options(MODES[child].0)
                    .iter()
                    .any(|o| matches_query(query, &["Modes", MODES[child].1, o.0, o.1]))
            }),
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
                    .child(text(description, 12., theme::muted_foreground()).line_height(px(19.))),
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
}
fn general_matches(child: usize, query: &str) -> bool {
    match child {
        0 => MODES.iter().any(|(_, name)| {
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
        }),
        1 => {
            THEME.iter().any(|(label, value)| {
                matches_query(query, &["General", "Appearance", label, value])
            }) || theme::ROLES.iter().any(|(_, label, _)| {
                matches_query(query, &["General", "Appearance", "color", label])
            })
        }
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
            if matching.is_empty() {
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.expanded[i] = !this.expanded[i];
                        this.group = i;
                        this.subgroup = None;
                        this.search_page = Some((i, None));
                        this.scroll.handle.set_offset(point(px(0.), px(0.)));
                        cx.notify();
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.group = i;
                                this.subgroup = Some(j);
                                this.search_page = Some((i, Some(j)));
                                this.scroll.handle.set_offset(point(px(0.), px(0.)));
                                cx.notify();
                            })),
                    );
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
                "Search results"
            } else {
                self.subgroup
                    .map_or(GROUPS[self.group], |i| SUBGROUPS[self.group][i])
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
                        "Choose which modes are available in the main view. Chats is always enabled and cannot be disabled.",
                        12., theme::muted_foreground(),
                    ));
                let features = config::current().general.features;
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
                content = content.child(self.theme_editor(cx));
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
                    include_str!("../LICENSE"),
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
                                .line_height(px(20.))
                                .p_4()
                                .bg(rgb(theme::muted()))
                                .rounded(px(3.)),
                        ),
                );
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
                if this.theme_dropdown {
                    this.theme_dropdown = false;
                    cx.notify();
                } else {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|_, _: &NextFocus, window, _| window.focus_next()))
            .on_action(cx.listener(|_, _: &PreviousFocus, window, _| window.focus_prev()))
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
