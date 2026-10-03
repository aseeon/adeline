//! Human-editable application preferences. Missing files are seeded, never replaced.
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

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
    pub machine_selector: bool,
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
            machine_selector: false,
            docs: false,
            workflows: false,
            services: false,
            groupchats: true,
            issues: false,
            whiteboard: false,
        }
    }
}
impl Features {
    pub fn enabled(&self, section: super::Section) -> bool {
        use super::Section;
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
    pub fn toggle(&mut self, section: super::Section) {
        use super::Section;
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
            interface_font: crate::fonts::DEFAULT.into(),
            font_size: 14,
            code_font: crate::fonts::CODE_DEFAULT.into(),
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
#[non_exhaustive]
#[serde(default, deny_unknown_fields)]
pub struct Modes {
    pub chats: Chats,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_value"
    )]
    docs: Option<serde_yaml_ng::Value>,
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
    /// Enter sends the message and Shift+Enter starts a new line.
    pub submit_on_enter: bool,
    /// Moved to the engine's own settings; kept so older files round-trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_limit: Option<usize>,
    pub thinking_animation: ThinkingAnimation,
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
        }
    }
}
/// How the "Thinking…" row moves while an agent works on a turn.
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
pub fn directory() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|p| PathBuf::from(p).join(".config/adeline"))
        .ok_or_else(|| "Could not find your home directory.".into())
}
pub fn current() -> Settings {
    ACTIVE.with(|s| s.borrow().settings.clone())
}
/// Reads one value without cloning all settings, for render paths.
pub fn with<T>(f: impl FnOnce(&Settings) -> T) -> T {
    ACTIVE.with(|s| f(&s.borrow().settings))
}
pub fn error() -> Option<String> {
    ACTIVE.with(|s| s.borrow().error.clone())
}
pub fn font() -> String {
    crate::fonts::resolve(
        &current().general.appearance.interface_font,
        crate::fonts::DEFAULT,
    )
}
pub fn code_font() -> String {
    crate::fonts::resolve(
        &current().general.appearance.code_font,
        crate::fonts::CODE_DEFAULT,
    )
}

pub fn code_font_size() -> u16 {
    ACTIVE.with(|s| {
        s.borrow()
            .settings
            .general
            .appearance
            .code_font_size
            .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
    })
}

/// Virtual rows must be measured again when either font family or size changes.
#[derive(PartialEq)]
pub struct Typography {
    interface: String,
    interface_size: u16,
    code: String,
    code_size: u16,
}

pub fn typography() -> Typography {
    Typography {
        interface: font(),
        interface_size: font_size(),
        code: code_font(),
        code_size: code_font_size(),
    }
}
pub const MIN_FONT_SIZE: u16 = 10;
pub const MAX_FONT_SIZE: u16 = 24;

pub fn font_size() -> u16 {
    ACTIVE.with(|s| {
        s.borrow()
            .settings
            .general
            .appearance
            .font_size
            .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
    })
}

