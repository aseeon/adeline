//! Application themes. Twelve-color YAML files (`docs/design/new_color_themes.md`)
//! project onto Kit's global theme for every window. Borders and hovers are
//! white ink in dark themes and black ink in light ones. Preferences stay separate from workspace data.
//! `apply` runs after Kit's initialization and after every appearance change.
use anyhow::{Context as _, Result, bail};
use gpui_kit::{
    App, Hsla,
    component::{Colorize as _, Theme as KitTheme, ThemeMode, highlighter::HighlightTheme},
    px, rgb,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap, path::Path};

use adeline_assets::BUNDLED_THEMES;

fn seed_bundled_themes(directory: &Path) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    for &(name, contents) in BUNDLED_THEMES {
        let theme: ThemeFile =
            serde_yaml_ng::from_str(contents).with_context(|| format!("Bundled theme {name}"))?;
        theme
            .validate()
            .with_context(|| format!("Bundled theme {name}"))?;
        let path = directory.join(name);
        // Bundled files from before the 12-color format no longer load.
        if std::fs::read_to_string(&path).is_ok_and(|text| text.contains("sidebar_ring:")) {
            std::fs::write(&path, contents)?;
        }
        adeline_core::files::seed(&path, contents.as_bytes())?;
    }
    Ok(())
}

