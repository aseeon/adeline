//! Drawing Docs mode: the resizable tree, doc and conversation panels, the
//! doc's toolbar island and banners, the dialogs, the conversation chooser
//! in the chat header, and the doc cards under a reply.

// Rust guideline compliant 2026-10-09

use super::{Act, Doc, Then, viewer};
use crate::*;
use adeline_core::card::Card;
use adeline_docs::Format;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::command::{Command, CommandItem};
use gpui_kit::component::input::Editor;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};

// In px, so it holds at any interface text size.
const MAIN_PANEL_MIN: Pixels = px(320.);
// Cards a reply shows before the rest fold into a list.
const CARD_LIMIT: usize = 4;

struct Tool {
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    action: Act,
}

fn tool(id: &'static str, label: &'static str, icon: &'static str, action: Act) -> Tool {
    Tool {
        id,
        label,
        icon,
        action,
    }
}

fn markdown_tools() -> Vec<Tool> {
    vec![
        tool("undo", "Undo (Ctrl+Z)", "arrow-u-up-left.svg", Act::Undo),
        tool("redo", "Redo (Ctrl+Y)", "arrow-u-up-right.svg", Act::Redo),
        tool(
            "h1",
            "Heading 1 (Ctrl+Alt+1)",
            "text-h-one.svg",
            Act::Format(Format::Heading(1)),
        ),
        tool(
            "h2",
            "Heading 2 (Ctrl+Alt+2)",
            "text-h-two.svg",
            Act::Format(Format::Heading(2)),
        ),
        tool(
            "h3",
            "Heading 3 (Ctrl+Alt+3)",
            "text-h-three.svg",
            Act::Format(Format::Heading(3)),
        ),
        tool(
            "bold",
            "Bold (Ctrl+B)",
            "text-b.svg",
            Act::Format(Format::Bold),
        ),
        tool(
            "italic",
            "Italic (Ctrl+I)",
            "text-italic.svg",
            Act::Format(Format::Italic),
        ),
        tool(
            "strike",
            "Strikethrough (Ctrl+Shift+X)",
            "text-strikethrough.svg",
            Act::Format(Format::Strikethrough),
        ),
        tool(
            "code",
            "Inline code (Ctrl+Alt+C)",
            "code.svg",
            Act::Format(Format::InlineCode),
        ),
        tool(
            "code-block",
            "Code block (Ctrl+Alt+B)",
            "code-block.svg",
            Act::Format(Format::CodeBlock),
        ),
        tool(
            "link",
            "Link (Ctrl+K)",
            "link.svg",
            Act::Format(Format::Link),
        ),
        tool(
            "bullets",
            "Bullet list (Ctrl+Shift+8)",
            "list-bullets.svg",
            Act::Format(Format::BulletList),
        ),
        tool(
            "numbers",
            "Numbered list (Ctrl+Shift+7)",
            "list-numbers.svg",
            Act::Format(Format::NumberedList),
        ),
        tool(
            "tasks",
            "Task list (Ctrl+Shift+9)",
            "list-checks.svg",
            Act::Format(Format::TaskList),
        ),
        tool(
            "quote",
            "Quote (Ctrl+Shift+.)",
            "quotes.svg",
            Act::Format(Format::Quote),
        ),
        tool(
            "table",
            "Table (Ctrl+Alt+T)",
            "table.svg",
            Act::Format(Format::Table),
        ),
        tool("find", "Find and replace (Ctrl+H)", "search.svg", Act::Find),
    ]
}

fn plain_tools() -> Vec<Tool> {
    vec![
        tool("undo", "Undo (Ctrl+Z)", "arrow-u-up-left.svg", Act::Undo),
        tool("redo", "Redo (Ctrl+Y)", "arrow-u-up-right.svg", Act::Redo),
        tool("find", "Find and replace (Ctrl+H)", "search.svg", Act::Find),
    ]
}

