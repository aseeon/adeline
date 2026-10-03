use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    pub config: Config,
    pub threads: Vec<Thread>,
    pub decisions: Vec<Decision>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub directory: PathBuf,
    /// When the project was last opened, in seconds since the Unix epoch.
    pub opened_at: Option<i64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Thread {
    #[serde(skip)]
    pub unread_messages: Vec<usize>,
    #[serde(skip)]
    pub search_text: crate::prepared::SearchText,
    pub id: String,
    pub title: String,
    pub provider: String,
    pub status: String,
    pub messages: Vec<Message>,
    pub activity: Vec<Activity>,
    pub created_at: String,
    /// Tokens in the agent's context and the size of its window, once the agent reports them.
    pub context: Option<(u64, u64)>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    pub text: String,
    pub created_at: String,
    pub read: bool,
    pub images: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub running: bool,
    /// For a tool call, the protocol's tool kind: `read`, `edit`, `execute`, ...
    pub tool: String,
    /// Files a tool call reads or changes.
    pub paths: Vec<String>,
    /// The user message whose turn made this tool call.
    pub turn: Option<usize>,
}

/// What one agent turn did with its tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TurnSummary {
    /// Distinct files read.
    pub read: usize,
    /// Distinct files edited, deleted or moved.
    pub edited: usize,
    /// Tool calls made.
    pub tools: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Decision {
    pub title: String,
    pub body: String,
    pub options: Vec<String>,
    pub thread_id: String,
    pub selected: Option<usize>,
    pub resolved: bool,
}
#[derive(Deserialize)]
struct Project {
    workspace: Workspace,
}
#[derive(Deserialize)]
struct Working {
    #[serde(rename = "threadId")]
    thread_id: String,
    activity: Vec<Activity>,
}
#[derive(Deserialize)]
struct Scene {
    working: Vec<Working>,
}
#[derive(Deserialize)]
struct Fixture {
    workspace: Workspace,
    projects: Vec<Project>,
    scene: Scene,
}

pub fn load() -> Vec<Workspace> {
    let data: Fixture =
        serde_json::from_str(include_str!("../assets/workspace.json")).expect("valid bundled demo");
    let mut projects = vec![data.workspace];
    projects.extend(data.projects.into_iter().map(|p| p.workspace));
    shift_to(&mut projects, crate::recency::now());
    for project in &mut projects {
        project.threads.reverse();
        for thread in &mut project.threads {
            if let Some(work) = data.scene.working.iter().find(|w| w.thread_id == thread.id) {
                thread.status = "processing".into();
                thread.activity = work.activity.clone();
            }
            thread.prepare_search();
        }
    }
    projects
}

/// Move every demo timestamp by the same amount so the newest one is `now`.
/// The bundled chats keep their relative ages, and so their sections, whenever
/// the demo runs.
fn shift_to(projects: &mut [Workspace], now: i64) {
    let newest = projects
        .iter()
        .flat_map(|project| &project.threads)
        .map(Thread::last_activity)
        .max()
        .unwrap_or(0);
    if newest == 0 {
        return;
    }
    let offset = now - newest;
    let move_stamp = |stamp: &mut String| {
        if let Some(at) = crate::recency::parse(stamp) {
            *stamp = crate::recency::iso(at + offset);
        }
    };
    for thread in projects.iter_mut().flat_map(|project| &mut project.threads) {
        move_stamp(&mut thread.created_at);
        for message in &mut thread.messages {
            move_stamp(&mut message.created_at);
        }
    }
}

