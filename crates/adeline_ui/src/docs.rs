//! Docs mode: the open project's docs tree, the open doc in the Viewer or the
//! Editor, and a conversation of the project beside them. Every file read,
//! write and change notice goes through the project's engine (`--demo`
//! answers from `assets/demo-docs.json` instead), so remote projects work
//! like local ones.
//!
//! Edits stay in memory until saved. A doc's buffer lives in the shared
//! Editor state while it is open, and in its `Doc` otherwise. Entering Docs
//! remembers the Chats selection and leaving restores it. What Docs shows per
//! project is kept in `ui_state.rs`.

// Rust guideline compliant 2026-10-09

use super::*;
use adeline_core::{
    card::EntityRef,
    config::DocView,
    protocol::{Command, DocChange, DocFile},
};
use adeline_docs::{Format, Row, Tree};
use gpui_kit::base::text::TextViewState;
use gpui_kit::component::input::{EditorState, Redo, Undo};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) mod mermaid;
mod view;
mod viewer;

// Viewer renderers can't reach `Adeline`, so they dispatch these.
#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = adeline, no_json)]
pub(crate) struct ToggleTask {
    pub offset: usize,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = adeline, no_json)]
pub(crate) struct NoteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = adeline, no_json)]
pub(crate) struct OpenLink {
    pub url: SharedString,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = adeline, no_json)]
pub(crate) struct ApplyFormat {
    pub format: Format,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = adeline, no_json)]
pub(crate) struct SwitchMode {
    pub index: usize,
}

actions!(
    adeline,
    [
        ToggleDocView,
        SaveDoc,
        NoteOnSelection,
        NewDoc,
        ChooseChat,
        FindDoc
    ]
);

/// Docs mode's shortcuts (docs/shortcuts.md), Ctrl and Cmd alike.
pub(crate) fn key_bindings() -> Vec<KeyBinding> {
    let mut bindings = Vec::new();
    for modifier in ["ctrl", "cmd"] {
        let key = |rest: &str| format!("{modifier}-{rest}");
        for index in 0..9 {
            bindings.push(KeyBinding::new(
                &key(&(index + 1).to_string()),
                SwitchMode { index },
                None,
            ));
        }
        bindings.extend([
            KeyBinding::new(&key("shift-v"), ToggleDocView, None),
            KeyBinding::new(&key("s"), SaveDoc, None),
            KeyBinding::new(&key("shift-k"), ChooseChat, None),
            KeyBinding::new(&key("shift-n"), NoteOnSelection, None),
            KeyBinding::new(&key("alt-n"), NewDoc, None),
            KeyBinding::new(&key("p"), FindDoc, None),
        ]);
        let editor = Some("DocsEditor");
        let format = |keys: &str, format: Format| {
            KeyBinding::new(&key(keys), ApplyFormat { format }, editor)
        };
        bindings.extend([
            format("b", Format::Bold),
            format("i", Format::Italic),
            format("shift-x", Format::Strikethrough),
            format("alt-c", Format::InlineCode),
            format("alt-b", Format::CodeBlock),
            format("k", Format::Link),
            format("shift-8", Format::BulletList),
            format("shift-7", Format::NumberedList),
            format("shift-9", Format::TaskList),
            format("shift-.", Format::Quote),
            format("alt-t", Format::Table),
            format("alt-1", Format::Heading(1)),
            format("alt-2", Format::Heading(2)),
            format("alt-3", Format::Heading(3)),
        ]);
    }
    bindings
}

#[derive(Clone, Debug)]
pub(crate) enum Act {
    Open(String),
    Folder(String),
    View,
    Save,
    New,
    Create,
    Rename(Option<String>),
    ConfirmRename,
    Delete(Option<String>),
    ConfirmDelete,
    Format(Format),
    Undo,
    Redo,
    Find,
    Note,
    KeepMine,
    TakeTheirs,
    CloseDoc,
    LeftPanel,
    ChatPanel,
    Gitignore,
    OpenIn,
    RemoteImages,
    Chooser,
    Chat(Option<String>),
    More(String),
    SaveAll,
    Discard,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Then {
    Quit,
    Close(usize),
}

#[derive(Debug, Default)]
pub(crate) struct Doc {
    saved: String,
    // The buffer while the doc isn't in the Editor; `None` means `saved`.
    text: Option<String>,
    dirty: bool,
    modified: u64,
    read_only: Option<String>,
    editor: bool,
    // The text on disk, changed while this doc had unsaved edits.
    conflict: Option<String>,
    gone: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Project {
    machine: String,
    id: String,
    tree: Tree,
    rows: Vec<Row>,
    listed: bool,
    list_error: Option<String>,
    expanded: HashSet<String>,
    open: Option<String>,
    // A doc that couldn't open, and why.
    failed: Option<(String, String)>,
    files: HashMap<String, Doc>,
    chat: Option<String>,
    tree_width: f32,
    chat_width: f32,
    scroll: Option<(usize, f32)>,
    cursor: Option<String>,
}

pub(crate) struct Docs {
    pub(super) search: Entity<InputState>,
    pub(super) editor: Entity<EditorState>,
    viewer: Entity<TextViewState>,
    panels: Entity<component::ResizableState>,
    path_input: Entity<InputState>,
    projects: HashMap<String, Project>,
    images: viewer::Images,
    // Where the note button shows, and the source range it notes.
    note_at: Option<(Point<Pixels>, std::ops::Range<usize>)>,
    // The doc a dialog acts on, and what follows the unsaved-docs prompt.
    target: Option<String>,
    then: Option<Then>,
    unsaved_ok: bool,
    // While Docs shows, the Chats mode selection to restore.
    chats_selection: Option<String>,
    tree_focus: FocusHandle,
    tree_scroll: UniformListScrollHandle,
    shown: Option<(String, String)>,
    more: HashSet<String>,
    demo: HashMap<String, BTreeMap<String, String>>,
    pending_anchor: Option<String>,
}

const TREE_WIDTH: f32 = 260.;
const CHAT_WIDTH: f32 = 380.;

fn language(path: &str) -> &'static str {
    match path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .as_deref()
    {
        Some("md" | "markdown") => "markdown",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        _ => "text",
    }
}

fn json(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// The note a selection of `text` at `range` makes, quoting it with the doc's path and lines.
fn note(path: &str, text: &str, range: std::ops::Range<usize>) -> String {
    let quoted = text[range.clone()].trim_end_matches(['\n', '\r']);
    let first = text[..range.start].matches('\n').count() + 1;
    let last = first + quoted.matches('\n').count();
    let lines = if first == last {
        format!("line {first}")
    } else {
        format!("lines {first}–{last}")
    };
    let mut draft = format!("`{path}`, {lines}:\n\n");
    for line in quoted.lines() {
        draft.push_str("> ");
        draft.push_str(line);
        draft.push('\n');
    }
    draft.push('\n');
    draft
}

impl Docs {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Adeline>) -> Self {
        Self {
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search docs")),
            editor: cx.new(|cx| EditorState::new(window, cx).language("markdown")),
            viewer: cx.new(|cx| TextViewState::markdown("", cx)),
            panels: cx.new(|_| component::ResizableState::default()),
            path_input: cx.new(|cx| InputState::new(window, cx)),
            projects: HashMap::new(),
            images: viewer::Images::default(),
            note_at: None,
            target: None,
            then: None,
            unsaved_ok: false,
            chats_selection: None,
            tree_focus: cx.focus_handle().tab_stop(true),
            tree_scroll: UniformListScrollHandle::new(),
            shown: None,
            more: HashSet::new(),
            demo: HashMap::new(),
            pending_anchor: None,
        }
    }

    pub(super) fn unsaved(&self) -> Vec<(String, String)> {
        let mut unsaved: Vec<_> = self
            .projects
            .iter()
            .flat_map(|(key, project)| {
                project
                    .files
                    .iter()
                    .filter(|(_, doc)| doc.dirty)
                    .map(move |(path, _)| (key.clone(), path.clone()))
            })
            .collect();
        unsaved.sort();
        unsaved
    }

    pub(super) fn more_open(&self, key: &str) -> bool {
        self.more.contains(key)
    }
}

impl Project {
    fn new(machine: &str, id: &str) -> Self {
        let saved = ui_state::docs(machine, id);
        Self {
            machine: machine.to_owned(),
            id: id.to_owned(),
            expanded: saved.expanded.into_iter().collect(),
            open: saved.open.clone(),
            chat: saved.chat,
            tree_width: saved.tree_width.unwrap_or(TREE_WIDTH),
            chat_width: saved.chat_width.unwrap_or(CHAT_WIDTH),
            scroll: saved.open.is_some().then_some(saved.scroll),
            files: saved
                .open
                .map(|path| {
                    (
                        path,
                        Doc {
                            editor: saved.editor,
                            ..Doc::default()
                        },
                    )
                })
                .into_iter()
                .collect(),
            ..Self::default()
        }
    }

