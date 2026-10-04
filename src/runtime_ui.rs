//! The UI side of conversations: it mirrors what the engine reports and
//! sends every action to the engine.
use super::*;
use crate::protocol::{Command, Delta, Live, Snapshot};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Runtime {
    pub conversations: HashMap<String, Live>,
    pub expanded_tools: HashSet<String>,
    /// The user chose Stop all or Finish in background, so quitting goes ahead.
    pub quitting: bool,
    /// Stop all is waiting for every agent to exit.
    pub stopping_all: bool,
}

impl Adeline {
    /// Sends a command to the engine; `done` runs with its reply.
    pub(super) fn request(
        &self,
        command: Command,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Value, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let owner = cx.weak_entity();
        let handle = self.main_window;
        client::request(
            command,
            Box::new(move |result, cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = owner.update(cx, |app, cx| done(app, result, window, cx));
                });
            }),
            cx,
        );
    }

    /// Sends a command whose only answer worth showing is a refusal.
    fn command(&self, command: Command, cx: &mut Context<Self>) {
        self.request(command, cx, |app, result, _, cx| {
            if let Err(error) = result {
                app.notify_toast(&error, cx);
            }
        });
    }

    pub(super) fn current_id(&self) -> Option<String> {
        self.selected
            .and_then(|i| self.workspace().threads.get(i))
            .map(|t| t.id.clone())
    }

    fn locate_conversation(&self, id: &str) -> Option<(usize, usize)> {
        self.projects.iter().enumerate().find_map(|(p, project)| {
            project
                .threads
                .iter()
                .position(|thread| thread.id == id)
                .map(|t| (p, t))
        })
    }

    /// The agent a conversation started with.
    pub(super) fn conversation_agent(&self, id: &str) -> Option<String> {
        self.runtime
            .conversations
            .get(id)
            .map(|live| live.agent_id.clone())
            .filter(|agent| !agent.is_empty())
    }

    pub(super) fn bound_definition(&self) -> Option<storage::ExecutionConfig> {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .and_then(|live| live.execution.clone())
    }

    /// The open chat's permission mode, or the one a new chat will start with.
    pub(super) fn current_permission_mode(&self) -> Option<agents::PermissionMode> {
        if self.selected.is_none() {
            return self.new_chat_permission.or_else(|| {
                self.selected_definition()
                    .map(|agent| agent.permission_mode)
            });
        }
        if self.demo_mode {
            return Some(agents::PermissionMode::ALL[self.permission]);
        }
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .and_then(|live| live.permission_mode)
    }

    pub(super) fn conversation_processing(&self) -> bool {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .is_some_and(|r| r.processing)
    }

    pub(super) fn mark_conversation_read(&mut self, cx: &mut Context<Self>) {
        if self.demo_mode {
            return;
        }
        let Some(id) = self.current_id() else {
            return;
        };
        let Some((p, t)) = self.locate_conversation(&id) else {
            return;
        };
        let Some(through) = self.projects[p].threads[t]
            .messages
            .iter()
            .rposition(|message| message.role == "assistant")
        else {
            return;
        };
        let live = self.runtime.conversations.entry(id.clone()).or_default();
        if live.storage_failed || live.last_read_through.is_some_and(|last| last >= through) {
            return;
        }
        live.last_read_through = Some(through);
        self.projects[p].threads[t].mark_read();
        self.command(Command::MarkRead { id, through }, cx);
    }

    pub(super) fn refresh_runtime_views(&self, cx: &mut Context<Self>) {
        self.sync_sidebar(cx);
        self.transcript
            .update(cx, |view, cx| view.sync(self, false, cx));
        self.composer_region.update(cx, |_, cx| cx.notify());
        self.header_region.update(cx, |_, cx| cx.notify());
        self.control_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn send_real(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_open_project() {
            return;
        }
        let prompt = self.composer.read(cx).value().trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        let conversation_id = self.current_id();
        let new = conversation_id.is_none();
        let agent_id = if new {
            if self.selected_definition().is_none() && self.agent_catalog.entries.len() == 1 {
                self.selected_agent = Some(self.agent_catalog.entries[0].id.clone());
            }
            if self.selected_definition().is_none() {
                self.notify_toast("Create or select an agent before sending.", cx);
                self.open_commands("agent", window, cx);
                return;
            }
            self.selected_agent.clone()
        } else {
            None
        };
        let project_id = self.workspace().config.id.clone();
        let command = Command::Send {
            project_id: project_id.clone(),
            conversation_id,
            agent_id,
            permission_mode: if new { self.new_chat_permission } else { None },
            prompt: prompt.clone(),
        };
        self.request(command, cx, move |app, result, window, cx| match result {
            Ok(id) => {
                if new {
                    app.new_chat_permission = None;
                    if app.selected.is_none() && app.workspace().config.id == project_id {
                        app.selected = app
                            .workspace()
                            .threads
                            .iter()
                            .position(|thread| Some(thread.id.as_str()) == id.as_str());
                    }
                }
                // The composer keeps anything typed since sending.
                if app.composer.read(cx).value().trim() == prompt {
                    app.composer
                        .update(cx, |state, cx| state.set_value("", window, cx));
                }
                app.refresh_runtime_views(cx);
            }
            Err(error) => app.notify_toast(&error, cx),
        });
    }

    /// The open conversation's offered options, live or from its last session.
    pub(super) fn conversation_options(&self) -> Vec<Value> {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .map(|live| live.options.clone())
            .unwrap_or_default()
    }

    /// Switches the open conversation's model or effort. It never changes the agent.
    pub(super) fn switch_setting(
        &mut self,
        kind: harness::Kind,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.current_id() {
            let effort = kind == harness::Kind::Effort;
            self.command(Command::SwitchSetting { id, effort, value }, cx);
        }
    }

    pub(super) fn stop_conversation(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::Stop { id }, cx);
        }
    }

    pub(super) fn complete_conversation(&mut self, archive: bool, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.finish_conversation(&id, archive, cx);
        }
    }

    /// Completes or archives a chat, stopping its agent. Completing a finished
    /// chat reopens it as idle.
    pub(super) fn finish_conversation(&mut self, id: &str, archive: bool, cx: &mut Context<Self>) {
        let Some((p, t)) = self.locate_conversation(id) else {
            return;
        };
        let finished = matches!(
            self.projects[p].threads[t].status.as_str(),
            "completed" | "archived"
        );
        let status = if archive {
            "archived"
        } else if finished {
            "idle"
        } else {
            "completed"
        };
        if self.demo_mode {
            self.projects[p].threads[t].status = status.into();
        } else {
            self.command(
                Command::SetStatus {
                    id: id.to_owned(),
                    status: status.into(),
                },
                cx,
            );
        }
        self.menu = None;
        self.refresh_runtime_views(cx);
    }

    pub(super) fn project_agents_stopped(&self, project_id: &str) -> bool {
        self.projects
            .iter()
            .filter(|p| p.config.id == project_id)
            .flat_map(|p| &p.threads)
            .all(|t| {
                self.runtime
                    .conversations
                    .get(&t.id)
                    .is_none_or(|r| !r.running)
            })
    }

    pub(super) fn forget_project_runtime(&mut self, ids: &[String]) {
        for id in ids {
            self.runtime.conversations.remove(id);
            let prefix = format!("{id}:");
            self.runtime
                .expanded_tools
                .retain(|key| !key.starts_with(&prefix));
        }
    }

    pub(super) fn force_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.command(
            Command::ForceProject {
                id: project_id.to_owned(),
            },
            cx,
        );
    }

    pub(super) fn force_conversation(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::ForceStop { id }, cx);
        }
    }

    /// Stops every agent the engine runs, for every client.
    pub(super) fn stop_all(&mut self, cx: &mut Context<Self>) {
        self.request(Command::StopAll, cx, |app, result, _, cx| {
            let message = match result {
                Ok(value) => match value["stopped"].as_array().map_or(0, Vec::len) {
                    0 => "No agents were running.".to_owned(),
                    1 => "Stopped 1 agent.".to_owned(),
                    count => format!("Stopped {count} agents."),
                },
                Err(error) => error,
            };
            app.notify_toast(&message, cx);
        });
    }

    /// Whether the window may close now. Quitting the last client while a
    /// turn is active asks first.
    pub(super) fn request_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.demo_mode || self.runtime.quitting {
            return true;
        }
        let connection = client::connection(cx);
        if connection.state != client::State::Connected
            || connection.status.clients > 1
            || !self
                .runtime
                .conversations
                .values()
                .any(|live| live.processing)
        {
            return true;
        }
        self.modal = Some("quit");
        self.refresh_runtime_views(cx);
        false
    }

    fn close_main_window(&self, cx: &mut Context<Self>) {
        let handle = self.main_window;
        let owner = cx.weak_entity();
        cx.defer(move |cx| settings::request_close(handle, owner, cx));
    }

    pub(super) fn quit_stopping_all(&mut self, cx: &mut Context<Self>) {
        if self.runtime.stopping_all {
            return;
        }
        self.runtime.stopping_all = true;
        self.request(Command::StopAll, cx, |app, result, _, cx| {
            app.runtime.stopping_all = false;
            if let Err(error) = result {
                app.notify_toast(&error, cx);
            }
            app.runtime.quitting = true;
            app.modal = None;
            app.close_main_window(cx);
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn quit_in_background(&mut self, cx: &mut Context<Self>) {
        self.runtime.quitting = true;
        self.modal = None;
        self.close_main_window(cx);
    }

    pub(super) fn retry_prompt(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::Retry { id }, cx);
        }
    }

    /// Copies the open chat through a finished reply into a new chat and opens it.
    pub(super) fn fork_conversation(
        &mut self,
        message: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.selected else {
            return;
        };
        if self.demo_mode {
            let source = &self.workspace().threads[ix];
            let fork = Thread {
                id: format!("local-{}", self.workspace().threads.len()),
                title: format!("{} (fork)", source.title),
                provider: source.provider.clone(),
                status: "idle".into(),
                messages: source.messages[..=message].to_vec(),
                activity: source
                    .activity
                    .iter()
                    .filter(|a| a.turn.is_some_and(|turn| turn < message))
                    .cloned()
                    .collect(),
                fork: Some(Fork {
                    id: source.id.clone(),
                    title: source.title.clone(),
                    text_copy: false,
                }),
                ..Default::default()
            };
            self.projects[self.project].threads.insert(0, fork);
            self.open_fork(0, window, cx);
            return;
        }
        let id = self.workspace().threads[ix].id.clone();
        self.request(
            Command::Fork { id, message },
            cx,
            move |app, result, window, cx| match result {
                Ok(Value::String(id)) => {
                    if let Some(ix) = app.workspace().threads.iter().position(|t| t.id == id) {
                        app.open_fork(ix, window, cx);
                    }
                }
                Ok(_) => {}
                Err(error) => app.notify_toast(&format!("Could not fork the chat: {error}"), cx),
            },
        );
    }

    fn open_fork(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.act(Action::Chat(ix), window, cx);
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
        window.focus(&self.composer.focus_handle(cx), cx);
    }

    pub(super) fn retry_storage(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::RetryStorage { id }, cx);
        }
    }

    pub(super) fn replace_session(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::ReplaceSession { id }, cx);
        }
    }

    pub(super) fn set_conversation_permission(&mut self, mode: usize, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            let mode = agents::PermissionMode::ALL[mode];
            self.command(Command::SetPermissionMode { id, mode }, cx);
        }
    }

    pub(super) fn answer_permission(&mut self, option: String, cx: &mut Context<Self>) {
        let Some(id) = self.current_id() else {
            return;
        };
        let Some(request_id) = self
            .runtime
            .conversations
            .get(&id)
            .and_then(|live| live.permission.first())
            .map(|request| request.request_id)
        else {
            return;
        };
        self.command(
            Command::AnswerPermission {
                id,
                request_id,
                option_id: option,
            },
            cx,
        );
    }

    // -----------------------------------------------------------------------
    // State from the engine

    /// Takes the engine's whole state, keeping this window's tabs and selection.
    pub(super) fn apply_snapshot(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        let first = !self.engine_loaded;
        self.engine_loaded = true;
        let current = self
            .has_open_project()
            .then(|| self.workspace().config.id.clone());
        let selected = self.current_id();
        let open: HashSet<String> = self
            .projects
            .iter()
            .zip(&self.open_projects)
            .filter(|(_, open)| **open)
            .map(|(project, _)| project.config.id.clone())
            .collect();
        let tints: HashMap<String, usize> = self
            .projects
            .iter()
            .zip(&self.project_tints)
            .map(|(project, tint)| (project.config.id.clone(), *tint))
            .collect();
        let mut projects = snapshot.projects;
        ui_state::apply(&mut projects);
        self.open_projects = projects
            .iter()
            .map(|project| first || open.contains(&project.config.id))
            .collect();
        self.project_tints = projects
            .iter()
            .enumerate()
            .map(|(i, project)| {
                tints
                    .get(&project.config.id)
                    .copied()
                    .unwrap_or([0, 2, 3][i.min(2)])
            })
            .collect();
        self.projects = projects;
        self.project = current
            .and_then(|id| self.projects.iter().position(|p| p.config.id == id))
            .unwrap_or(0);
        self.selected = selected.and_then(|id| {
            self.workspace()
                .threads
                .iter()
                .position(|thread| thread.id == id)
        });
        self.runtime.conversations = snapshot.live;
        self.agent_catalog.entries = snapshot.agents;
        self.agent_catalog.errors = snapshot.agent_errors;
        if self.selected_agent.is_none() && self.agent_catalog.entries.len() == 1 {
            self.selected_agent = Some(self.agent_catalog.entries[0].id.clone());
        }
        if !snapshot.errors.is_empty() {
            self.notify_toast(&snapshot.errors.join("\n"), cx);
        }
        self.agents_changed(None, None, false, cx);
        self.refresh_runtime_views(cx);
    }

    pub(super) fn apply_delta(&mut self, delta: Delta, cx: &mut Context<Self>) {
        match delta {
            Delta::Project {
                previous,
                mut workspace,
            } => {
                let key = previous.as_deref().unwrap_or(&workspace.config.id);
                if let Some(old) = &previous {
                    ui_state::rename(old, &workspace.config.id);
                }
                workspace.config.opened_at = ui_state::opened_at(&workspace.config.id);
                if let Some(project) = self.projects.iter_mut().find(|p| p.config.id == key) {
                    project.config = workspace.config;
                } else {
                    self.projects.push(workspace);
                    self.open_projects.push(false);
                    self.project_tints
                        .push((self.projects.len() - 1) % theme::project_colors().len());
                }
                self.sync_regions(&Action::Project(self.project), cx);
            }
            Delta::ProjectRemoved { id } => {
                if let Some(ix) = self.projects.iter().position(|p| p.config.id == id) {
                    self.remove_project_at(ix, cx);
                }
            }
            Delta::Agents { entries, errors } => {
                self.agent_catalog.entries = entries;
                self.agent_catalog.errors = errors;
                self.agents_changed(None, None, false, cx);
            }
            Delta::Notice { message } => self.notify_toast(&message, cx),
            delta => {
                let id = match &delta {
                    Delta::Live { id, .. } | Delta::Text { id, .. } => Some(id.clone()),
                    _ => None,
                };
                protocol::apply(&mut self.projects, &mut self.runtime.conversations, &delta);
                if id.is_some() && id == self.current_id() {
                    if let Some((p, t)) = id.as_deref().and_then(|id| self.locate_conversation(id))
                    {
                        // Text streamed into the open chat is read as it arrives.
                        self.projects[p].threads[t].mark_read();
                    }
                    if id
                        .as_ref()
                        .and_then(|id| self.runtime.conversations.get(id))
                        .is_some_and(|live| !live.processing)
                    {
                        self.mark_conversation_read(cx);
                    }
                }
            }
        }
        self.refresh_runtime_views(cx);
    }

    /// The engine went away: nothing runs any more until it starts again.
    pub(super) fn engine_stopped(&mut self, unexpected: bool, cx: &mut Context<Self>) {
        for live in self.runtime.conversations.values_mut() {
            live.running = false;
            live.shutting_down = false;
            live.shutdown_stuck = false;
            live.permission.clear();
            if live.processing {
                live.processing = false;
                live.progress = None;
                live.error = Some("The conversation engine stopped during this turn.".into());
            }
        }
        self.runtime.stopping_all = false;
        if unexpected {
            self.notify_toast("Conversation engine stopped unexpectedly", cx);
        }
        self.refresh_runtime_views(cx);
    }

    /// A full-window state while there's no engine state to show.
    pub(super) fn engine_screen(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let connection = client::connection(cx);
        let (title, detail, buttons): (String, Option<String>, Vec<Button>) =
            match &connection.state {
                client::State::Connecting | client::State::Starting if !connection.loaded => {
                    ("Starting conversation engine…".into(), None, Vec::new())
                }
                client::State::Unavailable(reason) => (
                    "Conversation engine unavailable".into(),
                    Some(format!("{reason}\nLogs: {}", ipc::log_path())),
                    vec![
                        self.button("engine-retry", "Retry", Action::EngineRetry, cx)
                            .primary(),
                    ],
                ),
                client::State::Mismatch(status) => {
                    let count = status.conversations.len();
                    let mut buttons = Vec::new();
                    if status.daemon && count == 0 {
                        buttons.push(
                            self.button(
                                "engine-restart",
                                "Restart engine",
                                Action::EngineStopOld,
                                cx,
                            )
                            .primary(),
                        );
                    } else {
                        buttons.push(self.button("engine-wait", "Wait", Action::EngineWait, cx));
                        buttons.push(
                            self.button(
                                "engine-stop-old",
                                "Stop them now",
                                Action::EngineStopOld,
                                cx,
                            )
                            .danger(),
                        );
                    }
                    (
                        format!(
                            "An older engine is finishing {count} conversation{}",
                            if count == 1 { "" } else { "s" }
                        ),
                        Some(format!(
                            "Version {} is still running. This Adeline needs a newer engine.",
                            status.version
                        )),
                        buttons,
                    )
                }
                client::State::Waiting => (
                    "Waiting for the older engine to finish…".into(),
                    Some("Adeline connects as soon as it exits.".into()),
                    vec![
                        self.button(
                            "engine-stop-old",
                            "Stop them now",
                            Action::EngineStopOld,
                            cx,
                        )
                        .danger(),
                    ],
                ),
                _ => return None,
            };
        Some(
            col()
                .id("engine-state")
                .role(Role::Status)
                .aria_label(title.clone())
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .child(div().text_lg().child(title))
                .children(detail.map(|detail| {
                    div()
                        .max_w(px(560.))
                        .text_sm()
                        .text_center()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail)
                }))
                .child(row().gap_2().children(buttons))
                .into_any_element(),
        )
    }

    /// A strip above the content while data is shown without an engine.
    pub(super) fn engine_banner(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let connection = client::connection(cx);
        let (message, button) = match connection.state {
            client::State::Stopped { unexpected } => (
                if unexpected {
                    "Conversation engine stopped unexpectedly"
                } else {
                    "Conversation engine stopped"
                },
                Some(self.button("start-engine", "Start engine", Action::StartEngine, cx)),
            ),
            client::State::Connecting | client::State::Starting if connection.loaded => {
                ("Starting conversation engine…", None)
            }
            _ => return None,
        };
        Some(
            row()
                .id("engine-banner")
                .role(Role::Status)
                .aria_label(message)
                .w_full()
                .gap_3()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().group_box)
                .text_sm()
                .child(div().flex_1().child(message))
                .children(button)
                .into_any_element(),
        )
    }

    pub(super) fn runtime_footer(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[index];
        let mut content = col().w_full().gap_3().pb_4();
        let processing = self
            .runtime
            .conversations
            .get(&thread.id)
            .is_some_and(|live| live.processing);
        // The running turn's steps show live; finished turns fold them into
        // the summary under their reply.
        let turn = thread.messages.iter().rposition(|m| m.role == "user");
        if processing
            && let Some(turn) = turn
            && !config::current().modes.chats.hide_tool_calls
        {
            let steps: Vec<_> = thread.turn_tools(turn).collect();
            if !steps.is_empty() {
                content = content.child(chat_render::tool_steps(steps, cx));
            }
        }
        for activity in thread.activity.iter().filter(|a| a.kind == "error") {
            content = content.child(text(activity.title.clone(), 13., theme::foreground()));
        }
        if let Some(live) = self.runtime.conversations.get(&thread.id) {
            if live.processing && live.storage_failed {
                content = content.child(text(
                    "Stopping the turn before storage can be retried…",
                    13.,
                    theme::muted_foreground(),
                ));
            } else if live.processing && live.permission.is_empty() && live.assistant.is_none() {
                content = content.child(
                    chat_render::agent_header(
                        self.agent_icon(&thread.provider, cx),
                        thread.provider.clone(),
                        cx,
                    )
                    .child(chat_render::thinking_label(live.progress.clone(), cx)),
                );
            }
            if let Some(request) = live.permission.first() {
                let mut options = row().flex_wrap().gap_2();
                for option in &request.options {
                    if option.kind == "reject_always" {
                        continue;
                    }
                    let label = match option.kind.as_str() {
                        "allow_once" => format!("Allow once: {}", option.name),
                        "allow_always" => {
                            format!("{} (harness remembers this choice)", option.name)
                        }
                        "reject_once" => format!("Deny once: {}", option.name),
                        _ => continue,
                    };
                    let action = Action::PermissionResponse(option.option_id.clone());
                    let button = Button::new(SharedString::from(format!(
                        "permission-{}",
                        option.option_id
                    )))
                    .small()
                    .label(label)
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(action.clone(), window, cx);
                    }));
                    options = options.child(if option.kind == "allow_once" {
                        button.primary()
                    } else {
                        button.outline()
                    });
                }
                content = content.child(
                    col()
                        .id("permission-request")
                        .role(Role::Group)
                        .aria_label("Permission request")
                        .w_full()
                        .gap_3()
                        .p_4()
                        .bg(cx.theme().group_box)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded_lg()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(request.title.clone()),
                        )
                        .child(options),
                );
            }
            if let Some(error) = &live.error {
                content = content.child(text(error.clone(), 13., theme::foreground()));
            }
            if live.storage_failed {
                content = content.child(self.button(
                    "retry-storage",
                    "Retry storage",
                    Action::RetryStorage,
                    cx,
                ));
            } else if live.replacement && !live.shutting_down {
                content = content.child(self.button(
                    "replace-session",
                    "Start new session with saved context",
                    Action::ReplaceSession,
                    cx,
                ));
            } else if live.error.is_some()
                && !live.shutting_down
                && !live.processing
                && !live.recovering_storage
                && !live.last_prompt.is_empty()
                && !matches!(thread.status.as_str(), "completed" | "archived")
            {
                content =
                    content.child(self.button("retry-prompt", "Retry", Action::RetryPrompt, cx));
            }
            if live.shutting_down {
                content = content.child(text(
                    "Waiting for the agent to stop…",
                    13.,
                    theme::muted_foreground(),
                ));
            }
            if live.shutdown_stuck {
                content =
                    content.child(self.button("force-stop", "Force Stop", Action::ForceStop, cx));
            }
        }
        content.into_any_element()
    }
}
