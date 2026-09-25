//! Quiet overlay scrollbars shared by ordinary panels and virtual chat lists.
use super::*;
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
enum Target {
    Panel(ScrollHandle),
    List(ListState),
}

#[derive(Clone)]
pub(super) struct Scrollbar {
    target: Target,
    drag: Rc<Cell<Option<(f32, f32)>>>,
}

#[derive(Clone, Default)]
pub(super) struct PanelScroll {
    pub handle: ScrollHandle,
    drag: Rc<Cell<Option<(f32, f32)>>>,
}

impl PanelScroll {
    pub fn wrap(&self, id: &'static str, content: impl IntoElement) -> Div {
        let bar = Scrollbar {
            target: Target::Panel(self.handle.clone()),
            drag: self.drag.clone(),
        };
        div()
            .relative()
            .size_full()
            .min_h_0()
            .child(
                col()
                    .id(id)
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.handle)
                    .child(div().w_full().flex_shrink_0().child(content)),
            )
            .child(bar.element())
    }
}

impl Scrollbar {
    pub fn list(state: ListState) -> Self {
        Self {
            target: Target::List(state),
            drag: Default::default(),
        }
    }

    fn metrics(&self) -> (f32, f32) {
        match &self.target {
            Target::Panel(h) => (h.max_offset().height.into(), -f32::from(h.offset().y)),
            Target::List(h) => (
                h.max_offset_for_scrollbar().height.into(),
                -f32::from(h.scroll_px_offset_for_scrollbar().y),
            ),
        }
    }

    fn set(&self, offset: f32) {
        let point = point(px(0.), px(-offset));
        match &self.target {
            Target::Panel(h) => h.set_offset(point),
            Target::List(h) => h.set_offset_from_scrollbar(point),
        }
    }

    pub fn element(&self) -> impl IntoElement {
        let bar = self.clone();
        canvas(
            move |bounds, window, _| {
                // Read after the sibling content's layout, including its first frame.
                let (max, offset) = bar.metrics();
                thumb(f32::from(bounds.size.height), max, offset).map(|(top, height)| {
                    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                    (bar, hitbox, top, height, max)
                })
            },
            |bounds, state, window, _| {
                let Some((bar, hitbox, top, height, max)) = state else {
                    return;
                };
                let hovered = hitbox.is_hovered(window) || bar.drag.get().is_some();
                let thumb_bounds = Bounds::new(
                    point(bounds.left() + px(3.), bounds.top() + px(top)),
                    size(px(4.), px(height)),
                );
                window.paint_quad(
                    fill(
                        thumb_bounds,
                        rgb(if hovered {
                            theme::muted_foreground()
                        } else {
                            theme::border()
                        }),
                    )
                    .corner_radii(px(2.)),
                );
                let travel = f32::from(bounds.size.height) - height;
                let down = bar.clone();
                let view = window.current_view();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase.bubble() && e.button == MouseButton::Left && hitbox.is_hovered(window)
                    {
                        if let Target::List(h) = &down.target {
                            h.scrollbar_drag_started();
                        }
                        let y = f32::from(e.position.y - bounds.top());
                        let offset = if y >= top && y <= top + height {
                            down.metrics().1
                        } else {
                            ((y - height / 2.) / travel * max).clamp(0., max)
                        };
                        down.set(offset);
                        down.drag.set(Some((f32::from(e.position.y), offset)));
                        window.prevent_default();
                        cx.stop_propagation();
                        cx.notify(view);
                    }
                });
                let moving = bar.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase.bubble() {
                        if let Some((start, offset)) = moving.drag.get() {
                            moving.set(
                                (offset + (f32::from(e.position.y) - start) / travel * max)
                                    .clamp(0., max),
                            );
                            cx.notify(view);
                            cx.stop_propagation();
                        } else if bounds.contains(&e.position) || hovered {
                            window.refresh();
                        }
                    }
                });
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase.bubble() && e.button == MouseButton::Left && bar.drag.take().is_some()
                    {
                        if let Target::List(h) = &bar.target {
                            h.scrollbar_drag_ended();
                        }
                        cx.notify(view);
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .top(px(3.))
        .bottom(px(3.))
        .right(px(3.))
        .w(px(10.))
    }
}

fn thumb(viewport: f32, max: f32, offset: f32) -> Option<(f32, f32)> {
    if viewport <= 1. || max <= 0.5 {
        return None;
    }
    let height = (viewport * viewport / (viewport + max))
        .max(24.)
        .min(viewport - 1.);
    Some(((viewport - height) * (offset / max).clamp(0., 1.), height))
}

#[cfg(test)]
mod tests {
    use super::thumb;
    #[test]
    fn only_overflowing_panels_show_a_thumb() {
        assert_eq!(thumb(400., 0., 0.), None);
        assert_eq!(thumb(0., 100., 0.), None);
        assert_eq!(thumb(400., 400., 0.), Some((0., 200.)));
        assert_eq!(thumb(400., 400., 400.), Some((200., 200.)));
    }
    #[test]
    #[expect(clippy::float_cmp, reason = "these thumb positions are exact in f32")]
    fn thumb_stays_usable_and_inside_short_or_long_panels() {
        let (top, height) = thumb(100., 100_000., 200_000.).unwrap();
        assert_eq!(height, 24.);
        assert_eq!(top + height, 100.);
        let (top, height) = thumb(12., 100., -50.).unwrap();
        assert_eq!(top, 0.);
        assert!(height < 12.);
    }
}
