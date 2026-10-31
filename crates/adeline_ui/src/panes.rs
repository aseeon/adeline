//! The Islands layout (docs/design/DESIGN.md): the window header, the left
//! island (mode rail and chat list), the main area, and the right island (the
//! side pane and its rail). Both rails share `rail` and `rail_button`, so they
//! look and behave the same.
use super::*;
use adeline_core::conversation::ThreadStatus;
use gpui_kit::base::ResizeHandleContext;

pub(super) const PAD: f32 = 8.;
pub(super) const TOP: f32 = 42.;
pub(super) const BOTTOM: f32 = 21.;
const RAIL: f32 = 56.;
const LIST: f32 = 286.;
const PANE: f32 = 330.;
const PANE_HEADER: f32 = 46.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SideTab {
    Activity,
    Plan,
    Traffic,
}
impl SideTab {
    fn name(self) -> &'static str {
        match self {
            Self::Activity => "Activity",
            Self::Plan => "Plan",
            Self::Traffic => "ACP traffic",
        }
    }
}

/// A length in the design's pixels, which scale with the interface text size.
pub(super) fn dp(pixels: f32) -> Pixels {
    fonts::text_pixels(pixels)
}

pub(super) fn island(cx: &App) -> Div {
    let isl = theme::islands(cx);
    row()
        .h_full()
        .flex_shrink_0()
        .items_stretch()
        .rounded(dp(14.))
        .bg(isl.island)
        .overflow_hidden()
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), isl.ring)
                .blur_radius(px(0.))
                .spread_radius(px(1.)),
            BoxShadow::new(px(0.), px(10.), hsla(0., 0., 0., 0.3)).blur_radius(px(30.)),
        ])
}

pub(super) fn rail(
    id: &'static str,
    label: &'static str,
    buttons: Vec<AnyElement>,
    pinned: AnyElement,
) -> Div {
    col()
        .h_full()
        .w(dp(RAIL))
        .flex_shrink_0()
        .items_center()
        .py(dp(14.))
        .child(
            col()
                .id(id)
                .role(Role::TabList)
                .aria_label(label)
                .items_center()
                .gap(dp(4.))
                .children(buttons),
        )
        .child(div().flex_1())
        .child(pinned)
}

pub(super) fn rail_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    icon_path: &str,
    active: bool,
    cx: &App,
) -> Button {
    let isl = theme::islands(cx);
    Button::new(id)
        .ghost()
        .size(dp(38.))
        .p_0()
        .rounded(dp(10.))
        .icon(Icon::default().path(icon_path.to_owned()).size(dp(18.)))
        .toggled(active)
        .accessibility_label(label.into())
        .tooltip(tooltip.into())
        .text_color(if active { isl.fg } else { isl.fg3 })
        .when(active, |button| button.bg(isl.active))
}

fn pinned_toggle(
    id: &'static str,
    label: &'static str,
    open: bool,
    right: bool,
    action: Action,
    cx: &Context<Adeline>,
) -> AnyElement {
    let icon = Icon::default().path("panel-left.svg").size(dp(18.));
    let tooltip = match action {
        Action::ToggleLeftPanel => format!("{label} ({})", conversation_ui::shortcut("Ctrl+B")),
        Action::ToggleSidePanel => {
            format!("{label} ({})", conversation_ui::shortcut("Ctrl+Shift+B"))
        }
        _ => label.to_owned(),
    };
    rail_button(id, label, tooltip, "panel-left.svg", false, cx)
        .icon(if right {
            icon.rotate(Radians(std::f32::consts::PI))
        } else {
            icon
        })
        .toggled(open)
        .on_click(cx.listener(move |app, _, window, cx| app.act(action.clone(), window, cx)))
        .into_any_element()
}

impl Adeline {
    pub(super) fn left_panel_is_open(&self) -> bool {
        self.section == Section::Chats && self.left_panel_open
    }
    pub(super) fn side_panel_is_open(&self) -> bool {
        self.section == Section::Chats && self.side_panel_open
    }

