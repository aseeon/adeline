//! MCP servers in Settings and the agent form: one row per server with Edit…
//! and Remove, and the editor dialog (DD11).
use super::*;
use crate::conversation::{McpServer, McpTransport};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    form::{Field, Form},
    input::{Input, InputState, Textarea, TextareaState},
    tab::{Tab, TabBar},
};
use std::rc::Rc;

/// Saves an edited server; an error keeps the dialog open and shows it.
pub(super) type Save = Rc<dyn Fn(McpServer, &mut Window, &mut App) -> Result<(), String>>;

pub(super) struct McpEditor {
    name: Entity<InputState>,
    /// 0 Command, 1 HTTP.
    kind: usize,
    command: Entity<InputState>,
    arguments: Entity<TextareaState>,
    environment: Entity<TextareaState>,
    url: Entity<InputState>,
    headers: Entity<TextareaState>,
    error: Option<String>,
}

fn field(
    value: &str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut input = InputState::new(window, cx).placeholder(placeholder);
        input.set_value(value.to_owned(), window, cx);
        input
    })
}

fn lines(
    value: String,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TextareaState> {
    cx.new(|cx| {
        let mut input = TextareaState::new(window, cx)
            .auto_grow(2, 6)
            .placeholder(placeholder);
        input.set_value(value, window, cx);
        input
    })
}

fn pairs_text(pairs: &[(String, String)], separator: &str) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{name}{separator}{value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `NAME=value` (or `Name: value`) lines as pairs.
fn parse_pairs(text: &str, separator: char, what: &str) -> Result<Vec<(String, String)>, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line
                .split_once(separator)
                .ok_or_else(|| format!("Write each {what} as name{separator}value: {line}"))?;
            Ok((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

impl McpEditor {
    pub(super) fn new(
        server: Option<&McpServer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (kind, command, arguments, environment, url, headers) =
            match server.map(|s| &s.transport) {
                Some(McpTransport::Http { url, headers }) => (
                    1,
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    url.clone(),
                    headers.clone(),
                ),
                Some(McpTransport::Stdio {
                    command,
                    arguments,
                    environment,
                }) => (
                    0,
                    command.to_string_lossy().into_owned(),
                    arguments.clone(),
                    environment.clone(),
                    String::new(),
                    Vec::new(),
                ),
                None => (
                    0,
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    String::new(),
                    Vec::new(),
                ),
            };
        Self {
            name: field(
                server.map_or("", |s| s.name.as_str()),
                "docs-search",
                window,
                cx,
            ),
            kind,
            command: field(&command, "npx-free command, e.g. uvx or a path", window, cx),
            arguments: lines(arguments.join("\n"), "One argument per line", window, cx),
            environment: lines(
                pairs_text(&environment, "="),
                "NAME=value, one per line",
                window,
                cx,
            ),
            url: field(&url, "https://example.com/mcp", window, cx),
            headers: lines(
                pairs_text(&headers, ": "),
                "Name: value, one per line",
                window,
                cx,
            ),
            error: None,
        }
    }

    fn value(&self, cx: &App) -> Result<McpServer, String> {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err("Name is required.".into());
        }
        let transport = if self.kind == 0 {
            let command = self.command.read(cx).value().trim().to_owned();
            if command.is_empty() {
                return Err("Command is required.".into());
            }
            McpTransport::Stdio {
                command: command.into(),
                arguments: self
                    .arguments
                    .read(cx)
                    .value()
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect(),
                environment: parse_pairs(&self.environment.read(cx).value(), '=', "variable")?,
            }
        } else {
            let url = self.url.read(cx).value().trim().to_owned();
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err("Enter an http:// or https:// URL.".into());
            }
            McpTransport::Http {
                url,
                headers: parse_pairs(&self.headers.read(cx).value(), ':', "header")?,
            }
        };
        Ok(McpServer { name, transport })
    }
}

impl Render for McpEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut form = Form::new()
            .child(
                Field::new()
                    .label("Name")
                    .child(Input::new(&self.name).aria_label("Server name")),
            )
            .child(
                Field::new().label("Type").child(
                    TabBar::new("mcp-type")
                        .segmented()
                        .selected_index(self.kind)
                        .child(Tab::new().label("Command"))
                        .child(Tab::new().label("HTTP"))
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.kind = *ix;
                            cx.notify();
                        })),
                ),
            );
        form = if self.kind == 0 {
            form.child(
                Field::new()
                    .label("Command")
                    .child(Input::new(&self.command).aria_label("Command")),
            )
            .child(
                Field::new()
                    .label("Arguments")
                    .child(Textarea::new(&self.arguments).aria_label("Arguments")),
            )
            .child(
                Field::new()
                    .label("Environment variables")
                    .child(Textarea::new(&self.environment).aria_label("Environment variables")),
            )
        } else {
            form.child(
                Field::new()
                    .label("URL")
                    .child(Input::new(&self.url).aria_label("URL")),
            )
            .child(
                Field::new()
                    .label("Headers")
                    .child(Textarea::new(&self.headers).aria_label("Headers")),
            )
        };
        col()
            .gap_3()
            .child(form)
            .when_some(self.error.clone(), |column, error| {
                column.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }
}

