//! The Engine and Agents › MCP servers pages.
use super::*;

impl SettingsWindow {
    fn retry_limit_row(&self, cx: &Context<Self>) -> Div {
        field_row(
            Field::new()
                .label(RETRY_LABEL)
                .description(RETRY_DESCRIPTION)
                .child(
                    div()
                        .w_24()
                        .child(Input::new(&self.retry_limit).aria_label(RETRY_LABEL)),
                ),
            self.retry_limit_error.clone(),
            cx,
        )
    }
    pub(super) fn save_engine_settings(
        &mut self,
        settings: protocol::EngineSettings,
        error: fn(&mut Self) -> &mut Option<String>,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity().downgrade();
        client::request(
            &self.machine,
            protocol::Command::SetSettings { settings },
            Box::new(move |result, cx| {
                let _ = view.update(cx, |this, cx| {
                    *error(this) = result.err();
                    cx.notify();
                });
            }),
            cx,
        );
    }

    fn stop_engine(&mut self, cx: &mut Context<Self>) {
        self.engine_stopping = true;
        let view = cx.entity().downgrade();
        client::request(
            &self.machine,
            protocol::Command::Shutdown,
            Box::new(move |result, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.engine_stopping = false;
                    this.engine_error = result.err();
                    cx.notify();
                });
            }),
            cx,
        );
        cx.notify();
    }

    pub(super) fn engine_settings(&self, query: &str, cx: &Context<Self>) -> Div {
        let mut page = div().w_full().flex().flex_col().gap_3();
        let name = machines::name(&self.machine);
        page = page
            .child(div().text_lg().child(if self.machine == machines::LOCAL {
                "Engine".to_owned()
            } else {
                format!("Engine on {name}")
            }))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(
                "The conversation engine runs agents in the background, so work can finish after Adeline closes. These settings apply immediately, including to work already running.",
            ));
        let Some(connection) = client::get(&self.machine, cx) else {
            let machine = self.machine.clone();
            return page
                .child(div().text_sm().child(format!(
                    "Check {name} in the machine selector to see and change its engine."
                )))
                .child(
                    div().flex().justify_end().child(
                        Button::new("engine-check-machine")
                            .small()
                            .primary()
                            .label(format!("Check {name}"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.owner.act(Action::Machine(machine.clone()), window, cx);
                            })),
                    ),
                );
        };
        let connected = connection.state == client::State::Connected;
        let mut rows = Vec::new();
        if matches_query(
            query,
            &["Engine", KEEP_RUNNING, KEEP_RUNNING_DESCRIPTION, "daemon"],
        ) {
            let view = cx.entity().downgrade();
            let checked = connection.settings.keep_running;
            rows.push(switch_row(
                KEEP_RUNNING,
                KEEP_RUNNING_DESCRIPTION,
                Switch::new("engine-keep-running")
                    .checked(checked)
                    .disabled(!connected)
                    .accessibility_label(KEEP_RUNNING)
                    .on_change(move |_, _, cx| {
                        let settings = protocol::EngineSettings {
                            keep_running: !checked,
                            ..view
                                .read_with(cx, |this, cx| engine_settings_of(&this.machine, cx))
                                .unwrap_or_default()
                        };
                        let _ = view.update(cx, |this, cx| {
                            this.save_engine_settings(settings, |this| &mut this.engine_error, cx);
                        });
                    }),
                cx,
            ));
        }
        if matches_query(query, &["Engine", RETRY_LABEL, RETRY_DESCRIPTION]) {
            rows.push(self.retry_limit_row(cx));
        }
        if let Some(card) = card(rows, cx) {
            page = page.child(card);
        }
        if let Some(message) = &self.engine_error {
            page = page.child(error(message.clone(), cx));
        }
        if matches_query(query, &[&["Engine"][..], &ENGINE_STATUS[..]].concat()) {
            let status = &connection.status;
            let state = match &connection.state {
                client::State::Connected => "Running".to_owned(),
                client::State::Connecting | client::State::Starting => "Starting…".to_owned(),
                client::State::Stopped { unexpected: true } => "Stopped unexpectedly".to_owned(),
                client::State::Stopped { .. } => "Stopped".to_owned(),
                client::State::Unavailable(reason) => format!("Unavailable: {reason}"),
                client::State::Mismatch(_) | client::State::Waiting => {
                    "An older engine is running".to_owned()
                }
                client::State::Demo => "Not used in demo mode".to_owned(),
                state => format!(
                    "{}{}",
                    state.label(),
                    state.detail().map(|d| format!(": {d}")).unwrap_or_default()
                ),
            };
            let uptime = status.uptime_secs + connection.status_at.elapsed().as_secs();
            let mut fields = vec![("Status", state)];
            if let Some(destination) = &connection.destination {
                fields.push(("Connected through", destination.clone()));
            }
            if connected {
                fields.extend([
                    ("PID", status.pid.to_string()),
                    ("Version", status.version.clone()),
                    ("Build", status.build().to_owned()),
                    ("Protocol", status.protocol.to_string()),
                    (
                        "Daemon mode",
                        if status.daemon { "On" } else { "Off" }.to_owned(),
                    ),
                    (
                        "Uptime",
                        format!("{}h {}m {}s", uptime / 3600, uptime / 60 % 60, uptime % 60),
                    ),
                    ("Connected clients", status.clients.to_string()),
                    (
                        "Active conversations",
                        status.conversations.len().to_string(),
                    ),
                ]);
                for conversation in &status.conversations {
                    fields.push((
                        "",
                        format!(
                            "{} / {} · {}",
                            conversation.project, conversation.title, conversation.state
                        ),
                    ));
                }
                fields.push(("Logs", status.log.clone()));
            }
            let rows = fields
                .into_iter()
                .map(|(label, value)| {
                    div()
                        .flex()
                        .justify_between()
                        .gap_4()
                        .px_4()
                        .py_2()
                        .child(label)
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(value),
                        )
                })
                .collect();
            page = page
                .child(caps("Status", cx).mt_2())
                .children(card(rows, cx));
            let button = if connected {
                Button::new("engine-stop")
                    .danger()
                    .label(if self.engine_stopping {
                        "Stopping engine…"
                    } else {
                        "Stop engine"
                    })
                    .disabled(self.engine_stopping)
                    .on_click(cx.listener(|this, _, _, cx| this.stop_engine(cx)))
            } else {
                Button::new("engine-start")
                    .primary()
                    .label("Start engine")
                    .disabled(matches!(
                        connection.state,
                        client::State::Demo | client::State::Connecting | client::State::Starting
                    ))
                    .on_click(cx.listener(|this, _, _, cx| client::retry(&this.machine, cx)))
            };
            page = page.child(div().flex().justify_end().mt_2().child(button.small()));
        }
        page
    }

    fn save_mcp_servers(&mut self, servers: Vec<conversation::McpServer>, cx: &mut Context<Self>) {
        let settings = protocol::EngineSettings {
            mcp_servers: servers,
            ..engine_settings_of(&self.machine, cx)
        };
        self.save_engine_settings(settings, |this| &mut this.mcp_error, cx);
    }

    pub(super) fn mcp_settings(&self, cx: &Context<Self>) -> Div {
        let view = cx.entity().downgrade();
        let servers = engine_settings_of(&self.machine, cx).mcp_servers;
        let edit_view = view.clone();
        let edit: Rc<dyn Fn(Option<usize>, &mut Window, &mut App)> =
            Rc::new(move |ix, window, cx| {
                let Some(settings) = edit_view.upgrade() else {
                    return;
                };
                let machine = settings.read(cx).machine.clone();
                let server = ix.and_then(|ix| {
                    engine_settings_of(&machine, cx)
                        .mcp_servers
                        .get(ix)
                        .cloned()
                });
                let saving = edit_view.clone();
                mcp_ui::open_editor(
                    server.as_ref(),
                    Rc::new(move |server, _, cx| {
                        let Some(settings) = saving.upgrade() else {
                            return Ok(());
                        };
                        let machine = settings.read(cx).machine.clone();
                        let mut servers = engine_settings_of(&machine, cx).mcp_servers;
                        if servers
                            .iter()
                            .enumerate()
                            .any(|(other, s)| s.name == server.name && Some(other) != ix)
                        {
                            anyhow::bail!("{} is already listed.", server.name);
                        }
                        mcp_ui::upsert(&mut servers, ix, server);
                        settings.update(cx, |this, cx| this.save_mcp_servers(servers, cx));
                        Ok(())
                    }),
                    window,
                    cx,
                );
            });
        let remove: Rc<dyn Fn(usize, &mut Window, &mut App)> = Rc::new(move |ix, _, cx| {
            let _ = view.update(cx, |this, cx| {
                let mut servers = engine_settings_of(&this.machine, cx).mcp_servers;
                mcp_ui::remove(&mut servers, ix);
                this.save_mcp_servers(servers, cx);
            });
        });
        let connected = client::state(&self.machine, cx).is_some_and(client::State::usable);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().child("MCP servers"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(MCP_DESCRIPTION),
            )
            .child(if connected || self.demo {
                mcp_ui::server_list("settings-mcp", &servers, &edit, &remove, cx).into_any_element()
            } else {
                div()
                    .text_sm()
                    .child("Connect this machine's engine to change its MCP servers.")
                    .into_any_element()
            })
            .when_some(self.mcp_error.clone(), |page, message| {
                page.child(error(message, cx))
            })
    }
}
