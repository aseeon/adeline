use super::*;
use std::path::Path;

impl Adeline {
    pub(super) fn create_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).content.trim().to_owned();
        let directory = self
            .project_directory_input
            .read(cx)
            .content
            .trim()
            .to_owned();
        let project = if self.demo_mode {
            if name.is_empty() {
                Err("Enter a project name.".into())
            } else {
                Ok(Workspace {
                    config: Config {
                        id: format!("local-project-{}", self.projects.len()),
                        name,
                        provider: "claude".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
            }
        } else {
            self.project_store
                .lock()
                .map_err(|_| "Project storage is unavailable.".to_owned())
                .and_then(|mut store| {
                    let id = store.save_project(None, &name, Path::new(&directory))?;
                    Ok(store
                        .projects
                        .iter()
                        .find(|project| project.id == id)
                        .expect("saved project is in the store")
                        .to_workspace())
                })
        };
        match project {
            Ok(project) => {
                let index = self.projects.len();
                self.collaboration.push(if self.demo_mode {
                    collaboration_modes::ProjectCollaboration::seed(&project)
                } else {
                    collaboration_modes::ProjectCollaboration::default()
                });
                self.projects.push(project);
                self.open_projects.push(false);
                self.project_tints.push(0);
                self.project_error = None;
                self.modal = None;
                self.act(Action::Project(index), window, cx);
            }
            Err(error) => {
                self.project_error = Some(error);
                cx.notify();
            }
        }
    }

    pub(super) fn save_project_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_open_project() {
            return;
        }
        let name = self.name_input.read(cx).content.trim().to_owned();
        let directory = self
            .project_directory_input
            .read(cx)
            .content
            .trim()
            .to_owned();
        let old_id = self.projects[self.project].config.id.clone();
        let saved = if self.demo_mode {
            if name.is_empty() {
                Err("Enter a project name.".into())
            } else {
                Ok(old_id.clone())
            }
        } else {
            self.project_store
                .lock()
                .map_err(|_| "Project storage is unavailable.".to_owned())
                .and_then(|mut store| {
                    store.save_project(Some(&old_id), &name, Path::new(&directory))
                })
        };
        match saved {
            Ok(id) => {
                let config = &mut self.projects[self.project].config;
                config.id.clone_from(&id);
                config.name = name;
                if !self.demo_mode {
                    config.directory = directory.into();
                }
                for service in &mut self.services {
                    if service.project_id == old_id {
                        service.project_id.clone_from(&id);
                    }
                }
                self.project_tints[self.project] = self.selected_tint;
                self.project_error = None;
                self.modal = None;
                window.set_window_title(&format!("{} — Adeline", config.name));
                self.sync_regions(&Action::Project(self.project), cx);
                self.sync_content_regions(&Action::Project(self.project), cx);
                cx.notify();
            }
            Err(error) => {
                self.project_error = Some(error);
                cx.notify();
            }
        }
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
        self.stop_project(&id, cx);
        self.finish_project_deletion(cx);
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
        self.finish_project_deletion(cx);
    }

    pub(super) fn finish_project_deletion(&mut self, cx: &mut Context<Self>) {
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
        if !self.project_agents_stopped(&id) {
            cx.notify();
            return;
        }
        let result = self
            .project_store
            .lock()
            .map_err(|_| "Project storage is unavailable.".to_owned())
            .and_then(|mut store| store.delete_project(&id));
        if let Err(error) = result {
            self.project_error = Some(error);
            cx.notify();
            return;
        }
        let ids: Vec<_> = self.projects[index]
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect();
        self.forget_project_runtime(&ids);
        let previous_project = self.project;
        self.projects.remove(index);
        self.open_projects.remove(index);
        self.collaboration.remove(index);
        self.project_tints.remove(index);
        self.services.retain(|service| service.project_id != id);
        self.archived_docs = self
            .archived_docs
            .drain()
            .filter_map(|(project, document)| {
                (project != index).then_some((
                    if project > index {
                        project - 1
                    } else {
                        project
                    },
                    document,
                ))
            })
            .collect();
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
        self.document = None;
        self.workflow = None;
        self.service = None;
        self.delete_project = None;
        self.project_error = None;
        self.modal = None;
        self.section = Section::Chats;
        self.composer.update(cx, |input, cx| input.set("", cx));
        self.query.update(cx, |input, cx| input.set("", cx));
        let title = if self.open_projects.iter().any(|open| *open) {
            format!("{} — Adeline", self.projects[self.project].config.name)
        } else {
            "Adeline".to_owned()
        };
        cx.defer(move |cx| {
            if let Some(owner) = cx
                .windows()
                .into_iter()
                .find_map(|window| window.downcast::<Adeline>())
            {
                let _ = owner.update(cx, |_, window, _| window.set_window_title(&title));
            }
        });
        self.sync_regions(&Action::Project(self.project), cx);
        self.sync_content_regions(&Action::Project(self.project), cx);
        cx.notify();
    }
}