impl Thread {
    pub fn push_message(&mut self, message: Message) {
        if self.search_text.is_empty() {
            self.prepare_search();
        }
        let separator = if self.messages.is_empty() { "" } else { " " };
        self.search_text = format!("{}{separator}{}", self.search_text, message.text)
            .to_lowercase()
            .into();
        if message.role == "assistant" && !message.read {
            self.unread_messages.push(self.messages.len());
        }
        self.messages.push(message);
    }
    /// The user message that opened the turn an assistant message belongs to.
    pub fn turn_of(&self, message: usize) -> Option<usize> {
        self.messages
            .get(..message)?
            .iter()
            .rposition(|m| m.role == "user")
    }
    /// Whether an assistant message is the last reply of its turn.
    pub fn ends_turn(&self, message: usize) -> bool {
        self.messages
            .get(message)
            .is_some_and(|m| m.role == "assistant")
            && self
                .messages
                .get(message + 1)
                .is_none_or(|next| next.role == "user")
    }
    /// The tool calls made in the turn opened by a user message.
    pub fn turn_tools(&self, turn: usize) -> impl Iterator<Item = &Activity> {
        self.activity
            .iter()
            .filter(move |a| a.kind.starts_with("tool:") && a.turn == Some(turn))
    }
    /// What the turn opened by a user message did.
    pub fn turn_summary(&self, turn: usize) -> TurnSummary {
        let mut read = std::collections::HashSet::new();
        let mut edited = std::collections::HashSet::new();
        let mut tools = 0;
        for call in self.turn_tools(turn) {
            tools += 1;
            let files = match call.tool.as_str() {
                "read" => &mut read,
                "edit" | "delete" | "move" => &mut edited,
                _ => continue,
            };
            files.extend(call.paths.iter().map(String::as_str));
        }
        TurnSummary {
            read: read.len(),
            edited: edited.len(),
            tools,
        }
    }
    pub fn unread(&self) -> bool {
        !self.unread_messages.is_empty()
    }
    /// When the chat last changed: its newest dated message, else its creation.
    /// Seconds since the epoch; 0 when neither carries a readable time.
    pub fn last_activity(&self) -> i64 {
        self.messages
            .iter()
            .rev()
            .find_map(|message| crate::recency::parse(&message.created_at))
            .or_else(|| crate::recency::parse(&self.created_at))
            .unwrap_or(0)
    }
    pub fn mark_read(&mut self) {
        for index in self.unread_messages.drain(..) {
            self.messages[index].read = true;
        }
    }
    /// What the chat list searches and sorts by.
    pub fn search_record(&self) -> crate::prepared::SearchRecord {
        crate::prepared::SearchRecord {
            id: std::sync::Arc::from(self.id.as_str()),
            text: self.search_text.clone(),
            completed: self.status == "completed",
            archived: self.status == "archived",
            blocked: self.status == "blocked",
            processing: self.status == "processing",
            unread: self.unread(),
            agent: std::sync::Arc::from(self.provider.as_str()),
            activity: self.last_activity(),
        }
    }
    pub fn prepare_search(&mut self) {
        self.unread_messages = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "assistant" && !m.read)
            .map(|(i, _)| i)
            .collect();
        self.search_text = format!(
            "{} {}",
            self.title,
            self.messages
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_lowercase()
        .into();
    }
}
impl Workspace {
    pub fn attention_count(&self) -> usize {
        self.threads
            .iter()
            .filter(|thread| thread.status == "blocked" || thread.unread())
            .count()
    }
    /// Whether something here needs the user or is still working.
    pub fn busy(&self) -> bool {
        self.attention_count() > 0 || self.threads.iter().any(|t| t.status == "processing")
    }
}

