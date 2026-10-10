//! Durable project definitions, conversation snapshots and ordered transcript events.
use crate::files::{self, checked_id, error as file_error};
use crate::{
    agents::{self, AgentDefinition, InstructionsMode},
    conversation::{AgentCommand, Features, McpServer, Selections, SessionOption, TodoStep},
    data,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The conversation format this version writes. Conversations saved in an
/// earlier format stay on disk untouched but are not loaded (scope R37).
pub const VERSION: u32 = 2;

/// How long streamed events may wait in memory before they are written.
pub const FLUSH_INTERVAL_MS: u64 = 250;

/// What a conversation started with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ExecutionConfig {
    pub name: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    /// The resolved executable.
    pub command: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Variables the registry sets for the agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<(String, String)>,
    /// Model, effort, mode and other options; the only part that changes
    /// after creation, and only for this conversation (scope R17).
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

impl ExecutionConfig {
    fn fixed(&self) -> Self {
        Self {
            selections: Selections::default(),
            ..self.clone()
        }
    }
}

/// An executable and how to run it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSettings {
    pub version: u32,
    pub agent_id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub previous_session_ids: Vec<String>,
    /// What the agent last offered, so the menus and the `/` list work while
    /// it is stopped and come back after a restart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SessionOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<AgentCommand>,
    #[serde(default)]
    pub features: Features,
    /// The agent's TODO list, kept until the agent sends an empty one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todo: Vec<TodoStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<ForkOrigin>,
}

/// The conversation and reply a fork was made from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForkOrigin {
    pub conversation_id: String,
    pub title: String,
    /// The fork point: the index of the source's reply the fork ends with.
    pub message: usize,
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
    /// When the project was last opened, in seconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opened_at: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct StoredConversation {
    pub id: String,
    pub settings: ConversationSettings,
    pub events: Vec<TranscriptEvent>,
    pub unsaved_events: Vec<TranscriptEvent>,
    pub storage_error: Option<String>,
    /// When the oldest unsaved streamed event was queued, in milliseconds.
    unsaved_since: Option<u64>,
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
            fork: self.settings.forked_from.as_ref().map(|origin| data::Fork {
                id: origin.conversation_id.clone(),
                title: origin.title.clone(),
                text_copy: false,
            }),
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
                        if message.role == "user" {
                            thread.timing.prompt(event.timestamp);
                        }
                        current_assistant =
                            (message.role == "assistant").then_some(thread.messages.len());
                        thread.messages.push(message);
                    }
                }
                kind @ ("assistant_chunk" | "thought_chunk") => {
                    if let Some(text) = event.data.get("text").and_then(Value::as_str) {
                        thread.stream(
                            &mut current_assistant,
                            event.data.get("message").and_then(Value::as_str),
                            text,
                            kind == "thought_chunk",
                            event.timestamp,
                        );
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
                    if let Ok(call) = serde_json::from_value::<data::ToolReport>(event.data.clone())
                    {
                        thread.apply_tool(&call, event.timestamp);
                    }
                }
                "usage" => {
                    if let (Some(used), Some(size)) = (
                        event.data.get("used").and_then(Value::as_u64),
                        event.data.get("size").and_then(Value::as_u64),
                    ) {
                        thread.context = Some((used, size));
                    }
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
                        turn: thread.messages.iter().rposition(|m| m.role == "user"),
                        ..Default::default()
                    });
                }
                "note" => {
                    if let Some(message) = event.data.get("message").and_then(Value::as_str) {
                        thread.activity.push(data::Activity {
                            kind: "note".into(),
                            title: message.to_owned(),
                            ..Default::default()
                        });
                    }
                }
                "fork_text_copy" => {
                    if let Some(fork) = &mut thread.fork {
                        fork.text_copy = true;
                    }
                }
                // A fork's copy of a lifecycle event that started a new reply.
                "reply_break" => current_assistant = None,
                "lifecycle" if starts_reply(event) => {
                    current_assistant = None;
                }
                _ => {}
            }
        }
        if self.interrupted {
            thread.activity.push(data::Activity {
                kind: "error".into(),
                title: "Prompt interrupted when the conversation engine stopped".into(),
                detail: "Retry continues from the saved session.".into(),
                running: false,
                ..Default::default()
            });
        }
        if let Some(error) = &self.storage_error {
            thread.activity.push(data::Activity {
                kind: "error".into(),
                title: "Conversation data could not be saved".into(),
                detail: error.clone(),
                running: false,
                ..Default::default()
            });
        }
        thread.prepare_search();
        thread
    }
}

