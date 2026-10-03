//! Messages between the conversation engine and its clients, one JSON object
//! per line. Nothing here assumes the client shares the engine's machine.
use crate::{
    agents::{AgentDefinition, AgentEntry, PermissionMode},
    data::{Activity, Message, Thread, Workspace},
    harness,
    storage::ExecutionConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

/// Bumped whenever a message changes shape. `hello`, `welcome`, `status`,
/// `stop_all` and `shutdown` must keep working across versions.
pub const PROTOCOL: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "messages are moved once, to or from the wire"
)]
pub enum ClientMessage {
    /// A command-line request doesn't count as a connected client.
    Hello {
        protocol: u32,
        cli: bool,
    },
    Request {
        id: u64,
        command: Command,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "messages are moved once, to or from the wire"
)]
pub enum EngineMessage {
    Welcome {
        status: Status,
    },
    Snapshot(Box<Snapshot>),
    Delta(Delta),
    Reply {
        id: u64,
        result: Result<Value, String>,
    },
    /// The engine is exiting on purpose.
    Bye,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "messages are moved once, to or from the wire"
)]
pub enum Command {
    /// Sends to `conversation_id`, or starts a conversation with `agent_id`.
    /// Replies with the conversation ID.
    Send {
        project_id: String,
        conversation_id: Option<String>,
        agent_id: Option<String>,
        permission_mode: Option<PermissionMode>,
        prompt: String,
    },
    Stop {
        id: String,
    },
    ForceStop {
        id: String,
    },
    Retry {
        id: String,
    },
    RetryStorage {
        id: String,
    },
    ReplaceSession {
        id: String,
    },
    SetPermissionMode {
        id: String,
        mode: PermissionMode,
    },
    SwitchSetting {
        id: String,
        effort: bool,
        value: String,
    },
    AnswerPermission {
        id: String,
        request_id: u64,
        option_id: String,
    },
    /// Completes, archives or reopens (`idle`) a conversation.
    SetStatus {
        id: String,
        status: String,
    },
    MarkRead {
        id: String,
        through: usize,
    },
    /// Replies with the project ID.
    SaveProject {
        original: Option<String>,
        name: String,
        directory: PathBuf,
    },
    /// Stops the project's agents, then deletes it. Replies once deleted.
    DeleteProject {
        id: String,
    },
    CancelDeleteProject {
        id: String,
    },
    ForceProject {
        id: String,
    },
    /// Replies with the agent ID.
    SaveAgent {
        original: Option<String>,
        definition: AgentDefinition,
        expected: Option<AgentDefinition>,
        overwrite: bool,
    },
    DeleteAgent {
        id: String,
    },
    RefreshHarnesses {
        fetch: bool,
    },
    /// Replies with `Result<Probed, ProbeError>`.
    Probe {
        probe: u64,
        harness: String,
        command: String,
        arguments: Vec<String>,
        model: Option<String>,
    },
    CancelProbe {
        probe: u64,
    },
    SetSettings {
        settings: EngineSettings,
    },
    Status,
    /// Stops every agent. Replies once all of them have exited.
    StopAll,
    /// Stop all, then the engine exits.
    Shutdown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineSettings {
    pub keep_running: bool,
    pub retry_limit: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub protocol: u32,
    pub version: String,
    pub pid: u32,
    pub daemon: bool,
    pub uptime_secs: u64,
    pub clients: usize,
    pub conversations: Vec<ActiveConversation>,
    pub log: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveConversation {
    pub project: String,
    pub title: String,
    /// `processing`, `retrying` or `waiting for permission`.
    pub state: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub projects: Vec<Workspace>,
    pub live: HashMap<String, Live>,
    pub agents: Vec<AgentEntry>,
    pub agent_errors: Vec<String>,
    pub harnesses: harness::Catalog,
    /// SVG icons by asset path: `registry-icons/<id>.svg`, `agent-avatars/<id>.svg`.
    pub icons: BTreeMap<String, String>,
    pub settings: EngineSettings,
    pub errors: Vec<String>,
    pub status: Status,
}

/// What a client shows of a conversation beyond its saved transcript.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Live {
    pub agent_id: String,
    /// An agent process is running for it.
    pub running: bool,
    pub processing: bool,
    pub shutting_down: bool,
    pub shutdown_stuck: bool,
    pub storage_failed: bool,
    pub replacement: bool,
    pub recovering_storage: bool,
    pub error: Option<String>,
    pub progress: Option<String>,
    pub permission: Vec<PendingPermission>,
    pub execution: Option<ExecutionConfig>,
    /// The ACP config options the agent offers now, or offered in its last session.
    pub options: Vec<Value>,
    pub permission_mode: Option<PermissionMode>,
    pub last_read_through: Option<usize>,
    /// The streaming assistant message of the running turn.
    pub assistant: Option<usize>,
    pub last_prompt: String,
    pub worked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingPermission {
    pub request_id: u64,
    pub title: String,
    pub options: Vec<PermissionOption>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "delta", rename_all = "snake_case")]
pub enum Delta {
    Live {
        id: String,
        live: Live,
    },
    Text {
        id: String,
        text: String,
    },
    Tool {
        id: String,
        tool_id: String,
        title: String,
        status: String,
        detail: String,
        kind: String,
        paths: Vec<String>,
    },
    Usage {
        id: String,
        used: u64,
        size: u64,
    },
    Status {
        id: String,
        status: String,
    },
    Message {
        id: String,
        message: Message,
    },
    Read {
        id: String,
        through: usize,
    },
    /// Adds a conversation at the top of its project, or replaces it.
    Thread {
        project_id: String,
        thread: Thread,
    },
    /// Adds or replaces a project; `previous` is its ID before a rename.
    Project {
        previous: Option<String>,
        workspace: Workspace,
    },
    ProjectRemoved {
        id: String,
    },
    Agents {
        entries: Vec<AgentEntry>,
        errors: Vec<String>,
    },
    Harnesses(harness::Catalog),
    Icons(BTreeMap<String, String>),
    Settings(EngineSettings),
    EngineStatus(Status),
    /// A problem worth a notification, such as an unreadable file.
    Notice {
        message: String,
    },
}

fn locate<'a>(projects: &'a mut [Workspace], id: &str) -> Option<&'a mut Thread> {
    projects
        .iter_mut()
        .flat_map(|project| &mut project.threads)
        .find(|thread| thread.id == id)
}

