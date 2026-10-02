//! The Add Agent and Edit Agent form: harness, model and effort come from the
//! harness itself through a background probe.
use super::*;
use agents::{AgentDefinition, InstructionsMode, PermissionMode};
use gpui_kit::component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonVariants},
    form::{Field, Form},
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    radio::RadioGroup,
    select::{SearchableVec, Select, SelectEvent, SelectGroup, SelectItem, SelectState},
    spinner::Spinner,
    text::TextView,
};
use harness::{CUSTOM, Catalog, Kind, ProbeError, Probed};
use std::{cell::RefCell, rc::Rc};

/// A harness in the picker, with its installed dot.
#[derive(Clone)]
struct HarnessItem {
    id: SharedString,
    name: SharedString,
    installed: Option<bool>,
    /// Where the installed executable was found.
    path: Option<SharedString>,
}

impl SelectItem for HarnessItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        harness_row(
            &self.id,
            self.name.clone(),
            self.installed,
            self.path.clone(),
            cx,
        )
    }

    fn value(&self) -> &SharedString {
        &self.id
    }
}

/// A model or effort the harness offers: its name, with the ID as secondary text.
#[derive(Clone)]
struct ChoiceItem {
    value: SharedString,
    name: SharedString,
    detail: bool,
}

impl SelectItem for ChoiceItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        row()
            .w_full()
            .gap_2()
            .child(div().truncate().child(self.name.clone()))
            .when(self.detail && self.value != self.name, |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.value.clone()),
                )
            })
    }

    fn value(&self) -> &SharedString {
        &self.value
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.name.to_lowercase().contains(&query) || self.value.to_lowercase().contains(&query)
    }
}

type HarnessPicker = SelectState<SearchableVec<HarnessItem>>;
type ModelPicker = SelectState<SearchableVec<SelectGroup<ChoiceItem>>>;
type EffortPicker = SelectState<Vec<ChoiceItem>>;

/// A harness icon and name, with a green dot when installed and red when not,
/// followed by where it was found.
fn harness_row(
    id: &str,
    name: impl Into<SharedString>,
    installed: Option<bool>,
    path: Option<SharedString>,
    cx: &App,
) -> Div {
    let icon = if harness::has_icon(id) {
        format!("registry-icons/{id}.svg")
    } else {
        "robot.svg".into()
    };
    row()
        .gap_2()
        .min_w_0()
        .child(
            Icon::default()
                .path(icon)
                .size_4()
                .text_color(cx.theme().foreground),
        )
        .child(div().flex_shrink_0().child(name.into()))
        .children(installed.map(installed_dot))
        .children(path.map(|path| found_on(&path, cx)))
}

/// `Found on <path>`, in muted text.
fn found_on(path: &str, cx: &App) -> Div {
    div()
        .min_w_0()
        .truncate()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(format!("Found on {path}"))
}

/// Green when the harness is installed, red when it is not.
pub(super) fn installed_dot(installed: bool) -> Div {
    div()
        .flex_shrink_0()
        .size_2()
        .rounded_full()
        .bg(if installed {
            hsla(142. / 360., 0.71, 0.45, 1.)
        } else {
            hsla(0., 0.84, 0.6, 1.)
        })
}

enum Probing {
    /// Nothing to probe: no harness, an uninstalled one, or no Custom command.
    Idle,
    Loading,
    Ready(Probed),
    Failed(ProbeError),
}

pub(super) struct AgentForm {
    pub(super) id: Option<String>,
    pub(super) original: AgentDefinition,
    pub(super) external_changed: bool,
    pub(super) status: Option<String>,
    name: Entity<InputState>,
    harness: Entity<HarnessPicker>,
    command: Entity<InputState>,
    arguments: Rc<RefCell<Vec<Entity<InputState>>>>,
    model: Entity<ModelPicker>,
    effort: Entity<EffortPicker>,
    model_text: Entity<InputState>,
    effort_text: Entity<InputState>,
    instructions: Entity<TextareaState>,
    instructions_mode: InstructionsMode,
    permission_mode: PermissionMode,
    probing: Probing,
    /// The probe's model and effort lists; kept while a model change re-probes.
    offered: Option<Probed>,
    probe: Option<harness::Probe>,
    probe_generation: u64,
    _subscriptions: Vec<Subscription>,
}

