//! Windows client-drawn chrome using GPUI's native non-client hit testing.
//! See `docs/WINDOWS_TITLEBAR.md` for the Zed implementation this follows.
use super::*;

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
                        .child(icon("logo").text_color(rgb(foreground)))
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