impl Adeline {
    pub(super) fn project_modal(&self, cx: &Context<Self>) -> AnyElement {
        let mut dialog = col()
            .w(px(600.))
            .p_8()
            .gap_5()
            .rounded(px(22.))
            .bg(rgb(theme::card()))
            .shadow(vec![theme::shadow(24.)]);
        match self.modal {
            Some("settings") => {
                let unfinished = self
                    .workspace()
                    .threads
                    .iter()
                    .any(|thread| !matches!(thread.status.as_str(), "completed" | "archived"));
                dialog = dialog
                    .child(
                        row()
                            .justify_between()
                            .child(text("Project settings", 26., theme::foreground()))
                            .child(self.ib("close-project-settings", "close", Action::Close, cx)),
                    )
                    .child(text("Project name", 13., theme::foreground()))
                    .child(
                        div()
                            .w_full()
                            .p_3()
                            .rounded_lg()
                            .bg(rgb(theme::secondary()))
                            .child(self.name_input.clone()),
                    )
                    .child(text(
                        format!("Configuration folder: {}", self.workspace().config.id),
                        12.,
                        theme::muted_foreground(),
                    ))
                    .child(text("Working directory", 13., theme::foreground()))
                    .child(
                        div()
                            .w_full()
                            .p_3()
                            .rounded_lg()
                            .bg(rgb(theme::secondary()))
                            .child(self.project_directory_input.clone()),
                    )
                    .child(text(
                        "This existing directory is where new agent conversations run.",
                        12.,
                        theme::muted_foreground(),
                    ))
                    .when(unfinished, |d| d.child(text(
                        "Complete or archive all active conversations before changing the working directory. Existing conversations keep their saved directory.",
                        12.,
                        theme::muted_foreground(),
                    )))
                    .when_some(self.project_error.clone(), |d, error| {
                        d.child(text(error, 12., theme::destructive()))
                    })
                    .child(
                        row()
                            .justify_between()
                            .child(
                                self.button("delete-project", "Delete project", Action::DeleteProject, cx)
                                    .text_color(rgb(theme::destructive())),
                            )
                            .child(
                                row()
                                    .gap_3()
                                    .child(self.button("cancel-project-settings", "Cancel", Action::Close, cx))
                                    .child(self.button("save-project-settings", "Save", Action::SaveSettings, cx)
                                        .bg(rgb(theme::secondary()))),
                            ),
                    );
            }
            Some("delete-project" | "delete-project-shutdown") => {
                let stopping = self.modal == Some("delete-project-shutdown");
                let stopped = self
                    .delete_project
                    .and_then(|index| self.projects.get(index))
                    .is_some_and(|project| self.project_agents_stopped(&project.config.id));
                let name = self
                    .delete_project
                    .and_then(|index| self.projects.get(index))
                    .map_or("project", |project| project.config.name.as_str());
                dialog = dialog
                    .child(text(
                        if stopping && !stopped { format!("Stopping agents for {name}") }
                        else if stopping { format!("Could not delete {name}") }
                        else { format!("Delete {name}?") },
                        24.,
                        theme::foreground(),
                    ))
                    .child(text(
                        if stopping && !stopped {
                            "Waiting for running agents to stop. Saved conversations and the project are kept until they stop. If shutdown is stuck, choose Force Stop."
                        } else if stopping {
                            "The project and working directory remain. Fix the error and try again."
                        } else {
                            "Running agents will stop, and this project's saved conversations will be deleted. The working directory and its files will be kept."
                        },
                        14.,
                        theme::muted_foreground(),
                    ))
                    .when_some(self.project_error.clone(), |d, error| {
                        d.child(text(error, 12., theme::destructive()))
                    })
                    .child(
                        row()
                            .justify_end()
                            .gap_3()
                            .child(self.button("cancel-delete-project", "Cancel deletion", Action::Close, cx))
                            .when(stopping && self.project_error.is_some(), |d| d.child(
                                self.button("retry-delete-project", "Try again", Action::ConfirmDeleteProject, cx)
                                    .bg(rgb(theme::secondary()))
                            ))
                            .when(stopping && !stopped, |d| d.child(
                                self.button("force-delete-project", "Force Stop", Action::ForceDeleteProject, cx)
                                    .text_color(rgb(theme::destructive()))
                            ))
                            .when(!stopping, |d| d.child(
                                self.button("confirm-delete-project", "Delete project", Action::ConfirmDeleteProject, cx)
                                    .text_color(rgb(theme::destructive()))
                            )),
                    );
            }
            Some("shutdown") => {
                dialog = dialog
                    .child(text("Stopping agents", 24., theme::foreground()))
                    .child(text(
                        "Adeline is waiting for running agents to stop. The window stays open until they stop. If shutdown is stuck, choose Force Stop.",
                        14.,
                        theme::muted_foreground(),
                    ))
                    .child(
                        row().justify_end().child(
                            self.button("force-stop-all", "Force Stop", Action::ForceStopAll, cx)
                                .text_color(rgb(theme::destructive())),
                        ),
                    );
            }
            _ => {}
        }
        div()
            .absolute()
            .inset_0()
            .bg(rgba((theme::shadow_base() << 8) | 0x80))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(dialog)
            .into_any_element()
    }
}