/// Opens the editor for a new server, or for `server`.
pub(super) fn open_editor(
    server: Option<&McpServer>,
    save: Save,
    window: &mut Window,
    cx: &mut App,
) {
    let editor = cx.new(|cx| McpEditor::new(server, window, cx));
    let title = if server.is_some() {
        "Edit MCP server"
    } else {
        "Add MCP server"
    };
    let submit = Rc::new({
        let editor = editor.clone();
        move |window: &mut Window, cx: &mut App| -> bool {
            let result = editor
                .read(cx)
                .value(cx)
                .and_then(|server| save(server, window, cx));
            match result {
                Ok(()) => true,
                Err(error) => {
                    editor.update(cx, |editor, cx| {
                        editor.error = Some(error);
                        cx.notify();
                    });
                    false
                }
            }
        }
    });
    window.open_dialog(cx, move |dialog, _, cx| {
        let (ok, button) = (submit.clone(), submit.clone());
        views::styled_dialog(dialog, cx)
            .w(px(480.))
            .title(views::dialog_title(title))
            .child(editor.clone())
            .on_ok(move |_, window, cx| ok(window, cx))
            .footer(
                row()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("mcp-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("mcp-save")
                            .small()
                            .primary()
                            .label("Save")
                            .on_click(move |_, window, cx| {
                                if button(window, cx) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
    });
}

/// One row per server, with Edit… and Remove, then "Add server…".
pub(super) fn server_list(
    id: &'static str,
    servers: &[McpServer],
    edit: &Rc<dyn Fn(Option<usize>, &mut Window, &mut App)>,
    remove: &Rc<dyn Fn(usize, &mut Window, &mut App)>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let mut list = col().w_full().gap_2();
    let mut rows = col()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(theme.border);
    for (ix, server) in servers.iter().enumerate() {
        let (edit, remove) = (edit.clone(), remove.clone());
        rows = rows.child(
            row()
                .w_full()
                .gap_2()
                .px_4()
                .py_2()
                .when(ix > 0, |row| row.border_t_1().border_color(theme.border))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(server.name.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(server.kind()),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-edit-{ix}")))
                        .small()
                        .label("Edit…")
                        .on_click(move |_, window, cx| edit(Some(ix), window, cx)),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-remove-{ix}")))
                        .small()
                        .ghost()
                        .label("Remove")
                        .on_click(move |_, window, cx| remove(ix, window, cx)),
                ),
        );
    }
    if !servers.is_empty() {
        list = list.child(rows);
    }
    let edit = edit.clone();
    list.child(
        div().child(
            Button::new(SharedString::from(format!("{id}-add")))
                .small()
                .label("Add server…")
                .on_click(move |_, window, cx| edit(None, window, cx)),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_pairs;

    #[test]
    fn name_value_lines_parse_and_reject_bare_names() {
        assert_eq!(
            parse_pairs("A=1\n\n B = two ", '=', "variable").unwrap(),
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "two".to_owned())
            ]
        );
        assert!(parse_pairs("JUST_A_NAME", '=', "variable").is_err());
        assert_eq!(
            parse_pairs("Authorization: Bearer x", ':', "header").unwrap()[0].1,
            "Bearer x"
        );
    }
}
