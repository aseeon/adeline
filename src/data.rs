use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    #[serde(skip)]
    pub counts: [usize; 4],
    pub config: Config,
    pub threads: Vec<Thread>,
    pub decisions: Vec<Decision>,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub directory: PathBuf,
}
#[derive(Clone, Default, Deserialize)]
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
}
#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    pub text: String,
    pub created_at: String,
    pub read: bool,
    pub images: Vec<String>,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Activity {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub running: bool,
}

#[derive(Clone, Default, Deserialize)]
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
    for project in &mut projects {
        project.threads.reverse();
        for thread in &mut project.threads {
            if let Some(work) = data.scene.working.iter().find(|w| w.thread_id == thread.id) {
                thread.status = "working".into();
                thread.activity = work.activity.clone();
            }
            thread.prepare_search();
        }
        project.rebuild_counts();
    }
    projects
}

impl Thread {
    pub fn push_message(&mut self, message: Message) {
        if self.search_text.len() == 0 {
            self.prepare_search();
        }
        let separator = if self.messages.is_empty() { "" } else { " " };
        self.search_text
            .append(&format!("{separator}{}", message.text).to_lowercase());
        if message.role == "assistant" && !message.read {
            self.unread_messages.push(self.messages.len());
        }
        self.messages.push(message);
    }
    pub fn unread(&self) -> bool {
        !self.unread_messages.is_empty()
    }
    pub fn mark_read(&mut self) {
        for index in self.unread_messages.drain(..) {
            self.messages[index].read = true;
        }
    }
    pub fn flags(&self) -> [usize; 4] {
        [
            1,
            usize::from(self.status == "blocked"),
            usize::from(self.status == "working"),
            usize::from(self.unread()),
        ]
    }
    pub fn matches_status(&self, filter: usize, completed: bool) -> bool {
        (completed || !matches!(self.status.as_str(), "completed" | "archived"))
            && match filter {
                1 => self.status == "blocked",
                2 => self.status == "working",
                _ => true,
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
    #[cfg(test)]
    pub fn matches(&self, query: &str, filter: usize, completed: bool) -> bool {
        self.matches_status(filter, completed)
            && (query.is_empty() || self.search_text.contains(&query.to_lowercase()))
    }
}
impl Workspace {
    pub fn attention_count(&self) -> usize {
        self.threads
            .iter()
            .filter(|thread| thread.status == "blocked" || thread.unread())
            .count()
    }
    pub fn rebuild_counts(&mut self) {
        self.counts = [0; 4];
        for thread in &self.threads {
            for (count, value) in self.counts.iter_mut().zip(thread.flags()) {
                *count += value;
            }
        }
    }
    pub fn update_counts(&mut self, before: [usize; 4], after: [usize; 4]) {
        for ((count, before), after) in self.counts.iter_mut().zip(before).zip(after) {
            *count = *count - before + after;
        }
    }
    pub fn notifications(&self) -> (usize, usize) {
        (self.counts[2], self.counts[3])
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
    #[test]
    fn archived_and_completed_conversations_stay_in_saved_history() {
        let mut thread = Thread::default();
        for status in ["completed", "archived"] {
            thread.status = status.into();
            assert!(!thread.matches("", 0, false));
            assert!(thread.matches("", 0, true));
        }
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
    fn cached_badges_follow_one_thread_mutation() {
        let mut projects = load();
        let workspace = &mut projects[0];
        for index in 0..workspace.threads.len() {
            let before = workspace.threads[index].flags();
            workspace.threads[index].mark_read();
            workspace.threads[index].status = "working".into();
            workspace.update_counts(before, workspace.threads[index].flags());
            let mut expected = workspace.clone();
            expected.rebuild_counts();
            assert_eq!(workspace.counts, expected.counts);
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
                thread.matches(query, 0, true),
                original.contains(&query.to_lowercase())
            );
        }
        thread.messages.push(Message {
            text: "Newly appended".into(),
            ..Default::default()
        });
        thread.prepare_search();
        assert!(thread.matches("newly APPENDED", 0, true));
        assert!(!thread.matches("anything", 1, true));
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
            assert_eq!(thread.status, "working");
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
    fn search_and_status_filters_compose() {
        let p = load();
        let t = p[0]
            .threads
            .iter()
            .find(|t| t.id == "adeline-session")
            .unwrap();
        assert!(t.matches("SESSION", 1, false));
        assert!(!t.matches("session", 2, true));
        assert!(!t.matches("no matching text", 0, true));
        let done = p[0]
            .threads
            .iter()
            .find(|t| t.status == "completed")
            .unwrap();
        assert!(!done.matches("", 0, false));
    }
}