    fn set_paths(&mut self, paths: Vec<String>) {
        self.tree = Tree::new(paths);
        self.listed = true;
    }

    fn put_path(&mut self, path: &str, present: bool) {
        let mut paths = self.tree.paths().to_vec();
        let had = paths.iter().position(|p| p == path);
        match (had, present) {
            (None, true) => paths.push(path.to_owned()),
            (Some(at), false) => {
                paths.remove(at);
            }
            _ => return,
        }
        paths.sort();
        self.tree = Tree::new(paths);
    }
}

impl Adeline {
    // A project made later with the same ID lists its docs afresh.
    pub(super) fn docs_forget(&mut self, key: &str) {
        self.docs.projects.remove(key);
        if self.docs.shown.as_ref().is_some_and(|(k, _)| k == key) {
            self.docs.shown = None;
        }
    }

    fn docs_key(&self) -> String {
        self.workspace().key()
    }

    fn docs_project(&mut self) -> &mut Project {
        let key = self.docs_key();
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        self.docs
            .projects
            .entry(key)
            .or_insert_with(|| Project::new(&machine, &id))
    }

    fn docs_current(&self) -> Option<&Project> {
        self.docs.projects.get(&self.docs_key())
    }

    pub(super) fn docs_open_doc(&self) -> Option<(&str, &Doc)> {
        let project = self.docs_current()?;
        let path = project.open.as_deref()?;
        Some((path, project.files.get(path)?))
    }

    fn docs_text(&self, cx: &App) -> String {
        self.docs.editor.read(cx).value().to_string()
    }

