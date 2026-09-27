//! Windows title bar with Kit window controls and Adeline's scaled app icon.
use super::*;
use gpui_kit::component::TitleBar;

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
pub(super) fn render(title: String, window: &Window) -> TitleBar {
    TitleBar::new().child(
        row()
            .min_w_0()
            .gap_2()
            .child(
                img(ImageSource::Resource(Resource::Embedded(
                    portrait_asset(window.scale_factor()).into(),
                )))
                .size(px(24.))
                .flex_shrink_0(),
            )
            .child(
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
