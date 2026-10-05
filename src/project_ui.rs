//! Projects from the UI: add, rename, settings, delete with undo, the project
//! dialog, and browsing a remote machine's folders.
use super::*;
use std::path::Path;

pub(super) enum BrowserPurpose {
    OpenFolder,
    ProjectFolder,
    Attach { directory: bool },
}

/// A remote machine's folders, listed by its engine.
pub(super) struct Browser {
    pub machine: String,
    pub purpose: BrowserPurpose,
    pub listing: Option<protocol::Listing>,
    pub error: Option<String>,
}

impl Browser {
    fn picks_files(&self) -> bool {
        matches!(self.purpose, BrowserPurpose::Attach { directory: false })
    }
}

/// Joins a name to a path from another machine, with that machine's separator.
fn join_remote(base: &str, name: &str) -> String {
    let separator = if base.contains('\\') || base.chars().nth(1) == Some(':') {
        '\\'
    } else {
        '/'
    };
    if base.ends_with(separator) {
        format!("{base}{name}")
    } else {
        format!("{base}{separator}{name}")
    }
}

/// The last part of a path from any machine.
fn last_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

impl Adeline {
    pub(super) fn create_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().trim().to_owned();
        let directory = self
            .project_directory_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        self.add_project(name, &directory, true, window, cx);
    }

    /// Saves a new project and opens it in a tab. Errors show in the project
    /// dialog when it's `from_dialog`, else as a notification.
    pub(super) fn add_project(
        &mut self,
        name: String,
        directory: &str,
        from_dialog: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let machine = self.project_machine.clone();
        if self.demo_mode {
            if name.is_empty() {
                self.project_error = Some("Enter a project name.".into());
                cx.notify();
                return;
            }
            let id = format!("local-project-{}", self.projects.len());
            let index = self.insert_project(
                Workspace {
                    machine,
                    config: Config {
                        id,
                        name,
                        provider: "claude".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                false,
            );
            if from_dialog {
                self.project_error = None;
                self.modal = None;
            }
            self.act(Action::Project(index), window, cx);
            return;
        }
        let command = protocol::Command::SaveProject {
            original: None,
            name,
            directory: directory.into(),
        };
        let target = machine.clone();
        self.machine_request(
            &machine,
            command,
            cx,
            move |app, result, window, cx| match result {
                Ok(id) => {
                    if from_dialog {
                        app.project_error = None;
                        app.modal = None;
                    }
                    // The engine's change arrived before its reply.
                    if let Some(index) = app.projects.iter().position(|project| {
                        project.machine == target && Some(project.config.id.as_str()) == id.as_str()
                    }) {
                        app.act(Action::Project(index), window, cx);
                    }
                }
                Err(error) if from_dialog => {
                    app.project_error = Some(error);
                    cx.notify();
                }
                Err(error) => window.push_notification(error, cx),
            },
        );
    }

    /// Remembers when this window opened a project, for the projects menu's order.
    pub(super) fn record_project_opened(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = recency::now();
        self.projects[ix].config.opened_at = Some(now);
        if self.demo_mode {
            return;
        }
        let id = self.projects[ix].config.id.clone();
        if let Err(error) = ui_state::record_opened(&self.projects[ix].machine, &id, now) {
            window.push_notification(format!("Couldn't record opening {id}: {error}"), cx);
        }
    }

    /// Saves a project's name and directory through the engine.
    fn save_project_as(
        &mut self,
        ix: usize,
        name: String,
        directory: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let original = self.projects[ix].config.id.clone();
        let machine = self.projects[ix].machine.clone();
        let command = protocol::Command::SaveProject {
            original: Some(original),
            name: name.clone(),
            directory,
        };
        let target = machine.clone();
        self.machine_request(&machine, command, cx, move |app, result, window, cx| {
            match result {
                Ok(id) => {
                    if let Some(ix) = app.projects.iter().position(|project| {
                        project.machine == target && Some(project.config.id.as_str()) == id.as_str()
                    }) {
                        if ix == app.project && app.open_projects[ix] {
                            window.set_window_title(&format!("{name} · Adeline"));
                        }
                        app.project_tints[ix] = app.selected_tint;
                    }
                    app.rename_project = None;
                    app.project_error = None;
                    app.modal = None;
                    app.sync_regions(&Action::Project(app.project), cx);
                }
                Err(error) => app.project_error = Some(error),
            }
            cx.notify();
        });
    }

    pub(super) fn save_project_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_open_project() {
            return;
        }
        let name = self.name_input.read(cx).value().trim().to_owned();
        let directory = self
            .project_directory_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if self.demo_mode {
            if name.is_empty() {
                self.project_error = Some("Enter a project name.".into());
            } else {
                self.projects[self.project].config.name = name;
                self.project_tints[self.project] = self.selected_tint;
                self.project_error = None;
                self.modal = None;
                self.sync_regions(&Action::Project(self.project), cx);
            }
            cx.notify();
            return;
        }
        self.save_project_as(self.project, name, directory.into(), cx);
    }

    /// Renames the project the rename dialog is for, keeping its directory.
    pub(super) fn save_project_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.rename_project.filter(|&ix| ix < self.projects.len()) else {
            return;
        };
        let name = self.name_input.read(cx).value().trim().to_owned();
        if self.demo_mode {
            if name.is_empty() {
                self.project_error = Some("Enter a project name.".into());
            } else {
                self.projects[ix].config.name.clone_from(&name);
                if ix == self.project && self.open_projects[ix] {
                    window.set_window_title(&format!("{name} · Adeline"));
                }
                self.rename_project = None;
                self.project_error = None;
                self.modal = None;
                self.sync_regions(&Action::Project(self.project), cx);
            }
            cx.notify();
            return;
        }
        // Renaming keeps the tab's color.
        self.selected_tint = self.project_tints[ix];
        let directory = self.projects[ix].config.directory.clone();
        self.save_project_as(ix, name, directory, cx);
    }

    pub(super) fn begin_project_delete(&mut self, cx: &mut Context<Self>) {
        if self.demo_mode || !self.has_open_project() {
            return;
        }
        self.delete_project = Some(self.project);
        self.project_error = None;
        self.modal = Some("delete-project");
        cx.notify();
    }

    /// Asks the engine to stop the project's agents and delete it. The engine
    /// removes it everywhere once they've stopped.
    pub(super) fn confirm_project_delete(&mut self, cx: &mut Context<Self>) {
        if self.modal != Some("delete-project") && self.modal != Some("delete-project-shutdown") {
            return;
        }
        let Some(index) = self
            .delete_project
            .filter(|index| *index < self.projects.len())
        else {
            return;
        };
        let id = self.projects[index].config.id.clone();
        let key = self.projects[index].key();
        let machine = self.projects[index].machine.clone();
        self.modal = Some("delete-project-shutdown");
        self.project_error = None;
        self.machine_request(
            &machine,
            protocol::Command::DeleteProject { id },
            cx,
            move |app, result, window, cx| {
                if let Err(error) = result {
                    let pending = app
                        .delete_project
                        .and_then(|ix| app.projects.get(ix))
                        .is_some_and(|project| project.key() == key);
                    if pending && app.modal == Some("delete-project-shutdown") {
                        app.project_error = Some(error);
                        app.open_modal(window, cx);
                    }
                    cx.notify();
                }
            },
        );
        cx.notify();
    }

    pub(super) fn force_project_delete(&mut self, cx: &mut Context<Self>) {
        if self.modal != Some("delete-project-shutdown") {
            return;
        }
        let Some(index) = self
            .delete_project
            .filter(|index| *index < self.projects.len())
        else {
            return;
        };
        self.force_project(index, cx);
    }

    /// Closing the stopping dialog keeps the project; agents already asked to
    /// stop keep stopping.
    pub(super) fn cancel_project_delete(&mut self, cx: &mut Context<Self>) {
        if self.modal == Some("delete-project-shutdown")
            && let Some(project) = self.delete_project.and_then(|ix| self.projects.get(ix))
            && !self.demo_mode
        {
            let id = project.config.id.clone();
            client::request(
                &project.machine.clone(),
                protocol::Command::CancelDeleteProject { id },
                Box::new(|_, _| {}),
                cx,
            );
        }
        self.delete_project = None;
    }

    /// Forgets a deleted project: its tab, runtime state and selection.
    pub(super) fn remove_project_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let ids: Vec<_> = self.projects[index]
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect();
        self.forget_project_runtime(&ids);
        let previous_project = self.project;
        self.projects.remove(index);
        self.open_projects.remove(index);
        self.project_tints.remove(index);
        self.project = if previous_project > index {
            previous_project - 1
        } else {
            previous_project.min(self.projects.len().saturating_sub(1))
        };
        if !self
            .open_projects
            .get(self.project)
            .copied()
            .unwrap_or(false)
        {
            self.project = self
                .open_projects
                .iter()
                .position(|open| *open)
                .unwrap_or(0);
        }
        self.selected = None;
        self.delete_project = None;
        self.project_error = None;
        self.modal = None;
        self.section = Section::Chats;
        let handle = self.main_window;
        let owner = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = cx.update_window(handle.into(), |_, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    app.composer
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    app.query
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    window.set_window_title(if app.has_open_project() {
                        &app.workspace().config.name
                    } else {
                        "Adeline"
                    });
                });
            });
        });
        self.sync_regions(&Action::Project(self.project), cx);
        cx.notify();
    }
}

