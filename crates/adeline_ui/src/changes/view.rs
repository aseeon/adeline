//! Drawing Changes mode: the files tree with each change's status color and
//! counts, the selected file's diff with its comment box and gutter markers,
//! the comment cards in the composer, and the summary in the Activity panel.

use super::{Act, Body, Comment};
use crate::*;
use adeline_core::protocol::{Change, ChangeStatus, Repo};
use gpui_kit::component::diff::{Diff, DiffAnnotation, DiffGutterMarker, DiffLinePosition};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use std::collections::HashMap;

fn status_name(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Modified => "modified",
        ChangeStatus::Added => "added",
        ChangeStatus::Deleted => "deleted",
        ChangeStatus::Renamed => "renamed",
        ChangeStatus::Untracked => "untracked",
    }
}

fn status_color(status: ChangeStatus, cx: &App) -> Hsla {
    let theme = cx.theme();
    match status {
        ChangeStatus::Added | ChangeStatus::Untracked => theme.success,
        ChangeStatus::Modified => theme.warning,
        ChangeStatus::Deleted => theme.danger,
        ChangeStatus::Renamed => theme.info,
    }
}

fn counts(added: usize, removed: usize) -> String {
    format!("+{added} −{removed}")
}

fn first_words(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    let mut words = String::new();
    for word in line.split_whitespace() {
        if words.chars().count() + word.chars().count() > 32 {
            words.push('…');
            return words;
        }
        if !words.is_empty() {
            words.push(' ');
        }
        words.push_str(word);
    }
    if line != text.trim_end() {
        words.push('…');
    }
    words
}

pub(crate) fn card_label(comment: &Comment) -> String {
    let lines = if comment.whole_file() {
        String::new()
    } else {
        format!(", lines {}", comment.span())
    };
    let stale = if comment.stale { ", lines changed" } else { "" };
    format!(
        "Comment on {}{lines}: {}{stale}",
        comment.path,
        first_words(&comment.text)
    )
}

fn comment_box(
    comment: &Entity<TextareaState>,
    owner: &WeakEntity<Adeline>,
    existing: Option<u64>,
    cx: &App,
) -> AnyElement {
    let isl = theme::islands(cx);
    let act = |id: &'static str, label: &'static str, act: Act| {
        let owner = owner.clone();
        Button::new(id)
            .small()
            .label(label)
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                let _ = owner.update(cx, |app, cx| {
                    app.act(Action::Changes(act.clone()), window, cx);
                });
            })
    };
    col()
        .id("comment-box")
        .test_support()
        .role(Role::Group)
        .aria_label("Comment box")
        .m(panes::dp(8.))
        .p(panes::dp(8.))
        .gap(panes::dp(6.))
        .w(panes::dp(320.))
        .rounded(panes::dp(10.))
        .bg(isl.raise)
        .border_1()
        .border_color(isl.ring_strong)
        .child(
            Textarea::new(comment)
                .aria_label("Comment")
                .appearance(false)
                .text_size(panes::dp(theme::T_BODY)),
        )
        .child(
            row()
                .gap(panes::dp(6.))
                .child(div().flex_1())
                .children(
                    existing.map(|id| act("comment-delete", "Delete", Act::DeleteComment(id))),
                )
                .child(act("comment-cancel", "Cancel", Act::CancelComment))
                .child(act("comment-save", "Save comment (Enter)", Act::SaveComment).primary()),
        )
        .into_any_element()
}

