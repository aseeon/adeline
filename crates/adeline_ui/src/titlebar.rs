//! Title bars drawn by Adeline: Kit window controls and Adeline's scaled app icon.
//!
//! The main window draws its own title bar on every platform. The project tabs share it, so
//! Windows and Linux put Kit's caption buttons at its right end and macOS keeps its traffic
//! lights at the left, with the app icon beside them.
use super::*;
use gpui_kit::component::GlobalState;

pub(super) const MAIN_HEIGHT: Pixels = px(36.);

pub(super) struct Hidden;
impl Global for Hidden {}

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
        titlebar: Some(TitlebarOptions {
            title: Some("Adeline".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(12.))),
        }),
        window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
        ..TitleBar::window_options()
    }
}

pub(super) fn without_text_selection(bar: impl IntoElement) -> Div {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            GlobalState::suppress_text_selection(cx);
        })
        .child(bar)
}

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
