use crate::prepared::PreparedDocument;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    #[serde(skip)]
    pub counts: [usize; 4],
    pub config: Config,
    pub threads: Vec<Thread>,
    pub docs: Vec<Document>,
    pub recipes: Vec<Recipe>,
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
#[serde(default)]
pub struct Document {
    #[serde(skip)]
    pub prepared_edit: Option<(u64, usize)>,
    #[serde(skip)]
    pub search_title: Arc<str>,
    #[serde(skip)]
    pub prepared: Arc<PreparedDocument>,
    #[serde(skip)]
    pub revision: u64,
    #[serde(skip)]
    pub prepared_revision: u64,
    pub title: String,
    pub content: Arc<String>,
    pub filename: String,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub collection: String,
    pub instructions: String,
    pub schedule_label: String,
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
#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Service {
    #[serde(skip)]
    pub search_name: Arc<str>,
    #[serde(skip)]
    pub lines: Arc<[Arc<str>]>,
    #[serde(skip)]
    pub max_line_chars: usize,
    pub project_id: String,
    pub thread_id: String,
    pub name: String,
    pub output: String,
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
    services: Vec<Service>,
}
#[derive(Deserialize)]
struct Fixture {
    workspace: Workspace,
    projects: Vec<Project>,
    scene: Scene,
}

pub fn load() -> (Vec<Workspace>, Vec<Service>) {
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
        for doc in &mut project.docs {
            doc.search_title = doc.title.to_lowercase().into();
            if doc.content.len() < 32 * 1024 {
                doc.prepare();
            } else {
                doc.revision = 1;
            }
        }
        project.rebuild_counts();
    }
    let mut services = data.scene.services;
    for service in &mut services {
        service.prepare();
    }
    (projects, services)
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

impl Service {
    pub fn prepare(&mut self) {
        self.search_name = self.name.to_lowercase().into();
        self.lines = self.output.lines().map(Arc::from).collect();
        self.max_line_chars = self
            .lines
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0);
    }
}

