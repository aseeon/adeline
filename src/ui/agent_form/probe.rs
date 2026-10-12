//! Probing the agent for its models, efforts and modes, and showing how the
//! probe went.
use super::*;

impl AgentForm {
    pub(super) fn start_probe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.probe = None;
        self.probe_generation += 1;
        cx.notify();
        let Some(id) = self.harness_id(cx) else {
            self.probing = Probing::Idle;
            return;
        };
        let catalog = cx.global::<Catalog>();
        // The engine finds the executable and runs the probe.
        let launch = if id == CUSTOM {
            let command = self.command.read(cx).value().trim().to_owned();
            if command.is_empty() {
                self.probing = Probing::Idle;
                return;
            }
            (command, self.arguments(cx))
        } else if catalog.is_installed(&id) == Some(true) {
            (String::new(), Vec::new())
        } else {
            self.probing = Probing::Idle;
            return;
        };
        if catalog.demo {
            self.probing = Probing::Failed(ProbeError {
                message: "Demo mode does not start agents. Type the model and effort instead."
                    .into(),
                auth: Vec::new(),
                version: false,
            });
            return;
        }
        let model = self
            .model
            .read(cx)
            .selected_value()
            .map(ToString::to_string)
            .or_else(|| (id == self.original.harness).then(|| self.original.model.clone()))
            .filter(|model| !model.is_empty());
        let (probe, results) =
            client::probe(&self.machine.clone(), id, launch.0, launch.1, model, cx);
        self.probe = Some(probe);
        self.probing = Probing::Loading;
        let generation = self.probe_generation;
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(result) = results.recv().await {
                let _ = this.update_in(cx, |form, window, cx| {
                    if form.probe_generation == generation {
                        form.apply(result, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn apply(
        &mut self,
        result: Result<Probed, ProbeError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.probe = None;
        match result {
            Ok(probed) => {
                let same_harness =
                    self.harness_id(cx).as_deref() == Some(self.original.harness.as_str());
                let wanted = |picked: Option<&SharedString>, saved: &str| {
                    picked
                        .map(ToString::to_string)
                        .or_else(|| same_harness.then(|| saved.to_owned()))
                        .unwrap_or_default()
                };
                let model = wanted(self.model.read(cx).selected_value(), &self.original.model);
                let effort = wanted(self.effort.read(cx).selected_value(), &self.original.effort);
                let mode = wanted(self.mode.read(cx).selected_value(), &self.original.mode);
                let mut groups: Vec<SelectGroup<ChoiceItem>> = Vec::new();
                if let Some(option) = conversation::option(&probed.options, Category::Model) {
                    for (choice, item) in option
                        .choices()
                        .iter()
                        .zip(choices(&probed, Category::Model))
                    {
                        match groups.last_mut() {
                            Some(group) if group.title.as_ref() == choice.group => {
                                group.items.push(item);
                            }
                            _ => groups.push(SelectGroup::new(choice.group.clone()).item(item)),
                        }
                    }
                }
                self.model.update(cx, |picker, cx| {
                    picker.set_items(SearchableVec::new(groups), window, cx);
                    picker.set_selected_value(&model.into(), window, cx);
                });
                let efforts = choices(&probed, Category::Effort);
                self.effort.update(cx, |picker, cx| {
                    picker.set_items(efforts, window, cx);
                    picker.set_selected_value(&effort.into(), window, cx);
                });
                // Plan modes are already left out by the agent layer.
                let modes = choices(&probed, Category::Mode);
                self.mode.update(cx, |picker, cx| {
                    picker.set_items(modes, window, cx);
                    picker.set_selected_value(&mode.into(), window, cx);
                });
                self.offered = Some(probed.clone());
                self.probing = Probing::Ready(probed);
            }
            Err(error) => {
                self.offered = None;
                self.probing = Probing::Failed(error);
            }
        }
        cx.notify();
    }

    pub(super) fn probe_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        match &self.probing {
            Probing::Loading => {
                let name = self.agent_label(cx);
                Some(
                    row()
                        .w_full()
                        .gap_3()
                        .p_4()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().primary)
                        .bg(cx.theme().primary.opacity(0.08))
                        .child(Spinner::new().large().color(cx.theme().primary))
                        .child(
                            div()
                                .text_lg()
                                .child(format!("Reading models and effort from {name}…")),
                        )
                        .into_any_element(),
                )
            }
            Probing::Failed(error) => {
                let mut column = col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(error.message.clone()),
                    );
                if !error.auth.is_empty() {
                    let mut buttons = row().flex_wrap().gap_2();
                    for method in &error.auth {
                        let label = if method.terminal.is_some() {
                            format!("{} in terminal…", method.name)
                        } else {
                            method.name.clone()
                        };
                        let chosen = method.clone();
                        buttons = buttons.child(
                            Button::new(SharedString::from(format!("probe-login-{}", method.id)))
                                .small()
                                .label(label)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.run_login(Some(chosen.clone()), window, cx);
                                })),
                        );
                    }
                    column = column
                        .child(buttons)
                        .children(self.login.clone().map(|note| muted(note, cx)));
                }
                if !error.version {
                    column = column
                        .child(muted(
                            "You can also type the model and effort below and save.",
                            cx,
                        ))
                        .child(
                            Button::new("agent-probe-retry")
                                .small()
                                .label("Retry")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_probe(window, cx);
                                })),
                        );
                }
                Some(column.into_any_element())
            }
            Probing::Idle | Probing::Ready(_) => None,
        }
    }
}