    /// The header: "Mode / project / item", centered over the main area,
    /// with the window controls in the corner above the right island.
    pub(super) fn header(&self, cx: &Context<Self>) -> Stateful<Div> {
        let isl = theme::islands(cx);
        let item = match self.section {
            Section::Docs => self
                .docs_open_doc()
                .map(|(path, _)| path.rsplit('/').next().unwrap_or(path).to_owned()),
            _ => Some(self.selected.map_or_else(
                || "New chat".to_owned(),
                |ix| self.workspace().threads[ix].title.clone(),
            )),
        };
        let slash = || div().flex_shrink_0().text_color(isl.fg3).child("/");
        let mut spoken = vec![project_bar::mode_name(self.section).to_owned()];
        if self.has_open_project() {
            spoken.push(self.workspace().config.name.clone());
            spoken.extend(item.clone());
        }
        let crumbs = row()
            .id("window-title")
            .test_support()
            .role(Role::Group)
            .aria_label(spoken.join(" / "))
            .min_w_0()
            .gap(dp(8.))
            .text_size(dp(theme::T_BODY))
            .text_color(isl.fg2)
            .child(
                div()
                    .flex_shrink_0()
                    .child(project_bar::mode_name(self.section)),
            )
            .when(self.has_open_project(), |crumbs| {
                crumbs
                    .child(slash())
                    .child(
                        div()
                            .flex_shrink_0()
                            .child(self.workspace().config.name.clone()),
                    )
                    .when_some(item, |crumbs, item| {
                        crumbs.child(slash()).child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(isl.fg)
                                .font_weight(FontWeight::MEDIUM)
                                .child(item),
                        )
                    })
            });
        let (left, right) = self.island_widths();
        let title = row()
            .absolute()
            .top(dp(PAD))
            .h(dp(26.))
            .left(dp(PAD * 2. + left))
            .right(dp(PAD * 2. + right))
            .justify_center()
            .child(crumbs);
        let bar = TitleBar::new()
            .h(dp(TOP))
            .when(!cfg!(target_os = "macos"), |bar| bar.pl_0())
            .border_b_0()
            .bg(transparent_black())
            .child(title);
        div()
            .id("window-header")
            .relative()
            .flex_shrink_0()
            .child(titlebar::without_text_selection(bar))
    }

    fn island_widths(&self) -> (f32, f32) {
        let chats = self.section == Section::Chats && self.has_open_project();
        let left = RAIL + if self.left_panel_is_open() { LIST } else { 0. };
        let right = if chats {
            RAIL + if self.side_panel_is_open() { PANE } else { 0. }
        } else if self.section == Section::Docs && self.has_open_project() {
            RAIL
        } else {
            0.
        };
        (left, right)
    }

    pub(super) fn islands(&self, body: AnyElement, window: &Window, cx: &Context<Self>) -> Div {
        let chats = self.section == Section::Chats && self.has_open_project();
        let docs = self.section == Section::Docs && self.has_open_project();
        let isl = theme::islands(cx);
        let list_open = self.left_panel_is_open() && self.has_open_project();
        let mut left = island(cx).child(
            self.mode_rail(cx)
                .when(list_open, |rail| rail.border_r_1().border_color(isl.line)),
        );
        if list_open {
            left = left.child(
                div()
                    .id("conversation-list-panel")
                    .role(Role::Group)
                    .aria_label("Conversation list panel")
                    .w(dp(LIST))
                    .h_full()
                    .child(panel_area("list", self.chat_list.clone(), cx)),
            );
        }
        let right = chats.then(|| {
            let open = self.side_panel_is_open();
            island(cx)
                .when(open, |island| {
                    island.child(div().w(dp(PANE)).h_full().child(panel_area(
                        "side",
                        chat::region(&self.side_panel, chat::full(), window),
                        cx,
                    )))
                })
                .child(
                    self.side_rail(cx)
                        .when(open, |rail| rail.border_l_1().border_color(isl.line)),
                )
        });
        let docs_rail = docs.then(|| island(cx).child(self.docs_rail(cx)));
        row()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .px(dp(PAD))
            .pb(dp(BOTTOM))
            .gap(dp(PAD))
            .child(left)
            .child(div().flex_1().min_w_0().h_full().child(if chats {
                panel_area(CHATS_MAIN, body, cx).into_any_element()
            } else {
                body
            }))
            .children(right)
            .children(docs_rail)
    }

    fn side_rail(&self, cx: &Context<Self>) -> Div {
        let open = self.side_panel_is_open();
        let mut tabs = vec![SideTab::Activity, SideTab::Plan];
        if self.verbose {
            tabs.push(SideTab::Traffic);
        }
        let buttons = tabs
            .into_iter()
            .map(|tab| {
                let (id, label, icon_path, tooltip) = match tab {
                    SideTab::Activity => (
                        "side-activity",
                        "Agent activity",
                        "pulse.svg",
                        "Activity".to_owned(),
                    ),
                    SideTab::Plan => (
                        "side-plan",
                        "Plan",
                        "list-checks.svg",
                        format!("Plan ({})", conversation_ui::shortcut("Ctrl+Shift+T")),
                    ),
                    SideTab::Traffic => (
                        "side-traffic",
                        "ACP traffic",
                        "code.svg",
                        format!(
                            "ACP traffic ({})",
                            conversation_ui::shortcut("Ctrl+Shift+L")
                        ),
                    ),
                };
                rail_button(
                    id,
                    label,
                    tooltip,
                    icon_path,
                    open && self.side_tab == tab,
                    cx,
                )
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.act(Action::PanelTab(tab), window, cx);
                }))
                .into_any_element()
            })
            .collect();
        rail(
            "side-tabs",
            "Side panel",
            buttons,
            pinned_toggle(
                "right-panel",
                "Side panel",
                open,
                true,
                Action::ToggleSidePanel,
                cx,
            ),
        )
    }

    fn docs_rail(&self, cx: &Context<Self>) -> Div {
        let docs = config::current().modes.docs;
        let settings = self.command_popover(
            "mode-settings",
            rail_button(
                "docs-settings",
                "Docs settings",
                "Docs settings",
                "settings.svg",
                false,
                cx,
            ),
            Anchor::TopRight,
            cx,
        );
        rail(
            "docs-tabs",
            "Docs panels",
            vec![settings.into_any_element()],
            pinned_toggle(
                "docs-right-panel",
                "Conversation panel",
                docs.show_conversation_panel,
                true,
                Action::Docs(docs::Act::ChatPanel),
                cx,
            ),
        )
    }

    pub(super) fn mode_rail_toggle(&self, cx: &Context<Self>) -> AnyElement {
        if self.section == Section::Docs {
            pinned_toggle(
                "docs-left-panel",
                "Docs tree",
                config::current().modes.docs.show_left_panel,
                false,
                Action::Docs(docs::Act::LeftPanel),
                cx,
            )
        } else {
            pinned_toggle(
                "left-panel",
                "Chat list",
                self.left_panel_open,
                false,
                Action::ToggleLeftPanel,
                cx,
            )
        }
    }

    /// The pane header: the open tab's name, 46 tall.
    pub(super) fn pane_header(&self, cx: &App) -> Div {
        let isl = theme::islands(cx);
        row()
            .flex_none()
            .h(dp(PANE_HEADER))
            .px(dp(16.))
            .text_size(dp(theme::T_BODY))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(isl.fg)
            .child(self.side_tab.name())
    }

    pub(super) fn waiting_chat(&self) -> bool {
        self.workspace()
            .threads
            .iter()
            .any(|thread| thread.status == ThreadStatus::Blocked)
    }
}

