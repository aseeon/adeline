use super::*;

impl Adeline {
    pub(super) fn chats(&self, cx: &Context<Self>) -> AnyElement {
        let threads = &self.workspace().threads;
        let mut main = col().flex_1().min_w_0().h_full();
        let title = self
            .selected
            .map_or("New chat".into(), |i| threads[i].title.clone());
        main = main.child(
            row()
                .h(config::text_pixels(48.))
                .flex_shrink_0()
                .px_5()
                .gap_3()
                .border_b_1()
                .border_color(rgb(theme::border()))
                .child(
                    row()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .child(
                            text(title, 14., theme::foreground())
                                .min_w_0()
                                .line_height(config::text_pixels(24.))
                                .truncate(),
                        )
                        .when_some(self.selected, |d, i| {
                            let status = match threads[i].status.as_str() {
                                "blocked" => "Attention",
                                "working" => "Active",
                                _ => "Idle",
                            };
                            d.child(
                                badge(status, theme::card(), theme::card_foreground())
                                    .flex_shrink_0(),
                            )
                        }),
                )
                .when(self.selected.is_some(), |d| {
                    d.child(
                        row()
                            .flex_shrink_0()
                            .p(px(3.))
                            .gap(px(2.))
                            .rounded(px(13.))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .bg(rgb(theme::muted()))
                            .when(self.selected.is_some(), |d| {
                                d.child(self.control_button(
                                    "complete",
                                    "check",
                                    Action::Complete,
                                    cx,
                                ))
                                .child(self.control_button(
                                    "thread-menu",
                                    "more",
                                    Action::ChatMenu,
                                    cx,
                                ))
                            }),
                    )
                }),
        );
        main = main.child(div().flex_1().min_h_0().w_full().child(
            AnyView::from(self.transcript.clone()).cached(StyleRefinement::default().size_full()),
        ));
        main = main.child(self.composer_region.clone());
        main.into_any_element()
    }
    fn control_button(
        &self,
        id: &'static str,
        name: &str,
        action: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        self.button(id, "", action, cx)
            .size(px(34.))
            .px_0()
            .justify_center()
            .rounded(px(9.))
            .child(icon(name).size(px(19.)))
    }
    pub(super) fn activity_content(&self, cx: &Context<Self>) -> Div {
        let mut panel = col().w_full().flex_shrink_0();
        if let Some(i) = self.selected {
            let t = &self.workspace().threads[i];
            panel = panel.child(
                row()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .child(text(provider(&t.provider), 12., theme::foreground()))
                    .child(badge(
                        if t.status == "blocked" {
                            "Needs input"
                        } else if t.status == "working" {
                            "Working"
                        } else {
                            "Done"
                        },
                        theme::secondary(),
                        theme::muted_foreground(),
                    )),
            );
            let mut events = col().id("events").flex_shrink_0().p_3().gap_2().child(text(
                format!("Thread activity       {} events", t.activity.len()),
                11.,
                theme::muted_foreground(),
            ));
            for (j, e) in t.activity.iter().enumerate() {
                events = events.child(
                    col()
                        .id(("event", j))
                        .p_3()
                        .gap_2()
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .rounded_lg()
                        .cursor_pointer()
                        .on_click(cx.listener(move |s, _, w, cx| s.act(Action::Event(j), w, cx)))
                        .child(icon_label(
                            if self.expanded_event == Some(j) {
                                "chevron"
                            } else {
                                "caret-right"
                            },
                            match e.kind.as_str() {
                                "command" => "Command · Done",
                                "files" => "Files · Done",
                                _ => "Update",
                            },
                            11.,
                            theme::muted_foreground(),
                        ))
                        .child(text(e.title.clone(), 12., theme::foreground()))
                        .when(self.expanded_event == Some(j), |d| {
                            d.child(
                                text(
                                    if e.detail.is_empty() {
                                        "Completed in the sample workspace.".into()
                                    } else {
                                        e.detail.clone()
                                    },
                                    12.,
                                    theme::muted_foreground(),
                                )
                                .p_2()
                                .bg(rgb(theme::muted())),
                            )
                        }),
                );
            }
            panel = panel.child(events);
        } else {
            panel = panel.child(
                text(
                    "Start a chat to see agent activity.",
                    13.,
                    theme::muted_foreground(),
                )
                .p_5(),
            );
        }
        panel
    }
    pub(super) fn files(&self, cx: &Context<Self>) -> AnyElement {
        let frame = row().items_start().size_full();
        let mut content = col().flex_1().min_w_0().h_full();
        let mut toolbar = row().h(px(52.)).px_3().gap_2().child(
            self.button("files-home", "", Action::DocsHome, cx)
                .child(icon("house"))
                .child("Home")
                .bg(rgb(theme::secondary())),
        );
        if let Some(i) = self.document {
            toolbar = toolbar
                .child(
                    self.button(
                        "open-doc",
                        self.workspace().docs[i].title.clone(),
                        Action::Document(i),
                        cx,
                    )
                    .bg(rgb(theme::secondary())),
                )
                .child(self.ib("close-doc", "close", Action::DocsHome, cx));
        }
        toolbar = toolbar
            .child(div().flex_1())
            .when(self.document.is_none(), |d| {
                d.child(
                    self.button("archive", "", Action::Archive, cx)
                        .child(icon("archive"))
                        .child("Archived")
                        .text_color(rgb(theme::muted_foreground())),
                )
            })
            .child(self.ib("new-file", "plus", Action::NewDoc, cx));
        content = content.child(toolbar);
        if let Some(i) = self.document {
            content = content.child(self.document_view(i, cx));
        } else {
            content = content.child(
                div().flex_1().min_h_0().w_full().child(
                    AnyView::from(self.files_home_region.clone())
                        .cached(StyleRefinement::default().size_full()),
                ),
            );
        }
        frame.child(content).into_any_element()
    }
    pub(super) fn explorer_content(&self, cx: &Context<Self>) -> AnyElement {
        let mut explorer = col()
            .id("left-explorer")
            .w_full()
            .flex_shrink_0()
            .p_3()
            .gap_2()
            .child(
                self.button("explorer-home", "", Action::DocsHome, cx)
                    .child(icon("house"))
                    .child("Docs home"),
            )
            .child(
                text("YOUR DOCUMENTS", 10., theme::muted_foreground())
                    .mt_5()
                    .mb_2(),
            );
        for (i, d) in self
            .workspace()
            .docs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.search_title.contains(&self.query(cx)))
        {
            explorer = explorer.child(
                self.button(
                    ("doc-tree", i),
                    short(&d.title, 26),
                    Action::Document(i),
                    cx,
                )
                .when(self.document == Some(i), |d| d.bg(rgb(theme::secondary())))
                .child(icon("file")),
            );
        }
        explorer = explorer
            .child(text("PROJECT FILES", 10., theme::muted_foreground()).mt_5())
            .child(
                text(
                    "Project files are unavailable\nin this preview.",
                    12.,
                    theme::muted_foreground(),
                )
                .text_center()
                .mt_8(),
            );
        col()
            .size_full()
            .child(self.mode_sidebar_header(cx))
            .child(
                div().flex_1().min_h_0().child(
                    self.left_scroll[self.section as usize].wrap("explorer-scroll", explorer),
                ),
            )
            .into_any_element()
    }
    pub(super) fn files_home_view(&self, cx: &Context<Self>) -> AnyElement {
        let mut home = col().w_full().p_8().gap_6().child(
            row()
                .justify_between()
                .child(text(
                    if self.archived { "Archived" } else { "Docs" },
                    28.,
                    theme::foreground(),
                ))
                .child(
                    row()
                        .gap_4()
                        .when(!self.left_panel_is_open(), |d| {
                            d.child(div().w(px(260.)).child(self.search_box(cx)))
                        })
                        .child(
                            self.button("new-doc", "", Action::NewDoc, cx)
                                .child(icon("plus"))
                                .child("New doc")
                                .bg(rgb(theme::secondary()))
                                .shadow(vec![theme::shadow(3.)]),
                        ),
                ),
        );
        if self.archived && !self.archived_docs.iter().any(|(p, _)| *p == self.project) {
            home = home.child(
                col()
                    .items_center()
                    .mt_16()
                    .gap_3()
                    .child(icon("folder"))
                    .child(text("No archived documents", 18., theme::foreground()))
                    .child(text(
                        "Your archived documents will appear here.",
                        13.,
                        theme::muted_foreground(),
                    )),
            );
        } else {
            home = home.child(
                text(
                    format!("Your documents    {}", self.workspace().docs.len()),
                    13.,
                    theme::foreground(),
                )
                .font_weight(FontWeight::SEMIBOLD),
            );
            let mut cards = row().items_start().flex_wrap().gap_5();
            for (i, d) in self.workspace().docs.iter().enumerate().filter(|(i, d)| {
                d.search_title.contains(&self.query(cx))
                    && self.archived_docs.contains(&(self.project, *i)) == self.archived
            }) {
                let mut preview = col()
                    .w(px(190.))
                    .min_w_0()
                    .flex_shrink_0()
                    .h(px(206.))
                    .overflow_hidden()
                    .p_3()
                    .bg(rgb(theme::display_tint(
                        [
                            theme::chart_4(),
                            theme::chart_4(),
                            theme::chart_5(),
                            theme::accent(),
                        ][i % 4],
                    )))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgb(theme::border()));
                // Keep intrinsic table widths from expanding the paper. Clip
                // at the paper too, so ink never reaches the colored border.
                let mut page = col()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .p_4()
                    .bg(rgb(theme::card()))
                    .gap_1();
                for block in d
                    .prepared
                    .preview
                    .iter()
                    .filter(|_| d.revision == d.prepared_revision)
                {
                    use crate::prepared::BlockKind;
                    match &block.kind {
                        BlockKind::Table(cells) => {
                            page = page.child(
                                row().w_full().min_w_0().items_start().gap_1().children(
                                    cells.iter().map(|cell| {
                                        text(
                                            SharedString::from(cell.clone()),
                                            8.,
                                            theme::foreground(),
                                        )
                                        .flex_1()
                                        .min_w_0()
                                        .whitespace_normal()
                                        .overflow_hidden()
                                    }),
                                ),
                            );
                        }
                        BlockKind::Text { display, heading } => {
                            page = page.child(
                                text(SharedString::from(display.clone()), 8., theme::foreground())
                                    .when(*heading > 0, |d| d.font_weight(FontWeight::BOLD)),
                            );
                        }
                        _ => {}
                    }
                }
                preview = preview.child(page);
                cards = cards.child(
                    col()
                        .id(("doc-card", i))
                        .w(px(190.))
                        .min_w_0()
                        .flex_shrink_0()
                        .gap_2()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .on_click(cx.listener(move |s, _, w, cx| s.act(Action::Document(i), w, cx)))
                        .child(preview)
                        .child(
                            text(d.title.clone(), 12., theme::foreground())
                                .px_2()
                                .h(px(34.)),
                        )
                        .child(text("Sep 25", 11., theme::muted_foreground()).px_2()),
                );
            }
            home = home
                .child(cards)
                .child(
                    row()
                        .gap_4()
                        .mt_5()
                        .child(
                            text("Project files", 13., theme::foreground())
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(text(
                            self.workspace().config.name.clone(),
                            12.,
                            theme::muted_foreground(),
                        )),
                )
                .child(
                    col()
                        .items_center()
                        .mt_8()
                        .gap_3()
                        .child(text(
                            "Project files are unavailable in this preview.",
                            13.,
                            theme::muted_foreground(),
                        ))
                        .child(self.button("retry-files", "Try again", Action::DocsHome, cx)),
                );
        }
        self.main_scroll[Section::Docs as usize]
            .wrap("files-home-scroll", home)
            .into_any_element()
    }
    fn document_view(&self, i: usize, cx: &Context<Self>) -> Div {
        let d = &self.workspace().docs[i];
        let mut tools = row()
            .bg(rgb(theme::card()))
            .flex_shrink_0()
            .mx_3()
            .px_2()
            .h(px(46.))
            .gap_1()
            .border_1()
            .border_color(rgb(theme::border()))
            .rounded_lg()
            .shadow(vec![theme::shadow(3.)])
            .child(
                self.button("text-style", "Text", Action::Format("## "), cx)
                    .child(icon("chevron"))
                    .bg(rgb(theme::secondary())),
            );
        for (j, (name, mark)) in [
            ("text-b", "**"),
            ("text-italic", "*"),
            ("text-strikethrough", "~~"),
            ("list-bullets", "- "),
            ("list-numbers", "1. "),
            ("check-square", "- [ ] "),
            ("quotes", "> "),
            ("code", "`"),
            ("link", "[link] "),
        ]
        .into_iter()
        .enumerate()
        {
            tools = tools.child(
                self.button(("format", j), "", Action::Format(mark), cx)
                    .child(icon(name))
                    .px_2(),
            );
        }
        tools = tools
            .child(div().flex_1())
            .child(
                self.button("rich", "Rich text", Action::Raw, cx)
                    .when(!self.raw, |d| d.bg(rgb(theme::secondary()))),
            )
            .child(
                self.button("raw", "Raw", Action::Raw, cx)
                    .when(self.raw, |d| d.bg(rgb(theme::secondary()))),
            )
            .child(self.ib("doc-menu", "more", Action::DocMenu, cx))
            .child(
                self.ib("pin-doc", "pin", Action::PinDoc, cx)
                    .when(self.pinned, |d| d.bg(rgb(theme::sidebar_accent()))),
            );
        col()
            .flex_1()
            .min_h_0()
            .child(tools)
            .child(
                div().flex_1().min_h_0().w_full().child(
                    AnyView::from(self.document_region.clone())
                        .cached(StyleRefinement::default().size_full()),
                ),
            )
            .child(
                row()
                    .h(px(32.))
                    .px_6()
                    .justify_between()
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .child(icon_label(
                        "file",
                        format!(".adeline/docs/{}", d.filename),
                        10.,
                        theme::muted_foreground(),
                    ))
                    .child(text(
                        format!("{} words", d.prepared.words),
                        10.,
                        theme::muted_foreground(),
                    )),
            )
    }
    pub(super) fn workflows(&self, cx: &Context<Self>) -> AnyElement {
        let recipes = &self.workspace().recipes;
        let mut collections = vec!["All".to_owned(), "Scheduled".into(), "Yours".into()];
        for r in recipes {
            if !collections.contains(&r.collection) {
                collections.push(r.collection.clone());
            }
        }
        let mut tabs = row().gap_2();
        for (i, c) in collections.iter().enumerate() {
            tabs = tabs.child(
                self.button(
                    ("collection", i),
                    c.clone(),
                    Action::Collection(c.clone()),
                    cx,
                )
                .rounded_full()
                .when(self.collection == *c, |d| d.bg(rgb(theme::sidebar()))),
            );
        }
        let mut library = col()
            .w_full()
            .min_w_0()
            .p_8()
            .gap_4()
            .child(
                row()
                    .gap_3()
                    .child(
                        div()
                            .p_2()
                            .rounded_lg()
                            .bg(rgb(theme::secondary()))
                            .child(icon("workflow").size(px(28.))),
                    )
                    .child(text("Workflows", 28., theme::foreground()))
                    .child(div().flex_1())
                    .child(
                        self.button(
                            "new-workflow-button",
                            "New workflow",
                            Action::NewWorkflow,
                            cx,
                        )
                        .child(icon("plus"))
                        .h(px(46.))
                        .bg(rgb(theme::muted()))
                        .shadow(vec![theme::shadow(3.)]),
                    ),
            )
            .child(tabs)
            .when(!self.left_panel_is_open(), |d| d.child(self.search_box(cx)));
        let mut visible_count = 0;
        let mut unique_collections = Vec::new();
        for recipe in recipes {
            if !unique_collections.contains(&recipe.collection) {
                unique_collections.push(recipe.collection.clone());
            }
        }
        for collection in &unique_collections {
            let visible = recipes
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    &r.collection == collection
                        && (self.collection == "All"
                            || self.collection == *collection
                            || self.collection == "Yours" && r.id.starts_with("local-")
                            || self.collection == "Scheduled" && r.schedule_label != "On demand")
                        && format!("{} {}", r.name, r.instructions)
                            .to_lowercase()
                            .contains(&self.query(cx))
                })
                .collect::<Vec<_>>();
            if visible.is_empty() {
                continue;
            }
            library = library.child(
                col()
                    .mt_3()
                    .gap_1()
                    .child(text(collection.clone(), 18., theme::foreground()))
                    .child(text("by Adeline demo", 11., theme::muted_foreground())),
            );
            let mut cards = row().flex_wrap().items_start().gap_4();
            for (i, r) in visible {
                visible_count += 1;
                cards = cards.child(
                    col()
                        .id(("workflow-card", i))
                        .w(px(280.))
                        .h(px(242.))
                        .p_5()
                        .gap_3()
                        .rounded(px(22.))
                        .border_1()
                        .border_color(rgb(if self.workflow == Some(i) {
                            theme::primary()
                        } else {
                            theme::border()
                        }))
                        .bg(rgb(if self.workflow == Some(i) {
                            theme::sidebar_accent()
                        } else {
                            theme::muted()
                        }))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(theme::secondary())))
                        .on_click(cx.listener(move |s, _, w, cx| s.act(Action::Workflow(i), w, cx)))
                        .child(
                            div()
                                .size(px(42.))
                                .p_3()
                                .bg(rgb(if i == 2 {
                                    theme::display_tint(theme::chart_5())
                                } else {
                                    theme::secondary()
                                }))
                                .rounded(px(12.))
                                .child(icon("workflow")),
                        )
                        .child(
                            text(r.name.clone(), 16., theme::foreground())
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            text(short(&r.instructions, 136), 13., theme::muted_foreground())
                                .line_height(config::text_pixels(21.))
                                .max_h(px(64.))
                                .overflow_hidden(),
                        )
                        .child(div().flex_1())
                        .child(
                            row()
                                .justify_end()
                                .gap_5()
                                .child(
                                    icon("arrow-counter-clockwise")
                                        .size(px(18.))
                                        .text_color(rgb(theme::muted_foreground())),
                                )
                                .child(
                                    div()
                                        .rounded_full()
                                        .bg(rgb(theme::secondary()))
                                        .p_2()
                                        .child(icon("play")),
                                ),
                        ),
                );
            }
            library = library.child(cards);
        }
        if visible_count == 0 {
            library = library.child(
                col()
                    .items_center()
                    .p_16()
                    .gap_3()
                    .child(icon("workflow"))
                    .child(text(
                        if self.collection == "Scheduled" {
                            "No scheduled workflows"
                        } else {
                            "No workflows found"
                        },
                        18.,
                        theme::muted_foreground(),
                    )),
            );
        }
        library = library.child(div().flex_1()).child(
            row()
                .mt_8()
                .pt_4()
                .border_t_1()
                .border_color(rgb(theme::border()))
                .justify_between()
                .child(
                    self.button(
                        "new-collection",
                        "New collection",
                        Action::NewCollection,
                        cx,
                    )
                    .child(icon("plus"))
                    .text_color(rgb(theme::muted_foreground())),
                )
                .child(text(
                    format!("{visible_count} workflows"),
                    11.,
                    theme::muted_foreground(),
                )),
        );
        self.main_scroll[Section::Workflows as usize]
            .wrap("workflow-library", library)
            .into_any_element()
    }

    pub(super) fn workflow_details(&self, cx: &Context<Self>) -> Div {
        let Some(i) = self.workflow else {
            return col().p_5().child(text(
                "Select a workflow to see its details.",
                13.,
                theme::muted_foreground(),
            ));
        };
        let recipe = &self.workspace().recipes[i];
        col()
            .w_full()
            .min_w_0()
            .p_4()
            .gap_4()
            .child(text(
                format!("{} / by Adeline demo", recipe.collection),
                11.,
                theme::muted_foreground(),
            ))
            .child(text(recipe.name.clone(), 20., theme::sidebar_foreground()))
            .child(icon_label(
                "sparkle",
                "Any agent",
                11.,
                theme::muted_foreground(),
            ))
            .child(
                self.button(
                    "workflow-schedule",
                    recipe.schedule_label.clone(),
                    Action::Schedule,
                    cx,
                )
                .bg(rgb(theme::secondary())),
            )
            .child(
                text(
                    recipe.instructions.clone(),
                    14.,
                    theme::sidebar_foreground(),
                )
                .line_height(config::text_pixels(23.)),
            )
            .child(
                text(
                    format!(".adeline/recipes/{}.md", recipe.id),
                    10.,
                    theme::muted_foreground(),
                )
                .truncate(),
            )
            .child(
                self.button(
                    "edit-workflow",
                    if self.editing_workflow {
                        "Cancel editing"
                    } else {
                        "Edit workflow"
                    },
                    Action::EditWorkflow,
                    cx,
                )
                .child(icon(if self.editing_workflow {
                    "chevron"
                } else {
                    "caret-right"
                }))
                .bg(rgb(theme::secondary())),
            )
            .when(self.editing_workflow, |d| {
                d.child(
                    col()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .child(text("Name", 12., theme::muted_foreground()))
                        .child(
                            div()
                                .w_full()
                                .p_2()
                                .rounded_lg()
                                .bg(rgb(theme::secondary()))
                                .child(self.name_input.clone()),
                        )
                        .child(text("Instructions", 12., theme::muted_foreground()))
                        .child(
                            div()
                                .w_full()
                                .p_2()
                                .rounded_lg()
                                .bg(rgb(theme::secondary()))
                                .child(self.edit_input.clone()),
                        )
                        .child(
                            self.button(
                                "save-workflow-inline",
                                "Save changes",
                                Action::SaveWorkflow,
                                cx,
                            )
                            .bg(rgb(theme::secondary())),
                        )
                        .child(text(
                            "Changes apply to this local session.",
                            11.,
                            theme::muted_foreground(),
                        )),
                )
            })
            .child(
                self.button("run-workflow", "Run workflow", Action::RunWorkflow, cx)
                    .child(icon("play"))
                    .bg(rgb(theme::secondary())),
            )
    }
    pub(super) fn service_sidebar_view(&self, cx: &Context<Self>) -> AnyElement {
        let services = self
            .services
            .iter()
            .enumerate()
            .filter(|(_, p)| p.project_id == self.workspace().config.id)
            .collect::<Vec<_>>();
        let mut sidebar = col()
            .id("service-list")
            .w_full()
            .flex_shrink_0()
            .p_3()
            .gap_4();
        for (i, p) in services
            .iter()
            .filter(|(_, p)| p.search_name.contains(&self.query(cx)))
        {
            let title = self
                .workspace()
                .threads
                .iter()
                .find(|t| t.id == p.thread_id)
                .map(|t| t.title.clone())
                .unwrap_or_default();
            sidebar = sidebar.child(
                col()
                    .p_2()
                    .gap_2()
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .rounded_lg()
                    .child(text(short(&title, 46), 12., theme::muted_foreground()).p_1())
                    .child(
                        self.button(("service", *i), "", Action::Service(*i), cx)
                            .child(icon("service"))
                            .child(p.name.clone())
                            .h(px(45.))
                            .when(self.service == Some(*i), |d| {
                                d.bg(rgb(theme::sidebar_accent()))
                                    .border_1()
                                    .border_color(rgb(theme::primary()))
                            }),
                    ),
            );
        }
        col()
            .size_full()
            .child(self.mode_sidebar_header(cx))
            .child(
                div().flex_1().min_h_0().child(
                    self.left_scroll[self.section as usize].wrap("services-scroll", sidebar),
                ),
            )
            .into_any_element()
    }
    pub(super) fn services_view(&self, cx: &Context<Self>) -> AnyElement {
        let mut main = col().flex_1().min_w_0().h_full();
        if let Some(i) = self.service {
            let p = &self.services[i];
            let thread = self
                .workspace()
                .threads
                .iter()
                .position(|t| t.id == p.thread_id);
            main = main
                .child(
                    col()
                        .p_6()
                        .gap_2()
                        .border_b_1()
                        .border_color(rgb(theme::border()))
                        .child(
                            row()
                                .gap_3()
                                .child(
                                    row()
                                        .p_2()
                                        .rounded(px(5.))
                                        .bg(rgb(theme::sidebar_accent()))
                                        .child(icon("service").text_color(rgb(theme::primary()))),
                                )
                                .child(text(p.name.clone(), 22., theme::foreground()))
                                .child(div().flex_1())
                                .child(
                                    self.button(
                                        "stop-service",
                                        if self.stopped { "Restart" } else { "Stop" },
                                        Action::StopService,
                                        cx,
                                    )
                                    .child(icon(if self.stopped { "play" } else { "stop" }))
                                    .bg(rgb(theme::muted()))
                                    .h(px(44.)),
                                ),
                        )
                        .when_some(thread, |d, t| {
                            d.child(
                                self.button(
                                    "service-thread",
                                    short(&self.workspace().threads[t].title, 100),
                                    Action::Chat(t),
                                    cx,
                                )
                                .on_click(cx.listener(move |s, _, w, cx| {
                                    s.section = Section::Chats;
                                    s.act(Action::Chat(t), w, cx);
                                }))
                                .text_color(rgb(theme::muted_foreground())),
                            )
                        })
                        .child(icon_label(
                            if self.stopped { "stop" } else { "play" },
                            if self.stopped { "Stopped" } else { "Running" },
                            12.,
                            if self.stopped {
                                theme::muted_foreground()
                            } else {
                                theme::primary()
                            },
                        )),
                )
                .child(
                    row()
                        .h(px(44.))
                        .px_5()
                        .gap_2()
                        .child(
                            text("Output", 12., theme::foreground())
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(text("Following log", 10., theme::muted_foreground()))
                        .child(div().flex_1())
                        .child(
                            self.ib("wrap", "text-align-left", Action::Wrap, cx)
                                .when(self.wrap, |d| d.bg(rgb(theme::sidebar_accent()))),
                        )
                        .child(self.button("copy-log", "Copy output", Action::CopyOutput, cx)),
                )
                .child(
                    div().flex_1().min_h_0().w_full().child(
                        AnyView::from(self.log_region.clone())
                            .cached(StyleRefinement::default().size_full()),
                    ),
                )
                .child(
                    row()
                        .h(px(36.))
                        .px_5()
                        .justify_between()
                        .border_t_1()
                        .border_color(rgb(theme::border()))
                        .child(text(
                            format!("{} lines", p.lines.len()),
                            10.,
                            theme::muted_foreground(),
                        ))
                        .child(
                            self.button("follow", "", Action::Follow, cx)
                                .child(checkbox(self.follow))
                                .child("Following latest")
                                .text_size(config::text_pixels(10.))
                                .text_color(rgb(theme::primary())),
                        ),
                );
        } else {
            main = main.justify_center().items_center().child(text(
                "Select a service to follow its output.",
                13.,
                theme::muted_foreground(),
            ));
        }
        main.into_any_element()
    }
    pub(super) fn menu_view(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let menu = self.menu.unwrap_or("");
        if menu == "files" {
            let trigger = self
                .menu_triggers
                .borrow()
                .get("files")
                .copied()
                .unwrap_or_default();
            return anchored()
                .anchor(Corner::BottomLeft)
                .position(trigger.origin - point(px(0.), px(6.)))
                .snap_to_window()
                .child(
                    menu_surface()
                        .w(px(210.))
                        .child(self.menu_button("add-file", "Add a file", Action::AddFile, cx))
                        .child(self.menu_button(
                            "add-directory",
                            "Add a directory",
                            Action::AddDirectory,
                            cx,
                        )),
                )
                .into_any_element();
        }
        if menu == "agent" {
            let groups: [(&str, Vec<&str>, usize); 5] = [
                (
                    "Agent",
                    self.available_agents[self.machine]
                        .iter()
                        .map(|&i| self.agents[i].as_str())
                        .collect(),
                    self.available_agents[self.machine]
                        .iter()
                        .position(|&i| i == self.agent)
                        .unwrap_or(0),
                ),
                (
                    "Model",
                    if self.agent == 0 {
                        vec!["Opus 5", "Sonnet", "Haiku"]
                    } else {
                        vec!["Astra", "Sol", "Terra", "Luna"]
                    },
                    self.model,
                ),
                (
                    "Effort",
                    vec!["Low", "Medium", "High", "Extra high", "Max", "Ultra"],
                    self.effort,
                ),
                ("Speed", vec!["Standard", "Fast"], self.speed),
                (
                    "Permissions",
                    vec![
                        "Read-only",
                        "Ask for approval",
                        "Approve for me",
                        "Full access",
                    ],
                    self.permission,
                ),
            ];
            let mut columns = row().items_start().p_3().gap_2();
            for (g, (name, options, selected)) in groups.into_iter().enumerate() {
                let mut column = col()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(text(name, 11., theme::muted_foreground()).px_2().py_2());
                for (i, option) in options.into_iter().enumerate() {
                    let a = match g {
                        0 => Action::Agent(self.available_agents[self.machine][i]),
                        1 => Action::Model(i),
                        2 => Action::Effort(i),
                        3 => Action::Speed(i),
                        _ => Action::Permission(i),
                    };
                    column = column.child(
                        self.menu_button(
                            (SharedString::from(format!("agent-group-{g}")), i),
                            option.to_owned(),
                            a,
                            cx,
                        )
                        .text_size(config::text_pixels(11.))
                        .px_2()
                        .when(selected == i, |d| {
                            d.bg(rgb(theme::secondary())).child(icon("check"))
                        }),
                    );
                }
                columns = columns.child(column);
            }
            return menu_surface().p_0().absolute().left(px(self.left_panel_width()+20.)).bottom(px(116.)).w(px((f32::from(window.viewport_size().width)-self.left_panel_width()-42.).max(540.))).child(columns).child(row().h(px(35.)).px_3().border_t_1().border_color(rgb(theme::border())).justify_between().child(text("Work within the project. Auto-review checks requests for additional access.",10.,theme::muted_foreground()))).into_any_element();
        }
        let mut popup = menu_surface().absolute().w(px(270.));
        match menu {
            "mode-settings" => {
                let title = match self.section {
                    Section::Chats => "Chat settings",
                    Section::Groupchats => "Groupchat settings",
                    Section::Issues => "Issue settings",
                    Section::Whiteboard => "Whiteboard settings",
                    Section::Docs => "Doc settings",
                    Section::Workflows => "Workflow settings",
                    Section::Services => "Service settings",
                };
                popup = popup
                    .left(px(50.))
                    .bottom(px(47.))
                    .w(px(260.))
                    .p_1()
                    .rounded(px(4.))
                    .child(text(title, 11., theme::muted_foreground()).px_2().py_2());
                for (i, (label, _, checked, action)) in
                    self.mode_options(self.section).into_iter().enumerate()
                {
                    if matches!(action, Action::LeftPanel(_)) {
                        popup = popup
                            .child(div().h(px(1.)).my_1().bg(rgb(theme::border())))
                            .child(text("Panels", 11., theme::muted_foreground()).px_2().py_1());
                    }
                    popup = popup.child(
                        self.menu_button(("quick-setting", i), "", action, cx)
                            .w_full()
                            .h(px(32.))
                            .px_2()
                            .rounded(px(3.))
                            .child(div().w(px(16.)).flex_shrink_0().when(checked, |d| {
                                d.child(
                                    icon("check")
                                        .size(px(13.))
                                        .text_color(rgb(theme::primary())),
                                )
                            }))
                            .child(text(label, 13., theme::sidebar_foreground())),
                    );
                }
                popup = popup
                    .child(div().h(px(1.)).my_1().bg(rgb(theme::border())))
                    .child(
                        self.menu_button(
                            "configure-mode-settings",
                            "",
                            Action::ConfigureModeSettings,
                            cx,
                        )
                        .w_full()
                        .h(px(32.))
                        .px_2()
                        .rounded(px(3.))
                        .child(icon("settings").size(px(14.)))
                        .child(text(
                            "Configure mode settings",
                            13.,
                            theme::sidebar_foreground(),
                        )),
                    );
            }
            "machines" => {
                popup = popup
                    .top(px(48.))
                    .right(px(248.))
                    .w(px(360.))
                    .p_3()
                    .gap_2()
                    .rounded(px(4.))
                    .child(
                        row()
                            .h(px(32.))
                            .justify_between()
                            .child(
                                text("Machines", 11., theme::muted_foreground())
                                    .font_weight(FontWeight::BOLD),
                            )
                            .child(
                                self.ib("close-machine-picker", "close", Action::Close, cx)
                                    .h(px(32.)),
                            ),
                    )
                    .child(
                        row()
                            .h(px(40.))
                            .w_full()
                            .gap_2()
                            .px_2()
                            .bg(rgb(theme::input()))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .rounded(px(3.))
                            .when(
                                self.machine_query.focus_handle(cx).is_focused(window),
                                |d| {
                                    d.border_color(rgb(theme::ring()))
                                        .shadow(vec![theme::shadow(3.)])
                                },
                            )
                            .child(icon("search"))
                            .child(div().flex_1().min_w_0().child(self.machine_query.clone())),
                    );
                let query = self.machine_query.read(cx).content.trim().to_lowercase();
                let mut list = col()
                    .id("machine-picker-list")
                    .max_h(px(320.))
                    .overflow_y_scroll();
                let mut count = 0;
                for (i, machine) in MACHINES.iter().enumerate().filter(|(_, machine)| {
                    machine.name.to_lowercase().contains(&query)
                        || machine.kind.to_lowercase().contains(&query)
                }) {
                    count += 1;
                    list = list.child(
                        self.menu_button(("machine-option", i), "", Action::Machine(i), cx)
                            .h(px(44.))
                            .flex_shrink_0()
                            .min_w_0()
                            .px_2()
                            .gap_3()
                            .when(self.machine == i, |d| d.bg(rgb(theme::secondary())))
                            .child(
                                row()
                                    .w(px(24.))
                                    .justify_center()
                                    .flex_shrink_0()
                                    .child(icon("devices").size(px(20.))),
                            )
                            .child(
                                text(machine.name, 13., theme::sidebar_foreground())
                                    .flex_1()
                                    .min_w_0()
                                    .truncate(),
                            )
                            .when(self.machine == i, |d| d.child(icon("check"))),
                    );
                }
                if count == 0 {
                    list =
                        list.child(text("No machines found", 13., theme::muted_foreground()).p_4());
                }
                popup = popup.child(list);
            }
            "agents" => {
                popup = popup
                    .top(px(48.))
                    .right(px(140.))
                    .w(px(360.))
                    .p_3()
                    .gap_2()
                    .rounded(px(4.))
                    .child(
                        row()
                            .h(px(32.))
                            .justify_between()
                            .child(
                                text("Agents", 11., theme::muted_foreground())
                                    .font_weight(FontWeight::BOLD),
                            )
                            .child(
                                self.ib("close-agent-picker", "close", Action::Close, cx)
                                    .h(px(32.)),
                            ),
                    )
                    .child(
                        row()
                            .h(px(40.))
                            .w_full()
                            .gap_2()
                            .px_2()
                            .bg(rgb(theme::input()))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .rounded(px(3.))
                            .when(self.agent_query.focus_handle(cx).is_focused(window), |d| {
                                d.border_color(rgb(theme::ring()))
                                    .shadow(vec![theme::shadow(3.)])
                            })
                            .child(icon("search"))
                            .child(div().flex_1().min_w_0().child(self.agent_query.clone())),
                    );
                let query = self.agent_query.read(cx).content.trim().to_lowercase();
                let mut list = col()
                    .id("agent-picker-list")
                    .max_h(px(320.))
                    .overflow_y_scroll();
                let mut count = 0;
                for &i in self.available_agents[self.machine]
                    .iter()
                    .filter(|&&i| self.agents[i].to_lowercase().contains(&query))
                {
                    count += 1;
                    list = list.child(
                        self.menu_button(("machine-agent", i), "", Action::Agent(i), cx)
                            .h(px(44.))
                            .flex_shrink_0()
                            .min_w_0()
                            .px_2()
                            .gap_3()
                            .when(self.agent == i, |d| d.bg(rgb(theme::secondary())))
                            .child(
                                row().w(px(24.)).justify_center().flex_shrink_0().child(
                                    icon(["claude", "chatgpt", "grok", "sparkle"][i.min(3)])
                                        .size(px(20.)),
                                ),
                            )
                            .child(
                                text(self.agents[i].clone(), 13., theme::sidebar_foreground())
                                    .flex_1()
                                    .min_w_0()
                                    .truncate(),
                            )
                            .when(self.agent == i, |d| d.child(icon("check"))),
                    );
                }
                if count == 0 {
                    list = list.child(
                        text(
                            if self.available_agents[self.machine].is_empty() {
                                "Currently there are no agents defined. Please add one below"
                            } else {
                                "No agents found"
                            },
                            13.,
                            theme::muted_foreground(),
                        )
                        .p_4(),
                    );
                }
                popup = popup
                    .child(list)
                    .child(div().h(px(1.)).bg(rgb(theme::border())))
                    .child(
                        self.button("add-agents", "", Action::AddAgent, cx)
                            .w_full()
                            .h(px(38.))
                            .justify_center()
                            .rounded(px(4.))
                            .bg(rgb(theme::secondary()))
                            .text_color(rgb(theme::secondary_foreground()))
                            .child(icon("plus").size(px(15.)))
                            .child("Add an Agent"),
                    );
            }
            "app" => {
                let modifier = if cfg!(target_os = "macos") {
                    "Cmd"
                } else {
                    "Ctrl"
                };
                let groups = vec![
                    vec![("About Adeline", String::new(), Action::About)],
                    vec![
                        ("Settings", format!("{modifier}+,"), Action::AppSettings),
                        ("Open Project Settings", String::new(), Action::Settings),
                        ("Switch Project...", String::new(), Action::Projects),
                    ],
                    vec![
                        ("New Chat", format!("{modifier}+N"), Action::NewChat),
                        (
                            "Groupchats",
                            String::new(),
                            Action::Section(Section::Groupchats),
                        ),
                        ("Issues", String::new(), Action::Section(Section::Issues)),
                        (
                            "Whiteboard",
                            String::new(),
                            Action::Section(Section::Whiteboard),
                        ),
                        ("Docs", String::new(), Action::Section(Section::Docs)),
                        (
                            "Workflows",
                            String::new(),
                            Action::Section(Section::Workflows),
                        ),
                        (
                            "Services",
                            String::new(),
                            Action::Section(Section::Services),
                        ),
                    ],
                    vec![(
                        "Keyboard Shortcuts",
                        String::new(),
                        Action::KeyboardShortcuts,
                    )],
                    vec![(
                        "Quit Adeline",
                        if cfg!(target_os = "macos") {
                            "Cmd+Q".into()
                        } else {
                            String::new()
                        },
                        Action::QuitApp,
                    )],
                ];
                popup = popup
                    .bottom(px(47.))
                    .left(px(16.))
                    .w(px(280.))
                    .p_1()
                    .rounded(px(4.));
                for (group_index, group) in groups.into_iter().enumerate() {
                    if group_index == 2 && !self.has_open_project() {
                        continue;
                    }
                    if group_index > 0 {
                        popup = popup.child(div().h(px(1.)).my_1().bg(rgb(theme::border())));
                    }
                    for (item_index, (label, shortcut, action)) in group.into_iter().enumerate() {
                        if matches!(action, Action::Settings) && !self.has_open_project() {
                            continue;
                        }
                        if let Action::Section(section) = &action
                            && !config::current().general.features.enabled(*section)
                        {
                            continue;
                        }
                        popup = popup.child(
                            self.menu_button(
                                ("app-menu-item", group_index * 10 + item_index),
                                "",
                                action,
                                cx,
                            )
                            .w_full()
                            .h(px(32.))
                            .px_2()
                            .rounded(px(3.))
                            .justify_between()
                            .child(text(label, 13., theme::sidebar_foreground()))
                            .child(text(
                                shortcut,
                                12.,
                                theme::muted_foreground(),
                            )),
                        );
                    }
                }
            }
            "projects" => {
                popup = popup
                    .top(px(48.))
                    .right(px(16.))
                    .w(px(360.))
                    .p_3()
                    .gap_2()
                    .rounded(px(4.))
                    .child(
                        row()
                            .h(px(32.))
                            .justify_between()
                            .child(
                                text("Your projects", 11., theme::muted_foreground())
                                    .font_weight(FontWeight::BOLD),
                            )
                            .child(
                                self.ib("close-project-picker", "close", Action::Close, cx)
                                    .h(px(32.)),
                            ),
                    )
                    .child(
                        row()
                            .h(px(40.))
                            .w_full()
                            .gap_2()
                            .px_2()
                            .bg(rgb(theme::input()))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .rounded(px(3.))
                            .when(
                                self.project_query.focus_handle(cx).is_focused(window),
                                |d| {
                                    d.border_color(rgb(theme::ring()))
                                        .shadow(vec![theme::shadow(3.)])
                                },
                            )
                            .child(icon("search"))
                            .child(div().flex_1().min_w_0().child(self.project_query.clone())),
                    );
                let query = self.project_query.read(cx).content.trim().to_lowercase();
                let indices: Vec<_> = (0..self.projects.len()).collect();
                let mut list = col()
                    .id("project-picker-list")
                    .max_h(px(320.))
                    .overflow_y_scroll();
                let mut count = 0;
                for i in indices.into_iter().filter(|i| {
                    self.projects[*i]
                        .config
                        .name
                        .to_lowercase()
                        .contains(&query)
                }) {
                    count += 1;
                    list = list.child(
                        row()
                            .h(px(44.))
                            .flex_shrink_0()
                            .rounded(px(3.))
                            .when(i == self.project && self.open_projects[i], |d| {
                                d.bg(rgb(theme::secondary()))
                            })
                            .child(
                                self.menu_button(("project-menu", i), "", Action::Project(i), cx)
                                    .h_full()
                                    .flex_1()
                                    .min_w_0()
                                    .px_2()
                                    .gap_3()
                                    .child(self.project_icon(i, true))
                                    .child(
                                        text(
                                            self.projects[i].config.name.clone(),
                                            13.,
                                            theme::sidebar_foreground(),
                                        )
                                        .flex_1()
                                        .min_w_0()
                                        .truncate(),
                                    )
                                    .child(
                                        text(
                                            if self.open_projects[i] {
                                                "Open"
                                            } else {
                                                "Closed"
                                            },
                                            11.,
                                            theme::muted_foreground(),
                                        )
                                        .flex_shrink_0(),
                                    ),
                            ),
                    );
                }
                if count == 0 {
                    list = list.child(
                        text(
                            if self.projects.is_empty() {
                                "Currently there are no projects defined. Please add one below"
                            } else {
                                "No projects found"
                            },
                            13.,
                            theme::muted_foreground(),
                        )
                        .p_4(),
                    );
                }
                popup = popup
                    .child(list)
                    .child(div().h(px(1.)).bg(rgb(theme::border())))
                    .child(
                        self.button("add-projects", "", Action::AddProject, cx)
                            .w_full()
                            .h(px(38.))
                            .justify_center()
                            .rounded(px(4.))
                            .bg(rgb(theme::secondary()))
                            .text_color(rgb(theme::secondary_foreground()))
                            .child(icon("plus").size(px(15.)))
                            .child("Add a project"),
                    );
            }
            "chat" => {
                popup = popup
                    .top(px(203.))
                    .right(px(if self.side_panel_is_open() {
                        self.right_panel_width + 23.
                    } else {
                        24.
                    }))
                    .child(self.menu_button("menu-complete", "Mark as idle", Action::Complete, cx))
                    .child(self.menu_button(
                        "menu-activity",
                        "Show agent activity",
                        Action::ToggleSidePanel,
                        cx,
                    ))
                    .child(self.menu_button("menu-new", "New chat", Action::NewChat, cx));
            }
            "document" => {
                popup = popup
                    .top(px(255.))
                    .right(px(25.))
                    .child(self.menu_button("menu-pin", "Pin document", Action::PinDoc, cx))
                    .child(self.menu_button("menu-raw", "Toggle raw Markdown", Action::Raw, cx))
                    .child(self.menu_button("menu-files", "Back to files", Action::DocsHome, cx));
                popup = popup.child(
                    self.menu_button(
                        "archive-document",
                        if self
                            .document
                            .is_some_and(|i| self.archived_docs.contains(&(self.project, i)))
                        {
                            "Restore document"
                        } else {
                            "Archive document"
                        },
                        Action::ArchiveDoc,
                        cx,
                    ),
                );
            }
            _ => return div().into_any_element(),
        }
        popup.into_any_element()
    }
    pub(super) fn modal_view(&self, cx: &Context<Self>) -> AnyElement {
        let mut dialog = col()
            .w(px(600.))
            .p_8()
            .gap_6()
            .rounded(px(28.))
            .bg(rgb(theme::card()))
            .shadow(vec![theme::shadow(24.)]);
        if matches!(self.modal, Some("about" | "shortcuts")) {
            let about = self.modal == Some("about");
            dialog = dialog.child(
                row()
                    .justify_between()
                    .child(
                        text(
                            if about {
                                "About Adeline"
                            } else {
                                "Keyboard shortcuts"
                            },
                            24.,
                            theme::foreground(),
                        )
                        .font_weight(FontWeight::BOLD),
                    )
                    .child(self.ib("close-app-dialog", "close", Action::Close, cx)),
            );
            if about {
                dialog = dialog
                    .child(text(
                        concat!("Adeline ", env!("CARGO_PKG_VERSION")),
                        16.,
                        theme::foreground(),
                    ))
                    .child(text(
                        "A native workspace for chats, files, workflows, and services.",
                        14.,
                        theme::muted_foreground(),
                    ))
                    .child(text(
                        "This demo uses sample data. Changes reset when the application restarts.",
                        13.,
                        theme::muted_foreground(),
                    ));
            } else {
                let modifier = if cfg!(target_os = "macos") {
                    "Cmd"
                } else {
                    "Ctrl"
                };
                for (label, shortcut) in [
                    ("New chat", format!("{modifier}+N")),
                    ("Focus search", format!("{modifier}+F")),
                    ("Send message", format!("{modifier}+Enter")),
                    ("Close dialog or menu", "Escape".into()),
                    ("Move between controls", "Tab / Shift+Tab".into()),
                    ("Activate focused button", "Enter / Space".into()),
                ] {
                    dialog = dialog.child(
                        row()
                            .justify_between()
                            .child(text(label, 14., theme::foreground()))
                            .child(text(shortcut, 13., theme::muted_foreground())),
                    );
                }
            }
        } else if matches!(
            self.modal,
            Some("workflow" | "collection" | "title" | "add-project" | "add-agent")
        ) {
            let workflow = self.modal == Some("workflow");
            let title = match self.modal {
                Some("workflow") => {
                    if self.workflow.is_some() {
                        "Edit workflow"
                    } else {
                        "New workflow"
                    }
                }
                Some("add-project") => "Add a project",
                Some("add-agent") => "Add an Agent",
                Some("collection") => "New collection",
                _ => "Document title",
            };
            let action = match self.modal {
                Some("workflow") => Action::SaveWorkflow,
                Some("add-project") => Action::SaveProject,
                Some("add-agent") => Action::SaveAgent,
                Some("collection") => Action::SaveCollection,
                _ => Action::SaveTitle,
            };
            dialog = dialog
                .child(
                    row()
                        .justify_between()
                        .child(text(title, 26., theme::foreground()))
                        .child(self.ib("close-form", "close", Action::Close, cx)),
                )
                .child(text("Name", 13., theme::muted_foreground()))
                .child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(theme::secondary()))
                        .child(self.name_input.clone()),
                )
                .when(workflow, |d| {
                    d.child(text(
                        "What should your agent do?",
                        13.,
                        theme::muted_foreground(),
                    ))
                    .child(
                        div()
                            .w_full()
                            .p_3()
                            .rounded_lg()
                            .bg(rgb(theme::secondary()))
                            .child(self.edit_input.clone()),
                    )
                })
                .child(
                    row()
                        .justify_end()
                        .gap_2()
                        .child(self.button("cancel-form", "Cancel", Action::Close, cx))
                        .child(
                            self.button("save-form", "Save", action, cx)
                                .bg(rgb(theme::secondary())),
                        ),
                );
        } else if self.modal == Some("edit") {
            dialog = dialog
                .child(
                    row()
                        .justify_between()
                        .child(text("Edit text", 24., theme::foreground()))
                        .child(self.ib("close-edit", "close", Action::Close, cx)),
                )
                .child(
                    div()
                        .p_3()
                        .bg(rgb(theme::secondary()))
                        .rounded_lg()
                        .child(self.edit_input.clone()),
                )
                .child(
                    row()
                        .justify_end()
                        .gap_2()
                        .child(self.button("cancel-edit", "Cancel", Action::Close, cx))
                        .child(
                            self.button("save-edit", "Save", Action::SaveLine, cx)
                                .bg(rgb(theme::secondary())),
                        ),
                );
        } else {
            dialog = dialog.child(
                row()
                    .gap_4()
                    .child(icon("chat-illustration").size(px(48.)))
                    .child(
                        text("Project settings", 28., theme::foreground())
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(div().flex_1())
                    .child(
                        self.ib("close-settings", "close", Action::Close, cx)
                            .rounded_full()
                            .bg(rgb(theme::secondary())),
                    ),
            );
            let mut colors = row().gap_3();
            for (i, color) in theme::project_colors().into_iter().enumerate() {
                colors = colors.child(
                    self.button(("tint", i), "", Action::Tint(i), cx)
                        .when(i == self.selected_tint, |d| d.child(icon("check")))
                        .size(px(34.))
                        .rounded_full()
                        .bg(rgb(color))
                        .border_1()
                        .border_color(rgb(theme::border())),
                );
            }
            dialog = dialog
                .child(
                    col()
                        .p_4()
                        .gap_4()
                        .rounded(px(18.))
                        .bg(rgb(theme::muted()))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .child(
                            row()
                                .gap_4()
                                .child(
                                    row().p_5().child(
                                        icon(
                                            [
                                                "project-circle",
                                                "project-triangle",
                                                "project-square",
                                            ][self.project.min(2)],
                                        )
                                        .size(px(36.))
                                        .text_color(rgb(
                                            theme::project_colors()[self.selected_tint],
                                        )),
                                    ),
                                )
                                .child(
                                    col()
                                        .flex_1()
                                        .gap_2()
                                        .child(text("Project name", 13., theme::muted_foreground()))
                                        .child(
                                            div()
                                                .p_2()
                                                .bg(rgb(theme::sidebar()))
                                                .rounded(px(12.))
                                                .child(self.name_input.clone()),
                                        ),
                                ),
                        )
                        .child(text("Folder name", 13., theme::muted_foreground()))
                        .child(
                            text(self.workspace().config.id.clone(), 16., theme::foreground())
                                .p_3()
                                .bg(rgb(theme::sidebar()))
                                .rounded(px(12.)),
                        )
                        .child(
                            col()
                                .p_4()
                                .gap_3()
                                .bg(rgb(theme::card()))
                                .rounded(px(14.))
                                .border_1()
                                .border_color(rgb(theme::border()))
                                .child(text("Tint color", 12., theme::muted_foreground()))
                                .child(colors)
                                .child(text(
                                    ["Chart 4", "Accent", "Chart 5", "Muted", "Card"]
                                        [self.selected_tint],
                                    11.,
                                    theme::muted_foreground(),
                                )),
                        ),
                )
                .child(
                    self.button(
                        "instructions",
                        format!(
                            "Instructions: {}",
                            if self.instructions {
                                "Project instructions"
                            } else {
                                "None"
                            }
                        ),
                        Action::Instructions,
                        cx,
                    )
                    .child(icon("chevron"))
                    .h(px(58.))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .rounded(px(16.)),
                )
                .when(self.instructions, |d| {
                    d.child(
                        text(
                            "Keep the interface human. Work in small, reviewable steps.",
                            13.,
                            theme::muted_foreground(),
                        )
                        .p_3()
                        .bg(rgb(theme::muted()))
                        .rounded_lg(),
                    )
                })
                .child(
                    row()
                        .justify_end()
                        .gap_3()
                        .child(
                            self.button("cancel-settings", "Cancel", Action::Close, cx)
                                .h(px(46.))
                                .bg(rgb(theme::secondary())),
                        )
                        .child(
                            self.button("save-settings", "Save", Action::SaveSettings, cx)
                                .h(px(46.))
                                .bg(rgb(theme::secondary()))
                                .shadow(vec![theme::shadow(3.)]),
                        ),
                );
        }
        div()
            .absolute()
            .inset_0()
            .bg(rgba((theme::shadow_base() << 8) | 0x80))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(dialog)
            .into_any_element()
    }
}