fn edited_at(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%b %-d, %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

impl Adeline {
    fn docs_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        icon: &str,
        act: Act,
        cx: &Context<Self>,
    ) -> Button {
        self.icon_button(id, label, Icon::default().path(icon), Action::Docs(act), cx)
    }

    pub(crate) fn docs_panels(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let settings = config::with(|s| s.modes.docs.clone());
        let (tree_width, chat_width) = self
            .docs_current()
            .map_or((super::TREE_WIDTH, super::CHAT_WIDTH), |p| {
                (p.tree_width, p.chat_width)
            });
        let owner = cx.weak_entity();
        h_resizable("docs-panels")
            .with_state(&self.docs.panels)
            .on_resize(move |state, _, cx| {
                let sizes = state.read(cx).sizes().clone();
                let _ = owner.update(cx, |app, cx| {
                    if !app.has_open_project() {
                        return;
                    }
                    let scale = fonts::text_pixels(1.);
                    let project = app.docs_project();
                    if let Some(width) = sizes.first().filter(|w| **w > px(0.)) {
                        project.tree_width = f32::from(*width) / f32::from(scale);
                    }
                    if let Some(width) = sizes.get(2).filter(|w| **w > px(0.)) {
                        project.chat_width = f32::from(*width) / f32::from(scale);
                    }
                    app.docs_record(cx);
                });
            })
            .child(
                resizable_panel()
                    .visible(settings.show_left_panel)
                    .size(fonts::text_pixels(tree_width))
                    .size_range(fonts::text_pixels(160.)..fonts::text_pixels(520.))
                    .flex_grow_0()
                    .flex_shrink_1()
                    .child(self.docs_tree(window, cx)),
            )
            .child(
                resizable_panel()
                    .size_range(MAIN_PANEL_MIN..Pixels::MAX)
                    .child(self.docs_main(window, cx)),
            )
            .child(
                resizable_panel()
                    .visible(settings.show_conversation_panel)
                    .size(fonts::text_pixels(chat_width))
                    .size_range(fonts::text_pixels(260.)..fonts::text_pixels(720.))
                    .flex_grow_0()
                    .flex_shrink_1()
                    .child(
                        div()
                            .size_full()
                            .border_l_1()
                            .border_color(cx.theme().border)
                            .child(self.chats(window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn docs_tree(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let project = self.docs_current();
        let rows = project.map_or(0, |p| p.rows.len());
        let error = project.and_then(|p| p.list_error.clone());
        let empty = project.is_some_and(|p| p.listed && p.rows.is_empty());
        let searching = !self.docs.search.read(cx).value().trim().is_empty();
        col()
            .id("docs-tree-panel")
            .role(Role::Group)
            .aria_label("Docs")
            .size_full()
            .bg(theme.background)
            .child(
                row()
                    .gap_1()
                    .p_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .capture_key_down(cx.listener(
                                |app, event: &KeyDownEvent, window, cx| {
                                    let modifiers = event.keystroke.modifiers;
                                    if event.keystroke.key == "down" && !modifiers.modified() {
                                        cx.stop_propagation();
                                        app.docs_focus_tree(window, cx);
                                    }
                                },
                            ))
                            .child(search_field(
                                Input::new(&self.docs.search)
                                    .small()
                                    .cleanable(true)
                                    .aria_label("Search docs"),
                                &self.docs.search,
                                window,
                                cx,
                            )),
                    )
                    .child(self.docs_button(
                        "doc-new",
                        "New doc (Ctrl+Alt+N)",
                        "file-plus.svg",
                        Act::New,
                        cx,
                    )),
            )
            .children(
                error.map(|error| div().px_3().text_sm().text_color(theme.danger).child(error)),
            )
            .when(empty, |panel| {
                panel.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if searching {
                            "No docs match."
                        } else {
                            "This project has no docs yet."
                        }),
                )
            })
            .child(
                div()
                    .id("docs-tree")
                    .role(Role::Tree)
                    .aria_label("Docs tree")
                    .flex_1()
                    .min_h_0()
                    .track_focus(&self.docs.tree_focus)
                    .key_context("DocsTree")
                    .on_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                        app.docs_tree_key(event, window, cx);
                    }))
                    .child(
                        uniform_list(
                            "docs-tree-rows",
                            rows,
                            cx.processor(|app, range: std::ops::Range<usize>, window, cx| {
                                range
                                    .map(|ix| app.docs_row(ix, window, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.docs.tree_scroll)
                        .size_full()
                        .pb_2(),
                    ),
            )
            .into_any_element()
    }

    fn docs_row(&self, ix: usize, window: &Window, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let Some(project) = self.docs_current() else {
            return div().into_any_element();
        };
        let Some(row_data) = project.rows.get(ix) else {
            return div().into_any_element();
        };
        let path = row_data.path.clone();
        let name = adeline_docs::file_name(&path).to_owned();
        let open = project.open.as_deref() == Some(path.as_str());
        let cursor = project.cursor.as_deref() == Some(path.as_str())
            && self.docs.tree_focus.contains_focused(window, cx);
        let dirty = !row_data.folder && project.files.get(&path).is_some_and(|d| d.dirty);
        let icon_path = if row_data.folder { "folder" } else { "file" };
        let act = if row_data.folder {
            Act::Folder(path.clone())
        } else {
            Act::Open(path.clone())
        };
        let mut label = name.clone();
        if dirty {
            label.push_str(", unsaved");
        }
        div()
            .w_full()
            .px_1p5()
            .child(
                row()
                    .id(SharedString::from(format!("doc:{path}")))
                    .role(Role::TreeItem)
                    .aria_label(label)
                    .aria_selected(open)
                    .when(row_data.folder, |row| row.aria_expanded(row_data.open))
                    .h(rems(chat::ROW_HEIGHT))
                    .pl(rems(0.5 + 0.875 * row_data.depth as f32))
                    .pr_2()
                    .gap_2()
                    .rounded(theme.radius)
                    .text_sm()
                    .when(open, |row| row.bg(theme.sidebar_primary))
                    .when(!open, |row| row.hover(|style| style.bg(theme.secondary)))
                    .when(cursor, |row| row.border_1().border_color(theme.ring))
                    .child(
                        icon(icon_path)
                            .size(rems(0.875))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(if open || row_data.folder {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(name),
                    )
                    .when(dirty, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .size(rems(0.5))
                                .rounded_full()
                                .bg(theme.primary),
                        )
                    })
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(Action::Docs(act.clone()), window, cx);
                    })),
            )
            .into_any_element()
    }

    fn docs_focus_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.docs_project();
        let listed = project
            .cursor
            .as_ref()
            .is_some_and(|cursor| project.rows.iter().any(|row| &row.path == cursor));
        if !listed {
            project.cursor = project.rows.first().map(|row| row.path.clone());
        }
        window.focus(&self.docs.tree_focus, cx);
        cx.notify();
    }

    fn docs_tree_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        let Some(project) = self.docs_current() else {
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
            "down" => Some(pick(at.map_or(0, |ix| ix + 1))),
            "up" => Some(pick(at.map_or(0, |ix| ix.saturating_sub(1)))),
            "home" => Some(pick(0)),
            "end" => Some(pick(rows.len() - 1)),
            "right" | "left" | "enter" | "space" | "f2" | "delete" => {
                let Some(row) = current else {
                    return;
                };
                let key = event.keystroke.key.as_str();
                let act = match key {
                    "right" if row.folder && !row.open => Act::Folder(row.path),
                    "left" if row.folder && row.open => Act::Folder(row.path),
                    "left" => {
                        let parent = adeline_docs::parent(&row.path);
                        if parent.is_empty() {
                            return;
                        }
                        self.docs_project().cursor = Some(parent.to_owned());
                        cx.notify();
                        return;
                    }
                    "enter" | "space" if row.folder => Act::Folder(row.path),
                    "enter" | "space" => Act::Open(row.path),
                    "f2" if !row.folder => Act::Rename(Some(row.path)),
                    "delete" if !row.folder => Act::Delete(Some(row.path)),
                    _ => return,
                };
                cx.stop_propagation();
                self.act(Action::Docs(act), window, cx);
                return;
            }
            _ => return,
        };
        cx.stop_propagation();
        if let Some(path) = next {
            if let Some(ix) = rows.iter().position(|r| r.path == path) {
                self.docs
                    .tree_scroll
                    .scroll_to_item(ix, ScrollStrategy::Nearest);
            }
            self.docs_project().cursor = Some(path);
        }
        cx.notify();
    }

    fn docs_main(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let project = self.docs_current();
        let failed = project.and_then(|p| p.failed.clone());
        let centered = |child: AnyElement| {
            col()
                .id("doc-panel")
                .role(Role::Main)
                .aria_label("Doc")
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .bg(theme.background)
                .child(child)
                .into_any_element()
        };
        if let Some((path, error)) = failed {
            let message = if error.contains("no longer exists") {
                format!("{path} no longer exists")
            } else {
                format!("Could not open {path}: {error}")
            };
            return centered(
                col()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("doc-missing")
                            .role(Role::Status)
                            .aria_label(message.clone())
                            .text_color(theme.muted_foreground)
                            .child(message),
                    )
                    .child(self.button(
                        "doc-missing-close",
                        "Close",
                        Action::Docs(Act::CloseDoc),
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        let Some((path, doc)) = self.docs_open_doc() else {
            return centered(
                col()
                    .items_center()
                    .gap_3()
                    .child(div().text_color(theme.muted_foreground).child(
                        if project.and_then(|p| p.open.as_ref()).is_some() {
                            "Opening…"
                        } else {
                            "Open a doc from the tree, or start a new one."
                        },
                    ))
                    .child(self.button("doc-empty-new", "New doc…", Action::Docs(Act::New), cx))
                    .into_any_element(),
            );
        };
        if doc.modified == 0 {
            return centered(
                div()
                    .text_color(theme.muted_foreground)
                    .child("Opening…")
                    .into_any_element(),
            );
        }
        let path = path.to_owned();
        let markdown = adeline_docs::is_markdown(&path);
        let offline = !self.demo_mode && !client::usable(&self.workspace().machine, cx);
        let locked = doc.read_only.is_some() || offline;
        let content = if !doc.editor && markdown {
            let owner = cx.weak_entity();
            div()
                .id("doc-viewer")
                .role(Role::Document)
                .aria_label(format!("{path}, Viewer"))
                .relative()
                .flex_1()
                .min_h_0()
                .px_6()
                .on_mouse_up(MouseButton::Left, move |event, _, cx| {
                    let at = event.position;
                    let _ = owner.update(cx, |app, cx| {
                        let selected = !app.docs.viewer.read(cx).selected_text().is_empty();
                        app.docs.note_at = selected.then_some(at);
                        cx.notify();
                    });
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _, _, cx| {
                        if app.docs.note_at.take().is_some() {
                            cx.notify();
                        }
                    }),
                )
                .child(
                    viewer::markdown(
                        &self.docs.viewer,
                        &path,
                        &self.docs.images,
                        config::with(|s| s.modes.docs.load_remote_images),
                    )
                    .size_full(),
                )
                .into_any_element()
        } else {
            let owner = cx.weak_entity();
            div()
                .id("doc-editor")
                .flex_1()
                .min_h_0()
                .key_context("DocsEditor")
                .on_mouse_up(MouseButton::Left, move |event, _, cx| {
                    let at = event.position;
                    let _ = owner.update(cx, |app, cx| {
                        let viewer = app.docs_open_doc().is_some_and(|(_, d)| !d.editor);
                        let selected = !app.docs.editor.read(cx).selected_range().is_empty();
                        app.docs.note_at = (viewer && selected).then_some(at);
                        cx.notify();
                    });
                })
                .child(
                    Editor::new(&self.docs.editor)
                        .size_full()
                        .bordered(false)
                        .readonly(locked || !doc.editor)
                        .aria_label(if doc.editor {
                            format!("{path}, Editor")
                        } else {
                            format!("{path}, Viewer")
                        }),
                )
                .into_any_element()
        };
        let note = self.docs.note_at.map(|at| {
            deferred(
                anchored()
                    .position(at + point(px(8.), px(8.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        Button::new("doc-note-selection")
                            .small()
                            .primary()
                            .icon(Icon::default().path("note-pencil.svg"))
                            .label("Note on selection")
                            .tooltip("Send a note about the selection to the chat (Ctrl+Shift+N)")
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.act(Action::Docs(Act::Note), window, cx);
                            })),
                    ),
            )
            .with_priority(1)
        });
        let _ = window;
        col()
            .id("doc-panel")
            .role(Role::Main)
            .aria_label(path.clone())
            .size_full()
            .bg(theme.background)
            .child(self.docs_toolbar(&path, doc, locked, cx))
            .children(self.docs_banners(doc, offline, cx))
            .child(content)
            .children(note)
            .into_any_element()
    }

    fn docs_toolbar(&self, path: &str, doc: &Doc, locked: bool, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let markdown = adeline_docs::is_markdown(path);
        let tools = if !doc.editor || locked {
            Vec::new()
        } else if markdown {
            markdown_tools()
        } else {
            plain_tools()
        };
        let view_label = if doc.editor {
            "Show in the Viewer (Ctrl+Shift+V)"
        } else {
            "Edit in the Editor (Ctrl+Shift+V)"
        };
        div()
            .p_2()
            .child(
                row()
                    .id("doc-toolbar")
                    .role(Role::Toolbar)
                    .aria_label("Doc toolbar")
                    .w_full()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.foreground.alpha(0.025))
                    .overflow_x_hidden()
                    .child(
                        row()
                            .flex_1()
                            .min_w(rems(6.))
                            .gap_1p5()
                            .text_sm()
                            .child(
                                div()
                                    .id("doc-path")
                                    .min_w_0()
                                    .truncate()
                                    .tooltip({
                                        let path = path.to_owned();
                                        move |window, cx| {
                                            component::tooltip::Tooltip::new(path.clone())
                                                .build(window, cx)
                                        }
                                    })
                                    .child(path.to_owned()),
                            )
                            .when(doc.dirty, |row| {
                                row.child(
                                    div()
                                        .id("doc-unsaved")
                                        .role(Role::Status)
                                        .aria_label("Unsaved changes")
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(theme.primary)
                                        .child("Unsaved"),
                                )
                            }),
                    )
                    .children(tools.into_iter().map(|tool| {
                        self.docs_button(
                            SharedString::from(format!("doc-tool-{}", tool.id)),
                            tool.label,
                            tool.icon,
                            tool.action,
                            cx,
                        )
                    }))
                    .child(div().w(px(1.)).h(rems(1.25)).mx_1().bg(theme.border))
                    .child(
                        self.docs_button(
                            "doc-view",
                            view_label,
                            if doc.editor { "eye.svg" } else { "pen.svg" },
                            Act::View,
                            cx,
                        )
                        .selected(doc.editor),
                    )
                    .child(
                        self.docs_button(
                            "doc-save",
                            "Save (Ctrl+S)",
                            "floppy-disk.svg",
                            Act::Save,
                            cx,
                        )
                        .disabled(!doc.dirty && !doc.gone || locked),
                    ),
            )
            .into_any_element()
    }

    fn docs_banners(&self, doc: &Doc, offline: bool, cx: &Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme();
        let banner = |id: &'static str, text: String, color: Hsla, buttons: Vec<Button>| {
            row()
                .id(id)
                .role(Role::Alert)
                .aria_label(text.clone())
                .mx_2()
                .mb_2()
                .px_3()
                .py_2()
                .gap_3()
                .rounded_lg()
                .border_1()
                .border_color(color.alpha(0.5))
                .bg(color.alpha(0.08))
                .text_sm()
                .child(div().flex_1().min_w_0().child(text))
                .children(buttons)
                .into_any_element()
        };
        let mut banners = Vec::new();
        if let Some(reason) = &doc.read_only {
            banners.push(banner(
                "doc-read-only",
                reason.clone(),
                theme.warning,
                Vec::new(),
            ));
        }
        if offline {
            banners.push(banner(
                "doc-offline",
                format!(
                    "{} is not connected, so this doc is read-only. Unsaved edits are kept; saving works again once it reconnects.",
                    machines::name(&self.workspace().machine)
                ),
                theme.warning,
                Vec::new(),
            ));
        }
        if doc.conflict.is_some() {
            banners.push(banner(
                "doc-conflict",
                "This doc changed on disk while it had unsaved edits.".into(),
                theme.warning,
                vec![
                    self.button(
                        "doc-keep",
                        "Keep my version",
                        Action::Docs(Act::KeepMine),
                        cx,
                    ),
                    self.button(
                        "doc-take",
                        "Take the version on disk",
                        Action::Docs(Act::TakeTheirs),
                        cx,
                    ),
                ],
            ));
        }
        if doc.gone {
            banners.push(banner(
                "doc-gone",
                "This doc was deleted or moved outside Docs. Its text is still here.".into(),
                theme.danger,
                vec![
                    self.button("doc-recreate", "Save", Action::Docs(Act::Save), cx),
                    self.button("doc-close", "Close", Action::Docs(Act::CloseDoc), cx),
                ],
            ));
        }
        banners
    }

    pub(crate) fn docs_dialog(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let footer = views::dialog_footer;
        let (title, body, buttons): (String, AnyElement, Vec<Button>) = match self.modal {
            Some("doc-new") => (
                "New doc".into(),
                col()
                    .gap_1()
                    .child(
                        Input::new(&self.docs.path_input)
                            .aria_label("Folder and name of the new doc"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Folder and file name, such as notes/plan.md. Without an extension it gets .md."),
                    )
                    .into_any_element(),
                vec![
                    self.button("doc-new-cancel", "Cancel", Action::Close, cx),
                    self.button("doc-new-create", "Create", Action::Docs(Act::Create), cx)
                        .primary(),
                ],
            ),
            Some("doc-rename") => (
                format!(
                    "Rename {}",
                    self.docs.target.as_deref().map_or("", adeline_docs::file_name)
                ),
                Input::new(&self.docs.path_input)
                    .aria_label("New name")
                    .into_any_element(),
                vec![
                    self.button("doc-rename-cancel", "Cancel", Action::Close, cx),
                    self.button("doc-rename-ok", "Rename", Action::Docs(Act::ConfirmRename), cx)
                        .primary(),
                ],
            ),
            Some("doc-delete") => (
                format!(
                    "Delete {}?",
                    self.docs.target.as_deref().map_or("", adeline_docs::file_name)
                ),
                div()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "{} is deleted from disk permanently. This can't be undone.",
                        self.docs.target.clone().unwrap_or_default()
                    ))
                    .into_any_element(),
                vec![
                    self.button("doc-delete-cancel", "Cancel", Action::Close, cx),
                    self.button(
                        "doc-delete-ok",
                        "Delete permanently",
                        Action::Docs(Act::ConfirmDelete),
                        cx,
                    )
                    .danger(),
                ],
            ),
            _ => {
                let unsaved = self.docs.unsaved();
                let names: Vec<String> = unsaved.iter().map(|(_, path)| path.clone()).collect();
                let leaving = if self.docs.then == Some(Then::Quit) {
                    "before Adeline quits"
                } else {
                    "before the project closes"
                };
                (
                    format!(
                        "Save changes to {} doc{}?",
                        names.len(),
                        if names.len() == 1 { "" } else { "s" }
                    ),
                    col()
                        .gap_1()
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format!("These docs have unsaved edits {leaving}:")),
                        )
                        .children(names.into_iter().map(|name| {
                            div()
                                .font_family(theme.mono_font_family.clone())
                                .text_xs()
                                .child(name)
                        }))
                        .into_any_element(),
                    vec![
                        self.button("doc-unsaved-cancel", "Cancel", Action::Close, cx),
                        self.button("doc-unsaved-discard", "Discard", Action::Docs(Act::Discard), cx)
                            .danger(),
                        self.button("doc-unsaved-save", "Save all", Action::Docs(Act::SaveAll), cx)
                            .primary(),
                    ],
                )
            }
        };
        col()
            .id("dialog-content")
            .role(Role::Group)
            .aria_label(title.clone())
            .gap_3()
            .text_sm()
            .child(views::dialog_title(title))
            .child(body)
            .child(footer().children(buttons))
            .into_any_element()
    }

    /// The chat header's title in Docs: a button opening the project's conversations.
    pub(crate) fn chat_chooser(&self, title: &str, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let content_owner = owner.clone();
        let state = self
            .command_popup
            .clone()
            .filter(|_| self.menu == Some("chats"));
        let query = state
            .as_ref()
            .map(|state| state.read(cx).query(cx).to_lowercase())
            .unwrap_or_default();
        let threads = &self.workspace().threads;
        let records: Vec<_> = threads.iter().map(Thread::search_record).collect();
        let criteria = prepared::Criteria {
            query,
            completed: self.show_completed,
            archived: self.show_archived,
            agent: None,
        };
        let new_chat = "new chat".contains(criteria.query.trim());
        let found = prepared::search(&records, &criteria, recency::now());
        let mut entries: Vec<(String, String, Option<String>)> = Vec::new();
        if new_chat {
            entries.push(("New chat".into(), String::new(), None));
        }
        entries.extend(found.groups.iter().flat_map(|(_, rows)| rows).map(|&ix| {
            let thread = &threads[ix];
            (
                thread.title.trim().to_owned(),
                chat::status_name(thread).to_owned(),
                Some(thread.id.clone()),
            )
        }));
        let trigger = Button::new("chat-chooser")
            .ghost()
            .small()
            .label(title.to_owned())
            .icon(Icon::default().path("chevron.svg"))
            .accessibility_label(format!(
                "Conversation: {title}. Choose another (Ctrl+Shift+K)"
            ))
            .tooltip("Choose a conversation (Ctrl+Shift+K)");
        component::popover::Popover::new("chats")
            .anchor(Anchor::TopLeft)
            .trigger(trigger)
            .open(self.menu == Some("chats"))
            .p_0()
            .shadow(views::menu_shadow(cx))
            .when_some(state.as_ref(), |popover, state| {
                popover.track_focus(&state.focus_handle(cx))
            })
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if *open {
                        app.act(Action::Docs(Act::Chooser), window, cx);
                    } else if app.menu == Some("chats") {
                        app.menu = None;
                        app.chat_header.update(cx, |_, cx| cx.notify());
                        cx.notify();
                    }
                });
            })
            .content(move |_, _, cx| {
                let owner = content_owner.clone();
                let query_owner = content_owner.clone();
                let entries = entries.clone();
                let popover = cx.entity();
                col()
                    .w(rems(24.))
                    .when_some(state.as_ref(), |column, state| {
                        column.child(
                            Command::new(state)
                                .bordered(false)
                                .filterable(false)
                                .placeholder("Search conversations")
                                .on_query(move |_, _, cx| {
                                    let _ = query_owner.update(cx, |app, cx| {
                                        app.chat_header.update(cx, |_, cx| cx.notify());
                                        cx.notify();
                                    });
                                })
                                .items(entries.iter().map(|(title, status, _)| {
                                    let (title, status) = (title.clone(), status.clone());
                                    CommandItem::new().label(title.clone()).child(move |_, cx| {
                                        row()
                                            .w_full()
                                            .gap_3()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .child(title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(status.clone()),
                                            )
                                    })
                                }))
                                .on_confirm(move |path, window, cx| {
                                    if let Some((_, _, id)) = entries.get(path.row) {
                                        popover.update(cx, |state, cx| state.dismiss(window, cx));
                                        let id = id.clone();
                                        let _ = owner.update(cx, |app, cx| {
                                            app.act(Action::Docs(Act::Chat(id)), window, cx);
                                        });
                                    }
                                }),
                        )
                    })
            })
            .into_any_element()
    }

    fn card_view(&self, thread: &Thread, card: &Card, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let route = card.entity.to_string();
        let open = Action::OpenCard {
            route: card.entity.clone(),
            chat: thread.id.clone(),
        };
        let path = card.subtitle.clone();
        let muted = card.gone;
        let edited = edited_at(card.edited);
        let detail = if muted {
            format!("Deleted · last edited {edited}")
        } else {
            format!("Edited {edited}")
        };
        row()
            .id(SharedString::from(format!("card:{}:{route}", thread.id)))
            .role(Role::Group)
            .aria_label(format!("{}, {path}, {detail}", card.title))
            .w_full()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.foreground.alpha(0.025))
            .when(muted, |row| row.opacity(0.6))
            .child(icon("file").text_color(theme.muted_foreground))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .truncate()
                            .when(muted, |title| title.line_through())
                            .child(card.title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "card-path:{}:{route}",
                                thread.id
                            )))
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .truncate()
                            .tooltip({
                                let path = path.clone();
                                move |window, cx| {
                                    component::tooltip::Tooltip::new(path.clone()).build(window, cx)
                                }
                            })
                            .child(path.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(detail),
                    ),
            )
            .child(
                self.button(
                    SharedString::from(format!("card-open:{}:{route}", thread.id)),
                    "Open",
                    open,
                    cx,
                )
                .primary()
                .accessibility_label(format!("Open {path}")),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "card-copy:{}:{route}",
                    thread.id
                )))
                .ghost()
                .small()
                .icon(Icon::default().path("copy.svg"))
                .accessibility_label(format!("Copy the path of {path}"))
                .tooltip("Copy path")
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                }),
            )
            .into_any_element()
    }

    fn card_link(&self, thread: &Thread, card: &Card, cx: &Context<Self>) -> AnyElement {
        let route = card.entity.to_string();
        let action = Action::OpenCard {
            route: card.entity.clone(),
            chat: thread.id.clone(),
        };
        div()
            .id(SharedString::from(format!(
                "card-link:{}:{route}",
                thread.id
            )))
            .role(Role::Link)
            .aria_label(card.subtitle.clone())
            .text_sm()
            .text_color(cx.theme().link)
            .when(card.gone, |link| link.line_through())
            .cursor_pointer()
            .child(card.subtitle.clone())
            .on_click(cx.listener(move |app, _, window, cx| {
                app.act(action.clone(), window, cx);
            }))
            .into_any_element()
    }

    /// The cards of the turn that reply `i` ends: the 4 latest, then a "+N more docs" list.
    pub(crate) fn turn_cards(&self, thread: &Thread, i: usize, cx: &Context<Self>) -> Option<Div> {
        let turn = thread.turn_of(i);
        let mut cards: Vec<&Card> = thread.cards.iter().filter(|c| c.turn == turn).collect();
        if cards.is_empty() {
            return None;
        }
        cards.sort_by_key(|card| std::cmp::Reverse(card.edited));
        let enabled = config::with(|s| s.general.features.docs);
        let mut column = col().mt_3().gap_2();
        if !enabled {
            return Some(
                column.children(cards.iter().map(|card| self.card_link(thread, card, cx))),
            );
        }
        column = column.children(
            cards
                .iter()
                .take(CARD_LIMIT)
                .map(|card| self.card_view(thread, card, cx)),
        );
        let rest = &cards[cards.len().min(CARD_LIMIT)..];
        if !rest.is_empty() {
            let key = format!("{}:{}", thread.id, turn.unwrap_or_default());
            let open = self.docs.more_open(&key);
            column = column.child(
                Button::new(SharedString::from(format!("cards-more:{key}")))
                    .ghost()
                    .small()
                    .label(format!("+{} more docs", rest.len()))
                    .selected(open)
                    .accessibility_label(format!(
                        "{} more docs, {}",
                        rest.len(),
                        if open { "shown" } else { "hidden" }
                    ))
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(Action::Docs(Act::More(key.clone())), window, cx);
                    })),
            );
            if open {
                column = column.child(
                    col()
                        .pl_3()
                        .gap_1()
                        .children(rest.iter().map(|card| self.card_link(thread, card, cx))),
                );
            }
        }
        Some(column)
    }
}
