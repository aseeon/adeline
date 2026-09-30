//! Application themes. Existing 32-color YAML files project onto Kit's global
//! theme for every window. Preferences stay separate from workspace data.
use gpui_kit::{
    App, Hsla,
    component::{Colorize as _, Theme as KitTheme, ThemeMode},
    px, rgb,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap, path::Path};

include!(concat!(env!("OUT_DIR"), "/themes.rs"));

fn seed_bundled_themes(directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for &(name, contents) in BUNDLED_THEMES {
        let theme: ThemeFile =
            serde_yaml_ng::from_str(contents).map_err(|e| format!("Bundled theme {name}: {e}"))?;
        theme
            .validate()
            .map_err(|e| format!("Bundled theme {name}: {e}"))?;
        crate::files::seed(&directory.join(name), contents.as_bytes())?;
    }
    Ok(())
}

macro_rules! palette {
    ($($name:ident, $label:literal, $color:literal;)*) => {
        pub const ROLES: &[(&str, &str, u32)] = &[$((stringify!($name), $label, $color)),*];
        $(
            #[expect(clippy::allow_attributes, reason = "`#[expect(dead_code)]` would fail on the used accessors")]
            #[allow(dead_code, reason = "every palette role gets an accessor; not all are drawn yet")]
            pub fn $name() -> u32 { ACTIVE.with(|s| s.borrow().colors[stringify!($name)]) })*
    };
}
palette! {
    background, "Background", 0xfcfbf7;
    foreground, "Foreground", 0x1c1c1a;
    card, "Card", 0xfafeff;
    card_foreground, "Card foreground", 0x1c1c1a;
    popover, "Popover", 0xfafeff;
    popover_foreground, "Popover foreground", 0x1c1c1a;
    primary, "Primary", 0xbb5e3a;
    primary_foreground, "Primary foreground", 0xfafeff;
    secondary, "Secondary", 0xe9e7e0;
    secondary_foreground, "Secondary foreground", 0x4a4a44;
    muted, "Muted", 0xf2f1eb;
    muted_foreground, "Muted foreground", 0x7a7a72;
    accent, "Accent", 0xecb37e;
    accent_foreground, "Accent foreground", 0xfcfbf7;
    destructive, "Destructive", 0xe15a44;
    destructive_foreground, "Destructive foreground", 0xffffff;
    border, "Border", 0xe2e0d5;
    input, "Input outline", 0xe2e0d5;
    ring, "Focus ring", 0xbb5e3a;
    chart_1, "Chart 1", 0xbb5e3a;
    chart_2, "Chart 2", 0xecb37e;
    chart_3, "Chart 3", 0xa2b771;
    chart_4, "Chart 4", 0x4b6584;
    chart_5, "Chart 5", 0xb894db;
    sidebar, "Sidebar background", 0xf7f6f0;
    sidebar_foreground, "Sidebar foreground", 0x1c1c1a;
    sidebar_primary, "Sidebar primary", 0xbb5e3a;
    sidebar_primary_foreground, "Sidebar primary foreground", 0xfafeff;
    sidebar_accent, "Sidebar selection", 0xedebe4;
    sidebar_accent_foreground, "Sidebar selection foreground", 0xce8264;
    sidebar_border, "Sidebar border", 0xe2e0d5;
    sidebar_ring, "Sidebar focus ring", 0xbb5e3a;
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
        serde_yaml_ng::from_str(include_str!("../bundled-themes/claude-plus.yml"))
            .expect("valid bundled Claude Plus theme")
    }
    fn validate(&self) -> Result<BTreeMap<&'static str, u32>, String> {
        if self.name.trim().is_empty() {
            return Err("Theme name cannot be empty.".into());
        }
        if self.colors.len() != ROLES.len() {
            return Err("A theme must contain exactly the 32 named colors.".into());
        }
        ROLES
            .iter()
            .map(|&(key, label, _)| {
                self.colors
                    .get(key)
                    .and_then(|v| parse_hex(v))
                    .map(|v| (key, v))
                    .ok_or_else(|| format!("{label}: expected a six-digit hex color."))
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
    fn fallback(mut error: String) -> Self {
        error.push_str(" Using Claude Plus until this is fixed.");
        Self {
            load_error: Some(error),
            ..Self::default()
        }
    }
}
thread_local! { static ACTIVE: RefCell<State> = RefCell::new(State::default()); }

pub fn load_error() -> Option<String> {
    ACTIVE
        .with(|s| s.borrow().load_error.clone())
        .or_else(crate::config::error)
}

pub fn parse_hex(value: &str) -> Option<u32> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    (value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
}
/// Kit has one global focus ring; sidebar navigation uses the YAML sidebar ring
/// on its focused border to preserve the separate palette role.
pub fn sidebar_focus() -> Hsla {
    rgb(sidebar_ring()).into()
}
fn read(path: &Path) -> Result<ThemeFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let theme: ThemeFile =
        serde_yaml_ng::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    theme
        .validate()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(theme)
}
fn theme_path(file: &str) -> Result<std::path::PathBuf, String> {
    let path = Path::new(file);
    if file.contains(['/', '\\', ':'])
        || path.components().count() != 1
        || !matches!(
            path.components().next(),
            Some(std::path::Component::Normal(_))
        )
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
    {
        return Err("Select a YAML filename inside the themes folder.".into());
    }
    Ok(crate::config::directory()?.join("themes").join(path))
}
pub fn discover() -> Result<(Vec<ThemeChoice>, Vec<String>), String> {
    let directory = crate::config::directory()?.join("themes");
    discover_in(&directory)
}
fn discover_in(directory: &Path) -> Result<(Vec<ThemeChoice>, Vec<String>), String> {
    let entries = std::fs::read_dir(directory).map_err(|e| e.to_string())?;
    let mut choices = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
        {
            continue;
        }
        match read(&path) {
            Ok(theme) => choices.push(ThemeChoice {
                file: entry.file_name().to_string_lossy().into(),
                name: theme.name,
                brightness: theme.brightness,
            }),
            Err(error) => errors.push(error),
        }
    }
    choices.sort_by(|a, b| a.name.cmp(&b.name).then(a.file.cmp(&b.file)));
    Ok((choices, errors))
}
pub fn init() {
    let result = (|| {
        let directory = crate::config::directory()?.join("themes");
        seed_bundled_themes(&directory)?;
        let file = crate::config::current().general.appearance.theme;
        let theme = read(&theme_path(&file)?)?;
        let colors = theme.validate()?;
        Ok::<_, String>(State {
            theme,
            colors,
            load_error: None,
        })
    })();
    ACTIVE.with(|s| {
        *s.borrow_mut() = result.unwrap_or_else(State::fallback);
    });
}
pub fn select(file: &str, cx: &mut App) -> Result<(), String> {
    let theme = read(&theme_path(file)?)?;
    let colors = theme.validate()?;
    crate::config::update(|s| s.general.appearance.theme = file.into())?;
    ACTIVE.with(|s| {
        *s.borrow_mut() = State {
            theme,
            colors,
            load_error: None,
        }
    });
    apply(cx);
    Ok(())
}

