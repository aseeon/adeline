//! The resizable workspace layout (left, main and side panels) and the control bar.
use super::*;
use gpui_kit::base::ResizeHandleContext;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use std::rc::Rc;

// In px, so it holds at any interface text size.
const MAIN_PANEL_MIN: Pixels = px(360.);

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
                    .bg(cx.theme().status_bar.alpha(titlebar::GLASS))
                    .when(
                        app.has_open_project() && app.section == Section::Chats,
                        |row| {
                            row.child(
                                app.icon_button(
                                    "left-panel",
                                    "Conversations",
                                    Icon::default().path("panel-left.svg"),
                                    Action::ToggleLeftPanel,
                                    cx,
                                )
                                .selected(app.left_panel_open),
                            )
                            .child(
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
                                .selected(app.side_panel_open),
                            )
                        },
                    )
                    .when(
                        app.has_open_project() && app.section == Section::Docs,
                        |row| {
                            let docs = config::current().modes.docs;
                            row.child(
                                app.icon_button(
                                    "docs-left-panel",
                                    "Docs tree",
                                    Icon::default().path("panel-left.svg"),
                                    Action::Docs(docs::Act::LeftPanel),
                                    cx,
                                )
                                .selected(docs.show_left_panel),
                            )
                            .child(
                                app.command_popover(
                                    "mode-settings",
                                    Button::new("docs-settings")
                                        .icon(Icon::default().path("settings.svg"))
                                        .accessibility_label("Docs settings")
                                        .tooltip("Docs settings")
                                        .small()
                                        .ghost(),
                                    Anchor::BottomLeft,
                                    cx,
                                ),
                            )
                            .child(div().flex_1())
                            .child(
                                app.icon_button(
                                    "docs-right-panel",
                                    "Conversation panel",
                                    Icon::default()
                                        .path("panel-left.svg")
                                        .rotate(Radians(std::f32::consts::PI)),
                                    Action::Docs(docs::Act::ChatPanel),
                                    cx,
                                )
                                .selected(docs.show_conversation_panel),
                            )
                        },
                    )
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}
impl Adeline {
    pub(super) fn left_panel_is_open(&self) -> bool {
        self.section == Section::Chats && self.left_panel_open
    }
    pub(super) fn side_panel_is_open(&self) -> bool {
        self.section == Section::Chats && self.side_panel_open
    }
    pub(super) fn workspace_panels(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        // A collapsed conversation list stays on screen as the chat rail.
        let rail = (!self.left_panel_is_open()).then(|| {
            div()
                .id("conversation-rail-panel")
                .role(Role::Group)
                .aria_label("Collapsed conversation list")
                .relative()
                .h_full()
                .w(rems(chat::COLLAPSED_WIDTH))
                .flex_shrink_0()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child(self.chat_list.clone())
                .child(fading_line(cx).absolute().top_0().right_0())
        });
        let panels = h_resizable("chat-panels")
            .with_state(&self.panel_state)
            .with_handle_appearance(Rc::new(fading_divider))
            .child(
                resizable_panel()
                    .visible(self.left_panel_is_open())
                    // Resizable takes logical pixels; scale with interface typography, not display DPI.
                    .size(fonts::text_pixels(self.sidebar_width))
                    .size_range(fonts::text_pixels(140.)..fonts::text_pixels(520.))
                    .flex_grow_0()
                    .flex_shrink_1()
                    .child(
                        div()
                            .id("conversation-list-panel")
                            .role(Role::Group)
                            .aria_label("Conversation list panel")
                            .size_full()
                            .bg(cx.theme().background)
                            .text_color(cx.theme().foreground)
                            .child(self.chat_list.clone()),
                    ),
            )
            .child(
                // The side panels shrink first: they give way down to their own
                // minimum before the main panel drops below this one.
                resizable_panel()
                    .size_range(MAIN_PANEL_MIN..Pixels::MAX)
                    .child(self.chats(window, cx)),
            )
            .child(
                resizable_panel()
                    .visible(self.side_panel_is_open())
                    .size(fonts::text_pixels(self.right_panel_width))
                    .size_range(fonts::text_pixels(140.)..fonts::text_pixels(520.))
                    .flex_grow_0()
                    .flex_shrink_1()
                    .child(chat::region(&self.side_panel, chat::full(), window)),
            );
        row()
            .size_full()
            .items_stretch()
            .children(rail)
            .child(div().flex_1().min_w_0().h_full().child(panels))
            .into_any_element()
    }
}

fn fading_divider(
    handle: &ResizeHandleContext,
    _: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if handle.is_active() || handle.axis() != Axis::Horizontal {
        return None;
    }
    Some(fading_line(cx).into_any_element())
}

fn fading_line(cx: &App) -> Div {
    const CLEAR: f32 = 2.5;
    const FADE: f32 = 7.5;
    let line = cx.theme().border;
    let ramp = |from: Hsla, to: Hsla| {
        div().flex_none().h(rems(FADE - CLEAR)).bg(linear_gradient(
            180.,
            linear_color_stop(from, 0.),
            linear_color_stop(to, 1.),
        ))
    };
    col()
        .flex_none()
        .h_full()
        .w(px(1.))
        .child(div().flex_none().h(rems(CLEAR)))
        .child(ramp(line.alpha(0.), line))
        .child(div().flex_1().min_h_0().bg(line))
        .child(ramp(line, line.alpha(0.)))
        .child(div().flex_none().h(rems(CLEAR)))
}
