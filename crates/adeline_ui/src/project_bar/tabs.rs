//! Fitting project tabs into the title bar. Each tab has a full and a short
//! width, and quiet tabs step down first; the active tab never does.
use super::*;

// In rem, matching `project_cell`: padding, mark, gap and close slot. A name
// adds its own gap and is capped in width, and an attention dot adds the same
// gap and its size.
const TAB_CHROME: f32 = 0.625 + 1.125 + 0.5 + 1.5 + 0.375;

const TAB_NAME_GAP: f32 = 0.5;

pub(super) const ATTENTION_DOT: f32 = 0.375;

pub(super) const TAB_NAME_MAX: f32 = 11.25;

pub(super) const NEW_TAB_WIDTH: f32 = 2.25;

pub(super) const OVERFLOW_WIDTH: f32 = 4.25;

pub(crate) struct FitTab {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) active: bool,
    pub(super) busy: bool,
    pub(super) dot: bool,
}

struct Measured {
    widths: [Pixels; 3],
    active: bool,
    busy: bool,
}

// Measured each frame from the strip's width and applied on the next.
#[derive(Default)]
pub(crate) struct TabFit {
    sizes: RefCell<Vec<(String, TabSize)>>,
}

fn fit_tabs(tabs: &[Measured], room: Pixels, more: Pixels) -> Vec<TabSize> {
    let fits = |sizes: &[TabSize]| {
        let mut total = Pixels::ZERO;
        for (tab, size) in tabs.iter().zip(sizes) {
            total += match size {
                TabSize::Hidden => continue,
                size => tab.widths[*size as usize],
            };
        }
        if sizes.contains(&TabSize::Hidden) {
            total += more;
        }
        total <= room
    };
    let mut sizes = vec![TabSize::Full; tabs.len()];
    for step in [TabSize::Short, TabSize::Letter, TabSize::Hidden] {
        for busy in [false, true] {
            for ix in (0..tabs.len()).rev() {
                if fits(&sizes) {
                    return sizes;
                }
                if !tabs[ix].active && tabs[ix].busy == busy {
                    sizes[ix] = step;
                }
            }
        }
    }
    sizes
}

pub(super) fn short_name(name: &str) -> String {
    if name.chars().count() <= 4 {
        return name.to_owned();
    }
    format!("{}…", name.chars().take(3).collect::<String>())
}

impl TabFit {
    pub(super) fn size(&self, id: &str) -> TabSize {
        self.sizes
            .borrow()
            .iter()
            .find(|(tab, _)| tab == id)
            .map_or(TabSize::Full, |(_, size)| *size)
    }

    pub(super) fn measure(
        &self,
        tabs: &[FitTab],
        available: Pixels,
        bar: EntityId,
        window: &mut Window,
        cx: &mut App,
    ) {
        if available <= Pixels::ZERO {
            return;
        }
        let rem = window.rem_size();
        let font_size = rems(0.875).to_pixels(rem);
        let style = window.text_style();
        let chrome = |dot: bool| {
            let dot = if dot {
                TAB_NAME_GAP + ATTENTION_DOT
            } else {
                0.
            };
            rems(TAB_CHROME + dot).to_pixels(rem) + px(1.)
        };
        let named = |label: String, dot: bool| {
            let run = style.to_run(label.len());
            let text = window
                .text_system()
                .shape_line(label.into(), font_size, &[run], None)
                .width;
            chrome(dot)
                + rems(TAB_NAME_GAP).to_pixels(rem)
                + text.min(rems(TAB_NAME_MAX).to_pixels(rem))
        };
        let measured: Vec<Measured> = tabs
            .iter()
            .map(|tab| Measured {
                widths: [
                    named(tab.name.clone(), tab.dot),
                    named(short_name(&tab.name), tab.dot),
                    chrome(tab.dot),
                ],
                active: tab.active,
                busy: tab.busy,
            })
            .collect();
        let room = available - rems(NEW_TAB_WIDTH).to_pixels(rem) - px(1.);
        let more = rems(OVERFLOW_WIDTH).to_pixels(rem) + px(1.);
        let sizes: Vec<_> = tabs
            .iter()
            .map(|tab| tab.id.clone())
            .zip(fit_tabs(&measured, room, more))
            .collect();
        if *self.sizes.borrow() != sizes {
            *self.sizes.borrow_mut() = sizes;
            // The bar is a cached view, so a refresh alone would replay it with
            // the old sizes, and a notify during the draw is dropped.
            cx.defer(move |cx| cx.notify(bar));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Measured, TabSize, fit_tabs};
    use gpui_kit::px;

    fn tab(full: f32, short: f32, active: bool, busy: bool) -> Measured {
        Measured {
            widths: [px(full), px(short), px(40.)],
            active,
            busy,
        }
    }

    #[test]
    fn tabs_step_down_quiet_first_and_never_the_active_one() {
        use TabSize::*;
        // Active, busy, quiet, quiet.
        let tabs = [
            tab(120., 70., true, false),
            tab(120., 70., false, true),
            tab(120., 70., false, false),
            tab(120., 70., false, false),
        ];
        let fit = |room: f32| fit_tabs(&tabs, px(room), px(50.));
        assert_eq!(fit(480.), [Full, Full, Full, Full]);
        assert_eq!(fit(430.), [Full, Full, Full, Short]);
        assert_eq!(fit(380.), [Full, Full, Short, Short]);
        assert_eq!(fit(330.), [Full, Short, Short, Short]);
        assert_eq!(fit(300.), [Full, Short, Short, Letter]);
        assert_eq!(fit(270.), [Full, Short, Letter, Letter]);
        assert_eq!(fit(240.), [Full, Letter, Letter, Letter]);
        // The overflow cell is wider than one letter tab, so two go at once.
        assert_eq!(fit(230.), [Full, Letter, Hidden, Hidden]);
        assert_eq!(fit(210.), [Full, Letter, Hidden, Hidden]);
        assert_eq!(fit(10.), [Full, Hidden, Hidden, Hidden]);
    }
}