impl Adeline {
    pub(crate) fn changes_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let isl = theme::islands(cx);
        let focused = self
            .changes
            .search
            .focus_handle(cx)
            .contains_focused(window, cx);
        let changed_only = self.changes.changed_only;
        let header = row()
            .h(panes::dp(44.))
            .flex_shrink_0()
            .pl(panes::dp(18.))
            .pr(panes::dp(10.))
            .gap(panes::dp(6.))
            .child(
                div()
                    .text_size(panes::dp(theme::T_BODY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(isl.fg)
                    .child("Changes"),
            )
            .child(div().flex_1())
            .child(
                panes::icon_button(
                    "changes-changed-only",
                    Icon::default().path("git-diff.svg"),
                    cx,
                )
                .toggled(changed_only)
                .accessibility_label("Changed only (Ctrl+Alt+C)")
                .tooltip("Changed only (Ctrl+Alt+C)")
                .on_click(cx.listener(|app, _, window, cx| {
                    cx.stop_propagation();
                    app.act(Action::Changes(Act::ChangedOnly), window, cx);
                })),
            );
        let search = row()
            .id("changes-search-field")
            .w_full()
            .h(panes::dp(32.))
            .pl(panes::dp(10.))
            .pr(px(1.))
            .rounded(panes::dp(8.))
            .border_1()
            .border_color(if focused { isl.ring_strong } else { isl.ring })
            .bg(isl.chip)
            .child(icon("search").size(panes::dp(14.)).text_color(isl.fg3))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .capture_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "down" && !event.keystroke.modifiers.modified() {
                            cx.stop_propagation();
                            app.changes_focus_tree(window, cx);
                        }
                    }))
                    .child(
                        Input::new(&self.changes.search)
                            .cleanable(true)
                            .appearance(false)
                            .aria_label("Search files"),
                    ),
            );
        col()
            .w_full()
            .min_w_0()
            .h_full()
            .child(self.project_switcher(cx))
            .child(header)
            .child(
                div()
                    .w_full()
                    .px(panes::dp(8.))
                    .pb(panes::dp(8.))
                    .child(search),
            )
            .child(self.changes_tree(cx))
            .into_any_element()
    }

    fn changes_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let isl = theme::islands(cx);
        let project = self.changes_current();
        let rows = project.map_or(0, |p| p.rows.len());
        let note = project.and_then(|p| p.error.clone()).or_else(|| {
            let project = project?;
            (project.loaded && project.rows.is_empty()).then(|| {
                if self.changes.changed_only {
                    "No uncommitted changes.".to_owned()
                } else {
                    "No files match.".to_owned()
                }
            })
        });
        col()
            .id("changes-tree-panel")
            .role(Role::Group)
            .aria_label("Changes")
            .w_full()
            .flex_1()
            .min_h_0()
            .children(note.map(|note| {
                div()
                    .px(panes::dp(12.))
                    .py(panes::dp(8.))
                    .text_size(panes::dp(theme::T_UI))
                    .text_color(isl.fg3)
                    .child(note)
            }))
            .child(
                div()
                    .id("changes-tree")
                    .test_support()
                    .role(Role::Tree)
                    .aria_label("Files tree")
                    .flex_1()
                    .min_h_0()
                    .track_focus(&self.changes.tree_focus)
                    .key_context("ChangesTree")
                    .on_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                        app.changes_tree_key(event, window, cx);
                    }))
                    .child(
                        uniform_list(
                            "changes-tree-rows",
                            rows,
                            cx.processor(|app, range: std::ops::Range<usize>, window, cx| {
                                range
                                    .map(|ix| app.changes_row(ix, window, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.changes.tree_scroll)
                        .size_full()
                        .pb(panes::dp(8.)),
                    ),
            )
            .into_any_element()
    }

    fn changes_row(&self, ix: usize, window: &Window, cx: &Context<Self>) -> AnyElement {
        let isl = theme::islands(cx);
        let Some(project) = self.changes_current() else {
            return div().into_any_element();
        };
        let Some(row_data) = project.rows.get(ix) else {
            return div().into_any_element();
        };
        let path = row_data.path.clone();
        let folder = row_data.folder;
        let name = adeline_docs::file_name(&path).to_owned();
        let open = project.open.as_deref() == Some(path.as_str());
        let cursor = project.cursor.as_deref() == Some(path.as_str())
            && self.changes.tree_focus.contains_focused(window, cx);
        let change: Option<&Change> = (!folder).then(|| project.change(&path)).flatten();
        let label = change.map_or_else(
            || name.clone(),
            |change| {
                let from = change
                    .old_path
                    .as_ref()
                    .map(|old| format!(", from {old}"))
                    .unwrap_or_default();
                format!(
                    "{name}, {}, {}{from}",
                    status_name(change.status),
                    counts(change.added, change.removed)
                )
            },
        );
        let chevron = if folder {
            let chevron = Icon::default()
                .path("chevron.svg")
                .size(panes::dp(12.))
                .text_color(isl.fg3);
            if row_data.open {
                chevron.into_any_element()
            } else {
                chevron
                    .rotate(Radians(-std::f32::consts::FRAC_PI_2))
                    .into_any_element()
            }
        } else {
            div().w(panes::dp(12.)).flex_shrink_0().into_any_element()
        };
        let color = change.map_or(if open { isl.fg } else { isl.fg2 }, |c| {
            status_color(c.status, cx)
        });
        let act = if folder {
            Act::Folder(path.clone())
        } else {
            Act::Open(path.clone())
        };
        let menu_path = path.clone();
        let item = row()
            .id(SharedString::from(format!("change:{path}")))
            .test_support()
            .role(Role::TreeItem)
            .aria_label(label)
            .aria_selected(open)
            .when(folder, |row| row.aria_expanded(row_data.open))
            .h(panes::dp(30.))
            .pl(panes::dp(8. + 14. * row_data.depth as f32))
            .pr(panes::dp(8.))
            .gap(panes::dp(6.))
            .rounded(panes::dp(7.))
            .text_size(panes::dp(theme::T_UI))
            .text_color(color)
            .when(open, |row| row.bg(isl.sel))
            .when(!open, |row| row.hover(|style| style.bg(isl.hover)))
            .when(cursor, |row| row.border_1().border_color(isl.ring_strong))
            .child(chevron)
            .child(
                icon(if folder { "folder" } else { "file" })
                    .size(panes::dp(14.))
                    .text_color(isl.fg3),
            )
            .child(
                row()
                    .flex_1()
                    .min_w_0()
                    .gap(panes::dp(6.))
                    .child(div().min_w_0().truncate().child(name))
                    .children(change.and_then(|c| c.old_path.clone()).map(|old| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(isl.fg3)
                            .child(format!("← {old}"))
                    })),
            )
            .children(change.map(|c| {
                div()
                    .flex_shrink_0()
                    .text_size(panes::dp(theme::T_CAPTION))
                    .text_color(isl.fg3)
                    .child(counts(c.added, c.removed))
            }))
            .on_click(cx.listener(move |app, event: &ClickEvent, window, cx| {
                app.act(Action::Changes(act.clone()), window, cx);
                if !folder && event.click_count() == 2 {
                    app.act(
                        Action::Changes(Act::CommentFile(Some(path.clone()))),
                        window,
                        cx,
                    );
                }
            }));
        let owner = cx.weak_entity();
        div()
            .w_full()
            .px(panes::dp(8.))
            .pb(px(1.))
            .child(if folder {
                item.into_any_element()
            } else {
                item.context_menu(move |menu, _, _| {
                    let (owner, path) = (owner.clone(), menu_path.clone());
                    menu.item(PopupMenuItem::new("Comment on file").on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |app, cx| {
                                app.act(
                                    Action::Changes(Act::CommentFile(Some(path.clone()))),
                                    window,
                                    cx,
                                );
                            });
                        },
                    ))
                })
                .into_any_element()
            })
            .into_any_element()
    }

    fn changes_focus_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.changes_project();
        let listed = project
            .cursor
            .as_ref()
            .is_some_and(|cursor| project.rows.iter().any(|row| &row.path == cursor));
        if !listed {
            project.cursor = project.rows.first().map(|row| row.path.clone());
        }
        window.focus(&self.changes.tree_focus, cx);
        cx.notify();
    }

    fn changes_tree_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        let Some(project) = self.changes_current() else {
            return;
        };
        let rows = &project.rows;
        if rows.is_empty() {
            return;
        }
        let at = project
            .cursor
            .as_ref()
            .and_then(|c| rows.iter().position(|r| &r.path == c));
        let pick = |ix: usize| rows[ix.min(rows.len() - 1)].path.clone();
        let current = at.map(|ix| rows[ix].clone());
        let next = match event.keystroke.key.as_str() {
            "down" => pick(at.map_or(0, |ix| ix + 1)),
            "up" => pick(at.map_or(0, |ix| ix.saturating_sub(1))),
            "home" => pick(0),
            "end" => pick(rows.len() - 1),
            "right" | "left" | "enter" | "space" => {
                let Some(row) = current else {
                    return;
                };
                let key = event.keystroke.key.as_str();
                let act = match key {
                    "right" | "left" if row.folder && row.open == (key == "left") => {
                        Act::Folder(row.path)
                    }
                    "enter" | "space" if row.folder => Act::Folder(row.path),
                    "enter" | "space" => Act::Open(row.path),
                    _ => return,
                };
                cx.stop_propagation();
                self.act(Action::Changes(act), window, cx);
                if row.folder || key == "space" {
                    window.focus(&self.changes.tree_focus, cx);
                }
                return;
            }
            _ => return,
        };
        cx.stop_propagation();
        if let Some(ix) = rows.iter().position(|r| r.path == next) {
            self.changes
                .tree_scroll
                .scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        self.changes_project().cursor = Some(next);
        cx.notify();
    }

    pub(crate) fn changes_card(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let isl = theme::islands(cx);
        div()
            .size_full()
            .px(panes::dp(12.))
            .child(
                div()
                    .size_full()
                    .rounded(panes::dp(14.))
                    .bg(isl.card)
                    .overflow_hidden()
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), isl.ring)
                            .blur_radius(px(0.))
                            .spread_radius(px(1.)),
                    ])
                    .child(self.changes_main(window, cx)),
            )
            .into_any_element()
    }

    fn changes_message(&self, id: &'static str, text: String, cx: &App) -> AnyElement {
        col()
            .id("changes-panel")
            .role(Role::Main)
            .aria_label("Changes")
            .size_full()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .id(id)
                    .test_support()
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child(text),
            )
            .into_any_element()
    }

    fn changes_main(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(project) = self.changes_current() else {
            return self.changes_message("changes-empty", "Loading…".into(), cx);
        };
        match project.listed.repo {
            Repo::NotGit if project.loaded => {
                return self.changes_message(
                    "changes-not-git",
                    "This project isn't a git repository.".into(),
                    cx,
                );
            }
            Repo::NoGit if project.loaded => {
                let machine = if project.machine == machines::LOCAL {
                    "this computer".to_owned()
                } else {
                    project.machine.clone()
                };
                return self.changes_message(
                    "changes-no-git",
                    format!("Git isn't installed on {machine}."),
                    cx,
                );
            }
            _ => {}
        }
        let Some((path, body)) = project
            .body
            .as_ref()
            .filter(|(p, _)| project.open.as_ref() == Some(p))
        else {
            let text = if project.loaded && project.listed.changes.is_empty() {
                "No uncommitted changes. Pick any file to comment on it."
            } else {
                "Pick a file from the tree."
            };
            return self.changes_message("changes-empty", text.into(), cx);
        };
        let change = project.change(path);
        let editing = self
            .changes
            .editing
            .as_ref()
            .filter(|d| d.path == *path && d.key == self.workspace().key());
        let content = match body {
            Body::Loading => self.changes_message("changes-loading", "Opening…".into(), cx),
            Body::Failed(error) => self.changes_message("changes-failed", error.clone(), cx),
            Body::Binary => self.changes_message(
                "changes-binary",
                if change.is_some() {
                    "Binary file changed".into()
                } else {
                    "Binary file".into()
                },
                cx,
            ),
            Body::TooLarge => self.changes_message(
                "changes-too-large",
                match change {
                    Some(c) => format!("Diff too large ({})", counts(c.added, c.removed)),
                    None => "File too large to show".into(),
                },
                cx,
            ),
            Body::Lines { .. } => self.changes_diff(path, window, cx),
        };
        let docked = editing.filter(|_| !matches!(body, Body::Lines { .. }));
        col()
            .id("changes-file")
            .role(Role::Main)
            .aria_label(path.clone())
            .size_full()
            .child(self.changes_toolbar(path, change, cx))
            .child(div().flex_1().min_h_0().child(content))
            .children(docked.map(|draft| {
                comment_box(&self.changes.comment, &cx.weak_entity(), draft.existing, cx)
            }))
            .into_any_element()
    }

    fn changes_toolbar(&self, path: &str, change: Option<&Change>, cx: &Context<Self>) -> Div {
        let isl = theme::islands(cx);
        let split = config::with(|s| s.modes.changes.side_by_side);
        row()
            .flex_shrink_0()
            .h(panes::dp(44.))
            .px(panes::dp(14.))
            .gap(panes::dp(8.))
            .border_b_1()
            .border_color(isl.line)
            .text_size(panes::dp(theme::T_UI))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(isl.fg)
                    .child(path.to_owned()),
            )
            .children(change.map(|c| {
                div()
                    .flex_shrink_0()
                    .text_color(status_color(c.status, cx))
                    .child(format!(
                        "{} {}",
                        status_name(c.status),
                        counts(c.added, c.removed)
                    ))
            }))
            .child(
                panes::icon_button(
                    "changes-comment-file",
                    Icon::default().path("note-pencil.svg"),
                    cx,
                )
                .accessibility_label("Comment on file")
                .on_click({
                    let path = path.to_owned();
                    cx.listener(move |app, _, window, cx| {
                        let act = Act::CommentFile(Some(path.clone()));
                        app.act(Action::Changes(act), window, cx);
                    })
                })
                .tooltip("Comment on file (Ctrl+Alt+M)"),
            )
            .child(
                self.button(
                    "changes-side-by-side",
                    "Side by side",
                    Action::Changes(Act::SideBySide),
                    cx,
                )
                .ghost()
                .toggled(split)
                .tooltip("Side by side (Ctrl+Shift+V)"),
            )
    }

    fn changes_diff(&self, path: &str, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let key = self.workspace().key();
        let comments: Vec<&Comment> = self
            .changes
            .comments
            .iter()
            .filter(|c| c.key == key && c.path == path)
            .collect();
        let mut marked: HashMap<ElementId, u64> = HashMap::new();
        let markers: Vec<DiffGutterMarker> = comments
            .iter()
            .filter_map(|comment| {
                let range = comment.range.as_ref()?;
                let id = ElementId::Name(format!("comment-marker:{}", comment.id).into());
                marked.insert(id.clone(), comment.id);
                let mut label = format!("Comment: {}", first_words(&comment.text));
                if comment.stale {
                    label.push_str(", lines changed");
                }
                Some(
                    DiffGutterMarker::new(
                        id,
                        DiffLinePosition::new(
                            path.to_owned(),
                            range.side(),
                            range.start().min(range.end()),
                        ),
                    )
                    .with_label(label),
                )
            })
            .collect();
        let stale: Vec<u64> = comments.iter().filter(|c| c.stale).map(|c| c.id).collect();
        let draft = self
            .changes
            .editing
            .as_ref()
            .filter(|d| d.path == path && d.key == key);
        let annotation = draft.map(|draft| match &draft.range {
            Some(range) => DiffAnnotation::line(
                "comment-draft",
                DiffLinePosition::new(
                    path.to_owned(),
                    range.end_side(),
                    if range.side() == range.end_side() {
                        range.start().max(range.end())
                    } else {
                        range.end()
                    },
                ),
            ),
            None => DiffAnnotation::file("comment-draft", path.to_owned()),
        });
        let existing = draft.and_then(|d| d.existing);
        let comment = self.changes.comment.clone();
        let box_owner = owner.clone();
        let marker_owner = owner.clone();
        let add_owner = owner;
        div()
            .id("changes-diff")
            .test_support()
            .role(Role::Document)
            .aria_label(format!("{path}, diff"))
            .size_full()
            .child(
                Diff::new(&self.changes.diff)
                    .size_full()
                    .header_visible(false)
                    .annotations(annotation)
                    .render_annotation(move |_, _, cx| {
                        comment_box(&comment, &box_owner, existing, cx)
                    })
                    .gutter_markers(markers)
                    .render_gutter_marker(move |marker, _, cx| {
                        let id = marked.get(marker.id()).copied().unwrap_or_default();
                        let owner = marker_owner.clone();
                        let warn = stale.contains(&id);
                        Button::new(marker.id().clone())
                            .ghost()
                            .xsmall()
                            .icon(Icon::default().path("note-pencil.svg"))
                            .when(warn, |button| button.text_color(cx.theme().warning))
                            .accessibility_label(marker.label().clone())
                            .tooltip(marker.label().clone())
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                let _ = owner.update(cx, |app, cx| {
                                    app.act(Action::Changes(Act::EditComment(id)), window, cx);
                                });
                            })
                    })
                    .on_add_annotation(move |range, window, cx| {
                        let _ = add_owner.update(cx, |app, cx| {
                            app.act(
                                Action::Changes(Act::CommentRange(range.clone())),
                                window,
                                cx,
                            );
                        });
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn comment_cards(&self, cx: &Context<Self>) -> Option<Div> {
        let pending = self.changes_pending();
        if pending.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let mut chips = row().flex_wrap().gap_1p5();
        for comment in pending {
            let id = comment.id;
            let label = card_label(comment);
            let mut title = adeline_docs::file_name(&comment.path).to_owned();
            if !comment.whole_file() {
                title.push(':');
                title.push_str(&comment.span());
            }
            let words = first_words(&comment.text);
            let open = Button::new(SharedString::from(format!("comment-card:{id}")))
                .ghost()
                .xsmall()
                .accessibility_label(label.clone())
                .tooltip(label.clone())
                .child(
                    row()
                        .gap_1p5()
                        .child(icon("note-pencil").size(rems(0.9)))
                        .child(div().flex_shrink_0().child(title))
                        .child(
                            div()
                                .max_w(rems(12.))
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(format!("· {words}")),
                        )
                        .when(comment.stale, |chip| {
                            chip.child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(theme.warning)
                                    .child("lines changed"),
                            )
                        }),
                )
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.act(Action::Changes(Act::EditComment(id)), window, cx);
                }));
            let remove = Button::new(SharedString::from(format!("remove-comment:{id}")))
                .ghost()
                .xsmall()
                .icon(Icon::default().path("close.svg"))
                .accessibility_label(format!("Remove {label}"))
                .tooltip("Remove comment")
                .on_click(cx.listener(move |app, _, window, cx| {
                    cx.stop_propagation();
                    app.act(Action::Changes(Act::DeleteComment(id)), window, cx);
                }));
            chips = chips.child(
                row()
                    .h(rems(2.))
                    .px_1()
                    .gap_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(if comment.stale {
                        theme.warning
                    } else {
                        theme.border
                    })
                    .bg(theme.background)
                    .text_size(panes::dp(theme::T_CAPTION))
                    .child(open)
                    .child(remove),
            );
        }
        Some(chips)
    }

    pub(crate) fn changes_summary(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.changes_on() {
            return None;
        }
        let (added, removed) = self.changes_current()?.totals()?;
        let isl = theme::islands(cx);
        let text = counts(added, removed);
        Some(
            div()
                .px(panes::dp(8.))
                .pb(panes::dp(6.))
                .child(
                    Button::new("changes-summary")
                        .ghost()
                        .w_full()
                        .h(panes::dp(30.))
                        .px(panes::dp(8.))
                        .rounded(panes::dp(7.))
                        .accessibility_label(format!("Changes {text}"))
                        .tooltip("Review the uncommitted changes")
                        .child(
                            row()
                                .w_full()
                                .gap(panes::dp(8.))
                                .text_size(panes::dp(theme::T_UI))
                                .child(
                                    Icon::default()
                                        .path("git-diff.svg")
                                        .size(panes::dp(14.))
                                        .text_color(isl.fg2),
                                )
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(isl.fg)
                                        .child("Changes"),
                                )
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .text_color(cx.theme().success)
                                        .child(format!("+{added}")),
                                )
                                .child(
                                    div()
                                        .text_color(cx.theme().danger)
                                        .child(format!("−{removed}")),
                                ),
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            app.act(Action::Changes(Act::FromSummary), window, cx);
                        })),
                )
                .into_any_element(),
        )
    }
}
