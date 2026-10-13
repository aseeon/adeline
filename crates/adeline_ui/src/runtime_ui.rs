//! The UI side of conversations: it mirrors what the engine reports and
//! sends every action to the engine.
//!
//! A streamed chunk of reply or thought redraws only the transcript and the
//! side panel. The chat list changes only when the chunk starts a new message.
//! Every other change goes through `refresh_runtime_views`, which redraws all
//! the regions.
use super::*;
use adeline_core::conversation::{MessageRole, ThreadStatus};
use adeline_core::protocol::{Command, Delta, Live, Snapshot};
use anyhow::Context as _;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

// The traffic tab keeps at most this many lines, dropping the oldest in batches
// so the list doesn't shift on every new line.
mod machine_sync;
mod screens;

const TRAFFIC_CAP: usize = 5000;
const TRAFFIC_TRIM: usize = 1000;

#[derive(Default)]
pub(super) struct Runtime {
    pub conversations: HashMap<String, Live>,
    pub expanded_tools: HashSet<String>,
    pub quitting: bool,
    pub stopping_all: bool,
}

impl Adeline {
    pub(super) fn request(
        &self,
        command: Command,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Value, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.machine_request(&self.current_machine(), command, cx, done);
    }

    pub(super) fn machine_request(
        &self,
        machine: &str,
        command: Command,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Value, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let owner = cx.weak_entity();
        let handle = self.main_window;
        client::request(
            machine,
            command,
            Box::new(move |result, cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = owner.update(cx, |app, cx| done(app, result, window, cx));
                });
            }),
            cx,
        );
    }

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

    pub(super) fn conversation_agent(&self, id: &str) -> Option<String> {
        self.runtime
            .conversations
            .get(id)
            .map(|live| live.agent_id.clone())
            .filter(|agent| !agent.is_empty())
    }

    pub(super) fn bound_definition(&self) -> Option<agents::ExecutionConfig> {
        self.current_live().and_then(|live| live.execution.clone())
    }

    pub(super) fn current_live(&self) -> Option<&Live> {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
    }

    pub(super) fn conversation_processing(&self) -> bool {
        self.current_live().is_some_and(|live| live.processing)
    }

    pub(super) fn mark_conversation_read(&mut self, cx: &mut Context<Self>) {
        let machine = self.current_machine();
        // A disconnected remote machine's chats stay as they were last received.
        if self.demo_mode || machine != machines::LOCAL && !client::usable(&machine, cx) {
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
            .rposition(|message| message.role == MessageRole::Assistant)
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
        self.resync_transcript(cx);
        self.composer_region.update(cx, |_, cx| cx.notify());
        self.header_region.update(cx, |_, cx| cx.notify());
        self.control_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn send_real(&mut self, now: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_open_project() {
            return;
        }
        let prompt = self.composer.read(cx).value().trim().to_owned();
        let attachments = self.attachments.clone();
        if prompt.is_empty() && attachments.is_empty() {
            // Send now with an empty composer sends the queue's first message.
            if now
                && let Some(id) = self.current_id()
                && let Some(first) = self
                    .runtime
                    .conversations
                    .get(&id)
                    .and_then(|l| l.queued.first())
            {
                let queued = first.id;
                self.command(Command::SendQueuedNow { id, queued }, cx);
            }
            return;
        }
        let conversation_id = self.current_id();
        let new = conversation_id.is_none();
        let agent_id = if new {
            if self.selected_definition().is_none() && self.agent_catalog().entries.len() == 1 {
                self.selected_agent = Some(self.agent_catalog().entries[0].id.clone());
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
        let key = self.workspace().key();
        let command = Command::Send {
            project_id,
            conversation_id,
            agent_id,
            prompt: prompt.clone(),
            attachments,
            now,
        };
        self.request(command, cx, move |app, result, window, cx| match result {
            Ok(id) => {
                if new && app.selected.is_none() && app.workspace().key() == key {
                    app.selected = app
                        .workspace()
                        .threads
                        .iter()
                        .position(|thread| Some(thread.id.as_str()) == id.as_str());
                }
                // The composer keeps anything typed since sending.
                if app.composer.read(cx).value().trim() == prompt {
                    app.composer
                        .update(cx, |state, cx| state.set_value("", window, cx));
                }
                app.attachments.clear();
                app.attachment_error = None;
                app.refresh_runtime_views(cx);
            }
            Err(error) => {
                app.attachment_error =
                    Some(error.clone()).filter(|e| e.contains("attach") || e.contains("images"));
                app.notify_toast(&error, cx);
                app.composer_region.update(cx, |_, cx| cx.notify());
            }
        });
    }

    pub(super) fn conversation_options(&self) -> Vec<conversation::SessionOption> {
        self.current_live()
            .map(|live| live.options.clone())
            .unwrap_or_default()
    }

    pub(super) fn set_option(
        &mut self,
        category: conversation::Category,
        option: String,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.current_id() else {
            return;
        };
        if self.demo_mode {
            if let Some(found) = self
                .runtime
                .conversations
                .get_mut(&id)
                .and_then(|live| live.options.iter_mut().find(|o| o.id == option))
            {
                match &mut found.kind {
                    conversation::OptionKind::Select { current, .. } => current.clone_from(&value),
                    conversation::OptionKind::Boolean { current } => *current = value == "true",
                }
            }
            self.refresh_runtime_views(cx);
            return;
        }
        self.command(
            Command::SetOption {
                id,
                category,
                option,
                value,
            },
            cx,
        );
    }

    pub(super) fn take_queued(
        &mut self,
        queued: u64,
        edit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.current_id() else {
            return;
        };
        if self.demo_mode {
            let live = self.runtime.conversations.entry(id).or_default();
            if let Some(index) = live.queued.iter().position(|q| q.id == queued) {
                let item = live.queued.remove(index);
                if edit {
                    self.composer
                        .update(cx, |state, cx| state.set_value(item.text, window, cx));
                    window.focus(&self.composer.focus_handle(cx), cx);
                }
            }
            self.refresh_runtime_views(cx);
            return;
        }
        self.request(
            Command::TakeQueued { id, queued },
            cx,
            move |app, result, window, cx| match result {
                Ok(Value::String(text)) if edit => {
                    app.composer
                        .update(cx, |state, cx| state.set_value(text, window, cx));
                    window.focus(&app.composer.focus_handle(cx), cx);
                }
                Ok(_) => {}
                Err(error) => app.notify_toast(&error, cx),
            },
        );
    }

    pub(super) fn send_queued_now(&mut self, queued: u64, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::SendQueuedNow { id, queued }, cx);
        }
    }

    pub(super) fn login(&mut self, method: &str, cx: &mut Context<Self>) {
        let Some(id) = self.current_id() else {
            return;
        };
        let Some(live) = self.runtime.conversations.get(&id) else {
            return;
        };
        let Some(method) = live.features.auth.iter().find(|m| m.id == method).cloned() else {
            return;
        };
        let Some(execution) = live.execution.clone() else {
            return;
        };
        let name = execution.name.clone();
        let terminal = method.terminal.is_some();
        self.request(
            Command::Login {
                harness: execution.harness,
                command: execution.command.to_string_lossy().into_owned(),
                arguments: execution.arguments,
                method: Some(method),
            },
            cx,
            move |app, result, _, cx| {
                match result {
                    Ok(_) if terminal => {
                        app.notify_toast("Finish logging in in the terminal, then retry.", cx);
                    }
                    Ok(_) => {
                        app.logged_in.insert(id.clone());
                        app.notify_toast(&format!("Logged in to {name}."), cx);
                    }
                    Err(error) => app.notify_toast(&error, cx),
                }
                app.refresh_runtime_views(cx);
            },
        );
    }

    pub(super) fn watch_traffic(&mut self, cx: &mut Context<Self>) {
        let watched = self
            .current_id()
            .filter(|_| self.traffic_tab && self.side_panel_is_open());
        self.traffic.clear();
        if self.demo_mode {
            if let Some(id) = &watched {
                self.traffic = demo_traffic(id);
            }
            return;
        }
        self.request(
            Command::WatchTraffic {
                id: watched.clone(),
            },
            cx,
            move |app, result, _, cx| {
                if app.current_id() == watched
                    && let Ok(Value::Array(entries)) = result
                {
                    app.traffic = entries
                        .into_iter()
                        .filter_map(|entry| serde_json::from_value(entry).ok())
                        .collect();
                    app.traffic_scroll.scroll_to_bottom();
                    app.control_pane.update(cx, |_, cx| cx.notify());
                }
            },
        );
    }

    pub(super) fn traffic_arrived(
        &mut self,
        id: &str,
        entry: conversation::TrafficEntry,
        cx: &mut Context<Self>,
    ) {
        if self.current_id().as_deref() != Some(id) {
            return;
        }
        let offset = self.traffic_scroll.offset().y;
        let bottom = self.traffic_scroll.max_offset().y;
        let following = -offset >= bottom - px(8.);
        self.traffic.push(entry);
        if self.traffic.len() > TRAFFIC_CAP {
            self.traffic.drain(..TRAFFIC_TRIM);
        }
        if following {
            self.traffic_scroll.scroll_to_bottom();
        }
        self.control_pane.update(cx, |_, cx| cx.notify());
    }

    pub(super) fn restart_agent(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::Restart { id }, cx);
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

    pub(super) fn finish_conversation(&mut self, id: &str, archive: bool, cx: &mut Context<Self>) {
        let Some((p, t)) = self.locate_conversation(id) else {
            return;
        };
        let finished = self.projects[p].threads[t].status.closed();
        let status = if archive {
            ThreadStatus::Archived
        } else if finished {
            ThreadStatus::Idle
        } else {
            ThreadStatus::Completed
        };
        if self.demo_mode {
            self.projects[p].threads[t].status = status;
        } else {
            self.command(
                Command::SetStatus {
                    id: id.to_owned(),
                    status,
                },
                cx,
            );
        }
        self.menu = None;
        self.refresh_runtime_views(cx);
    }

    pub(super) fn project_agents_stopped(&self, ix: usize) -> bool {
        self.projects
            .get(ix)
            .into_iter()
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

    pub(super) fn force_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(project) = self.projects.get(ix) else {
            return;
        };
        let command = Command::ForceProject {
            id: project.config.id.clone(),
        };
        self.machine_request(&project.machine, command, cx, |app, result, _, cx| {
            if let Err(error) = result {
                app.notify_toast(&error, cx);
            }
        });
    }

    pub(super) fn force_conversation(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.command(Command::ForceStop { id }, cx);
        }
    }

    fn connected_machines(cx: &App) -> Vec<String> {
        client::connections(cx)
            .iter()
            .filter(|c| c.state == client::State::Connected)
            .map(|c| c.machine.clone())
            .collect()
    }

    pub(super) fn stop_all(&mut self, cx: &mut Context<Self>) {
        for machine in Self::connected_machines(cx) {
            let name = machines::name(&machine);
            let several = client::connections(cx).len() > 1;
            self.machine_request(&machine, Command::StopAll, cx, move |app, result, _, cx| {
                let message = match result {
                    Ok(value) => match value["stopped"].as_array().map_or(0, Vec::len) {
                        0 => "No agents were running.".to_owned(),
                        1 => "Stopped 1 agent.".to_owned(),
                        count => format!("Stopped {count} agents."),
                    },
                    Err(error) => error,
                };
                app.notify_toast(
                    &if several {
                        format!("{name}: {message}")
                    } else {
                        message
                    },
                    cx,
                );
            });
        }
    }

    pub(super) fn request_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.demo_mode || self.runtime.quitting {
            return true;
        }
        let busy = client::connections(cx).iter().any(|connection| {
            connection.state == client::State::Connected
                && connection.status.clients <= 1
                && self
                    .projects
                    .iter()
                    .filter(|p| p.machine == connection.machine)
                    .flat_map(|p| &p.threads)
                    .any(|t| {
                        self.runtime
                            .conversations
                            .get(&t.id)
                            .is_some_and(|live| live.processing)
                    })
        });
        if !busy {
            return true;
        }
        self.modal = Some("quit");
        self.refresh_runtime_views(cx);
        false
    }

    pub(super) fn save_agent(
        entity: &WeakEntity<Self>,
        machine: &str,
        original: Option<&str>,
        definition: agents::AgentDefinition,
        expected: Option<agents::AgentDefinition>,
        overwrite: bool,
        cx: &mut App,
    ) -> Result<String> {
        let (demo, select_first) = entity.read_with(cx, |app, _| {
            (
                app.demo_mode,
                original.is_none() && app.catalog(machine).entries.is_empty(),
            )
        })?;
        let saved = if demo {
            entity.update(cx, move |app, _| {
                app.catalog_mut(machine)
                    .save(original, definition, expected.as_ref(), overwrite)
            })??
        } else {
            let command = Command::SaveAgent {
                original: original.map(str::to_owned),
                definition,
                expected,
                overwrite,
            };
            client::request_blocking(machine, command, cx)
                .map_err(anyhow::Error::msg)?
                .as_str()
                .map(str::to_owned)
                .context("The conversation engine returned no agent ID.")?
        };
        entity.update(cx, |app, cx| {
            app.agents_changed(original, Some(&saved), select_first, cx);
        })?;
        Ok(saved)
    }

    pub(super) fn delete_agent(
        entity: &WeakEntity<Self>,
        machine: &str,
        id: &str,
        cx: &mut App,
    ) -> Result<()> {
        let demo = entity.read_with(cx, |app, _| app.demo_mode)?;
        if demo {
            entity.update(cx, |app, _| app.catalog_mut(machine).delete(id))??;
        } else {
            client::request_blocking(machine, Command::DeleteAgent { id: id.to_owned() }, cx)
                .map_err(anyhow::Error::msg)?;
        }
        entity.update(cx, |app, cx| app.agents_changed(Some(id), None, false, cx))
    }

    fn close_main_window(&self, cx: &mut Context<Self>) {
        let handle = self.main_window;
        let owner = cx.weak_entity();
        cx.defer(move |cx| interaction::request_close(handle, owner, cx));
    }

    pub(super) fn quit_stopping_all(&mut self, cx: &mut Context<Self>) {
        if self.runtime.stopping_all {
            return;
        }
        self.runtime.stopping_all = true;
        let machines = Self::connected_machines(cx);
        let waiting = std::rc::Rc::new(std::cell::Cell::new(machines.len()));
        for machine in machines {
            let waiting = std::rc::Rc::clone(&waiting);
            self.machine_request(&machine, Command::StopAll, cx, move |app, result, _, cx| {
                if let Err(error) = result {
                    app.notify_toast(&error, cx);
                }
                waiting.set(waiting.get() - 1);
                if waiting.get() == 0 {
                    app.runtime.stopping_all = false;
                    app.runtime.quitting = true;
                    app.modal = None;
                    app.close_main_window(cx);
                }
                cx.notify();
            });
        }
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
                status: ThreadStatus::Idle,
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

    pub(super) fn show_prompt(&mut self, cx: &mut Context<Self>) {
        if self.modal == Some("prompt") {
            cx.notify();
            return;
        }
        if self.modal.is_some() {
            // Another dialog is open; the prompt waits for it to close.
            return;
        }
        self.open_modal_later("prompt", cx);
    }

    fn open_modal_later(&mut self, modal: &'static str, cx: &mut Context<Self>) {
        self.modal = Some(modal);
        let handle = self.main_window;
        let owner = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = cx.update_window(handle.into(), |_, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if app.modal == Some(modal) {
                        app.prompt_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        app.open_modal(window, cx);
                    }
                });
            });
        });
        cx.notify();
    }

    pub(super) fn apply_delta(&mut self, machine: &str, delta: Delta, cx: &mut Context<Self>) {
        let range = self.machine_range(machine);
        match delta {
            Delta::Project {
                previous,
                mut workspace,
            } => {
                let key = previous.as_deref().unwrap_or(&workspace.config.id);
                if let Some(old) = &previous {
                    ui_state::rename(machine, old, &workspace.config.id);
                }
                machine.clone_into(&mut workspace.machine);
                workspace.config.opened_at = ui_state::opened_at(machine, &workspace.config.id);
                if let Some(project) = self.projects[range].iter_mut().find(|p| p.config.id == key)
                {
                    project.config = workspace.config;
                } else {
                    self.insert_project(workspace, false);
                }
                self.sync_regions(&Action::Project(self.project), cx);
            }
            Delta::ProjectRemoved { id } => {
                if let Some(ix) = self.projects[range.clone()]
                    .iter()
                    .position(|p| p.config.id == id)
                {
                    self.remove_project_at(range.start + ix, cx);
                }
            }
            Delta::Agents { entries, errors } => {
                let catalog = self.catalog_mut(machine);
                catalog.entries = entries;
                catalog.errors = errors;
                self.agents_changed(None, None, false, cx);
            }
            Delta::Notice { message } => {
                self.notify_toast(&self.about_machine(machine, &message), cx);
            }
            // A login page the agent wants open, in this computer's browser.
            Delta::OpenUrl { url } => {
                if url.starts_with("https://") || url.starts_with("http://") {
                    cx.open_url(&url);
                }
            }
            delta => {
                let id = match &delta {
                    Delta::Live { id, .. } | Delta::Text { id, .. } | Delta::Thought { id, .. } => {
                        Some(id.clone())
                    }
                    _ => None,
                };
                let streamed = matches!(delta, Delta::Text { .. } | Delta::Thought { .. });
                protocol::apply(
                    &mut self.projects[range],
                    &mut self.runtime.conversations,
                    &delta,
                );
                if id.is_some() && id == self.current_id() {
                    if let Some((p, t)) = id.as_deref().and_then(|id| self.locate_conversation(id))
                    {
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
                if streamed && let Some(id) = id {
                    self.refresh_streamed(&id, cx);
                    return;
                }
            }
        }
        self.refresh_runtime_views(cx);
    }

    fn refresh_streamed(&self, id: &str, cx: &mut Context<Self>) {
        if self.current_id().as_deref() == Some(id) {
            self.resync_transcript(cx);
            self.side_panel.update(cx, |_, cx| cx.notify());
        }
        self.chat_list
            .update(cx, |list, cx| list.update_streamed(self, id, cx));
    }

    pub(super) fn engine_stopped(
        &mut self,
        machine: &str,
        unexpected: bool,
        cx: &mut Context<Self>,
    ) {
        let ids: HashSet<&str> = self.projects[self.machine_range(machine)]
            .iter()
            .flat_map(|p| &p.threads)
            .map(|t| t.id.as_str())
            .collect();
        for (_, live) in self
            .runtime
            .conversations
            .iter_mut()
            .filter(|(id, _)| ids.contains(id.as_str()))
        {
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
            self.notify_toast(
                &self.about_machine(machine, "Conversation engine stopped unexpectedly"),
                cx,
            );
        }
        self.refresh_runtime_views(cx);
    }
}
