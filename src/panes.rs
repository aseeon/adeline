use super::*;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};

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
                    .id("control-bar")
                    .overflow_x_scroll()
                    .w_full()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        app.command_popover(
                            "app",
                            Button::new("app-menu")
                                .icon(Icon::default().path("menu.svg"))
                                .accessibility_label("Main menu")
                                .tooltip("Main menu")
                                .small()
                                .ghost(),
                            Anchor::BottomLeft,
                            cx,
                        ),
                    )
                    .when(
                        app.has_open_project() && app.section == Section::Chats,
                        |row| {
                            row.child(
                                app.command_popover(
                                    "mode-settings",
                                    Button::new("chat-settings")
                                        .icon(Icon::default().path("settings.svg"))
                                        .accessibility_label("Chat settings")
                                        .tooltip("Chat settings")
                                        .small()
                                        .ghost(),
                                    Anchor::BottomLeft,
                                    cx,
                                ),
                            )
                            .child(
                                app.icon_button(
                                    "left-panel",
                                    "Conversations",
                                    Icon::default().path("panel-left.svg"),
                                    Action::ToggleLeftPanel,
                                    cx,
                                )
                                .selected(app.left_panel_open[0]),
                            )
                            .child(div().flex_1())
                            .child(
                                app.icon_button(
                                    "right-panel",
                                    "Agent activity",
                                    Icon::default()
                                        .path("panel-left.svg")
                                        .rotate(Radians(std::f32::consts::PI)),
                                    Action::ToggleSidePanel,
                                    cx,
                                )
                                .selected(app.side_panel_open[0]),
                            )
                        },
                    )
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}
impl Adeline {
    pub(super) fn mode_sidebar_header(&self, search: impl IntoElement, cx: &Context<Self>) -> Div {
        col()
            .p_3()
            .gap_3()
            .child(
                row()
                    .gap_2()
                    .child(div().flex_1().text_lg().child("Chats"))
                    .child(self.icon_button(
                        "new-chat",
                        "New chat",
                        Icon::default().path("plus.svg"),
                        Action::NewChat,
                        cx,
                    )),
            )
            .child(search)
    }
    pub(super) fn left_panel_is_open(&self) -> bool {
        self.section == Section::Chats && self.left_panel_open[0]
    }
    pub(super) fn side_panel_is_open(&self) -> bool {
        self.section == Section::Chats && self.side_panel_open[0]
    }
    pub(super) fn workspace_panels(&self, cx: &Context<Self>) -> AnyElement {
        h_resizable("chat-panels")
            .with_state(&self.panel_state)
            .child(
                resizable_panel()
                    .visible(self.left_panel_is_open())
                    // Resizable takes logical pixels; scale with interface typography, not display DPI.
                    .size(config::text_pixels(self.sidebar_width))
                    .size_range(config::text_pixels(140.)..config::text_pixels(520.))
                    .flex_grow_0()
                    .flex_shrink_1()
                    .child(
                        div()
                            .id("conversation-list-panel")
                            .role(Role::Group)
                            .aria_label("Conversation list panel")
                            .size_full()
                            .bg(cx.theme().sidebar)
                            .text_color(cx.theme().sidebar_foreground)
                            .child(self.chat_list.clone()),
                    ),
            )
            .child(
                resizable_panel()
                    .size_range(config::text_pixels(168.)..Pixels::MAX)
                    .child(self.chats(cx)),
            )
            .child(
                resizable_panel()
                    .visible(self.side_panel_is_open())
                    .size(config::text_pixels(self.right_panel_width))
                    .size_range(config::text_pixels(140.)..config::text_pixels(520.))
                    .flex_grow_0()
                    .flex_shrink_1()
                    .child(
                        col()
                            .id("agent-activity-panel")
                            .role(Role::Group)
                            .aria_label("Agent activity panel")
                            .size_full()
                            .bg(cx.theme().sidebar)
                            .text_color(cx.theme().sidebar_foreground)
                            .child(div().p_3().text_lg().child("Agent activity"))
                            .child(
                                div()
                                    .id("activity-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .child(self.activity_content(cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
