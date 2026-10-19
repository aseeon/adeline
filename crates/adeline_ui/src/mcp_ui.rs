//! MCP servers in Settings and the agent form: one row per server with Edit…
//! and Remove, and the editor dialog. A save error keeps the dialog open
//! and shows it.
use super::*;
use crate::views::text_input;
use adeline_core::conversation::{McpServer, McpTransport};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::ButtonVariants as _,
    form::{Field, Form},
    input::Textarea,
    tab::{Tab, TabBar},
};
use std::rc::Rc;

pub(super) type Save = Rc<dyn Fn(McpServer, &mut Window, &mut App) -> Result<()>>;

pub(super) struct McpEditor {
    name: Entity<InputState>,
    http: bool,
    command: Entity<InputState>,
    arguments: Entity<TextareaState>,
    environment: Entity<TextareaState>,
    url: Entity<InputState>,
    headers: Entity<TextareaState>,
    error: Option<String>,
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

fn parse_pairs(text: &str, separator: char, what: &str) -> Result<Vec<(String, String)>> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(separator).ok_or_else(|| {
                anyhow::anyhow!("Write each {what} as name{separator}value: {line}")
            })?;
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
        let (http, command, arguments, environment, url, headers) =
            match server.map(|s| &s.transport) {
                Some(McpTransport::Http { url, headers }) => (
                    true,
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
                    false,
                    command.to_string_lossy().into_owned(),
                    arguments.clone(),
                    environment.clone(),
                    String::new(),
                    Vec::new(),
                ),
                None => (
                    false,
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    String::new(),
                    Vec::new(),
                ),
            };
        Self {
            name: text_input(
                server.map_or("", |s| s.name.as_str()),
                "e.g. github",
                window,
                cx,
            ),
            http,
            command: text_input(&command, "npx-free command, e.g. uvx or a path", window, cx),
            arguments: lines(arguments.join("\n"), "One argument per line", window, cx),
            environment: lines(
                pairs_text(&environment, "="),
                "NAME=value, one per line",
                window,
                cx,
            ),
            url: text_input(&url, "https://example.com/mcp", window, cx),
            headers: lines(
                pairs_text(&headers, ": "),
                "Name: value, one per line",
                window,
                cx,
            ),
            error: None,
        }
    }

    fn value(&self, cx: &App) -> Result<McpServer> {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            anyhow::bail!("Name is required.");
        }
        let transport = if self.http {
            let url = self.url.read(cx).value().trim().to_owned();
            if !url.starts_with("http://") && !url.starts_with("https://") {
                anyhow::bail!("Enter an http:// or https:// URL.");
            }
            McpTransport::Http {
                url,
                headers: parse_pairs(&self.headers.read(cx).value(), ':', "header")?,
            }
        } else {
            let command = self.command.read(cx).value().trim().to_owned();
            if command.is_empty() {
                anyhow::bail!("Command is required.");
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
                        .selected_index(usize::from(self.http))
                        .child(Tab::new().label("Command"))
                        .child(Tab::new().label("HTTP"))
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.http = *ix == 1;
                            cx.notify();
                        })),
                ),
            );
        form = if self.http {
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
        } else {
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
        };
        col()
            .gap_3()
            .child(form)
            .when_some(self.error.clone(), |column, error| {
                column.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }
}

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
                        editor.error = Some(format!("{error:#}"));
                        cx.notify();
                    });
                    false
                }
            }
        }
    });
    window.open_dialog(cx, move |dialog, _, cx| {
        let (ok, button) = (Rc::clone(&submit), Rc::clone(&submit));
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

pub(super) fn upsert(servers: &mut Vec<McpServer>, ix: Option<usize>, server: McpServer) {
    match ix {
        Some(ix) if ix < servers.len() => servers[ix] = server,
        _ => servers.push(server),
    }
}

pub(super) fn remove(servers: &mut Vec<McpServer>, ix: usize) {
    if ix < servers.len() {
        servers.remove(ix);
    }
}

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
        let (edit, remove) = (Rc::clone(edit), Rc::clone(remove));
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
    let edit = Rc::clone(edit);
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
        parse_pairs("JUST_A_NAME", '=', "variable").unwrap_err();
        assert_eq!(
            parse_pairs("Authorization: Bearer x", ':', "header").unwrap()[0].1,
            "Bearer x"
        );
    }
}
