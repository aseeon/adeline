//! GPUI SVGs require a local color. Resolve inherited text color at paint
//! time so parent hover/selection styles work without freezing a palette value.
use gpui_kit::{
    App, Bounds, ElementId, GlobalElementId, Hsla, InspectorElementId, Interactivity, LayoutId,
    Pixels, StyleRefinement, Svg, Window, prelude::*, svg,
};

pub(crate) struct ThemedIcon(Svg);

impl ThemedIcon {
    pub(crate) fn new(name: &str) -> Self {
        Self(svg().path(format!("{name}.svg")))
    }

    pub(crate) fn path(path: &str) -> Self {
        Self(svg().path(path.to_owned()))
    }

    // Cached elements must inherit afresh on the next frame, including
    // after pointer exit, selection changes, and theme changes.
    fn with_color(&mut self, inherited: Hsla, paint: impl FnOnce(&mut Svg)) {
        let explicit = self.0.style().text.color;
        self.0.style().text.color = Some(explicit.unwrap_or(inherited));
        paint(&mut self.0);
        self.0.style().text.color = explicit;
    }
}

impl Styled for ThemedIcon {
    fn style(&mut self) -> &mut StyleRefinement {
        self.0.style()
    }
}

impl InteractiveElement for ThemedIcon {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.0.interactivity()
    }
}

impl IntoElement for ThemedIcon {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ThemedIcon {
    type RequestLayoutState = <Svg as Element>::RequestLayoutState;
    type PrepaintState = <Svg as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.0)
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.0.source_location()
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.0.request_layout(id, inspector_id, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.0
            .prepaint(id, inspector_id, bounds, request_layout, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let inherited = window.text_style().color;
        self.with_color(inherited, |svg| {
            svg.paint(
                id,
                inspector_id,
                bounds,
                request_layout,
                prepaint,
                window,
                cx,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::rgb;

    #[test]
    fn inherited_icon_color_tracks_states_and_preserves_overrides() {
        let painted = |icon: &mut ThemedIcon, inherited: u32| {
            let mut seen = None;
            icon.with_color(rgb(inherited).into(), |svg| seen = svg.style().text.color);
            seen
        };
        let mut icon = ThemedIcon::new("chat");
        // Hover, a theme change, then pointer exit: each frame takes the parent's color.
        for color in [0xeeeeee, 0x123456, 0xffffff, 0xeeeeee] {
            assert_eq!(painted(&mut icon, color), Some(rgb(color).into()));
            assert_eq!(icon.style().text.color, None);
        }
        let own: Hsla = rgb(0xe06030).into();
        let mut icon = ThemedIcon::new("project-circle").text_color(own);
        assert_eq!(painted(&mut icon, 0xffffff), Some(own));
        assert_eq!(icon.style().text.color, Some(own));
    }
}
