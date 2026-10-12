//! Bundled interface font, the font families available to GPUI this session,
//! and the fonts and text sizes the settings resolve to.
use adeline_core::config::{self, DEFAULT_CODE_FONT as CODE_DEFAULT, DEFAULT_FONT as DEFAULT};
use std::borrow::Cow;
use std::cell::RefCell;

pub const LICENSE: &str = include_str!("../../../assets/fonts/ChivoMono-OFL.txt");

thread_local! {
    static FAMILIES: RefCell<Vec<String>> = RefCell::new(vec![DEFAULT.into(), CODE_DEFAULT.into()]);
}

pub fn init(cx: &gpui_kit::App) {
    // Register before any window shapes text. The font travels inside the binary.
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../../../assets/fonts/Chivo.ttf")),
            Cow::Borrowed(include_bytes!("../../../assets/fonts/ChivoMono.ttf")),
        ])
        .expect("load bundled Chivo fonts");
    refresh(cx);
}

pub fn refresh(cx: &gpui_kit::App) {
    let families = catalogue(cx.text_system().all_font_names());
    FAMILIES.with(|active| *active.borrow_mut() = families);
}

fn catalogue(mut names: Vec<String>) -> Vec<String> {
    names.retain(|name| {
        !name.trim().is_empty()
            && !name.starts_with('.')
            && !name.eq_ignore_ascii_case(DEFAULT)
            && !name.eq_ignore_ascii_case(CODE_DEFAULT)
    });
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names.insert(0, CODE_DEFAULT.into());
    names.insert(0, DEFAULT.into());
    names
}

pub fn families() -> Vec<String> {
    FAMILIES.with(|active| active.borrow().clone())
}

fn resolve_from(requested: &str, families: &[String], fallback: &str) -> String {
    families
        .iter()
        .find(|name| name.eq_ignore_ascii_case(requested.trim()))
        .map_or_else(|| fallback.into(), Clone::clone)
}

pub fn resolve(requested: &str, fallback: &str) -> String {
    FAMILIES.with(|active| resolve_from(requested, &active.borrow(), fallback))
}

pub fn interface() -> String {
    config::with(|s| resolve(&s.general.appearance.interface_font, DEFAULT))
}
pub fn code() -> String {
    config::with(|s| resolve(&s.general.appearance.code_font, CODE_DEFAULT))
}

// Virtual rows must be measured again when any of these changes.
#[derive(PartialEq)]
pub struct Typography {
    interface: String,
    interface_size: u16,
    code: String,
    code_size: u16,
}

pub fn typography() -> Typography {
    Typography {
        interface: interface(),
        interface_size: config::font_size(),
        code: code(),
        code_size: config::code_font_size(),
    }
}
pub fn text_pixels(base: f32) -> gpui_kit::Pixels {
    gpui_kit::px(base * f32::from(config::font_size()) / 14.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_font_falls_back() {
        let requested = "Example Mono";
        assert_eq!(
            resolve_from(requested, &catalogue(vec![requested.into()]), DEFAULT),
            requested
        );
        assert_eq!(
            resolve_from(requested, &catalogue(vec![]), DEFAULT),
            DEFAULT
        );
    }

    #[test]
    fn default_legacy_empty_and_unknown_fonts_use_the_bundle() {
        let names = catalogue(vec!["Arial".into()]);
        for requested in [DEFAULT, "System", "", "Missing Font"] {
            assert_eq!(resolve_from(requested, &names, DEFAULT), DEFAULT);
        }
        assert_eq!(resolve_from(" arial ", &names, DEFAULT), "Arial");
    }

    #[test]
    fn text_scales_with_the_font_size() {
        let size = f32::from(config::font_size());
        assert_eq!(text_pixels(14.), gpui_kit::px(size));
        assert_eq!(text_pixels(21.), gpui_kit::px(size * 1.5));
    }

    #[test]
    fn catalogue_includes_bundle_once_and_sorts_system_families() {
        assert_eq!(
            catalogue(vec![
                "Zebra".into(),
                "Arial".into(),
                "arial".into(),
                DEFAULT.into(),
                ".SystemUIFont".into(),
                String::new()
            ]),
            vec![DEFAULT, CODE_DEFAULT, "Arial", "Zebra"]
        );
    }
}
