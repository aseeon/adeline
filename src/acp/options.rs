//! Session options: what the agent offers, in Adeline's categories, and applying
//! the user's selections.
use super::*;

impl Worker {
    pub(super) fn refresh_options(&mut self) {
        self.options = translate_options(&self.raw_options, self.modes.as_ref(), self.profile);
        self.emit(EventKind::Options(self.options.clone()));
    }

    fn selection_steps(&self) -> Vec<(Category, String, String)> {
        // Model first: some agents (Codex) offer effort only once a model is set.
        let selections = &self.config.selections;
        let mut steps = vec![
            (Category::Model, String::new(), selections.model.clone()),
            (Category::Effort, String::new(), selections.effort.clone()),
            (Category::Mode, String::new(), selections.mode.clone()),
        ];
        steps.extend(
            selections
                .other
                .iter()
                .map(|(id, value)| (Category::Other, id.clone(), value.clone())),
        );
        steps
    }

    pub(super) fn configure_next(&mut self) {
        let steps = self.selection_steps();
        while let Some((category, id, mut value)) = steps.get(self.config_step).cloned() {
            self.config_step += 1;
            // A probe sets the current model, so efforts that depend on it show.
            if self.probing && category == Category::Model && value.is_empty() {
                value = crate::conversation::option(&self.options, Category::Model)
                    .map(SessionOption::current)
                    .unwrap_or_default();
            }
            if value.is_empty() {
                continue;
            }
            let option = self.find_option(category, &id);
            let label = match category {
                Category::Model => "model",
                Category::Effort => "effort",
                Category::Mode => "mode",
                Category::Other => "option",
            };
            let Some(option) = option else {
                if matches!(category, Category::Model | Category::Effort) {
                    self.fail(format!("Agent does not expose required {label} configuration; saved settings cannot be honored. Log in with the configured agent or choose one that offers this option."), FailureKind::Configuration);
                    return;
                }
                continue;
            };
            if !option.offers(&value) && !profiles::is_plan_mode(self.profile, &value) {
                if matches!(category, Category::Model | Category::Effort) {
                    self.fail(format!("Agent does not offer saved {label} value '{value}'. Choose a supported setting for this conversation or in the agent definition."), FailureKind::Configuration);
                    return;
                }
                continue;
            }
            if category != Category::Model && option.current() == value {
                continue;
            }
            self.set_option(&option, &value, Request::Config);
            return;
        }
        self.configured();
    }

    fn set_option(&mut self, option: &SessionOption, value: &str, request: Request) {
        let Some(session) = self.session_id.clone() else {
            return;
        };
        let outgoing = if option.id == MODE_OPTION {
            Outgoing::SetMode(wire::SetSessionModeRequest::new(session, value.to_owned()))
        } else if matches!(option.kind, OptionKind::Boolean { .. }) {
            Outgoing::SetConfig(wire::SetSessionConfigOptionRequest::new(
                session,
                option.id.clone(),
                value == "true",
            ))
        } else {
            Outgoing::SetConfig(wire::SetSessionConfigOptionRequest::new(
                session,
                option.id.clone(),
                value,
            ))
        };
        if option.id == MODE_OPTION {
            // `session/set_mode` answers without the new state.
            if let Some(modes) = &mut self.modes {
                modes.current_mode_id = wire::SessionModeId::new(value.to_owned());
            }
        }
        self.request(outgoing, request);
    }

    fn find_option(&self, category: Category, id: &str) -> Option<SessionOption> {
        match category {
            Category::Other => self.options.iter().find(|option| option.id == id),
            category => crate::conversation::option(&self.options, category),
        }
        .cloned()
    }

    fn configured(&mut self) {
        self.configured = true;
        if self.probing {
            self.emit(EventKind::Probed);
            return;
        }
        if self.replacing {
            self.replacing = false;
            let context = self.replacement.take().unwrap_or_default();
            if !context.trim().is_empty() {
                if let Some(turn) = &mut self.active {
                    turn.prompt = vec![Part::Text(format!(
                        "Saved conversation context follows. Treat it as history, not a new user request. Do not repeat completed tool actions.\n\n{context}\n\nContinue the interrupted turn from where it stopped; do not repeat completed work."
                    ))];
                    turn.attempt = 0;
                } else {
                    self.pending_context = Some(context);
                }
            }
        }
        if self.active.is_some() {
            self.send_prompt();
        }
    }