pub fn text_pixels(base: f32) -> gpui_kit::Pixels {
    gpui_kit::px(base * f32::from(font_size()) / 14.)
}
pub fn bind_keys(cx: &mut gpui_kit::App) {
    use super::*;
    use gpui_kit::base::actions::Cancel;
    let keys = current().general.keymap;
    let mut bindings = Vec::new();
    macro_rules! bind {
        ($keys:expr, $action:expr) => {
            for key in &$keys {
                match KeyBinding::load(
                    key,
                    Box::new($action),
                    None,
                    false,
                    None,
                    &gpui_kit::DummyKeyboardMapper,
                ) {
                    Ok(binding) => bindings.push(binding),
                    Err(error) => ACTIVE.with(|s| {
                        s.borrow_mut().error = Some(format!("Invalid shortcut {key}: {error}"))
                    }),
                }
            }
        };
    }
    bind!(keys.open_settings, OpenSettings);
    bind!(keys.new_chat, NewThread);
    bind!(keys.focus_search, Search);
    bind!(keys.send_message, SendMessage);
    bind!(keys.close_dialog_or_popup, Cancel);
    bind!(keys.next_control, NextFocus);
    bind!(keys.previous_control, PreviousFocus);
    bindings.push(KeyBinding::new("cmd-q", Quit, None));
    cx.bind_keys(bindings);
}
pub fn write_yaml(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = serde_yaml_ng::to_string(value).map_err(|e| e.to_string())?;
    crate::files::replace(path, text.as_bytes())
}
pub fn seed_yaml(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = serde_yaml_ng::to_string(value).map_err(|e| e.to_string())?;
    crate::files::seed(path, text.as_bytes())
}
fn read(path: &Path) -> Result<Settings, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_yaml_ng::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn init() {
    let result = (|| {
        let root = directory()?;
        std::fs::create_dir_all(root.join("themes")).map_err(|e| e.to_string())?;
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
        Err(error) => s.borrow_mut().error = Some(error),
    });
}
pub fn update(change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    update_at(&directory()?.join("settings.yml"), change)
}
fn update_at(path: &Path, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
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
impl super::Adeline {
    pub(super) fn load_settings(&mut self) {
        let s = current();
        self.show_completed = s.modes.chats.show_completed_chats;
        self.show_archived = s.modes.chats.show_archived_chats;
        self.left_panel_open = [
            s.modes.chats.show_left_panel,
            false,
            false,
            false,
            false,
            false,
            false,
        ];
        self.side_panel_open = [
            s.modes.chats.show_agent_activity,
            false,
            false,
            false,
            false,
            false,
            false,
        ];
    }
    pub(super) fn save_settings(&self, action: &super::Action) -> Result<(), String> {
        use super::{Action, Section};
        update(|s| match action {
            Action::ShowCompleted => s.modes.chats.show_completed_chats = self.show_completed,
            Action::ShowArchived => s.modes.chats.show_archived_chats = self.show_archived,
            Action::ToggleLeftPanel => {
                s.modes.chats.show_left_panel = self.left_panel_open[Section::Chats as usize];
            }
            Action::ToggleSidePanel => {
                s.modes.chats.show_agent_activity = self.side_panel_open[Section::Chats as usize];
            }
            _ => (),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interface_and_code_preferences_are_independent_and_backward_compatible() {
        let defaults: Appearance = serde_yaml_ng::from_str("{}").unwrap();
        assert_eq!(defaults.interface_font, "Chivo");
        assert_eq!(defaults.code_font, "Chivo Mono");
        assert_eq!(defaults.code_font_size, 14);
        let legacy: Appearance =
            serde_yaml_ng::from_str("interface_font: Arial\nfont_size: 18").unwrap();
        assert_eq!(legacy.interface_font, "Arial");
        assert_eq!(legacy.font_size, 18);
        assert_eq!(legacy.code_font, "Chivo Mono");
        assert_eq!(legacy.code_font_size, 14);
        let original = current();
        ACTIVE.with(|state| {
            let mut state = state.borrow_mut();
            state.settings.general.appearance = legacy;
            state.settings.general.appearance.code_font = "Missing code family".into();
            state.settings.general.appearance.interface_font = "Missing interface family".into();
        });
        assert_eq!(font(), "Chivo");
        assert_eq!(code_font(), "Chivo Mono");
        for (requested, expected) in [(0, 10), (14, 14), (20, 20), (65535, 24)] {
            ACTIVE.with(|s| s.borrow_mut().settings.general.appearance.code_font_size = requested);
            assert_eq!(font_size(), 18);
            assert_eq!(code_font_size(), expected);
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
    fn thinking_animation_defaults_to_dots_and_round_trips_by_name() {
        let defaults: Chats = serde_yaml_ng::from_str("{}").unwrap();
        assert_eq!(defaults.thinking_animation, ThinkingAnimation::Dots);
        for choice in ThinkingAnimation::ALL {
            let yaml = serde_yaml_ng::to_string(&Chats {
                thinking_animation: choice,
                ..Chats::default()
            })
            .unwrap();
            assert_eq!(
                serde_yaml_ng::from_str::<Chats>(&yaml)
                    .unwrap()
                    .thinking_animation,
                choice
            );
        }
        let saved: Chats = serde_yaml_ng::from_str("thinking_animation: fill").unwrap();
        assert_eq!(saved.thinking_animation, ThinkingAnimation::Fill);
    }

    #[test]
    fn font_size_defaults_round_trips_and_bounds_rendering() {
        let mut appearance: Appearance = serde_yaml_ng::from_str("theme: custom.yml").unwrap();
        assert_eq!(appearance.font_size, 14);
        appearance.font_size = 18;
        let restored: Appearance =
            serde_yaml_ng::from_str(&serde_yaml_ng::to_string(&appearance).unwrap()).unwrap();
        assert_eq!(restored.font_size, 18);
        let original = current();
        for (requested, expected) in [(0, 10), (14, 14), (18, 18), (65535, 24)] {
            ACTIVE.with(|s| s.borrow_mut().settings.general.appearance.font_size = requested);
            assert_eq!(font_size(), expected);
            assert_eq!(text_pixels(14.), gpui_kit::px(f32::from(expected)));
            assert_eq!(text_pixels(21.), gpui_kit::px(f32::from(expected) * 1.5));
        }
        ACTIVE.with(|s| s.borrow_mut().settings = original);
    }
    #[test]
    fn legacy_features_use_new_defaults_and_explicit_choices_persist() {
        use crate::Section;
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
                section == Section::Groupchats
            );
            settings.general.features.toggle(section);
        }
        assert!(!settings.general.features.machine_selector);
        settings.general.features.machine_selector = true;
        settings.general.features.toggle(Section::Chats);
        let yaml = serde_yaml_ng::to_string(&settings).unwrap();
        assert!(!yaml.contains("close_picker_after_selection"));
        let restored: Settings = serde_yaml_ng::from_str(&yaml).unwrap();
        assert!(restored.general.features.enabled(Section::Chats));
        assert!(restored.general.features.machine_selector);
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
                section != Section::Groupchats
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
            "docs",
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
    fn conversation_list_starts_expanded_then_remembers_the_last_choice() {
        let path = std::env::temp_dir().join(format!(
            "adeline-left-panel-{}-{:?}.yml",
            std::process::id(),
            std::thread::current().id()
        ));
        seed_yaml(&path, &Settings::default()).unwrap();
        assert!(read(&path).unwrap().modes.chats.show_left_panel);
        update_at(&path, |s| s.modes.chats.show_left_panel = false).unwrap();
        assert!(!read(&path).unwrap().modes.chats.show_left_panel);
        update_at(&path, |s| s.modes.chats.show_left_panel = true).unwrap();
        assert!(read(&path).unwrap().modes.chats.show_left_panel);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn legacy_values_survive_unrelated_updates_without_validation_or_recreation() {
        let path = std::env::temp_dir().join(format!(
            "adeline-legacy-settings-{}-{:?}.yml",
            std::process::id(),
            std::thread::current().id()
        ));
        let original = "general:\n  features:\n    services: true\n    issues: true\nmodes:\n  docs: null\n  workflows: 42\n  services: [true, {obsolete: false}]\n  groupchats: {show_left_panel: definitely-not-a-boolean, custom: [one, two]}\n  issues: false\n  whiteboard: this was a scalar\n";
        std::fs::write(&path, original).unwrap();
        let before: serde_yaml_ng::Value = serde_yaml_ng::from_str(original).unwrap();
        update_at(&path, |s| s.general.appearance.font_size = 18).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let after: serde_yaml_ng::Value = serde_yaml_ng::from_str(&saved).unwrap();
        for mode in [
            "docs",
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