fn input(
    value: &str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut input = InputState::new(window, cx).placeholder(placeholder);
        input.set_value(value.to_owned(), window, cx);
        input
    })
}

fn harness_items(cx: &App) -> SearchableVec<HarnessItem> {
    let catalog = cx.global::<Catalog>();
    let mut items: Vec<_> = catalog
        .sorted()
        .into_iter()
        .map(|harness| HarnessItem {
            id: harness.id.clone().into(),
            name: harness.name.clone().into(),
            installed: catalog.is_installed(&harness.id),
            path: catalog
                .installed
                .get(&harness.id)
                .map(|path| path.display().to_string().into()),
        })
        .collect();
    items.push(HarnessItem {
        id: CUSTOM.into(),
        name: "Custom".into(),
        installed: None,
        path: None,
    });
    SearchableVec::new(items)
}

impl AgentForm {
    pub(super) fn new(
        id: Option<String>,
        definition: AgentDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = input(&definition.name, "Name", window, cx);
        let harness = cx.new(|cx| {
            let mut picker = SelectState::new(harness_items(cx), None, window, cx).searchable(true);
            picker.set_selected_value(&definition.harness.clone().into(), window, cx);
            picker
        });
        let command = input(&definition.command, "omp.exe", window, cx);
        let model = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let effort = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let model_text = input(&definition.model, "Model ID", window, cx);
        let effort_text = input(&definition.effort, "Effort", window, cx);
        let instructions = cx.new(|cx| {
            let mut state =
                TextareaState::new(window, cx).placeholder("System instructions (optional)");
            state.set_value(definition.system_instructions.clone(), window, cx);
            state
        });
        let arguments = Rc::new(RefCell::new(
            definition
                .arguments
                .iter()
                .map(|value| input(value, "Argument", window, cx))
                .collect(),
        ));
        let mut subscriptions = vec![
            cx.subscribe_in(
                &harness,
                window,
                |this, _, _: &SelectEvent<_>, window, cx| {
                    this.status = None;
                    this.offered = None;
                    this.start_probe(window, cx);
                },
            ),
            cx.subscribe_in(&model, window, |this, _, _: &SelectEvent<_>, window, cx| {
                this.status = None;
                // The effort list depends on the model.
                this.start_probe(window, cx);
            }),
            cx.subscribe(&effort, |this, _, _: &SelectEvent<_>, cx| {
                this.status = None;
                cx.notify();
            }),
            cx.subscribe_in(
                &command,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.start_probe(window, cx);
                    }
                },
            ),
            cx.observe_global_in::<Catalog>(window, |this, window, cx| {
                let selected = this.harness_id(cx);
                this.harness.update(cx, |picker, cx| {
                    picker.set_items(harness_items(cx), window, cx);
                    if let Some(selected) = selected {
                        picker.set_selected_value(&selected.into(), window, cx);
                    }
                });
                // Detection may have just found the selected harness.
                if matches!(this.probing, Probing::Idle) {
                    this.start_probe(window, cx);
                }
                cx.notify();
            }),
        ];
        for text in [&name, &model_text, &effort_text, &command] {
            subscriptions.push(cx.subscribe(text, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.status = None;
                    cx.notify();
                }
            }));
        }
        // The instructions show as Markdown until focused.
        subscriptions.push(cx.subscribe(&instructions, |this, _, _: &InputEvent, cx| {
            this.status = None;
            cx.notify();
        }));
        let mut form = Self {
            id,
            instructions_mode: definition.instructions_mode,
            permission_mode: definition.permission_mode,
            original: definition,
            external_changed: false,
            status: None,
            name,
            harness,
            command,
            arguments,
            model,
            effort,
            model_text,
            effort_text,
            instructions,
            probing: Probing::Idle,
            offered: None,
            probe: None,
            probe_generation: 0,
            _subscriptions: subscriptions,
        };
        form.start_probe(window, cx);
        form
    }

    pub(super) fn name_focus(&self, cx: &App) -> FocusHandle {
        self.name.read(cx).focus_handle(cx)
    }

    pub(super) fn name(&self, cx: &App) -> String {
        self.name.read(cx).value().to_string()
    }

    fn harness_id(&self, cx: &App) -> Option<String> {
        self.harness
            .read(cx)
            .selected_value()
            .map(ToString::to_string)
    }

    fn identity(&self, cx: &App) -> String {
        match (&self.probing, &self.offered) {
            (_, Some(probed)) | (Probing::Ready(probed), _) if !probed.identity.is_empty() => {
                probed.identity.clone()
            }
            _ if self.harness_id(cx).as_deref() == Some(self.original.harness.as_str()) => {
                self.original.identity.clone()
            }
            _ => String::new(),
        }
    }

    fn arguments(&self, cx: &App) -> Vec<String> {
        self.arguments
            .borrow()
            .iter()
            .map(|input| input.read(cx).value().to_string())
            .collect()
    }

    /// Starts a fresh probe for the selected harness, stopping any earlier one.
    fn start_probe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.probe = None;
        self.probe_generation += 1;
        cx.notify();
        let Some(id) = self.harness_id(cx) else {
            self.probing = Probing::Idle;
            return;
        };
        let catalog = cx.global::<Catalog>();
        let launch = if id == CUSTOM {
            let command = self.command.read(cx).value().trim().to_owned();
            if command.is_empty() {
                self.probing = Probing::Idle;
                return;
            }
            let Some(path) = harness::resolve(&command) else {
                self.probing = Probing::Failed(ProbeError {
                    message: format!("Command {command} was not found on this machine."),
                    login: Vec::new(),
                });
                return;
            };
            (path, self.arguments(cx))
        } else if let (Some(path), Some(harness)) = (catalog.installed.get(&id), catalog.get(&id)) {
            (path.clone(), harness.arguments.clone())
        } else {
            self.probing = Probing::Idle;
            return;
        };
        if catalog.demo() {
            self.probing = Probing::Failed(ProbeError {
                message: "Demo mode does not start harnesses. Type the model and effort instead."
                    .into(),
                login: Vec::new(),
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
        let (probe, results) = harness::probe(launch.0, launch.1, model);
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
                let models = harness::setting(&probed.options, Kind::Model);
                let efforts = harness::setting(&probed.options, Kind::Effort);
                let mut groups: Vec<SelectGroup<ChoiceItem>> = Vec::new();
                for choice in models.iter().flat_map(|setting| &setting.choices) {
                    let item = ChoiceItem {
                        value: choice.value.clone().into(),
                        name: choice.name.clone().into(),
                        detail: true,
                    };
                    match groups.last_mut() {
                        Some(group) if group.title.as_ref() == choice.group => {
                            group.items.push(item);
                        }
                        _ => groups.push(SelectGroup::new(choice.group.clone()).item(item)),
                    }
                }
                self.model.update(cx, |picker, cx| {
                    picker.set_items(SearchableVec::new(groups), window, cx);
                    picker.set_selected_value(&model.into(), window, cx);
                });
                let efforts: Vec<_> = efforts
                    .iter()
                    .flat_map(|setting| &setting.choices)
                    .map(|choice| ChoiceItem {
                        value: choice.value.clone().into(),
                        name: choice.name.clone().into(),
                        detail: false,
                    })
                    .collect();
                self.effort.update(cx, |picker, cx| {
                    picker.set_items(efforts, window, cx);
                    picker.set_selected_value(&effort.into(), window, cx);
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

    fn setting(&self, kind: Kind) -> Option<harness::Setting> {
        self.offered
            .as_ref()
            .and_then(|probed| harness::setting(&probed.options, kind))
    }

    /// The model or effort to save: the picked offer, the free text after a
    /// failed probe, or the saved value while nothing is known yet.
    fn chosen(&self, kind: Kind, cx: &App) -> String {
        let (picker, text, saved) = match kind {
            Kind::Model => (
                self.model.read(cx).selected_value().cloned(),
                &self.model_text,
                &self.original.model,
            ),
            Kind::Effort => (
                self.effort.read(cx).selected_value().cloned(),
                &self.effort_text,
                &self.original.effort,
            ),
        };
        if matches!(self.probing, Probing::Failed(_)) {
            return text.read(cx).value().trim().to_owned();
        }
        if self.offered.is_some() {
            return if self.setting(kind).is_some() {
                picker.map(|value| value.to_string()).unwrap_or_default()
            } else {
                String::new()
            };
        }
        if self.harness_id(cx).as_deref() == Some(self.original.harness.as_str()) {
            saved.clone()
        } else {
            String::new()
        }
    }

    pub(super) fn values(&self, cx: &App) -> AgentDefinition {
        let harness = self.harness_id(cx).unwrap_or_default();
        let custom = harness == CUSTOM;
        AgentDefinition {
            name: self.name(cx),
            identity: if custom {
                self.identity(cx)
            } else {
                String::new()
            },
            command: if custom {
                self.command.read(cx).value().to_string()
            } else {
                String::new()
            },
            arguments: if custom {
                self.arguments(cx)
            } else {
                Vec::new()
            },
            model: self.chosen(Kind::Model, cx),
            effort: self.chosen(Kind::Effort, cx),
            permission_mode: self.permission_mode,
            system_instructions: self.instructions.read(cx).value().to_string(),
            instructions_mode: self.instructions_mode,
            harness,
            ..Default::default()
        }
    }

    /// Why Save is unavailable: no usable harness, or options still loading.
    pub(super) fn blocked(&self, cx: &App) -> bool {
        let Some(id) = self.harness_id(cx) else {
            return true;
        };
        cx.global::<Catalog>().is_installed(&id) != Some(true)
            || matches!(self.probing, Probing::Loading)
    }

    /// Checks the choices only the form knows: required model and effort.
    pub(super) fn check(&self, cx: &App) -> Result<(), String> {
        if self.name(cx).trim().is_empty() {
            return Err("Name is required.".into());
        }
        let Some(id) = self.harness_id(cx) else {
            return Err("Choose a harness.".into());
        };
        if cx.global::<Catalog>().is_installed(&id) != Some(true) {
            return Err("This harness is not installed.".into());
        }
        match &self.probing {
            Probing::Loading => return Err("Wait for the harness options to load.".into()),
            Probing::Idle if id == CUSTOM => return Err("Command is required.".into()),
            Probing::Failed(_) => return Ok(()),
            _ => {}
        }
        for (kind, label, saved) in [
            (Kind::Model, "model", &self.original.model),
            (Kind::Effort, "effort", &self.original.effort),
        ] {
            if self.setting(kind).is_some() && self.chosen(kind, cx).is_empty() {
                return Err(if self.not_offered(kind, cx) {
                    format!("Saved {label} {saved} is not offered by the harness. Choose another.")
                } else {
                    format!("Choose a {label}.")
                });
            }
        }
        Ok(())
    }

    /// The saved value of an unchanged harness is missing from its offers.
    fn not_offered(&self, kind: Kind, cx: &App) -> bool {
        let saved = match kind {
            Kind::Model => &self.original.model,
            Kind::Effort => &self.original.effort,
        };
        !saved.is_empty()
            && self.harness_id(cx).as_deref() == Some(self.original.harness.as_str())
            && self
                .setting(kind)
                .is_some_and(|setting| !setting.offers(saved))
    }

    pub(super) fn dirty(&self, cx: &App) -> bool {
        self.values(cx) != self.original
    }

    pub(super) fn reload(
        &mut self,
        definition: AgentDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name.update(cx, |input, cx| {
            input.set_value(definition.name.clone(), window, cx);
        });
        self.command.update(cx, |input, cx| {
            input.set_value(definition.command.clone(), window, cx);
        });
        self.model_text.update(cx, |input, cx| {
            input.set_value(definition.model.clone(), window, cx);
        });
        self.effort_text.update(cx, |input, cx| {
            input.set_value(definition.effort.clone(), window, cx);
        });
        self.instructions.update(cx, |input, cx| {
            input.set_value(definition.system_instructions.clone(), window, cx);
        });
        *self.arguments.borrow_mut() = definition
            .arguments
            .iter()
            .map(|value| input(value, "Argument", window, cx))
            .collect();
        self.harness.update(cx, |picker, cx| {
            picker.set_selected_value(&definition.harness.clone().into(), window, cx);
        });
        self.model
            .update(cx, |picker, cx| picker.set_selected_index(None, window, cx));
        self.effort
            .update(cx, |picker, cx| picker.set_selected_index(None, window, cx));
        self.permission_mode = definition.permission_mode;
        self.instructions_mode = definition.instructions_mode;
        self.original = definition;
        self.external_changed = false;
        self.status = None;
        self.offered = None;
        self.start_probe(window, cx);
    }

    fn harness_status(&self, id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let catalog = cx.global::<Catalog>();
        if id == CUSTOM {
            return None;
        }
        match catalog.is_installed(id) {
            Some(true) => catalog.installed.get(id).map(|path| {
                row()
                    .gap_2()
                    .min_w_0()
                    .child(installed_dot(true))
                    .child(found_on(&path.display().to_string(), cx))
                    .into_any_element()
            }),
            None => Some(muted("Checking whether this harness is installed…", cx)),
            Some(false) => {
                let website = catalog
                    .get(id)
                    .map(|harness| harness.website.clone())
                    .unwrap_or_default();
                Some(
                    row()
                        .gap_2()
                        .text_sm()
                        .child(installed_dot(false))
                        .child("Not installed")
                        .when(!website.is_empty(), |row| {
                            row.child(
                                Button::new("agent-harness-website")
                                    .link()
                                    .small()
                                    .label(website.clone())
                                    .on_click(move |_, _, cx| cx.open_url(&website)),
                            )
                        })
                        .into_any_element(),
                )
            }
        }
    }

    fn probe_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        match &self.probing {
            Probing::Loading => {
                let name = self.harness_id(cx).map_or_else(
                    || "the harness".to_owned(),
                    |id| cx.global::<Catalog>().label(&id, &self.identity(cx)),
                );
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
                if !error.login.is_empty() {
                    column = column.child(div().text_sm().child(
                        "Sign in through the harness outside Adeline with one of its login methods, then retry:",
                    ));
                    for method in &error.login {
                        column = column.child(div().text_sm().pl_3().child(format!("• {method}")));
                    }
                }
                Some(
                    column
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
                        )
                        .into_any_element(),
                )
            }
            Probing::Idle | Probing::Ready(_) => None,
        }
    }

    fn argument_fields(&self) -> Div {
        let mut fields = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child("Arguments (optional, one literal value per entry)");
        let count = self.arguments.borrow().len();
        for (i, input) in self.arguments.borrow().iter().enumerate() {
            let key = input.entity_id();
            let mut row = div().w_full().flex().items_center().gap_2().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(input).aria_label(format!("Argument {}", i + 1))),
            );
            for (action, label) in ["Up", "Down", "Remove"].into_iter().enumerate() {
                let arguments = self.arguments.clone();
                row = row.child(
                    Button::new(format!("argument-{key:?}-{action}"))
                        .small()
                        .ghost()
                        .label(label)
                        .accessibility_label(format!("{label} argument {}", i + 1))
                        .disabled((action == 0 && i == 0) || (action == 1 && i + 1 == count))
                        .on_click(move |_, _, cx| {
                            let mut inputs = arguments.borrow_mut();
                            if i >= inputs.len() {
                                return;
                            }
                            match action {
                                0 if i > 0 => inputs.swap(i, i - 1),
                                1 if i + 1 < inputs.len() => inputs.swap(i, i + 1),
                                2 => {
                                    inputs.remove(i);
                                }
                                _ => return,
                            }
                            cx.refresh_windows();
                        }),
                );
            }
            fields = fields.child(row);
        }
        let arguments = self.arguments.clone();
        fields.child(
            Button::new("agent-add-argument")
                .small()
                .label("Add argument")
                .on_click(move |_, window, cx| {
                    arguments
                        .borrow_mut()
                        .push(input("", "Argument", window, cx));
                    cx.refresh_windows();
                }),
        )
    }

    fn instructions_field(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let text = self.instructions.read(cx).value().to_string();
        let focus = self.instructions.read(cx).focus_handle(cx);
        // Markdown until focused; clicking it switches to the raw text.
        if focus.is_focused(window) || text.trim().is_empty() {
            return Textarea::new(&self.instructions)
                .aria_label("System instructions")
                .h_32()
                .into_any_element();
        }
        div()
            .id("agent-instructions-preview")
            .w_full()
            .min_h_32()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().input)
            .cursor_text()
            .child(TextView::markdown("agent-instructions-markdown", text))
            .on_click(move |_, window, cx| window.focus(&focus, cx))
            .into_any_element()
    }
}

