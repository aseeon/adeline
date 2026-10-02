//! Title bars drawn by Adeline: Kit window controls and Adeline's scaled app icon.
//!
//! The main window draws its own title bar on every platform. The project tabs share it, so
//! Windows and Linux put Kit's caption buttons at its right end and macOS keeps its traffic
//! lights at the left, with the app icon beside them.
use super::*;
use gpui_kit::component::{GlobalState, TitleBar};

/// Height of the main window's title bar: 4px of headroom over the 32px project tabs.
pub(super) const MAIN_HEIGHT: Pixels = px(36.);

/// Opacity of the main window's chrome over the system blur (Acrylic on Windows, vibrancy on
/// macOS). Linux has no dependable blur, so its chrome stays opaque.
pub(super) const GLASS: f32 = if cfg!(target_os = "linux") { 1. } else { 0.7 };

/// Options for a window that draws the unified title bar, so the title bar owns dragging.
pub(super) fn main_window_options() -> WindowOptions {
    WindowOptions {
        window_background: if cfg!(target_os = "linux") {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::Blurred
        },
        titlebar: Some(TitlebarOptions {
            title: Some("Adeline".into()),
            appears_transparent: true,
            // Centre the traffic lights on the project tabs, which fill the bar's bottom 32px.
            traffic_light_position: Some(point(px(9.), px(12.))),
        }),
        // Linux asks the window manager to drop its frame; Kit keeps the frame and hides its own
        // caption buttons when the session can't draw client-side decorations.
        window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
        ..TitleBar::window_options()
    }
}

/// Wraps a title bar so pressing it never starts text selection. Windows keeps the mouse-up for
/// its window-move loop, so a selection begun there would trail the pointer across the window.
pub(super) fn without_text_selection(bar: impl IntoElement) -> Div {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            GlobalState::suppress_text_selection(cx);
        })
        .child(bar)
}

// Match the raster to the monitor's physical pixels instead of shrinking a large PNG.
fn portrait_asset(scale_factor: f32) -> String {
    let physical_size = 24. * scale_factor;
    let size = (24_u16..=120)
        .min_by(|a, b| {
            (f32::from(*a) - physical_size)
                .abs()
                .total_cmp(&(f32::from(*b) - physical_size).abs())
        })
        .expect("title-bar icon sizes");
    format!("adeline-titlebar-{size}.png")
}
pub(super) fn app_icon(window: &Window) -> Img {
    img(ImageSource::Resource(Resource::Embedded(
        portrait_asset(window.scale_factor()).into(),
    )))
    .size(px(24.))
    .flex_shrink_0()
}
/// Title bar for secondary windows, which Windows draws itself and other platforms leave native.
#[cfg(target_os = "windows")]
pub(super) fn render(title: String, window: &Window) -> TitleBar {
    TitleBar::new().border_b_0().child(
        row().min_w_0().gap_2().child(app_icon(window)).child(
            div()
                .text_sm()
                .text_color(rgb(theme::muted_foreground()))
                .truncate()
                .child(title),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::portrait_asset;
    use crate::embedded;

    #[test]
    fn portrait_raster_matches_display_pixels() {
        for (scale, expected) in [
            (1., 24_u32),
            (1.25, 30),
            (1.5, 36),
            (1.75, 42),
            (2., 48),
            (3., 72),
            (5., 120),
        ] {
            let name = portrait_asset(scale);
            let png = embedded(&name).expect("embedded title-bar raster");
            let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
            assert_eq!((width, height), (expected, expected), "scale {scale}");
        }
    }
}
