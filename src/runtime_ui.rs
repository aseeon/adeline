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
    /// Sends a command to the open project's engine; `done` runs with its reply.
    pub(super) fn request(
        &self,
        command: Command,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<Value, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.machine_request(&self.current_machine(), command, cx, done);
    }

    /// Sends a command to one machine's engine; `done` runs with its reply.
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

    /// What the engine reports about the open chat.
    pub(super) fn current_live(&self) -> Option<&Live> {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
    }

    pub(super) fn conversation_processing(&self) -> bool {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .is_some_and(|r| r.processing)
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

    /// Sends the composer's message and files. During a turn it is queued,
    /// or with `now` delivered at once (scope R30).
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

    /// The open conversation's offered options, live or from its last session.
    pub(super) fn conversation_options(&self) -> Vec<conversation::SessionOption> {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .map(|live| live.options.clone())
            .unwrap_or_default()
    }

    /// Sets a model, effort, mode or other option for the open conversation
    /// only. It never changes the agent definition (scope R17).
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
            if let Some(live) = self.runtime.conversations.get_mut(&id) {
                if let Some(found) = live.options.iter_mut().find(|o| o.id == option)
                    && let conversation::OptionKind::Select { current, .. } = &mut found.kind
                {
                    current.clone_from(&value);
                }
                if let Some(found) = live.options.iter_mut().find(|o| o.id == option)
                    && let conversation::OptionKind::Boolean { current } = &mut found.kind
                {
                    *current = value == "true";
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

    /// Takes a queued message out; with `edit`, back into the composer (scope R31).
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

    /// Logs the open chat's agent in with one of its methods (scope R47).
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
                command: execution.command,
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

    /// Starts or stops receiving the open chat's ACP traffic for the traffic tab.
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

    /// A line of the watched chat's traffic arrived.
    pub(super) fn traffic_arrived(
        &mut self,
        id: &str,
        entry: conversation::TrafficEntry,
        cx: &mut Context<Self>,
    ) {
        if self.current_id().as_deref() != Some(id) {
            return;
        }
        // Following the end unless the reader scrolled up.
        let offset = self.traffic_scroll.offset().y;
        let bottom = self.traffic_scroll.max_offset().y;
        let following = -offset >= bottom - px(8.);
        self.traffic.push(entry);
        if self.traffic.len() > 5000 {
            self.traffic.drain(..1000);
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

    /// Connected machines, for actions that reach every engine.
    fn connected_machines(cx: &App) -> Vec<String> {
        client::connections(cx)
            .iter()
            .filter(|c| c.state == client::State::Connected)
            .map(|c| c.machine.clone())
            .collect()
    }

    /// Stops every agent of every connected machine's engine, for every client.
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

    /// Whether the window may close now. Quitting the last client of an
    /// engine while one of its turns is active asks first.
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
        let machines = Self::connected_machines(cx);
        let waiting = std::rc::Rc::new(std::cell::Cell::new(machines.len()));
        for machine in machines {
            let waiting = waiting.clone();
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

    /// The open project's key and chat, to find them again after the
    /// project list changes.
    fn place(&self) -> (Option<String>, Option<String>) {
        (
            self.has_open_project().then(|| self.workspace().key()),
            self.current_id(),
        )
    }

    /// Finds the open project and chat again by `place`.
    fn restore(&mut self, (current, selected): (Option<String>, Option<String>)) {
        let found = current.and_then(|key| self.projects.iter().position(|p| p.key() == key));
        self.project = found
            .or_else(|| self.open_projects.iter().position(|open| *open))
            .unwrap_or(0);
        self.selected = if found.is_some() {
            selected.and_then(|id| {
                self.workspace()
                    .threads
                    .iter()
                    .position(|thread| thread.id == id)
            })
        } else {
            None
        };
    }

    /// Where a machine's projects sit in the list, which keeps each
    /// machine's projects together.
    fn machine_range(&self, machine: &str) -> std::ops::Range<usize> {
        let start = self.projects.iter().position(|p| p.machine == machine);
        let Some(start) = start else {
            let order = machines::all();
            let rank = |m: &str| order.iter().position(|o| o == m).unwrap_or(order.len());
            let at = self
                .projects
                .iter()
                .position(|p| rank(&p.machine) > rank(machine))
                .unwrap_or(self.projects.len());
            return at..at;
        };
        let end = start
            + self.projects[start..]
                .iter()
                .take_while(|p| p.machine == machine)
                .count();
        start..end
    }

    /// Adds a project at the end of its machine's projects. Returns its index.
    pub(super) fn insert_project(&mut self, project: Workspace, open: bool) -> usize {
        let at = self.machine_range(&project.machine).end;
        self.projects.insert(at, project);
        self.open_projects.insert(at, open);
        self.project_tints
            .insert(at, at % theme::project_colors().len());
        if self.project >= at && self.projects.len() > 1 {
            self.project += 1;
        }
        at
    }

    /// Replaces a machine's projects with `projects`, in the same place.
    fn replace_machine_projects(
        &mut self,
        machine: &str,
        mut projects: Vec<Workspace>,
        open_all: bool,
    ) {
        let place = self.place();
        let open: HashSet<String> = self
            .projects
            .iter()
            .zip(&self.open_projects)
            .filter(|(_, open)| **open)
            .map(|(project, _)| project.key())
            .collect();
        let tints: HashMap<String, usize> = self
            .projects
            .iter()
            .zip(&self.project_tints)
            .map(|(project, tint)| (project.key(), *tint))
            .collect();
        let range = self.machine_range(machine);
        for project in &mut projects {
            machine.clone_into(&mut project.machine);
        }
        let gone: Vec<String> = self.projects[range.clone()]
            .iter()
            .flat_map(|p| &p.threads)
            .map(|t| t.id.clone())
            .collect();
        for id in gone {
            self.runtime.conversations.remove(&id);
        }
        let start = range.start;
        let opened: Vec<bool> = projects
            .iter()
            .map(|project| open_all || open.contains(&project.key()))
            .collect();
        let tinted: Vec<usize> = projects
            .iter()
            .enumerate()
            .map(|(i, project)| {
                tints
                    .get(&project.key())
                    .copied()
                    .unwrap_or([0, 2, 3][(start + i).min(2)])
            })
            .collect();
        self.projects.splice(range.clone(), projects);
        self.open_projects.splice(range.clone(), opened);
        self.project_tints.splice(range, tinted);
        self.restore(place);
    }

    /// Takes a machine's whole state, keeping this window's tabs and selection.
    pub(super) fn apply_snapshot(
        &mut self,
        machine: &str,
        snapshot: Snapshot,
        cx: &mut Context<Self>,
    ) {
        let first = self.loaded_machines.insert(machine.to_owned());
        let mut projects = snapshot.projects;
        ui_state::apply(machine, &mut projects);
        self.replace_machine_projects(machine, projects, first);
        self.runtime.conversations.extend(snapshot.live);
        let catalog = self.catalog_mut(machine);
        catalog.entries = snapshot.agents;
        catalog.errors = snapshot.agent_errors;
        if self.selected_agent.is_none() && self.agent_catalog().entries.len() == 1 {
            self.selected_agent = Some(self.agent_catalog().entries[0].id.clone());
        }
        if !snapshot.errors.is_empty() {
            self.notify_toast(
                &self.about_machine(machine, &snapshot.errors.join("\n")),
                cx,
            );
        }
        self.agents_changed(None, None, false, cx);
        self.refresh_runtime_views(cx);
    }

    /// A message about a machine, named when several machines show.
    fn about_machine(&self, machine: &str, message: &str) -> String {
        if machines::checked().len() > 1 {
            format!("{}: {message}", machines::name(machine))
        } else {
            message.to_owned()
        }
    }

    /// An unchecked machine's projects and conversations leave the window.
    pub(super) fn machine_unchecked(&mut self, machine: &str, cx: &mut Context<Self>) {
        let place = self.place();
        self.replace_machine_projects(machine, Vec::new(), false);
        self.catalogs.remove(machine);
        self.loaded_machines.remove(machine);
        if self.has_open_project() {
            self.restore(place);
        } else {
            self.selected = None;
        }
        self.agents_changed(None, None, false, cx);
        self.sync_regions(&Action::Project(self.project), cx);
        self.refresh_runtime_views(cx);
    }

    /// Demo mode: a checked machine shows its bundled projects again.
    pub(super) fn machine_checked(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.demo_mode {
            let projects = load()
                .into_iter()
                .filter(|project| project.machine == machine)
                .collect();
            self.replace_machine_projects(machine, projects, true);
            self.catalogs
                .insert(machine.to_owned(), agents::AgentCatalog::new(true));
        }
        self.sync_regions(&Action::Project(self.project), cx);
        self.refresh_runtime_views(cx);
    }

    /// A machine's connection changed state: its banner and labels follow.
    pub(super) fn refresh_machine(&mut self, _machine: &str, cx: &mut Context<Self>) {
        self.header_region.update(cx, |_, cx| cx.notify());
        self.refresh_runtime_views(cx);
    }

    /// A machine reconnected and caught up on what it missed.
    pub(super) fn machine_resumed(&mut self, machine: &str, cx: &mut Context<Self>) {
        self.refresh_machine(machine, cx);
    }

    /// Asks whether to restart an older remote engine, which stops its agents.
    pub(super) fn ask_upgrade(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.modal.is_some() {
            return;
        }
        self.upgrade_machine = Some(machine.to_owned());
        self.open_modal_later("upgrade", cx);
    }

    /// Shows the waiting SSH prompt.
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

    /// Opens a dialog from outside an action, once the current update ends.
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
                protocol::apply(
                    &mut self.projects[range],
                    &mut self.runtime.conversations,
                    &delta,
                );
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

    /// A machine's engine went away: nothing runs there until it starts again.
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

    /// A full-window state while the only checked machine has no state to show.
    pub(super) fn engine_screen(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let checked = machines::checked();
        // With several machines, each one's state shows in the machine selector.
        let [machine] = checked.as_slice() else {
            return None;
        };
        let connection = client::connection(machine, cx)?;
        if machine != machines::LOCAL {
            return self.remote_screen(machine, connection, cx);
        }
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
        Some(Self::state_screen(title, detail, buttons, cx))
    }

    /// The full-window state of a remote machine that has nothing to show yet.
    fn remote_screen(
        &self,
        machine: &str,
        connection: &client::Connection,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let name = machines::name(machine);
        let retry = || {
            vec![
                self.button(
                    "machine-retry",
                    "Retry",
                    Action::MachineRetry(machine.to_owned()),
                    cx,
                )
                .primary(),
            ]
        };
        let (title, buttons) = match &connection.state {
            client::State::Connecting | client::State::Starting if !connection.loaded => {
                (format!("Connecting to {name}…"), Vec::new())
            }
            client::State::Disconnected(_) if !connection.loaded => {
                (format!("Can't reach {name}"), retry())
            }
            client::State::SignInFailed(_) => (format!("Sign-in to {name} failed"), retry()),
            client::State::Unavailable(_) if !connection.loaded => {
                (format!("{name} is unavailable"), retry())
            }
            client::State::UpgradeNeeded { .. } if !connection.loaded => (
                format!("{name} runs an older Adeline"),
                vec![
                    self.button(
                        "machine-upgrade",
                        "Upgrade now",
                        Action::MachineUpgrade(machine.to_owned()),
                        cx,
                    )
                    .danger(),
                ],
            ),
            client::State::LocalUpdateNeeded(_) if !connection.loaded => {
                ("Update Adeline on this computer".into(), retry())
            }
            client::State::Unsupported(_) => (format!("{name} isn't supported"), Vec::new()),
            _ => return None,
        };
        Some(Self::state_screen(
            title,
            connection.state.detail(),
            buttons,
            cx,
        ))
    }

    fn state_screen(
        title: String,
        detail: Option<String>,
        buttons: Vec<Button>,
        cx: &Context<Self>,
    ) -> AnyElement {
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
            .into_any_element()
    }

    /// A strip above the content while the open project's machine shows data
    /// without its engine.
    pub(super) fn engine_banner(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let machine = self.current_machine();
        let connection = client::connection(&machine, cx)?;
        let name = machines::name(&machine);
        let retry = || {
            Some(self.button(
                "machine-retry",
                "Retry",
                Action::MachineRetry(machine.clone()),
                cx,
            ))
        };
        let (message, button) = match &connection.state {
            client::State::Stopped { unexpected } => (
                self.about_machine(
                    &machine,
                    if *unexpected {
                        "Conversation engine stopped unexpectedly"
                    } else {
                        "Conversation engine stopped"
                    },
                ),
                Some(self.button("start-engine", "Start engine", Action::StartEngine, cx)),
            ),
            client::State::Connecting | client::State::Starting if connection.loaded => {
                if machine == machines::LOCAL {
                    (
                        self.about_machine(&machine, "Starting conversation engine…"),
                        None,
                    )
                } else {
                    (
                        format!("Reconnecting to {name}… Showing what was last received."),
                        None,
                    )
                }
            }
            state if machine != machines::LOCAL && !state.usable() && connection.loaded => (
                format!(
                    "{}. Showing what was last received; changes wait until it reconnects.",
                    match state.label() {
                        "Disconnected" => format!("{name} is disconnected"),
                        label => format!("{name} isn't connected: {}", label.to_lowercase()),
                    }
                ),
                if matches!(state, client::State::UpgradeNeeded { .. }) {
                    Some(self.button(
                        "machine-upgrade",
                        "Upgrade…",
                        Action::MachineUpgrade(machine.clone()),
                        cx,
                    ))
                } else {
                    retry()
                },
            ),
            _ => return None,
        };
        Some(
            row()
                .id("engine-banner")
                .role(Role::Status)
                .aria_label(message.clone())
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
}
