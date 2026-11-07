//! `Adeline::act`, the dispatcher every `Action` goes through, sending the
//! composer's message, saving the preferences `act` toggles, and closing the
//! main window, which asks before quitting with agents still working.
use super::*;
use adeline_core::conversation::{MessageRole, ThreadStatus};
use std::fmt::Write as _;

impl Adeline {
    fn switch_project(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let docs = self.section == Section::Docs;
        let same = self.project == ix && self.open_projects[ix];
        let switched = self.project != ix;
        if !same {
            self.docs_leave(window, cx);
        }
        self.open_projects[ix] = true;
        if self.project != ix {
            self.selected = None;
            self.agent_filter = None;
            clear_input(&self.query, window, cx);
            if self.projects[ix].machine != self.current_machine() {
                self.project = ix;
                self.agents_changed(None, None, false, cx);
            }
        }
        self.project = ix;
        if !(docs && same) {
            self.section = Section::Chats;
        }
        window.set_window_title(&format!("{} · Adeline", self.workspace().config.name));
        self.record_project_opened(ix, window, cx);
        if docs && !same {
            self.docs_enter(None, window, cx);
        }
        if switched {
            self.open_last_active_chat(window, cx);
        }
    }

    fn close_project(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = close_project_tab(&mut self.open_projects, self.project, ix) {
            if next != self.project {
                self.act(Action::Project(next), window, cx);
            }
        } else {
            self.docs_leave(window, cx);
            self.selected = None;
            self.section = Section::Chats;
            clear_input(&self.composer, window, cx);
            window.set_window_title("Adeline");
        }
    }

