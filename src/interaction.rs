//! `Adeline::act`, the dispatcher every `Action` goes through, and sending the
//! composer's message.
use super::*;
use std::fmt::Write as _;

impl Adeline {
    pub(super) fn act(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_open_project()
            && !matches!(
                action,
                Action::Project(_)
                    | Action::AddProject
                    | Action::SaveProject
                    | Action::AddAgent
                    | Action::Agents
                    | Action::Agent(_)
                    | Action::AgentMenu
                    | Action::Projects
                    | Action::Machines
                    | Action::Machine(_)
                    | Action::MachineRetry(_)
                    | Action::ManageMachines
                    | Action::MachineUpgrade(_)
                    | Action::ConfirmUpgrade
                    | Action::AnswerPrompt(_)
                    | Action::FolderMachine(_)
                    | Action::ProjectMachine(_)
                    | Action::BrowseTo(_)
                    | Action::BrowsePick(_)
                    | Action::AppMenu
                    | Action::AppSettings
                    | Action::AgentSettings
                    | Action::About
                    | Action::QuitApp
                    | Action::Close
                    | Action::StopAll
                    | Action::QuitStopAll
                    | Action::FinishInBackground
                    | Action::StartEngine
                    | Action::EngineRetry
                    | Action::EngineWait
                    | Action::EngineStopOld
                    | Action::HideToolCalls
                    | Action::SubmitOnEnter
                    | Action::ToggleMode(_)
                    | Action::RemoveClosedProject(_)
                    | Action::UndoProjectRemoval(_)
                    | Action::RenameProject(_)
                    | Action::SaveRename
                    | Action::ToggleProjectSort
                    | Action::OpenFolder
            )
        {
            return;
        }
        if let Some(menu) = action.menu_target() {
            if matches!(action, Action::AgentMenu | Action::Agents) {
                client::refresh_harnesses(&self.current_machine(), false, cx);
            }
            if matches!(action, Action::AgentMenu) && !self.demo_mode && self.selected.is_some() {
                window.push_notification(
                    "This conversation's agent and execution settings are fixed.",
                    cx,
                );
                return;
            }
            self.open_commands(menu, window, cx);
            return;
        }
        let changed = action.clone();
        let previous_modal = self.modal;
        match action {
            Action::AppSettings => settings::open(
                window.window_handle().downcast::<Root>().unwrap(),
                cx.weak_entity(),
                cx,
            ),
            Action::AgentSettings => {
                let id = self
                    .current_id()
                    .and_then(|id| self.conversation_agent(&id))
                    .or_else(|| self.selected_agent.clone())
                    .filter(|id| {
                        self.agent_catalog()
                            .entries
                            .iter()
                            .any(|entry| &entry.id == id)
                    });
                settings::open_agent_page(
                    window.window_handle().downcast::<Root>().unwrap(),
                    cx.weak_entity(),
                    self.current_machine(),
                    id,
                    cx,
                );
            }
            Action::AddAgent => settings::open_agent(
                window.window_handle().downcast::<Root>().unwrap(),
                cx.weak_entity(),
                self.current_machine(),
                cx,
            ),
            Action::ConfigureModeSettings => settings::open_mode(
                window.window_handle().downcast::<Root>().unwrap(),
                cx.weak_entity(),
                self.section,
                cx,
            ),
            Action::QuitApp => {
                let handle = window.window_handle().downcast::<Root>().unwrap();
                let owner = cx.weak_entity();
                cx.defer(move |cx| settings::request_close(handle, owner, cx));
            }
            Action::About => self.modal = Some("about"),
            Action::AddProject => {
                self.name_input
                    .update(cx, |state, cx| state.set_value("", window, cx));
                self.project_directory_input
                    .update(cx, |state, cx| state.set_value("", window, cx));
                self.project_error = None;
                self.project_machine = self.current_machine();
                self.modal = Some("add-project");
            }
            Action::ProjectMachine(machine) => {
                if machine != self.project_machine {
                    self.project_machine = machine;
                    // A folder path means nothing on another machine.
                    self.project_directory_input
                        .update(cx, |state, cx| state.set_value("", window, cx));
                }
            }
            Action::SaveProject => self.create_project(window, cx),
            Action::Settings => {
                self.name_input.update(cx, |state, cx| {
                    state.set_value(self.workspace().config.name.clone(), window, cx);
                });
                self.project_directory_input.update(cx, |state, cx| {
                    state.set_value(
                        self.workspace()
                            .config
                            .directory
                            .to_string_lossy()
                            .into_owned(),
                        window,
                        cx,
                    );
                });
                self.selected_tint = self.project_tints[self.project];
                self.project_error = None;
                self.project_machine = self.workspace().machine.clone();
                self.modal = Some("settings");
            }
            Action::SaveSettings => self.save_project_settings(window, cx),
            Action::RenameProject(ix) => {
                self.menu = None;
                self.name_input.update(cx, |state, cx| {
                    state.set_value(self.projects[ix].config.name.clone(), window, cx);
                });
                self.rename_project = Some(ix);
                self.project_error = None;
                self.modal = Some("rename-project");
            }
            Action::SaveRename => self.save_project_name(window, cx),
            Action::DeleteProject => self.begin_project_delete(cx),
            Action::ConfirmDeleteProject => self.confirm_project_delete(cx),
            Action::ForceDeleteProject => self.force_project_delete(cx),
            Action::Close if self.modal == Some("browser") && self.browser_return.is_some() => {
                self.browser = None;
                self.modal = self.browser_return.take();
            }
            Action::Close => {
                self.cancel_project_delete(cx);
                self.menu = None;
                self.modal = None;
                window.close_dialog(cx);
            }
            Action::Project(ix) => {
                self.open_projects[ix] = true;
                if self.project != ix {
                    self.selected = None;
                    self.filter = 0;
                    self.agent_filter = None;
                    self.query
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    // Agents belong to a machine.
                    if self.projects[ix].machine != self.current_machine() {
                        self.project = ix;
                        self.agents_changed(None, None, false, cx);
                    }
                }
                self.project = ix;
                self.section = Section::Chats;
                window.set_window_title(&format!("{} · Adeline", self.workspace().config.name));
                self.record_project_opened(ix, window, cx);
            }
            Action::RemoveClosedProject(id) => self.remove_closed_project(&id, window, cx),
            Action::UndoProjectRemoval(id) => self.undo_project_removal(&id, window, cx),
            Action::ToggleProjectSort => {
                self.project_sort = match self.project_sort {
                    project_bar::ProjectSort::Recent => project_bar::ProjectSort::Name,
                    project_bar::ProjectSort::Name => project_bar::ProjectSort::Recent,
                };
                self.project_list_scroll.scroll_to_item(0);
            }
            Action::OpenFolder => {
                self.menu = None;
                let checked = machines::checked();
                if let [machine] = checked.as_slice() {
                    self.browse_folder_to_open(machine.clone(), window, cx);
                } else {
                    // Several machines: ask which one's folders to browse.
                    self.modal = Some("folder-machine");
                }
            }
            Action::FolderMachine(machine) => {
                // A remote machine's browser takes the dialog's place.
                self.modal = None;
                self.browse_folder_to_open(machine, window, cx);
            }
            Action::Machine(machine) => {
                let checked = !machines::is_checked(&machine);
                match machines::set_checked(&machine, checked) {
                    Ok(()) => client::sync(cx),
                    Err(error) => window.push_notification(error, cx),
                }
            }
            Action::MachineRetry(machine) => client::retry(&machine, cx),
            Action::ManageMachines => {
                self.menu = None;
                settings::open_machines(
                    window.window_handle().downcast::<Root>().unwrap(),
                    cx.weak_entity(),
                    cx,
                );
            }
            Action::MachineUpgrade(machine) => {
                self.menu = None;
                self.upgrade_machine = Some(machine);
                self.modal = Some("upgrade");
            }
            Action::ConfirmUpgrade => {
                self.modal = None;
                if let Some(machine) = self.upgrade_machine.take() {
                    client::upgrade(&machine, cx);
                }
            }
            Action::AnswerPrompt(accept) => {
                let (_, _, host_key) = client::prompt(cx).unwrap_or_default();
                let answer = accept.then(|| {
                    if host_key {
                        "yes".to_owned()
                    } else {
                        self.prompt_input.read(cx).value().to_string()
                    }
                });
                self.prompt_input
                    .update(cx, |state, cx| state.set_value("", window, cx));
                client::answer_prompt(answer, cx);
                if client::prompt(cx).is_none() {
                    self.modal = None;
                } else {
                    window.focus(&self.prompt_input.focus_handle(cx), cx);
                }
            }
            Action::BrowseTo(path) => self.browse(Some(path), cx),
            Action::BrowsePick(file) => self.browse_pick(file, window, cx),
            Action::CloseProject(ix) => {
                if let Some(next) = close_project_tab(&mut self.open_projects, self.project, ix) {
                    if next != self.project {
                        self.act(Action::Project(next), window, cx);
                    }
                } else {
                    self.selected = None;
                    self.section = Section::Chats;
                    self.composer
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    window.set_window_title("Adeline");
                }
            }
            Action::Section(section) => {
                if !config::current().general.features.enabled(section) {
                    return;
                }
                self.section = section;
                self.query
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
            Action::Chat(ix) => {
                self.selected = Some(ix);
                self.activity.reset();
                self.projects[self.project].threads[ix].mark_read();
                if !self.demo_mode {
                    self.mark_conversation_read(cx);
                }
            }
            Action::NewChat => {
                self.section = Section::Chats;
                self.selected = None;
                self.filter = 0;
                self.query
                    .update(cx, |state, cx| state.set_value("", window, cx));
                self.composer
                    .update(cx, |state, cx| state.set_value("", window, cx));
                if !self.demo_mode && self.agent_catalog().entries.len() == 1 {
                    self.selected_agent = Some(self.agent_catalog().entries[0].id.clone());
                }
                window.focus(&self.composer.focus_handle(cx), cx);
            }
            // Agent and search combine, so changing one keeps the other.
            Action::AgentFilter(agent) => self.agent_filter = agent,
            Action::ClearChatFilters => {
                self.filter = 0;
                self.agent_filter = None;
                self.query
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
            Action::ShowCompleted => self.show_completed = !self.show_completed,
            Action::ShowArchived => self.show_archived = !self.show_archived,
            Action::SubmitOnEnter => {
                let submit = !config::current().modes.chats.submit_on_enter;
                if let Err(error) =
                    config::update(|settings| settings.modes.chats.submit_on_enter = submit)
                {
                    window.push_notification(error, cx);
                }
                let submit = config::current().modes.chats.submit_on_enter;
                self.composer
                    .update(cx, |state, cx| state.set_submit_on_enter(submit, cx));
                self.composer_region.update(cx, |_, cx| cx.notify());
            }
            Action::HideToolCalls => {
                if let Err(error) = config::update(|settings| {
                    settings.modes.chats.hide_tool_calls = !settings.modes.chats.hide_tool_calls;
                }) {
                    window.push_notification(error, cx);
                }
                self.transcript
                    .update(cx, |view, cx| view.sync(self, false, cx));
            }
            Action::ToggleLeftPanel => self.left_panel_open[0] = !self.left_panel_open[0],
            Action::ToggleSidePanel => self.side_panel_open[0] = !self.side_panel_open[0],
            Action::Complete => {
                if !self.demo_mode {
                    self.complete_conversation(false, cx);
                } else if let Some(ix) = self.selected {
                    let thread = &mut self.projects[self.project].threads[ix];
                    thread.status = if thread.status == "completed" {
                        "idle"
                    } else {
                        "completed"
                    }
                    .into();
                }
            }
            Action::ArchiveChat(ix) => {
                if let Some(id) = self.workspace().threads.get(ix).map(|t| t.id.clone()) {
                    self.finish_conversation(&id, true, cx);
                }
            }
            Action::Agent(id) => {
                if self
                    .agent_catalog()
                    .entries
                    .iter()
                    .any(|entry| entry.id == id)
                {
                    self.selected_agent = Some(id);
                    // A new chat takes the newly chosen agent's permission default.
                    self.new_chat_permission = None;
                } else {
                    self.notify_toast("The selected agent is no longer available.", cx);
                }
            }
            Action::Speed(ix) => self.speed = ix,
            Action::Permission(ix) => {
                if self.selected.is_none() {
                    self.new_chat_permission = Some(agents::PermissionMode::ALL[ix]);
                } else if self.demo_mode {
                    self.permission = ix;
                } else {
                    self.set_conversation_permission(ix, cx);
                }
            }
            Action::ToggleMode(section) => {
                if let Err(error) =
                    config::update(|settings| settings.general.features.toggle(section))
                {
                    window.push_notification(format!("Could not save settings: {error}"), cx);
                }
                if !config::current().general.features.enabled(self.section) {
                    self.section = Section::Chats;
                }
                cx.refresh_windows();
            }
            Action::Send => self.send(window, cx),
            Action::Stop => self.stop_conversation(cx),
            Action::ForceStop => self.force_conversation(cx),
            Action::StopAll => self.stop_all(cx),
            Action::QuitStopAll => self.quit_stopping_all(cx),
            Action::FinishInBackground => self.quit_in_background(cx),
            Action::StartEngine => client::connect(&self.current_machine(), cx),
            Action::EngineRetry => client::connect(machines::LOCAL, cx),
            Action::EngineWait => client::replace_old(false, cx),
            Action::EngineStopOld => client::replace_old(true, cx),
            Action::RetryPrompt => self.retry_prompt(cx),
            Action::RetryStorage => self.retry_storage(cx),
            Action::ReplaceSession => self.replace_session(cx),
            Action::PermissionResponse(option) => self.answer_permission(option, cx),
            Action::ReplyTo(message) => {
                if let Some(text) = self.message_text(message) {
                    let mut quote = String::new();
                    for line in text.lines() {
                        let _ = writeln!(quote, "> {line}");
                    }
                    self.composer.update(cx, |state, cx| {
                        let lead = if state.value().trim().is_empty() {
                            ""
                        } else {
                            "\n\n"
                        };
                        state.insert(format!("{lead}{quote}\n"), window, cx);
                    });
                    window.focus(&self.composer.focus_handle(cx), cx);
                }
            }
            Action::CopyMessage(message) => {
                if let Some(text) = self.message_text(message) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Action::Fork(message) => self.fork_conversation(message, window, cx),
            Action::ToggleTool(id) => {
                if !self.runtime.expanded_tools.remove(&id) {
                    self.runtime.expanded_tools.insert(id);
                }
                self.transcript
                    .update(cx, |view, cx| view.sync(self, false, cx));
            }
            Action::Decision(ix) => {
                if let Some(thread) = self.selected {
                    let id = self.workspace().threads[thread].id.clone();
                    if let Some(decision) = self.projects[self.project]
                        .decisions
                        .iter_mut()
                        .find(|decision| decision.thread_id == id)
                    {
                        decision.selected = Some(ix);
                        decision.resolved = true;
                    }
                    self.projects[self.project].threads[thread].status = "idle".into();
                }
            }
            Action::AddFile | Action::AddDirectory
                if self.workspace().machine != machines::LOCAL && !self.demo_mode =>
            {
                // A remote project's files are on its machine.
                let directory = matches!(action, Action::AddDirectory);
                self.open_browser(project_ui::BrowserPurpose::Attach { directory }, cx);
            }
            Action::AddFile | Action::AddDirectory => {
                let directory = matches!(action, Action::AddDirectory);
                let selection = cx.prompt_for_paths(PathPromptOptions {
                    files: !directory,
                    directories: directory,
                    multiple: false,
                    prompt: Some(
                        if directory {
                            "Add a directory"
                        } else {
                            "Add a file"
                        }
                        .into(),
                    ),
                });
                cx.spawn_in(window, async move |this, cx| {
                    let result = selection.await;
                    let _ = this.update_in(cx, |app, window, cx| match result {
                        Ok(Ok(Some(paths))) => {
                            for path in paths {
                                app.attach_path(&path.display().to_string(), window, cx);
                            }
                        }
                        Ok(Ok(None)) => {}
                        _ => window.push_notification("Could not open the file picker.", cx),
                    });
                })
                .detach();
            }
            Action::Tint(ix) => self.selected_tint = ix,
            Action::AppMenu
            | Action::ModeSettings
            | Action::Machines
            | Action::Projects
            | Action::Agents
            | Action::AgentMenu
            | Action::InsertFiles => unreachable!("command surfaces handled above"),
        }
        if matches!(
            changed,
            Action::ShowCompleted
                | Action::ShowArchived
                | Action::ToggleLeftPanel
                | Action::ToggleSidePanel
        ) && let Err(error) = self.save_settings(&changed)
        {
            window.push_notification(format!("Could not save settings: {error}"), cx);
        }
        self.sync_regions(&changed, cx);
        if self.modal.is_some() && previous_modal.is_none() {
            self.open_modal(window, cx);
        } else if self.modal.is_none() && previous_modal.is_some() {
            window.close_dialog(cx);
        }
        cx.notify();
    }

    /// Adds `@"path"` to the composer.
    pub(super) fn attach_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut value = self.composer.read(cx).value().to_string();
        if !value.is_empty() && !value.ends_with(char::is_whitespace) {
            value.push(' ');
        }
        let _ = write!(value, "@\"{path}\" ");
        self.composer
            .update(cx, |state, cx| state.set_value(value, window, cx));
    }

    /// The text of a message in the open chat.
    fn message_text(&self, message: usize) -> Option<String> {
        let thread = &self.workspace().threads[self.selected?];
        Some(thread.messages.get(message)?.text.clone())
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != Section::Chats {
            return;
        }
        if !self.demo_mode {
            self.send_real(window, cx);
            return;
        }
        let prompt = self.composer.read(cx).value().trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        let ix = if let Some(ix) = self.selected {
            ix
        } else {
            let id = format!("local-{}", self.workspace().threads.len());
            self.projects[self.project].threads.insert(
                0,
                Thread {
                    id,
                    title: short(&prompt, 100),
                    provider: if self.selected_agent.as_deref() == Some("claude-code") {
                        "claude"
                    } else {
                        "codex"
                    }
                    .into(),
                    status: "idle".into(),
                    ..Default::default()
                },
            );
            self.selected = Some(0);
            0
        };
        let thread = &mut self.projects[self.project].threads[ix];
        thread.push_message(Message {
            role: "user".into(),
            text: prompt,
            read: true,
            ..Default::default()
        });
        thread.push_message(Message { role: "assistant".into(), text: "I've added this to our local demo chat. We can work through the next step here. This preview uses sample responses and doesn't run commands or connect to external services.".into(), read: true, ..Default::default() });
        thread.status = "idle".into();
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
    }
}

fn close_project_tab(open: &mut [bool], active: usize, closing: usize) -> Option<usize> {
    *open.get_mut(closing)? = false;
    if open.get(active).copied().unwrap_or(false) {
        return Some(active);
    }
    (closing + 1..open.len())
        .chain(0..closing)
        .find(|&ix| open[ix])
}

#[cfg(test)]
mod project_tab_tests {
    use super::close_project_tab;
    #[test]
    fn closing_tabs_preserves_an_open_selection() {
        let mut open = [true, true, true];
        assert_eq!(close_project_tab(&mut open, 1, 0), Some(1));
        assert_eq!(close_project_tab(&mut open, 1, 1), Some(2));
        assert_eq!(close_project_tab(&mut open, 2, 2), None);
    }
}
