//! User agent definitions, stored under the Adeline configuration directory.
use crate::files::{self, checked_id, error as file_error};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The agent file format this version writes. Older files are ignored.
pub const VERSION: u32 = 1;

/// The agent's icon, copied from its harness when the agent is added.
pub const AVATAR: &str = "avatar.svg";

/// A field older files carry and this version ignores: read, never written.
/// Earlier agent files saved a `permission_mode`, which the agent's own modes
/// replace (scope R22).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dropped;

impl<'de> Deserialize<'de> for Dropped {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer).map(|_| Self)
    }
}

/// Whether system instructions add to or replace the harness's own guidance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionsMode {
    #[default]
    Append,
    Overwrite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AgentDefinition {
    pub version: u32,
    pub name: String,
    /// A registry ID, `omp`, or `custom`.
    pub harness: String,
    /// The name a Custom harness reported in its ACP handshake.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    /// Custom harnesses only; others are located when a conversation starts.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    /// Empty when the harness offers no model option.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// Empty when the harness offers no effort option.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effort: String,
    /// The mode new conversations start in; empty keeps the agent's default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    #[serde(default, rename = "permission_mode", skip_serializing)]
    pub dropped_permission_mode: Dropped,
    #[serde(default)]
    pub system_instructions: String,
    #[serde(default)]
    pub instructions_mode: InstructionsMode,
    /// MCP servers for this agent, added to the global ones (scope R28).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<crate::conversation::McpServer>,
}

impl Default for AgentDefinition {
    fn default() -> Self {
        Self {
            version: VERSION,
            name: String::new(),
            harness: String::new(),
            identity: String::new(),
            command: String::new(),
            arguments: Vec::new(),
            model: String::new(),
            effort: String::new(),
            mode: String::new(),
            dropped_permission_mode: Dropped,
            system_instructions: String::new(),
            instructions_mode: InstructionsMode::Append,
            mcp_servers: Vec::new(),
        }
    }
}

impl AgentDefinition {
    pub(crate) fn validate(&self) -> Result<String, String> {
        if self.version != VERSION {
            return Err(format!("Agent file version must be {VERSION}."));
        }
        for (field, value) in [("Name", &self.name), ("Harness", &self.harness)] {
            if value.trim().is_empty() {
                return Err(format!("{field} is required."));
            }
        }
        if self.harness == crate::harness::CUSTOM {
            if self.command.trim().is_empty() {
                return Err("Command is required.".into());
            }
            if self.command != self.command.trim()
                || (self.command.chars().any(char::is_whitespace)
                    && !Path::new(&self.command).is_file()
                    && !looks_like_executable_path(&self.command))
            {
                return Err(
                    "Command must be one executable name or path; enter each argument separately."
                        .into(),
                );
            }
        } else if !self.command.is_empty() || !self.arguments.is_empty() {
            return Err("Only Custom agents store a command and arguments.".into());
        }
        normalize_name(&self.name)
    }
}