    fn pick_attachment(&mut self, directory: bool, window: &mut Window, cx: &mut Context<Self>) {
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
                        if directory {
                            app.attach_path(&path.display().to_string(), window, cx);
                        } else {
                            app.attach_local(path, cx);
                        }
                    }
                }
                Ok(Ok(None)) => {}
                _ => window.push_notification(toast("Could not open the file picker."), cx),
            });
        })
        .detach();
    }

    pub(super) fn act(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_open_project() && !action.works_without_project() {
            return;
        }
        if let Some(menu) = action.menu_target() {
            if matches!(action, Action::AgentMenu | Action::Agents) {
                client::refresh_harnesses(&self.current_machine(), false, cx);
            }
            if matches!(action, Action::AgentMenu) && !self.demo_mode && self.selected.is_some() {
                window.push_notification(
                    toast("This conversation's agent and execution settings are fixed."),
                    cx,
                );
                return;
            }
            self.open_commands(menu, window, cx);
            return;
        }
        if let Action::Section(section) = action
            && !config::current().general.features.enabled(section)
        {
            return;
        }
        let changed = action.clone();
        let previous_modal = self.modal;
        match action.area() {
            Area::Project => self.act_project(action, window, cx),
            Area::Chat => self.act_chat(action, window, cx),
            Area::App => self.act_app(&action, window, cx),
            Area::Menu(_) => unreachable!("menu actions returned above"),
        }
        if matches!(
            changed,
            Action::ShowCompleted
                | Action::ShowArchived
                | Action::ToggleLeftPanel
                | Action::ToggleSidePanel
                | Action::PanelTab(_)
                | Action::TodoList
        ) && let Err(error) = self.save_settings(&changed)
        {
            window.push_notification(toast(format!("Could not save settings: {error:#}")), cx);
        }
        self.sync_regions(&changed, cx);
        if self.modal.is_some() && previous_modal.is_none() {
            self.open_modal(window, cx);
        } else if self.modal.is_none() && previous_modal.is_some() {
            window.close_dialog(cx);
            if !self.focus.contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
        }
        cx.notify();
    }

    fn act_project(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::AddProject => self.begin_add_project(window, cx),
            Action::ProjectMachine(machine) => {
                if machine != self.project_machine {
                    self.project_machine = machine;
                    clear_input(&self.project_directory_input, window, cx);
                }
            }
            Action::SaveProject => self.create_project(window, cx),
            Action::Settings => self.begin_project_settings(window, cx),
            Action::SaveSettings => self.save_project_settings(window, cx),
            Action::RenameProject(ix) => self.begin_project_rename(ix, window, cx),
            Action::SaveRename => self.save_project_name(window, cx),
            Action::DeleteProject => self.begin_project_delete(cx),
            Action::ConfirmDeleteProject => self.confirm_project_delete(cx),
            Action::ForceDeleteProject => self.force_project_delete(cx),
            Action::Project(ix) => self.switch_project(ix, window, cx),
            Action::RemoveClosedProject(id) => self.remove_closed_project(&id, window, cx),
            Action::UndoProjectRemoval(id) => self.undo_project_removal(&id, window, cx),
            Action::ToggleProjectSort => {
                self.project_sort = match self.project_sort {
                    project_bar::ProjectSort::Recent => project_bar::ProjectSort::Name,
                    project_bar::ProjectSort::Name => project_bar::ProjectSort::Recent,
                };
                self.project_list_scroll.scroll_to_item(0);
            }
            Action::OpenFolder => self.choose_folder_to_open(window, cx),
            Action::FolderMachine(machine) => {
                self.modal = None;
                self.browse_folder_to_open(machine, window, cx);
            }
            Action::CloseProject(ix) if !self.docs_may_close(docs::Then::Close(ix), cx) => {}
            Action::CloseProject(ix) => self.close_project(ix, window, cx),
            Action::BrowseTo(path) => self.browse(Some(path), cx),
            Action::BrowsePick(file) => self.browse_pick(file, window, cx),
            Action::Tint(ix) => self.selected_tint = ix,
            Action::Machine(machine) => self.toggle_machine(&machine, window, cx),
            Action::MachineRetry(machine) => client::retry(&machine, cx),
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
            Action::AnswerPrompt(accept) => self.answer_host_prompt(accept, window, cx),
            Action::StartEngine => client::connect(&self.current_machine(), cx),
            Action::EngineRetry => client::connect(machines::LOCAL, cx),
            Action::EngineWait => client::replace_old(false, cx),
            Action::EngineStopOld => client::replace_old(true, cx),
            _ => unreachable!("Action::area routes only project actions here"),
        }
    }

    fn act_chat(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Section(section) => self.switch_section(section, window, cx),
            Action::Chat(ix) => self.show_chat(ix, cx),
            Action::NewChat => self.new_chat(window, cx),
            Action::AgentFilter(agent) => self.agent_filter = agent,
            Action::ClearChatFilters => {
                self.agent_filter = None;
                clear_input(&self.query, window, cx);
            }
            Action::ShowCompleted => self.show_completed = !self.show_completed,
            Action::ShowArchived => self.show_archived = !self.show_archived,
            Action::ToggleThought(key) => {
                toggle_member(&mut self.open_thoughts, key);
                self.resync_transcript(cx);
            }
            Action::OptionMenu(category) => {
                if self.selected.is_some() && !self.conversation_processing() {
                    self.open_commands(chat::option_menu_name(category), window, cx);
                }
            }
            Action::SetOption {
                category,
                option,
                value,
            } => self.set_option(category, option, value, cx),
            Action::EditQueued(queued) => self.take_queued(queued, true, window, cx),
            Action::EditLastQueued => self.edit_last_queued(window, cx),
            Action::RemoveQueued(queued) => self.take_queued(queued, false, window, cx),
            Action::SendQueuedNow(queued) => self.send_queued_now(queued, cx),
            Action::DismissError(message) => self.dismiss_error(message, cx),
            Action::RemoveAttachment(ix) => {
                if ix < self.attachments.len() {
                    self.attachments.remove(ix);
                }
                self.attachment_error = None;
            }
            Action::PreviewImage(message, ix) => self.preview_image(message, ix, window, cx),
            Action::Login(method) => self.login(&method, cx),
            Action::Restart => self.restart_agent(cx),
            Action::Complete => self.complete_conversation(false, cx),
            Action::ArchiveChat(ix) => {
                if let Some(id) = self.workspace().threads.get(ix).map(|t| t.id.clone()) {
                    self.finish_conversation(&id, true, cx);
                }
            }
            Action::Agent(id) => self.pick_agent(id, cx),
            Action::Speed(ix) => self.speed = ix,
            Action::Send => self.send(false, window, cx),
            Action::SendNow => self.send(true, window, cx),
            Action::Stop => self.stop_conversation(cx),
            Action::ForceStop => self.force_conversation(cx),
            Action::RetryPrompt => self.retry_prompt(cx),
            Action::RetryStorage => self.retry_storage(cx),
            Action::ReplaceSession => self.replace_session(cx),
            Action::PermissionResponse(option) => self.answer_permission(option, cx),
            Action::ReplyTo(message) => self.reply_to(message, window, cx),
            Action::CopyMessage(message) => {
                if let Some(text) = self.message_text(message) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Action::Fork(message) => self.fork_conversation(message, window, cx),
            Action::ToggleTool(id) => {
                toggle_member(&mut self.runtime.expanded_tools, id);
                self.resync_transcript(cx);
            }
            Action::Decision(ix) => self.decide(ix),
            Action::AddFile | Action::AttachFile | Action::AddDirectory
                if self.workspace().machine != machines::LOCAL && !self.demo_mode =>
            {
                let directory = matches!(action, Action::AddDirectory);
                self.open_browser(project_ui::BrowserPurpose::Attach { directory }, cx);
            }
            Action::AddFile | Action::AttachFile | Action::AddDirectory => {
                self.pick_attachment(matches!(action, Action::AddDirectory), window, cx);
            }
            Action::Docs(act) => self.docs_act(act, window, cx),
            Action::OpenCard { route, chat } => self.open_route(&route, Some(chat), window, cx),
            _ => unreachable!("Action::area routes only chat actions here"),
        }
    }

    fn act_app(&mut self, action: &Action, window: &mut Window, cx: &mut Context<Self>) {
        let root = kit_root(window);
        match action {
            Action::AppSettings => settings::open(root, cx.weak_entity(), cx),
            Action::AgentSettings => self.open_agent_settings(false, root, cx),
            Action::EditAgent => self.open_agent_settings(true, root, cx),
            Action::AddAgent => {
                settings::open_agent(root, cx.weak_entity(), self.current_machine(), cx);
            }
            Action::ConfigureModeSettings => {
                settings::open_mode(root, cx.weak_entity(), self.section, cx);
            }
            Action::ManageMachines => {
                self.menu = None;
                settings::open_machines(root, cx.weak_entity(), cx);
            }
            Action::QuitApp => {
                let owner = cx.weak_entity();
                cx.defer(move |cx| request_close(root, owner, cx));
            }
            Action::About => self.modal = Some("about"),
            Action::Close if self.modal == Some("browser") && self.browser_return.is_some() => {
                self.browser = None;
                self.modal = self.browser_return.take();
            }
            Action::Close => self.close_overlays(window, cx),
            Action::StopAll => self.stop_all(cx),
            Action::QuitStopAll => self.quit_stopping_all(cx),
            Action::FinishInBackground => self.quit_in_background(cx),
            Action::SubmitOnEnter => self.toggle_submit_on_enter(window, cx),
            Action::HideToolCalls => self.toggle_transcript_setting(
                |s| s.modes.chats.hide_tool_calls = !s.modes.chats.hide_tool_calls,
                window,
                cx,
            ),
            Action::ShowThinking => self.toggle_transcript_setting(
                |s| s.modes.chats.show_thinking = !s.modes.chats.show_thinking,
                window,
                cx,
            ),
            Action::ToggleLeftPanel => self.left_panel_open = !self.left_panel_open,
            Action::ToggleSidePanel => {
                self.side_panel_open = !self.side_panel_open;
                self.watch_traffic(cx);
            }
            Action::TrafficView => self.show_traffic(cx),
            Action::PanelTab(tab) => self.toggle_side_tab(*tab, cx),
            Action::TodoList => self.toggle_side_tab(panes::SideTab::Plan, cx),
            Action::CopyTraffic => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.traffic_text()));
            }
            Action::ToggleMode(section) => self.toggle_mode(*section, window, cx),
            _ => unreachable!("Action::area routes only app actions here"),
        }
    }

    fn open_agent_settings(&self, edit: bool, root: WindowHandle<Root>, cx: &mut Context<Self>) {
        let entries = &self.agent_catalog().entries;
        let known = |id: &String| entries.iter().any(|entry| &entry.id == id);
        let conversation = self
            .current_id()
            .and_then(|id| self.conversation_agent(&id))
            .or_else(|| {
                let thread = self.workspace().threads.get(self.selected?)?;
                let name = self.agent_name(&thread.provider);
                let entry = entries.iter().find(|e| e.definition.name == name)?;
                Some(entry.id.clone())
            })
            .filter(known);
        let id = if edit {
            conversation.or_else(|| entries.first().map(|entry| entry.id.clone()))
        } else {
            conversation.or_else(|| self.selected_agent.clone().filter(known))
        };
        settings::open_agent_page(root, cx.weak_entity(), self.current_machine(), id, cx);
    }

    fn close_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_project_delete(cx);
        self.docs_cancel();
        if self.menu.take().is_some() {
            self.refocus_after_menu(window, cx);
        }
        self.modal = None;
        window.close_dialog(cx);
    }

    fn toggle_machine(&mut self, machine: &str, window: &mut Window, cx: &mut Context<Self>) {
        let checked = !machines::is_checked(machine);
        match machines::set_checked(machine, checked) {
            Ok(()) => client::sync(cx),
            Err(error) => window.push_notification(toast(format!("{error:#}")), cx),
        }
    }

    fn answer_host_prompt(&mut self, accept: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (_, _, host_key) = client::prompt(cx).unwrap_or_default();
        let answer = accept.then(|| {
            if host_key {
                "yes".to_owned()
            } else {
                self.prompt_input.read(cx).value().to_string()
            }
        });
        clear_input(&self.prompt_input, window, cx);
        client::answer_prompt(answer, cx);
        if client::prompt(cx).is_none() {
            self.modal = None;
        } else {
            window.focus(&self.prompt_input.focus_handle(cx), cx);
        }
    }

    fn switch_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        if section != Section::Docs {
            self.docs_leave(window, cx);
            self.section = section;
        }
        clear_input(&self.query, window, cx);
        if section == Section::Docs {
            self.docs_enter(None, window, cx);
        }
    }

    fn show_chat(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.activity.reset();
        self.attachment_error = None;
        self.slash = None;
        self.watch_traffic(cx);
        self.projects[self.project].threads[ix].mark_read();
        self.mark_conversation_read(cx);
    }

    fn new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != Section::Docs {
            self.section = Section::Chats;
        }
        self.selected = None;
        clear_input(&self.query, window, cx);
        clear_input(&self.composer, window, cx);
        if !self.demo_mode && self.agent_catalog().entries.len() == 1 {
            self.selected_agent = Some(self.agent_catalog().entries[0].id.clone());
        }
        window.focus(&self.composer.focus_handle(cx), cx);
    }

    fn toggle_submit_on_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let submit = !config::current().modes.chats.submit_on_enter;
        toast_err(
            config::update(|settings| settings.modes.chats.submit_on_enter = submit),
            window,
            cx,
        );
        let submit = config::current().modes.chats.submit_on_enter;
        self.composer
            .update(cx, |state, cx| state.set_submit_on_enter(submit, cx));
        self.composer_region.update(cx, |_, cx| cx.notify());
    }

    fn toggle_transcript_setting(
        &mut self,
        toggle: impl FnOnce(&mut config::Settings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        toast_err(config::update(toggle), window, cx);
        self.resync_transcript(cx);
    }

    fn show_traffic(&mut self, cx: &mut Context<Self>) {
        if self.verbose {
            self.side_panel_open = true;
            self.side_tab = panes::SideTab::Traffic;
            self.watch_traffic(cx);
        }
    }

    fn toggle_side_tab(&mut self, tab: panes::SideTab, cx: &mut Context<Self>) {
        self.side_panel_open = !(self.side_panel_open && self.side_tab == tab);
        self.side_tab = tab;
        self.watch_traffic(cx);
    }

    fn edit_last_queued(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(last) = self
            .current_live()
            .and_then(|live| live.queued.last())
            .map(|q| q.id)
        {
            self.take_queued(last, true, window, cx);
        }
    }

    fn dismiss_error(&mut self, message: String, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            let thread = &self.workspace().threads[index];
            let turn = thread.turn_of(thread.messages.len());
            self.dismissed_errors
                .insert((thread.id.clone(), turn, message));
            self.resync_transcript(cx);
        }
    }

    fn pick_agent(&mut self, id: String, cx: &mut Context<Self>) {
        if self
            .agent_catalog()
            .entries
            .iter()
            .any(|entry| entry.id == id)
        {
            self.selected_agent = Some(id);
        } else {
            self.notify_toast("The selected agent is no longer available.", cx);
        }
    }

    fn toggle_mode(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = config::update(|settings| settings.general.features.toggle(section)) {
            window.push_notification(toast(format!("Could not save settings: {error:#}")), cx);
        }
        if !config::current().general.features.enabled(self.section) {
            self.docs_leave(window, cx);
            self.section = Section::Chats;
        }
        if section == Section::Docs {
            let machines: Vec<String> = client::connections(cx)
                .iter()
                .map(|c| c.machine.clone())
                .collect();
            for machine in machines {
                self.docs_sync_engine(&machine, cx);
            }
        }
        cx.refresh_windows();
    }

    fn reply_to(&mut self, message: usize, window: &mut Window, cx: &mut Context<Self>) {
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

    fn decide(&mut self, ix: usize) {
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
            self.projects[self.project].threads[thread].status = ThreadStatus::Idle;
        }
    }

    pub(super) fn open_last_active_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != Section::Chats || !self.has_open_project() {
            return;
        }
        let last_active = self
            .workspace()
            .threads
            .iter()
            .enumerate()
            .filter(|(_, thread)| thread.status != ThreadStatus::Archived)
            .max_by_key(|(_, thread)| thread.last_activity())
            .map(|(i, _)| i);
        if let Some(chat) = last_active {
            self.act(Action::Chat(chat), window, cx);
        }
    }

    pub(super) fn load_settings(&mut self) {
        let s = config::current();
        self.show_completed = s.modes.chats.show_completed_chats;
        self.show_archived = s.modes.chats.show_archived_chats;
        self.left_panel_open = s.modes.chats.show_left_panel;
        self.side_panel_open = s.modes.chats.show_agent_activity;
    }
    fn save_settings(&self, action: &Action) -> Result<()> {
        config::update(|s| match action {
            Action::ShowCompleted => s.modes.chats.show_completed_chats = self.show_completed,
            Action::ShowArchived => s.modes.chats.show_archived_chats = self.show_archived,
            Action::ToggleLeftPanel => {
                s.modes.chats.show_left_panel = self.left_panel_open;
            }
            Action::ToggleSidePanel | Action::PanelTab(_) | Action::TodoList => {
                s.modes.chats.show_agent_activity = self.side_panel_open;
            }
            _ => (),
        })
    }

    pub(super) fn attach_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut value = self.composer.read(cx).value().to_string();
        if !value.is_empty() && !value.ends_with(char::is_whitespace) {
            value.push(' ');
        }
        let _ = write!(value, "@\"{path}\" ");
        self.composer
            .update(cx, |state, cx| state.set_value(value, window, cx));
    }

    fn message_text(&self, message: usize) -> Option<String> {
        let thread = &self.workspace().threads[self.selected?];
        Some(thread.messages.get(message)?.text.clone())
    }

    fn send(&mut self, now: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.section, Section::Chats | Section::Docs) {
            return;
        }
        if !self.demo_mode {
            self.send_real(now, window, cx);
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
                    title: conversation::title_from(&prompt),
                    provider: if self.selected_agent.as_deref() == Some("claude-code") {
                        "claude"
                    } else {
                        "codex"
                    }
                    .into(),
                    status: ThreadStatus::Idle,
                    ..Default::default()
                },
            );
            self.selected = Some(0);
            0
        };
        let id = self.projects[self.project].threads[ix].id.clone();
        if let Some(live) = self.runtime.conversations.get_mut(&id)
            && live.processing
            && !now
        {
            let next = live.queued.iter().map(|q| q.id).max().unwrap_or(0) + 1;
            live.queued.push(protocol::Queued {
                id: next,
                text: prompt,
                files: self.attachments.drain(..).map(|a| a.name).collect(),
            });
            clear_input(&self.composer, window, cx);
            return;
        }
        let attachments = std::mem::take(&mut self.attachments);
        let thread = &mut self.projects[self.project].threads[ix];
        thread.push_message(Message {
            role: MessageRole::User,
            text: prompt,
            read: true,
            attachments,
            ..Default::default()
        });
        thread.push_message(Message { role: MessageRole::Assistant, text: "I've added this to our local demo chat. We can work through the next step here. This preview uses sample responses and doesn't run commands or connect to external services.".into(), read: true, ..Default::default() });
        thread.status = ThreadStatus::Idle;
        clear_input(&self.composer, window, cx);
    }
}

pub(super) fn request_close(owner: WindowHandle<Root>, entity: WeakEntity<Adeline>, cx: &mut App) {
    cx.defer(move |cx| {
        if settings::can_close(owner, cx) && can_quit(owner, &entity, cx) {
            let _ = owner.update(cx, |_, window, _| window.remove_window());
        }
    });
}
fn can_quit(owner: WindowHandle<Root>, entity: &WeakEntity<Adeline>, cx: &mut App) -> bool {
    cx.update_window(owner.into(), |_, window, cx| {
        entity
            .update(cx, |app, cx| {
                let opening = !matches!(app.modal, Some("quit" | "doc-unsaved"));
                let ready = app.request_quit(cx);
                if !ready && opening {
                    app.open_modal(window, cx);
                }
                ready
            })
            .unwrap_or(false)
    })
    .unwrap_or(false)
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
        let mut open = [true, true, true];
        assert_eq!(close_project_tab(&mut open, 2, 2), Some(0));
    }
}
