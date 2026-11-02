//! Bundled interface font, the font families available to GPUI this session,
//! and the fonts and text sizes the settings resolve to.
use adeline_core::config::{self, DEFAULT_CODE_FONT as CODE_DEFAULT, DEFAULT_FONT as DEFAULT};
use gpui_kit::base::RootPlugin;
use gpui_kit::{
    App, Context, Div, Empty, FontFeatures, IntoElement, Refineable, Render, Stateful,
    StyleRefinement, Styled, Window,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::Arc;

pub const LICENSE: &str = include_str!("../../../assets/fonts/ChivoMono-OFL.txt");

thread_local! {
    static FAMILIES: RefCell<Vec<String>> = RefCell::new(vec![DEFAULT.into(), CODE_DEFAULT.into()]);
}

pub fn init(cx: &App) {
    // Register before any window shapes text. The font travels inside the binary.
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../../../assets/fonts/Chivo.ttf")),
            Cow::Borrowed(include_bytes!("../../../assets/fonts/ChivoMono.ttf")),
        ])
        .expect("load bundled Chivo fonts");
    refresh(cx);
}

pub fn refresh(cx: &App) {
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

// Coding fonts draw `->` and `!=` through `calt`, and pairs like the `ff` in `diff` through
// `liga`. Turning both off on every window's root reaches all code text, inline code, Kit
// code blocks and overlays included. Inline code has no font features of its own, so the
// interface font loses them too.
pub struct Ligatures;
impl Render for Ligatures {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}
impl RootPlugin for Ligatures {
    fn style(&self, surface: &mut Stateful<Div>, _: &mut Window, _: &mut App) {
        if !config::with(|s| s.general.appearance.ligatures) {
            let off = FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0)]));
            surface
                .style()
                .refine(&StyleRefinement::default().font_features(off));
        }
    }
}

// Virtual rows must be measured again when any of these changes.
#[derive(PartialEq)]
pub struct Typography {
    interface: String,
    interface_size: u16,
    code: String,
    code_size: u16,
    ligatures: bool,
}

pub fn typography() -> Typography {
    Typography {
        interface: interface(),
        interface_size: config::font_size(),
        code: code(),
        code_size: config::code_font_size(),
        ligatures: config::with(|s| s.general.appearance.ligatures),
    }
}
pub fn text_pixels(base: f32) -> gpui_kit::Pixels {
    gpui_kit::px(base * f32::from(config::font_size()) / 14.)
}

#[cfg(test)]
mod tests {
    use super::*;

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
