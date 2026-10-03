//! GPUI SVGs require a local color. Resolve inherited text color at paint
//! time so parent hover/selection styles work without freezing a palette value.
use gpui_kit::{
    App, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId, Interactivity,
    LayoutId, Pixels, StyleRefinement, Svg, Window, prelude::*, svg,
};

pub(crate) struct ThemedIcon(Svg);

impl ThemedIcon {
    pub(crate) fn new(name: &str) -> Self {
        Self(svg().path(format!("{name}.svg")))
    }

    /// An icon by its full asset path.
    pub(crate) fn path(path: &str) -> Self {
        Self(svg().path(path.to_owned()))
    }

    fn resolve_color(&mut self, inherited: Hsla) -> Option<Hsla> {
        let color = &mut self.0.style().text.color;
        let explicit = *color;
        *color = Some(explicit.unwrap_or(inherited));
        explicit
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
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.0.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.0.prepaint(id, inspector, bounds, layout, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let explicit = self.resolve_color(window.text_style().color);
        self.0
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
        // Cached elements must inherit afresh on the next frame, including
        // after pointer exit, selection changes, and theme changes.
        self.0.style().text.color = explicit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::rgb;

    #[test]
    fn inherited_icon_color_tracks_states_and_preserves_overrides() {
        let mut icon = ThemedIcon::new("chat");
        for color in [0xeeeeee, 0x123456, 0xffffff, 0xeeeeee] {
            let foreground: Hsla = rgb(color).into();
            let explicit = icon.resolve_color(foreground);
            assert_eq!(icon.style().text.color, Some(foreground));
            assert_eq!(explicit, None);
            icon.style().text.color = explicit;
        }
        let foreground: Hsla = rgb(0xe06030).into();
        let mut icon = ThemedIcon::new("project-circle").text_color(foreground);
        let explicit = icon.resolve_color(rgb(0xffffff).into());
        assert_eq!(explicit, Some(foreground));
        assert_eq!(icon.style().text.color, Some(foreground));
    }
}
