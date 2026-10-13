//! Adeline's own conversation model: what an agent session offers and does,
//! independent of ACP.
//!
//! `acp.rs` translates the protocol to and from it, and the engine↔client protocol,
//! storage and UI use only these types.
//!
//! The TODO list is what ACP calls the `plan`. `StopReason::Signal` ends a turn
//! on the profile's turn-end signal, without a prompt response. An `Attachment`
//! carries its bytes base64-encoded, or a `path` on the engine's machine to read
//! instead, and `read_attachment` reads a local file into one. Traffic times
//! are epoch milliseconds.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Model,
    Effort,
    Mode,
    #[default]
    Other,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub value: String,
    pub name: String,
    pub description: String,
    pub group: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionKind {
    Select {
        current: String,
        choices: Vec<Choice>,
    },
    Boolean {
        current: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub category: Category,
    pub kind: OptionKind,
}

impl SessionOption {
    pub fn current(&self) -> String {
        match &self.kind {
            OptionKind::Select { current, .. } => current.clone(),
            OptionKind::Boolean { current } => current.to_string(),
        }
    }

    pub fn choices(&self) -> &[Choice] {
        match &self.kind {
            OptionKind::Select { choices, .. } => choices,
            OptionKind::Boolean { .. } => &[],
        }
    }

    pub fn offers(&self, value: &str) -> bool {
        match &self.kind {
            OptionKind::Select { choices, .. } => choices.iter().any(|c| c.value == value),
            OptionKind::Boolean { .. } => matches!(value, "true" | "false"),
        }
    }

    pub fn name_of<'a>(&'a self, value: &'a str) -> &'a str {
        self.choices()
            .iter()
            .find(|choice| choice.value == value)
            .map_or(value, |choice| choice.name.as_str())
    }
}

pub fn option(options: &[SessionOption], category: Category) -> Option<&SessionOption> {
    options.iter().find(|option| option.category == category)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Selections {
    pub model: String,
    pub effort: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other: Vec<(String, String)>,
}

impl Selections {
    pub fn get(&self, option: &SessionOption) -> Option<&str> {
        let value = match option.category {
            Category::Model => &self.model,
            Category::Effort => &self.effort,
            Category::Mode => &self.mode,
            Category::Other => {
                return self
                    .other
                    .iter()
                    .find(|(id, _)| *id == option.id)
                    .map(|(_, value)| value.as_str());
            }
        };
        (!value.is_empty()).then_some(value.as_str())
    }

    pub fn set(&mut self, category: Category, id: &str, value: &str) {
        match category {
            Category::Model => value.clone_into(&mut self.model),
            Category::Effort => value.clone_into(&mut self.effort),
            Category::Mode => value.clone_into(&mut self.mode),
            Category::Other => {
                self.other.retain(|(other, _)| other != id);
                self.other.push((id.to_owned(), value.to_owned()));
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentCommand {
    pub name: String,
    pub description: String,
    pub hint: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoStep {
    pub text: String,
    pub status: StepStatus,
}

// Earlier versions saved new chats as "active" and running ones as "working".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadStatus {
    #[serde(alias = "working")]
    Processing,
    Blocked,
    Completed,
    Archived,
    #[default]
    #[serde(alias = "active", other)]
    Idle,
}

impl ThreadStatus {
    pub fn closed(self) -> bool {
        matches!(self, Self::Completed | Self::Archived)
    }

    pub fn live(self) -> bool {
        matches!(self, Self::Processing | Self::Blocked)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    #[default]
    #[serde(other)]
    Other,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Other => "",
        }
    }
}

pub fn title_from(prompt: &str) -> String {
    const MAX: usize = 100;
    if prompt.chars().count() > MAX {
        format!("{}…", prompt.chars().take(MAX).collect::<String>())
    } else {
        prompt.into()
    }
}

pub fn current_step(steps: &[TodoStep]) -> Option<&TodoStep> {
    steps
        .iter()
        .find(|step| step.status == StepStatus::InProgress)
        .or_else(|| steps.iter().find(|step| step.status == StepStatus::Pending))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    #[default]
    #[serde(other)]
    Other,
}

impl ToolKind {
    pub fn edits(self) -> bool {
        matches!(self, Self::Edit | Self::Delete | Self::Move)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolStatus {
    pub fn running(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    pub kind: PermissionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    Signal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    #[default]
    Idle,
    Running,
    NeedsAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthMethod {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub terminal: Option<TerminalLogin>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLogin {
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub images: bool,
    pub embedded_files: bool,
    pub load: bool,
    pub resume: bool,
    pub fork: bool,
    pub close: bool,
    pub steering: bool,
    pub logout: bool,
    pub mcp_http: bool,
    pub mcp_sse: bool,
    pub auth: Vec<AuthMethod>,
    pub known: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    #[serde(flatten)]
    pub transport: McpTransport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpTransport {
    Stdio {
        command: PathBuf,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        arguments: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        environment: Vec<(String, String)>,
    },
    Http {
        url: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        headers: Vec<(String, String)>,
    },
}

impl McpServer {
    pub fn kind(&self) -> &'static str {
        match self.transport {
            McpTransport::Stdio { .. } => "Command",
            McpTransport::Http { .. } => "HTTP",
        }
    }
}

pub const ATTACHMENT_LIMIT: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub size: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Attachment {
    pub fn image(&self) -> bool {
        self.mime.starts_with("image/")
    }
}

pub fn read_attachment(path: &std::path::Path) -> anyhow::Result<Attachment> {
    use anyhow::Context as _;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mime = mime_for(&name).to_owned();
    let size = std::fs::metadata(path)
        .with_context(|| path.display().to_string())?
        .len();
    // Too big to read; `attach` reports it.
    let data = if size > ATTACHMENT_LIMIT {
        String::new()
    } else {
        base64(&std::fs::read(path).with_context(|| path.display().to_string())?)
    };
    Ok(Attachment {
        name,
        mime,
        size,
        data,
        path: None,
    })
}

pub fn mime_for(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "txt" | "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "toml" | "yml" | "yaml" | "c" | "h"
        | "cpp" | "go" | "java" | "sh" | "ps1" | "css" | "xml" | "log" => "text/plain",
        _ => "application/octet-stream",
    }
}

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> shift & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let text = text.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= u32::from(value(c)?) << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    ToAgent,
    FromAgent,
    Stderr,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficNote {
    #[default]
    None,
    NotJson,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficEntry {
    pub at: u64,
    pub direction: Direction,
    pub text: String,
    #[serde(default)]
    pub note: TrafficNote,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_padding() {
        for text in ["", "a", "ab", "abc", "abcd", "abcde", "abcdef"] {
            let encoded = base64(text.as_bytes());
            assert_eq!(unbase64(&encoded).unwrap(), text.as_bytes(), "{text}");
        }
        assert_eq!(base64(b"abcdef"), "YWJjZGVm");
        assert_eq!(base64(b"ab"), "YWI=");
        assert!(unbase64("not base64!").is_none());
    }

    #[test]
    fn selections_keep_one_value_per_option() {
        let mut selections = Selections::default();
        selections.set(Category::Other, "fast", "true");
        selections.set(Category::Other, "fast", "false");
        selections.set(Category::Mode, "mode", "plan");
        assert_eq!(selections.other, [("fast".into(), "false".into())]);
        let mode = SessionOption {
            id: "mode".into(),
            name: "Mode".into(),
            description: String::new(),
            category: Category::Mode,
            kind: OptionKind::Select {
                current: "default".into(),
                choices: Vec::new(),
            },
        };
        assert_eq!(selections.get(&mode), Some("plan"));
    }

    #[test]
    fn the_current_step_is_the_running_one_else_the_next_pending() {
        let step = |text: &str, status| TodoStep {
            text: text.into(),
            status,
        };
        let steps = [
            step("a", StepStatus::Completed),
            step("b", StepStatus::Pending),
            step("c", StepStatus::InProgress),
        ];
        assert_eq!(current_step(&steps).unwrap().text, "c");
        assert_eq!(current_step(&steps[..2]).unwrap().text, "b");
        assert!(current_step(&steps[..1]).is_none());
    }

    #[test]
    fn statuses_keep_their_saved_names() {
        let parse = |text: &str| serde_json::from_str::<ThreadStatus>(text).unwrap();
        assert_eq!(parse(r#""active""#), ThreadStatus::Idle);
        assert_eq!(parse(r#""working""#), ThreadStatus::Processing);
        assert_eq!(parse(r#""someday""#), ThreadStatus::Idle);
        assert_eq!(
            serde_json::to_string(&ThreadStatus::Blocked).unwrap(),
            r#""blocked""#
        );
        assert_eq!(
            serde_json::from_str::<MessageRole>(r#""assistant""#).unwrap(),
            MessageRole::Assistant
        );
    }
}