/// Projects the 32-color YAML palette and persisted typography into Kit's
/// global semantic and component colors, including popups and child windows.
/// Call after Kit initialization and whenever appearance preferences change.
pub fn apply(cx: &mut App) {
    ACTIVE.with(|state| {
        let state = state.borrow();
        let c = |role| rgb(state.colors[role]).into();
        let theme = KitTheme::global_mut(cx);
        theme.mode = if state.theme.brightness.is_dark() {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        theme.background = c("background");
        theme.foreground = c("foreground");
        theme.group_box = c("card");
        theme.title_bar = c("sidebar");
        theme.title_bar_border = c("sidebar_border");
        theme.status_bar = c("sidebar");
        theme.status_bar_border = c("sidebar_border");
        theme.window_border = c("border");
        theme.overlay = c("foreground").alpha(0.55);
        theme.group_box_foreground = c("card_foreground");
        theme.popover = c("popover");
        theme.popover_foreground = c("popover_foreground");
        theme.primary = c("primary");
        theme.primary_foreground = c("primary_foreground");
        theme.primary_hover = theme.primary.mix_oklab(theme.foreground, 0.12);
        theme.primary_active = theme.primary.mix_oklab(theme.foreground, 0.22);
        theme.secondary = c("secondary");
        theme.secondary_foreground = c("secondary_foreground");
        theme.secondary_hover = c("sidebar_accent");
        theme.secondary_active = c("sidebar_accent");
        theme.muted = c("muted");
        theme.muted_foreground = c("muted_foreground");
        theme.accent = c("accent");
        theme.accent_foreground = c("accent_foreground");
        theme.danger = c("destructive");
        theme.danger_foreground = c("destructive_foreground");
        theme.danger_hover = theme.danger.mix_oklab(theme.foreground, 0.12);
        theme.danger_active = theme.danger.mix_oklab(theme.foreground, 0.22);
        theme.border = c("border");
        theme.input = c("input");
        theme.ring = c("ring");
        theme.caret = c("foreground");
        theme.selection = c("sidebar_accent");
        theme.chart_1 = c("chart_1");
        theme.chart_2 = c("chart_2");
        theme.chart_3 = c("chart_3");
        theme.chart_4 = c("chart_4");
        theme.chart_5 = c("chart_5");
        theme.success = c("chart_3");
        theme.warning = c("chart_2");
        theme.link = c("chart_2");
        theme.link_hover = theme.link.mix_oklab(theme.foreground, 0.12);
        theme.link_active = theme.link.mix_oklab(theme.foreground, 0.22);
        theme.info = c("chart_4");
        theme.success_foreground = c("background");
        theme.warning_foreground = c("foreground");
        theme.info_foreground = c("background");
        theme.sidebar = c("sidebar");
        theme.sidebar_foreground = c("sidebar_foreground");
        theme.sidebar_primary = c("sidebar_primary");
        theme.sidebar_primary_foreground = c("sidebar_primary_foreground");
        theme.sidebar_accent = c("sidebar_accent");
        theme.sidebar_accent_foreground = c("sidebar_accent_foreground");
        theme.sidebar_border = c("sidebar_border");
        theme.scrollbar = c("sidebar");
        theme.scrollbar_thumb = c("sidebar_border");
        theme.scrollbar_thumb_hover = c("sidebar_primary");
        theme.colors.list = c("card");
        theme.list_hover = c("sidebar_accent");
        theme.list_active = c("sidebar_accent");
        theme.list_head = c("muted");
        theme.table = c("card");
        theme.table_head = c("muted");
        theme.table_head_foreground = c("foreground");
        theme.table_hover = c("sidebar_accent");
        theme.table_active = c("sidebar_accent");
        theme.table_row_border = c("border");
        theme.tab_bar = c("sidebar");
        // The track behind segmented tabs; the selected tab sits on the background.
        theme.tab_bar_segmented = c("sidebar_accent");
        theme.tab = c("secondary");
        theme.tab_active = c("sidebar_accent");
        theme.tab_active_foreground = c("sidebar_accent_foreground");
        theme.tab_foreground = c("secondary_foreground");
        theme.switch = c("muted");
        theme.switch_thumb = c("primary_foreground");
        theme.drag_border = c("ring");
        theme.button = c("secondary");
        theme.button_foreground = c("secondary_foreground");
        theme.button_hover = c("sidebar_accent");
        theme.button_active = c("sidebar_accent");
        theme.button_primary = c("primary");
        theme.button_primary_foreground = c("primary_foreground");
        theme.button_primary_hover = theme.primary_hover;
        theme.button_primary_active = theme.primary_active;
        theme.button_secondary = c("secondary");
        theme.button_secondary_foreground = c("secondary_foreground");
        theme.button_secondary_hover = c("sidebar_accent");
        theme.button_secondary_active = c("sidebar_accent");
        theme.button_danger = c("destructive");
        theme.button_danger_foreground = c("destructive_foreground");
        theme.button_danger_hover = theme.danger_hover;
        theme.button_danger_active = theme.danger_active;
        theme.font_family = crate::config::font().into();
        theme.font_size = px(f32::from(crate::config::font_size()));
        theme.mono_font_family = crate::config::code_font().into();
        theme.mono_font_size = px(f32::from(crate::config::code_font_size()));
        theme.tokens = (&theme.colors).into();
    });
    KitTheme::sync_base(cx);
    style_scrollbars(cx);
    cx.refresh_windows();
}

/// Width of every scrollbar's track. Panels that float over a scrolling view
/// stop this far from its right edge so the bar stays visible.
pub const SCROLLBAR_TRACK: gpui_kit::Pixels = px(10.);

/// Scrollbars have no track: only a narrow bar in the border color, the same at
/// rest, under the pointer and while dragged. Kit's scroll-to-reveal timing stays.
fn style_scrollbars(cx: &mut App) {
    use gpui_kit::base::{
        ScrollbarStyles, ScrollbarTheme, ScrollbarThumbStyle, ScrollbarTrackStyle,
    };
    let bar = KitTheme::global(cx).border;
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

pub fn project_colors() -> [u32; 5] {
    [chart_1(), chart_2(), chart_3(), chart_4(), chart_5()]
}

/// Title-bar cell roles derived from the active palette, so every theme gets
/// them without new YAML keys.
pub struct BarColors {
    /// The line between title-bar cells, one step quieter than `border`.
    pub divider: Hsla,
    /// An inactive cell under the pointer.
    pub hover: Hsla,
    /// A projects-menu row under the pointer or the keyboard highlight.
    pub menu_hover: Hsla,
}
pub fn bar_colors(theme: &KitTheme) -> BarColors {
    BarColors {
        divider: theme.border.mix_oklab(theme.title_bar, 0.5),
        hover: theme.foreground.mix_oklab(theme.title_bar, 0.04),
        menu_hover: theme.foreground.mix_oklab(theme.popover, 0.08),
    }
}

/// A project's letter mark: a soft wash of its tint at rest and the full tint
/// when active, with the letter in whichever of foreground or background
/// stands further from the fill.
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
        assert_eq!(BUNDLED_THEMES.len(), 13);
        seed_bundled_themes(&root).unwrap();
        let (choices, errors) = discover_in(&root).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(choices.len(), BUNDLED_THEMES.len());
        for &(name, contents) in BUNDLED_THEMES {
            assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), contents);
        }
        // Preserve even malformed user edits; startup must never replace them.
        std::fs::write(root.join("lightos.yml"), "user-edited theme").unwrap();
        let missing = BUNDLED_THEMES
            .iter()
            .find(|(name, _)| *name != "lightos.yml")
            .unwrap();
        std::fs::remove_file(root.join(missing.0)).unwrap();
        seed_bundled_themes(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("lightos.yml")).unwrap(),
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
    fn bundled_themes_load_with_all_native_roles() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("bundled-themes");
        let (choices, errors) = discover_in(&directory).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(choices.len(), BUNDLED_THEMES.len());
        let mut names = std::collections::BTreeSet::new();
        for choice in choices {
            assert!(names.insert(choice.name));
            let theme = read(&directory.join(choice.file)).unwrap();
            assert_eq!(theme.validate().unwrap().len(), ROLES.len());
        }
    }
    #[test]
    fn discovers_added_themes_and_reports_invalid_files() {
        let root =
            std::env::temp_dir().join(format!("adeline-theme-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut theme = ThemeFile::builtin();
        theme.name = "My theme".into();
        theme.brightness = Brightness::VeryDark;
        theme.colors.insert("primary".into(), "#123456".into());
        crate::config::seed_yaml(&root.join("custom.yml"), &theme).unwrap();
        let (choices, errors) = discover_in(&root).unwrap();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].name, "My theme");
        assert!(errors.is_empty());
        assert_eq!(
            read(&root.join("custom.yml")).unwrap().validate().unwrap()["primary"],
            0x123456
        );
        crate::config::seed_yaml(&root.join("custom.yml"), &ThemeFile::builtin()).unwrap();
        assert_eq!(read(&root.join("custom.yml")).unwrap().name, "My theme");
        std::fs::write(root.join("invalid.yml"), "name: Broken\nbrightness: Dark\n").unwrap();
        crate::config::seed_yaml(&root.join("added.yaml"), &ThemeFile::builtin()).unwrap();
        let (choices, errors) = discover_in(&root).unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(errors.len(), 1);
        for name in ["custom.yml", "invalid.yml", "added.yaml"] {
            std::fs::remove_file(root.join(name)).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn missing_theme_falls_back_to_embedded_claude_plus() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bundled-themes")
            .join("missing-theme-for-test.yml");
        let state = State::fallback(read(&missing).unwrap_err());
        assert_eq!(state.theme.name, "Claude Plus");
        assert_eq!(state.theme.brightness, Brightness::Dark);
        assert_eq!(state.colors, ThemeFile::builtin().validate().unwrap());
        let error = state.load_error.unwrap();
        assert!(error.contains("missing-theme-for-test.yml"));
        assert!(error.contains("Using Claude Plus"));
    }
    #[test]
    fn theme_yaml_round_trip_and_validation() {
        let default = State::default();
        assert_eq!(default.theme.name, "Claude Plus");
        let legacy: Brightness = serde_yaml_ng::from_str("Very Light").unwrap();
        assert_eq!(legacy, Brightness::Light);
        assert_eq!(serde_yaml_ng::to_string(&legacy).unwrap().trim(), "Light");
        assert_eq!(default.theme.brightness, Brightness::Dark);
        assert_eq!(default.colors["background"], 0x262626);
        assert_eq!(
            crate::config::Appearance::default().theme,
            "claude-plus.yml"
        );
        {
            let theme = ThemeFile::builtin();
            let yaml = serde_yaml_ng::to_string(&theme).unwrap();
            assert!(yaml.starts_with("name:"));
            let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
            assert_eq!(document.as_mapping().unwrap().len(), 3);
            assert_eq!(document["colors"].as_mapping().unwrap().len(), 32);
            assert!(document.get("background").is_none());
            let mut loaded: ThemeFile = serde_yaml_ng::from_str(&yaml).unwrap();
            assert_eq!(loaded.validate().unwrap(), theme.validate().unwrap());
            loaded.colors.remove("primary");
            assert!(loaded.validate().is_err());
            loaded.colors.insert("primary".into(), "#GGFFFF".into());
            assert!(loaded.validate().is_err());
        }
        assert!(serde_yaml_ng::from_str::<ThemeFile>("name: bad\nbrightness: Medium").is_err());
        assert!(
            serde_yaml_ng::from_str::<ThemeFile>(
                "name: Flat theme\nbrightness: Dark\nbackground: '#0F1218'\n"
            )
            .is_err()
        );
        assert!(theme_path("../outside.yml").is_err());
        assert!(theme_path("C:\\outside.yml").is_err());
        assert!(theme_path("C:outside.yml").is_err());
        assert!(theme_path("..\\outside.yml").is_err());
        assert_eq!(parse_hex("#aBc123"), Some(0xabc123));
        assert_eq!(parse_hex("１２３"), None);
    }
}
