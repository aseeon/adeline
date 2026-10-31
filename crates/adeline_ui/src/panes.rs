//! The Islands layout (docs/design/DESIGN.md): the window header, the left
//! island (mode rail and chat list), the main area, and the right island (the
//! side pane and its rail). Both rails share `rail` and `rail_button`, so they
//! look and behave the same.
use super::*;
use adeline_core::conversation::ThreadStatus;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(super) const PAD: f32 = 8.;
pub(super) const TOP: f32 = 42.;
pub(super) const BOTTOM: f32 = 21.;
const RAIL: f32 = 56.;
const LIST: f32 = 286.;
const PANE: f32 = 330.;
const CHAT_PANE: f32 = 440.;
// How far the right island's pane can be dragged, and how much it leaves the main area.
const PANE_MIN: f32 = 260.;
const MAIN_MIN: f32 = 320.;
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
            // Shorter than the design's 0 10 30 at 30%: GPUI doesn't dither, and a
            // long faint shadow on a dark window shows wide 8-bit bands.
            BoxShadow::new(px(0.), px(3.), hsla(0., 0., 0., 0.35)).blur_radius(px(10.)),
        ])
}

pub(super) type Slide = (Option<usize>, Option<usize>);

const RAIL_BUTTON: f32 = 38.;
const RAIL_GAP: f32 = 4.;

pub(super) fn rail(
    id: &'static str,
    label: &'static str,
    buttons: Vec<AnyElement>,
    (from, active): Slide,
    pinned: AnyElement,
    cx: &App,
) -> Div {
    let pill = active.map(|to| {
        let at = |index: f32| dp((RAIL_BUTTON + RAIL_GAP) * index);
        let pill = div()
            .absolute()
            .left_0()
            .top(at(to as f32))
            .size(dp(RAIL_BUTTON))
            .rounded(dp(10.))
            .bg(theme::islands(cx).active);
        match from {
            // The active background slides over from the tab active before.
            Some(from) if from != to => pill
                .with_animation(
                    ElementId::Name(format!("{id}-slide-{from}-{to}").into()),
                    Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
                    move |pill, t| pill.top(at(from as f32 + (to as f32 - from as f32) * t)),
                )
                .into_any_element(),
            _ => pill.into_any_element(),
        }
    });
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
                .relative()
                .items_center()
                .gap(dp(RAIL_GAP))
                .children(pill)
                .children(buttons),
        )
        .child(div().flex_1())
        .child(pinned)
}

pub(super) fn rail_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    icon: Icon,
    active: bool,
    cx: &App,
) -> Button {
    let isl = theme::islands(cx);
    Button::new(id)
        .ghost()
        .size(dp(RAIL_BUTTON))
        .p_0()
        .rounded(dp(10.))
        // A child keeps its size, where `.icon` would take the button's.
        .child(icon.size(dp(18.)))
        .toggled(active)
        .accessibility_label(label.into())
        .tooltip(tooltip.into())
        .text_color(if active { isl.fg } else { isl.fg3 })
}

// A 30 square button with a 16 icon, as in island headers.
pub(super) fn icon_button(id: impl Into<ElementId>, icon: Icon, cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .size(dp(30.))
        .p_0()
        .rounded(dp(7.))
        .text_color(theme::islands(cx).fg2)
        .child(icon.size(dp(16.)))
}

fn pinned_toggle(
    id: &'static str,
    label: &'static str,
    open: bool,
    right: bool,
    action: Action,
    cx: &Context<Adeline>,
) -> AnyElement {
    let icon = Icon::default().path("panel-left.svg");
    let tooltip = match action {
        Action::ToggleLeftPanel | Action::Docs(docs::Act::LeftPanel) => {
            format!("{label} ({})", conversation_ui::shortcut("Ctrl+B"))
        }
        Action::ToggleSidePanel | Action::Docs(docs::Act::ChatPanel) => {
            format!("{label} ({})", conversation_ui::shortcut("Ctrl+Shift+B"))
        }
        _ => label.to_owned(),
    };
    let icon = if right {
        icon.rotate(Radians(std::f32::consts::PI))
    } else {
        icon
    };
    rail_button(id, label, tooltip, icon, false, cx)
        .toggled(open)
        .on_click(cx.listener(move |app, _, window, cx| app.act(action.clone(), window, cx)))
        .into_any_element()
}

/// Dragged from the right island's left edge to resize its pane.
pub(super) struct PaneDrag;

struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn resize_handle() -> impl IntoElement {
    div()
        .id("pane-resize")
        .test_support()
        .role(Role::Splitter)
        .aria_label("Resize panel")
        .absolute()
        .top_0()
        .left_0()
        .h_full()
        .w(px(6.))
        .cursor_col_resize()
        // The press belongs to the drag, so it must not also start a text selection.
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            component::GlobalState::suppress_text_selection(cx);
        })
        .on_drag(PaneDrag, |_, _, _, cx| cx.new(|_| DragGhost))
}

impl Adeline {
    pub(super) fn left_panel_is_open(&self) -> bool {
        match self.section {
            Section::Chats => self.left_panel_open,
            Section::Docs => config::current().modes.docs.show_left_panel,
            _ => false,
        }
    }
    pub(super) fn side_panel_is_open(&self) -> bool {
        match self.section {
            Section::Chats => self.side_panel_open,
            Section::Docs => config::current().modes.docs.show_conversation_panel,
            _ => false,
        }
    }

    /// The header: "Mode / project / item", centered over the main area,
    /// with the window controls in the corner above the right island.
    pub(super) fn header(&self, window: &Window, cx: &Context<Self>) -> Stateful<Div> {
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
        let (left, right) = self.island_widths(window);
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

    fn left_width(&self) -> f32 {
        RAIL + if self.left_panel_is_open() { LIST } else { 0. }
    }

    fn island_widths(&self, window: &Window) -> (f32, f32) {
        let chats = self.section == Section::Chats && self.has_open_project();
        let left = self.left_width();
        let docs = self.section == Section::Docs && self.has_open_project();
        let right = if chats || docs {
            RAIL + if self.side_panel_is_open() {
                self.pane_width(window)
            } else {
                0.
            }
        } else {
            0.
        };
        (left, right)
    }

    pub(super) fn islands(
        &self,
        body: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let chats = self.section == Section::Chats && self.has_open_project();
        let docs = self.section == Section::Docs && self.has_open_project();
        let isl = *theme::islands(cx);
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
                    .child(if docs {
                        panel_area("docs-tree", self.docs_sidebar(window, cx), cx)
                    } else {
                        panel_area("list", self.chat_list.clone(), cx)
                    }),
            );
        }
        let right = (chats || docs).then(|| {
            let open = self.side_panel_is_open();
            let rail = if chats {
                self.side_rail(cx)
            } else {
                self.docs_rail(cx)
            };
            island(cx)
                .when(open, |island| {
                    let content = if chats {
                        panel_area(
                            "side",
                            chat::region(&self.side_panel, chat::full(), window),
                            cx,
                        )
                    } else {
                        panel_area(DOCS_CHAT, self.chats(window, cx), cx)
                    };
                    island.child(
                        div()
                            .relative()
                            .w(dp(self.pane_width(window)))
                            .h_full()
                            .child(content)
                            .child(resize_handle()),
                    )
                })
                .child(rail.when(open, |rail| rail.border_l_1().border_color(isl.line)))
        });
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
            } else if docs {
                panel_area(DOCS_MAIN, body, cx).into_any_element()
            } else {
                body
            }))
            .children(right)
            .on_drag_move(cx.listener(|app, event: &DragMoveEvent<PaneDrag>, _, cx| {
                app.drag_pane(event, cx);
            }))
            .on_drop(cx.listener(|app, _: &PaneDrag, _, cx| {
                if let Err(error) = ui_state::record_panes(app.panes) {
                    app.notify_toast(&format!("Could not save the panel width: {error:#}"), cx);
                }
            }))
    }

    /// The right island's pane width: the side pane in Chats, the chat elsewhere.
    /// A saved width gives way so the main area keeps `MAIN_MIN` in a narrow window.
    fn pane_width(&self, window: &Window) -> f32 {
        let saved = if self.section == Section::Chats {
            self.panes.side.unwrap_or(PANE)
        } else {
            self.panes.chat.unwrap_or(CHAT_PANE)
        };
        let room = self.pane_room(f32::from(window.viewport_size().width) / f32::from(dp(1.)));
        saved.min(room).max(PANE_MIN)
    }

    fn pane_room(&self, row: f32) -> f32 {
        row - PAD * 4. - self.left_width() - RAIL - MAIN_MIN
    }

    // The pane's left edge follows the pointer; the rail and the island's right edge stay.
    fn drag_pane(&mut self, event: &DragMoveEvent<PaneDrag>, cx: &mut Context<Self>) {
        let scale = f32::from(dp(1.));
        let row = event.bounds;
        let edge = f32::from(row.right() - event.event.position.x) / scale;
        let room = self.pane_room(f32::from(row.size.width) / scale);
        // At most twice the mode's default width.
        let most = room.min(
            2. * if self.section == Section::Chats {
                PANE
            } else {
                CHAT_PANE
            },
        );
        let width = (edge - PAD - RAIL).clamp(PANE_MIN, most.max(PANE_MIN));
        let pane = if self.section == Section::Chats {
            &mut self.panes.side
        } else {
            &mut self.panes.chat
        };
        if *pane != Some(width) {
            *pane = Some(width);
            cx.notify();
        }
    }

    /// Records `active` as the rail's active tab and returns it with the one before.
    pub(super) fn rail_slide(&self, rail: &'static str, active: Option<usize>) -> Slide {
        let mut slides = self.rail_slides.borrow_mut();
        let slide = slides.entry(rail).or_insert((None, active));
        if slide.1 != active {
            *slide = (slide.1, active);
        }
        *slide
    }

    fn side_rail(&self, cx: &Context<Self>) -> Div {
        let open = self.side_panel_is_open();
        let mut tabs = vec![SideTab::Activity, SideTab::Plan];
        if self.verbose {
            tabs.push(SideTab::Traffic);
        }
        let active = tabs.iter().position(|tab| open && self.side_tab == *tab);
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
                    Icon::default().path(icon_path),
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
            self.rail_slide("side-tabs", active),
            pinned_toggle(
                "right-panel",
                "Side panel",
                open,
                true,
                Action::ToggleSidePanel,
                cx,
            ),
            cx,
        )
    }

    fn docs_rail(&self, cx: &Context<Self>) -> Div {
        let open = self.side_panel_is_open();
        let chat = rail_button(
            "docs-chat-tab",
            "Chat",
            "Chat",
            Icon::default().path("chat.svg"),
            open,
            cx,
        )
        .on_click(cx.listener(move |app, _, window, cx| {
            if !app.side_panel_is_open() {
                app.act(Action::Docs(docs::Act::ChatPanel), window, cx);
            }
        }));
        rail(
            "docs-tabs",
            "Docs panels",
            vec![chat.into_any_element()],
            self.rail_slide("docs-tabs", open.then_some(0)),
            pinned_toggle(
                "docs-right-panel",
                "Conversation panel",
                open,
                true,
                Action::Docs(docs::Act::ChatPanel),
                cx,
            ),
            cx,
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
            let up = owner.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase.capture() {
                    set_drag_panel(None, &up, cx);
                }
            });
            // Windows keeps the mouse up when a press starts a window resize, and a drop
            // stops it, so a move with no button held also ends the drag.
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase.capture() && event.pressed_button.is_none() {
                    base::TextSelection::end(window, cx);
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

// A glide moves its child from where it last drew to its new place when that
// place changes, as the composer does between the main area and the right island.
const GLIDE: Duration = Duration::from_millis(200);

#[derive(Default)]
pub(super) struct GlideState {
    place: Option<bool>,
    origin: Option<Point<Pixels>>,
    from: Option<(Point<Pixels>, Instant)>,
}

pub(super) struct Glide {
    child: Option<AnyElement>,
    place: bool,
    state: Rc<RefCell<GlideState>>,
}

pub(super) fn glide(
    child: impl IntoElement,
    place: bool,
    state: &Rc<RefCell<GlideState>>,
) -> Glide {
    Glide {
        child: Some(child.into_any_element()),
        place,
        state: Rc::clone(state),
    }
}

impl Element for Glide {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let child = self.child.as_mut().expect("laid out once");
        (child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        &mut (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut state = self.state.borrow_mut();
        if state.place != Some(self.place) {
            if let (Some(_), Some(old)) = (state.place, state.origin)
                && !cx.reduce_motion()
            {
                state.from = Some((old - bounds.origin, cx.background_executor().now()));
            }
            state.place = Some(self.place);
        }
        state.origin = Some(bounds.origin);
        // The executor's clock, which tests move by hand.
        let now = cx.background_executor().now();
        let progress = state
            .from
            .map(|(_, start)| (now - start).as_secs_f32() / GLIDE.as_secs_f32());
        let child = self.child.as_mut().expect("prepainted once");
        match (state.from, progress) {
            (Some((offset, _)), Some(t)) if t < 1. => {
                // Deferred and unclipped, so it crosses the islands' edges on its way.
                let shift = offset * (1. - ease_out_quint()(t));
                let child = self.child.take().expect("prepainted once");
                window.defer_draw(child, window.element_offset() + shift, 1, None);
                window.request_animation_frame();
            }
            _ => {
                state.from = None;
                child.prepaint(window, cx);
            }
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        &mut (): &mut (),
        &mut (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = self.child.as_mut() {
            child.paint(window, cx);
        }
    }
}

impl IntoElement for Glide {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}