impl Adeline {
    /// Fills the project dialog from a picked or dropped folder. A new
    /// project takes the folder's name unless the user typed their own.
    pub(super) fn set_project_folder(
        &mut self,
        directory: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote = self.project_machine != machines::LOCAL;
        if !remote && !directory.is_dir() {
            self.project_error = Some("Drop a folder, not a file.".into());
            cx.notify();
            return;
        }
        let folder_name = |path: &Path| last_name(&path.to_string_lossy());
        let previous = folder_name(Path::new(
            self.project_directory_input.read(cx).value().as_ref(),
        ));
        let name = self.name_input.read(cx).value().trim().to_owned();
        if self.modal == Some("add-project") && (name.is_empty() || name == previous) {
            self.name_input.update(cx, |state, cx| {
                state.set_value(folder_name(directory), window, cx);
            });
        }
        self.project_directory_input.update(cx, |state, cx| {
            state.set_value(directory.to_string_lossy().into_owned(), window, cx);
        });
        self.project_error = None;
        cx.notify();
    }

    fn browse_project_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_machine != machines::LOCAL {
            self.open_browser(BrowserPurpose::ProjectFolder, cx);
            return;
        }
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = selection.await;
            let _ = this.update_in(cx, |app, window, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(directory) = paths.first() {
                        app.set_project_folder(directory, window, cx);
                    }
                }
                Ok(Ok(None)) => {}
                _ => window.push_notification("Could not open the folder picker.", cx),
            });
        })
        .detach();
    }

    /// Open folder on `machine`: the native picker for this computer, the
    /// folder browser for a remote machine.
    pub(super) fn browse_folder_to_open(
        &mut self,
        machine: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        machines::set_last_folder(&machine);
        if machine != machines::LOCAL {
            self.project_machine = machine;
            self.open_browser(BrowserPurpose::OpenFolder, cx);
            return;
        }
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
                        app.open_folder(machines::LOCAL, &directory, window, cx);
                    }
                }
                Ok(Ok(None)) => {}
                _ => window.push_notification("Could not open the folder picker.", cx),
            });
        })
        .detach();
    }

    /// Opens the folder browser for a remote machine at its home folder.
    pub(super) fn open_browser(&mut self, purpose: BrowserPurpose, cx: &mut Context<Self>) {
        let machine = match purpose {
            BrowserPurpose::Attach { .. } => self.workspace().machine.clone(),
            _ => self.project_machine.clone(),
        };
        if self.demo_mode {
            self.notify_toast("Demo machines have no folders to browse.", cx);
            return;
        }
        // A project dialog shows the browser in its place, and gets it back after.
        self.browser_return = if matches!(purpose, BrowserPurpose::ProjectFolder) {
            self.modal
        } else {
            None
        };
        self.browser = Some(Browser {
            machine,
            purpose,
            listing: None,
            error: None,
        });
        self.menu = None;
        self.modal = Some("browser");
        cx.notify();
        self.browse(None, cx);
    }

    /// Lists a folder of the browser's machine; its home folder without `path`.
    pub(super) fn browse(&mut self, path: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        let Some(browser) = &self.browser else {
            return;
        };
        let machine = browser.machine.clone();
        self.machine_request(
            &machine,
            protocol::Command::ListDirectory { path },
            cx,
            |app, result, _, cx| {
                if let Some(browser) = &mut app.browser {
                    match result.and_then(|value| {
                        serde_json::from_value::<protocol::Listing>(value)
                            .map_err(|e| e.to_string())
                    }) {
                        Ok(listing) => {
                            browser.listing = Some(listing);
                            browser.error = None;
                        }
                        Err(error) => browser.error = Some(error),
                    }
                }
                cx.notify();
            },
        );
    }

    /// The browser's choice: the shown folder, or `file` in it.
    pub(super) fn browse_pick(
        &mut self,
        file: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(browser) = self.browser.take() else {
            return;
        };
        let Some(listing) = &browser.listing else {
            return;
        };
        let folder = listing.path.to_string_lossy().into_owned();
        let path = file.map_or_else(|| folder.clone(), |name| join_remote(&folder, &name));
        // `act` closes the dialog, or shows the project dialog again.
        self.modal = self.browser_return.take();
        match browser.purpose {
            BrowserPurpose::OpenFolder => {
                self.open_folder(&browser.machine, Path::new(&path), window, cx);
            }
            BrowserPurpose::ProjectFolder => {
                self.set_project_folder(Path::new(&path), window, cx);
            }
            BrowserPurpose::Attach { .. } => self.attach_path(&path, window, cx),
        }
    }

    fn browser_title(&self) -> Option<String> {
        let browser = self.browser.as_ref()?;
        let name = machines::name(&browser.machine);
        Some(match browser.purpose {
            BrowserPurpose::OpenFolder => format!("Open folder on {name}"),
            BrowserPurpose::ProjectFolder => format!("Choose a folder on {name}"),
            BrowserPurpose::Attach { directory: true } => format!("Add a directory from {name}"),
            BrowserPurpose::Attach { directory: false } => format!("Add a file from {name}"),
        })
    }

    /// The folder browser's body.
    fn browser_view(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let mut content = col().gap_3();
        let Some(browser) = &self.browser else {
            return content;
        };
        content = content.child(views::dialog_title(
            self.browser_title().unwrap_or_default(),
        ));
        let Some(listing) = &browser.listing else {
            return content.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(browser.error.clone().unwrap_or_else(|| "Loading…".into())),
            );
        };
        let folder = listing.path.to_string_lossy().into_owned();
        let mut rows = col()
            .id("browser-entries")
            .role(Role::List)
            .aria_label("Folder contents")
            .h(rems(18.))
            .overflow_y_scroll()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .p_1();
        for (ix, entry) in listing.entries.iter().enumerate() {
            let pickable = entry.directory || browser.picks_files();
            if !pickable && !matches!(browser.purpose, BrowserPurpose::Attach { .. }) {
                continue;
            }
            let action = if entry.directory {
                Some(Action::BrowseTo(join_remote(&folder, &entry.name).into()))
            } else if browser.picks_files() {
                Some(Action::BrowsePick(Some(entry.name.clone())))
            } else {
                None
            };
            let label = if entry.directory {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            let mut row = row()
                .id(("browser-entry", ix))
                .role(Role::ListItem)
                .aria_label(label.clone())
                .gap_2()
                .px_2()
                .py_1()
                .rounded(theme.radius)
                .text_sm()
                .child(
                    icon(if entry.directory { "folder" } else { "file" })
                        .text_color(theme.muted_foreground),
                )
                .child(div().truncate().child(label));
            if let Some(action) = action {
                row = row
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.accent))
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(action.clone(), window, cx);
                    }));
            } else {
                row = row.text_color(theme.muted_foreground);
            }
            rows = rows.child(row);
        }
        if listing.entries.is_empty() {
            rows = rows.child(
                div()
                    .p_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("This folder is empty."),
            );
        }
        let choose_folder = !browser.picks_files();
        content
            .child(
                row()
                    .gap_2()
                    .children(listing.parent.clone().map(|parent| {
                        self.button("browser-up", "Up", Action::BrowseTo(parent), cx)
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.muted_foreground)
                            .child(folder),
                    ),
            )
            .child(rows)
            .children(
                browser
                    .error
                    .clone()
                    .map(|error| div().text_color(theme.danger).child(error)),
            )
            .child(
                row()
                    .justify_end()
                    .gap_2()
                    .child(self.button("browser-cancel", "Cancel", Action::Close, cx))
                    .when(choose_folder, |row| {
                        row.child(
                            self.button(
                                "browser-choose",
                                "Choose this folder",
                                Action::BrowsePick(None),
                                cx,
                            )
                            .primary(),
                        )
                    }),
            )
    }

    /// Buttons choosing among the checked machines.
    fn machine_choice(
        &self,
        id: &'static str,
        selected: Option<&str>,
        action: impl Fn(String) -> Action,
        cx: &Context<Self>,
    ) -> Div {
        row()
            .gap_2()
            .flex_wrap()
            .children(
                machines::checked()
                    .into_iter()
                    .enumerate()
                    .map(|(ix, machine)| {
                        let chosen = selected == Some(machine.as_str());
                        let button =
                            self.button((id, ix), machines::name(&machine), action(machine), cx);
                        if chosen { button.primary() } else { button }
                    }),
            )
    }

    /// The folder drop zone that leads the project dialogs.
    fn folder_zone(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let accent = theme.primary;
        let directory = self.project_directory_input.read(cx).value();
        let remote = self.project_machine != machines::LOCAL;
        col()
            .id("project-folder")
            .items_center()
            .gap_2()
            .py_4()
            .px_3()
            .rounded(theme.radius_lg)
            .border_1()
            .border_dashed()
            .border_color(theme.border)
            .when(!remote, |zone| {
                zone.drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(accent))
                    .on_drop(cx.listener(|app, paths: &ExternalPaths, window, cx| {
                        if let Some(directory) = paths.paths().first() {
                            app.set_project_folder(directory, window, cx);
                        }
                    }))
            })
            .child(icon("folder").size_5().text_color(accent))
            .child(
                row()
                    .gap_2()
                    .when(!remote, |row| row.child("Drop a folder or"))
                    .child(
                        Button::new("browse-folder")
                            .label("Browse")
                            .small()
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.browse_project_folder(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .text_xs()
                    .text_center()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(if directory.is_empty() {
                        "No folder chosen".into()
                    } else {
                        directory
                    }),
            )
    }

    pub(super) fn project_modal(&self, cx: &Context<Self>) -> AnyElement {
        use gpui_kit::component::form::{Field, Form};
        let muted = cx.theme().muted_foreground;
        let note = move |text: &'static str| div().text_color(muted).child(text);
        let error = self
            .project_error
            .clone()
            .map(|error| div().text_color(cx.theme().danger).child(error));
        let footer = || row().justify_end().gap_2().pt_1();
        let mut content = col().gap_3().text_sm();
        // The newer dialogs name themselves for assistive technology.
        let mut label = None;
        match self.modal {
            Some("browser") => {
                label = self.browser_title();
                content = content.child(self.browser_view(cx));
            }
            Some("folder-machine") => {
                label = Some("Open a folder on which machine?".to_owned());
                let last = machines::last_folder().filter(|m| machines::is_checked(m));
                let default = last.or_else(|| machines::checked().into_iter().next());
                content = content
                    .child(views::dialog_title("Open a folder on which machine?"))
                    .child(self.machine_choice(
                        "folder-machine",
                        default.as_deref(),
                        Action::FolderMachine,
                        cx,
                    ))
                    .child(footer().child(self.button(
                        "cancel-folder",
                        "Cancel",
                        Action::Close,
                        cx,
                    )));
            }
            Some("upgrade") => {
                let machine = self.upgrade_machine.clone().unwrap_or_default();
                let name = machines::name(&machine);
                label = Some(format!("Upgrade {name}?"));
                let detail = match client::state(&machine, cx) {
                    Some(client::State::UpgradeNeeded { version, active }) => format!(
                        "{name} runs Adeline {version} with {active} active conversation{}. Upgrading restarts its engine with Adeline {}, which stops them.",
                        if *active == 1 { "" } else { "s" },
                        env!("CARGO_PKG_VERSION"),
                    ),
                    _ => format!(
                        "{name} runs an older Adeline. Upgrading restarts its engine, which stops its agents."
                    ),
                };
                content = content
                    .child(views::dialog_title(format!("Upgrade {name}?")))
                    .child(div().text_color(muted).child(detail))
                    .child(
                        footer()
                            .child(self.button("upgrade-later", "Later", Action::Close, cx))
                            .child(
                                self.button(
                                    "upgrade-now",
                                    "Upgrade now (stops them)",
                                    Action::ConfirmUpgrade,
                                    cx,
                                )
                                .danger(),
                            ),
                    );
            }
            Some("prompt") => {
                if let Some((machine, text, host_key)) = client::prompt(cx) {
                    let title = if host_key {
                        format!("Trust {}?", machines::name(&machine))
                    } else {
                        format!("Sign in to {}", machines::name(&machine))
                    };
                    let group = col()
                        .id("ssh-prompt")
                        .role(Role::Group)
                        .aria_label(title.clone())
                        .gap_3()
                        .child(views::dialog_title(title))
                        .child(
                            div()
                                .id("ssh-prompt-text")
                                .role(Role::Status)
                                .aria_label(text.clone())
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(text),
                        )
                        .when(!host_key, |column| {
                            column.child(Input::new(&self.prompt_input).aria_label("SSH answer"))
                        })
                        .child(
                            footer()
                                .child(self.button(
                                    "prompt-cancel",
                                    if host_key { "Reject" } else { "Cancel" },
                                    Action::AnswerPrompt(false),
                                    cx,
                                ))
                                .child(
                                    self.button(
                                        "prompt-ok",
                                        if host_key { "Accept" } else { "Continue" },
                                        Action::AnswerPrompt(true),
                                        cx,
                                    )
                                    .primary(),
                                ),
                        );
                    content = content.child(group);
                }
            }
            Some("add-project" | "settings") => {
                let creating = self.modal == Some("add-project");
                let unfinished = self
                    .workspace()
                    .threads
                    .iter()
                    .any(|thread| !matches!(thread.status.as_str(), "completed" | "archived"));
                content = content
                    .child(views::dialog_title(if creating { "New project" } else { "Project settings" }))
                    .when(creating && machines::checked().len() > 1, |column| column.child(
                        col().gap_2().child("Machine").child(self.machine_choice(
                            "project-machine",
                            Some(&self.project_machine),
                            Action::ProjectMachine,
                            cx,
                        ))))
                    .when(!self.demo_mode, |column| column.child(self.folder_zone(cx)))
                    .when(!creating && unfinished && !self.demo_mode, |column| column.child(note(
                        "Complete or archive active conversations before changing the folder. Existing conversations keep their saved folder.")))
                    .child(Form::new().child(Field::new().label("Name").child(Input::new(&self.name_input).aria_label("Project name"))))
                    .when(!creating && self.demo_mode, |column| column.child(col().gap_2().child("Project color")
                        .child(row().gap_2().flex_wrap().children(theme::project_colors().into_iter().enumerate().map(|(ix, color)| {
                            self.button(("project-color", ix), format!("Color {}", ix + 1), Action::Tint(ix), cx)
                                .selected(self.selected_tint == ix)
                                .child(div().size_2().rounded_full().bg(rgb(color)))
                        })))))
                    .children(error)
                    .child(footer()
                        .when(!creating && !self.demo_mode, |row| row.child(self.button("delete-project", "Delete project…", Action::DeleteProject, cx).danger()))
                        .child(div().flex_1())
                        .child(self.button("cancel-project", "Cancel", Action::Close, cx))
                        .child(self.button("save-project", if creating { "Create" } else { "Save" }, if creating { Action::SaveProject } else { Action::SaveSettings }, cx).primary()));
            }
            Some("rename-project") => {
                content = content
                    .child(views::dialog_title("Rename project"))
                    .child(
                        Form::new().child(
                            Field::new()
                                .label("Name")
                                .child(Input::new(&self.name_input).aria_label("Project name")),
                        ),
                    )
                    .children(error)
                    .child(
                        footer()
                            .child(self.button("cancel-rename", "Cancel", Action::Close, cx))
                            .child(
                                self.button("save-rename", "Rename", Action::SaveRename, cx)
                                    .primary(),
                            ),
                    );
            }
            Some("delete-project" | "delete-project-shutdown") => {
                let stopping = self.modal == Some("delete-project-shutdown");
                let stopped = self
                    .delete_project
                    .and_then(|ix| self.projects.get(ix))
                    .is_some_and(|_| {
                        self.project_agents_stopped(self.delete_project.unwrap_or_default())
                    });
                let name = self
                    .delete_project
                    .and_then(|ix| self.projects.get(ix))
                    .map_or("project", |project| project.config.name.as_str());
                content = content.child(views::dialog_title(if stopping && !stopped { format!("Stopping agents for {name}") } else { format!("Delete \"{name}\"?") }))
                    .child(note(if stopping && !stopped { "Waiting for agents to stop. Saved conversations remain until they stop. Force stop is available if shutdown stalls." } else { "This deletes the project's saved conversations. The working directory and its files will be kept." }))
                    .children(error)
                    .child(footer()
                        .child(self.button("cancel-delete", "Cancel deletion", Action::Close, cx))
                        .when(stopping && self.project_error.is_some(), |row| row.child(self.button("retry-delete", "Try again", Action::ConfirmDeleteProject, cx)))
                        .when(stopping && !stopped, |row| row.child(self.button("force-delete", "Force stop", Action::ForceDeleteProject, cx).danger()))
                        .when(!stopping, |row| row.child(self.button("confirm-delete", "Delete", Action::ConfirmDeleteProject, cx).danger())));
            }
            Some("quit") => {
                let stopping = self.runtime.stopping_all;
                content = content
                    .child(views::dialog_title(if stopping { "Stopping agents…" } else { "Agents are still working" }))
                    .child(note(if stopping {
                        "Adeline closes once every agent has exited. Agents still running after 5 seconds are stopped."
                    } else {
                        "Stop all cancels running turns and closes every agent. Finish in background keeps them running after Adeline closes, and their replies are here when you reopen it."
                    }))
                    .when(!stopping, |column| column.child(footer()
                        .child(self.button("quit-cancel", "Cancel", Action::Close, cx))
                        .child(self.button("quit-background", "Finish in background", Action::FinishInBackground, cx))
                        .child(self.button("quit-stop-all", "Stop all", Action::QuitStopAll, cx).danger())));
            }
            _ => {}
        }
        match label {
            Some(label) => col()
                .id("dialog-content")
                .role(Role::Group)
                .aria_label(label)
                .child(content)
                .into_any_element(),
            None => content.into_any_element(),
        }
    }
}
