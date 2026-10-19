//! Docs for clients: listing, reading, writing, renaming and deleting a
//! project's docs and reading its images, reporting doc changes on disk to
//! the clients watching a project, and the cards for docs an agent writes.

// Rust guideline compliant 2026-10-09

use super::{Engine, Input};
use adeline_core::{
    card::{Card, EntityRef},
    conversation::{MessageRole, ToolKind, ToolStatus},
    data::ToolReport,
    protocol::{Command, Delta, DocChange, DocFile},
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::task::block_in_place;

// Images the Viewer shows; larger ones stay links.
const IMAGE_LIMIT: u64 = 20 * 1024 * 1024;
const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];
// Larger docs don't open at all: reading one would hold up every client, as
// all commands share one task. Docs above `MAX_EDITABLE` open read-only.
const READ_LIMIT: u64 = 50 * 1024 * 1024;

fn modified(path: &Path) -> Option<u64> {
    let time = std::fs::metadata(path).ok()?.modified().ok()?;
    let since = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

fn json(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn io_error(path: &str, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("{path} no longer exists.")
    } else {
        format!("{path}: {error}")
    }
}

fn check_new(root: &Path, path: &str) -> Result<PathBuf, String> {
    adeline_docs::check_name(adeline_docs::file_name(path))?;
    let full = adeline_docs::resolve(root, path)?;
    if full.exists() {
        return Err(format!("{path} already exists."));
    }
    Ok(full)
}

fn read(root: &Path, path: &str) -> Result<DocFile, String> {
    let full = adeline_docs::resolve(root, path)?;
    let size = std::fs::metadata(&full)
        .map_err(|e| io_error(path, &e))?
        .len();
    if size > READ_LIMIT {
        return Err(format!(
            "{path} is larger than 50 MB, so it can't be opened."
        ));
    }
    let bytes = std::fs::read(&full).map_err(|e| io_error(path, &e))?;
    let (text, read_only) = adeline_docs::decode(bytes);
    Ok(DocFile {
        text,
        modified: modified(&full).unwrap_or_default(),
        read_only: read_only.map(str::to_owned),
    })
}

fn write(root: &Path, path: &str, text: &str, create: bool) -> Result<u64, String> {
    if create {
        let full = check_new(root, path)?;
        if let Some(folder) = full.parent() {
            std::fs::create_dir_all(folder).map_err(|e| io_error(path, &e))?;
        }
        adeline_core::files::write_new(&full, text.as_bytes()).map_err(|e| format!("{e:#}"))?;
        return Ok(modified(&full).unwrap_or_default());
    }
    // The link's target, so a doc that is a link inside the project stays one.
    let full = adeline_docs::resolve(root, path)?
        .canonicalize()
        .map_err(|e| io_error(path, &e))?;
    let original = std::fs::read(&full).map_err(|e| io_error(path, &e))?;
    let permissions = std::fs::metadata(&full)
        .map_err(|e| io_error(path, &e))?
        .permissions();
    if permissions.readonly() {
        return Err(format!("{path} is read-only, so it can't be saved."));
    }
    let text = adeline_docs::conform(text, &String::from_utf8_lossy(&original));
    // A new file renamed over the old one, so a full disk or a crash leaves
    // the old text whole.
    let temp = full.with_extension(adeline_core::files::unique("tmp"));
    adeline_core::files::write_new(&temp, text.as_bytes()).map_err(|e| format!("{e:#}"))?;
    let saved =
        std::fs::set_permissions(&temp, permissions).and_then(|()| std::fs::rename(&temp, &full));
    if let Err(e) = saved {
        let _ = std::fs::remove_file(&temp);
        return Err(io_error(path, &e));
    }
    Ok(modified(&full).unwrap_or_default())
}

fn rename(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let source = adeline_docs::resolve(root, from)?;
    let target = check_new(root, to)?;
    if !source.is_file() {
        return Err(format!("{from} no longer exists."));
    }
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder).map_err(|e| io_error(to, &e))?;
    }
    std::fs::rename(&source, &target).map_err(|e| io_error(from, &e))
}

fn image(root: &Path, path: &str) -> Result<String, String> {
    let extension = path.rsplit_once('.').map(|(_, ext)| ext.to_lowercase());
    if !extension.is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.as_str())) {
        return Err(format!("{path} is not an image."));
    }
    let full = adeline_docs::resolve(root, path)?;
    let size = std::fs::metadata(&full)
        .map_err(|e| io_error(path, &e))?
        .len();
    if size > IMAGE_LIMIT {
        return Err(format!("{path} is larger than 20 MB."));
    }
    let bytes = std::fs::read(&full).map_err(|e| io_error(path, &e))?;
    Ok(adeline_core::conversation::base64(&bytes))
}

impl Engine {
    fn project_root(&self, project_id: &str) -> Result<PathBuf, String> {
        self.projects
            .iter()
            .find(|project| project.config.id == project_id)
            .map(|project| project.config.directory.clone())
            .ok_or_else(|| format!("Project {project_id} no longer exists."))
    }