pub const TOKENS: &[(&str, &str)] = &[
    ("background", "Background"),
    ("background_dark", "Dark background"),
    ("background_light", "Light background"),
    ("main_text", "Main text"),
    ("muted_text", "Muted text"),
    ("fail", "Fail"),
    ("attention", "Attention"),
    ("warning", "Warning"),
    ("success", "Success"),
    ("inline_highlight", "Inline highlight"),
    ("button_highlight", "Button highlight"),
    ("effects", "Effects"),
];
pub fn color(token: &str) -> u32 {
    ACTIVE.with(|s| s.borrow().colors[token])
}
pub fn hsla(token: &str) -> Hsla {
    rgb(color(token)).into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Brightness {
    #[serde(rename = "Very Dark")]
    VeryDark,
    Dark,
    #[serde(alias = "Very Light")]
    Light,
}
impl Brightness {
    pub fn label(&self) -> &'static str {
        match self {
            Self::VeryDark => "Very Dark",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub fn is_dark(&self) -> bool {
        matches!(self, Self::VeryDark | Self::Dark)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    pub name: String,
    pub brightness: Brightness,
    pub colors: BTreeMap<String, String>,
}
impl ThemeFile {
    pub fn builtin() -> Self {
        serde_yaml_ng::from_str(include_str!("../../../bundled-themes/islands.yml"))
            .expect("valid bundled Islands theme")
    }
    fn validate(&self) -> Result<BTreeMap<&'static str, u32>> {
        if self.name.trim().is_empty() {
            bail!("Theme name cannot be empty.");
        }
        if self.colors.len() != TOKENS.len() {
            bail!("A theme must contain exactly the 12 named colors.");
        }
        TOKENS
            .iter()
            .map(|&(key, label)| {
                self.colors
                    .get(key)
                    .and_then(|v| parse_hex(v))
                    .map(|v| (key, v))
                    .with_context(|| format!("{label}: expected a six-digit hex color."))
            })
            .collect()
    }
}
#[derive(Clone)]
pub struct ThemeChoice {
    pub file: String,
    pub name: String,
    pub brightness: Brightness,
}
impl gpui_kit::component::searchable_list::SearchableListItem for ThemeChoice {
    type Value = String;

    fn title(&self) -> gpui_kit::SharedString {
        format!(
            "{} ({}) · {}",
            self.name,
            self.brightness.label(),
            self.file
        )
        .into()
    }

    fn value(&self) -> &String {
        &self.file
    }
}
struct State {
    theme: ThemeFile,
    colors: BTreeMap<&'static str, u32>,
    load_error: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        let theme = ThemeFile::builtin();
        Self {
            colors: theme.validate().unwrap(),
            theme,
            load_error: None,
        }
    }
}
impl State {
    fn fallback(error: &anyhow::Error) -> Self {
        Self {
            load_error: Some(format!("{error:#} Using Islands until this is fixed.")),
            ..Self::default()
        }
    }
}
thread_local! { static ACTIVE: RefCell<State> = RefCell::new(State::default()); }

pub fn load_error() -> Option<String> {
    ACTIVE
        .with(|s| s.borrow().load_error.clone())
        .or_else(adeline_core::config::error)
}

pub fn parse_hex(value: &str) -> Option<u32> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    (value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
}
fn read(path: &Path) -> Result<State> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let theme: ThemeFile =
        serde_yaml_ng::from_str(&text).with_context(|| path.display().to_string())?;
    let colors = theme
        .validate()
        .with_context(|| path.display().to_string())?;
    Ok(State {
        theme,
        colors,
        load_error: None,
    })
}
fn is_yaml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
}
fn theme_path(file: &str) -> Result<std::path::PathBuf> {
    let path = Path::new(file);
    if file.contains(['/', '\\', ':'])
        || path.components().count() != 1
        || !matches!(
            path.components().next(),
            Some(std::path::Component::Normal(_))
        )
        || !is_yaml(path)
    {
        bail!("Select a YAML filename inside the themes folder.");
    }
    Ok(adeline_core::config::directory()?.join("themes").join(path))
}
pub fn discover() -> Result<(Vec<ThemeChoice>, Vec<String>)> {
    let directory = adeline_core::config::directory()?.join("themes");
    discover_in(&directory)
}
fn discover_in(directory: &Path) -> Result<(Vec<ThemeChoice>, Vec<String>)> {
    let entries = std::fs::read_dir(directory)?;
    let mut choices = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || !is_yaml(&path) {
            continue;
        }
        match read(&path) {
            Ok(State { theme, .. }) => choices.push(ThemeChoice {
                file: entry.file_name().to_string_lossy().into(),
                name: theme.name,
                brightness: theme.brightness,
            }),
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    choices.sort_by(|a, b| a.name.cmp(&b.name).then(a.file.cmp(&b.file)));
    Ok((choices, errors))
}
pub fn init() {
    let result = (|| {
        let directory = adeline_core::config::directory()?.join("themes");
        seed_bundled_themes(&directory)?;
        let file = adeline_core::config::current().general.appearance.theme;
        read(&theme_path(&file)?)
    })();
    ACTIVE.with(|s| {
        *s.borrow_mut() = result.unwrap_or_else(|error| State::fallback(&error));
    });
}
pub fn select(file: &str, cx: &mut App) -> Result<()> {
    let state = read(&theme_path(file)?)?;
    adeline_core::config::update(|s| s.general.appearance.theme = file.into())?;
    ACTIVE.with(|s| *s.borrow_mut() = state);
    apply(cx);
    Ok(())
}

pub fn apply(cx: &mut App) {
    ACTIVE.with(|state| {
        let state = state.borrow();
        let c = |token| -> Hsla { rgb(state.colors[token]).into() };
        let dark = state.theme.brightness.is_dark();
        let ink: Hsla = if dark {
            gpui_kit::white()
        } else {
            gpui_kit::black()
        };
        let (bg, fg, muted_text) = (c("background"), c("main_text"), c("muted_text"));
        let (island, raised) = (c("background_dark"), c("background_light"));
        // Menus and code blocks lift toward the ink: lighter in dark themes, darker in light ones.
        let toward_ink = if dark { 1. } else { -1. };
        let surface = lift(bg, 0.025 * toward_ink);
        let line = ink.alpha(0.09);
        let hover = ink.alpha(0.07);
        let highlight = c("button_highlight");
        let theme = KitTheme::global_mut(cx);
        theme.mode = if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        theme.background = bg;
        theme.foreground = fg;
        // Code in the Docs Editor follows the theme's brightness and background.
        let mut highlight_theme = if dark {
            HighlightTheme::default_dark()
        } else {
            HighlightTheme::default_light()
        };
        let style = &mut std::sync::Arc::make_mut(&mut highlight_theme).style;
        style.editor_background = Some(bg);
        style.editor_gutter_background = Some(bg);
        style.editor_foreground = Some(fg);
        theme.highlight_theme = highlight_theme;
        theme.group_box = surface;
        theme.title_bar = island;
        theme.title_bar_border = line;
        theme.status_bar = island;
        theme.status_bar_border = line;
        theme.window_border = line;
        // Dim with black: a foreground scrim turns milky in dark themes.
        theme.overlay = gpui_kit::black().alpha(0.5);
        theme.group_box_foreground = fg;
        theme.popover = surface;
        theme.popover_foreground = fg;
        theme.primary = highlight;
        theme.primary_foreground = bg;
        theme.primary_hover = highlight.mix_oklab(fg, 0.12);
        theme.primary_active = highlight.mix_oklab(fg, 0.22);
        theme.secondary = raised;
        theme.secondary_foreground = fg;
        theme.secondary_hover = hover;
        theme.secondary_active = hover;
        theme.muted = lift(bg, 0.035 * toward_ink);
        theme.muted_foreground = muted_text;
        // The kit highlights menu, list and command rows with `accent`: a quiet
        // wash of the text over the popover, as in the projects menu.
        theme.accent = fg.mix_oklab(surface, 0.08);
        theme.accent_foreground = fg;
        theme.danger = c("fail");
        theme.danger_foreground = bg;
        theme.danger_hover = theme.danger.mix_oklab(fg, 0.12);
        theme.danger_active = theme.danger.mix_oklab(fg, 0.22);
        theme.border = line;
        theme.input = ink.alpha(0.12);
        theme.ring = fg.mix_oklab(muted_text, 0.5);
        theme.caret = fg;
        // Text paints over the selection fill, so it must stay see-through.
        theme.selection = highlight.alpha(0.3);
        [
            theme.chart_1,
            theme.chart_2,
            theme.chart_3,
            theme.chart_4,
            theme.chart_5,
        ] = PROJECT_TOKENS.map(c);
        theme.success = c("success");
        theme.warning = c("warning");
        theme.link = highlight;
        theme.link_hover = theme.primary_hover;
        theme.link_active = theme.primary_active;
        theme.info = highlight;
        theme.success_foreground = bg;
        theme.warning_foreground = bg;
        theme.info_foreground = bg;
        theme.sidebar = island;
        theme.sidebar_foreground = theme.ring;
        theme.sidebar_primary = raised;
        theme.sidebar_primary_foreground = fg;
        theme.sidebar_accent = hover;
        theme.sidebar_accent_foreground = fg;
        theme.sidebar_border = line;
        theme.scrollbar = island;
        theme.scrollbar_thumb = line;
        theme.scrollbar_thumb_hover = raised;
        theme.colors.list = surface;
        theme.list_hover = hover;
        theme.list_active = hover;
        theme.list_head = theme.muted;
        theme.table = surface;
        theme.table_head = theme.muted;
        theme.table_head_foreground = fg;
        theme.table_hover = hover;
        theme.table_active = hover;
        theme.table_row_border = line;
        theme.tab_bar = island;
        // The track behind segmented tabs; the selected tab sits on the background.
        theme.tab_bar_segmented = hover;
        theme.tab = raised;
        theme.tab_active = hover;
        theme.tab_active_foreground = fg;
        theme.tab_foreground = fg;
        // An off track of ink at a fixed opacity shows on every surface.
        theme.switch = ink.alpha(if dark { 0.16 } else { 0.14 });
        theme.switch_thumb = gpui_kit::white();
        theme.drag_border = theme.ring;
        theme.button = raised;
        theme.button_foreground = fg;
        theme.button_hover = hover;
        theme.button_active = hover;
        theme.button_primary = highlight;
        theme.button_primary_foreground = bg;
        theme.button_primary_hover = theme.primary_hover;
        theme.button_primary_active = theme.primary_active;
        theme.button_secondary = raised;
        theme.button_secondary_foreground = fg;
        theme.button_secondary_hover = hover;
        theme.button_secondary_active = hover;
        theme.button_danger = theme.danger;
        theme.button_danger_foreground = bg;
        theme.button_danger_hover = theme.danger_hover;
        theme.button_danger_active = theme.danger_active;
        theme.font_family = crate::fonts::interface().into();
        theme.font_size = px(f32::from(adeline_core::config::font_size()));
        theme.mono_font_family = crate::fonts::code().into();
        theme.mono_font_size = px(f32::from(adeline_core::config::code_font_size()));
        theme.tokens = (&theme.colors).into();
    });
    KitTheme::sync_base(cx);
    let islands = Islands::new(KitTheme::global(cx));
    cx.set_global(islands);
    style_scrollbars(cx);
    cx.refresh_windows();
}

// The Islands surfaces (docs/design/DESIGN.md): the theme's dark background
// for islands, its light background for bubbles and the composer, and cards
// lifted from the window. Overlays are `ink` at fixed opacities.
#[derive(Clone, Copy)]
pub struct Islands {
    pub island: Hsla,
    pub bg: Hsla,
    pub card: Hsla,
    pub raise: Hsla,
    pub bubble: Hsla,
    pub fg: Hsla,
    pub fg2: Hsla,
    pub fg3: Hsla,
    pub line: Hsla,
    pub hover: Hsla,
    pub chip: Hsla,
    pub sel: Hsla,
    pub ring: Hsla,
    pub ring_strong: Hsla,
    pub active: Hsla,
    pub accent: Hsla,
    pub ok: Hsla,
    pub err: Hsla,
    pub ctx: Hsla,
    pub send_bg: Hsla,
    pub send_fg: Hsla,
}
impl gpui_kit::Global for Islands {}
impl Islands {
    fn new(theme: &KitTheme) -> Self {
        let dark = theme.mode.is_dark();
        let bg = theme.background;
        let ink: Hsla = if dark {
            gpui_kit::white()
        } else {
            gpui_kit::black()
        };
        let fg = theme.foreground;
        let fg3 = theme.muted_foreground;
        Self {
            island: hsla("background_dark"),
            bg,
            card: lift(bg, 0.025),
            raise: hsla("background_light"),
            bubble: hsla("background_light"),
            fg,
            fg2: fg.mix_oklab(fg3, 0.5),
            fg3,
            line: ink.alpha(0.09),
            hover: ink.alpha(0.07),
            chip: ink.alpha(0.08),
            sel: ink.alpha(0.12),
            ring: ink.alpha(0.1),
            ring_strong: ink.alpha(0.22),
            active: ink.alpha(0.11),
            accent: hsla("attention"),
            ok: theme.success,
            err: theme.danger,
            ctx: theme.info,
            send_bg: lift(hsla("background_light"), 0.025),
            send_fg: theme.primary,
        }
    }
}
pub fn islands(cx: &App) -> &Islands {
    cx.global::<Islands>()
}

// Moves a color's OKLab lightness by `delta`, toward white or black.
fn lift(color: Hsla, delta: f32) -> Hsla {
    let rgb = color.to_rgb();
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [red, green, blue] = [rgb.r, rgb.g, rgb.b].map(linear);
    let cone = |[x, y, z]: [f32; 3]| (x * red + y * green + z * blue).cbrt();
    let long = cone([0.412_221_46, 0.536_332_55, 0.051_445_995]);
    let medium = cone([0.211_903_5, 0.680_699_5, 0.107_396_96]);
    let short = cone([0.088_302_46, 0.281_718_85, 0.629_978_7]);
    let lightness = 0.210_454_26 * long + 0.793_617_8 * medium - 0.004_072_047 * short;
    if delta > 0. {
        let toward = (delta / (1. - lightness).max(0.001)).min(1.);
        gpui_kit::white().mix_oklab(color, toward)
    } else {
        let toward = (-delta / lightness.max(0.001)).min(1.);
        gpui_kit::black().mix_oklab(color, toward)
    }
}

// The Islands type scale, in pixels at a 14 px interface size.
pub const T_MICRO: f32 = 11.;
pub const T_CAPTION: f32 = 12.;
pub const T_UI: f32 = 13.;
pub const T_BODY: f32 = 14.;
pub const T_PROSE: f32 = 15.;
pub const T_TITLE: f32 = 17.;
pub const T_DOC: f32 = 24.;

// Panels that float over a scrolling view stop this far from its right edge
// so the bar stays visible.
pub const SCROLLBAR_TRACK: gpui_kit::Pixels = px(10.);

fn style_scrollbars(cx: &mut App) {
    // No track: only a narrow bar in the border color, the same at rest, under
    // the pointer and while dragged. Kit's scroll-to-reveal timing stays.
    use gpui_kit::base::{
        ScrollbarStyles, ScrollbarTheme, ScrollbarThumbStyle, ScrollbarTrackStyle,
    };
    let bar = islands(cx).sel.alpha(0.2);
    let clear = gpui_kit::transparent_black();
    let track =
        |style: ScrollbarTrackStyle| style.bg(clear).border_color(clear).width(SCROLLBAR_TRACK);
    let thumb =
        |style: ScrollbarThumbStyle| style.bg(bar).width(px(6.)).inset(px(2.)).radius(px(3.));
    let base = gpui_kit::base::Theme::global_mut(cx);
    base.scrollbar = ScrollbarTheme::new()
        .with_mode(base.scrollbar.mode())
        .with_motion(base.scrollbar.motion())
        .with_styles(
            ScrollbarStyles::default()
                .track(track)
                .track_hover(track)
                .track_active(track)
                .thumb(thumb)
                .thumb_hover(thumb)
                .thumb_active(thumb),
        );
}

// Project marks cycle through the accents in the order the old chart colors had.
const PROJECT_TOKENS: [&str; 5] = [
    "attention",
    "warning",
    "success",
    "button_highlight",
    "effects",
];
pub fn project_colors() -> [u32; 5] {
    PROJECT_TOKENS.map(color)
}

pub fn project_mark(tint: Hsla, active: bool, theme: &KitTheme) -> (Hsla, Hsla) {
    if active {
        let letter = if (tint.l - theme.foreground.l).abs() >= (tint.l - theme.background.l).abs() {
            theme.foreground
        } else {
            theme.background
        };
        (tint, letter)
    } else {
        // Palettes include near-black tints, so the letter leans on the
        // foreground to stay legible whatever the tint.
        (
            tint.mix_oklab(theme.title_bar, 0.4),
            tint.mix_oklab(theme.foreground, 0.35),
        )
    }
}

pub fn project_tint(index: usize) -> Hsla {
    let colors = project_colors();
    rgb(colors[index % colors.len()]).into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_themes_populate_fresh_config_and_preserve_existing_files() {
        let root =
            std::env::temp_dir().join(format!("adeline-bundled-themes-{}", std::process::id()));
        assert!(!root.exists());
        assert_eq!(BUNDLED_THEMES.len(), 23);
        seed_bundled_themes(&root).unwrap();
        let (choices, errors) = discover_in(&root).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(choices.len(), BUNDLED_THEMES.len());
        for &(name, contents) in BUNDLED_THEMES {
            assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), contents);
        }
        // Preserve even malformed user edits; startup must never replace them.
        std::fs::write(root.join("rose.yml"), "user-edited theme").unwrap();
        let missing = BUNDLED_THEMES
            .iter()
            .find(|(name, _)| *name != "rose.yml")
            .unwrap();
        std::fs::remove_file(root.join(missing.0)).unwrap();
        let legacy = BUNDLED_THEMES
            .iter()
            .find(|(name, _)| *name != "rose.yml" && *name != missing.0)
            .unwrap();
        std::fs::write(root.join(legacy.0), "colors:\n  sidebar_ring: '#000000'\n").unwrap();
        seed_bundled_themes(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join(legacy.0)).unwrap(),
            legacy.1
        );
        assert_eq!(
            std::fs::read_to_string(root.join("rose.yml")).unwrap(),
            "user-edited theme"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(missing.0)).unwrap(),
            missing.1
        );
        for &(name, _) in BUNDLED_THEMES {
            std::fs::remove_file(root.join(name)).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn bundled_theme_names_are_unique() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bundled-themes");
        let (choices, _) = discover_in(&directory).unwrap();
        let names: std::collections::BTreeSet<_> = choices.iter().map(|c| &c.name).collect();
        assert_eq!(names.len(), BUNDLED_THEMES.len());
    }
    #[test]
    fn discovers_added_themes_and_reports_invalid_files() {
        let root =
            std::env::temp_dir().join(format!("adeline-theme-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut theme = ThemeFile::builtin();
        theme.name = "My theme".into();
        theme.brightness = Brightness::VeryDark;
        theme.colors.insert("attention".into(), "#123456".into());
        adeline_core::config::seed_yaml(&root.join("custom.yml"), &theme).unwrap();
        let (choices, errors) = discover_in(&root).unwrap();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].name, "My theme");
        assert!(errors.is_empty());
        assert_eq!(
            read(&root.join("custom.yml")).unwrap().colors["attention"],
            0x123456
        );
        std::fs::write(root.join("invalid.yml"), "name: Broken\nbrightness: Dark\n").unwrap();
        adeline_core::config::seed_yaml(&root.join("added.yaml"), &ThemeFile::builtin()).unwrap();
        let (choices, errors) = discover_in(&root).unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(errors.len(), 1);
        for name in ["custom.yml", "invalid.yml", "added.yaml"] {
            std::fs::remove_file(root.join(name)).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn missing_theme_falls_back_to_embedded_islands() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled-themes")
            .join("missing-theme-for-test.yml");
        let state = State::fallback(&read(&missing).err().unwrap());
        assert_eq!(state.theme.name, "Islands");
        assert_eq!(state.theme.brightness, Brightness::Dark);
        assert_eq!(state.colors, ThemeFile::builtin().validate().unwrap());
        let error = state.load_error.unwrap();
        assert!(error.contains("missing-theme-for-test.yml"));
        assert!(error.contains("Using Islands"));
    }
    #[test]
    fn theme_files_reject_missing_or_bad_colors() {
        let legacy: Brightness = serde_yaml_ng::from_str("Very Light").unwrap();
        assert_eq!(legacy, Brightness::Light);
        let mut theme = ThemeFile::builtin();
        theme.colors.remove("attention");
        theme.validate().unwrap_err();
        theme.colors.insert("attention".into(), "#GGFFFF".into());
        theme.validate().unwrap_err();
        assert_eq!(parse_hex("#aBc123"), Some(0xabc123));
        assert_eq!(parse_hex("１２３"), None);
    }

    #[test]
    fn theme_names_cannot_leave_the_themes_folder() {
        for outside in [
            "../outside.yml",
            "..\\outside.yml",
            "C:\\outside.yml",
            "C:outside.yml",
            "/etc/outside.yml",
        ] {
            assert!(theme_path(outside).is_err(), "{outside}");
        }
    }
}
