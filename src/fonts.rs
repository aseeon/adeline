//! Bundled interface font and the font families available to GPUI this session.
use std::{borrow::Cow, cell::RefCell};

pub const DEFAULT: &str = "Chivo";
pub const CODE_DEFAULT: &str = "Chivo Mono";
pub const LICENSE: &str = include_str!("../assets/fonts/ChivoMono-OFL.txt");

thread_local! {
    static FAMILIES: RefCell<Vec<String>> = RefCell::new(vec![DEFAULT.into(), CODE_DEFAULT.into()]);
}

pub fn init(cx: &gpui::App) {
    // Register before any window shapes text. The font travels inside the binary.
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Chivo.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/ChivoMono.ttf")),
        ])
        .expect("load bundled Chivo fonts");
    refresh(cx);
}

pub fn refresh(cx: &gpui::App) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_font_falls_back_without_changing_the_preference() {
        let requested = "Example Mono";
        assert_eq!(
            resolve_from(requested, &catalogue(vec![requested.into()]), DEFAULT),
            requested
        );
        assert_eq!(
            resolve_from(requested, &catalogue(vec![]), DEFAULT),
            DEFAULT
        );
        assert_eq!(
            resolve_from(requested, &catalogue(vec![requested.into()]), DEFAULT),
            requested
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
