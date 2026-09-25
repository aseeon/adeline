//! Shared workspace panes. Page content is separate from panel chrome.
use super::*;

/// Shared surface for navigation and detail panels in every workspace mode.
pub(super) fn panel_surface() -> Div {
    col()
        .bg(rgb(theme::sidebar()))
        .text_color(rgb(theme::sidebar_foreground()))
}

/// The main canvas is independent of card and panel colors.
fn workspace_surface() -> Div {
    col()
        .bg(rgb(theme::background()))
        .text_color(rgb(theme::foreground()))
}

pub(super) struct ControlPane {
    owner: WeakEntity<Adeline>,
}

impl ControlPane {
    pub(super) fn new(owner: WeakEntity<Adeline>) -> Self {
        Self { owner }
    }
}

impl Render for ControlPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |app, cx| {
                row()
                    .id("control-pane")
                    .w_full()
                    .h(px(43.))
                    .flex_shrink_0()
                    .px_2()
                    .gap_2()
                    .bg(rgb(theme::background()))
                    .text_color(rgb(theme::foreground()))
                    .child(app.ib("app-menu", "menu", Action::AppMenu, cx))
                    .child(app.ib("mode-settings", "settings", Action::ModeSettings, cx))
                    .child(
                        app.ib(
                            "toggle-left-panel",
                            "panel-left",
                            Action::ToggleLeftPanel,
                            cx,
                        )
                        .flex_shrink_0(),
                    )
                    .child(div().flex_1())
                    .child(
                        app.button("side-panel", "", Action::ToggleSidePanel, cx)
                            .w(px(34.))
                            .px_0()
                            .justify_center()
                            .child(
                                icon("panel-left").with_transformation(Transformation::rotate(
                                    radians(std::f32::consts::PI),
                                )),
                            ),
                    )
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Adeline {
    pub(super) fn mode_sidebar_header(&self, cx: &Context<Self>) -> Div {
        let (name, glyph, action) = match self.section {
            Section::Chats => ("Chats", "chat", Action::NewChat),
            Section::Groupchats => ("Groupchats", "group", Action::NewGroup),
            Section::Issues => ("Issues", "check-square", Action::NewIssue),
            Section::Whiteboard => ("Whiteboard", "whiteboard", Action::BoardTool(2)),
            Section::Docs => ("Docs", "file", Action::NewDoc),
            Section::Workflows => ("Workflows", "workflow", Action::NewWorkflow),
            Section::Services => ("Services", "service", Action::NewService),
        };
        panel_surface()
            .w_full()
            .flex_shrink_0()
            .child(
                row()
                    .h(px(77.))
                    .px(px(18.))
                    .gap_2()
                    .child(
                        div()
                            .w(px(36.))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .child(
                                icon(glyph)
                                    .size(px(24.))
                                    .text_color(rgb(theme::sidebar_foreground())),
                            ),
                    )
                    .child(text(name, 20., theme::sidebar_foreground()))
                    .child(div().flex_1())
                    .child(
                        self.button("new-mode-item", "", action, cx)
                            .size(px(34.))
                            .flex_shrink_0()
                            .px_0()
                            .justify_center()
                            .rounded(px(12.))
                            .bg(rgb(theme::secondary()))
                            .child(
                                icon("plus")
                                    .size(px(18.))
                                    .text_color(rgb(theme::sidebar_foreground())),
                            ),
                    ),
            )
            .child(
                self.search_box(cx)
                    .w(px(self.sidebar_width - 28.))
                    .mx(px(14.)),
            )
    }

    pub(super) fn left_panel_is_open(&self) -> bool {
        self.left_panel_open[self.section as usize]
    }

    pub(super) fn left_panel_width(&self) -> f32 {
        if self.left_panel_is_open() {
            self.sidebar_width + 1.
        } else {
            0.
        }
    }

    /// Page content shares the optional left panel above the global control bar.
    pub(super) fn workspace_with_left_panel(
        &self,
        body: AnyElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        if !self.left_panel_is_open() {
            return workspace_surface()
                .size_full()
                .child(div().flex_1().min_h_0().child(body))
                .into_any_element();
        }
        row()
            .size_full()
            .items_start()
            .child(
                col()
                    .w(px(self.sidebar_width))
                    .h_full()
                    .flex_shrink_0()
                    .child(self.left_panel(self.page_left_panel(cx))),
            )
            .child(
                div()
                    .relative()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(rgb(theme::border()))
                    .child(
                        div()
                            .id("left-panel-resize")
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(-3.))
                            .w(px(7.))
                            .cursor(CursorStyle::ResizeLeftRight)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|s, e: &MouseDownEvent, _, cx| {
                                    if e.click_count == 2 {
                                        s.sidebar_width = 360.;
                                    } else {
                                        s.dragging = true;
                                    }
                                    cx.notify();
                                }),
                            ),
                    ),
            )
            .child(workspace_surface().flex_1().min_w_0().h_full().child(body))
            .into_any_element()
    }

    fn left_panel(&self, content: AnyElement) -> Stateful<Div> {
        panel_surface()
            .id("left-panel")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(content)
    }

    fn page_left_panel(&self, cx: &Context<Self>) -> AnyElement {
        match self.section {
            Section::Groupchats | Section::Issues | Section::Whiteboard => {
                self.collaboration_left(cx)
            }
            Section::Chats => AnyView::from(self.chat_list.clone())
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Section::Services => AnyView::from(self.service_sidebar_region.clone())
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Section::Docs => self.explorer_content(cx),
            Section::Workflows => {
                let mut names = vec!["All".to_owned(), "Scheduled".into(), "Yours".into()];
                for recipe in &self.workspace().recipes {
                    if !names.contains(&recipe.collection) {
                        names.push(recipe.collection.clone());
                    }
                }
                let mut content = col()
                    .id("workflow-collections")
                    .w_full()
                    .flex_shrink_0()
                    .p_4()
                    .gap_2();
                for (i, name) in names.into_iter().enumerate() {
                    let active = self.collection == name;
                    content = content.child(
                        self.button(
                            ("left-collection", i),
                            name.clone(),
                            Action::Collection(name),
                            cx,
                        )
                        .when(active, |d| {
                            d.bg(rgb(theme::sidebar_accent()))
                                .text_color(rgb(theme::sidebar_accent_foreground()))
                        }),
                    );
                }
                col()
                    .size_full()
                    .child(self.mode_sidebar_header(cx))
                    .child(div().flex_1().min_h_0().child(
                        self.left_scroll[self.section as usize].wrap("collections-scroll", content),
                    ))
                    .into_any_element()
            }
        }
    }

    pub(super) fn side_panel_is_open(&self) -> bool {
        self.side_panel_open[self.section as usize]
    }

    /// Reusable panel frame: callers supply a title and arbitrary page content.
    fn side_panel(
        &self,
        title: &'static str,
        content: AnyElement,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        panel_surface()
            .id("side-panel")
            .relative()
            .w(px(self.right_panel_width))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(rgb(theme::border()))
            .child(
                div()
                    .id("right-panel-resize")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-3.))
                    .w(px(7.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|s, e: &MouseDownEvent, _, cx| {
                            if e.click_count == 2 {
                                s.right_panel_width = 302.;
                            } else {
                                s.dragging_right = true;
                            }
                            cx.notify();
                        }),
                    ),
            )
            .child(
                row()
                    .h(px(58.))
                    .flex_shrink_0()
                    .px_4()
                    .justify_between()
                    .child(text(title, 16., theme::sidebar_foreground()))
                    .child(self.ib("close-side-panel", "close", Action::ToggleSidePanel, cx)),
            )
            .child(
                div().flex_1().min_h_0().child(
                    self.right_scroll[self.section as usize].wrap("details-scroll", content),
                ),
            )
    }

    pub(super) fn page_side_panel(&self, cx: &Context<Self>) -> Stateful<Div> {
        let (title, content) = match self.section {
            Section::Groupchats | Section::Whiteboard => ("Members", self.collaboration_members()),
            Section::Issues => ("Ticket details", self.issue_details(cx)),
            Section::Chats => (
                "Agent activity",
                self.activity_content(cx).into_any_element(),
            ),
            Section::Docs => (
                "Doc details",
                text(
                    self.document.map_or_else(
                        || "Select a document to see its details.".into(),
                        |i| self.workspace().docs[i].filename.clone(),
                    ),
                    13.,
                    theme::muted_foreground(),
                )
                .p_5()
                .into_any_element(),
            ),
            Section::Workflows => (
                "Workflow details",
                self.workflow_details(cx).into_any_element(),
            ),
            Section::Services => (
                "Service details",
                text(
                    self.service.map_or_else(
                        || "Select a service to see its details.".into(),
                        |i| self.services[i].name.clone(),
                    ),
                    13.,
                    theme::muted_foreground(),
                )
                .p_5()
                .into_any_element(),
            ),
        };
        self.side_panel(title, content, cx)
    }
}