    pub(super) fn docs_command(&mut self, client: u64, command: Command) -> Result<Value, String> {
        let respect = self.settings.docs.respect_gitignore;
        match command {
            Command::ListDocs { project_id } => {
                let root = self.project_root(&project_id)?;
                json(block_in_place(|| adeline_docs::list(&root, respect)))
            }
            Command::ReadDoc { project_id, path } => {
                let root = self.project_root(&project_id)?;
                let result = block_in_place(|| read(&root, &path));
                if result.is_err() && !root.join(&path).exists() {
                    self.doc_cards(&project_id, &path, None);
                }
                json(result?)
            }
            Command::WriteDoc {
                project_id,
                path,
                text,
                create,
            } => {
                let root = self.project_root(&project_id)?;
                let modified = block_in_place(|| write(&root, &path, &text, create))?;
                self.doc_cards(&project_id, &path, Some(modified));
                json(modified)
            }
            Command::RenameDoc {
                project_id,
                from,
                to,
            } => {
                let root = self.project_root(&project_id)?;
                block_in_place(|| rename(&root, &from, &to))?;
                self.doc_cards(&project_id, &from, None);
                Ok(Value::Null)
            }
            Command::DeleteDoc { project_id, path } => {
                let root = self.project_root(&project_id)?;
                let full = adeline_docs::resolve(&root, &path)?;
                block_in_place(|| std::fs::remove_file(&full)).map_err(|e| io_error(&path, &e))?;
                self.doc_cards(&project_id, &path, None);
                Ok(Value::Null)
            }
            Command::ReadImage { project_id, path } => {
                let root = self.project_root(&project_id)?;
                block_in_place(|| image(&root, &path)).map(Value::String)
            }
            Command::WatchDocs { project_id } => {
                match project_id {
                    Some(id) => self.docs_watching.insert(client, id),
                    None => self.docs_watching.remove(&client),
                };
                self.watch_docs();
                Ok(Value::Null)
            }
            _ => unreachable!("docs_command only gets docs commands"),
        }
    }

