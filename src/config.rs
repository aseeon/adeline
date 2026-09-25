//! Human-editable application preferences. Missing files are seeded, never replaced.
use serde::{Deserialize, Serialize};
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
#[serde(default, deny_unknown_fields)]
pub struct Features {
    // Accept old settings files; picker selections now always close the popup.
    #[serde(skip_serializing)]
    pub close_picker_after_selection: bool,
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
            close_picker_after_selection: true,
            docs: true,
            workflows: true,
            services: true,
            groupchats: true,
            issues: true,
            whiteboard: true,
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
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "lightos.yml".into(),
            interface_font: "System".into(),
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
    pub workflows: Workflows,
    pub services: Services,
    pub groupchats: CollaborationPanels,
    pub issues: CollaborationPanels,
    pub whiteboard: CollaborationPanels,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CollaborationPanels {
    pub show_left_panel: bool,
    pub show_right_panel: bool,
}
impl Default for CollaborationPanels {
    fn default() -> Self {
        Self {
            show_left_panel: true,
            show_right_panel: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Chats {
    pub show_completed_chats: bool,
    pub show_left_panel: bool,
    pub show_agent_activity: bool,
}
impl Default for Chats {
    fn default() -> Self {
        Self {
            show_completed_chats: true,
            show_left_panel: true,
            show_agent_activity: false,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Docs {
    pub show_raw_markdown: bool,
    pub show_archived_documents: bool,
    pub show_left_panel: bool,
    pub show_document_details: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Workflows {
    pub only_scheduled_workflows: bool,
    pub show_left_panel: bool,
    pub show_workflow_details: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Services {
    pub wrap_output_lines: bool,
    pub follow_latest_output: bool,
    pub show_left_panel: bool,
    pub show_service_details: bool,
}
impl Default for Services {
    fn default() -> Self {
        Self {
            wrap_output_lines: true,
            follow_latest_output: true,
            show_left_panel: true,
            show_service_details: false,
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
pub fn error() -> Option<String> {
    ACTIVE.with(|s| s.borrow().error.clone())
}
pub fn font() -> String {
    let value = current().general.appearance.interface_font;
    if value == "System" {
        if cfg!(windows) {
            "Segoe UI"
        } else {
            ".SystemUIFont"
        }
        .into()
    } else {
        value
    }
}
pub fn bind_keys(cx: &mut gpui::App) {
    use super::*;
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
                    &gpui::DummyKeyboardMapper,
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
    bind!(keys.close_dialog_or_popup, Dismiss);
    bind!(keys.next_control, NextFocus);
    bind!(keys.previous_control, PreviousFocus);
    bindings.push(KeyBinding::new("cmd-q", Quit, None));
    cx.bind_keys(bindings);
}
pub fn write_yaml(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = serde_yaml_ng::to_string(value).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("yml.tmp");
    std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn seed_yaml(path: &Path, value: &impl Serialize) -> Result<(), String> {
    use std::io::Write;
    let text = serde_yaml_ng::to_string(value).map_err(|e| e.to_string())?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(text.as_bytes()).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
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
    let path = directory()?.join("settings.yml");
    // Re-read to preserve hand edits and refuse to overwrite malformed files.
    let mut settings = read(&path)?;
    change(&mut settings);
    write_yaml(&path, &settings)?;
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
        self.raw = s.modes.docs.show_raw_markdown;
        self.archived = s.modes.docs.show_archived_documents;
        self.collection = if s.modes.workflows.only_scheduled_workflows {
            "Scheduled"
        } else {
            "All"
        }
        .into();
        self.wrap = s.modes.services.wrap_output_lines;
        self.follow = s.modes.services.follow_latest_output;
        self.left_panel_open = [
            s.modes.chats.show_left_panel,
            s.modes.docs.show_left_panel,
            s.modes.workflows.show_left_panel,
            s.modes.services.show_left_panel,
            s.modes.groupchats.show_left_panel,
            s.modes.issues.show_left_panel,
            s.modes.whiteboard.show_left_panel,
        ];
        self.side_panel_open = [
            s.modes.chats.show_agent_activity,
            s.modes.docs.show_document_details,
            s.modes.workflows.show_workflow_details,
            s.modes.services.show_service_details,
            s.modes.groupchats.show_right_panel,
            s.modes.issues.show_right_panel,
            s.modes.whiteboard.show_right_panel,
        ];
    }
    pub(super) fn save_settings(&self, action: &super::Action) -> Result<(), String> {
        use super::Action;
        update(|s| match action {
            Action::ShowCompleted => s.modes.chats.show_completed_chats = self.show_completed,
            Action::Raw => s.modes.docs.show_raw_markdown = self.raw,
            Action::Archive => s.modes.docs.show_archived_documents = self.archived,
            Action::Collection(_) => {
                s.modes.workflows.only_scheduled_workflows = self.collection == "Scheduled"
            }
            Action::Wrap => s.modes.services.wrap_output_lines = self.wrap,
            Action::Follow => s.modes.services.follow_latest_output = self.follow,
            Action::LeftPanel(section) | Action::RightPanel(section) => {
                self.save_panel(s, *section, matches!(action, Action::LeftPanel(_)))
            }
            Action::ToggleLeftPanel | Action::ToggleSidePanel => {
                self.save_panel(s, self.section, matches!(action, Action::ToggleLeftPanel))
            }
            _ => (),
        })
    }
    fn save_panel(&self, s: &mut Settings, section: super::Section, left: bool) {
        use super::Section;
        let target = match (section, left) {
            (Section::Groupchats, true) => &mut s.modes.groupchats.show_left_panel,
            (Section::Groupchats, false) => &mut s.modes.groupchats.show_right_panel,
            (Section::Issues, true) => &mut s.modes.issues.show_left_panel,
            (Section::Issues, false) => &mut s.modes.issues.show_right_panel,
            (Section::Whiteboard, true) => &mut s.modes.whiteboard.show_left_panel,
            (Section::Whiteboard, false) => &mut s.modes.whiteboard.show_right_panel,
            (Section::Chats, true) => &mut s.modes.chats.show_left_panel,
            (Section::Docs, true) => &mut s.modes.docs.show_left_panel,
            (Section::Workflows, true) => &mut s.modes.workflows.show_left_panel,
            (Section::Services, true) => &mut s.modes.services.show_left_panel,
            (Section::Chats, false) => &mut s.modes.chats.show_agent_activity,
            (Section::Docs, false) => &mut s.modes.docs.show_document_details,
            (Section::Workflows, false) => &mut s.modes.workflows.show_workflow_details,
            (Section::Services, false) => &mut s.modes.services.show_service_details,
        };
        *target = if left {
            self.left_panel_open[section as usize]
        } else {
            self.side_panel_open[section as usize]
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_features_load_and_disabled_modes_persist() {
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
            assert!(settings.general.features.enabled(section));
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
            assert!(!restored.general.features.enabled(section));
        }
    }
    #[test]
    fn yaml_defaults_and_custom_settings_round_trip() {
        let mut s: Settings = serde_yaml_ng::from_str("general:\n  appearance:\n    theme: custom.yml\nmodes:\n  services:\n    wrap_output_lines: false\n").unwrap();
        assert_eq!(s.general.appearance.theme, "custom.yml");
        assert!(!s.modes.services.wrap_output_lines);
        assert!(s.modes.chats.show_completed_chats);
        s.general.features.close_picker_after_selection = true;
        assert_eq!(
            serde_yaml_ng::from_str::<Settings>(&serde_yaml_ng::to_string(&s).unwrap()).unwrap(),
            s
        );
        assert!(
            serde_yaml_ng::from_str::<Settings>(
                "modes:\n  chats:\n    show_completed_chats: wrong"
            )
            .is_err()
        );
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