    fn docs_request(
        &mut self,
        machine: &str,
        project_id: &str,
        command: Command,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Value, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.demo_mode {
            self.machine_request(machine, command, cx, done);
            return;
        }
        let result = self.demo_docs(project_id, command);
        let owner = cx.weak_entity();
        let handle = self.main_window;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = owner.update(cx, |app, cx| done(app, result, window, cx));
            });
        });
    }

    fn demo_docs(&mut self, project_id: &str, command: Command) -> Result<Value, String> {
        let files = self
            .docs
            .demo
            .entry(project_id.to_owned())
            .or_insert_with(|| {
                embedded("demo-docs.json")
                    .and_then(|bytes| {
                        serde_json::from_slice::<HashMap<String, BTreeMap<String, String>>>(bytes)
                            .ok()
                    })
                    .and_then(|mut all| all.remove(project_id))
                    .unwrap_or_default()
            });
        let gone = |path: &str| format!("{path} no longer exists.");
        match command {
            Command::ListDocs { .. } => json(files.keys().collect::<Vec<_>>()),
            Command::ReadDoc { path, .. } => {
                let text = files.get(&path).cloned().ok_or_else(|| gone(&path))?;
                json(DocFile {
                    text,
                    modified: 1,
                    read_only: None,
                })
            }
            Command::WriteDoc {
                path, text, create, ..
            } => {
                if create {
                    adeline_docs::check_name(adeline_docs::file_name(&path))?;
                    if files.contains_key(&path) {
                        return Err(format!("{path} already exists."));
                    }
                }
                let text =
                    adeline_docs::conform(&text, files.get(&path).map_or("", String::as_str));
                files.insert(path, text);
                Ok(Value::from(1))
            }
            Command::RenameDoc { from, to, .. } => {
                adeline_docs::check_name(adeline_docs::file_name(&to))?;
                if files.contains_key(&to) {
                    return Err(format!("{to} already exists."));
                }
                let text = files.remove(&from).ok_or_else(|| gone(&from))?;
                files.insert(to, text);
                Ok(Value::Null)
            }
            Command::DeleteDoc { path, .. } => files
                .remove(&path)
                .map(|_| Value::Null)
                .ok_or_else(|| gone(&path)),
            Command::ReadImage { path, .. } => Err(format!("{path} is not in the demo.")),
            _ => Ok(Value::Null),
        }
    }

    /// Shows Docs for the open project, with `chat` on the right when a card opened it.
    pub(super) fn docs_enter(
        &mut self,
        chat: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_open_project() {
            return;
        }
        if self.section != Section::Docs {
            self.docs.chats_selection = self.current_id();
        }
        self.section = Section::Docs;
        let current = self.current_id();
        let project = self.docs_project();
        let chat = chat.or_else(|| project.chat.clone()).or(current);
        let listed = project.listed;
        self.docs_select_chat(chat, window, cx);
        if !listed {
            self.docs_list(cx);
        }
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        self.docs_request(
            &machine,
            &id,
            Command::WatchDocs {
                project_id: Some(id.clone()),
            },
            cx,
            |_, _, _, _| {},
        );
        if let Some(path) = self.docs_project().open.clone() {
            self.docs_open(path, window, cx);
        }
        self.docs_refresh_rows(cx);
    }

    /// Leaves Docs: keeps unsaved edits, remembers what showed and restores the Chats selection.
    pub(super) fn docs_leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != Section::Docs {
            return;
        }
        self.docs_stash(cx);
        let current = self.current_id();
        if self.has_open_project() {
            self.docs_project().chat = current;
            self.docs_record(cx);
            let (machine, id) = (
                self.workspace().machine.clone(),
                self.workspace().config.id.clone(),
            );
            self.docs_request(
                &machine,
                &id,
                Command::WatchDocs { project_id: None },
                cx,
                |_, _, _, _| {},
            );
        }
        self.docs.note_at = None;
        let previous = self.docs.chats_selection.take();
        self.docs_select_chat(previous, window, cx);
    }

    pub(super) fn docs_select_chat(
        &mut self,
        id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found = id.and_then(|id| self.workspace().threads.iter().position(|t| t.id == id));
        if let Some(ix) = found {
            if self.selected != Some(ix) {
                self.act(Action::Chat(ix), window, cx);
            }
        } else {
            self.selected = None;
        }
        self.resync_transcript(cx);
    }

    fn docs_list(&mut self, cx: &mut Context<Self>) {
        let key = self.docs_key();
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        self.docs_request(
            &machine,
            &id,
            Command::ListDocs {
                project_id: id.clone(),
            },
            cx,
            move |app, result, _, cx| {
                let Some(project) = app.docs.projects.get_mut(&key) else {
                    return;
                };
                match result.and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string())) {
                    Ok(paths) => {
                        project.set_paths(paths);
                        project.list_error = None;
                    }
                    Err(error) => project.list_error = Some(error),
                }
                app.docs_refresh_rows(cx);
            },
        );
    }

    pub(super) fn docs_refresh_rows(&mut self, cx: &mut Context<Self>) {
        let query = self.docs.search.read(cx).value().trim().to_owned();
        if let Some(project) = self.docs.projects.get_mut(&self.workspace().key()) {
            project.rows = if query.is_empty() {
                project.tree.rows(&project.expanded)
            } else {
                project.tree.search(&query)
            };
        }
        cx.notify();
    }

    // Keeps the Editor's buffer and scroll with the doc before another doc shows.
    fn docs_stash(&mut self, cx: &mut Context<Self>) {
        let Some((key, path)) = self.docs.shown.take() else {
            return;
        };
        let text = self.docs_text(cx);
        let editor_scroll = self.docs.editor.read(cx).scroll_offset().y;
        let top = self.docs.viewer.read(cx).list_state().logical_scroll_top();
        let Some(project) = self.docs.projects.get_mut(&key) else {
            return;
        };
        let Some(doc) = project.files.get_mut(&path) else {
            return;
        };
        doc.text = doc.dirty.then_some(text);
        project.scroll = Some(if doc.editor {
            (0, f32::from(-editor_scroll))
        } else {
            (top.item_ix, f32::from(top.offset_in_item))
        });
    }

    pub(super) fn docs_open(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.docs_key();
        if self
            .docs
            .shown
            .as_ref()
            .is_some_and(|(k, p)| *k == key && *p == path)
        {
            return;
        }
        self.docs_stash(cx);
        self.docs.note_at = None;
        let project = self.docs_project();
        project.open = Some(path.clone());
        project.failed = None;
        project.cursor = Some(path.clone());
        if project.files.get(&path).is_some_and(|doc| doc.modified > 0) {
            self.docs_show(window, cx);
            return;
        }
        let (machine, id) = (project.machine.clone(), project.id.clone());
        self.docs_request(
            &machine,
            &id,
            Command::ReadDoc {
                project_id: id.clone(),
                path: path.clone(),
            },
            cx,
            move |app, result, window, cx| {
                let Some(project) = app.docs.projects.get_mut(&key) else {
                    return;
                };
                match result
                    .and_then(|v| serde_json::from_value::<DocFile>(v).map_err(|e| e.to_string()))
                {
                    Ok(file) => {
                        let editor = project.files.get(&path).map_or(
                            config::with(|s| s.modes.docs.open_in == DocView::Editor),
                            |doc| doc.editor,
                        );
                        project.files.insert(
                            path.clone(),
                            Doc {
                                saved: file.text,
                                modified: file.modified.max(1),
                                read_only: file.read_only,
                                editor,
                                ..Doc::default()
                            },
                        );
                    }
                    Err(error) => {
                        project.files.remove(&path);
                        if project.open.as_deref() == Some(&path) {
                            project.failed = Some((path.clone(), error));
                        }
                    }
                }
                if project.open.as_deref() == Some(&path) && app.docs_key() == key {
                    app.docs_show(window, cx);
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn docs_show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.docs_key();
        let Some(project) = self.docs.projects.get_mut(&key) else {
            return;
        };
        let Some(path) = project.open.clone() else {
            return;
        };
        let Some(doc) = project.files.get(&path) else {
            return;
        };
        let text = doc.text.clone().unwrap_or_else(|| doc.saved.clone());
        let scroll = project.scroll.take();
        let editor_view = doc.editor;
        self.docs.editor.update(cx, |state, cx| {
            state.set_highlighter(language(&path), cx);
            state.set_value(text.clone(), window, cx);
            if let (Some((_, offset)), true) = (scroll, editor_view) {
                state.set_scroll_offset(point(px(0.), px(-offset)), cx);
            }
        });
        if adeline_docs::is_markdown(&path) {
            self.docs
                .viewer
                .update(cx, |state, cx| state.set_text(&text, cx));
            if let (Some((item_ix, offset)), false) = (scroll, editor_view) {
                self.docs
                    .viewer
                    .read(cx)
                    .list_state()
                    .scroll_to(ListOffset {
                        item_ix,
                        offset_in_item: px(offset),
                    });
            }
        }
        self.docs.shown = Some((key, path));
        if let Some(anchor) = self.docs.pending_anchor.take() {
            self.docs_anchor(&anchor, cx);
        }
        cx.notify();
    }

    pub(super) fn docs_edited(&mut self, cx: &mut Context<Self>) {
        let Some((key, path)) = self.docs.shown.clone() else {
            return;
        };
        if let Some(doc) = self
            .docs
            .projects
            .get_mut(&key)
            .and_then(|project| project.files.get_mut(&path))
            && !doc.dirty
        {
            doc.dirty = true;
            cx.notify();
        }
    }

    fn docs_set_view(&mut self, editor: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.docs_text(cx);
        let Some((key, path)) = self.docs.shown.clone() else {
            return;
        };
        if let Some(doc) = self
            .docs
            .projects
            .get_mut(&key)
            .and_then(|project| project.files.get_mut(&path))
        {
            doc.editor = editor;
        }
        if editor {
            window.focus(&self.docs.editor.focus_handle(cx), cx);
        } else {
            // The hidden Editor had focus; shortcuts need a focused element.
            window.focus(&self.focus, cx);
            if adeline_docs::is_markdown(&path) {
                self.docs
                    .viewer
                    .update(cx, |state, cx| state.set_text(&text, cx));
            }
        }
        self.docs.note_at = None;
        self.docs_record(cx);
    }

    // Writes `path` of the project at `key`, then runs `after` with whether it saved.
    fn docs_save(
        &mut self,
        key: &str,
        path: &str,
        cx: &mut Context<Self>,
        after: impl FnOnce(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let shown = self
            .docs
            .shown
            .as_ref()
            .is_some_and(|(k, p)| k == key && p == path);
        let text = if shown {
            self.docs_text(cx)
        } else {
            let Some(doc) = self.docs.projects.get(key).and_then(|p| p.files.get(path)) else {
                return;
            };
            doc.text.clone().unwrap_or_else(|| doc.saved.clone())
        };
        let Some(project) = self.docs.projects.get(key) else {
            return;
        };
        if let Some(reason) = project.files.get(path).and_then(|d| d.read_only.clone()) {
            self.notify_toast(&reason, cx);
            return;
        }
        let (machine, id) = (project.machine.clone(), project.id.clone());
        let (key, path) = (key.to_owned(), path.to_owned());
        self.docs_request(
            &machine,
            &id.clone(),
            Command::WriteDoc {
                project_id: id,
                path: path.clone(),
                text: text.clone(),
                create: false,
            },
            cx,
            move |app, result, window, cx| {
                let saved = match result {
                    Ok(modified) => {
                        if let Some(doc) = app
                            .docs
                            .projects
                            .get_mut(&key)
                            .and_then(|project| project.files.get_mut(&path))
                        {
                            doc.saved = text;
                            doc.text = None;
                            doc.dirty = false;
                            doc.gone = false;
                            doc.conflict = None;
                            doc.modified = modified.as_u64().unwrap_or(doc.modified).max(1);
                        }
                        if let Some(project) = app.docs.projects.get_mut(&key) {
                            project.put_path(&path, true);
                        }
                        app.docs_refresh_rows(cx);
                        true
                    }
                    Err(error) => {
                        app.notify_toast(&format!("Could not save {path}: {error}"), cx);
                        false
                    }
                };
                after(app, saved, window, cx);
                cx.notify();
            },
        );
    }

    // A doc changed on disk: reload it, or ask when it has unsaved edits.
    pub(super) fn docs_changed(
        &mut self,
        machine: &str,
        project_id: &str,
        changes: Vec<DocChange>,
        relist: bool,
        cx: &mut Context<Self>,
    ) {
        let key = format!("{machine}:{project_id}");
        let Some(project) = self.docs.projects.get_mut(&key) else {
            return;
        };
        let mut reload = Vec::new();
        for change in changes {
            project.put_path(&change.path, change.modified.is_some());
            let Some(doc) = project.files.get_mut(&change.path) else {
                continue;
            };
            match change.modified {
                None => doc.gone = doc.modified > 0,
                Some(modified) if modified != doc.modified && doc.modified > 0 => {
                    doc.gone = false;
                    reload.push(change.path);
                }
                Some(_) => doc.gone = false,
            }
        }
        if relist && self.docs_key() == key {
            self.docs_list(cx);
        }
        for path in reload {
            self.docs_reload(&key, path, cx);
        }
        self.docs_refresh_rows(cx);
    }

    fn docs_reload(&mut self, key: &str, path: String, cx: &mut Context<Self>) {
        let Some(project) = self.docs.projects.get(key) else {
            return;
        };
        let (machine, id, key) = (project.machine.clone(), project.id.clone(), key.to_owned());
        self.docs_request(
            &machine,
            &id.clone(),
            Command::ReadDoc {
                project_id: id,
                path: path.clone(),
            },
            cx,
            move |app, result, window, cx| {
                let Ok(file) = result
                    .and_then(|v| serde_json::from_value::<DocFile>(v).map_err(|e| e.to_string()))
                else {
                    return;
                };
                let shown = app
                    .docs
                    .shown
                    .as_ref()
                    .is_some_and(|(k, p)| *k == key && *p == path);
                let Some(doc) = app
                    .docs
                    .projects
                    .get_mut(&key)
                    .and_then(|project| project.files.get_mut(&path))
                else {
                    return;
                };
                if file.modified == doc.modified {
                    return;
                }
                doc.modified = file.modified.max(1);
                doc.read_only = file.read_only;
                if doc.dirty {
                    doc.conflict = Some(file.text);
                } else {
                    doc.saved = file.text;
                    doc.text = None;
                    if shown {
                        app.docs.shown = None;
                        app.docs_show(window, cx);
                    }
                }
                cx.notify();
            },
        );
    }

    fn docs_resolve(&mut self, keep_mine: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((key, path)) = self.docs.shown.clone() else {
            return;
        };
        let Some(doc) = self
            .docs
            .projects
            .get_mut(&key)
            .and_then(|project| project.files.get_mut(&path))
        else {
            return;
        };
        let Some(disk) = doc.conflict.take() else {
            return;
        };
        doc.saved = disk;
        if !keep_mine {
            doc.dirty = false;
            doc.text = None;
            self.docs.shown = None;
            self.docs_show(window, cx);
        }
    }

    fn docs_note(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((path, doc)) = self.docs_open_doc() else {
            return;
        };
        let path = path.to_owned();
        let viewer = !doc.editor && adeline_docs::is_markdown(&path);
        let text = self.docs_text(cx);
        let range = range.or_else(|| {
            let range = if viewer {
                self.docs.viewer.read(cx).selected_source_range()?
            } else {
                self.docs.editor.read(cx).selected_range()
            };
            (!range.is_empty()).then_some(range)
        });
        let Some(range) = range.filter(|r| {
            r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end)
        }) else {
            window.push_notification("Select some text in the doc first.", cx);
            return;
        };
        let draft = note(&path, &text, range);
        self.docs.note_at = None;
        if !config::with(|s| s.modes.docs.show_conversation_panel) {
            self.act(Action::Docs(Act::ChatPanel), window, cx);
        }
        self.composer.update(cx, |state, cx| {
            let lead = if state.value().trim().is_empty() {
                ""
            } else {
                "\n\n"
            };
            state.insert(format!("{lead}{draft}"), window, cx);
        });
        window.focus(&self.composer.focus_handle(cx), cx);
        self.composer_region.update(cx, |_, cx| cx.notify());
    }

    fn docs_toggle_task(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.docs_text(cx);
        let Some(mark) = text.get(offset..offset + 3) else {
            return;
        };
        let replacement = match mark {
            "[ ]" => "[x]",
            "[x]" | "[X]" => "[ ]",
            _ => return,
        };
        let clean = self.docs_open_doc().is_some_and(|(_, doc)| !doc.dirty);
        self.docs.editor.update(cx, |state, cx| {
            state.set_selected_range(offset..offset + 3, cx);
            state.replace(replacement, window, cx);
        });
        let text = self.docs_text(cx);
        self.docs
            .viewer
            .update(cx, |state, cx| state.set_text(&text, cx));
        if clean && let Some((key, path)) = self.docs.shown.clone() {
            self.docs_save(&key, &path, cx, |_, _, _, _| {});
        }
    }

    fn docs_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) {
        let text = self.docs_text(cx);
        let heading = text
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix('#'))
            .map(|rest| rest.trim_start_matches('#').trim())
            .find(|heading| slug(heading) == anchor);
        let Some(heading) = heading else {
            return;
        };
        let rendered = self.docs.viewer.read(cx).rendered_text();
        if let Some(at) = rendered.as_str().find(heading) {
            let _ = self.docs.viewer.update(cx, |state, cx| {
                state.reveal_range(at..at + heading.len(), cx)
            });
        }
    }

    fn docs_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
            cx.open_url(url);
            return;
        }
        if let Some(route) = EntityRef::parse(url) {
            self.open_route(&route, None, window, cx);
            return;
        }
        if let Some(anchor) = url.strip_prefix('#') {
            self.docs_anchor(anchor, cx);
            return;
        }
        let Some(from) = self.docs_open_doc().map(|(path, _)| path.to_owned()) else {
            return;
        };
        let (target, anchor) = url.split_once('#').unwrap_or((url, ""));
        let Some(path) = adeline_docs::join(&from, target) else {
            window.push_notification(format!("{url} is outside the project."), cx);
            return;
        };
        if adeline_docs::has_doc_extension(&path) {
            self.docs.pending_anchor = (!anchor.is_empty()).then(|| anchor.to_owned());
            self.docs_open(path, window, cx);
        } else {
            self.open_project_file(&path, window, cx);
        }
    }

    fn open_project_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace().machine == machines::LOCAL && !self.demo_mode {
            cx.open_with_system(&self.workspace().config.directory.join(path));
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(path.to_owned()));
            window.push_notification(format!("Copied the path of {path}"), cx);
        }
    }

    /// Opens an `adeline://` route from a card or a link, falling back to the file when its mode is off.
    pub(super) fn open_route(
        &mut self,
        route: &EntityRef,
        chat: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if route.mode() != "docs" {
            return;
        }
        let path = route.path().to_owned();
        if !config::with(|s| s.general.features.docs) {
            self.open_project_file(&path, window, cx);
            return;
        }
        if self.section == Section::Docs {
            if chat.is_some() {
                self.docs_select_chat(chat, window, cx);
            }
        } else {
            self.docs_enter(chat, window, cx);
        }
        self.docs_open(path, window, cx);
    }

    fn docs_record(&mut self, cx: &mut Context<Self>) {
        if !self.has_open_project() {
            return;
        }
        let current = self.current_id();
        let in_docs = self.section == Section::Docs;
        let Some(project) = self.docs.projects.get(&self.docs_key()) else {
            return;
        };
        let mut expanded: Vec<String> = project.expanded.iter().cloned().collect();
        expanded.sort();
        let open = project.open.clone();
        let editor = open
            .as_ref()
            .and_then(|path| project.files.get(path))
            .is_some_and(|doc| doc.editor);
        let state = ui_state::Docs {
            open,
            editor,
            scroll: project.scroll.unwrap_or_default(),
            tree_width: Some(project.tree_width),
            chat_width: Some(project.chat_width),
            expanded,
            chat: if in_docs {
                current
            } else {
                project.chat.clone()
            },
        };
        if let Err(error) = ui_state::record_docs(&project.machine, &project.id, state) {
            self.notify_toast(&format!("Could not save Docs state: {error:#}"), cx);
        }
    }

    fn docs_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut path = self.docs.path_input.read(cx).value().trim().to_owned();
        if !adeline_docs::has_doc_extension(&path) && !path.ends_with('/') && !path.is_empty() {
            path.push_str(".md");
        }
        let key = self.docs_key();
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        self.docs_request(
            &machine,
            &id.clone(),
            Command::WriteDoc {
                project_id: id,
                path: path.clone(),
                text: String::new(),
                create: true,
            },
            cx,
            move |app, result, window, cx| match result {
                Ok(_) => {
                    app.modal = None;
                    window.close_dialog(cx);
                    if let Some(project) = app.docs.projects.get_mut(&key) {
                        project.put_path(&path, true);
                        let parent = adeline_docs::parent(&path);
                        if !parent.is_empty() {
                            let mut folder = String::new();
                            for part in parent.split('/') {
                                if !folder.is_empty() {
                                    folder.push('/');
                                }
                                folder.push_str(part);
                                project.expanded.insert(folder.clone());
                            }
                        }
                        project.files.insert(
                            path.clone(),
                            Doc {
                                editor: true,
                                ..Doc::default()
                            },
                        );
                    }
                    app.docs_refresh_rows(cx);
                    app.docs_open(path, window, cx);
                    window.focus(&app.docs.editor.focus_handle(cx), cx);
                }
                Err(error) => window.push_notification(error, cx),
            },
        );
        let _ = window;
    }

    fn docs_rename(&mut self, cx: &mut Context<Self>) {
        let Some(from) = self.docs.target.clone() else {
            return;
        };
        let name = self.docs.path_input.read(cx).value().trim().to_owned();
        let folder = adeline_docs::parent(&from);
        let to = if folder.is_empty() {
            name
        } else {
            format!("{folder}/{name}")
        };
        if to == from {
            self.modal = None;
            return;
        }
        let key = self.docs_key();
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        self.docs_stash(cx);
        self.docs_request(
            &machine,
            &id.clone(),
            Command::RenameDoc {
                project_id: id,
                from: from.clone(),
                to: to.clone(),
            },
            cx,
            move |app, result, window, cx| match result {
                Ok(_) => {
                    app.modal = None;
                    window.close_dialog(cx);
                    if let Some(project) = app.docs.projects.get_mut(&key) {
                        project.put_path(&from, false);
                        project.put_path(&to, true);
                        if let Some(doc) = project.files.remove(&from) {
                            project.files.insert(to.clone(), doc);
                        }
                        if project.open.as_deref() == Some(&from) {
                            project.open = Some(to.clone());
                        }
                        project.cursor = Some(to.clone());
                    }
                    app.docs.shown = None;
                    if app.docs_key() == key {
                        app.docs_show(window, cx);
                    }
                    app.docs_refresh_rows(cx);
                }
                Err(error) => window.push_notification(error, cx),
            },
        );
    }

    fn docs_delete(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.docs.target.clone() else {
            return;
        };
        let key = self.docs_key();
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        self.docs_request(
            &machine,
            &id.clone(),
            Command::DeleteDoc {
                project_id: id,
                path: path.clone(),
            },
            cx,
            move |app, result, window, cx| match result {
                Ok(_) => {
                    app.modal = None;
                    window.close_dialog(cx);
                    if let Some(project) = app.docs.projects.get_mut(&key) {
                        project.put_path(&path, false);
                        project.files.remove(&path);
                        if project.open.as_deref() == Some(&path) {
                            project.open = None;
                        }
                    }
                    if app.docs.shown.as_ref().is_some_and(|(_, p)| *p == path) {
                        app.docs.shown = None;
                    }
                    app.docs_refresh_rows(cx);
                }
                Err(error) => window.push_notification(error, cx),
            },
        );
    }

    fn docs_format(&mut self, format: Format, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, doc)) = self.docs_open_doc() else {
            return;
        };
        if !doc.editor || !adeline_docs::is_markdown(path) || doc.read_only.is_some() {
            return;
        }
        self.docs.editor.update(cx, |state, cx| {
            let text = state.value().to_string();
            let edit = adeline_docs::apply(format, &text, state.selected_range());
            state.set_selected_range(edit.range, cx);
            state.replace(edit.text, window, cx);
            state.set_selected_range(edit.selection, cx);
        });
        window.focus(&self.docs.editor.focus_handle(cx), cx);
    }

    // Saves every unsaved doc, then does what the unsaved-docs prompt held back.
    fn docs_save_all(&mut self, cx: &mut Context<Self>) {
        let unsaved = self.docs.unsaved();
        let remaining = std::rc::Rc::new(std::cell::Cell::new(unsaved.len()));
        let failed = std::rc::Rc::new(std::cell::Cell::new(false));
        if unsaved.is_empty() {
            let owner = cx.weak_entity();
            let handle = self.main_window;
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = owner.update(cx, |app, cx| app.docs_continue(window, cx));
                });
            });
            return;
        }
        for (key, path) in unsaved {
            let remaining = std::rc::Rc::clone(&remaining);
            let failed = std::rc::Rc::clone(&failed);
            self.docs_save(&key, &path, cx, move |app, saved, window, cx| {
                failed.set(failed.get() || !saved);
                remaining.set(remaining.get() - 1);
                if remaining.get() == 0 && !failed.get() {
                    app.docs_continue(window, cx);
                }
            });
        }
    }

    fn docs_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let closing = match self.docs.then {
            Some(Then::Close(ix)) => Some(self.projects[ix].key()),
            _ => None,
        };
        for (key, project) in &mut self.docs.projects {
            if closing.as_ref().is_some_and(|closing| closing != key) {
                continue;
            }
            for doc in project.files.values_mut() {
                doc.dirty = false;
                doc.text = None;
            }
        }
        self.docs.shown = None;
        self.docs_continue(window, cx);
    }

    fn docs_continue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let then = self.docs.then.take();
        self.modal = None;
        window.close_dialog(cx);
        match then {
            Some(Then::Quit) => {
                self.docs.unsaved_ok = true;
                let root = self.main_window;
                let owner = cx.weak_entity();
                cx.defer(move |cx| interaction::request_close(root, owner, cx));
            }
            Some(Then::Close(ix)) => {
                self.docs.unsaved_ok = true;
                self.act(Action::CloseProject(ix), window, cx);
                self.docs.unsaved_ok = false;
            }
            None => {}
        }
        cx.notify();
    }

    pub(super) fn docs_focus_path(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.docs.path_input.focus_handle(cx), cx);
    }

    /// Forgets what the unsaved-docs prompt was asked for once it is cancelled.
    pub(super) fn docs_cancel(&mut self) {
        self.docs.then = None;
    }

    /// Whether closing the window or project `close` can go ahead without losing edits.
    pub(super) fn docs_may_close(&mut self, then: Then, cx: &mut Context<Self>) -> bool {
        if self.docs.unsaved_ok {
            return true;
        }
        self.docs_stash(cx);
        let unsaved = self.docs.unsaved();
        let affected = match then {
            Then::Quit => !unsaved.is_empty(),
            Then::Close(ix) => {
                let key = self.projects[ix].key();
                unsaved.iter().any(|(k, _)| *k == key)
            }
        };
        if affected {
            self.docs.then = Some(then);
            self.modal = Some("doc-unsaved");
            self.docs_show_again(cx);
        }
        !affected
    }

    fn docs_show_again(&mut self, cx: &mut Context<Self>) {
        // `docs_stash` emptied the Editor's slot; the next `docs_show` reloads it.
        let owner = cx.weak_entity();
        let handle = self.main_window;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if app.section == Section::Docs {
                        app.docs_show(window, cx);
                    }
                });
            });
        });
    }

    /// Re-watches and relists a machine's docs after its engine connection is back.
    pub(super) fn docs_connected(&mut self, machine: &str, cx: &mut Context<Self>) {
        self.docs_sync_engine(machine, cx);
        if self.section != Section::Docs
            || !self.has_open_project()
            || self.workspace().machine != machine
        {
            return;
        }
        let id = self.workspace().config.id.clone();
        self.docs_request(
            machine,
            &id.clone(),
            Command::WatchDocs {
                project_id: Some(id),
            },
            cx,
            |_, _, _, _| {},
        );
        self.docs_list(cx);
    }

    /// Tells the machine's engine the Docs settings that decide which files get cards.
    pub(super) fn docs_sync_engine(&self, machine: &str, cx: &mut Context<Self>) {
        if self.demo_mode {
            return;
        }
        let Some(connection) = client::get(machine, cx) else {
            return;
        };
        let wanted = protocol::DocsSettings {
            enabled: config::with(|s| s.general.features.docs),
            respect_gitignore: config::with(|s| s.modes.docs.respect_gitignore),
        };
        if connection.state != client::State::Connected || connection.settings.docs == wanted {
            return;
        }
        let settings = protocol::EngineSettings {
            docs: wanted,
            ..connection.settings.clone()
        };
        self.machine_request(
            machine,
            Command::SetSettings { settings },
            cx,
            |_, _, _, _| {},
        );
    }

    fn docs_setting(&mut self, act: &Act, window: &mut Window, cx: &mut Context<Self>) {
        let result = config::update(|settings| {
            let docs = &mut settings.modes.docs;
            match act {
                Act::LeftPanel => docs.show_left_panel = !docs.show_left_panel,
                Act::ChatPanel => docs.show_conversation_panel = !docs.show_conversation_panel,
                Act::Gitignore => docs.respect_gitignore = !docs.respect_gitignore,
                Act::RemoteImages => docs.load_remote_images = !docs.load_remote_images,
                Act::OpenIn => {
                    docs.open_in = match docs.open_in {
                        DocView::Viewer => DocView::Editor,
                        DocView::Editor => DocView::Viewer,
                    };
                }
                _ => {}
            }
        });
        if let Err(error) = result {
            window.push_notification(format!("Could not save settings: {error:#}"), cx);
        }
        if matches!(act, Act::Gitignore) {
            let machines: Vec<String> = client::connections(cx)
                .iter()
                .map(|c| c.machine.clone())
                .collect();
            for machine in machines {
                self.docs_sync_engine(&machine, cx);
            }
            for project in self.docs.projects.values_mut() {
                project.listed = false;
            }
            if self.section == Section::Docs {
                // The engine relists with the new rules once it has them.
                let owner = cx.weak_entity();
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(300))
                        .await;
                    let _ = owner.update(cx, |app, cx| app.docs_list(cx));
                })
                .detach();
            }
        }
        cx.refresh_windows();
    }

    pub(super) fn docs_act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            Act::Open(path) => {
                let editor = self.docs_current().and_then(|p| p.files.get(&path)).map_or(
                    config::with(|s| s.modes.docs.open_in == DocView::Editor),
                    |doc| doc.editor,
                );
                self.docs_open(path, window, cx);
                if editor {
                    window.focus(&self.docs.editor.focus_handle(cx), cx);
                }
                self.docs_record(cx);
            }
            Act::Folder(path) => {
                let project = self.docs_project();
                project.cursor = Some(path.clone());
                if !project.expanded.remove(&path) {
                    project.expanded.insert(path);
                }
                self.docs_refresh_rows(cx);
                self.docs_record(cx);
            }
            Act::View => {
                let editor = self.docs_open_doc().is_some_and(|(_, doc)| doc.editor);
                self.docs_set_view(!editor, window, cx);
            }
            Act::Save => {
                if let Some((key, path)) = self.docs.shown.clone() {
                    self.docs_save(&key, &path, cx, |_, _, _, _| {});
                }
            }
            Act::New => {
                let folder = self
                    .docs_current()
                    .and_then(|p| p.cursor.clone())
                    .map(|cursor| {
                        let is_folder = self
                            .docs_current()
                            .is_some_and(|p| p.rows.iter().any(|r| r.folder && r.path == cursor));
                        if is_folder {
                            format!("{cursor}/")
                        } else {
                            let parent = adeline_docs::parent(&cursor);
                            if parent.is_empty() {
                                String::new()
                            } else {
                                format!("{parent}/")
                            }
                        }
                    })
                    .unwrap_or_default();
                self.docs.path_input.update(cx, |state, cx| {
                    state.set_value(folder, window, cx);
                });
                self.modal = Some("doc-new");
            }
            Act::Create => self.docs_create(window, cx),
            Act::Rename(path) => {
                let Some(path) =
                    path.or_else(|| self.docs_current().and_then(|p| p.cursor.clone()))
                else {
                    return;
                };
                if !adeline_docs::has_doc_extension(&path) {
                    return;
                }
                self.docs.path_input.update(cx, |state, cx| {
                    state.set_value(adeline_docs::file_name(&path).to_owned(), window, cx);
                });
                self.docs.target = Some(path);
                self.modal = Some("doc-rename");
            }
            Act::ConfirmRename => self.docs_rename(cx),
            Act::Delete(path) => {
                let Some(path) =
                    path.or_else(|| self.docs_current().and_then(|p| p.cursor.clone()))
                else {
                    return;
                };
                if !adeline_docs::has_doc_extension(&path) {
                    return;
                }
                self.docs.target = Some(path);
                self.modal = Some("doc-delete");
            }
            Act::ConfirmDelete => self.docs_delete(cx),
            Act::Format(format) => self.docs_format(format, window, cx),
            Act::Undo | Act::Redo => {
                let focus = self.docs.editor.focus_handle(cx);
                window.focus(&focus, cx);
                if matches!(act, Act::Undo) {
                    focus.dispatch_action(&Undo, window, cx);
                } else {
                    focus.dispatch_action(&Redo, window, cx);
                }
            }
            Act::Find => {
                window.focus(&self.docs.editor.focus_handle(cx), cx);
                self.docs
                    .editor
                    .update(cx, |state, cx| state.open_search(true, cx));
            }
            Act::Note => self.docs_note(None, window, cx),
            Act::KeepMine => self.docs_resolve(true, window, cx),
            Act::TakeTheirs => self.docs_resolve(false, window, cx),
            Act::CloseDoc => {
                let key = self.docs_key();
                if let Some(project) = self.docs.projects.get_mut(&key)
                    && let Some(path) = project.open.take()
                {
                    project.files.remove(&path);
                    project.failed = None;
                }
                self.docs.shown = None;
                self.docs_record(cx);
            }
            Act::LeftPanel | Act::ChatPanel | Act::Gitignore | Act::OpenIn | Act::RemoteImages => {
                self.docs_setting(&act, window, cx);
            }
            Act::Chooser => self.open_commands("chats", window, cx),
            Act::Chat(id) => {
                self.menu = None;
                self.docs_select_chat(id, window, cx);
                let chat = self.current_id();
                if let Some(project) = self.docs.projects.get_mut(&self.workspace().key()) {
                    project.chat = chat;
                }
                self.docs_record(cx);
                window.focus(&self.composer.focus_handle(cx), cx);
            }
            Act::More(key) => {
                if !self.docs.more.remove(&key) {
                    self.docs.more.insert(key);
                }
                self.transcript.update(cx, chat::Transcript::refresh_all);
            }
            Act::SaveAll => self.docs_save_all(cx),
            Act::Discard => self.docs_discard(window, cx),
        }
        self.control_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// The handlers of Docs mode's actions, on the window's root while Docs shows.
    pub(super) fn docs_actions(&self, content: Div, cx: &Context<Self>) -> Div {
        content
            .on_action(cx.listener(|app, _: &ToggleDocView, window, cx| {
                app.act(Action::Docs(Act::View), window, cx);
            }))
            .on_action(cx.listener(|app, _: &SaveDoc, window, cx| {
                app.act(Action::Docs(Act::Save), window, cx);
            }))
            .on_action(cx.listener(|app, _: &ChooseChat, window, cx| {
                app.act(Action::Docs(Act::Chooser), window, cx);
            }))
            .on_action(cx.listener(|app, _: &NoteOnSelection, window, cx| {
                app.act(Action::Docs(Act::Note), window, cx);
            }))
            .on_action(cx.listener(|app, _: &NewDoc, window, cx| {
                app.act(Action::Docs(Act::New), window, cx);
            }))
            .on_action(cx.listener(|app, _: &FindDoc, window, cx| {
                window.focus(&app.docs.search.focus_handle(cx), cx);
            }))
            .on_action(cx.listener(|app, action: &ApplyFormat, window, cx| {
                app.act(Action::Docs(Act::Format(action.format)), window, cx);
            }))
            .on_action(cx.listener(|app, action: &ToggleTask, window, cx| {
                app.docs_toggle_task(action.offset, window, cx);
            }))
            .on_action(cx.listener(|app, action: &NoteRange, window, cx| {
                app.docs_note(Some(action.start..action.end), window, cx);
            }))
            .on_action(cx.listener(|app, action: &OpenLink, window, cx| {
                app.docs_link(&action.url, window, cx);
            }))
    }

    // Reads the images the Viewer asked for since the last frame.
    pub(super) fn docs_load_images(&mut self, cx: &mut Context<Self>) {
        let wanted = self.docs.images.wanted();
        if wanted.is_empty() || !self.has_open_project() {
            return;
        }
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        for path in wanted {
            let images = self.docs.images.clone();
            self.docs_request(
                &machine,
                &id.clone(),
                Command::ReadImage {
                    project_id: id.clone(),
                    path: path.clone(),
                },
                cx,
                move |app, result, _, cx| {
                    let image = result.ok().and_then(|value| {
                        let bytes = conversation::unbase64(value.as_str()?)?;
                        let format = match path.rsplit_once('.')?.1.to_lowercase().as_str() {
                            "png" => ImageFormat::Png,
                            "jpg" | "jpeg" => ImageFormat::Jpeg,
                            "gif" => ImageFormat::Gif,
                            "webp" => ImageFormat::Webp,
                            "bmp" => ImageFormat::Bmp,
                            "svg" => ImageFormat::Svg,
                            _ => return None,
                        };
                        Some(std::sync::Arc::new(Image::from_bytes(format, bytes)))
                    });
                    images.put(path, image);
                    app.docs
                        .viewer
                        .update(cx, |state, cx| state.invalidate_inline_layout(cx));
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{note, slug};

    #[test]
    fn a_note_quotes_the_selection_with_its_path_and_lines() {
        let text = "# Plan\n\nFirst line\nSecond line\nThird\n";
        let start = text.find("First").unwrap();
        let end = text.find("Third").unwrap();
        assert_eq!(
            note("notes/plan.md", text, start..end),
            "`notes/plan.md`, lines 3–4:\n\n> First line\n> Second line\n\n"
        );
        let word = text.find("Plan").unwrap();
        assert!(note("a.md", text, word..word + 4).starts_with("`a.md`, line 1:"));
    }

    #[test]
    fn anchors_follow_github_heading_slugs() {
        assert_eq!(slug("Saving and reloading"), "saving-and-reloading");
        assert_eq!(slug(" What's new in 2.0? "), "whats-new-in-20");
        assert_eq!(slug("R4 — conflicts"), "r4--conflicts");
    }
}
