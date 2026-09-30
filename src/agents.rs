//! User agent definitions, stored under the Adeline configuration directory.
use crate::files::{self, checked_id, error as file_error};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const EFFORTS: [&str; 5] = ["Low", "Medium", "High", "Extra High", "Max"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffortParameterName {
    #[default]
    Thinking,
    Effort,
    ReasoningEffort,
    ThoughtLevel,
}

impl EffortParameterName {
    pub const ALL: [Self; 4] = [
        Self::Thinking,
        Self::Effort,
        Self::ReasoningEffort,
        Self::ThoughtLevel,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Thinking => "thinking",
            Self::Effort => "effort",
            Self::ReasoningEffort => "reasoning_effort",
            Self::ThoughtLevel => "thought_level",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    #[default]
    Ask,
    AllowEverything,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AgentDefinition {
    pub name: String,
    pub harness: String,
    pub driver: String,
    pub command: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub permission_mode: PermissionMode,
    pub model: String,
    pub effort: String,
    #[serde(default)]
    pub effort_parameter_name: EffortParameterName,
    #[serde(default)]
    pub system_instructions: String,
}

impl AgentDefinition {
    pub(crate) fn validate(&self) -> Result<String, String> {
        for (field, value) in [
            ("Name", &self.name),
            ("Harness", &self.harness),
            ("Driver", &self.driver),
            ("Command", &self.command),
            ("Provider/model", &self.model),
            ("Effort", &self.effort),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{field} is required."));
            }
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
        if self.driver != "ACP" {
            return Err("Only the ACP driver is supported.".into());
        }
        if !EFFORTS.contains(&self.effort.as_str()) {
            return Err(format!("Effort must be one of: {}.", EFFORTS.join(", ")));
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
                        harness: "OMP".into(),
                        driver: "ACP".into(),
                        command: "omp.exe".into(),
                        arguments: vec!["acp".into()],
                        permission_mode: PermissionMode::Ask,
                        model: model.into(),
                        effort: "High".into(),
                        effort_parameter_name: EffortParameterName::default(),
                        system_instructions: String::new(),
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
            let disk = load(path);
            if !overwrite && (expected.is_none() || disk.as_ref().ok() != expected) {
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
        if load(&path).as_ref().ok() != Some(&old.definition) {
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

fn load(folder: &Path) -> Result<AgentDefinition, String> {
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
    let has_arguments = yaml
        .as_mapping()
        .is_some_and(|fields| fields.contains_key("arguments"));
    let mut definition: AgentDefinition =
        serde_yaml_ng::from_value(yaml).map_err(|e| file_error(&path, e))?;
    let migrated = !has_arguments && definition.command.chars().any(char::is_whitespace);
    if migrated {
        if Path::new(&definition.command).is_file() {
            // A real executable path may contain spaces; leave it intact.
        } else {
            let parts: Vec<_> = definition.command.split_ascii_whitespace().collect();
            let executable = parts.first().copied().unwrap_or_default();
            if parts.len() < 2
                || ((executable.contains('/')
                    || executable.contains('\\')
                    || executable.contains(':'))
                    && !Path::new(executable).is_file()
                    && !looks_like_executable_path(executable))
                || parts.iter().any(|part| {
                    part.chars()
                        .any(|c| c.is_whitespace() || "\"'`|&;<>^%$".contains(c))
                })
                || parts.iter().any(|part| part.ends_with('\\'))
            {
                return Err(file_error(
                    &path,
                    "Ambiguous legacy command. Edit agent.yml: set command to the executable and arguments to a list of literal strings.",
                ));
            }
            let command = parts[0].to_owned();
            definition.arguments = parts[1..].iter().map(|part| (*part).to_owned()).collect();
            definition.command = command;
        }
    }
    definition.validate().map_err(|e| file_error(&path, e))?;
    if migrated && !definition.arguments.is_empty() {
        replace(&path, &definition)?;
    }
    Ok(definition)
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
            Ok(definition) => entries.push(AgentEntry { id, definition }),
            Err(error) => errors.push(error),
        }
    }
    (entries, errors)
}

fn ensure_only_definition(folder: &Path) -> Result<(), String> {
    for entry in fs::read_dir(folder).map_err(|e| file_error(folder, e))? {
        let entry = entry.map_err(|e| file_error(folder, e))?;
        if entry.file_name() != "agent.yml" {
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
            harness: "OMP".into(),
            driver: "ACP".into(),
            command: "omp.exe".into(),
            arguments: vec!["acp".into()],
            permission_mode: PermissionMode::Ask,
            model: "openai-codex/gpt-6-luna".into(),
            effort: "Max".into(),
            effort_parameter_name: EffortParameterName::default(),
            system_instructions: "Answer plainly.".into(),
        }
    }

    #[test]
    fn effort_parameter_names_persist_and_legacy_agents_default_to_thinking() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        for parameter in EffortParameterName::ALL {
            let mut definition = agent(parameter.as_str());
            definition.effort_parameter_name = parameter;
            let id = catalog.save(None, definition.clone(), None, false).unwrap();
            let yaml = fs::read_to_string(file(&root, &id)).unwrap();
            assert!(yaml.contains(&format!("effort_parameter_name: {}", parameter.as_str())));
            assert_eq!(
                serde_yaml_ng::from_str::<AgentDefinition>(&yaml).unwrap(),
                definition
            );
        }
        let mut legacy = serde_yaml_ng::to_value(agent("Legacy")).unwrap();
        legacy
            .as_mapping_mut()
            .unwrap()
            .remove("effort_parameter_name");
        let restored: AgentDefinition = serde_yaml_ng::from_value(legacy.clone()).unwrap();
        assert_eq!(
            restored.effort_parameter_name,
            EffortParameterName::Thinking
        );
        legacy["effort_parameter_name"] = "unknown".into();
        assert!(serde_yaml_ng::from_value::<AgentDefinition>(legacy).is_err());
    }

    #[test]
    fn migrates_unambiguous_legacy_commands_and_preserves_literal_arguments() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let path = file(&root, "josh");
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "name: Josh\nharness: OMP\ndriver: ACP\ncommand: omp.exe acp\nmodel: openai-codex/gpt-6-luna\neffort: Max\n",
        )
        .unwrap();
        assert!(catalog.refresh());
        let loaded = catalog.entries[0].definition.clone();
        assert_eq!(loaded.command, "omp.exe");
        assert_eq!(loaded.arguments, ["acp"]);
        assert_eq!(loaded.permission_mode, PermissionMode::Ask);
        let migrated = fs::read_to_string(&path).unwrap();
        assert_eq!(
            serde_yaml_ng::from_str::<AgentDefinition>(&migrated).unwrap(),
            loaded
        );
        assert_eq!(open_catalog(&root, false).entries[0].definition, loaded);

        let mut updated = loaded.clone();
        updated.arguments = vec![
            "acp".into(),
            "--arg1".into(),
            "value with spaces".into(),
            String::new(),
        ];
        updated.permission_mode = PermissionMode::AllowEverything;
        catalog
            .save(Some("josh"), updated.clone(), Some(&loaded), false)
            .unwrap();
        assert_eq!(open_catalog(&root, false).entries[0].definition, updated);
    }

    #[test]
    fn rejects_ambiguous_legacy_without_changing_disk_and_respects_explicit_empty_arguments() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let path = file(&root, "josh");
        fs::create_dir(path.parent().unwrap()).unwrap();
        let original = "name: Josh\nharness: OMP\ndriver: ACP\ncommand: '\"C:/Program Files/omp.exe\" acp'\nmodel: openai-codex/gpt-6-luna\neffort: Max\n";
        fs::write(&path, original).unwrap();
        assert!(catalog.refresh());
        assert!(catalog.errors[0].contains("Ambiguous legacy command"));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let explicit = "name: Josh\nharness: OMP\ndriver: ACP\ncommand: omp.exe acp\narguments: []\nmodel: openai-codex/gpt-6-luna\neffort: Max\n";
        fs::write(&path, explicit).unwrap();
        catalog.refresh();
        assert!(catalog.errors[0].contains("Command must be one executable"));
        assert_eq!(fs::read_to_string(&path).unwrap(), explicit);
        let omitted = "name: Josh\nharness: OMP\ndriver: ACP\ncommand: omp.exe\nmodel: openai-codex/gpt-6-luna\neffort: Max\n";
        fs::write(&path, omitted).unwrap();
        assert!(catalog.refresh());
        assert!(catalog.entries[0].definition.arguments.is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), omitted);
    }

    #[test]
    fn existing_executable_path_with_spaces_is_not_split_during_legacy_load() {
        let root = TempDir::new("adeline-agents");
        let executable = root.join("Program Files").join("omp.exe");
        fs::create_dir(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "").unwrap();
        let mut definition = agent("Josh");
        definition.command = executable.to_string_lossy().into_owned();
        definition.arguments.clear();
        let mut document = serde_yaml_ng::to_value(&definition).unwrap();
        assert!(
            document
                .as_mapping_mut()
                .unwrap()
                .remove("arguments")
                .is_some()
        );
        let source = serde_yaml_ng::to_string(&document).unwrap();
        let path = file(&root, "josh");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &source).unwrap();
        let catalog = open_catalog(&root, false);
        assert_eq!(catalog.entries[0].definition, definition);
        assert_eq!(fs::read_to_string(path).unwrap(), source);
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
    fn saves_non_omp_acp_agent() {
        let root = TempDir::new("adeline-agents");
        let mut catalog = open_catalog(&root, false);
        let mut definition = agent("Other harness");
        definition.harness = "Other".into();
        definition.command = "other-agent.exe".into();
        definition.arguments = vec!["--stdio".into()];
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
            ("model", ""),
            ("harness", ""),
            ("driver", "Unknown"),
            ("effort", "Ultra"),
        ] {
            let mut bad = agent("Bad");
            match change.0 {
                "name" => bad.name = change.1.into(),
                "command" => bad.command = change.1.into(),
                "model" => bad.model = change.1.into(),
                "harness" => bad.harness = change.1.into(),
                "driver" => bad.driver = change.1.into(),
                _ => bad.effort = change.1.into(),
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
        fs::write(file(&root, "good"), "name: Good\nharness: OMP\ndriver: ACP\ncommand: omp.exe\narguments: []\nmodel: openai-codex/gpt-6-luna\neffort: Max\n").unwrap();
        fs::create_dir(root.join("agents/bad")).unwrap();
        let mut bad = agent("Bad");
        bad.effort = "Ultra".into();
        fs::write(file(&root, "bad"), serde_yaml_ng::to_string(&bad).unwrap()).unwrap();
        fs::create_dir(root.join("agents/missing")).unwrap();
        assert!(catalog.refresh());
        assert_eq!(catalog.entries[0].definition, good);
        assert_eq!(catalog.errors.len(), 2);
        assert!(catalog.errors.iter().any(|error| error.contains("bad")
            && error.contains("agent.yml")
            && error.contains("Effort")));
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
        edited.command = "demo-only".into();
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