pub fn image_asset(path: &str) -> &'static str {
    if path.contains("dad60182") {
        "notebook.png"
    } else if path.contains("3c5e8b84") {
        "studio.png"
    } else {
        "launch.png"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{Criteria, search};

    /// Whether the chat list shows a chat for a search and tab (0 all, 1 needs
    /// input, 2 processing), with completed and archived chats each shown only
    /// when asked for.
    fn shows(thread: &Thread, query: &str, filter: usize, completed: bool, archived: bool) -> bool {
        let criteria = Criteria {
            query: query.to_lowercase(),
            filter,
            completed,
            archived,
            agent: None,
        };
        search([thread.search_record()].iter(), &criteria, 0).matches() == 1
    }

    #[test]
    fn completed_and_archived_conversations_show_only_when_asked_for() {
        let mut thread = Thread {
            status: "completed".into(),
            ..Default::default()
        };
        assert!(!shows(&thread, "", 0, false, true));
        assert!(shows(&thread, "", 0, true, false));
        thread.status = "archived".into();
        assert!(!shows(&thread, "", 0, true, false));
        assert!(shows(&thread, "", 0, false, true));
    }

    #[test]
    fn appended_messages_keep_search_semantics_and_unread_counts() {
        let mut thread = Thread {
            title: "ΟΣ İ ŻÓŁĆ".into(),
            ..Default::default()
        };
        thread.prepare_search();
        for body in [
            "First body",
            "Second message",
            "ΟΔΟΣ",
            "\u{301} İstanbul",
            "",
        ] {
            let snapshot = thread.search_text.clone();
            thread.push_message(Message {
                role: "assistant".into(),
                text: body.into(),
                ..Default::default()
            });
            let original = format!(
                "{} {}",
                thread.title,
                thread
                    .messages
                    .iter()
                    .map(|m| m.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            for query in [
                "body second",
                "ος",
                "İstanbul",
                "οδός",
                "",
                "absent",
                "first",
            ] {
                let query = query.to_lowercase();
                assert_eq!(
                    thread.search_text.contains(&query),
                    original.contains(&query)
                );
            }
            assert!(thread.unread());
            assert!(snapshot.len() <= thread.search_text.len());
        }
        thread.mark_read();
        assert!(!thread.unread());
        assert!(thread.messages.iter().all(|m| m.read));
    }
    #[test]
    fn demo_chats_fill_every_section_whenever_the_demo_runs() {
        use crate::prepared::Group;
        // Noon UTC on two dates far apart; sections follow UTC days.
        for now in [1_790_510_400, 4_102_488_000] {
            let mut projects = load();
            shift_to(&mut projects, now);
            let newest = projects[0].threads.iter().map(Thread::last_activity).max();
            assert_eq!(newest, Some(now));
            let records: Vec<_> = projects[0]
                .threads
                .iter()
                .map(Thread::search_record)
                .collect();
            let criteria = Criteria {
                completed: true,
                ..Criteria::default()
            };
            let sections: Vec<_> = search(&records, &criteria, now)
                .groups
                .into_iter()
                .map(|(group, rows)| (group, rows.len()))
                .collect();
            let rows = |wanted: Group| {
                sections
                    .iter()
                    .find(|&&(group, _)| group == wanted)
                    .map_or(0, |&(_, rows)| rows)
            };
            // Four live chats split between waiting and running, and every period
            // holds enough chats to scroll.
            assert!(rows(Group::NeedsInput) > 0, "{sections:?}");
            assert!(rows(Group::Processing) > 0, "{sections:?}");
            assert_eq!(
                rows(Group::NeedsInput) + rows(Group::Processing),
                4,
                "{sections:?}"
            );
            for period in [Group::Today, Group::LastThreeDays, Group::Earlier] {
                assert!(rows(period) >= 6, "{sections:?}");
            }
        }
    }

    #[test]
    fn prepared_search_matches_original_full_text_semantics_and_edits() {
        let mut thread = Thread {
            title: "ŻÓŁĆ İstanbul ΟΣ".into(),
            messages: vec![
                Message {
                    text: "First body".into(),
                    ..Default::default()
                },
                Message {
                    text: "Second message".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        thread.prepare_search();
        for query in [
            "",
            "żółć",
            "İstanbul",
            "ος",
            "ος first",
            "body second",
            "absent",
        ] {
            let original = format!(
                "{} {}",
                thread.title,
                thread
                    .messages
                    .iter()
                    .map(|m| m.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            assert_eq!(
                shows(&thread, query, 0, true, true),
                original.contains(&query.to_lowercase())
            );
        }
        thread.messages.push(Message {
            text: "Newly appended".into(),
            ..Default::default()
        });
        thread.prepare_search();
        assert!(shows(&thread, "newly APPENDED", 0, true, true));
        assert!(!shows(&thread, "anything", 1, true, true));
    }
    #[test]
    fn fixture_keeps_chat_activity_and_decisions_with_their_projects() {
        let projects = load();
        for (id, working) in [
            ("demo-adeline", "adeline-search"),
            ("demo-skills", "skills-eval"),
            ("demo-relay", "relay-retry"),
        ] {
            let project = projects
                .iter()
                .find(|project| project.config.id == id)
                .unwrap();
            let thread = project
                .threads
                .iter()
                .find(|thread| thread.id == working)
                .unwrap();
            assert_eq!(thread.status, "processing");
            assert!(!thread.activity.is_empty());
            for decision in &project.decisions {
                let thread = project
                    .threads
                    .iter()
                    .find(|thread| thread.id == decision.thread_id)
                    .unwrap();
                assert_eq!(thread.status, "blocked");
            }
        }
    }
    #[test]
    fn a_turn_summary_counts_distinct_files_and_every_call() {
        let message = |role: &str| Message {
            role: role.into(),
            ..Default::default()
        };
        let call = |id: &str, tool: &str, paths: &[&str], turn: usize| Activity {
            kind: format!("tool:{id}"),
            tool: tool.into(),
            paths: paths.iter().map(|&p| p.to_owned()).collect(),
            turn: Some(turn),
            ..Default::default()
        };
        let thread = Thread {
            messages: vec![
                message("user"),
                message("assistant"),
                message("user"),
                message("assistant"),
                message("assistant"),
            ],
            activity: vec![
                call("1", "read", &["a.md"], 0),
                call("2", "read", &["a.md", "b.md"], 2),
                call("3", "edit", &["b.md"], 2),
                call("4", "execute", &[], 2),
                Activity {
                    kind: "error".into(),
                    turn: Some(2),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(thread.turn_of(4), Some(2));
        assert!(thread.ends_turn(1) && !thread.ends_turn(3) && thread.ends_turn(4));
        assert!(!thread.ends_turn(2));
        assert_eq!(
            thread.turn_summary(2),
            TurnSummary {
                read: 2,
                edited: 1,
                tools: 3
            }
        );
        assert_eq!(thread.turn_summary(0).tools, 1);
    }

    #[test]
    fn search_and_status_filters_compose() {
        let p = load();
        let t = p[0]
            .threads
            .iter()
            .find(|t| t.id == "adeline-session")
            .unwrap();
        assert!(shows(t, "SESSION", 1, false, false));
        assert!(!shows(t, "session", 2, true, true));
        assert!(!shows(t, "no matching text", 0, true, true));
        let done = p[0]
            .threads
            .iter()
            .find(|t| t.status == "completed")
            .unwrap();
        assert!(!shows(done, "", 0, false, false));
    }
}
