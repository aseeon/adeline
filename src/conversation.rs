//! Adeline's own conversation model: what an agent session offers and does,
//! independent of ACP. `acp.rs` translates the protocol to and from it, and
//! the engine↔client protocol, storage and UI use only these types.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What an option controls. The composer gives Model, Effort and Mode their own
/// menus; everything else goes under More options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Model,
    Effort,
    Mode,
    #[default]
    Other,
}

/// One value of a select option.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub value: String,
    pub name: String,
    pub description: String,
    /// The agent's group, else the `provider/` prefix of the value, else empty.
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

/// A setting the running agent offers for its session.
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

    /// The display name of one of its values.
    pub fn name_of<'a>(&'a self, value: &'a str) -> &'a str {
        self.choices()
            .iter()
            .find(|choice| choice.value == value)
            .map_or(value, |choice| choice.name.as_str())
    }
}

/// The first option of a category.
pub fn option(options: &[SessionOption], category: Category) -> Option<&SessionOption> {
    options.iter().find(|option| option.category == category)
}

/// A value chosen for an option, by option category for Model, Effort and
/// Mode (which the agent definition can preset), else by option ID.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Selections {
    pub model: String,
    pub effort: String,
    pub mode: String,
    /// More options: option ID and value, `true`/`false` for a boolean.
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

/// A slash command the agent advertises.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentCommand {
    pub name: String,
    pub description: String,
    /// What to type after the command, when it takes input.
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

/// One step of the agent's TODO list (ACP calls the list `plan`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoStep {
    pub text: String,
    pub status: StepStatus,
}

/// The step the TODO button shows: the first in progress, else the first pending.
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
    /// Whether the call changes files.
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

/// The agent's answers to a permission request. "Reject always" never reaches
/// Adeline's model: it stays hidden (scope R21).
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
    /// A turn-end signal from the agent's profile, without a prompt response.
    Signal,
}

/// Where a conversation's turn stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    #[default]
    Idle,
    Running,
    /// Waiting for the user: a permission, a login, a decision about the session.
    NeedsAction,
}

/// A way the agent offers to log in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthMethod {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Runs the agent's own program with these arguments in the user's
    /// terminal, instead of asking the agent to log in.
    #[serde(default)]
    pub terminal: Option<TerminalLogin>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLogin {
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
}

/// What the running agent offers beyond prompting. Anything it does not
/// offer is shown as unavailable (scope R5).
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
    /// Known once the agent has answered `initialize` at least once.
    pub known: bool,
}

/// An MCP server given to agents at session start (scope R28).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// Files over this size can't be attached (scope R23).
pub const ATTACHMENT_LIMIT: u64 = 20 * 1024 * 1024;

/// A file sent with a prompt. Images and small files carry their bytes;
/// `path` names a file on the engine's machine to read instead.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// The file's bytes, base64-encoded.
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

/// The MIME type for a file name, for the common types agents read.
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

/// Standard base64 with padding.
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

/// Decodes standard base64; `None` for anything else.
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

/// Which way a line of ACP traffic went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    ToAgent,
    FromAgent,
    Stderr,
}

/// What Adeline made of a line from the agent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficNote {
    #[default]
    None,
    NotJson,
    Unknown,
}

/// One line of the ACP traffic view (scope R36).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficEntry {
    /// Milliseconds since the epoch.
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
}
