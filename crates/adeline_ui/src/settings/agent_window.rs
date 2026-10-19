//! The Add Agent and Edit Agent window: its form, saving, and closing with
//! unsaved changes.
use super::*;

pub(super) struct AgentWindow {
    pub(super) owner: Owner,
    machine: String,
    pub(super) form: Entity<agent_form::AgentForm>,
    focus: FocusHandle,
    pending: bool,
    _subscriptions: Vec<Subscription>,
}

impl AgentWindow {
    pub(super) fn new(
        owner: Owner,
        machine: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        client::focus(&machine, cx);
        client::ensure(&machine, cx);
        client::refresh_harnesses(&machine, true, cx);
        let form = cx.new(|cx| {
            agent_form::AgentForm::new(
                machine.clone(),
                None,
                agents::AgentDefinition::default(),
                window,
                cx,
            )
        });
        let focus = cx.focus_handle().tab_stop(true);
        let name = form.read(cx).name_focus(cx);
        window.focus(&name, cx);
        let subscriptions = vec![cx.observe(&form, |_, _, cx| cx.notify())];
        let view = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, cx| view.confirm_close(false, window, cx))
        });
        Self {
            owner,
            machine,
            form,
            focus,
            pending: false,
            _subscriptions: subscriptions,
        }
    }

    fn save(&mut self, close_owner: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        cx.notify();
        let view = cx.entity().downgrade();
        let handle = window.window_handle();
        persist_form(&self.owner, &self.form, false, cx, move |result, cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.pending = false;
                    match result {
                        Ok(_) => {
                            window.remove_window();
                            if close_owner {
                                interaction::request_close(
                                    view.owner.window,
                                    view.owner.entity.clone(),
                                    cx,
                                );
                            }
                        }
                        Err(error) => view.form.update(cx, |form, cx| {
                            form.status = Some(format!("{error:#}"));
                            cx.notify();
                        }),
                    }
                    cx.notify();
                });
            });
        });
    }
    pub(super) fn confirm_close(
        &mut self,
        close_owner: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.form.read(cx).dirty(cx) {
            if close_owner {
                window.remove_window();
            }
            return true;
        }
        if self.pending {
            return false;
        }
        self.pending = true;
        let weak = cx.entity().downgrade();
        choose(
            window,
            cx,
            "Save changes to this agent?".into(),
            "Unsaved agent details will be lost if you discard them.",
            &["Save", "Discard", "Cancel"],
            move |choice, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.pending = false;
                    match choice {
                        0 => view.save(close_owner, window, cx),
                        1 => {
                            let owner = view.owner.clone();
                            window.remove_window();
                            if close_owner {
                                interaction::request_close(owner.window, owner.entity, cx);
                            }
                        }
                        _ => {}
                    }
                });
            },
        );
        false
    }
}

impl Render for AgentWindow {
    #[cfg_attr(
        not(target_os = "windows"),
        expect(unused_variables, reason = "only the Windows titlebar reads window")
    )]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .p_6()
            .child(div().text_xl().child(if self.machine == machines::LOCAL {
                "Add an agent".to_owned()
            } else {
                format!("Add an agent on {}", machines::name(&self.machine))
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Configure an agent for this machine's projects. Saving does not start the command.",
                    ),
            )
            .child(self.form.clone())
            .child(diagnostics(self.owner.diagnostics(&self.machine, cx), cx));
        let shell = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground);
        #[cfg(target_os = "windows")]
        let shell = shell.child(titlebar::render("Add an agent · Adeline".into(), window));
        shell
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .id("agent-form-scroll")
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .p_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .when_some(self.form.read(cx).status.clone(), |row, status| {
                        row.child(error(status, cx).flex_1().min_w_0())
                    })
                    .child(Button::new("agent-create-cancel").label("Cancel").on_click(
                        cx.listener(|view, _, window, cx| {
                            if view.confirm_close(false, window, cx) {
                                window.remove_window();
                            }
                        }),
                    ))
                    .child(
                        Button::new("agent-create-save")
                            .primary()
                            .label("Save")
                            .disabled(self.pending || self.form.read(cx).blocked(cx))
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.save(false, window, cx);
                            })),
                    ),
            )
            .track_focus(&self.focus)
            .on_action(cx.listener(|view, _: &Cancel, window, cx| {
                if view.confirm_close(false, window, cx) {
                    window.remove_window();
                }
            }))
    }
}
