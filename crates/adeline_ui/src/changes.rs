//! Changes mode: the open project's files tree with its uncommitted changes
//! against `HEAD`, the selected file's diff (or its whole text, unchanged), a
//! conversation beside them, and comments on lines or files that collect as
//! cards in the composer until the message goes out. The engine runs git and
//! reads the files (`--demo` answers from `assets/demo-changes.json`), so
//! remote projects work like local ones.
//!
//! The open project's changes are watched in every mode while Changes is on,
//! so the Activity panel can sum them. Entering Changes remembers the Chats
//! selection and leaving restores it, as in Docs.

use super::*;
use adeline_core::protocol::{Change, ChangeFile, Changes as Listed, Command, Repo};
use adeline_docs::{Row, Tree};
use gpui_kit::component::diff::{DiffFile, DiffLineRange, DiffSide, DiffState};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

mod view;

actions!(
    adeline,
    [
        NextFile,
        PreviousFile,
        NextHunk,
        PreviousHunk,
        CommentOnFile,
        ToggleChangedOnly
    ]
);

pub(crate) fn key_bindings(cx: &App) -> Vec<KeyBinding> {
    let mut bindings = vec![
        KeyBinding::new("f8", NextFile, None),
        KeyBinding::new("shift-f8", PreviousFile, None),
        KeyBinding::new("f7", NextHunk, None),
        KeyBinding::new("shift-f7", PreviousHunk, None),
    ];
    for modifier in ["ctrl", "cmd"] {
        let key = |rest: &str| format!("{modifier}-{rest}");
        bindings.extend([
            key_binding(&key("alt-m"), CommentOnFile, None, cx),
            key_binding(&key("alt-c"), ToggleChangedOnly, None, cx),
        ]);
    }
    bindings
}

#[derive(Clone, Debug)]
pub(crate) enum Act {
    Open(String),
    Folder(String),
    FromSummary,
    ChangedOnly,
    Wrap,
    Step { file: bool, forward: bool },
    CommentSelection,
    CommentFile(Option<String>),
    CommentRange(DiffLineRange),
    SaveComment,
    CancelComment,
    EditComment(u64),
    DeleteComment(u64),
    LeftPanel,
    ChatPanel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    kind: char,
    old: Option<usize>,
    new: Option<usize>,
    text: String,
}

impl Line {
    fn number(&self, side: DiffSide) -> Option<usize> {
        match side {
            DiffSide::Original => self.old,
            DiffSide::Modified => self.new,
        }
    }
}