fn muted(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}

impl Render for AgentForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.harness_id(cx);
        let usable = id
            .as_deref()
            .is_some_and(|id| cx.global::<Catalog>().is_installed(id) == Some(true));
        let failed = matches!(self.probing, Probing::Failed(_));
        let custom = id.as_deref() == Some(CUSTOM);
        let mut form = Form::new()
            .child(
                Field::new()
                    .label("Name")
                    .child(Input::new(&self.name).aria_label("Name")),
            )
            .child(
                Field::new().label("Harness").child(
                    Select::new(&self.harness)
                        .placeholder("Choose a harness")
                        .accessibility_label("Harness"),
                ),
            );
        let status = id.as_deref().and_then(|id| self.harness_status(id, cx));
        let mut body = col().w_full().gap_4().child(form);
        body = body.children(status);
        if custom {
            body = body
                .child(
                    Form::new().child(
                        Field::new()
                            .label("Command")
                            .child(Input::new(&self.command).aria_label("Command")),
                    ),
                )
                .child(self.argument_fields());
            if let Some(identity) = (!self.identity(cx).is_empty()).then(|| self.identity(cx)) {
                body = body.child(muted(cx.global::<Catalog>().label(CUSTOM, &identity), cx));
            }
        }
        if usable {
            body = body.children(self.probe_status(cx));
        }
        form = Form::new();
        if usable && failed {
            form = form
                .child(
                    Field::new()
                        .label("Model")
                        .child(Input::new(&self.model_text).aria_label("Model")),
                )
                .child(
                    Field::new()
                        .label("Effort")
                        .child(Input::new(&self.effort_text).aria_label("Effort")),
                );
        } else if usable && self.offered.is_some() {
            if self.setting(Kind::Model).is_some() {
                let marker = self
                    .not_offered(Kind::Model, cx)
                    .then(|| format!("{} — not offered by harness", self.original.model));
                form = form.child(
                    Field::new()
                        .label("Model")
                        .child(
                            Select::new(&self.model)
                                .placeholder(marker.clone().unwrap_or("Choose a model".into()))
                                .search_placeholder("Search models")
                                .accessibility_label("Model"),
                        )
                        .when_some(marker, |field, marker| field.description(marker)),
                );
            }
            if self.setting(Kind::Effort).is_some() {
                let marker = self
                    .not_offered(Kind::Effort, cx)
                    .then(|| format!("{} — not offered by harness", self.original.effort));
                form = form.child(
                    Field::new()
                        .label("Effort")
                        .child(
                            Select::new(&self.effort)
                                .placeholder(marker.clone().unwrap_or("Choose an effort".into()))
                                .accessibility_label("Effort")
                                .disabled(matches!(self.probing, Probing::Loading)),
                        )
                        .when_some(marker, |field, marker| field.description(marker)),
                );
            }
        }
        let permission = match self.permission_mode {
            PermissionMode::Ask => 0,
            PermissionMode::AllowEverything => 1,
        };
        form = form.child(
            Field::new().label("Default permission mode").child(
                RadioGroup::horizontal("agent-permission")
                    .children(["Ask", "Allow everything"])
                    .selected_index(Some(permission))
                    .on_change(cx.listener(|this, index: &usize, _, cx| {
                        this.permission_mode = if *index == 0 {
                            PermissionMode::Ask
                        } else {
                            PermissionMode::AllowEverything
                        };
                        cx.notify();
                    })),
            ),
        );
        body = body.child(form);
        if let Some(id) = id.as_deref() {
            if harness::supports_instructions(id, &self.identity(cx)) {
                let mode = match self.instructions_mode {
                    InstructionsMode::Append => 0,
                    InstructionsMode::Overwrite => 1,
                };
                body = body.child(
                    Form::new()
                        .child(
                            Field::new()
                                .label("System instructions (optional, Markdown)")
                                .child(self.instructions_field(window, cx)),
                        )
                        .child(
                            Field::new().label("Instructions").child(
                                RadioGroup::horizontal("agent-instructions-mode")
                                    .children(["Append to the harness's guidance", "Overwrite it"])
                                    .selected_index(Some(mode))
                                    .on_change(cx.listener(|this, index: &usize, _, cx| {
                                        this.instructions_mode = if *index == 0 {
                                            InstructionsMode::Append
                                        } else {
                                            InstructionsMode::Overwrite
                                        };
                                        cx.notify();
                                    })),
                            ),
                        ),
                );
            } else if usable {
                body = body.child(muted(
                    "This harness does not accept system instructions from Adeline.",
                    cx,
                ));
            }
        }
        body
    }
}