/// Whether a lifecycle event starts a new reply rather than continuing the last one.
fn starts_reply(event: &TranscriptEvent) -> bool {
    let name = event.data.get("event").and_then(Value::as_str);
    name == Some("replacement_session_approved")
        || (name == Some("turn_started")
            && event.data.get("retry").and_then(Value::as_bool) == Some(true))
}

#[derive(Clone, Debug)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub directory: PathBuf,
    pub opened_at: Option<i64>,
    pub conversations: Vec<StoredConversation>,
}

impl ProjectRecord {
    pub fn to_workspace(&self) -> data::Workspace {
        data::Workspace {
            config: data::Config {
                id: self.id.clone(),
                name: self.name.clone(),
                provider: String::new(),
                directory: self.directory.clone(),
                opened_at: self.opened_at,
            },
            threads: self
                .conversations
                .iter()
                .map(StoredConversation::to_thread)
                .collect(),
            ..Default::default()
        }
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
                    opened_at: definition.opened_at,
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
        // Editing a project keeps when it was last opened.
        let opened_at = original
            .and_then(|old| self.projects.iter().find(|p| p.id == old))
            .and_then(|project| project.opened_at);
        let definition = ProjectDefinition {
            name: name.to_owned(),
            directory: directory.to_owned(),
            opened_at,
        };
        let text = serde_yaml_ng::to_string(&definition).map_err(|e| e.to_string())?;
        if let Some(old) = original {
            let old_path = root.join(old);
            safe_directory(&old_path)?;
            if id != old {
                fs::rename(&old_path, &destination).map_err(|e| file_error(&old_path, e))?;
            }
            let write = files::replace(&destination.join("project.yml"), text.as_bytes());
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
                .and_then(|()| files::write_new(&destination.join("project.yml"), text.as_bytes()));
            if let Err(error) = result {
                let _ = fs::remove_dir(destination.join("conversations"));
                let _ = fs::remove_dir(&destination);
                return Err(error);
            }
            self.projects.push(ProjectRecord {
                id: id.clone(),
                name: name.to_owned(),
                directory: directory.to_owned(),
                opened_at: None,
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
        launch: Launch,
        mcp_servers: Vec<McpServer>,
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
        // A new conversation always starts with the definition's defaults (scope R16).
        let settings = ConversationSettings {
            version: VERSION,
            agent_id,
            title: title.to_owned(),
            status: "idle".into(),
            created_at: now_millis().to_string(),
            execution: ExecutionConfig {
                name: agent.name.clone(),
                harness: agent.harness.clone(),
                identity: agent.identity.clone(),
                command: launch.command,
                arguments: launch.arguments,
                environment: launch.environment,
                selections: Selections {
                    model: agent.model.clone(),
                    effort: agent.effort.clone(),
                    mode: agent.mode.clone(),
                    other: Vec::new(),
                },
                system_instructions: agent.system_instructions.clone(),
                instructions_mode: agent.instructions_mode,
                directory: project.directory.clone(),
                mcp_servers,
            },
            session_id: None,
            previous_session_ids: Vec::new(),
            options: Vec::new(),
            commands: Vec::new(),
            features: Features::default(),
            todo: Vec::new(),
            forked_from: None,
        };
        let conversation = write_conversation(&base, settings, Vec::new())?;
        let id = conversation.id.clone();
        project.conversations.push(conversation);
        Ok(id)
    }

    /// Copies a conversation's visible history through its reply `message`
    /// into a new conversation in the same project. Raw protocol traffic,
    /// lifecycle records and permission decisions stay behind.
    pub fn fork_conversation(&mut self, source_id: &str, message: usize) -> Result<String, String> {
        checked_id(source_id)?;
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let project = self
            .projects
            .iter_mut()
            .find(|p| p.conversations.iter().any(|c| c.id == source_id))
            .ok_or_else(|| format!("Conversation {source_id} no longer exists."))?;
        let source = project
            .conversations
            .iter()
            .find(|c| c.id == source_id)
            .expect("found above");
        let thread = source.to_thread();
        if !thread.ends_turn(message) {
            return Err("Fork from the last reply of a finished turn.".into());
        }
        let base = root.join(&project.id).join("conversations");
        safe_directory(root)?;
        safe_directory(&root.join(&project.id))?;
        safe_directory(&base)?;
        let prompts = thread.messages[..=message]
            .iter()
            .filter(|m| m.role == "user")
            .count();
        let mut seen = 0;
        let mut events = Vec::new();
        for event in source.events.iter().chain(&source.unsaved_events) {
            if event.kind == "message" && event.data["role"] == "user" {
                seen += 1;
                if seen > prompts {
                    break;
                }
            }
            match event.kind.as_str() {
                "message" | "assistant_chunk" | "thought_chunk" | "message_update" | "tool"
                | "error" => {
                    events.push(event.clone());
                }
                "lifecycle" if starts_reply(event) => events.push(TranscriptEvent {
                    kind: "reply_break".into(),
                    data: Value::Object(serde_json::Map::new()),
                    ..event.clone()
                }),
                _ => {}
            }
        }
        events.push(TranscriptEvent::new(
            "message_read",
            serde_json::json!({"through": message}),
        ));
        let settings = ConversationSettings {
            title: format!("{} (fork)", source.settings.title),
            status: "idle".into(),
            created_at: now_millis().to_string(),
            session_id: None,
            previous_session_ids: Vec::new(),
            forked_from: Some(ForkOrigin {
                conversation_id: source_id.to_owned(),
                title: source.settings.title.clone(),
                message,
            }),
            ..source.settings.clone()
        };
        let conversation = write_conversation(&base, settings, events)?;
        let id = conversation.id.clone();
        project.conversations.push(conversation);
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
            || conversation.settings.execution.fixed() != settings.execution.fixed()
        {
            return Err(
                "An existing conversation's agent and launch settings cannot change.".into(),
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

    /// Saves an event before returning, with everything queued before it.
    pub fn record_event(
        &mut self,
        conversation_id: &str,
        event: &TranscriptEvent,
    ) -> Result<(), String> {
        self.queue_event(conversation_id, event.clone())?;
        self.flush(conversation_id)
    }

    /// Queues a streamed event; [`Self::flush_due`] writes it within
    /// [`FLUSH_INTERVAL_MS`] (scope R41).
    pub fn queue_event(
        &mut self,
        conversation_id: &str,
        event: TranscriptEvent,
    ) -> Result<(), String> {
        let conversation = self
            .projects
            .iter_mut()
            .flat_map(|p| &mut p.conversations)
            .find(|c| c.id == conversation_id)
            .ok_or_else(|| format!("Conversation {conversation_id} no longer exists."))?;
        conversation.unsaved_events.push(event);
        conversation.unsaved_since.get_or_insert_with(now_millis);
        conversation.storage_error.clone().map_or(Ok(()), Err)
    }

    fn flush(&mut self, conversation_id: &str) -> Result<(), String> {
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
        if let Some(error) = &conversation.storage_error {
            return Err(error.clone());
        }
        if conversation.unsaved_events.is_empty() {
            return Ok(());
        }
        let root = self.root.as_ref().map_err(Clone::clone)?;
        let path = root
            .join(project_id)
            .join("conversations")
            .join(conversation_id)
            .join("transcript.jsonl");
        conversation.unsaved_since = None;
        flush_events(conversation, &path)
    }

    /// Writes every conversation's queued events that have waited long
    /// enough, one batch each. Returns the conversations that failed.
    pub fn flush_due(&mut self) -> Vec<(String, String)> {
        let due = now_millis().saturating_sub(FLUSH_INTERVAL_MS);
        let ids: Vec<String> = self
            .projects
            .iter()
            .flat_map(|p| &p.conversations)
            .filter(|c| c.storage_error.is_none() && c.unsaved_since.is_some_and(|at| at <= due))
            .map(|c| c.id.clone())
            .collect();
        ids.into_iter()
            .filter_map(|id| self.flush(&id).err().map(|error| (id, error)))
            .collect()
    }

    /// The conversation's recorded ACP traffic, newest last.
    pub fn traffic(&self, conversation_id: &str, limit: usize) -> Vec<Value> {
        let Some(conversation) = self.conversation(conversation_id) else {
            return Vec::new();
        };
        let all: Vec<_> = conversation
            .events
            .iter()
            .chain(&conversation.unsaved_events)
            .filter(|event| event.kind == "traffic")
            .map(|event| event.data.clone())
            .collect();
        all[all.len().saturating_sub(limit)..].to_vec()
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

/// What changed on disk outside the engine, from [`ProjectStore::reconcile`].
#[derive(Default)]
pub struct Reconciled {
    /// Added projects, and projects whose name or directory changed.
    pub projects: Vec<data::Workspace>,
    pub removed: Vec<String>,
    /// Added or changed conversations.
    pub conversations: Vec<String>,
    /// Problems the fresh load found that weren't known before.
    pub errors: Vec<String>,
}

impl ProjectStore {
    /// Takes in a fresh load of the projects folder. Conversations in `busy`
    /// and projects in `deleting` are the engine's own business and stay.
    pub fn reconcile(
        &mut self,
        fresh: Self,
        busy: &std::collections::HashSet<String>,
        deleting: &std::collections::HashSet<String>,
    ) -> Reconciled {
        let mut changes = Reconciled {
            errors: fresh
                .errors
                .iter()
                .filter(|error| !self.errors.contains(error))
                .cloned()
                .collect(),
            ..Default::default()
        };
        self.errors.clone_from(&fresh.errors);
        for project in &self.projects {
            if !deleting.contains(&project.id)
                && !fresh.projects.iter().any(|p| p.id == project.id)
                && project.conversations.iter().all(|c| !busy.contains(&c.id))
                && self
                    .root
                    .as_ref()
                    .is_ok_and(|root| !root.join(&project.id).exists())
            {
                changes.removed.push(project.id.clone());
            }
        }
        self.projects.retain(|p| !changes.removed.contains(&p.id));
        for fresh_project in fresh.projects {
            if deleting.contains(&fresh_project.id) {
                continue;
            }
            let Some(project) = self.projects.iter_mut().find(|p| p.id == fresh_project.id) else {
                changes.projects.push(fresh_project.to_workspace());
                self.projects.push(fresh_project);
                continue;
            };
            if project.name != fresh_project.name || project.directory != fresh_project.directory {
                project.name = fresh_project.name;
                project.directory = fresh_project.directory;
                changes.projects.push(project.to_workspace());
            }
            for conversation in fresh_project.conversations {
                if busy.contains(&conversation.id) {
                    continue;
                }
                match project
                    .conversations
                    .iter_mut()
                    .find(|c| c.id == conversation.id)
                {
                    None => {
                        changes.conversations.push(conversation.id.clone());
                        project.conversations.push(conversation);
                    }
                    // Unsaved history stays in memory until storage is retried.
                    Some(existing)
                        if existing.unsaved_events.is_empty()
                            && existing.pending_settings.is_none()
                            && (existing.settings != conversation.settings
                                || existing.persisted_len != conversation.persisted_len
                                || existing.storage_error != conversation.storage_error) =>
                    {
                        changes.conversations.push(conversation.id.clone());
                        *existing = conversation;
                    }
                    Some(_) => {}
                }
            }
        }
        changes
    }
}

/// Saves a new conversation folder, leaving nothing behind on failure.
fn write_conversation(
    base: &Path,
    settings: ConversationSettings,
    events: Vec<TranscriptEvent>,
) -> Result<StoredConversation, String> {
    let id = loop {
        let id = files::unique(&format!("{:x}", now_nanos()));
        match fs::create_dir(base.join(&id)) {
            Ok(()) => break id,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(file_error(base, e)),
        }
    };
    let folder = base.join(&id);
    let mut transcript = Vec::new();
    for event in &events {
        serde_json::to_writer(&mut transcript, event).map_err(|e| e.to_string())?;
        transcript.push(b'\n');
    }
    let result = serde_yaml_ng::to_string(&settings)
        .map_err(|e| e.to_string())
        .and_then(|text| files::write_new(&folder.join("conversation.yml"), text.as_bytes()))
        .and_then(|()| files::write_new(&folder.join("transcript.jsonl"), &transcript));
    if let Err(error) = result {
        let _ = fs::remove_file(folder.join("conversation.yml"));
        let _ = fs::remove_file(folder.join("transcript.jsonl"));
        let _ = fs::remove_dir(&folder);
        return Err(error);
    }
    Ok(StoredConversation {
        id,
        settings,
        events,
        unsaved_events: Vec::new(),
        storage_error: None,
        unsaved_since: None,
        persisted_len: transcript.len() as u64,
        needs_rollback: false,
        pending_settings: None,
        load_error: false,
        interrupted: false,
    })
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
        files::replace(path, text.as_bytes())
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
    let text = fs::read_to_string(&settings_path).map_err(|e| file_error(&settings_path, e))?;
    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).map_err(|e| file_error(&settings_path, e))?;
    // An earlier format stays on disk untouched and is not shown (scope R37).
    if yaml
        .get("version")
        .and_then(serde_yaml_ng::Value::as_u64)
        .is_none_or(|version| version < u64::from(VERSION))
    {
        return Ok(None);
    }
    let mut settings: ConversationSettings =
        serde_yaml_ng::from_value(yaml).map_err(|e| file_error(&settings_path, e))?;
    // Earlier versions saved running chats as "working" and new ones as "active".
    if settings.status == "active" {
        settings.status = "idle".into();
    }
    let interrupted = matches!(settings.status.as_str(), "processing" | "working");
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
        unsaved_since: None,
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
#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