    pub(super) fn switch(&mut self, category: Category, id: &str, value: &str) {
        self.config.selections.set(category, id, value);
        // Without a ready, idle session the next setup applies the new value.
        if !self.configured || self.active.is_some() || self.link.is_none() {
            return;
        }
        let option = self.find_option(category, id);
        if let Some(option) = option
            && option.offers(value)
        {
            self.set_option(&option, value, Request::Switch);
            if option.id == MODE_OPTION {
                self.refresh_options();
            }
        }
    }
}

fn translate_options(
    raw: &[wire::SessionConfigOption],
    modes: Option<&wire::SessionModeState>,
    profile: Option<&Profile>,
) -> Vec<SessionOption> {
    let mut options: Vec<SessionOption> = Vec::new();
    for option in raw {
        let id = option.id.0.to_string();
        let mut category = match &option.category {
            Some(wire::SessionConfigOptionCategory::Mode) => Category::Mode,
            Some(wire::SessionConfigOptionCategory::Model) => Category::Model,
            Some(wire::SessionConfigOptionCategory::ThoughtLevel) => Category::Effort,
            Some(_) => Category::Other,
            None => match id.as_str() {
                "model" => Category::Model,
                "thinking" | "effort" | "reasoning_effort" | "thought_level" => Category::Effort,
                "mode" => Category::Mode,
                _ => Category::Other,
            },
        };
        // Each of the composer's own menus shows one option.
        if category != Category::Other && options.iter().any(|o| o.category == category) {
            category = Category::Other;
        }
        let kind = match &option.kind {
            wire::SessionConfigKind::Boolean(boolean) => OptionKind::Boolean {
                current: boolean.current_value,
            },
            wire::SessionConfigKind::Select(select) => {
                let mut choices = Vec::new();
                match &select.options {
                    wire::SessionConfigSelectOptions::Ungrouped(items) => {
                        choices.extend(items.iter().map(|item| select_choice(item, "")));
                        if !choices.is_empty() && choices.iter().all(|c| c.value.contains('/')) {
                            for choice in &mut choices {
                                choice.group = choice
                                    .value
                                    .split('/')
                                    .next()
                                    .unwrap_or_default()
                                    .to_owned();
                            }
                        }
                    }
                    wire::SessionConfigSelectOptions::Grouped(groups) => {
                        for group in groups {
                            choices.extend(
                                group
                                    .options
                                    .iter()
                                    .map(|item| select_choice(item, &group.name)),
                            );
                        }
                    }
                    _ => {}
                }
                if category == Category::Mode {
                    choices.retain(|choice| !profiles::is_plan_mode(profile, &choice.value));
                    for choice in &mut choices {
                        choice.description =
                            profiles::mode_description(profile, &choice.value, &choice.description);
                    }
                }
                OptionKind::Select {
                    current: select.current_value.0.to_string(),
                    choices,
                }
            }
            _ => continue,
        };
        options.push(SessionOption {
            id,
            name: option.name.clone(),
            description: option.description.clone().unwrap_or_default(),
            category,
            kind,
        });
    }
    if let Some(modes) = modes
        && !options
            .iter()
            .any(|option| option.category == Category::Mode)
    {
        options.push(SessionOption {
            id: MODE_OPTION.into(),
            name: "Mode".into(),
            description: String::new(),
            category: Category::Mode,
            kind: OptionKind::Select {
                current: modes.current_mode_id.0.to_string(),
                choices: modes
                    .available_modes
                    .iter()
                    .filter(|mode| !profiles::is_plan_mode(profile, &mode.id.0))
                    .map(|mode| Choice {
                        value: mode.id.0.to_string(),
                        name: mode.name.clone(),
                        description: profiles::mode_description(
                            profile,
                            &mode.id.0,
                            mode.description.as_deref().unwrap_or_default(),
                        ),
                        group: String::new(),
                    })
                    .collect(),
            },
        });
    }
    options
}

fn select_choice(item: &wire::SessionConfigSelectOption, group: &str) -> Choice {
    Choice {
        value: item.value.0.to_string(),
        name: item.name.clone(),
        description: item.description.clone().unwrap_or_default(),
        group: group.to_owned(),
    }
}
