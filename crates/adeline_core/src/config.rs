//! Human-editable application preferences. Missing files are seeded, never replaced.
//! Render paths read through `with`, which doesn't clone the settings.
use anyhow::Context as _;
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Section {
    Chats,
    Docs,
    Workflows,
    Services,
    Groupchats,
    Issues,
    Whiteboard,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub general: General,
    pub modes: Modes,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    pub features: Features,
    pub appearance: Appearance,
    pub keymap: Keymap,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
// Not `deny_unknown_fields`: old files carry flags that no longer exist.
#[serde(default)]
pub struct Features {
    pub docs: bool,
    pub workflows: bool,
    pub services: bool,
    pub groupchats: bool,
    pub issues: bool,
    pub whiteboard: bool,
}
impl Default for Features {
    fn default() -> Self {
        Self {
            docs: true,
            workflows: false,
            services: false,
            groupchats: true,
            issues: false,
            whiteboard: false,
        }
    }
}
impl Features {
    pub fn enabled(&self, section: Section) -> bool {
        match section {
            Section::Chats => true,
            Section::Docs => self.docs,
            Section::Workflows => self.workflows,
            Section::Services => self.services,
            Section::Groupchats => self.groupchats,
            Section::Issues => self.issues,
            Section::Whiteboard => self.whiteboard,
        }
    }
    pub fn toggle(&mut self, section: Section) {
        let enabled = match section {
            Section::Chats => return,
            Section::Docs => &mut self.docs,
            Section::Workflows => &mut self.workflows,
            Section::Services => &mut self.services,
            Section::Groupchats => &mut self.groupchats,
            Section::Issues => &mut self.issues,
            Section::Whiteboard => &mut self.whiteboard,
        };
        *enabled = !*enabled;
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub theme: String,
    pub interface_font: String,
    pub font_size: u16,
    pub code_font: String,
    pub code_font_size: u16,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "claude-plus.yml".into(),
            interface_font: DEFAULT_FONT.into(),
            font_size: 14,
            code_font: DEFAULT_CODE_FONT.into(),
            code_font_size: 14,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Keymap {
    pub open_settings: Vec<String>,
    pub new_chat: Vec<String>,
    pub focus_search: Vec<String>,
    pub send_message: Vec<String>,
    pub close_dialog_or_popup: Vec<String>,
    pub next_control: Vec<String>,
    pub previous_control: Vec<String>,
}
impl Default for Keymap {
    fn default() -> Self {
        let keys = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Self {
            open_settings: keys(&["ctrl-,", "cmd-,"]),
            new_chat: keys(&["ctrl-n", "cmd-n"]),
            focus_search: keys(&["ctrl-f", "cmd-f"]),
            send_message: keys(&["ctrl-enter", "cmd-enter"]),
            close_dialog_or_popup: keys(&["escape"]),
            next_control: keys(&["tab"]),
            previous_control: keys(&["shift-tab"]),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Modes {
    pub chats: Chats,
    pub docs: Docs,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_value"
    )]
    workflows: Option<serde_yaml_ng::Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_value"
    )]
    services: Option<serde_yaml_ng::Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_value"
    )]
    groupchats: Option<serde_yaml_ng::Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_value"
    )]
    issues: Option<serde_yaml_ng::Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_value"
    )]
    whiteboard: Option<serde_yaml_ng::Value>,
}