impl Document {
    pub fn replace_line(&mut self, line: usize, replacement: &str) -> bool {
        let mut start = 0;
        let mut range = None;
        for (index, raw) in self.content.split_inclusive('\n').enumerate() {
            if index == line {
                let text = raw.strip_suffix('\n').unwrap_or(raw);
                let text = if raw.ends_with('\n') {
                    text.strip_suffix('\r').unwrap_or(text)
                } else {
                    text
                };
                range = Some(start..start + text.len());
                break;
            }
            start += raw.len();
        }
        let Some(range) = range else {
            return false;
        };
        if &self.content[range.clone()] == replacement {
            return true;
        }
        let prepared = if self.revision == self.prepared_revision {
            self.prepared.replace_line(line, replacement)
        } else {
            None
        };
        Arc::make_mut(&mut self.content).replace_range(range, replacement);
        let base = self.revision;
        self.revision += 1;
        self.prepared_edit = None;
        if let Some((parsed, block)) = prepared {
            self.prepared = Arc::new(parsed);
            self.prepared_revision = self.revision;
            self.prepared_edit = Some((base, block));
        }
        true
    }
    pub fn publish_prepared(&mut self, revision: u64, parsed: Arc<PreparedDocument>) -> bool {
        if self.revision != revision {
            return false;
        }
        self.prepared = parsed;
        self.prepared_edit = None;
        self.prepared_revision = revision;
        true
    }
    pub fn prepare(&mut self) {
        self.search_title = self.title.to_lowercase().into();
        self.prepared = Arc::new(PreparedDocument::parse(&self.content));
        self.prepared_edit = None;
        self.prepared_revision = self.revision;
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
    fn targeted_document_edits_match_full_parse_and_preserve_snapshots() {
        let mut doc = Document {
            content: Arc::new("# Heading\r\n\r\n- [ ] Task\r\nBody Żółć\r\n| A | B |\r\n".into()),
            ..Default::default()
        };
        doc.prepare();
        for (line, replacement) in [
            (2, "- [x] Task"),
            (3, "Longer **body**"),
            (4, "| One | Two |"),
            (3, "![art](x)"),
        ] {
            let previous = doc.prepared.clone();
            let before = doc.revision;
            assert!(doc.replace_line(line, replacement));
            assert_eq!(doc.prepared_revision, doc.revision);
            assert_eq!(doc.prepared_edit.unwrap().0, before);
            let expected = PreparedDocument::parse(&doc.content);
            assert_eq!(doc.prepared.words, expected.words);
            assert_eq!(doc.prepared.rich_rows, expected.rich_rows);
            for (a, b) in doc.prepared.blocks.iter().zip(expected.blocks.iter()) {
                assert_eq!((a.line, &a.raw, &a.kind), (b.line, &b.raw, &b.kind));
            }
            for (a, b) in doc.prepared.preview.iter().zip(&expected.preview) {
                assert_eq!((&a.raw, &a.kind), (&b.raw, &b.kind));
            }
            assert_eq!(previous.blocks[0].raw.as_ref(), "# Heading");
            assert!(doc.content.ends_with("\r\n"));
        }
        assert!(doc.replace_line(3, "Paragraph\n\nNew paragraph"));
        assert_ne!(doc.prepared_revision, doc.revision);
        doc.prepare();
        assert_eq!(doc.prepared_revision, doc.revision);
        assert!(!doc.replace_line(900, "invalid"));
    }
    #[test]
    fn cached_badges_follow_one_thread_mutation() {
        let (mut projects, _) = load();
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
    fn stale_document_parse_cannot_replace_newer_content() {
        let mut doc = Document {
            content: Arc::new("New text".into()),
            revision: 2,
            ..Default::default()
        };
        let old = Arc::new(PreparedDocument::parse("Old text"));
        assert!(!doc.publish_prepared(1, old));
        assert_ne!(doc.prepared_revision, doc.revision);
        assert!(doc.publish_prepared(2, Arc::new(PreparedDocument::parse(&doc.content))));
        assert_eq!(doc.prepared.blocks[0].raw.as_ref(), "New text");
        doc.content = Arc::new("Third revision".into());
        doc.revision += 1;
        assert!(!doc.publish_prepared(2, Arc::new(PreparedDocument::parse("Late result"))));
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
    fn fixture_preserves_all_projects_and_scene_states() {
        let (p, logs) = load();
        assert_eq!(p.len(), 3);
        assert_eq!(
            p.iter().map(|p| p.threads.len()).collect::<Vec<_>>(),
            vec![20, 20, 20]
        );
        assert_eq!(
            p[0].threads
                .iter()
                .filter(|t| t.status == "working")
                .count(),
            1
        );
        assert_eq!(logs.len(), 3);
        assert!(
            p.iter()
                .all(|p| !p.docs.is_empty() && !p.recipes.is_empty())
        );
        assert_eq!(
            p.iter().map(|p| p.config.name.as_str()).collect::<Vec<_>>(),
            vec!["ade-project", "just-skills", "relay-server"]
        );
        for project in &p {
            assert_eq!(project.docs.len(), 3);
            assert_eq!(project.recipes.len(), 3);
            assert_eq!(project.counts[..3], [20, 1, 1]);
            let ids: std::collections::HashSet<_> =
                project.threads.iter().map(|thread| &thread.id).collect();
            assert_eq!(ids.len(), project.threads.len());
            assert!(
                project
                    .threads
                    .iter()
                    .all(|thread| !thread.messages.is_empty())
            );
            for decision in &project.decisions {
                assert!(project.threads.iter().any(|thread|
                    thread.id == decision.thread_id && thread.status == "blocked"));
            }
            let services: Vec<_> = logs
                .iter()
                .filter(|service| service.project_id == project.config.id)
                .collect();
            assert_eq!(services.len(), 1);
            assert!(
                project
                    .threads
                    .iter()
                    .any(|thread| thread.id == services[0].thread_id && thread.status == "working")
            );
        }
    }
    #[test]
    fn search_and_status_filters_compose() {
        let (p, _) = load();
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
