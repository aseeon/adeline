//! The Machines page: adding, editing and removing machines.
use super::*;

impl SettingsWindow {
    pub(super) fn fill_machine_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .editing
            .as_deref()
            .and_then(machines::remote)
            .map(|m| m.name)
            .unwrap_or_default();
        self.machine_name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.machine_destination
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn machines_changed(
        &mut self,
        result: Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.machine_error = result.err();
        if !machines::all().contains(&self.machine) {
            machines::LOCAL.clone_into(&mut self.machine);
            client::focus(machines::LOCAL, cx);
        }
        let name = machines::name(&self.machine);
        self.machine_picker.update(cx, |picker, cx| {
            picker.set_items(SearchableVec::new(machine_labels()), window, cx);
            picker.set_selected_value(&name, window, cx);
        });
        client::sync(cx);
        cx.refresh_windows();
        cx.notify();
    }

    fn add_machine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.machine_name.read(cx).value().to_string();
        let destination = self.machine_destination.read(cx).value().to_string();
        let result = machines::add(&name, &destination).map(|_| ());
        if result.is_ok() {
            self.fill_machine_inputs(window, cx);
        }
        self.machines_changed(result, window, cx);
    }

    fn confirm_remove(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        choose(
            window,
            cx,
            format!("Remove {}?", machines::name(&id)),
            "Adeline forgets it on this computer. Its engine, projects and agents stay on that machine, and running agents keep running.",
            &["Remove", "Cancel"],
            move |choice, window, cx| {
                if choice != 0 {
                    return;
                }
                let _ = view.update(cx, |this, cx| {
                    if this.editing.as_deref() == Some(id.as_str()) {
                        this.editing = None;
                    }
                    let result = machines::remove(&id);
                    this.machines_changed(result, window, cx);
                });
            },
        );
    }

    fn edit_machine(
        &mut self,
        id: &str,
        edit: impl FnOnce(&mut machines::Machine) -> Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = machines::edit(id, edit);
        self.machines_changed(result, window, cx);
    }

    pub(super) fn machines_settings(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let mut page = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().child("Machines"))
            .child(div().text_sm().text_color(theme.muted_foreground).child(
                "Remote machines are reached with your ssh setup and saved on this computer only. Removing one only forgets it here: its engine, projects and agents stay on that machine.",
            ));
        if self.demo {
            page = page.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Demo machines can't be changed."),
            );
        }
        for (ix, machine) in machines::remotes().into_iter().enumerate() {
            let editing = self.editing.as_deref() == Some(machine.id.as_str());
            let id = machine.id.clone();
            let mut rows = vec![
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .child(machine.name.clone()),
                    )
                    .when(!editing && !self.demo, |row| {
                        let id = id.clone();
                        row.child(
                            Button::new(("machine-edit", ix))
                                .small()
                                .label("Edit")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.editing = Some(id.clone());
                                    this.machine_error = None;
                                    this.fill_machine_inputs(window, cx);
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!self.demo, |row| {
                        let id = id.clone();
                        row.child(
                            Button::new(("machine-remove", ix))
                                .small()
                                .danger()
                                .label("Remove")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_remove(id.clone(), window, cx);
                                })),
                        )
                    }),
            ];
            let count = machine.destinations.len();
            for (at, destination) in machine.destinations.iter().enumerate() {
                let button = |name: &'static str, label: &'static str, enabled: bool| {
                    Button::new(format!("{name}-{ix}-{at}"))
                        .xsmall()
                        .ghost()
                        .label(label)
                        .disabled(!enabled || self.demo)
                };
                let (up, down, remove) = (id.clone(), id.clone(), id.clone());
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_4()
                        .py_2()
                        .child(
                            div()
                                .w(rems(1.5))
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(format!("{}.", at + 1)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(theme.mono_font_family.clone())
                                .text_sm()
                                .child(destination.clone()),
                        )
                        .child(button("destination-up", "Up", at > 0).on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.edit_machine(
                                    &up,
                                    |m| {
                                        m.destinations.swap(at - 1, at);
                                        Ok(())
                                    },
                                    window,
                                    cx,
                                );
                            },
                        )))
                        .child(button("destination-down", "Down", at + 1 < count).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.edit_machine(
                                    &down,
                                    |m| {
                                        m.destinations.swap(at, at + 1);
                                        Ok(())
                                    },
                                    window,
                                    cx,
                                );
                            }),
                        ))
                        .child(button("destination-remove", "Remove", count > 1).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.edit_machine(
                                    &remove,
                                    |m| {
                                        m.destinations.remove(at);
                                        Ok(())
                                    },
                                    window,
                                    cx,
                                );
                            }),
                        )),
                );
            }
            if editing {
                let (rename, add) = (id.clone(), id.clone());
                rows.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .px_4()
                        .py_3()
                        .child(
                            Field::new().label("Name").child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().flex_1().child(
                                        Input::new(&self.machine_name).aria_label("Machine name"),
                                    ))
                                    .child(
                                        Button::new("machine-rename")
                                            .small()
                                            .label("Rename")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                let name =
                                                    this.machine_name.read(cx).value().to_string();
                                                this.edit_machine(
                                                    &rename,
                                                    |m| {
                                                        m.name = name;
                                                        Ok(())
                                                    },
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                        )
                        .child(
                            Field::new().label("Another destination").child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        div().flex_1().child(
                                            Input::new(&self.machine_destination)
                                                .aria_label("New destination"),
                                        ),
                                    )
                                    .child(
                                        Button::new("destination-add")
                                            .small()
                                            .label("Add destination")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                let destination = this
                                                    .machine_destination
                                                    .read(cx)
                                                    .value()
                                                    .trim()
                                                    .to_owned();
                                                this.edit_machine(
                                                    &add,
                                                    |m| {
                                                        remote::check_destination(&destination)?;
                                                        if m.destinations.contains(&destination) {
                                                            return Err(format!(
                                                                "{destination} is already listed."
                                                            ));
                                                        }
                                                        m.destinations.push(destination);
                                                        Ok(())
                                                    },
                                                    window,
                                                    cx,
                                                );
                                                if this.machine_error.is_none() {
                                                    this.machine_destination.update(
                                                        cx,
                                                        |input, cx| {
                                                            input.set_value("", window, cx);
                                                        },
                                                    );
                                                }
                                            })),
                                    ),
                            ),
                        )
                        .child(div().flex().justify_end().child(
                            Button::new("machine-done").small().label("Done").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.editing = None;
                                    this.machine_error = None;
                                    this.fill_machine_inputs(window, cx);
                                    cx.notify();
                                }),
                            ),
                        )),
                );
            }
            page = page.children(card(rows, cx));
        }
        if let Some(message) = &self.machine_error {
            page = page.child(error(message.clone(), cx));
        }
        if self.editing.is_none() && !self.demo {
            page = page.child(caps("Add a machine", cx).mt_2()).children(card(
                vec![
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .px_4()
                        .py_3()
                        .child(
                            Field::new()
                                .label("Name")
                                .child(Input::new(&self.machine_name).aria_label("Machine name")),
                        )
                        .child(
                            Field::new()
                                .label("Destination")
                                .description(
                                    "Tried first. You can add more destinations afterwards.",
                                )
                                .child(
                                    Input::new(&self.machine_destination).aria_label("Destination"),
                                ),
                        )
                        .child(
                            div().flex().justify_end().child(
                                Button::new("machine-add")
                                    .small()
                                    .primary()
                                    .label("Add machine")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_machine(window, cx);
                                    })),
                            ),
                        ),
                ],
                cx,
            ));
        }
        page
    }
}
