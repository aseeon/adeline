//! Durable project definitions, conversation snapshots and ordered transcript events.
use crate::{
    agents::{self, AgentDefinition, PermissionMode},
    data,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ExecutionConfig {
    pub name: String,
    #[serde(default = "default_harness")]
    pub harness: String,
    pub command: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub model: String,
    pub effort: String,
    #[serde(default)]
    pub effort_parameter_name: agents::EffortParameterName,
    pub system_instructions: String,
    pub directory: PathBuf,
}

fn default_harness() -> String {
    "OMP".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSettings {
    pub agent_id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub execution: ExecutionConfig,
    pub permission_mode: PermissionMode,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub previous_session_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TranscriptEvent {
    pub timestamp: u64,
    pub kind: String,
    pub data: Value,
}

impl TranscriptEvent {
    pub fn new(kind: &str, data: Value) -> Self {
        Self {
            timestamp: now_millis(),
            kind: kind.to_owned(),
            data,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectDefinition {
    name: String,
    directory: PathBuf,
}

#[derive(Clone, Debug)]
pub struct StoredConversation {
    pub id: String,
    pub settings: ConversationSettings,
    pub events: Vec<TranscriptEvent>,
    pub unsaved_events: Vec<TranscriptEvent>,
    pub storage_error: Option<String>,
    persisted_len: u64,
    needs_rollback: bool,
    pending_settings: Option<ConversationSettings>,
    load_error: bool,
    interrupted: bool,
}

impl StoredConversation {
    pub fn to_thread(&self) -> data::Thread {
        let mut thread = data::Thread {
            id: self.id.clone(),
            title: self.settings.title.clone(),
            provider: self.settings.execution.name.clone(),
            status: self.settings.status.clone(),
            created_at: self.settings.created_at.clone(),
            ..Default::default()
        };
        let mut current_assistant = None;
        for event in self.events.iter().chain(&self.unsaved_events) {
            match event.kind.as_str() {
                "message" => {
                    if let Ok(mut message) =
                        serde_json::from_value::<data::Message>(event.data.clone())
                    {
                        if message.created_at.is_empty() {
                            message.created_at = event.timestamp.to_string();
                        }
                        current_assistant =
                            (message.role == "assistant").then_some(thread.messages.len());
                        thread.messages.push(message);
                    }
                }
                "assistant_chunk" => {
                    if let Some(text) = event.data.get("text").and_then(Value::as_str) {
                        if let Some(index) = current_assistant {
                            if let Some(message) = thread.messages.get_mut(index) {
                                message.text.push_str(text);
                            }
                        } else {
                            current_assistant = Some(thread.messages.len());
                            thread.messages.push(data::Message {
                                role: "assistant".into(),
                                text: text.to_owned(),
                                created_at: event.timestamp.to_string(),
                                ..Default::default()
                            });
                        }
                    }
                }
                "message_update" => {
                    if let (Some(index), Some(text)) = (
                        event
                            .data
                            .get("index")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok()),
                        event.data.get("text").and_then(Value::as_str),
                    ) && let Some(message) = thread.messages.get_mut(index)
                    {
                        text.clone_into(&mut message.text);
                        current_assistant = Some(index);
                    }
                }
                "message_read" => {
                    if let Some(through) = event
                        .data
                        .get("through")
                        .and_then(Value::as_u64)
                        .and_then(|index| usize::try_from(index).ok())
                    {
                        for message in thread.messages.iter_mut().take(through.saturating_add(1)) {
                            message.read = true;
                        }
                    }
                }
                "tool" => {
                    let id = event
                        .data
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let key = format!("tool:{id}");
                    let title = event
                        .data
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Tool");
                    let status = event
                        .data
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let detail = event
                        .data
                        .get("detail")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let activity =
                        if let Some(index) = thread.activity.iter().position(|a| a.kind == key) {
                            &mut thread.activity[index]
                        } else {
                            thread.activity.push(data::Activity {
                                kind: key,
                                ..Default::default()
                            });
                            thread.activity.last_mut().expect("just added")
                        };
                    activity.title = format!("{title} ({status})");
                    detail.clone_into(&mut activity.detail);
                    activity.running = matches!(status, "pending" | "in_progress");
                }
                "error" => {
                    let title = event
                        .data
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Conversation error");
                    thread.activity.push(data::Activity {
                        kind: "error".into(),
                        title: title.to_owned(),
                        detail: String::new(),
                        running: false,
                    });
                }
                "lifecycle"
                    if event.data.get("event").and_then(Value::as_str)
                        == Some("replacement_session_approved")
                        || (event.data.get("event").and_then(Value::as_str)
                            == Some("turn_started")
                            && event.data.get("retry").and_then(Value::as_bool) == Some(true)) =>
                {
                    current_assistant = None;
                }
                _ => {}
            }
        }
        if self.interrupted {
            thread.activity.push(data::Activity {
                kind: "error".into(),
                title: "Prompt interrupted by application exit".into(),
                detail: "Send a new message to continue this conversation.".into(),
                running: false,
            });
        }
        if let Some(error) = &self.storage_error {
            thread.activity.push(data::Activity {
                kind: "error".into(),
                title: "Conversation data could not be saved".into(),
                detail: error.clone(),
                running: false,
            });
        }
        thread.prepare_search();
        thread
    }
}

#[derive(Clone, Debug)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub directory: PathBuf,
    pub conversations: Vec<StoredConversation>,
}

impl ProjectRecord {
    pub fn to_workspace(&self) -> data::Workspace {
        let mut workspace = data::Workspace {
            config: data::Config {
                id: self.id.clone(),
                name: self.name.clone(),
                provider: String::new(),
                directory: self.directory.clone(),
            },
            threads: self
                .conversations
                .iter()
                .map(StoredConversation::to_thread)
                .collect(),
            ..Default::default()
        };
        workspace.rebuild_counts();
        workspace
    }
}

pub struct ProjectStore {
    pub projects: Vec<ProjectRecord>,
    pub errors: Vec<String>,
    root: Result<PathBuf, String>,
}

impl ProjectStore {
    pub fn new() -> Self {
        match crate::config::directory() {
            Ok(root) => Self::with_root(root.join("projects")),
            Err(error) => Self {
                projects: Vec::new(),
                errors: vec![error.clone()],
                root: Err(error),
            },
        }
    }

    pub fn empty() -> Self {
        Self {
            projects: Vec::new(),
            errors: Vec::new(),
            root: Err("Demo projects are not saved.".into()),
        }
    }

    pub fn with_root(root: PathBuf) -> Self {
        let mut store = Self {
            projects: Vec::new(),
            errors: Vec::new(),
            root: Ok(root),
        };
        store.reload();
        store
    }

    pub fn to_workspaces(&self) -> Vec<data::Workspace> {
        self.projects
            .iter()
            .map(ProjectRecord::to_workspace)
            .collect()
    }

    pub fn conversation(&self, id: &str) -> Option<&StoredConversation> {
        self.projects
            .iter()
            .flat_map(|project| &project.conversations)
            .find(|conversation| conversation.id == id)
    }

    fn reload(&mut self) {
        self.projects.clear();
        self.errors.clear();
        let Ok(root) = &self.root else {
            self.errors.push(self.root.as_ref().unwrap_err().clone());
            return;
        };
        match fs::symlink_metadata(root) {
            Ok(metadata) if !metadata.file_type().is_dir() || is_link(&metadata) => {
                self.errors.push(format!(
                    "{}: expected a real projects folder.",
                    root.display()
                ));
                return;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) => {
                self.errors.push(file_error(root, error));
                return;
            }
        }
        let folders = match fs::read_dir(root) {
            Ok(folders) => folders,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) => {
                self.errors.push(file_error(root, error));
                return;
            }
        };
        for entry in folders {
            let result = entry.map_err(|e| file_error(root, e)).and_then(|entry| {
                let folder = entry.path();
                let kind = entry.file_type().map_err(|e| file_error(&folder, e))?;
                if kind.is_symlink() {
                    return Err(format!(
                        "{}: linked project folders are not loaded.",
                        folder.display()
                    ));
                }
                if !kind.is_dir() {
                    return Ok(None);
                }
                safe_directory(&folder)?;
                let id = entry.file_name().to_string_lossy().into_owned();
                checked_id(&id)?;
                ensure_regular_file(&folder.join("project.yml"))?;
                let text = fs::read_to_string(folder.join("project.yml"))
                    .map_err(|e| file_error(&folder.join("project.yml"), e))?;
                let definition: ProjectDefinition = serde_yaml_ng::from_str(&text)
                    .map_err(|e| file_error(&folder.join("project.yml"), e))?;
                if agents::normalize_name(&definition.name)? != id
                    || !definition.directory.is_absolute()
                {
                    return Err(format!(
                        "{}: project name or directory is invalid.",
                        folder.display()
                    ));
                }
                if let Err(error) = outside_project_storage(&definition.directory, root) {
                    self.errors
                        .push(file_error(&folder.join("project.yml"), error));
                }
                let mut project = ProjectRecord {
                    id,
                    name: definition.name,
                    directory: definition.directory,
                    conversations: Vec::new(),
                };
                let conversations = folder.join("conversations");
                safe_directory(&conversations)?;
                match fs::read_dir(&conversations) {
                    Ok(entries) => {
                        for entry in entries {
                            match entry
                                .map_err(|e| file_error(&conversations, e))
                                .and_then(|entry| load_conversation(&entry.path()))
                            {
                                Ok(Some(conversation)) => {
                                    if let Some(error) = &conversation.storage_error {
                                        self.errors.push(error.clone());
                                    }
                                    project.conversations.push(conversation);
                                }
                                Ok(None) => {}
                                Err(error) => self.errors.push(error),
                            }
                        }
                    }
                    Err(error) => self.errors.push(file_error(&conversations, error)),
                }
                project.conversations.sort_by(|a, b| {
                    b.settings
                        .created_at
                        .cmp(&a.settings.created_at)
                        .then(b.id.cmp(&a.id))
                });
                Ok(Some(project))
            });
            match result {
                Ok(Some(project)) => self.projects.push(project),
                Ok(None) => {}
                Err(error) => self.errors.push(error),
            }
        }
        self.projects
            .sort_by_cached_key(|project| project.name.to_lowercase());
    }

    /// Creates or edits a project. The caller must stop agents before deleting, not here.
    pub fn save_project(
        &mut self,
        original: Option<&str>,
        name: &str,
        directory: &Path,
    ) -> Result<String, String> {
        let id = agents::normalize_name(name)
            .map_err(|_| "Project name does not produce a valid folder name.".to_owned())?;
        valid_working_directory(directory)?;
        let root = self.root.as_ref().map_err(Clone::clone)?;
        if let Some(old) = original {
            checked_id(old)?;
            let project = self
                .projects
                .iter()
                .find(|p| p.id == old)
                .ok_or_else(|| format!("Project {old} no longer exists."))?;
            if project.directory != directory
                && project
                    .conversations
                    .iter()
                    .any(|c| !matches!(c.settings.status.as_str(), "completed" | "archived"))
            {
                return Err(
                    "Complete or archive every conversation before changing the project directory."
                        .into(),
                );
            }
        }
        fs::create_dir_all(root).map_err(|e| file_error(root, e))?;
        safe_directory(root)?;
        outside_project_storage(directory, root)?;
        let destination = root.join(&id);
        if original != Some(id.as_str()) {
            for entry in fs::read_dir(root).map_err(|e| file_error(root, e))? {
                let entry = entry.map_err(|e| file_error(root, e))?;
                if entry.file_name().to_string_lossy().to_lowercase() == id {
                    return Err(format!("Project folder {id} already exists."));
                }
            }
        }
        let definition = ProjectDefinition {
            name: name.to_owned(),
            directory: directory.to_owned(),
        };
        let text = serde_yaml_ng::to_string(&definition).map_err(|e| e.to_string())?;
        if let Some(old) = original {
            let old_path = root.join(old);
            safe_directory(&old_path)?;
            if id != old {
                fs::rename(&old_path, &destination).map_err(|e| file_error(&old_path, e))?;
            }
            let write = replace_file(&destination.join("project.yml"), text.as_bytes());
            if let Err(error) = write {
                if id != old {
                    let _ = fs::rename(&destination, &old_path);
                }
                return Err(error);
            }
            let project = self
                .projects
                .iter_mut()
                .find(|p| p.id == old)
                .expect("checked above");
            project.id.clone_from(&id);
            name.clone_into(&mut project.name);
            directory.clone_into(&mut project.directory);
        } else {
            fs::create_dir(&destination).map_err(|e| file_error(&destination, e))?;
            let result = fs::create_dir(destination.join("conversations"))
                .map_err(|e| file_error(&destination, e))
                .and_then(|()| write_new_file(&destination.join("project.yml"), text.as_bytes()));
            if let Err(error) = result {
                let _ = fs::remove_dir(destination.join("conversations"));
                let _ = fs::remove_dir(&destination);
                return Err(error);
            }
            self.projects.push(ProjectRecord {
                id: id.clone(),
                name: name.to_owned(),
                directory: directory.to_owned(),
                conversations: Vec::new(),
            });
        }
        Ok(id)
    }

    /// Call only after every live agent in this project has stopped successfully.
    pub fn delete_project(&mut self, id: &str) -> Result<(), String> {
        checked_id(id)?;
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let project = self
            .projects
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("Project {id} no longer exists."))?;
        outside_project_storage(&project.directory, root)?;
        safe_directory(root)?;
        let folder = root.join(id);
        safe_directory(&folder)?;
        let definition_path = folder.join("project.yml");
        ensure_regular_file(&definition_path)?;
        let disk: ProjectDefinition = serde_yaml_ng::from_str(
            &fs::read_to_string(&definition_path).map_err(|e| file_error(&definition_path, e))?,
        )
        .map_err(|e| file_error(&definition_path, e))?;
        outside_project_storage(&disk.directory, root)?;
        let conversations = folder.join("conversations");
        safe_directory(&conversations)?;
        let mut folders = Vec::new();
        for entry in fs::read_dir(&conversations).map_err(|e| file_error(&conversations, e))? {
            let entry = entry.map_err(|e| file_error(&conversations, e))?;
            checked_id(&entry.file_name().to_string_lossy())?;
            safe_directory(&entry.path())?;
            check_contents(&entry.path(), &["conversation.yml", "transcript.jsonl"])?;
            folders.push(entry.path());
        }
        check_contents(&folder, &["project.yml", "conversations"])?;
        for folder in folders {
            fs::remove_file(folder.join("conversation.yml")).map_err(|e| file_error(&folder, e))?;
            fs::remove_file(folder.join("transcript.jsonl")).map_err(|e| file_error(&folder, e))?;
            fs::remove_dir(&folder).map_err(|e| file_error(&folder, e))?;
        }
        fs::remove_dir(&conversations).map_err(|e| file_error(&conversations, e))?;
        fs::remove_file(folder.join("project.yml")).map_err(|e| file_error(&folder, e))?;
        fs::remove_dir(&folder).map_err(|e| file_error(&folder, e))?;
        self.projects.retain(|p| p.id != id);
        Ok(())
    }

    pub fn create_conversation(
        &mut self,
        project_id: &str,
        agent: &AgentDefinition,
        title: &str,
    ) -> Result<String, String> {
        checked_id(project_id)?;
        let agent_id = agent.validate()?;
        let project = self
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project {project_id} no longer exists."))?;
        valid_working_directory(&project.directory)?;
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let base = root.join(project_id).join("conversations");
        safe_directory(root)?;
        safe_directory(&root.join(project_id))?;
        safe_directory(&base)?;
        outside_project_storage(&project.directory, root)?;
        let settings = ConversationSettings {
            agent_id,
            title: title.to_owned(),
            status: "active".into(),
            created_at: now_millis().to_string(),
            execution: ExecutionConfig {
                name: agent.name.clone(),
                harness: agent.harness.clone(),
                command: agent.command.clone(),
                arguments: agent.arguments.clone(),
                model: agent.model.clone(),
                effort: agent.effort.clone(),
                effort_parameter_name: agent.effort_parameter_name,
                system_instructions: agent.system_instructions.clone(),
                directory: project.directory.clone(),
            },
            permission_mode: agent.permission_mode,
            session_id: None,
            previous_session_ids: Vec::new(),
        };
        let id = loop {
            let id = format!(
                "{:x}-{:x}-{:x}",
                now_nanos(),
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            );
            match fs::create_dir(base.join(&id)) {
                Ok(()) => break id,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(file_error(&base, e)),
            }
        };
        let folder = base.join(&id);
        let text = serde_yaml_ng::to_string(&settings).map_err(|e| e.to_string())?;
        let result = write_new_file(&folder.join("conversation.yml"), text.as_bytes())
            .and_then(|()| write_new_file(&folder.join("transcript.jsonl"), b""));
        if let Err(error) = result {
            let _ = fs::remove_file(folder.join("conversation.yml"));
            let _ = fs::remove_dir(&folder);
            return Err(error);
        }
        project.conversations.push(StoredConversation {
            id: id.clone(),
            settings,
            events: Vec::new(),
            unsaved_events: Vec::new(),
            storage_error: None,
            persisted_len: 0,
            needs_rollback: false,
            pending_settings: None,
            load_error: false,
            interrupted: false,
        });
        Ok(id)
    }

    pub fn update_conversation(
        &mut self,
        project_id: &str,
        id: &str,
        settings: ConversationSettings,
    ) -> Result<(), String> {
        checked_id(project_id)?;
        checked_id(id)?;
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let project = self
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project {project_id} no longer exists."))?;
        let conversation = project
            .conversations
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("Conversation {id} no longer exists."))?;
        if conversation.settings.agent_id != settings.agent_id
            || conversation.settings.created_at != settings.created_at
            || conversation.settings.execution != settings.execution
        {
            return Err(
                "An existing conversation's agent and execution settings cannot change.".into(),
            );
        }
        conversation.pending_settings = Some(settings.clone());
        if settings.status != "blocked" {
            conversation.interrupted = false;
        }
        conversation.settings = settings;
        if let Some(error) = &conversation.storage_error {
            return Err(error.clone());
        }
        let path = root
            .join(project_id)
            .join("conversations")
            .join(id)
            .join("conversation.yml");
        flush_settings(conversation, &path)
    }

    pub fn record_raw(
        &mut self,
        conversation_id: &str,
        direction: &str,
        message: &Value,
    ) -> Result<(), String> {
        self.record_event(
            conversation_id,
            &TranscriptEvent::new(
                "raw",
                serde_json::json!({
                    "direction": direction, "message": message,
                }),
            ),
        )
    }

    pub fn record_event(
        &mut self,
        conversation_id: &str,
        event: &TranscriptEvent,
    ) -> Result<(), String> {
        let (project_id, conversation) = self
            .projects
            .iter_mut()
            .find_map(|p| {
                p.conversations
                    .iter_mut()
                    .find(|c| c.id == conversation_id)
                    .map(|c| (&p.id, c))
            })
            .ok_or_else(|| format!("Conversation {conversation_id} no longer exists."))?;
        conversation.unsaved_events.push(event.clone());
        if let Some(error) = &conversation.storage_error {
            return Err(error.clone());
        }
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let path = root
            .join(project_id)
            .join("conversations")
            .join(conversation_id)
            .join("transcript.jsonl");
        flush_events(conversation, &path)
    }

    pub fn retry_unsaved(&mut self, conversation_id: &str) -> Result<(), String> {
        let (project_id, conversation) = self
            .projects
            .iter_mut()
            .find_map(|p| {
                p.conversations
                    .iter_mut()
                    .find(|c| c.id == conversation_id)
                    .map(|c| (&p.id, c))
            })
            .ok_or_else(|| format!("Conversation {conversation_id} no longer exists."))?;
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let folder = root
            .join(project_id)
            .join("conversations")
            .join(conversation_id);
        let had_pending_settings = conversation.pending_settings.is_some();
        if had_pending_settings {
            flush_settings(conversation, &folder.join("conversation.yml"))?;
        }
        if conversation.load_error {
            return Err(conversation.storage_error.clone().unwrap_or_else(|| {
                "Transcript must be repaired and reloaded before continuing.".into()
            }));
        }
        if conversation.unsaved_events.is_empty() {
            if had_pending_settings && !conversation.load_error {
                conversation.storage_error = None;
            }
            return conversation.storage_error.clone().map_or(Ok(()), Err);
        }
        flush_events(conversation, &folder.join("transcript.jsonl"))
    }
}

fn flush_settings(conversation: &mut StoredConversation, path: &Path) -> Result<(), String> {
    let result = (|| {
        safe_conversation_path(path)?;
        if !fs::symlink_metadata(path)
            .map_err(|e| file_error(path, e))?
            .file_type()
            .is_file()
        {
            return Err(format!(
                "{}: settings are not a regular file.",
                path.display()
            ));
        }
        let settings = conversation
            .pending_settings
            .as_ref()
            .expect("pending settings");
        let text = serde_yaml_ng::to_string(settings).map_err(|e| file_error(path, e))?;
        replace_file(path, text.as_bytes())
    })();
    match result {
        Ok(()) => {
            conversation.pending_settings = None;
            Ok(())
        }
        Err(error) => {
            conversation.storage_error = Some(error.clone());
            Err(error)
        }
    }
}

fn flush_events(conversation: &mut StoredConversation, path: &Path) -> Result<(), String> {
    let result = (|| {
        safe_conversation_path(path)?;
        if !fs::symlink_metadata(path)
            .map_err(|e| file_error(path, e))?
            .file_type()
            .is_file()
        {
            return Err(format!(
                "{}: transcript is not a regular file.",
                path.display()
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| file_error(path, e))?;
        let len = file.metadata().map_err(|e| file_error(path, e))?.len();
        if len < conversation.persisted_len {
            return Err(format!(
                "{}: saved transcript was truncated; reload before writing.",
                path.display()
            ));
        }
        if conversation.needs_rollback {
            file.set_len(conversation.persisted_len)
                .map_err(|e| file_error(path, e))?;
            file.sync_all().map_err(|e| file_error(path, e))?;
            conversation.needs_rollback = false;
        } else if len != conversation.persisted_len {
            return Err(format!(
                "{}: transcript changed on disk; reload before writing.",
                path.display()
            ));
        }
        file.seek(SeekFrom::Start(conversation.persisted_len))
            .map_err(|e| file_error(path, e))?;
        let write_result = (|| {
            for event in &conversation.unsaved_events {
                serde_json::to_writer(&mut file, event).map_err(|e| file_error(path, e))?;
                file.write_all(b"\n").map_err(|e| file_error(path, e))?;
            }
            file.sync_all().map_err(|e| file_error(path, e))?;
            file.stream_position().map_err(|e| file_error(path, e))
        })();
        match write_result {
            Ok(len) => conversation.persisted_len = len,
            Err(error) => {
                conversation.needs_rollback = true;
                return Err(error);
            }
        }
        conversation.events.append(&mut conversation.unsaved_events);
        conversation.storage_error = None;
        Ok(())
    })();
    if let Err(error) = &result {
        conversation.storage_error = Some(error.clone());
    }
    result
}

fn load_conversation(folder: &Path) -> Result<Option<StoredConversation>, String> {
    let metadata = fs::symlink_metadata(folder).map_err(|e| file_error(folder, e))?;
    if is_link(&metadata) {
        return Err(format!(
            "{}: linked conversation folders are not loaded.",
            folder.display()
        ));
    }
    if !metadata.file_type().is_dir() {
        return Ok(None);
    }
    let id = folder
        .file_name()
        .ok_or_else(|| format!("{}: missing folder name.", folder.display()))?
        .to_string_lossy()
        .into_owned();
    checked_id(&id)?;
    let settings_path = folder.join("conversation.yml");
    ensure_regular_file(&settings_path)?;
    let mut settings: ConversationSettings = serde_yaml_ng::from_str(
        &fs::read_to_string(&settings_path).map_err(|e| file_error(&settings_path, e))?,
    )
    .map_err(|e| file_error(&settings_path, e))?;
    let interrupted = settings.status == "working";
    if interrupted {
        settings.status = "blocked".into();
    }
    if !settings.execution.directory.is_absolute() {
        return Err(format!(
            "{}: saved working directory is not absolute.",
            settings_path.display()
        ));
    }
    let transcript_path = folder.join("transcript.jsonl");
    ensure_regular_file(&transcript_path)?;
    let file = File::open(&transcript_path).map_err(|e| file_error(&transcript_path, e))?;
    let persisted_len = file
        .metadata()
        .map_err(|e| file_error(&transcript_path, e))?
        .len();
    let mut events = Vec::new();
    let mut load_error = None;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        match line
            .map_err(|e| file_error(&transcript_path, e))
            .and_then(|line| {
                serde_json::from_str(&line)
                    .map_err(|e| format!("{}:{}: {e}", transcript_path.display(), index + 1))
            }) {
            Ok(event) => events.push(event),
            Err(error) => {
                load_error = Some(error);
                break;
            }
        }
    }
    Ok(Some(StoredConversation {
        id,
        settings,
        events,
        unsaved_events: Vec::new(),
        load_error: load_error.is_some(),
        storage_error: load_error,
        persisted_len,
        needs_rollback: false,
        pending_settings: None,
        interrupted,
    }))
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn now_millis() -> u64 {
    u64::try_from(now_nanos() / 1_000_000).unwrap_or(u64::MAX)
}
fn file_error(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}
fn checked_id(id: &str) -> Result<(), String> {
    if agents::normalize_name(id).as_deref() == Ok(id) {
        Ok(())
    } else {
        Err(format!("Invalid storage folder: {id}"))
    }
}
fn outside_project_storage(directory: &Path, root: &Path) -> Result<(), String> {
    let root_path = fs::canonicalize(root).map_err(|e| file_error(root, e))?;
    if directory.starts_with(root)
        || (directory.exists()
            && fs::canonicalize(directory)
                .map_err(|e| file_error(directory, e))?
                .starts_with(root_path))
    {
        return Err(format!(
            "{}: working directory cannot be inside Adeline's projects storage folder.",
            directory.display()
        ));
    }
    Ok(())
}

fn valid_working_directory(directory: &Path) -> Result<(), String> {
    if !directory.is_absolute() {
        return Err("Choose an absolute working directory.".into());
    }
    if !directory.is_dir() {
        return Err(format!(
            "{}: working directory does not exist or is not a directory.",
            directory.display()
        ));
    }
    Ok(())
}
#[cfg(windows)]
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // Junctions are reparse points too, but FileType::is_symlink can miss them.
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn safe_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| file_error(path, e))?;
    if metadata.file_type().is_dir() && !is_link(&metadata) {
        Ok(())
    } else {
        Err(format!(
            "{}: expected a real directory, not a link.",
            path.display()
        ))
    }
}
fn safe_conversation_path(file: &Path) -> Result<(), String> {
    for ancestor in file.ancestors().skip(1).take(4) {
        safe_directory(ancestor)?;
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| file_error(path, e))?;
    if metadata.file_type().is_file() && !is_link(&metadata) {
        Ok(())
    } else {
        Err(format!(
            "{}: expected a regular file, not a link.",
            path.display()
        ))
    }
}

fn check_contents(folder: &Path, allowed: &[&str]) -> Result<(), String> {
    for entry in fs::read_dir(folder).map_err(|e| file_error(folder, e))? {
        let entry = entry.map_err(|e| file_error(folder, e))?;
        let path = entry.path();
        let name = entry.file_name();
        let metadata = fs::symlink_metadata(&path).map_err(|e| file_error(&path, e))?;
        if !allowed
            .iter()
            .any(|allowed| name == std::ffi::OsStr::new(*allowed))
            || is_link(&metadata)
        {
            return Err(format!(
                "{}: unexpected file or link prevents safe deletion.",
                path.display()
            ));
        }
    }
    for name in allowed {
        if !folder.join(name).exists() {
            return Err(format!(
                "{}: missing saved project data.",
                folder.join(name).display()
            ));
        }
    }
    Ok(())
}
fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| file_error(path, e))?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(file_error(path, error));
    }
    Ok(())
}
fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let suffix = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("tmp-{}-{suffix}", std::process::id()));
    write_new_file(&temp, bytes)?;
    // Windows rename does not replace an existing file. Keep the original until
    // the new file is fully synced, and restore it if installing the new one fails.
    let backup = path.with_extension(format!("bak-{}-{suffix}", std::process::id()));
    if let Err(error) = fs::rename(path, &backup) {
        let _ = fs::remove_file(&temp);
        return Err(file_error(path, error));
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temp);
        return Err(file_error(path, error));
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