    // Watches the folders of the projects some client watches.
    pub(super) fn watch_docs(&mut self) {
        use notify::Watcher as _;
        let wanted: HashSet<PathBuf> = self
            .docs_watching
            .values()
            .filter_map(|id| self.project_root(id).ok())
            .collect();
        if wanted.is_empty() {
            self.docs_watcher = None;
            self.docs_watched.clear();
            return;
        }
        if self.docs_watcher.is_none() {
            let input = self.input.clone();
            self.docs_watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if let Ok(event) = event
                        && !matches!(event.kind, notify::EventKind::Access(_))
                    {
                        let _ = input.send(Input::Docs(event.paths));
                    }
                })
                .map_err(|error| {
                    tracing::event!(
                        name: "engine.docs.watch_failed",
                        tracing::Level::WARN,
                        error.message = %error,
                        "Cannot watch project docs: {{error.message}}"
                    );
                })
                .ok();
            self.docs_watched.clear();
        }
        let Some(watcher) = &mut self.docs_watcher else {
            return;
        };
        for gone in self.docs_watched.difference(&wanted) {
            let _ = watcher.unwatch(gone);
        }
        for folder in wanted.difference(&self.docs_watched) {
            if let Err(error) = watcher.watch(folder, notify::RecursiveMode::Recursive) {
                tracing::event!(
                    name: "engine.docs.watch_failed",
                    tracing::Level::WARN,
                    file.directory = %folder.display(),
                    error.message = %error,
                    "Cannot watch {{file.directory}}: {{error.message}}"
                );
            }
        }
        self.docs_watched = wanted;
    }

    pub(super) fn docs_settled(&mut self) {
        let paths = std::mem::take(&mut self.doc_events);
        let respect = self.settings.docs.respect_gitignore;
        let watched: HashSet<String> = self.docs_watching.values().cloned().collect();
        for project_id in watched {
            let Ok(root) = self.project_root(&project_id) else {
                continue;
            };
            // macOS reports events under the folder's real path.
            let real = root.canonicalize().unwrap_or_else(|_| root.clone());
            let mut changes: Vec<DocChange> = Vec::new();
            let mut relist = false;
            for path in &paths {
                let Some(relative) = path
                    .strip_prefix(&root)
                    .or_else(|_| path.strip_prefix(&real))
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(|p| p.replace('\\', "/"))
                else {
                    continue;
                };
                if relative.is_empty() || relative.split('/').any(|part| part == ".git") {
                    continue;
                }
                if adeline_docs::has_doc_extension(&relative) {
                    if adeline_docs::is_doc(&root, &relative, respect)
                        && !changes.iter().any(|c| c.path == relative)
                    {
                        let modified = block_in_place(|| modified(path));
                        changes.push(DocChange {
                            path: relative,
                            modified,
                        });
                    }
                } else if path.is_dir()
                    || !adeline_docs::file_name(&relative).contains('.')
                    || adeline_docs::file_name(&relative) == ".gitignore"
                {
                    // A folder appeared, went or was renamed, or the ignore rules changed.
                    relist = true;
                }
            }
            for change in &changes {
                self.doc_cards(&project_id, &change.path, change.modified);
            }
            if !changes.is_empty() || relist {
                self.broadcast(Delta::Docs {
                    project_id,
                    changes,
                    relist,
                });
            }
        }
    }

    pub(super) fn doc_events(&mut self, paths: Vec<PathBuf>) {
        self.doc_events.extend(paths);
        self.docs_due = Some(Instant::now() + super::WATCH_SETTLE);
    }

    // Marks the project's cards for `path` gone, or back with a new edit time.
    fn doc_cards(&mut self, project_id: &str, path: &str, modified: Option<u64>) {
        let entity = EntityRef::doc(path);
        let updates: Vec<(String, Card)> = self
            .projects
            .iter()
            .filter(|project| project.config.id == project_id)
            .flat_map(|project| &project.threads)
            .filter_map(|thread| {
                let card = thread.cards.iter().find(|card| card.entity == entity)?;
                let updated = Card {
                    gone: modified.is_none(),
                    edited: modified.unwrap_or(card.edited),
                    ..card.clone()
                };
                (updated != *card).then(|| (thread.id.clone(), updated))
            })
            .collect();
        for (id, card) in updates {
            self.put_card(&id, card);
        }
    }

    fn put_card(&mut self, id: &str, card: Card) {
        self.queue_visible(id, "card", serde_json::to_value(&card).unwrap_or_default());
        self.broadcast(Delta::Card {
            id: id.to_owned(),
            card,
        });
    }

    // A finished edit of a doc inside the project gets a card in the turn's reply.
    pub(super) fn tool_cards(&mut self, id: &str, report: &ToolReport) {
        if !self.settings.docs.enabled
            || report.status != ToolStatus::Completed
            || !matches!(report.kind, ToolKind::Edit | ToolKind::Move)
        {
            return;
        }
        let Some(project_id) = self.project_of(id) else {
            return;
        };
        let Ok(root) = self.project_root(&project_id) else {
            return;
        };
        let respect = self.settings.docs.respect_gitignore;
        let Some(thread) = self.locate(id) else {
            return;
        };
        let existing = thread.cards.clone();
        let turn = thread
            .messages
            .iter()
            .rposition(|m| m.role == MessageRole::User);
        let cards: Vec<Card> = block_in_place(|| {
            report
                .paths
                .iter()
                .filter_map(|reported| adeline_docs::project_path(&root, reported))
                .filter(|path| adeline_docs::is_doc(&root, path, respect))
                .filter_map(|path| {
                    let edited = modified(&root.join(&path))?;
                    Some(Card {
                        entity: EntityRef::doc(&path),
                        title: adeline_docs::file_name(&path).to_owned(),
                        subtitle: path,
                        turn,
                        edited,
                        gone: false,
                    })
                })
                .collect()
        });
        for card in cards {
            let unchanged = existing.iter().any(|existing| {
                existing.entity == card.entity && existing.edited == card.edited && !existing.gone
            });
            if !unchanged {
                self.put_card(id, card);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adeline_core::files::TempDir;

    #[test]
    fn a_save_replaces_the_whole_file_and_refuses_what_it_cannot_save() {
        let root = TempDir::new("adeline-docs-write");
        std::fs::write(root.join("a.md"), "one\r\ntwo").unwrap();
        write(&root, "a.md", "one\nTWO", false).unwrap();
        assert_eq!(std::fs::read(root.join("a.md")).unwrap(), b"one\r\nTWO");
        assert_eq!(std::fs::read_dir(&*root).unwrap().count(), 1);

        let error = write(&root, "gone.md", "text", false).unwrap_err();
        assert!(error.contains("no longer exists"), "{error}");
        assert!(!root.join("gone.md").exists());

        let mut permissions = std::fs::metadata(root.join("a.md")).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(root.join("a.md"), permissions.clone()).unwrap();
        let error = write(&root, "a.md", "changed", false).unwrap_err();
        assert!(error.contains("read-only"), "{error}");
        assert_eq!(std::fs::read(root.join("a.md")).unwrap(), b"one\r\nTWO");
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "lets TempDir clean up"
        )]
        permissions.set_readonly(false);
        std::fs::set_permissions(root.join("a.md"), permissions).unwrap();
    }

    #[test]
    fn a_doc_too_large_to_read_is_refused_without_reading_it() {
        let root = TempDir::new("adeline-docs-read");
        let file = std::fs::File::create(root.join("huge.txt")).unwrap();
        file.set_len(READ_LIMIT + 1).unwrap();
        drop(file);
        let error = read(&root, "huge.txt").unwrap_err();
        assert!(error.contains("50 MB"), "{error}");
    }
}
