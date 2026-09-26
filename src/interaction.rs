use super::*;
impl Adeline {
    pub(super) fn act(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open_projects.iter().any(|&open| open)
            && !matches!(
                action,
                Action::Project(_)
                    | Action::AddProject
                    | Action::SaveProject
                    | Action::AddAgent
                    | Action::SaveAgent
                    | Action::Agents
                    | Action::Agent(_)
                    | Action::Projects
                    | Action::Machines
                    | Action::Machine(_)
                    | Action::AppMenu
                    | Action::AppSettings
                    | Action::About
                    | Action::KeyboardShortcuts
                    | Action::QuitApp
                    | Action::Close
            )
        {
            return;
        }
        if matches!(
            action,
            Action::EditLine(_) | Action::CheckLine(_) | Action::SaveLine | Action::Raw
        ) && self.document.is_some_and(|i| {
            let doc = &self.workspace().docs[i];
            doc.revision != doc.prepared_revision
        }) {
            return;
        }
        let changed = action.clone();
        if matches!(
            action,
            Action::Project(_)
                | Action::Section(_)
                | Action::Collection(_)
                | Action::NewWorkflow
                | Action::RunWorkflow
                | Action::Close
        ) {
            self.editing_workflow = false;
        }
        let previous_count = self.workspace().threads.len();
        let changed_thread = match &action {
            Action::Chat(i) => Some(*i),
            Action::Complete | Action::Decision(_) | Action::Send | Action::RunWorkflow => {
                self.selected
            }
            _ => None,
        };
        let previous_flags = changed_thread.map(|i| self.workspace().threads[i].flags());
        if self.menu == Some("app") && !matches!(action, Action::AppMenu | Action::Close) {
            self.menu = None;
            window.focus(&self.focus);
        }
        if self.menu == Some("mode-settings")
            && !matches!(action, Action::ModeSettings | Action::Close)
        {
            self.menu = None;
            window.focus(&self.focus);
        }
        match action {
            Action::Group(_)
            | Action::SendGroup
            | Action::NewGroup
            | Action::Issue(_)
            | Action::IssueFilter(_)
            | Action::IssueScroll(_)
            | Action::IssueStatus(_)
            | Action::NewIssue
            | Action::BoardTool(_)
            | Action::BoardUndo => self.collaboration_action(&action, window, cx),
            Action::AppSettings => {
                self.menu = None;
                settings::open(window.window_handle().downcast::<Adeline>().unwrap(), cx);
            }
            Action::LeftPanel(section) => {
                self.left_panel_open[section as usize] = !self.left_panel_open[section as usize];
                self.dragging = false;
                self.sync_regions(&Action::ToggleLeftPanel, cx);
                self.sync_content_regions(&Action::ToggleLeftPanel, cx);
            }
            Action::RightPanel(section) => {
                self.side_panel_open[section as usize] = !self.side_panel_open[section as usize];
                self.sync_content_regions(&Action::ToggleSidePanel, cx);
            }
            Action::Machines => {
                if !config::current().general.features.machine_selector {
                    return;
                }
                self.menu = if self.menu == Some("machines") {
                    None
                } else {
                    Some("machines")
                };
                if self.menu.is_some() {
                    self.machine_query.update(cx, |v, cx| v.set("", cx));
                    window.focus(&self.machine_query.focus_handle(cx));
                } else {
                    window.focus(&self.focus);
                }
            }
            Action::Machine(i) => {
                if !config::current().general.features.machine_selector {
                    return;
                }
                if i < MACHINES.len() {
                    self.machine = i;
                    self.agent = self.machine_agents[i];
                    self.model = 0;
                }
                self.menu = None;
                self.control_pane.update(cx, |_, cx| cx.notify());
                window.focus(&self.focus);
            }
            Action::AppMenu => {
                self.menu = if self.menu == Some("app") {
                    None
                } else {
                    Some("app")
                };
                window.focus(&self.focus);
            }
            Action::About => self.modal = Some("about"),
            Action::KeyboardShortcuts => self.modal = Some("shortcuts"),
            Action::QuitApp => cx.quit(),
            Action::AddProject | Action::AddAgent => {
                self.menu = None;
                self.modal = Some(if matches!(action, Action::AddProject) {
                    "add-project"
                } else {
                    "add-agent"
                });
                self.name_input.update(cx, |v, cx| v.set("", cx));
                window.focus(&self.name_input.focus_handle(cx));
            }
            Action::SaveProject => {
                let name = self.name_input.read(cx).content.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                let i = self.projects.len();
                let project = Workspace {
                    config: Config {
                        id: format!("local-project-{i}"),
                        name,
                        provider: "claude".into(),
                    },
                    ..Default::default()
                };
                self.collaboration
                    .push(collaboration_modes::ProjectCollaboration::seed(&project));
                self.projects.push(project);
                self.open_projects.push(false);
                self.project_tints.push(0);
                self.modal = None;
                self.act(Action::Project(i), window, cx);
                return;
            }
            Action::SaveAgent => {
                let name = self.name_input.read(cx).content.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                if self.available_agents[self.machine]
                    .iter()
                    .any(|&i| self.agents[i].eq_ignore_ascii_case(&name))
                {
                    self.notify_toast("An agent with this name already exists.", cx);
                    return;
                }
                let i = self.agents.len();
                self.agents.push(name);
                self.available_agents[self.machine].push(i);
                self.modal = None;
                self.act(Action::Agent(i), window, cx);
                return;
            }
            Action::CloseProject(i) => {
                let next = close_project_tab(&mut self.open_projects, self.project, i);
                if let Some(next) = next {
                    if self.project != next {
                        self.act(Action::Project(next), window, cx);
                    }
                } else {
                    self.selected = None;
                    self.document = None;
                    self.workflow = None;
                    self.service = None;
                    self.edit_line = None;
                    self.editing_workflow = false;
                    self.modal = None;
                    self.menu = None;
                    self.section = Section::Chats;
                    self.composer.update(cx, |v, cx| v.set("", cx));
                    window.set_window_title("Adeline");
                    self.sync_regions(&Action::Project(self.project), cx);
                    self.sync_content_regions(&Action::Project(self.project), cx);
                }
                window.focus(&self.focus);
                self.header_region.update(cx, |_, cx| cx.notify());
                cx.notify();
                return;
            }
            Action::Project(i) => {
                let was_open = self.open_projects[i];
                self.open_projects[i] = true;
                if self.project == i && was_open {
                    self.menu = None;
                    window.focus(&self.focus);
                    cx.notify();
                    return;
                }
                self.project = i;
                self.section = Section::Chats;
                self.selected = None;
                self.document = None;
                self.workflow = None;
                self.service = None;
                self.menu = None;
                self.filter = 0;
                self.query.update(cx, |v, cx| v.set("", cx));
                self.agent = usize::from(self.workspace().config.provider != "claude");
                window.set_window_title(&format!("{} — Adeline", self.workspace().config.name));
                window.focus(&self.focus);
            }
            Action::Section(s) => {
                if !config::current().general.features.enabled(s) {
                    return;
                }
                self.section = s;
                self.menu = None;
                self.query.update(cx, |v, cx| v.set("", cx));
            }
            Action::Chat(i) => {
                self.selected = Some(i);
                self.menu = None;
                self.expanded_event = None;
                let t = &mut self.projects[self.project].threads[i];
                t.mark_read();
                self.agent = usize::from(t.provider != "claude");
            }
            Action::NewChat => {
                self.section = Section::Chats;
                self.selected = None;
                self.filter = 0;
                self.query.update(cx, |v, cx| v.set("", cx));
                self.composer.update(cx, |v, cx| v.set("", cx));
                window.focus(&self.composer.focus_handle(cx));
            }
            Action::Filter(i) => {
                self.filter = i;
                self.query.update(cx, |v, cx| v.set("", cx));
            }
            Action::ShowCompleted => self.show_completed = !self.show_completed,
            Action::ToggleSidePanel => {
                let index = self.section as usize;
                self.side_panel_open[index] = !self.side_panel_open[index];
            }
            Action::Event(i) => {
                self.expanded_event = if self.expanded_event == Some(i) {
                    None
                } else {
                    Some(i)
                }
            }
            Action::Complete => {
                if let Some(i) = self.selected {
                    let t = &mut self.projects[self.project].threads[i];
                    t.status = if t.status == "completed" {
                        "idle"
                    } else {
                        "completed"
                    }
                    .into();
                }
            }
            Action::ChatMenu => {
                self.menu = if self.menu == Some("chat") {
                    None
                } else {
                    Some("chat")
                }
            }
            Action::Projects => {
                self.menu = if self.menu == Some("projects") {
                    None
                } else {
                    Some("projects")
                };
                if self.menu.is_some() {
                    self.project_query.update(cx, |v, cx| v.set("", cx));
                    window.focus(&self.project_query.focus_handle(cx));
                } else {
                    window.focus(&self.focus);
                }
            }
            Action::ModeSettings => {
                self.menu = if self.menu == Some("mode-settings") {
                    None
                } else {
                    Some("mode-settings")
                };
                window.focus(&self.focus);
            }
            Action::ConfigureModeSettings => {
                self.menu = None;
                settings::open_mode(
                    window.window_handle().downcast::<Adeline>().unwrap(),
                    self.section,
                    cx,
                );
            }
            Action::Settings => {
                self.modal = Some("settings");
                self.menu = None;
                let name = self.workspace().config.name.clone();
                self.name_input.update(cx, |v, cx| v.set(name, cx));
                self.selected_tint = self.project_tints[self.project];
            }
            Action::Close => {
                // Dismiss the popup without closing the content underneath it.
                if self.menu.take().is_some() {
                    window.focus(&self.focus);
                    cx.notify();
                    return;
                }
                self.modal = None;
                self.workflow = None;
                self.edit_line = None;
                window.focus(&self.focus);
            }
            Action::SaveSettings => {
                let n = self.name_input.read(cx).content.trim().to_string();
                if !n.is_empty() {
                    self.projects[self.project].config.name = n;
                }
                self.modal = None;
                self.project_tints[self.project] = self.selected_tint;
                window.set_window_title(&format!("{} — Adeline", self.workspace().config.name));
            }
            Action::AgentMenu => {
                self.menu = if self.menu == Some("agent") {
                    None
                } else {
                    Some("agent")
                }
            }
            Action::Agents => {
                self.menu = if self.menu == Some("agents") {
                    None
                } else {
                    Some("agents")
                };
                if self.menu.is_some() {
                    self.agent_query.update(cx, |v, cx| v.set("", cx));
                    window.focus(&self.agent_query.focus_handle(cx));
                } else {
                    window.focus(&self.focus);
                }
            }
            Action::Agent(i) => {
                if !self.available_agents[self.machine].contains(&i) {
                    return;
                }
                self.agent = i;
                self.machine_agents[self.machine] = i;
                self.model = 0;
                self.menu = None;
            }
            Action::Model(i) => {
                self.model = i;
                self.menu = None;
            }
            Action::Effort(i) => {
                self.effort = i;
                self.menu = None;
            }
            Action::Speed(i) => {
                self.speed = i;
                self.menu = None;
            }
            Action::Permission(i) => self.permission = i,
            Action::ToggleMode(_) | Action::ToggleMachineSelector => {
                if let Err(error) = config::update(|s| {
                    if let Action::ToggleMode(section) = action {
                        s.general.features.toggle(section);
                    } else {
                        s.general.features.machine_selector = !s.general.features.machine_selector;
                    }
                }) {
                    self.toast = Some(format!("Could not save settings: {error}"));
                } else {
                    cx.defer(|cx| {
                        for handle in cx.windows() {
                            if let Some(handle) = handle.downcast::<Adeline>() {
                                let _ = handle.update(cx, |app, window, cx| {
                                    if !config::current().general.features.machine_selector
                                        && app.menu == Some("machines")
                                    {
                                        app.menu = None;
                                        window.focus(&app.focus);
                                    }
                                    if !config::current().general.features.enabled(app.section) {
                                        app.modal = None;
                                        app.act(Action::Section(Section::Chats), window, cx);
                                    }
                                    // Feature visibility is rendered by the separately cached header.
                                    app.header_region.update(cx, |_, cx| cx.notify());
                                    cx.notify();
                                });
                            }
                        }
                    });
                }
            }
            Action::Send => self.send(cx),
            Action::DocsHome => {
                self.document = None;
                self.archived = false;
                self.menu = None;
            }
            Action::ToggleLeftPanel => {
                let index = self.section as usize;
                self.left_panel_open[index] = !self.left_panel_open[index];
                self.dragging = false;
            }
            Action::Document(i) => {
                if self.workspace().docs[i].revision != self.workspace().docs[i].prepared_revision {
                    self.prepare_document(i, cx);
                }
                self.document = Some(i);
                self.left_panel_open[Section::Docs as usize] = true;
                self.menu = None;
            }
            Action::Raw => self.raw = !self.raw,
            Action::Archive => {
                self.archived = !self.archived;
                self.document = None;
            }
            Action::DocMenu => {
                self.menu = if self.menu == Some("document") {
                    None
                } else {
                    Some("document")
                }
            }
            Action::NewDoc => {
                self.projects[self.project].docs.push(Document {
                    title: "Untitled document".into(),
                    filename: "Untitled document.md".into(),
                    content: std::sync::Arc::new(
                        "## A new thought\n\nClick any paragraph to start writing.".into(),
                    ),
                    ..Default::default()
                });
                self.document = Some(self.workspace().docs.len() - 1);
                self.left_panel_open[Section::Docs as usize] = true;
            }
            Action::PinDoc => self.pinned = !self.pinned,
            Action::EditLine(i) => {
                if let Some(d) = self.document {
                    let line = self.workspace().docs[d]
                        .content
                        .lines()
                        .nth(i)
                        .unwrap_or("")
                        .to_owned();
                    self.edit_input.update(cx, |v, cx| v.set(line, cx));
                    self.edit_line = Some(i);
                    self.modal = Some("edit");
                    window.focus(&self.edit_input.focus_handle(cx));
                }
            }
            Action::SaveLine => {
                if let (Some(d), Some(i)) = (self.document, self.edit_line) {
                    let replacement = self.edit_input.read(cx).content.to_string();
                    self.projects[self.project].docs[d].replace_line(i, &replacement);
                }
                self.modal = None;
                self.edit_line = None;
            }
            Action::Format(mark) => {
                if let Some(d) = self.document {
                    use std::fmt::Write as _;
                    let content =
                        std::sync::Arc::make_mut(&mut self.projects[self.project].docs[d].content);
                    let _ = write!(content, "\n\n{mark}New text{mark}");
                    self.notify_toast("Added a text block. Click it to edit.", cx);
                }
            }
            Action::CheckLine(i) => {
                if let Some(d) = self.document {
                    let line = self.workspace().docs[d]
                        .content
                        .lines()
                        .nth(i)
                        .unwrap_or("");
                    let replacement = if line.contains("[x]") {
                        line.replacen("[x]", "[ ]", 1)
                    } else {
                        line.replacen("[ ]", "[x]", 1)
                    };
                    self.projects[self.project].docs[d].replace_line(i, &replacement);
                }
            }
            Action::Workflow(i) => {
                if self.workflow != Some(i) {
                    self.editing_workflow = false;
                }
                self.workflow = Some(i);
                self.side_panel_open[Section::Workflows as usize] = true;
            }
            Action::Collection(s) => {
                self.collection = s;
                self.workflow = None;
            }
            Action::RunWorkflow => {
                if let Some(i) = self.workflow {
                    let prompt = self.workspace().recipes[i].instructions.clone();
                    self.composer.update(cx, |v, cx| v.set(prompt, cx));
                    self.selected = None;
                    self.section = Section::Chats;
                    self.workflow = None;
                    self.send(cx);
                }
            }
            Action::Schedule => {
                if let Some(i) = self.workflow {
                    let r = &mut self.projects[self.project].recipes[i];
                    r.schedule_label = if r.schedule_label == "On demand" {
                        "Every weekday at 9:00"
                    } else {
                        "On demand"
                    }
                    .into();
                }
            }
            Action::Service(i) => {
                self.service = Some(i);
                self.stopped = false;
            }
            Action::StopService => self.stopped = !self.stopped,
            Action::NewService => {
                let number = self
                    .services
                    .iter()
                    .filter(|s| s.project_id == self.workspace().config.id)
                    .count()
                    + 1;
                let mut service = Service {
                    project_id: self.workspace().config.id.clone(),
                    name: format!("New service {number}"),
                    output: "New local demo service. No command is running.\n".into(),
                    ..Default::default()
                };
                service.prepare();
                self.services.push(service);
                self.service = Some(self.services.len() - 1);
                self.stopped = false;
                self.query.update(cx, |v, cx| v.set("", cx));
            }
            Action::Wrap => self.wrap = !self.wrap,
            Action::Follow => self.follow = !self.follow,
            Action::CopyOutput => {
                if let Some(i) = self.service {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        self.services[i].output.clone(),
                    ));
                    self.notify_toast("Output copied", cx);
                }
            }
            Action::Decision(i) => {
                if let Some(t) = self.selected {
                    let id = self.workspace().threads[t].id.clone();
                    if let Some(d) = self.projects[self.project]
                        .decisions
                        .iter_mut()
                        .find(|d| d.thread_id == id)
                    {
                        d.selected = Some(i);
                        d.resolved = true;
                    }
                    self.projects[self.project].threads[t].status = "idle".into();
                }
            }
            Action::InsertFiles => {
                self.menu = if self.menu == Some("files") {
                    None
                } else {
                    Some("files")
                }
            }
            Action::AddFile | Action::AddDirectory => {
                let directory = matches!(action, Action::AddDirectory);
                self.menu = None;
                window.focus(&self.composer.focus_handle(cx));
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
                cx.spawn(async move |this, cx| {
                    let result = selection.await;
                    let _ = this.update(cx, |s, cx| {
                        match result {
                            Ok(Ok(Some(paths))) => {
                                let mut value = s.composer.read(cx).content.to_string();
                                for path in paths {
                                    if !value.is_empty() && !value.ends_with(char::is_whitespace) {
                                        value.push(' ');
                                    }
                                    value = format!("{value}@\"{}\" ", path.display());
                                }
                                s.composer.update(cx, |v, cx| v.set(value, cx));
                            }
                            Ok(Ok(None)) => {}
                            _ => s.notify_toast("Could not open the file picker.", cx),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            Action::Tint(i) => self.selected_tint = i,
            Action::Instructions => self.instructions = !self.instructions,
            Action::NewWorkflow => {
                self.workflow = None;
                self.modal = Some("workflow");
                self.name_input.update(cx, |v, cx| v.set("", cx));
                self.edit_input.update(cx, |v, cx| v.set("", cx));
                window.focus(&self.name_input.focus_handle(cx));
            }
            Action::EditWorkflow => {
                if let Some(i) = self.workflow {
                    if self.editing_workflow {
                        self.editing_workflow = false;
                        window.focus(&self.focus);
                        cx.notify();
                        return;
                    }
                    let r = self.workspace().recipes[i].clone();
                    self.name_input.update(cx, |v, cx| v.set(r.name, cx));
                    self.edit_input
                        .update(cx, |v, cx| v.set(r.instructions, cx));
                    self.editing_workflow = true;
                    self.side_panel_open[Section::Workflows as usize] = true;
                    window.focus(&self.name_input.focus_handle(cx));
                }
            }
            Action::SaveWorkflow => {
                let name = self.name_input.read(cx).content.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                let instructions = self.edit_input.read(cx).content.to_string();
                if let Some(i) = self.workflow {
                    let r = &mut self.projects[self.project].recipes[i];
                    r.name = name;
                    r.instructions = instructions;
                } else {
                    let i = self.workspace().recipes.len();
                    self.projects[self.project].recipes.push(Recipe {
                        id: format!("local-workflow-{i}"),
                        name,
                        instructions,
                        collection: "Yours".into(),
                        schedule_label: "On demand".into(),
                    });
                    self.workflow = Some(i);
                }
                self.section = Section::Workflows;
                self.editing_workflow = false;
                self.side_panel_open[Section::Workflows as usize] = true;
                self.modal = None;
                self.collection = "All".into();
            }
            Action::NewCollection => {
                self.modal = Some("collection");
                self.name_input.update(cx, |v, cx| v.set("", cx));
            }
            Action::SaveCollection => {
                let name = self.name_input.read(cx).content.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                let i = self.workspace().recipes.len();
                self.projects[self.project].recipes.push(Recipe {
                    id: format!("local-workflow-{i}"),
                    name: "New workflow".into(),
                    collection: name.clone(),
                    instructions: "Describe what you want your agent to do.".into(),
                    schedule_label: "On demand".into(),
                });
                self.collection = name;
                self.modal = None;
            }
            Action::EditTitle => {
                if let Some(i) = self.document {
                    let title = self.workspace().docs[i].title.clone();
                    self.name_input.update(cx, |v, cx| v.set(title, cx));
                    self.modal = Some("title");
                }
            }
            Action::SaveTitle => {
                if let Some(i) = self.document {
                    let title = self.name_input.read(cx).content.trim().to_owned();
                    if !title.is_empty() {
                        self.projects[self.project].docs[i].search_title =
                            title.to_lowercase().into();
                        self.projects[self.project].docs[i].title.clone_from(&title);
                        self.projects[self.project].docs[i].filename = format!("{title}.md");
                    }
                }
                self.modal = None;
            }
            Action::ArchiveDoc => {
                if let Some(i) = self.document {
                    let key = (self.project, i);
                    if !self.archived_docs.remove(&key) {
                        self.archived_docs.insert(key);
                    }
                    self.document = None;
                    self.menu = None;
                }
            }
        }
        if !matches!(changed, Action::Project(_)) {
            if self.workspace().threads.len() > previous_count {
                let flags = self.workspace().threads[0].flags();
                self.projects[self.project].update_counts([0; 4], flags);
            } else if let (Some(i), Some(before)) = (changed_thread, previous_flags) {
                let after = self.workspace().threads[i].flags();
                self.projects[self.project].update_counts(before, after);
            }
        }
        if matches!(
            changed,
            Action::NewDoc | Action::SaveLine | Action::CheckLine(_) | Action::Format(_)
        ) && let Some(i) = self.document
        {
            let document = &mut self.projects[self.project].docs[i];
            if matches!(changed, Action::NewDoc | Action::Format(_)) {
                document.revision += 1;
            }
            if document.revision != document.prepared_revision {
                self.prepare_document(i, cx);
            }
        }
        if matches!(
            changed,
            Action::ShowCompleted
                | Action::Raw
                | Action::Archive
                | Action::Collection(_)
                | Action::Wrap
                | Action::Follow
                | Action::LeftPanel(_)
                | Action::RightPanel(_)
                | Action::ToggleLeftPanel
                | Action::ToggleSidePanel
        ) && let Err(error) = self.save_settings(&changed)
        {
            self.toast = Some(format!("Could not save settings: {error}"));
        }
        self.sync_regions(&changed, cx);
        self.sync_content_regions(&changed, cx);
        cx.notify();
    }
    fn send(&mut self, cx: &mut Context<Self>) {
        let prompt = self.composer.read(cx).content.trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        let i = if let Some(i) = self.selected {
            i
        } else {
            let id = format!("local-{}", self.workspace().threads.len());
            self.projects[self.project].threads.insert(
                0,
                Thread {
                    id,
                    title: short(&prompt, 100),
                    provider: if self.agent == 0 { "claude" } else { "codex" }.into(),
                    status: "idle".into(),
                    ..Default::default()
                },
            );
            self.selected = Some(0);
            0
        };
        let t = &mut self.projects[self.project].threads[i];
        t.push_message(Message {
            role: "user".into(),
            text: prompt,
            read: true,
            ..Default::default()
        });
        t.push_message(Message{role:"assistant".into(),text:"I've added this to our local demo chat. We can work through the next step here. This preview uses sample responses and doesn't run commands or connect to external services.".into(),read:true,..Default::default()});
        t.status = "idle".into();
        self.composer.update(cx, |v, cx| v.set("", cx));
    }
}

/// Keep selection on an open tab, preferring the next tab to the right and wrapping.
fn close_project_tab(open: &mut [bool], active: usize, closing: usize) -> Option<usize> {
    *open.get_mut(closing)? = false;
    if open.get(active).copied().unwrap_or(false) {
        return Some(active);
    }
    (closing + 1..open.len())
        .chain(0..closing)
        .find(|&i| open[i])
}

#[cfg(test)]
mod project_tab_tests {
    use super::close_project_tab;

    #[test]
    fn closing_active_tab_selects_the_next_open_tab_and_wraps() {
        let mut open = [true, true, true];
        assert_eq!(close_project_tab(&mut open, 1, 1), Some(2));
        assert_eq!(open, [true, false, true]);
        assert_eq!(close_project_tab(&mut open, 2, 2), Some(0));
        assert_eq!(close_project_tab(&mut open, 0, 0), None);
        assert_eq!(open, [false; 3]);
        open[1] = true;
        assert_eq!(close_project_tab(&mut open, 1, 1), None);
    }

    #[test]
    fn closing_background_tab_keeps_the_active_project() {
        let mut open = [true, true, true];
        assert_eq!(close_project_tab(&mut open, 1, 0), Some(1));
        assert_eq!(open, [false, true, true]);
    }
}