fn patch_lines(patch: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let (mut old, mut new) = (0, 0);
    let mut in_hunk = false;
    for raw in patch.lines() {
        if let Some(header) = raw.strip_prefix("@@ ") {
            let mut parts = header.split(' ');
            let start = |part: Option<&str>, sign: char| {
                part.and_then(|p| p.strip_prefix(sign))
                    .and_then(|p| p.split(',').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(1)
            };
            old = start(parts.next(), '-');
            new = start(parts.next(), '+');
            in_hunk = true;
            continue;
        }
        if raw.starts_with("diff --git ") {
            in_hunk = false;
        }
        if !in_hunk {
            continue;
        }
        let (kind, text) = match raw.chars().next() {
            Some(kind @ (' ' | '-' | '+')) => (kind, &raw[1..]),
            None => (' ', ""),
            _ => continue,
        };
        let line = Line {
            kind,
            old: (kind != '+').then_some(old),
            new: (kind != '-').then_some(new),
            text: text.to_owned(),
        };
        old += usize::from(kind != '+');
        new += usize::from(kind != '-');
        lines.push(line);
    }
    lines
}

fn text_lines(text: &str) -> Vec<Line> {
    text.lines()
        .enumerate()
        .map(|(ix, line)| Line {
            kind: ' ',
            old: Some(ix + 1),
            new: Some(ix + 1),
            text: line.to_owned(),
        })
        .collect()
}

fn quote(lines: &[Line], range: &DiffLineRange) -> Vec<Line> {
    if range.side() == range.end_side() {
        let side = range.side();
        let (low, high) = (
            range.start().min(range.end()),
            range.start().max(range.end()),
        );
        return lines
            .iter()
            .filter(|line| line.number(side).is_some_and(|n| (low..=high).contains(&n)))
            .cloned()
            .collect();
    }
    let at = |side: DiffSide, n: usize| lines.iter().position(|l| l.number(side) == Some(n));
    match (
        at(range.side(), range.start()),
        at(range.end_side(), range.end()),
    ) {
        (Some(a), Some(b)) => lines[a.min(b)..=a.max(b)].to_vec(),
        _ => Vec::new(),
    }
}

fn span(quoted: &[Line]) -> String {
    let number = |line: &Line| line.new.or(line.old).unwrap_or(0);
    match (quoted.first(), quoted.last()) {
        (Some(first), Some(last)) if number(first) != number(last) => {
            format!("{}–{}", number(first), number(last))
        }
        (Some(first), _) => number(first).to_string(),
        _ => String::new(),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Comment {
    pub(crate) id: u64,
    key: String,
    pub(crate) path: String,
    range: Option<DiffLineRange>,
    quoted: Vec<Line>,
    diff: bool,
    pub(crate) text: String,
    pub(crate) stale: bool,
}

impl Comment {
    pub(crate) fn span(&self) -> String {
        span(&self.quoted)
    }

    pub(crate) fn whole_file(&self) -> bool {
        self.range.is_none()
    }

    fn message(&self) -> String {
        if self.range.is_none() {
            return format!("`{}`:\n{}", self.path, self.text);
        }
        let body: Vec<String> = self
            .quoted
            .iter()
            .map(|line| {
                if self.diff {
                    format!("{}{}", line.kind, line.text)
                } else {
                    line.text.clone()
                }
            })
            .collect();
        let body = body.join("\n");
        let mut fence = "```".to_owned();
        while body.contains(&fence) {
            fence.push('`');
        }
        let span = self.span();
        let lines = if span.contains('–') {
            "lines"
        } else {
            "line"
        };
        let language = if self.diff { "diff" } else { "" };
        format!(
            "`{}`, {lines} {span}:\n{fence}{language}\n{body}\n{fence}\n{}",
            self.path, self.text
        )
    }
}

pub(crate) fn prompt(comments: &[&Comment], typed: &str) -> String {
    let mut parts: Vec<String> = comments.iter().map(|comment| comment.message()).collect();
    if !typed.is_empty() {
        parts.push(typed.to_owned());
    }
    parts.join("\n\n")
}

#[derive(Clone, Debug)]
pub(crate) struct Draft {
    key: String,
    path: String,
    range: Option<DiffLineRange>,
    existing: Option<u64>,
}

#[derive(Clone, Debug)]
enum Body {
    Loading,
    Lines {
        lines: Vec<Line>,
        diff: bool,
        source: String,
    },
    Binary,
    TooLarge,
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Project {
    machine: String,
    id: String,
    files: Vec<String>,
    listed: Listed,
    loaded: bool,
    error: Option<String>,
    tree: Tree,
    rows: Vec<Row>,
    expanded: HashSet<String>,
    open: Option<String>,
    body: Option<(String, Body)>,
    chat: Option<String>,
    cursor: Option<String>,
    listing: u64,
    changed_since: bool,
    reading: HashMap<String, u64>,
}

impl Project {
    fn change(&self, path: &str) -> Option<&Change> {
        self.listed
            .changes
            .iter()
            .find(|change| change.path == path)
    }

    fn holds_change(&self, folder: &str) -> bool {
        self.listed.changes.iter().any(|change| {
            change
                .path
                .strip_prefix(folder)
                .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    fn changed_in_order(&self) -> Vec<String> {
        let tree = Tree::new(self.listed.changes.iter().map(|c| c.path.clone()).collect());
        let all: HashSet<String> = tree.folders().into_iter().collect();
        tree.rows(&all)
            .into_iter()
            .filter(|row| !row.folder)
            .map(|row| row.path)
            .collect()
    }

    pub(crate) fn totals(&self) -> Option<(usize, usize)> {
        (self.loaded && self.listed.repo == Repo::Git && !self.listed.changes.is_empty()).then(
            || {
                self.listed
                    .changes
                    .iter()
                    .fold((0, 0), |(a, d), c| (a + c.added, d + c.removed))
            },
        )
    }
}

pub(crate) struct Changes {
    pub(super) search: Entity<InputState>,
    pub(super) diff: Entity<DiffState>,
    pub(super) comment: Entity<TextareaState>,
    projects: HashMap<String, Project>,
    pub(super) comments: Vec<Comment>,
    next_comment: u64,
    pub(super) editing: Option<Draft>,
    chats_selection: Option<String>,
    tree_focus: FocusHandle,
    tree_scroll: UniformListScrollHandle,
    changed_only: bool,
    watching: Option<(String, String)>,
    shown: Option<(String, String)>,
    demo: HashMap<String, Value>,
}

impl Changes {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Adeline>) -> Self {
        Self {
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search files")),
            diff: cx.new(|cx| DiffState::new([], cx)),
            comment: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 8)
                    .submit_on_enter(true)
                    .placeholder("Comment for the agent")
            }),
            projects: HashMap::new(),
            comments: Vec::new(),
            next_comment: 1,
            editing: None,
            chats_selection: None,
            tree_focus: cx.focus_handle().tab_stop(true),
            tree_scroll: UniformListScrollHandle::new(),
            changed_only: false,
            watching: None,
            shown: None,
            demo: HashMap::new(),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(result: Result<Value, String>) -> Result<T, String> {
    result.and_then(|value| serde_json::from_value(value).map_err(|e| e.to_string()))
}

fn json(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

impl Adeline {
    pub(super) fn changes_on(&self) -> bool {
        config::with(|s| s.general.features.changes)
    }

    fn changes_key(&self) -> String {
        self.workspace().key()
    }

    fn changes_project(&mut self) -> &mut Project {
        let key = self.changes_key();
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        self.changes.projects.entry(key).or_insert_with(|| Project {
            machine,
            id,
            ..Project::default()
        })
    }

    pub(super) fn changes_current(&self) -> Option<&Project> {
        self.changes.projects.get(&self.changes_key())
    }

    pub(super) fn changes_open_path(&self) -> Option<&str> {
        self.changes_current()?.open.as_deref()
    }

    pub(super) fn changes_pending(&self) -> Vec<&Comment> {
        let key = self.changes_key();
        self.changes
            .comments
            .iter()
            .filter(|comment| comment.key == key)
            .collect()
    }

    fn changes_request(
        &mut self,
        machine: &str,
        project_id: &str,
        command: Command,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Value, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.demo_mode {
            self.machine_request(machine, command, cx, |app, result, _, cx| {
                app.defer_with_window(cx, move |app, window, cx| done(app, result, window, cx));
            });
            return;
        }
        let result = self.demo_changes(project_id, command);
        self.defer_with_window(cx, move |app, window, cx| done(app, result, window, cx));
    }

    fn demo_changes(&mut self, project_id: &str, command: Command) -> Result<Value, String> {
        let demo = self
            .changes
            .demo
            .entry(project_id.to_owned())
            .or_insert_with(|| {
                embedded("demo-changes.json")
                    .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
                    .and_then(|mut all| all.get_mut(project_id).map(Value::take))
                    .unwrap_or_default()
            });
        let files = demo["files"].as_object().cloned().unwrap_or_default();
        match command {
            Command::ListFiles { .. } => json(files.keys().collect::<Vec<_>>()),
            Command::ListChanges { .. } => Ok(serde_json::json!({
                "repo": "git",
                "changes": demo["changes"].as_array().cloned().unwrap_or_default(),
            })),
            Command::ReadChange { path, .. } => {
                if let Some(patch) = demo["patches"][&path].as_str() {
                    return json(ChangeFile {
                        patch: Some(patch.to_owned()),
                        ..ChangeFile::default()
                    });
                }
                let text = files
                    .get(&path)
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{path} is not in the demo."))?;
                json(ChangeFile {
                    text: Some(text.to_owned()),
                    ..ChangeFile::default()
                })
            }
            _ => Ok(Value::Null),
        }
    }

    pub(super) fn changes_follow(&mut self, cx: &mut Context<Self>) {
        let wanted = (self.changes_on() && self.has_open_project()).then(|| {
            (
                self.workspace().machine.clone(),
                self.workspace().config.id.clone(),
            )
        });
        if self.changes.watching == wanted {
            return;
        }
        if let Some((machine, id)) = self.changes.watching.take()
            && wanted.as_ref().is_none_or(|(m, _)| *m != machine)
        {
            self.changes_request(
                &machine,
                &id,
                Command::WatchChanges { project_id: None },
                cx,
                |_, _, _, _| {},
            );
        }
        let Some((machine, id)) = wanted else {
            return;
        };
        if !self.demo_mode && !client::usable(&machine, cx) {
            return;
        }
        self.changes.watching = Some((machine.clone(), id.clone()));
        self.changes_request(
            &machine,
            &id,
            Command::WatchChanges {
                project_id: Some(id.clone()),
            },
            cx,
            |_, _, _, _| {},
        );
        self.changes_project();
        self.changes_list(cx);
    }

    pub(super) fn changes_connected(&mut self, machine: &str) {
        if self
            .changes
            .watching
            .as_ref()
            .is_some_and(|(m, _)| m == machine)
        {
            self.changes.watching = None;
        }
    }

    fn changes_list(&mut self, cx: &mut Context<Self>) {
        let key = self.changes_key();
        let (machine, id) = (
            self.workspace().machine.clone(),
            self.workspace().config.id.clone(),
        );
        let project = self.changes_project();
        project.error = None;
        project.changed_since = false;
        project.listing += 1;
        let listing = project.listing;
        let current = move |app: &Self, key: &str| {
            app.changes
                .projects
                .get(key)
                .is_some_and(|project| project.listing == listing && !project.changed_since)
        };
        self.changes_request(
            &machine,
            &id,
            Command::ListFiles {
                project_id: id.clone(),
            },
            cx,
            {
                let key = key.clone();
                move |app, result, _, cx| {
                    if !current(app, &key) {
                        return;
                    }
                    if let Some(project) = app.changes.projects.get_mut(&key) {
                        match parse(result) {
                            Ok(files) => project.files = files,
                            Err(error) => project.error = Some(error),
                        }
                    }
                    app.changes_refresh_rows(cx);
                }
            },
        );
        self.changes_request(
            &machine,
            &id,
            Command::ListChanges {
                project_id: id.clone(),
            },
            cx,
            move |app, result, _, cx| {
                if !current(app, &key) {
                    let again = app.changes_key() == key
                        && app.changes_current().is_some_and(|p| p.listing == listing);
                    if again {
                        app.changes_list(cx);
                    }
                    return;
                }
                if let Some(project) = app.changes.projects.get_mut(&key) {
                    match parse(result) {
                        Ok(listed) => {
                            project.listed = listed;
                            project.loaded = true;
                        }
                        Err(error) => project.error = Some(error),
                    }
                }
                app.changes_refresh_rows(cx);
                app.changes_recheck(&key, cx);
            },
        );
    }

    pub(super) fn changes_delta(
        &mut self,
        machine: &str,
        project_id: &str,
        listed: Listed,
        files: Option<Vec<String>>,
        cx: &mut Context<Self>,
    ) {
        let key = format!("{machine}:{project_id}");
        let Some(project) = self.changes.projects.get_mut(&key) else {
            return;
        };
        project.listed = listed;
        project.loaded = true;
        project.error = None;
        project.changed_since = true;
        if let Some(files) = files {
            project.files = files;
        }
        self.changes_refresh_rows(cx);
        self.changes_recheck(&key, cx);
    }

    pub(super) fn changes_refresh_rows(&mut self, cx: &mut Context<Self>) {
        let query = self.changes.search.read(cx).value().trim().to_owned();
        let changed_only = self.changes.changed_only;
        if let Some(project) = self.changes.projects.get_mut(&self.workspace().key()) {
            let mut paths: Vec<String> = project
                .listed
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect();
            if !changed_only {
                paths.extend(project.files.iter().cloned());
                paths.sort();
                paths.dedup();
            }
            project.tree = Tree::new(paths);
            project.rows = if !query.is_empty() {
                project.tree.search(&query)
            } else if changed_only {
                let all: HashSet<String> = project.tree.folders().into_iter().collect();
                project.tree.rows(&all)
            } else {
                project.tree.rows(&project.expanded)
            };
        }
        cx.notify();
    }

    pub(super) fn changes_enter(
        &mut self,
        chat: Option<String>,
        open: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_open_project() {
            return;
        }
        if self.section != Section::Changes {
            self.changes.chats_selection = self.current_id();
        }
        self.section = Section::Changes;
        let current = self.current_id();
        let saved = self.changes_project().chat.clone();
        self.docs_select_chat(chat.or(saved).or(current), window, cx);
        self.changes_follow(cx);
        self.changes_list(cx);
        let open = open.or_else(|| self.changes_project().open.clone());
        self.changes.shown = None;
        if let Some(path) = open {
            self.changes_open(&path, window, cx);
        }
        self.changes_refresh_rows(cx);
    }

    pub(super) fn changes_leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != Section::Changes {
            return;
        }
        let current = self.current_id();
        if self.has_open_project() {
            self.changes_project().chat = current;
        }
        let previous = self.changes.chats_selection.take();
        self.docs_select_chat(previous, window, cx);
    }

    pub(super) fn changes_chat_picked(&mut self, chat: Option<String>) {
        self.changes_project().chat = chat;
    }

    fn changes_open(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.changes_key();
        let project = self.changes_project();
        project.open = Some(path.to_owned());
        project.cursor = Some(path.to_owned());
        if project.body.as_ref().is_none_or(|(p, _)| p != path) {
            project.body = Some((path.to_owned(), Body::Loading));
        }
        if let Some(draft) = &self.changes.editing
            && (draft.key != key || draft.path != path)
        {
            self.changes.editing = None;
        }
        self.changes_load(&key, path, window, cx);
        cx.notify();
    }

    fn changes_load(&mut self, key: &str, path: &str, _: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.changes.projects.get_mut(key) else {
            return;
        };
        let (machine, id) = (project.machine.clone(), project.id.clone());
        let reading = project.reading.entry(path.to_owned()).or_default();
        *reading += 1;
        let reading = *reading;
        let (key, path) = (key.to_owned(), path.to_owned());
        self.changes_request(
            &machine,
            &id.clone(),
            Command::ReadChange {
                project_id: id,
                path: path.clone(),
            },
            cx,
            move |app, result, _, cx| {
                let latest = app
                    .changes
                    .projects
                    .get(&key)
                    .and_then(|p| p.reading.get(&path));
                if latest != Some(&reading) {
                    return;
                }
                let body = match parse::<ChangeFile>(result) {
                    Ok(file) if file.too_large => Body::TooLarge,
                    Ok(file) if file.binary => Body::Binary,
                    Ok(ChangeFile {
                        patch: Some(patch), ..
                    }) => Body::Lines {
                        lines: patch_lines(&patch),
                        diff: true,
                        source: patch,
                    },
                    Ok(file) => {
                        let text = file.text.unwrap_or_default();
                        Body::Lines {
                            lines: text_lines(&text),
                            diff: false,
                            source: text,
                        }
                    }
                    Err(error) => Body::Failed(error),
                };
                app.changes_mark_stale(&key, &path, &body);
                let Some(project) = app.changes.projects.get_mut(&key) else {
                    return;
                };
                if project.open.as_deref() != Some(&path) {
                    cx.notify();
                    return;
                }
                let same = match (&project.body, &body) {
                    (Some((p, Body::Lines { source: a, .. })), Body::Lines { source: b, .. }) => {
                        *p == path && a == b
                    }
                    _ => false,
                };
                project.body = Some((path.clone(), body));
                if !same {
                    app.changes.shown = None;
                }
                app.changes_show(cx);
                cx.notify();
            },
        );
    }

    fn changes_show(&mut self, cx: &mut Context<Self>) {
        let key = self.changes_key();
        let Some(project) = self.changes.projects.get(&key) else {
            return;
        };
        let Some((path, body)) = &project.body else {
            return;
        };
        if self.changes.shown.as_ref() == Some(&(key.clone(), path.clone())) {
            return;
        }
        let file = match body {
            Body::Lines {
                diff: true, source, ..
            } => DiffFile::parse(source)
                .ok()
                .and_then(|files| files.into_iter().next()),
            Body::Lines { source, .. } => Some(DiffFile::unchanged(path.clone(), source)),
            _ => None,
        };
        self.changes.shown = Some((key, path.clone()));
        self.changes
            .diff
            .update(cx, |state, cx| state.set_files(file, cx));
    }

    fn changes_recheck(&mut self, key: &str, cx: &mut Context<Self>) {
        let mut paths: Vec<String> = self
            .changes
            .comments
            .iter()
            .filter(|c| c.key == key && c.range.is_some())
            .map(|c| c.path.clone())
            .collect();
        let open = self
            .changes
            .projects
            .get(key)
            .and_then(|project| project.open.clone())
            .filter(|_| self.section == Section::Changes && self.changes_key() == key);
        paths.extend(open);
        paths.sort();
        paths.dedup();
        for path in paths {
            self.defer_with_window(cx, {
                let key = key.to_owned();
                move |app, window, cx| app.changes_load(&key, &path, window, cx)
            });
        }
    }

    fn changes_mark_stale(&mut self, key: &str, path: &str, body: &Body) {
        let lines = match body {
            Body::Lines { lines, .. } => Some(lines),
            _ => None,
        };
        for comment in &mut self.changes.comments {
            if comment.key != key || comment.path != path {
                continue;
            }
            if let Some(range) = &comment.range {
                comment.stale = lines.is_none_or(|lines| quote(lines, range) != comment.quoted);
            }
        }
    }

    fn changes_start_comment(
        &mut self,
        path: String,
        range: Option<DiffLineRange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = self.changes_key();
        if self.changes_open_path() != Some(path.as_str()) {
            self.changes_open(&path, window, cx);
        }
        self.changes.editing = Some(Draft {
            key,
            path,
            range,
            existing: None,
        });
        self.changes
            .comment
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.changes_focus_comment(window, cx);
    }

    fn changes_focus_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !config::with(|s| s.modes.changes.show_conversation_panel) {
            self.changes_setting(&Act::ChatPanel, window, cx);
        }
        window.focus(&self.changes.comment.focus_handle(cx), cx);
        cx.notify();
    }

    fn changes_comment_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.changes_open_path().map(str::to_owned) else {
            return;
        };
        let Some(range) = self.changes.diff.read(cx).selected_lines() else {
            window.push_notification(toast("Select some lines first."), cx);
            return;
        };
        self.changes_start_comment(path, Some(range), window, cx);
    }

    fn changes_save_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.changes.editing.take() else {
            return;
        };
        let text = self.changes.comment.read(cx).value().trim().to_owned();
        if text.is_empty() {
            if let Some(id) = draft.existing {
                self.changes.comments.retain(|c| c.id != id);
            }
            self.changes_after_comment(window, cx);
            return;
        }
        if let Some(id) = draft.existing {
            if let Some(comment) = self.changes.comments.iter_mut().find(|c| c.id == id) {
                comment.text = text;
            }
            self.changes_after_comment(window, cx);
            return;
        }
        let body = self
            .changes
            .projects
            .get(&draft.key)
            .and_then(|p| p.body.as_ref())
            .filter(|(path, _)| *path == draft.path)
            .map(|(_, body)| body.clone());
        let (quoted, diff) = match (&draft.range, &body) {
            (Some(range), Some(Body::Lines { lines, diff, .. })) => (quote(lines, range), *diff),
            _ => (Vec::new(), false),
        };
        let id = self.changes.next_comment;
        self.changes.next_comment += 1;
        self.changes.comments.push(Comment {
            id,
            key: draft.key,
            path: draft.path,
            range: draft.range.filter(|_| !quoted.is_empty()),
            quoted,
            diff,
            text,
            stale: false,
        });
        self.changes_after_comment(window, cx);
    }

    fn changes_after_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.changes
            .comment
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.changes
            .diff
            .update(cx, |state, cx| state.set_selected_lines(None, cx));
        window.focus(&self.composer.focus_handle(cx), cx);
        self.composer_region.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn changes_edit_comment(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(comment) = self.changes.comments.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        if self.section != Section::Changes || self.changes_key() != comment.key {
            if self.changes_key() != comment.key {
                return;
            }
            self.changes_enter(None, Some(comment.path.clone()), window, cx);
        } else if self.changes_open_path() != Some(comment.path.as_str()) {
            self.changes_open(&comment.path, window, cx);
        }
        self.changes.editing = Some(Draft {
            key: comment.key,
            path: comment.path,
            range: comment.range,
            existing: Some(id),
        });
        self.changes
            .comment
            .update(cx, |state, cx| state.set_value(comment.text, window, cx));
        self.changes_focus_comment(window, cx);
    }

    pub(super) fn changes_sent(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        self.changes.comments.retain(|c| !ids.contains(&c.id));
        cx.notify();
    }

    pub(super) fn changes_cancel(&mut self) -> bool {
        self.changes.editing.take().is_some()
    }

    fn changes_step(
        &mut self,
        file: bool,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !file {
            self.changes.diff.update(cx, |state, cx| {
                if forward {
                    state.next_change(cx);
                } else {
                    state.previous_change(cx);
                }
            });
            return;
        }
        let Some(project) = self.changes_current() else {
            return;
        };
        let changed = project.changed_in_order();
        if changed.is_empty() {
            return;
        }
        let at = project
            .open
            .as_ref()
            .and_then(|open| changed.iter().position(|p| p == open));
        let next = match (at, forward) {
            (Some(at), true) => (at + 1) % changed.len(),
            (Some(at), false) => (at + changed.len() - 1) % changed.len(),
            (None, true) => 0,
            (None, false) => changed.len() - 1,
        };
        let path = changed[next].clone();
        self.changes_reveal(&path);
        self.changes_open(&path, window, cx);
        self.changes_refresh_rows(cx);
        window.focus(&self.changes.diff.focus_handle(cx), cx);
    }

    fn changes_reveal(&mut self, path: &str) {
        let project = self.changes_project();
        let parent = adeline_docs::parent(path);
        let mut folder = String::new();
        for part in parent.split('/').filter(|p| !p.is_empty()) {
            if !folder.is_empty() {
                folder.push('/');
            }
            folder.push_str(part);
            project.expanded.insert(folder.clone());
        }
    }

    fn changes_setting(&mut self, act: &Act, window: &mut Window, cx: &mut Context<Self>) {
        let result = config::update(|settings| {
            let changes = &mut settings.modes.changes;
            match act {
                Act::LeftPanel => changes.show_left_panel = !changes.show_left_panel,
                Act::ChatPanel => {
                    changes.show_conversation_panel = !changes.show_conversation_panel;
                }
                Act::Wrap => changes.wrap = !changes.wrap,
                _ => {}
            }
        });
        if let Err(error) = result {
            window.push_notification(toast(format!("Could not save settings: {error:#}")), cx);
        }
        cx.refresh_windows();
    }

    pub(super) fn changes_act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            Act::Open(path) => {
                self.changes_open(&path, window, cx);
                window.focus(&self.changes.diff.focus_handle(cx), cx);
            }
            Act::Folder(path) => {
                let project = self.changes_project();
                project.cursor = Some(path.clone());
                toggle_member(&mut project.expanded, path);
                self.changes_refresh_rows(cx);
            }
            Act::FromSummary => {
                let chat = self.current_id();
                self.changes_project();
                let first = self
                    .changes_current()
                    .and_then(|p| p.changed_in_order().into_iter().next());
                if let Some(path) = &first {
                    self.changes_reveal(path);
                }
                self.docs_leave(window, cx);
                self.changes_enter(chat, first, window, cx);
            }
            Act::ChangedOnly => {
                self.changes.changed_only = !self.changes.changed_only;
                self.changes_refresh_rows(cx);
            }
            Act::Step { file, forward } => self.changes_step(file, forward, window, cx),
            Act::CommentSelection => self.changes_comment_selection(window, cx),
            Act::CommentFile(path) => {
                let path = path.or_else(|| {
                    let project = self.changes_current()?;
                    project
                        .cursor
                        .clone()
                        .filter(|c| !project.rows.iter().any(|r| r.folder && r.path == *c))
                        .or_else(|| project.open.clone())
                });
                if let Some(path) = path {
                    self.changes_start_comment(path, None, window, cx);
                }
            }
            Act::CommentRange(range) => {
                let path = range.path().to_string();
                self.changes_start_comment(path, Some(range), window, cx);
            }
            Act::SaveComment => self.changes_save_comment(window, cx),
            Act::CancelComment => {
                self.changes.editing = None;
                window.focus(&self.changes.diff.focus_handle(cx), cx);
            }
            Act::EditComment(id) => self.changes_edit_comment(id, window, cx),
            Act::DeleteComment(id) => {
                self.changes.comments.retain(|c| c.id != id);
                if self
                    .changes
                    .editing
                    .as_ref()
                    .is_some_and(|d| d.existing == Some(id))
                {
                    self.changes.editing = None;
                }
                self.composer_region.update(cx, |_, cx| cx.notify());
            }
            Act::Wrap | Act::LeftPanel | Act::ChatPanel => {
                self.changes_setting(&act, window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn changes_actions(&self, content: Div, cx: &Context<Self>) -> Div {
        let step = |file, forward| Action::Changes(Act::Step { file, forward });
        content
            .on_action(act_on::<NextFile>(step(true, true), cx))
            .on_action(act_on::<PreviousFile>(step(true, false), cx))
            .on_action(act_on::<NextHunk>(step(false, true), cx))
            .on_action(act_on::<PreviousHunk>(step(false, false), cx))
            .on_action(act_on::<CommentOnFile>(
                Action::Changes(Act::CommentFile(None)),
                cx,
            ))
            .on_action(act_on::<ToggleChangedOnly>(
                Action::Changes(Act::ChangedOnly),
                cx,
            ))
            .on_action(act_on::<docs::NoteOnSelection>(
                Action::Changes(Act::CommentSelection),
                cx,
            ))
            .on_action(act_on::<docs::ToggleDocView>(
                Action::Changes(Act::Wrap),
                cx,
            ))
            .on_action(act_on::<docs::ChooseChat>(
                Action::Docs(docs::Act::Chooser),
                cx,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Comment, DiffLineRange, DiffSide, patch_lines, prompt, quote, text_lines};

    const PATCH: &str = "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,4 +1,4 @@\n fn main() {\n-    retry();\n+    retry_forever();\n     done();\n }\n";

    #[test]
    fn comments_quote_their_lines_and_come_before_the_typed_text() {
        let lines = patch_lines(PATCH);
        assert_eq!(lines.len(), 5);
        assert_eq!((lines[2].old, lines[2].new), (None, Some(2)));
        let range = DiffLineRange::new("src/app.rs", DiffSide::Original, 2, 3)
            .with_end_side(DiffSide::Modified);
        let quoted = quote(&lines, &range);
        assert_eq!(quoted.len(), 3);
        let line = Comment {
            id: 1,
            key: "k".into(),
            path: "src/app.rs".into(),
            range: Some(range),
            quoted,
            diff: true,
            text: "cap it at 3".into(),
            stale: false,
        };
        let file = Comment {
            id: 2,
            range: None,
            quoted: Vec::new(),
            path: "README.md".into(),
            text: "mention the cache".into(),
            ..line.clone()
        };
        assert_eq!(line.span(), "2–3");
        assert_eq!(
            prompt(&[&line, &file], "Fix these two"),
            "`src/app.rs`, lines 2–3:\n```diff\n-    retry();\n+    retry_forever();\n     done();\n```\ncap it at 3\n\n`README.md`:\nmention the cache\n\nFix these two"
        );
    }

    #[test]
    fn a_rewritten_line_no_longer_matches_its_quote() {
        let range = DiffLineRange::new("a", DiffSide::Modified, 2, 2);
        let before = quote(&text_lines("one\ntwo\nthree"), &range);
        assert_eq!(before[0].text, "two");
        assert_eq!(quote(&text_lines("one\ntwo\nthree\n"), &range), before);
        assert_ne!(quote(&text_lines("one\nTWO\nthree"), &range), before);
    }
}
