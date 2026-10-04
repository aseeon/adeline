//! Projects from the UI: add, rename, settings, delete with undo, and the project dialog.
use super::*;
use std::path::Path;

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
        if self.demo_mode {
            if name.is_empty() {
                self.project_error = Some("Enter a project name.".into());
                cx.notify();
                return;
            }
            let index = self.projects.len();
            self.projects.push(Workspace {
                config: Config {
                    id: format!("local-project-{index}"),
                    name,
                    provider: "claude".into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            self.open_projects.push(false);
            self.project_tints
                .push(index % theme::project_colors().len());
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
        self.request(command, cx, move |app, result, window, cx| match result {
            Ok(id) => {
                if from_dialog {
                    app.project_error = None;
                    app.modal = None;
                }
                // The engine's change arrived before its reply.
                if let Some(index) = app
                    .projects
                    .iter()
                    .position(|project| Some(project.config.id.as_str()) == id.as_str())
                {
                    app.act(Action::Project(index), window, cx);
                }
            }
            Err(error) if from_dialog => {
                app.project_error = Some(error);
                cx.notify();
            }
            Err(error) => window.push_notification(error, cx),
        });
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
        if let Err(error) = ui_state::record_opened(&id, now) {
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
        let command = protocol::Command::SaveProject {
            original: Some(original),
            name: name.clone(),
            directory,
        };
        self.request(command, cx, move |app, result, window, cx| {
            match result {
                Ok(id) => {
                    if let Some(ix) = app
                        .projects
                        .iter()
                        .position(|project| Some(project.config.id.as_str()) == id.as_str())
                    {
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
        self.modal = Some("delete-project-shutdown");
        self.project_error = None;
        self.request(
            protocol::Command::DeleteProject { id: id.clone() },
            cx,
            move |app, result, window, cx| {
                if let Err(error) = result {
                    let pending = app
                        .delete_project
                        .and_then(|ix| app.projects.get(ix))
                        .is_some_and(|project| project.config.id == id);
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
        let id = self.projects[index].config.id.clone();
        self.force_project(&id, cx);
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
        if !directory.is_dir() {
            self.project_error = Some("Drop a folder, not a file.".into());
            cx.notify();
            return;
        }
        let folder_name = |path: &Path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
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

    /// The folder drop zone that leads the project dialogs.
    fn folder_zone(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let accent = theme.primary;
        let directory = self.project_directory_input.read(cx).value();
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
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(accent))
            .on_drop(cx.listener(|app, paths: &ExternalPaths, window, cx| {
                if let Some(directory) = paths.paths().first() {
                    app.set_project_folder(directory, window, cx);
                }
            }))
            .child(icon("folder").size_5().text_color(accent))
            .child(
                row().gap_2().child("Drop a folder or").child(
                    Button::new("browse-folder")
                        .label("Browse")
                        .small()
                        .on_click(
                            cx.listener(|app, _, window, cx| app.browse_project_folder(window, cx)),
                        ),
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
        match self.modal {
            Some("add-project" | "settings") => {
                let creating = self.modal == Some("add-project");
                let unfinished = self
                    .workspace()
                    .threads
                    .iter()
                    .any(|thread| !matches!(thread.status.as_str(), "completed" | "archived"));
                content = content
                    .child(views::dialog_title(if creating { "New project" } else { "Project settings" }))
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
                    .is_some_and(|project| self.project_agents_stopped(&project.config.id));
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
        content.into_any_element()
    }
}