// The panels a text selection can start in. Text outside the one a drag
// started in can't be selected until the button is up: off text, gpui-kit
// carries a drag into whatever text sits nearest above the pointer, in any panel.
pub(super) const CHATS_MAIN: &str = "chats-main";
pub(super) const DOCS_MAIN: &str = "docs-main";
pub(super) const DOCS_CHAT: &str = "docs-chat";

#[derive(Default)]
struct DragPanel(Option<&'static str>);

impl Global for DragPanel {}

/// Whether text in `panel` can be selected now.
pub(super) fn selectable_in(panel: &str, cx: &App) -> bool {
    cx.try_global::<DragPanel>()
        .and_then(|drag| drag.0)
        .is_none_or(|started| started == panel)
}

/// A panel's contents, recording a left-button press in it as the panel a drag started in.
pub(super) fn panel_area(
    id: &'static str,
    content: impl IntoElement,
    cx: &Context<Adeline>,
) -> Div {
    let owner = cx.weak_entity();
    let watch = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            let down = owner.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase.capture() && event.button == MouseButton::Left && hitbox.is_hovered(window)
                {
                    set_drag_panel(Some(id), &down, cx);
                }
            });
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase.capture() {
                    set_drag_panel(None, &owner, cx);
                }
            });
        },
    )
    .absolute()
    .size_full();
    div().relative().size_full().child(watch).child(content)
}

fn set_drag_panel(panel: Option<&'static str>, owner: &WeakEntity<Adeline>, cx: &mut App) {
    if cx.default_global::<DragPanel>().0 == panel {
        return;
    }
    cx.set_global(DragPanel(panel));
    // The transcript's rows are cached; they redraw only when told.
    let _ = owner.update(cx, |app, cx| {
        app.transcript.update(cx, chat::Transcript::refresh_all);
        cx.notify();
    });
}

/// The separator between the resizable panels of every mode.
pub(super) fn fading_divider(
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