// Option's ordinary deserializer treats an explicit YAML null as absent.
fn legacy_value<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_yaml_ng::Value>, D::Error> {
    serde_yaml_ng::Value::deserialize(deserializer).map(Some)
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[expect(
    clippy::struct_field_names,
    reason = "field names are the settings YAML keys"
)]
#[serde(default, deny_unknown_fields)]
pub struct Chats {
    pub show_completed_chats: bool,
    pub show_archived_chats: bool,
    pub show_left_panel: bool,
    pub show_agent_activity: bool,
    pub hide_tool_calls: bool,
    pub submit_on_enter: bool,
    // Moved to the engine's settings. Kept so older files round-trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_limit: Option<usize>,
    pub thinking_animation: ThinkingAnimation,
    pub show_thinking: bool,
    // 0 turns it off.
    pub silence_notice_minutes: u32,
}
impl Default for Chats {
    fn default() -> Self {
        Self {
            show_completed_chats: true,
            show_archived_chats: false,
            show_left_panel: true,
            show_agent_activity: false,
            hide_tool_calls: false,
            submit_on_enter: true,
            retry_limit: None,
            thinking_animation: ThinkingAnimation::default(),
            show_thinking: false,
            silence_notice_minutes: 10,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Docs {
    pub respect_gitignore: bool,
    pub show_left_panel: bool,
    pub show_conversation_panel: bool,
    pub open_in: DocView,
    pub load_remote_images: bool,
}
impl Default for Docs {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            show_left_panel: true,
            show_conversation_panel: true,
            open_in: DocView::Viewer,
            load_remote_images: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocView {
    #[default]
    Viewer,
    Editor,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingAnimation {
    #[default]
    Dots,
    Words,
    Braille,
    Fill,
}
impl ThinkingAnimation {
    pub const ALL: [Self; 4] = [Self::Dots, Self::Words, Self::Braille, Self::Fill];
    pub fn label(self) -> &'static str {
        match self {
            Self::Dots => "Typing dots",
            Self::Words => "Cycling words",
            Self::Braille => "Braille spinner",
            Self::Fill => "Liquid fill",
        }
    }
}
#[derive(Default)]
struct State {
    settings: Settings,
    error: Option<String>,
}
thread_local! { static ACTIVE: RefCell<State> = RefCell::new(State::default()); }
pub fn home() -> anyhow::Result<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .context("Could not find your home directory.")
}
pub fn directory() -> anyhow::Result<PathBuf> {
    Ok(home()?.join(".config/adeline"))
}
pub fn current() -> Settings {
    ACTIVE.with(|s| s.borrow().settings.clone())
}
pub fn with<T>(f: impl FnOnce(&Settings) -> T) -> T {
    ACTIVE.with(|s| f(&s.borrow().settings))
}
pub fn error() -> Option<String> {
    ACTIVE.with(|s| s.borrow().error.clone())
}
pub fn set_error(message: String) {
    ACTIVE.with(|s| s.borrow_mut().error = Some(message));
}
pub fn code_font_size() -> u16 {
    with(|s| {
        s.general
            .appearance
            .code_font_size
            .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
    })
}

pub const DEFAULT_FONT: &str = "Chivo";
pub const DEFAULT_CODE_FONT: &str = "Chivo Mono";
pub const MIN_FONT_SIZE: u16 = 10;
pub const MAX_FONT_SIZE: u16 = 24;

pub fn font_size() -> u16 {
    with(|s| {
        s.general
            .appearance
            .font_size
            .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
    })
}

pub fn write_yaml(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let text = serde_yaml_ng::to_string(value)?;
    crate::files::replace(path, text.as_bytes())
}
pub fn seed_yaml(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let text = serde_yaml_ng::to_string(value)?;
    crate::files::seed(path, text.as_bytes())
}
fn read(path: &Path) -> anyhow::Result<Settings> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    serde_yaml_ng::from_str(&text).with_context(|| path.display().to_string())
}
pub fn init() {
    let result = (|| {
        let root = directory()?;
        std::fs::create_dir_all(root.join("themes"))?;
        let path = root.join("settings.yml");
        seed_yaml(&path, &Settings::default())?;
        read(&path)
    })();
    ACTIVE.with(|s| match result {
        Ok(settings) => {
            *s.borrow_mut() = State {
                settings,
                error: None,
            }
        }
        Err(error) => s.borrow_mut().error = Some(format!("{error:#}")),
    });
}
pub fn update(change: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
    update_at(&directory()?.join("settings.yml"), change)
}
fn update_at(path: &Path, change: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
    // Re-read to preserve hand edits and refuse to overwrite malformed files.
    let mut settings = read(path)?;
    change(&mut settings);
    write_yaml(path, &settings)?;
    ACTIVE.with(|s| {
        *s.borrow_mut() = State {
            settings,
            error: None,
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_font_settings_load_and_each_size_is_bounded_on_its_own() {
        let defaults = Appearance::default();
        let legacy: Appearance =
            serde_yaml_ng::from_str("interface_font: Arial\nfont_size: 18").unwrap();
        assert_eq!(legacy.interface_font, "Arial");
        assert_eq!(legacy.font_size, 18);
        assert_eq!(legacy.code_font, defaults.code_font);
        assert_eq!(legacy.code_font_size, defaults.code_font_size);
        let original = current();
        ACTIVE.with(|s| s.borrow_mut().settings.general.appearance = legacy);
        for (requested, expected) in [(0, 10), (14, 14), (20, 20), (65535, 24)] {
            ACTIVE.with(|s| s.borrow_mut().settings.general.appearance.code_font_size = requested);
            assert_eq!(font_size(), 18);
            assert_eq!(code_font_size(), expected);
        }
        for (requested, expected) in [(0, 10), (18, 18), (65535, 24)] {
            ACTIVE.with(|s| s.borrow_mut().settings.general.appearance.font_size = requested);
            assert_eq!(font_size(), expected);
            assert_eq!(code_font_size(), 24);
        }
        let settings = current();
        assert_eq!(
            serde_yaml_ng::from_str::<Settings>(&serde_yaml_ng::to_string(&settings).unwrap())
                .unwrap(),
            settings
        );
        ACTIVE.with(|s| s.borrow_mut().settings = original);
    }
    #[test]
    fn legacy_features_use_new_defaults_and_explicit_choices_persist() {
        let mut settings: Settings = serde_yaml_ng::from_str(
            "general:\n  features:\n    close_picker_after_selection: false\n",
        )
        .unwrap();
        for section in [
            Section::Docs,
            Section::Workflows,
            Section::Services,
            Section::Groupchats,
            Section::Issues,
            Section::Whiteboard,
        ] {
            assert_eq!(
                settings.general.features.enabled(section),
                matches!(section, Section::Docs | Section::Groupchats)
            );
            settings.general.features.toggle(section);
        }
        settings.general.features.toggle(Section::Chats);
        let yaml = serde_yaml_ng::to_string(&settings).unwrap();
        assert!(!yaml.contains("close_picker_after_selection"));
        let restored: Settings = serde_yaml_ng::from_str(&yaml).unwrap();
        assert!(restored.general.features.enabled(Section::Chats));
        for section in [
            Section::Docs,
            Section::Workflows,
            Section::Services,
            Section::Groupchats,
            Section::Issues,
            Section::Whiteboard,
        ] {
            assert_eq!(
                restored.general.features.enabled(section),
                !matches!(section, Section::Docs | Section::Groupchats)
            );
        }
    }
    #[test]
    fn new_settings_never_seed_excluded_mode_preferences() {
        let path = std::env::temp_dir().join(format!(
            "adeline-new-settings-{}-{:?}.yml",
            std::process::id(),
            std::thread::current().id()
        ));
        seed_yaml(&path, &Settings::default()).unwrap();
        update_at(&path, |s| s.general.features.docs = true).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let modes: serde_yaml_ng::Value = serde_yaml_ng::from_str(&saved).unwrap();
        for mode in [
            "workflows",
            "services",
            "groupchats",
            "issues",
            "whiteboard",
        ] {
            assert!(
                modes["modes"].as_mapping().unwrap().get(mode).is_none(),
                "{mode}"
            );
        }
        assert!(read(&path).unwrap().general.features.docs);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn legacy_values_survive_unrelated_updates_without_validation_or_recreation() {
        let path = std::env::temp_dir().join(format!(
            "adeline-legacy-settings-{}-{:?}.yml",
            std::process::id(),
            std::thread::current().id()
        ));
        let original = "general:\n  features:\n    services: true\n    issues: true\nmodes:\n  workflows: 42\n  services: [true, {obsolete: false}]\n  groupchats: {show_left_panel: definitely-not-a-boolean, custom: [one, two]}\n  issues: false\n  whiteboard: this was a scalar\n";
        std::fs::write(&path, original).unwrap();
        let before: serde_yaml_ng::Value = serde_yaml_ng::from_str(original).unwrap();
        update_at(&path, |s| s.general.appearance.font_size = 18).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let after: serde_yaml_ng::Value = serde_yaml_ng::from_str(&saved).unwrap();
        for mode in [
            "workflows",
            "services",
            "groupchats",
            "issues",
            "whiteboard",
        ] {
            assert!(
                after["modes"].as_mapping().unwrap().contains_key(mode),
                "{mode}"
            );
            assert_eq!(before["modes"][mode], after["modes"][mode], "{mode}");
        }
        let settings = read(&path).unwrap();
        assert_eq!(settings.general.appearance.font_size, 18);
        assert!(settings.general.features.services);
        assert!(settings.general.features.issues);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn malformed_active_settings_are_not_overwritten_while_legacy_is_ignored() {
        let path = std::env::temp_dir().join(format!(
            "adeline-invalid-settings-{}-{:?}.yml",
            std::process::id(),
            std::thread::current().id()
        ));
        for invalid in [
            "modes:\n  chats:\n    retry_limit: invalid\n  docs: {old: choice}\n",
            "modes:\n  docs: [malformed\n",
        ] {
            std::fs::write(&path, invalid).unwrap();
            assert!(update_at(&path, |s| s.general.features.docs = true).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn seeding_preserves_existing_file() {
        let root = std::env::temp_dir().join(format!("adeline-config-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.yml");
        std::fs::write(&path, "# keep my file\ngeneral: {}\n").unwrap();
        seed_yaml(&path, &Settings::default()).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("# keep my file")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
