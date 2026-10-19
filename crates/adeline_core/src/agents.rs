//! User agent definitions, stored under the Adeline configuration directory.
//! Files older than `VERSION` are left alone and not listed.
use crate::conversation::{McpServer, Selections};
use crate::files::{self, checked_id, error as file_error, normalize_name};
use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const VERSION: u32 = 1;

pub const AVATAR: &str = "avatar.svg";

// Read, never written: older files' `permission_mode`, which the agent's modes replace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dropped;

impl<'de> Deserialize<'de> for Dropped {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer).map(|_| Self)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionsMode {
    #[default]
    Append,
    Overwrite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinition {
    pub version: u32,
    pub name: String,
    // A registry ID, `omp`, or `custom`.
    pub harness: String,
    // What a Custom harness reported in its ACP handshake.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    // Custom harnesses only. Others are located when a conversation starts.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effort: String,
    // Empty keeps the agent's default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    #[serde(default, rename = "permission_mode", skip_serializing)]
    pub dropped_permission_mode: Dropped,
    #[serde(default)]
    pub system_instructions: String,
    #[serde(default)]
    pub instructions_mode: InstructionsMode,
    // Added to the global ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServer>,
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
    pub fn validate(&self) -> Result<String> {
        if self.version != VERSION {
            bail!("Agent file version must be {VERSION}.");
        }
        for (field, value) in [("Name", &self.name), ("Harness", &self.harness)] {
            if value.trim().is_empty() {
                bail!("{field} is required.");
            }
        }
        if self.harness == crate::profiles::CUSTOM {
            if self.command.trim().is_empty() {
                bail!("Command is required.");
            }
            if self.command != self.command.trim()
                || (self.command.chars().any(char::is_whitespace)
                    && !Path::new(&self.command).is_file()
                    && !looks_like_executable_path(&self.command))
            {
                bail!(
                    "Command must be one executable name or path; enter each argument separately."
                );
            }
        } else if !self.command.is_empty() || !self.arguments.is_empty() {
            bail!("Only Custom agents store a command and arguments.");
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEntry {
    pub id: String,
    pub definition: AgentDefinition,
}

#[derive(Debug)]
pub struct AgentCatalog {
    pub entries: Vec<AgentEntry>,
    pub errors: Vec<String>,
    root: Result<PathBuf, String>,
    demo: bool,
}

impl AgentCatalog {
    pub fn new(demo: bool) -> Self {
        Self::with_root(
            crate::config::directory()
                .map(|path| path.join("agents"))
                .map_err(|e| format!("{e:#}")),
            demo,
        )
    }

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

    pub fn save(
        &mut self,
        original: Option<&str>,
        definition: AgentDefinition,
        expected: Option<&AgentDefinition>,
        overwrite: bool,
    ) -> Result<String> {
        // `expected` was loaded when editing began. A changed, deleted or invalid
        // file needs `overwrite`.
        let id = definition.validate()?;
        if self.demo {
            if let Some(original) = original
                && !self.entries.iter().any(|entry| entry.id == original)
                && !overwrite
            {
                bail!("Agent {original} changed outside this form. Reload or overwrite it.");
            }
            if original != Some(id.as_str()) && self.entries.iter().any(|entry| entry.id == id) {
                bail!("Agent folder {id} already exists.");
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

        let root = self.root.as_ref().map_err(|e| anyhow!("{e}"))?;
        if let Some(original) = original {
            checked_id(original)?;
        }
        fs::create_dir_all(root).with_context(|| root.display().to_string())?;
        let old = original.map(|id| root.join(id));
        if let Some(path) = old.as_deref() {
            match fs::symlink_metadata(path) {
                Ok(meta) if !meta.file_type().is_dir() => {
                    bail!(
                        "{}: agent folder is not a directory.",
                        path.join("agent.yml").display()
                    );
                }
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    return Err(error)
                        .with_context(|| path.join("agent.yml").display().to_string());
                }
                _ => {}
            }
        }
        if let Some(path) = old.as_deref() {
            let disk = load(path).ok().flatten();
            if !overwrite && (expected.is_none() || disk.as_ref() != expected) {
                bail!(
                    "{}: definition changed outside this form. Reload or overwrite it.",
                    path.join("agent.yml").display()
                );
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
                fs::create_dir(&destination).with_context(|| destination.display().to_string())?;
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
            fs::create_dir(&destination).with_context(|| destination.display().to_string())?;
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
                return Err(error).with_context(|| old.display().to_string());
            }
        }
        self.refresh();
        Ok(id)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        checked_id(id)?;
        if self.demo {
            let index = self
                .entries
                .iter()
                .position(|entry| entry.id == id)
                .with_context(|| format!("Agent {id} no longer exists."))?;
            self.entries.remove(index);
            return Ok(());
        }
        let root = self.root.as_ref().map_err(|e| anyhow!("{e}"))?;
        let old = self
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .with_context(|| format!("Agent {id} no longer exists."))?;
        let path = root.join(id);
        if load(&path).ok().flatten().as_ref() != Some(&old.definition) {
            bail!(
                "{}: definition changed outside this form. Reload before deleting it.",
                path.join("agent.yml").display()
            );
        }
        ensure_only_definition(&path)?;
        fs::remove_dir_all(&path).with_context(|| path.display().to_string())?;
        self.refresh();
        Ok(())
    }
}

pub fn avatar_path(id: &str, harness: &str) -> String {
    let path = format!("agent-avatars/{id}.svg");
    if crate::harness::has_icon(&path) {
        path
    } else {
        crate::harness::icon_path(harness)
    }
}

fn load(folder: &Path) -> Result<Option<AgentDefinition>> {
    let path = folder.join("agent.yml");
    let context = || path.display().to_string();
    if !fs::symlink_metadata(folder)
        .with_context(context)?
        .file_type()
        .is_dir()
    {
        bail!("{}: agent folder is not a directory.", path.display());
    }
    let text = fs::read_to_string(&path).with_context(context)?;
    let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).with_context(context)?;
    if yaml
        .get("version")
        .and_then(serde_yaml_ng::Value::as_u64)
        .is_none_or(|version| version < u64::from(VERSION))
    {
        return Ok(None);
    }
    let definition: AgentDefinition = serde_yaml_ng::from_value(yaml).with_context(context)?;
    definition.validate().with_context(context)?;
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
            .with_context(|| path.join("agent.yml").display().to_string())
            .and_then(|()| load(&path));
        match result {
            Ok(Some(definition)) => entries.push(AgentEntry { id, definition }),
            Ok(None) => {}
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    (entries, errors)
}

fn ensure_only_definition(folder: &Path) -> Result<()> {
    let context = || folder.display().to_string();
    for entry in fs::read_dir(folder).with_context(context)? {
        let entry = entry.with_context(context)?;
        if entry.file_name() != "agent.yml" && entry.file_name() != AVATAR {
            bail!(
                "{}: cannot rename or delete a folder containing other files.",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn ensure_no_collision(root: &Path, id: &str) -> Result<()> {
    let context = || root.display().to_string();
    for entry in fs::read_dir(root).with_context(context)? {
        let entry = entry.with_context(context)?;
        if entry.file_name().to_string_lossy().to_lowercase() == id {
            bail!("{}: agent folder already exists.", entry.path().display());
        }
    }
    Ok(())
}

fn yaml(definition: &AgentDefinition, path: &Path) -> Result<String> {
    serde_yaml_ng::to_string(definition).with_context(|| path.display().to_string())
}

fn write_new(path: &Path, definition: &AgentDefinition) -> Result<()> {
    files::write_new(path, yaml(definition, path)?.as_bytes())
}

fn replace(path: &Path, definition: &AgentDefinition) -> Result<()> {
    files::replace(path, yaml(definition, path)?.as_bytes())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    pub name: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    pub command: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<(String, String)>,
    #[serde(default)]
    pub selections: Selections,
    #[serde(default)]
    pub system_instructions: String,
    #[serde(default)]
    pub instructions_mode: InstructionsMode,
    pub directory: PathBuf,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServer>,
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
            harness: crate::profiles::CUSTOM.into(),
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
        catalog.save(None, with_command, None, false).unwrap_err();
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
        catalog.save(None, agent("Josh"), None, false).unwrap_err();
        assert_eq!(fs::read_to_string(file(&root, "josh")).unwrap(), old);
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
        catalog
            .save(None, agent("Josh Smith"), None, true)
            .unwrap_err();
        assert_eq!(fs::read(file(&root, "josh-smith")).unwrap(), before);
        for invalid in [" -- !!! ", "CON", "com1", "COM¹", "LPT9"] {
            assert!(
                catalog.save(None, agent(invalid), None, false).is_err(),
                "{invalid}"
            );
        }
        catalog
            .save(None, agent(&"a".repeat(256)), None, false)
            .unwrap_err();
        fs::create_dir(root.join("agents").join("JOSH")).unwrap();
        catalog.save(None, agent("Josh"), None, true).unwrap_err();
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
        // A path with spaces is one command, and it may not be installed yet.
        good.command = root
            .join("Program Files")
            .join("omp.exe")
            .to_string_lossy()
            .into_owned();
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
        let error = catalog
            .save(Some("josh"), mine.clone(), Some(&original), false)
            .unwrap_err();
        assert!(format!("{error:#}").contains("agent.yml"));
        assert_eq!(
            fs::read_to_string(file(&root, "josh")).unwrap(),
            serde_yaml_ng::to_string(&theirs).unwrap()
        );
        fs::write(file(&root, "josh"), "name: Broken\n").unwrap();
        catalog
            .save(Some("josh"), mine.clone(), Some(&original), false)
            .unwrap_err();
        fs::remove_file(file(&root, "josh")).unwrap();
        catalog
            .save(Some("josh"), mine.clone(), Some(&original), false)
            .unwrap_err();
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
        assert_eq!(avatar("josh"), adeline_assets::embedded("omp.svg").unwrap());
        let mut renamed = josh.clone();
        renamed.name = "Josh Smith".into();
        catalog
            .save(Some("josh"), renamed, Some(&josh), false)
            .unwrap();
        assert_eq!(
            avatar("josh-smith"),
            adeline_assets::embedded("omp.svg").unwrap()
        );
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
        catalog
            .save(Some("josh"), alice.clone(), Some(&josh), true)
            .unwrap_err();
        assert_eq!(
            fs::read_to_string(file(&root, "alice")).unwrap(),
            serde_yaml_ng::to_string(&alice).unwrap()
        );
        let mut renamed = josh.clone();
        renamed.name = "Josh Smith".into();
        let extra = file(&root, "josh").parent().unwrap().join("notes.txt");
        fs::write(&extra, "keep this").unwrap();
        catalog
            .save(Some("josh"), renamed.clone(), Some(&josh), false)
            .unwrap_err();
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
        let extra = file(&root, "josh-smith")
            .parent()
            .unwrap()
            .join("notes.txt");
        fs::write(&extra, "keep this").unwrap();
        assert!(catalog.delete("josh-smith").is_err());
        assert_eq!(fs::read_to_string(&extra).unwrap(), "keep this");
        fs::remove_file(extra).unwrap();
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
