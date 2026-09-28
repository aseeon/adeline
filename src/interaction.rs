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
                    | Action::AppMenu
                    | Action::AppSettings
                    | Action::About
                    | Action::KeyboardShortcuts
                    | Action::QuitApp
                    | Action::Close
                    | Action::ForceStopAll
                    | Action::HideToolCalls
                    | Action::ToggleMode(_)
                    | Action::ToggleMachineSelector
                    | Action::RemoveClosedProject(_)
                    | Action::UndoProjectRemoval(_)
                    | Action::ToggleProjectSort
                    | Action::OpenFolder
            )
        {
            return;
        }
        if let Some(menu) = action.menu_target() {
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
        let previous_count = self.workspace().threads.len();
        let changed_thread = match &action {
            Action::Chat(ix) => Some(*ix),
            Action::Complete | Action::Decision(_) | Action::Send => self.selected,
            _ => None,
        };
        let previous_flags = changed_thread.map(|ix| self.workspace().threads[ix].flags());
        match action {
            Action::AppSettings => settings::open(
                window.window_handle().downcast::<Root>().unwrap(),
                cx.weak_entity(),
                cx,
            ),
            Action::AddAgent => settings::open_agent(
                window.window_handle().downcast::<Root>().unwrap(),
                cx.weak_entity(),
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
            Action::KeyboardShortcuts => self.modal = Some("shortcuts"),
            Action::AddProject => {
                self.name_input
                    .update(cx, |state, cx| state.set_value("", window, cx));
                self.project_directory_input
                    .update(cx, |state, cx| state.set_value("", window, cx));
                self.project_error = None;
                self.modal = Some("add-project");
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
                self.modal = Some("settings");
            }
            Action::SaveSettings => self.save_project_settings(window, cx),
            Action::DeleteProject => self.begin_project_delete(cx),
            Action::ConfirmDeleteProject => self.confirm_project_delete(cx),
            Action::ForceDeleteProject => self.force_project_delete(cx),
            Action::Close => {
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
                let selection = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Open folder".into()),
                });
                cx.spawn_in(window, async move |this, cx| {
                    let result = selection.await;
                    let _ = this.update_in(cx, |app, window, cx| match result {
                        Ok(Ok(Some(paths))) => {
                            if let Some(directory) = paths.into_iter().next() {
                                app.open_folder(&directory, window, cx);
                            }
                        }
                        Ok(Ok(None)) => {}
                        _ => window.push_notification("Could not open the folder picker.", cx),
                    });
                })
                .detach();
            }
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
                self.expanded_event = None;
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
                if !self.demo_mode && self.agent_catalog.entries.len() == 1 {
                    self.selected_agent = Some(self.agent_catalog.entries[0].id.clone());
                }
                window.focus(&self.composer.focus_handle(cx), cx);
            }
            // Tabs, agent and search combine, so changing one keeps the others.
            Action::Filter(ix) => self.filter = ix,
            Action::AgentFilter(agent) => self.agent_filter = agent,
            Action::ClearChatFilters => {
                self.filter = 0;
                self.agent_filter = None;
                self.query
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
            Action::ShowCompleted => self.show_completed = !self.show_completed,
            Action::HideToolCalls => {
                if let Err(error) = config::update(|settings| {
                    settings.modes.chats.hide_tool_calls = !settings.modes.chats.hide_tool_calls;
                }) {
                    window.push_notification(error, cx);
                }
                self.transcript
                    .update(cx, |view, cx| view.sync(self, false, cx));
            }
            Action::LeftPanel(section) => {
                if section == Section::Chats {
                    self.left_panel_open[0] = !self.left_panel_open[0];
                }
            }
            Action::RightPanel(section) => {
                if section == Section::Chats {
                    self.side_panel_open[0] = !self.side_panel_open[0];
                }
            }
            Action::ToggleLeftPanel => {
                if self.section == Section::Chats {
                    self.left_panel_open[0] = !self.left_panel_open[0];
                }
            }
            Action::ToggleSidePanel => {
                if self.section == Section::Chats {
                    self.side_panel_open[0] = !self.side_panel_open[0];
                }
            }
            Action::Event(ix) => {
                self.expanded_event = (self.expanded_event != Some(ix)).then_some(ix);
            }
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
            Action::ArchiveChat => self.complete_conversation(true, cx),
            Action::Agent(id) => {
                if self
                    .agent_catalog
                    .entries
                    .iter()
                    .any(|entry| entry.id == id)
                {
                    self.selected_agent = Some(id);
                } else {
                    self.notify_toast("The selected agent is no longer available.", cx);
                }
            }
            Action::Machine(ix) => {
                if ix < if self.demo_mode { MACHINES.len() } else { 1 } {
                    self.machine = ix;
                }
            }
            Action::Speed(ix) => self.speed = ix,
            Action::Permission(ix) => {
                if self.demo_mode {
                    self.permission = ix;
                } else {
                    self.set_conversation_permission(ix, cx);
                }
            }
            Action::ToggleMode(_) | Action::ToggleMachineSelector => {
                if let Err(error) = config::update(|settings| {
                    if let Action::ToggleMode(section) = action {
                        settings.general.features.toggle(section);
                    } else {
                        settings.general.features.machine_selector =
                            !settings.general.features.machine_selector;
                    }
                }) {
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
            Action::ForceStopAll => self.force_all(cx),
            Action::RetryPrompt => self.retry_prompt(cx),
            Action::RetryStorage => self.retry_storage(cx),
            Action::ReplaceSession => self.replace_session(cx),
            Action::PermissionResponse(option) => self.answer_permission(option, cx),
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
                            let mut value = app.composer.read(cx).value().to_string();
                            for path in paths {
                                if !value.is_empty() && !value.ends_with(char::is_whitespace) {
                                    value.push(' ');
                                }
                                let _ = write!(value, "@\"{}\" ", path.display());
                            }
                            app.composer
                                .update(cx, |state, cx| state.set_value(value, window, cx));
                        }
                        Ok(Ok(None)) => {}
                        _ => window.push_notification("Could not open the file picker.", cx),
                    });
                })
                .detach();
            }
            Action::Tint(ix) => self.selected_tint = ix,
            Action::ResizePanel(ix, delta) => {
                self.panel_state.update(cx, |state, cx| {
                    if let Some(size) = state.sizes().get(ix).copied() {
                        state.resize_panel(ix, size + config::text_pixels(delta), window, cx);
                    }
                });
            }
            Action::ResetPanels => {
                self.panel_state.update(cx, |state, cx| {
                    state.resize_panel(0, config::text_pixels(360.), window, cx);
                    state.resize_panel(2, config::text_pixels(302.), window, cx);
                });
            }
            Action::AppMenu
            | Action::ModeSettings
            | Action::Machines
            | Action::Projects
            | Action::Agents
            | Action::AgentMenu
            | Action::ChatMenu
            | Action::InsertFiles => unreachable!("command surfaces handled above"),
        }
        if !self.demo_mode
            && matches!(
                changed,
                Action::Chat(_) | Action::Complete | Action::Send | Action::ArchiveChat
            )
        {
            if let Some(project) = self.projects.get_mut(self.project) {
                project.rebuild_counts();
            }
        } else if matches!(
            changed,
            Action::Chat(_) | Action::Complete | Action::Decision(_) | Action::Send
        ) {
            if self.workspace().threads.len() > previous_count {
                let flags = self.workspace().threads[0].flags();
                self.projects[self.project].update_counts([0; 4], flags);
            } else if let (Some(ix), Some(before)) = (changed_thread, previous_flags) {
                let after = self.workspace().threads[ix].flags();
                self.projects[self.project].update_counts(before, after);
            }
        }
        if matches!(
            changed,
            Action::ShowCompleted
                | Action::LeftPanel(_)
                | Action::RightPanel(_)
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
