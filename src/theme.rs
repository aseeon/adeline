//! lightos theme. All rendering reads the UI-thread palette; changing it refreshes
//! every GPUI window, including cached child views. YAML preferences are saved separately
//! from workspace data. Source: docs/themes/SOURCES.md.
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap, path::Path};

include!(concat!(env!("OUT_DIR"), "/themes.rs"));

fn seed_bundled_themes(directory: &Path) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for &(name, contents) in BUNDLED_THEMES {
        let theme: ThemeFile =
            serde_yaml_ng::from_str(contents).map_err(|e| format!("Bundled theme {name}: {e}"))?;
        theme
            .validate()
            .map_err(|e| format!("Bundled theme {name}: {e}"))?;
        let path = directory.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => file
                .write_all(contents.as_bytes())
                .map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    crate::config::seed_yaml(&directory.join("lightos.yml"), &ThemeFile::builtin())
}

macro_rules! palette {
    ($($name:ident, $label:literal, $color:literal;)*) => {
        pub const ROLES: &[(&str, &str, u32)] = &[$((stringify!($name), $label, $color)),*];
        $(pub fn $name() -> u32 { ACTIVE.with(|s| s.borrow().colors[stringify!($name)]) })*
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
    Light,
    #[serde(rename = "Very Light")]
    VeryLight,
}
impl Brightness {
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
        Self {
            name: "lightos".into(),
            brightness: Brightness::Light,
            colors: ROLES
                .iter()
                .map(|&(key, _, color)| (key.into(), format!("#{color:06X}")))
                .collect(),
        }
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
}
struct State {
    file: String,
    theme: ThemeFile,
    colors: BTreeMap<&'static str, u32>,
    load_error: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        let theme = ThemeFile::builtin();
        Self {
            file: "lightos.yml".into(),
            colors: theme.validate().unwrap(),
            theme,
            load_error: None,
        }
    }
}
thread_local! { static ACTIVE: RefCell<State> = RefCell::new(State::default()); }
pub fn active_theme() -> ThemeFile {
    ACTIVE.with(|s| s.borrow().theme.clone())
}
pub fn active_file() -> String {
    ACTIVE.with(|s| s.borrow().file.clone())
}
pub fn load_error() -> Option<String> {
    ACTIVE
        .with(|s| s.borrow().load_error.clone())
        .or_else(crate::config::error)
}
pub fn current_colors() -> BTreeMap<&'static str, u32> {
    ACTIVE.with(|s| s.borrow().colors.clone())
}
pub fn is_dark() -> bool {
    ACTIVE.with(|s| s.borrow().theme.brightness.is_dark())
}
pub fn parse_hex(value: &str) -> Option<u32> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    (value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
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
    if path.components().count() != 1
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
            file,
            theme,
            colors,
            load_error: None,
        })
    })();
    ACTIVE.with(|s| match result {
        Ok(state) => *s.borrow_mut() = state,
        Err(error) => {
            s.borrow_mut().load_error = Some(format!("{error} Using lightos until this is fixed."))
        }
    });
}
pub fn select(file: &str, cx: &mut gpui::App) -> Result<(), String> {
    let theme = read(&theme_path(file)?)?;
    let colors = theme.validate()?;
    crate::config::update(|s| s.general.appearance.theme = file.into())?;
    ACTIVE.with(|s| {
        *s.borrow_mut() = State {
            file: file.into(),
            theme,
            colors,
            load_error: None,
        }
    });
    cx.refresh_windows();
    Ok(())
}
pub fn apply_colors(theme: ThemeFile, cx: &mut gpui::App) -> Result<(), String> {
    if let Some(error) = ACTIVE.with(|s| s.borrow().load_error.clone()) {
        return Err(format!(
            "Select a valid theme before saving colors. {error}"
        ));
    }
    let colors = theme.validate()?;
    let file = active_file();
    let path = theme_path(&file)?;
    // Do not silently overwrite a malformed file loaded with fallback colors.
    read(&path)?;
    crate::config::write_yaml(&path, &theme)?;
    ACTIVE.with(|s| {
        *s.borrow_mut() = State {
            file,
            theme,
            colors,
            load_error: None,
        }
    });
    cx.refresh_windows();
    Ok(())
}
pub fn blend(color: u32, base: u32, amount: f32) -> u32 {
    [16, 8, 0].into_iter().fold(0, |result, shift| {
        let a = ((color >> shift) & 255) as f32;
        let b = ((base >> shift) & 255) as f32;
        result | (((a * amount + b * (1. - amount)).round() as u32) << shift)
    })
}
pub fn display_tint(color: u32) -> u32 {
    blend(color, card(), if is_dark() { 0.15 } else { 0.25 })
}
pub fn success() -> u32 {
    if is_dark() {
        chart_3()
    } else {
        blend(chart_3(), foreground(), 0.55)
    }
}
pub fn project_colors() -> [u32; 5] {
    [chart_1(), chart_2(), chart_3(), chart_4(), chart_5()]
}
pub fn shadow_base() -> u32 {
    if is_dark() { sidebar() } else { foreground() }
}
pub fn shadow(blur: f32) -> gpui::BoxShadow {
    gpui::BoxShadow {
        color: gpui::rgba((shadow_base() << 8) | 0x30).into(),
        offset: gpui::point(gpui::px(0.), gpui::px(blur / 3.)),
        blur_radius: gpui::px(blur),
        spread_radius: gpui::px(0.),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_themes_populate_fresh_config_and_preserve_existing_files() {
        let root =
            std::env::temp_dir().join(format!("adeline-bundled-themes-{}", std::process::id()));
        assert!(!root.exists());
        assert_eq!(BUNDLED_THEMES.len(), 15);
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
    fn theme_yaml_round_trip_and_validation() {
        let default = State::default();
        assert_eq!(default.file, "lightos.yml");
        assert_eq!(default.theme.name, "lightos");
        assert_eq!(default.theme.brightness, Brightness::Light);
        assert_eq!(default.colors["background"], 0xfcfbf7);
        assert_eq!(crate::config::Appearance::default().theme, default.file);
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
        assert_eq!(parse_hex("#aBc123"), Some(0xabc123));
        assert_eq!(parse_hex("１２３"), None);
    }
}
