//! Messages between the conversation engine and its clients, one JSON object
//! per line. Nothing here assumes the client shares the engine's machine.
//!
//! Bump `PROTOCOL` whenever a message changes shape. `hello`, `welcome`,
//! `status`, `stop_all` and `shutdown` must keep working across versions.
//! Fields added later default, so older engines send `at: 0` and no `headless`.
//! Times are epoch milliseconds.
//!
//! A CLI `Hello` doesn't count as a connected client. With `resume`, the engine
//! answers `Resumed` and the missed deltas while it still holds them, else a
//! full snapshot. `apply` runs in the engine and every client alike, so all
//! hold the same state.
//!
//! Replies other than `null`: `Send` and `Fork` give a conversation ID,
//! `SaveProject` a project ID, `SaveAgent` an agent ID, `TakeQueued` the
//! message's text, `Probe` a `Result<Probed, ProbeError>`, `PlanInstall` a
//! `Vec<install::Planned>`, `ListDirectory` a `Listing` (of the home folder
//! without a path) and `WatchTraffic` the traffic recorded so far.
//! `DeleteProject`, `StopAll`, `Shutdown` and `Login` reply once done, and
//! `Install` streams `Output` lines first.
use crate::{
    agents::{AgentDefinition, AgentEntry, ExecutionConfig},
    conversation::{
        AgentCommand, Attachment, AuthMethod, Category, Features, McpServer, MessageRole,
        PermissionOption, SessionOption, ThreadStatus, TodoStep, TrafficEntry, TurnState,
    },
    data::{Message, Thread, ToolReport, Workspace},
    harness, install,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

pub const PROTOCOL: u32 = 4;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probed {
    pub identity: String,
    pub version: String,
    pub features: Features,
    pub options: Vec<SessionOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeError {
    pub message: String,
    #[serde(default)]
    pub auth: Vec<AuthMethod>,
    // The agent speaks another ACP version; retrying won't help.
    #[serde(default)]
    pub version: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "messages are moved once, to or from the wire"
)]
pub enum ClientMessage {
    Hello {
        protocol: u32,
        cli: bool,
        #[serde(default)]
        resume: Option<Resume>,
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
    Resumed,
    Delta(Delta),
    Reply {
        id: u64,
        result: Result<Value, String>,
    },
    Output {
        request: u64,
        line: String,
    },
    Traffic {
        id: String,
        entry: TrafficEntry,
    },
    Bye,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "messages are moved once, to or from the wire"
)]
pub enum Command {
    // During a turn the message is queued, or with `now` sent at once.
    Send {
        project_id: String,
        conversation_id: Option<String>,
        agent_id: Option<String>,
        prompt: String,
        #[serde(default)]
        attachments: Vec<Attachment>,
        #[serde(default)]
        now: bool,
    },
    TakeQueued {
        id: String,
        queued: u64,
    },
    SendQueuedNow {
        id: String,
        queued: u64,
    },
    Stop {
        id: String,
    },
    ForceStop {
        id: String,
    },
    // Kills the agent's process and continues the turn in a new one.
    Restart {
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
    Fork {
        id: String,
        message: usize,
    },
    SetOption {
        id: String,
        category: Category,
        option: String,
        value: String,
    },
    AnswerPermission {
        id: String,
        request_id: u64,
        option_id: String,
    },
    SetStatus {
        id: String,
        status: ThreadStatus,
    },
    MarkRead {
        id: String,
        through: usize,
    },
    SaveProject {
        original: Option<String>,
        name: String,
        directory: PathBuf,
    },
    DeleteProject {
        id: String,
    },
    CancelDeleteProject {
        id: String,
    },
    ForceProject {
        id: String,
    },
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
    PlanInstall {
        target: install::Target,
    },
    Install {
        target: install::Target,
    },
    Login {
        harness: String,
        command: String,
        arguments: Vec<String>,
        method: Option<AuthMethod>,
    },
    WatchTraffic {
        id: Option<String>,
    },
    SetSettings {
        settings: EngineSettings,
    },
    ListDirectory {
        path: Option<PathBuf>,
    },
    Status,
    StopAll,
    Shutdown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineSettings {
    pub keep_running: bool,
    pub retry_limit: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServer>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub epoch: String,
    pub seq: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Listing {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
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
    #[serde(default)]
    pub engine_id: String,
    #[serde(default)]
    pub headless: bool,
}

impl Status {
    pub fn build(&self) -> &'static str {
        if self.headless { "headless" } else { "full" }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveConversation {
    pub project: String,
    pub title: String,
    pub state: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub projects: Vec<Workspace>,
    pub live: HashMap<String, Live>,
    pub agents: Vec<AgentEntry>,
    pub agent_errors: Vec<String>,
    pub harnesses: harness::Catalog,
    pub icons: BTreeMap<String, String>,
    pub settings: EngineSettings,
    pub errors: Vec<String>,
    pub status: Status,
    pub epoch: String,
    pub seq: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Live {
    pub agent_id: String,
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
    pub options: Vec<SessionOption>,
    pub commands: Vec<AgentCommand>,
    pub features: Features,
    pub todo: Vec<TodoStep>,
    pub turn: TurnState,
    pub queued: Vec<Queued>,
    pub quiet_since: Option<u64>,
    pub auth_required: bool,
    pub stderr: String,
    pub last_read_through: Option<usize>,
    pub assistant: Option<usize>,
    pub last_prompt: String,
    pub worked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingPermission {
    pub request_id: u64,
    pub title: String,
    pub options: Vec<PermissionOption>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "delta", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "deltas are moved once, to or from the wire"
)]
pub enum Delta {
    Live {
        id: String,
        live: Live,
    },
    Text {
        id: String,
        text: String,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        at: u64,
    },
    Thought {
        id: String,
        text: String,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        at: u64,
    },
    Tool {
        id: String,
        report: ToolReport,
        #[serde(default)]
        at: u64,
    },
    Title {
        id: String,
        title: String,
    },
    Usage {
        id: String,
        used: u64,
        size: u64,
    },
    Status {
        id: String,
        status: ThreadStatus,
    },
    Message {
        id: String,
        message: Message,
    },
    Read {
        id: String,
        through: usize,
    },
    Thread {
        project_id: String,
        thread: Thread,
    },
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
    Notice {
        message: String,
    },
    OpenUrl {
        url: String,
    },
}

fn received(at: u64) -> u64 {
    if at == 0 {
        crate::recency::now_ms()
    } else {
        at
    }
}

fn locate<'a>(projects: &'a mut [Workspace], id: &str) -> Option<&'a mut Thread> {
    projects
        .iter_mut()
        .flat_map(|project| &mut project.threads)
        .find(|thread| thread.id == id)
}

#[expect(
    clippy::implicit_hasher,
    reason = "every caller keeps its live states in a default HashMap"
)]
pub fn apply(projects: &mut [Workspace], live: &mut HashMap<String, Live>, delta: &Delta) -> bool {
    // Returns whether the delta changed a conversation.
    match delta {
        Delta::Live { id, live: state } => {
            live.insert(id.clone(), state.clone());
        }
        Delta::Text {
            id,
            text,
            message,
            at,
        }
        | Delta::Thought {
            id,
            text,
            message,
            at,
        } => {
            let state = live.entry(id.clone()).or_default();
            state.worked = true;
            if let Some(thread) = locate(projects, id) {
                let thought = matches!(delta, Delta::Thought { .. });
                thread.stream(
                    &mut state.assistant,
                    message.as_deref(),
                    text,
                    thought,
                    received(*at),
                );
                if !thought {
                    thread.search_stale = true;
                }
            }
        }
        Delta::Tool { id, report, at } => {
            live.entry(id.clone()).or_default().worked = true;
            if let Some(thread) = locate(projects, id) {
                thread.apply_tool(report, received(*at));
            }
        }
        Delta::Title { id, title } => {
            if let Some(thread) = locate(projects, id) {
                title.clone_into(&mut thread.title);
                thread.prepare_search();
            }
        }
        Delta::Usage { id, used, size } => {
            if let Some(thread) = locate(projects, id) {
                thread.context = Some((*used, *size));
            }
        }
        Delta::Status { id, status } => {
            if let Some(thread) = locate(projects, id) {
                thread.status = *status;
                if thread.search_stale {
                    thread.prepare_search();
                }
            }
        }
        Delta::Message { id, message } => {
            if let Some(thread) = locate(projects, id) {
                if message.role == MessageRole::User {
                    let sent = crate::recency::parse(&message.created_at)
                        .and_then(|at| u64::try_from(at).ok())
                        .map_or_else(crate::recency::now_ms, |at| at * 1000);
                    thread.timing.prompt(sent);
                }
                thread.push_message(message.clone());
            }
        }
        Delta::Read { id, through } => {
            live.entry(id.clone()).or_default().last_read_through = Some(*through);
            if let Some(thread) = locate(projects, id) {
                for message in thread.messages.iter_mut().take(through.saturating_add(1)) {
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
            Delta::Thought {
                id: "c".into(),
                text: "Greet them.".into(),
                message: None,
                at: 0,
            },
            Delta::Text {
                id: "c".into(),
                text: "Good ".into(),
                message: None,
                at: 0,
            },
            Delta::Text {
                id: "c".into(),
                text: "morning".into(),
                message: None,
                at: 0,
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
        assert_eq!(messages[0].thought, "Greet them.");
        assert!(projects[0].threads[0].unread());
        assert!(!projects[0].threads[0].search_text.contains("morning"));
        apply(
            &mut projects,
            &mut live,
            &Delta::Status {
                id: "c".into(),
                status: ThreadStatus::Idle,
            },
        );
        assert!(projects[0].threads[0].search_text.contains("good morning"));
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
