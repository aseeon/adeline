//! The agent editor page: showing, saving, reloading and deleting an agent.
use super::*;

impl SettingsWindow {
    pub(super) fn agent_dirty(&self, cx: &App) -> bool {
        self.agent_form
            .as_ref()
            .is_some_and(|form| form.read(cx).dirty(cx))
    }

    pub(super) fn close_agent(&mut self) {
        self.agent_page = None;
        self.agent_form = None;
        self.agent_subscriptions.clear();
    }

    fn agent_definition(&self, id: Option<&String>, cx: &App) -> Option<agents::AgentDefinition> {
        let machine = &self.machine;
        self.owner
            .entity
            .read_with(cx, |app, _| {
                app.catalog(machine)
                    .entries
                    .iter()
                    .find(|entry| Some(&entry.id) == id)
                    .map(|entry| entry.definition.clone())
            })
            .ok()
            .flatten()
    }

    pub(super) fn show_agent(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let machine = self.machine.clone();
        let definition = self.agent_definition(Some(&id), cx);
        self.agent_page = Some(id.clone());
        self.machines_page = false;
        self.group = Group::Agents;
        self.subgroup = None;
        self.folded[Group::Agents as usize] = false;
        self.agent_status = None;
        self.search_page = Some((Group::Agents, None));
        self.agent_subscriptions.clear();
        client::ensure(&machine, cx);
        client::refresh_harnesses(&machine, false, cx);
        self.agent_form = definition.map(|definition| {
            cx.new(|cx| agent_form::AgentForm::new(machine, Some(id), definition, window, cx))
        });
        if let Some(form) = &self.agent_form {
            self.agent_subscriptions
                .push(cx.observe(form, |_, _, cx| cx.notify()));
        }
        cx.notify();
    }

