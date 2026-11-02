//! Title bars drawn by Adeline: Kit window controls and Adeline's scaled app icon.
//!
//! The main window draws its own title bar on every platform. The project tabs share it, so
//! Windows and Linux put Kit's caption buttons at its right end and macOS keeps its traffic
//! lights at the left, with the app icon beside them.
use super::*;
use gpui_kit::component::GlobalState;

// 4px of headroom over the 32px project tabs.
pub(super) const MAIN_HEIGHT: Pixels = px(36.);

// Glass draws over the system blur (Acrylic on Windows, vibrancy on macOS). Linux has no
// dependable blur, so its windows stay opaque.
pub(super) fn glass() -> bool {
    !cfg!(target_os = "linux") && config::current().general.appearance.glass
}

// A surface's opacity: `alpha` under Glass, solid otherwise.
pub(super) fn glass_alpha(alpha: f32) -> f32 {
    if glass() { alpha } else { 1. }
}

pub(super) fn background() -> WindowBackgroundAppearance {
    if glass() {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

pub(super) fn apply_background(cx: &mut App) {
    cx.defer(|cx| {
        for window in cx.windows() {
            let _ = window.update(cx, |_, window, _| {
                window.set_background_appearance(background());
            });
        }
    });
}

// Set by `--hidden` (control builds): every window stays off screen for `adeline ui`.
pub(super) struct Hidden;
impl Global for Hidden {}

// A hidden window keeps the platform's default placement until it shows.
pub(super) fn size_hidden(window: &mut Window, bounds: Bounds<Pixels>, cx: &App) {
    if cx.has_global::<Hidden>() {
        window.resize(bounds.size);
    }
}

pub(super) fn main_window_options(cx: &App) -> WindowOptions {
    let hidden = cx.has_global::<Hidden>();
    WindowOptions {
        show: !hidden,
        focus: !hidden,
        window_background: background(),
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

pub(super) fn without_text_selection(bar: impl IntoElement) -> Div {
    // Windows keeps the mouse-up for its window-move loop, so a selection begun on the bar would
    // trail the pointer across the window.
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            GlobalState::suppress_text_selection(cx);
        })
        .child(bar)
}

// Mirrors which caption buttons Kit's `WindowControls` draws at the bar's right end.
pub(super) fn caption_buttons_width(window: &Window) -> Pixels {
    let drawn = !cfg!(target_os = "macos")
        && (!cfg!(target_os = "linux")
            || matches!(window.window_decorations(), Decorations::Client { .. }));
    if !drawn {
        return px(0.);
    }
    let controls = window.window_controls();
    let count = 1 + u8::from(controls.minimize) + u8::from(controls.maximize);
    component::TITLE_BAR_HEIGHT * f32::from(count)
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
#[cfg(target_os = "windows")]
pub(super) fn render(title: String, window: &Window) -> TitleBar {
    TitleBar::new().border_b_0().child(
        row().min_w_0().gap_2().child(app_icon(window)).child(
            div()
                .text_size(panes::dp(theme::T_UI))
                .text_color(rgb(theme::color("muted_text")))
                .truncate()
                .child(title),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::portrait_asset;
    use adeline_assets::embedded;

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