/// Applies a conversation change. The engine and every client run the same
/// code, so they hold the same state. Returns whether it was a conversation change.
pub fn apply(projects: &mut [Workspace], live: &mut HashMap<String, Live>, delta: &Delta) -> bool {
    match delta {
        Delta::Live { id, live: state } => {
            live.insert(id.clone(), state.clone());
        }
        Delta::Text { id, text } => {
            let state = live.entry(id.clone()).or_default();
            state.worked = true;
            if let Some(thread) = locate(projects, id) {
                if let Some(message) = state.assistant.and_then(|i| thread.messages.get_mut(i)) {
                    message.text.push_str(text);
                } else {
                    state.assistant = Some(thread.messages.len());
                    thread.push_message(Message {
                        role: "assistant".into(),
                        text: text.clone(),
                        created_at: crate::recency::now().to_string(),
                        ..Default::default()
                    });
                }
                thread.prepare_search();
            }
        }
        Delta::Tool {
            id,
            tool_id,
            title,
            status,
            detail,
            kind,
            paths,
        } => {
            live.entry(id.clone()).or_default().worked = true;
            if let Some(thread) = locate(projects, id) {
                let turn = thread.messages.iter().rposition(|m| m.role == "user");
                let key = format!("tool:{tool_id}");
                let item = if let Some(index) = thread.activity.iter().position(|a| a.kind == key) {
                    index
                } else {
                    thread.activity.push(Activity {
                        kind: key,
                        ..Default::default()
                    });
                    thread.activity.len() - 1
                };
                let activity = &mut thread.activity[item];
                activity.title = format!("{title} ({status})");
                activity.detail.clone_from(detail);
                activity.running = matches!(status.as_str(), "pending" | "in_progress");
                if !kind.is_empty() {
                    activity.tool.clone_from(kind);
                }
                if !paths.is_empty() {
                    activity.paths.clone_from(paths);
                }
                activity.turn = activity.turn.or(turn);
            }
        }
        Delta::Usage { id, used, size } => {
            if let Some(thread) = locate(projects, id) {
                thread.context = Some((*used, *size));
            }
        }
        Delta::Status { id, status } => {
            if let Some(thread) = locate(projects, id) {
                thread.status.clone_from(status);
            }
        }
        Delta::Message { id, message } => {
            if let Some(thread) = locate(projects, id) {
                thread.push_message(message.clone());
            }
        }
        Delta::Read { id, through } => {
            live.entry(id.clone()).or_default().last_read_through = Some(*through);
            if let Some(thread) = locate(projects, id) {
                for message in thread.messages.iter_mut().take(through + 1) {
                    message.read = true;
                }
                thread.prepare_search();
            }
        }
        Delta::Thread { project_id, thread } => {
            let mut thread = thread.clone();
            thread.prepare_search();
            if let Some(existing) = locate(projects, &thread.id) {
                *existing = thread;
            } else if let Some(project) = projects.iter_mut().find(|p| p.config.id == *project_id) {
                project.threads.insert(0, thread);
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_and_client_reach_the_same_transcript() {
        let thread = Thread {
            id: "c".into(),
            ..Default::default()
        };
        let mut projects = vec![Workspace {
            config: crate::data::Config {
                id: "p".into(),
                ..Default::default()
            },
            threads: vec![thread],
            ..Default::default()
        }];
        let mut live = HashMap::new();
        for delta in [
            Delta::Text {
                id: "c".into(),
                text: "Good ".into(),
            },
            Delta::Text {
                id: "c".into(),
                text: "morning".into(),
            },
        ] {
            // Deltas travel as JSON lines.
            let line = serde_json::to_string(&EngineMessage::Delta(delta)).unwrap();
            let EngineMessage::Delta(delta) = serde_json::from_str(&line).unwrap() else {
                panic!("not a delta");
            };
            assert!(apply(&mut projects, &mut live, &delta));
        }
        let messages = &projects[0].threads[0].messages;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "Good morning");
        assert!(projects[0].threads[0].unread());
        apply(
            &mut projects,
            &mut live,
            &Delta::Read {
                id: "c".into(),
                through: 0,
            },
        );
        assert!(!projects[0].threads[0].unread());
    }
}