fn looks_like_executable_path(command: &str) -> bool {
    (command.contains('/') || command.contains('\\') || command.contains(':'))
        && command.rsplit_once('.').is_some_and(|(_, extension)| {
            ["exe", "cmd", "bat", "sh"]
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

/// A folder ID is made only from letters, digits and single separators.
pub(crate) fn normalize_name(name: &str) -> Result<String, String> {
    let mut id = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            id.push(c);
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_end_matches('-').to_owned();
    let reserved = matches!(id.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|prefix| {
            id.strip_prefix(*prefix).is_some_and(|number| {
                matches!(
                    number,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    if id.is_empty() || reserved || id.len() > 255 || id.encode_utf16().count() > 255 {
        return Err("Name does not produce a valid agent folder name.".into());
    }
    Ok(id)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEntry {
    pub id: String,
    pub definition: AgentDefinition,
}

pub struct AgentCatalog {
    pub entries: Vec<AgentEntry>,
    pub errors: Vec<String>,
    root: Result<PathBuf, String>,
    demo: bool,
}

impl AgentCatalog {
    pub fn new(demo: bool) -> Self {
        Self::with_root(
            crate::config::directory().map(|path| path.join("agents")),
            demo,
        )
    }

    /// A client's copy of the engine's agents; the engine saves them.
    pub fn remote() -> Self {
        Self {
            entries: Vec::new(),
            errors: Vec::new(),
            root: Err("Agents are saved by the conversation engine.".into()),
            demo: false,
        }
    }

    fn with_root(root: Result<PathBuf, String>, demo: bool) -> Self {
        let mut catalog = Self {
            entries: Vec::new(),
            errors: Vec::new(),
            root,
            demo,
        };
        if !demo && let Ok(path) = &catalog.root {
            // Create this directory before the parent starts watching for changes.
            if let Err(error) = fs::create_dir_all(path) {
                catalog.errors.push(file_error(path, error));
                return catalog;
            }
        }
        if demo {
            for (name, model) in [
                ("Claude Code", "anthropic/claude-opus-5"),
                ("Codex", "openai-codex/gpt-6-luna"),
                ("Grok Build", "xai/grok-build"),
                ("Antigravity", "google/antigravity"),
            ] {
                catalog.entries.push(AgentEntry {
                    id: normalize_name(name).expect("valid demo agent name"),
                    definition: AgentDefinition {
                        name: name.into(),
                        harness: crate::harness::OMP.into(),
                        model: model.into(),
                        effort: "high".into(),
                        ..Default::default()
                    },
                });
            }
        } else {
            catalog.refresh();
        }
        catalog
    }

    /// Returns whether visible definitions or filesystem errors changed.
    pub fn refresh(&mut self) -> bool {
        if self.demo {
            return false;
        }
        let (mut entries, mut errors) = match &self.root {
            Ok(root) => discover(root),
            Err(error) => (Vec::new(), vec![error.clone()]),
        };
        entries.sort_by(|a, b| {
            a.definition
                .name
                .to_lowercase()
                .cmp(&b.definition.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        errors.sort();
        let changed = self.entries != entries || self.errors != errors;
        self.entries = entries;
        self.errors = errors;
        changed
    }

    /// `expected` is the definition loaded when editing began. A changed, deleted,
    /// or invalid file requires an explicit overwrite decision.
    pub fn save(
        &mut self,
        original: Option<&str>,
        definition: AgentDefinition,
        expected: Option<&AgentDefinition>,
        overwrite: bool,
    ) -> Result<String, String> {
        let id = definition.validate()?;
        if self.demo {
            if let Some(original) = original
                && !self.entries.iter().any(|entry| entry.id == original)
                && !overwrite
            {
                return Err(format!(
                    "Agent {original} changed outside this form. Reload or overwrite it."
                ));
            }
            if original != Some(id.as_str()) && self.entries.iter().any(|entry| entry.id == id) {
                return Err(format!("Agent folder {id} already exists."));
            }
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| Some(entry.id.as_str()) == original)
            {
                entry.id.clone_from(&id);
                entry.definition = definition;
            } else {
                self.entries.push(AgentEntry {
                    id: id.clone(),
                    definition,
                });
            }
            return Ok(id);
        }

        let root = self.root.as_ref().map_err(Clone::clone)?;
        if let Some(original) = original {
            checked_id(original)?;
        }
        fs::create_dir_all(root).map_err(|e| file_error(root, e))?;
        let old = original.map(|id| root.join(id));
        if let Some(path) = old.as_deref() {
            match fs::symlink_metadata(path) {
                Ok(meta) if !meta.file_type().is_dir() => {
                    return Err(format!(
                        "{}: agent folder is not a directory.",
                        path.join("agent.yml").display()
                    ));
                }
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    return Err(file_error(&path.join("agent.yml"), error));
                }
                _ => {}
            }
        }
        if let Some(path) = old.as_deref() {
            let disk = load(path).ok().flatten();
            if !overwrite && (expected.is_none() || disk.as_ref() != expected) {
                return Err(format!(
                    "{}: definition changed outside this form. Reload or overwrite it.",
                    path.join("agent.yml").display()
                ));
            }
        }
        if original != Some(id.as_str())
            && let Some(path) = old.as_deref()
            && path.exists()
        {
            ensure_only_definition(path)?;
        }
        let destination = root.join(&id);
        if original == Some(id.as_str()) {
            if !destination.exists() {
                fs::create_dir(&destination).map_err(|e| file_error(&destination, e))?;
            }
            let path = destination.join("agent.yml");
            if path.exists() {
                replace(&path, &definition)?;
            } else {
                write_new(&path, &definition)?;
            }
        } else {
            // The directory itself is reserved exclusively. No invalid or missing
            // definition can be overwritten by a new agent or a rename.
            ensure_no_collision(root, &id)?;
            fs::create_dir(&destination).map_err(|e| file_error(&destination, e))?;
            if let Err(error) = write_new(&destination.join("agent.yml"), &definition) {
                let _ = fs::remove_dir(&destination);
                return Err(error);
            }
            // The avatar is decoration: a missing one falls back to the robot.
            let avatar = match old.as_deref() {
                Some(old) => fs::read(old.join(AVATAR)).ok(),
                None => crate::harness::icon_svg(&definition.harness),
            };
            if let Some(avatar) = avatar {
                let _ = files::write_new(&destination.join(AVATAR), &avatar);
            }
            if let Some(old) = old.as_deref()
                && old.exists()
                && let Err(error) = fs::remove_dir_all(old)
            {
                // Keep the complete new definition if removing the old directory
                // fails or stops partway through.
                self.refresh();
                return Err(file_error(old, error));
            }
        }
        self.refresh();
        Ok(id)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        checked_id(id)?;
        if self.demo {
            let index = self
                .entries
                .iter()
                .position(|entry| entry.id == id)
                .ok_or_else(|| format!("Agent {id} no longer exists."))?;
            self.entries.remove(index);
            return Ok(());
        }
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let old = self
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("Agent {id} no longer exists."))?;
        let path = root.join(id);
        if load(&path).ok().flatten().as_ref() != Some(&old.definition) {
            return Err(format!(
                "{}: definition changed outside this form. Reload before deleting it.",
                path.join("agent.yml").display()
            ));
        }
        fs::remove_dir_all(&path).map_err(|e| file_error(&path, e))?;
        self.refresh();
        Ok(())
    }
}

/// An agent's avatar asset path, else its harness icon.
pub fn avatar_path(id: &str, harness: &str) -> String {
    let path = format!("agent-avatars/{id}.svg");
    if crate::harness::has_icon(&path) {
        path
    } else {
        crate::harness::icon_path(harness)
    }
}

/// `None` for a file older than [`VERSION`]: it is left alone and not listed.
fn load(folder: &Path) -> Result<Option<AgentDefinition>, String> {
    let path = folder.join("agent.yml");
    if !fs::symlink_metadata(folder)
        .map_err(|e| file_error(&path, e))?
        .file_type()
        .is_dir()
    {
        return Err(format!(
            "{}: agent folder is not a directory.",
            path.display()
        ));
    }
    let text = fs::read_to_string(&path).map_err(|e| file_error(&path, e))?;
    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).map_err(|e| file_error(&path, e))?;
    if yaml
        .get("version")
        .and_then(serde_yaml_ng::Value::as_u64)
        .is_none_or(|version| version < u64::from(VERSION))
    {
        return Ok(None);
    }
    let definition: AgentDefinition =
        serde_yaml_ng::from_value(yaml).map_err(|e| file_error(&path, e))?;
    definition.validate().map_err(|e| file_error(&path, e))?;
    Ok(Some(definition))
}

fn discover(root: &Path) -> (Vec<AgentEntry>, Vec<String>) {
    let directory = match fs::read_dir(root) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(error) => return (Vec::new(), vec![file_error(root, error)]),
    };
    let (mut entries, mut errors) = (Vec::new(), Vec::new());
    for entry in directory {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(file_error(root, error));
                continue;
            }
        };
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => {}
            Ok(_) => continue,
            Err(error) => {
                errors.push(file_error(&path, error));
                continue;
            }
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let result = checked_id(&id)
            .map_err(|e| file_error(&path.join("agent.yml"), e))
            .and_then(|()| load(&path));
        match result {
            Ok(Some(definition)) => entries.push(AgentEntry { id, definition }),
            Ok(None) => {}
            Err(error) => errors.push(error),
        }
    }
    (entries, errors)
}

fn ensure_only_definition(folder: &Path) -> Result<(), String> {
    for entry in fs::read_dir(folder).map_err(|e| file_error(folder, e))? {
        let entry = entry.map_err(|e| file_error(folder, e))?;
        if entry.file_name() != "agent.yml" && entry.file_name() != AVATAR {
            return Err(format!(
                "{}: cannot rename a folder containing other files.",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn ensure_no_collision(root: &Path, id: &str) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| file_error(root, e))? {
        let entry = entry.map_err(|e| file_error(root, e))?;
        if entry.file_name().to_string_lossy().to_lowercase() == id {
            return Err(format!(
                "{}: agent folder already exists.",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn yaml(definition: &AgentDefinition, path: &Path) -> Result<String, String> {
    serde_yaml_ng::to_string(definition).map_err(|e| file_error(path, e))
}

fn write_new(path: &Path, definition: &AgentDefinition) -> Result<(), String> {
    files::write_new(path, yaml(definition, path)?.as_bytes())
}

fn replace(path: &Path, definition: &AgentDefinition) -> Result<(), String> {
    files::replace(path, yaml(definition, path)?.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::files::TempDir;

    fn open_catalog(root: &Path, demo: bool) -> AgentCatalog {
        AgentCatalog::with_root(Ok(root.join("agents")), demo)
    }
    fn file(root: &Path, id: &str) -> PathBuf {
        root.join("agents").join(id).join("agent.yml")
    }
    fn agent(name: &str) -> AgentDefinition {
        AgentDefinition {
            name: name.into(),
            harness: crate::harness::CUSTOM.into(),
            identity: "omp".into(),
            command: "omp.exe".into(),
            arguments: vec!["acp".into()],
            model: "openai-codex/gpt-6-luna".into(),
            effort: "max".into(),
            system_instructions: "Answer plainly.".into(),
            ..Default::default()
        }
    }

    #[test]
    fn registry_agents_store_identity_and_defaults_but_no_command() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let gemini = AgentDefinition {
            name: "Gem".into(),
            harness: "gemini".into(),
            model: "gemini-3-pro".into(),
            effort: "high".into(),
            instructions_mode: InstructionsMode::Overwrite,
            ..Default::default()
        };
        catalog.save(None, gemini.clone(), None, false).unwrap();
        let yaml = fs::read_to_string(file(&root, "gem")).unwrap();
        assert!(yaml.starts_with("version: 1\n"), "{yaml}");
        for field in [
            "name: Gem",
            "harness: gemini",
            "model: gemini-3-pro",
            "effort: high",
            "system_instructions:",
            "instructions_mode: Overwrite",
        ] {
            assert!(yaml.contains(field), "{field} in {yaml}");
        }
        assert!(!yaml.contains("command"));
        assert!(!yaml.contains("permission_mode"));
        assert_eq!(open_catalog(&root, false).entries[0].definition, gemini);
        let mut with_command = gemini;
        with_command.name = "Other".into();
        with_command.command = "gemini".into();
        assert!(catalog.save(None, with_command, None, false).is_err());
        let custom = agent("Custom one");
        catalog.save(None, custom.clone(), None, false).unwrap();
        let yaml = fs::read_to_string(file(&root, "custom-one")).unwrap();
        assert!(yaml.contains("command: omp.exe") && yaml.contains("- acp"));
        let mut no_model = custom;
        no_model.name = "Defaults".into();
        no_model.model.clear();
        no_model.effort.clear();
        catalog.save(None, no_model.clone(), None, false).unwrap();
        assert!(
            open_catalog(&root, false)
                .entries
                .iter()
                .any(|entry| entry.definition == no_model)
        );
    }

    #[test]
    fn a_saved_permission_mode_loads_and_is_dropped_on_save() {
        let root = TempDir::new("adeline-agents");
        fs::create_dir_all(file(&root, "josh").parent().unwrap()).unwrap();
        fs::write(
            file(&root, "josh"),
            "version: 1
name: Josh
harness: omp
permission_mode: AllowEverything
",
        )
        .unwrap();
        let mut catalog = open_catalog(&root, false);
        let loaded = catalog.entries[0].definition.clone();
        assert_eq!(loaded.name, "Josh");
        catalog
            .save(Some("josh"), loaded.clone(), Some(&loaded), false)
            .unwrap();
        let yaml = fs::read_to_string(file(&root, "josh")).unwrap();
        assert!(!yaml.contains("permission_mode"), "{yaml}");
    }

    #[test]
    fn old_format_files_are_silently_ignored_and_left_unchanged() {
        let root = TempDir::new("adeline-agents");
        let old = "name: Josh\nharness: OMP\ndriver: ACP\ncommand: omp.exe acp\nmodel: openai-codex/gpt-6-luna\neffort: Max\n";
        let older = "version: 0\nname: Old\nharness: omp\n";
        for (id, text) in [("josh", old), ("old", older)] {
            fs::create_dir_all(file(&root, id).parent().unwrap()).unwrap();
            fs::write(file(&root, id), text).unwrap();
        }
        let mut catalog = open_catalog(&root, false);
        assert!(catalog.entries.is_empty());
        assert!(catalog.errors.is_empty());
        assert_eq!(fs::read_to_string(file(&root, "josh")).unwrap(), old);
        assert_eq!(fs::read_to_string(file(&root, "old")).unwrap(), older);
        assert!(catalog.save(None, agent("Josh"), None, false).is_err());
        assert_eq!(fs::read_to_string(file(&root, "josh")).unwrap(), old);
    }

    #[test]
    fn accepts_explicit_command_path_with_spaces_before_installation() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let mut definition = agent("Future");
        definition.command = root
            .join("Program Files")
            .join("omp.exe")
            .to_string_lossy()
            .into_owned();
        catalog.save(None, definition.clone(), None, false).unwrap();
        assert_eq!(open_catalog(&root, false).entries[0].definition, definition);
    }

    #[test]
    fn persists_complete_definitions_and_refreshes_external_changes() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let josh = agent("Josh");
        assert_eq!(
            catalog.save(None, josh.clone(), None, false).unwrap(),
            "josh"
        );
        assert_eq!(
            serde_yaml_ng::from_str::<AgentDefinition>(
                &fs::read_to_string(file(&root, "josh")).unwrap()
            )
            .unwrap(),
            josh
        );
        assert_eq!(open_catalog(&root, false).entries[0].definition, josh);
        let external = agent("Outside");
        fs::create_dir(file(&root, "outside").parent().unwrap()).unwrap();
        fs::write(
            file(&root, "outside"),
            serde_yaml_ng::to_string(&external).unwrap(),
        )
        .unwrap();
        assert!(catalog.refresh());
        assert_eq!(catalog.entries.len(), 2);
        let mut edited = josh;
        edited.arguments = vec!["acp".into(), "--verbose".into()];
        edited.name = "Renamed externally".into();
        fs::write(
            file(&root, "josh"),
            serde_yaml_ng::to_string(&edited).unwrap(),
        )
        .unwrap();
        assert!(catalog.refresh());
        assert_eq!(
            catalog
                .entries
                .iter()
                .find(|entry| entry.id == "josh")
                .unwrap()
                .definition,
            edited
        );
        fs::remove_dir_all(file(&root, "josh").parent().unwrap()).unwrap();
        assert!(catalog.refresh());
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].definition, external);
    }

    #[test]
    fn normalizes_names_but_never_overwrites_collisions() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        assert_eq!(
            catalog
                .save(None, agent(" -- Josh...  Smith !! "), None, false)
                .unwrap(),
            "josh-smith"
        );
        let before = fs::read(file(&root, "josh-smith")).unwrap();
        assert!(catalog.save(None, agent("Josh Smith"), None, true).is_err());
        assert_eq!(fs::read(file(&root, "josh-smith")).unwrap(), before);
        for invalid in [" -- !!! ", "CON", "com1", "COM¹", "LPT9"] {
            assert!(
                catalog.save(None, agent(invalid), None, false).is_err(),
                "{invalid}"
            );
        }
        assert!(
            catalog
                .save(None, agent(&"a".repeat(256)), None, false)
                .is_err()
        );
        fs::create_dir(root.join("agents").join("JOSH")).unwrap();
        assert!(catalog.save(None, agent("Josh"), None, true).is_err());
    }

    #[test]
    fn validates_fields_and_reports_bad_neighbors_without_hiding_valid_agents() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        for change in [
            ("name", ""),
            ("command", "  "),
            ("command", "omp.exe acp"),
            ("harness", ""),
        ] {
            let mut bad = agent("Bad");
            match change.0 {
                "name" => bad.name = change.1.into(),
                "command" => bad.command = change.1.into(),
                _ => bad.harness = change.1.into(),
            }
            assert!(
                catalog.save(None, bad, None, false).is_err(),
                "{}",
                change.0
            );
        }
        let mut good = agent("Good");
        good.system_instructions.clear();
        good.arguments.clear();
        catalog.save(None, good.clone(), None, false).unwrap();
        fs::create_dir(root.join("agents/bad")).unwrap();
        let mut bad = agent("Bad");
        bad.command = "omp.exe acp".into();
        fs::write(file(&root, "bad"), serde_yaml_ng::to_string(&bad).unwrap()).unwrap();
        fs::create_dir(root.join("agents/missing")).unwrap();
        assert!(catalog.refresh());
        assert_eq!(catalog.entries[0].definition, good);
        assert_eq!(catalog.errors.len(), 2);
        assert!(catalog.errors.iter().any(|error| error.contains("bad")
            && error.contains("agent.yml")
            && error.contains("Command")));
        assert!(
            catalog
                .errors
                .iter()
                .any(|error| error.contains("missing") && error.contains("agent.yml"))
        );
    }

    #[test]
    fn external_edit_or_invalid_or_deleted_file_conflicts_until_overwrite() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let original = agent("Josh");
        catalog.save(None, original.clone(), None, false).unwrap();
        let mut mine = original.clone();
        mine.model = "openai-codex/my-model".into();
        let mut theirs = original.clone();
        theirs.arguments = vec!["acp".into(), "--external".into()];
        fs::write(
            file(&root, "josh"),
            serde_yaml_ng::to_string(&theirs).unwrap(),
        )
        .unwrap();
        assert!(
            catalog
                .save(Some("josh"), mine.clone(), Some(&original), false)
                .unwrap_err()
                .contains("agent.yml")
        );
        assert_eq!(
            fs::read_to_string(file(&root, "josh")).unwrap(),
            serde_yaml_ng::to_string(&theirs).unwrap()
        );
        fs::write(file(&root, "josh"), "name: Broken\n").unwrap();
        assert!(
            catalog
                .save(Some("josh"), mine.clone(), Some(&original), false)
                .is_err()
        );
        fs::remove_file(file(&root, "josh")).unwrap();
        assert!(
            catalog
                .save(Some("josh"), mine.clone(), Some(&original), false)
                .is_err()
        );
        catalog
            .save(Some("josh"), mine.clone(), Some(&original), true)
            .unwrap();
        assert_eq!(open_catalog(&root, false).entries[0].definition, mine);
    }

    #[test]
    fn adding_copies_the_harness_icon_and_renaming_keeps_it() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let josh = AgentDefinition {
            name: "Josh".into(),
            harness: crate::harness::OMP.into(),
            ..Default::default()
        };
        catalog.save(None, josh.clone(), None, false).unwrap();
        let avatar = |id: &str| fs::read(file(&root, id).parent().unwrap().join(AVATAR)).unwrap();
        assert_eq!(avatar("josh"), crate::embedded("omp.svg").unwrap());
        let mut renamed = josh.clone();
        renamed.name = "Josh Smith".into();
        catalog
            .save(Some("josh"), renamed, Some(&josh), false)
            .unwrap();
        assert_eq!(avatar("josh-smith"), crate::embedded("omp.svg").unwrap());
        assert!(!file(&root, "josh").exists());
    }

    #[test]
    fn renames_and_deletes_without_damaging_existing_destinations() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let josh = agent("Josh");
        let alice = agent("Alice");
        catalog.save(None, josh.clone(), None, false).unwrap();
        catalog.save(None, alice.clone(), None, false).unwrap();
        assert!(
            catalog
                .save(Some("josh"), alice.clone(), Some(&josh), true)
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(file(&root, "alice")).unwrap(),
            serde_yaml_ng::to_string(&alice).unwrap()
        );
        let mut renamed = josh.clone();
        renamed.name = "Josh Smith".into();
        let extra = file(&root, "josh").parent().unwrap().join("notes.txt");
        fs::write(&extra, "keep this").unwrap();
        assert!(
            catalog
                .save(Some("josh"), renamed.clone(), Some(&josh), false)
                .is_err()
        );
        assert_eq!(fs::read_to_string(&extra).unwrap(), "keep this");
        assert!(!file(&root, "josh-smith").exists());
        fs::remove_file(extra).unwrap();
        catalog
            .save(Some("josh"), renamed.clone(), Some(&josh), false)
            .unwrap();
        assert!(!file(&root, "josh").exists());
        assert_eq!(
            open_catalog(&root, false)
                .entries
                .iter()
                .find(|entry| entry.id == "josh-smith")
                .unwrap()
                .definition,
            renamed
        );
        let mut changed = renamed;
        changed.arguments = vec!["acp".into(), "--external".into()];
        fs::write(
            file(&root, "josh-smith"),
            serde_yaml_ng::to_string(&changed).unwrap(),
        )
        .unwrap();
        assert!(catalog.delete("josh-smith").is_err());
        assert!(file(&root, "josh-smith").exists());
        catalog.refresh();
        catalog.delete("josh-smith").unwrap();
        assert!(!file(&root, "josh-smith").parent().unwrap().exists());
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].definition, alice);
    }

    #[test]
    fn demo_store_never_reads_or_writes_real_definitions() {
        let root = TempDir::new("adeline-agents");
        let mut real = open_catalog(&root, false);
        real.save(None, agent("Personal"), None, false).unwrap();
        let mut demo = open_catalog(&root, true);
        assert_eq!(demo.entries.len(), 4);
        assert!(!demo.entries.iter().any(|entry| entry.id == "personal"));
        let first = demo.entries[0].clone();
        let mut edited = first.definition.clone();
        edited.model = "demo-only".into();
        demo.save(Some(&first.id), edited, Some(&first.definition), false)
            .unwrap();
        demo.delete(&first.id).unwrap();
        demo.save(None, agent("Session"), None, false).unwrap();
        assert!(demo.entries.iter().any(|entry| entry.id == "session"));
        assert_eq!(open_catalog(&root, true).entries.len(), 4);
        assert_eq!(
            open_catalog(&root, false).entries[0].definition.name,
            "Personal"
        );
        assert!(!file(&root, "session").exists());
    }
}
