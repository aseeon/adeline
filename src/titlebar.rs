//! Windows client-drawn chrome using GPUI's native non-client hit testing.
use super::*;

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
pub(super) fn render(title: String, window: &Window) -> Div {
    let foreground = theme::foreground();
    // Keep resize space above the draggable caption, while the control
    // backgrounds and hit regions extend all the way to the top edge.
    col()
        .w_full()
        .h(px(36.))
        .flex_shrink_0()
        .bg(rgb(theme::background()))
        .child(
            row()
                .h_full()
                .child(
                    row()
                        .id("window-caption")
                        .window_control_area(WindowControlArea::Drag)
                        .flex_1()
                        .min_w_0()
                        .h(px(32.))
                        .mt(px(4.))
                        .px(px(16.))
                        .gap_2()
                        .child(
                            img(ImageSource::Resource(Resource::Embedded(
                                portrait_asset(window.scale_factor()).into(),
                            )))
                            .size(px(24.))
                            .flex_shrink_0(),
                        )
                        .child(text(title, 12., theme::muted_foreground()).truncate()),
                )
                .child(caption_button(
                    "window-minimize",
                    "minus",
                    WindowControlArea::Min,
                    foreground,
                ))
                .child(caption_button(
                    "window-maximize",
                    if window.is_maximized() {
                        "copy"
                    } else {
                        "square"
                    },
                    WindowControlArea::Max,
                    foreground,
                ))
                .child(caption_button(
                    "window-close",
                    "close",
                    WindowControlArea::Close,
                    foreground,
                )),
        )
}

fn caption_button(
    id: &'static str,
    glyph: &'static str,
    area: WindowControlArea,
    foreground: u32,
) -> Stateful<Div> {
    let close = matches!(area, WindowControlArea::Close);
    row()
        .id(id)
        .group(id)
        .occlude()
        .w(px(44.))
        .h_full()
        .flex_shrink_0()
        .justify_center()
        .text_color(rgb(foreground))
        .hover(move |s| {
            s.bg(rgb(if close {
                theme::destructive()
            } else {
                theme::sidebar_accent()
            }))
            .text_color(rgb(if close {
                theme::destructive_foreground()
            } else {
                theme::sidebar_foreground()
            }))
        })
        .active(move |s| {
            s.bg(rgb(if close {
                theme::destructive()
            } else {
                theme::secondary()
            }))
            .text_color(rgb(if close {
                theme::destructive_foreground()
            } else {
                theme::sidebar_foreground()
            }))
        })
        // Intentionally no on_click: GPUI forwards these regions to the native
        // Windows caption machinery, including maximize hover/Snap Layouts.
        .window_control_area(area)
        .child(
            svg()
                .path(format!("{glyph}.svg"))
                .size(px(14.))
                .text_color(rgb(foreground))
                .group_hover(id, move |s| {
                    s.text_color(rgb(if close {
                        theme::destructive_foreground()
                    } else {
                        theme::sidebar_foreground()
                    }))
                })
                .flex_shrink_0(),
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