    pub(super) fn sync_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.agent_form.clone() else {
            return;
        };
        let form_id = form.read(cx).id.clone();
        let current = self.agent_definition(form_id.as_ref(), cx);
        if current.as_ref() == Some(&form.read(cx).original) {
            form.update(cx, |form, _| form.external_changed = false);
        } else if form.read(cx).dirty(cx) {
            form.update(cx, |form, _| form.external_changed = true);
        } else if let Some(definition) = current {
            form.update(cx, |form, cx| form.reload(definition, window, cx));
        } else {
            self.close_agent();
        }
        cx.notify();
    }

    pub(super) fn save_agent(
        &mut self,
        after: Option<AfterAgent>,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.agent_form.clone() else {
            return;
        };
        let form_id = form.read(cx).id.clone();
        let current = self.agent_definition(form_id.as_ref(), cx);
        if !overwrite
            && (form.read(cx).external_changed || current.as_ref() != Some(&form.read(cx).original))
        {
            if self.pending {
                return;
            }
            self.pending = true;
            let view = cx.entity().downgrade();
            choose(
                window,
                cx,
                "This agent changed outside Adeline.".into(),
                "Reload the external definition and lose your unsaved edits, or overwrite it with your edits.",
                &["Reload", "Overwrite", "Cancel"],
                move |choice, window, cx| {
                    let after = after.clone();
                    let _ = view.update(cx, |view, cx| {
                        view.pending = false;
                        match choice {
                            0 => {
                                // The engine watches agent files, so the list is current.
                                if let Some(form) = view.agent_form.clone() {
                                    let form_id = form.read(cx).id.clone();
                                    let definition = view.agent_definition(form_id.as_ref(), cx);
                                    if let Some(definition) = definition {
                                        form.update(cx, |form, cx| {
                                            form.reload(definition, window, cx);
                                        });
                                    } else {
                                        view.close_agent();
                                    }
                                }
                                if let Some(after) = after {
                                    view.after_agent(after, window, cx);
                                }
                            }
                            1 => view.save_agent(after, true, window, cx),
                            _ => {}
                        }
                    });
                },
            );
            return;
        }
        if self.pending {
            return;
        }
        self.pending = true;
        cx.notify();
        let view = cx.entity().downgrade();
        let saved = form.clone();
        let handle = window.window_handle();
        persist_form(&self.owner, &form, overwrite, cx, move |result, cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.pending = false;
                    view.saved_agent(&saved, result, after, window, cx);
                });
            });
        });
    }

    fn saved_agent(
        &mut self,
        form: &Entity<agent_form::AgentForm>,
        result: Result<String>,
        after: Option<AfterAgent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(saved) => {
                form.update(cx, |form, cx| {
                    let definition = form.values(cx);
                    form.id = Some(saved.clone());
                    form.reload(definition, window, cx);
                });
                let after = after.map(|next| match next {
                    AfterAgent::Delete(_) => AfterAgent::Delete(saved.clone()),
                    other => other,
                });
                self.agent_page = Some(saved);
                if let Some(after) = after {
                    self.after_agent(after, window, cx);
                }
            }
            Err(message) => {
                let message = format!("{message:#}");
                let conflict = message.contains("changed outside this form");
                form.update(cx, |form, cx| {
                    form.external_changed = conflict;
                    form.status = Some(message);
                    cx.notify();
                });
            }
        }
        cx.notify();
    }

    fn confirm_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if self.agent_dirty(cx) {
            self.leave(AfterAgent::Delete(id), window, cx);
        } else {
            self.confirm_delete_after_leaving(id, window, cx);
        }
    }
    pub(super) fn confirm_delete_after_leaving(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending {
            return;
        }
        self.pending = true;
        let name = self
            .agent_form
            .as_ref()
            .map_or_else(|| id.clone(), |form| form.read(cx).original.name.clone());
        let view = cx.entity().downgrade();
        choose(
            window,
            cx,
            format!("Delete {name}?"),
            "The agent's folder and saved definition will be removed.",
            &["Delete", "Cancel"],
            move |choice, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    if choice != 0 {
                        view.pending = false;
                        return;
                    }
                    let owner = view.owner.clone();
                    let machine = view.machine.clone();
                    let weak = cx.entity().downgrade();
                    Adeline::delete_agent(
                        &owner.entity,
                        &machine,
                        id.clone(),
                        cx,
                        move |result, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.pending = false;
                                match result {
                                    Ok(()) => {
                                        view.close_agent();
                                        view.agent_status = None;
                                    }
                                    Err(message) => {
                                        view.agent_status = Some(format!("{message:#}"));
                                    }
                                }
                                cx.notify();
                            });
                        },
                    );
                    cx.notify();
                });
            },
        );
    }

    pub(super) fn agent_settings(
        &self,
        query: &str,
        searching: bool,
        cx: &Context<Self>,
    ) -> Option<Div> {
        let machine = self.machine.clone();
        let matching_agent = self
            .owner
            .entity
            .read_with(cx, |app, _| {
                app.catalog(&machine)
                    .entries
                    .iter()
                    .any(|entry| agent_matches(entry, query))
            })
            .unwrap_or(false);
        if searching && !matching_agent && !matches_query(query, &["Agents"]) {
            return None;
        }
        let mut page = div().w_full().flex().flex_col().gap_3();
        if let Some(form) = self.agent_form.clone() {
            if self.group == Group::Agents && (!searching || self.search_page.is_some()) {
                let blocked = form.read(cx).blocked(cx);
                page = page.child(form.clone());
                let form = form.read(cx);
                if form.external_changed {
                    page = page.child(error("This agent changed outside Adeline. Your edits are kept. Save to choose Reload or Overwrite.", cx));
                }
                if let Some(message) = &form.status {
                    page = page.child(error(message.clone(), cx));
                }
                page = page.child(
                    div()
                        .w_full()
                        .flex()
                        .justify_between()
                        .gap_2()
                        .mt_4()
                        .child(
                            Button::new("settings-delete-agent")
                                .danger()
                                .label("Delete agent…")
                                .disabled(self.pending)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if let Some(id) = this.agent_page.clone() {
                                        this.confirm_delete(id, window, cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new("settings-save-agent")
                                .primary()
                                .label("Save")
                                .disabled(self.pending || blocked)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_agent(None, false, window, cx);
                                })),
                        ),
                );
            }
        } else {
            page = page.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Select an agent in the sidebar to edit its settings."),
            );
        }
        if let Some(message) = &self.agent_status {
            page = page.child(error(message.clone(), cx));
        }
        Some(page.child(diagnostics(self.owner.diagnostics(&self.machine, cx), cx)))
    }
}
