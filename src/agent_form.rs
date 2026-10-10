//! The Add Agent and Edit Agent form. Model, effort and mode come from the
//! agent itself through a background probe. Under the harness picker it
//! installs, updates and logs in supported agents (DD6, DD16).
use super::*;
use crate::conversation::{AuthMethod, Category, McpServer};
use crate::install::{Planned, Target};
use acp::{ProbeError, Probed};
use agents::{AgentDefinition, InstructionsMode};
use gpui_kit::component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonVariants},
    form::{Field, Form},
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    link::Link,
    radio::RadioGroup,
    select::{SearchableVec, Select, SelectEvent, SelectGroup, SelectItem, SelectState},
    spinner::Spinner,
    text::TextView,
};
use harness::{CUSTOM, Catalog};
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

/// A model, effort or mode the agent offers: its name, with the ID or the
/// description as secondary text.
#[derive(Clone)]
struct ChoiceItem {
    value: SharedString,
    name: SharedString,
    detail: SharedString,
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
            .when(!self.detail.is_empty() && self.detail != self.name, |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.detail.clone()),
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
type ChoicePicker = SelectState<Vec<ChoiceItem>>;

/// A harness icon and name, with a green dot when installed and red when not,
/// followed by where it was found.
fn harness_row(
    id: &str,
    name: impl Into<SharedString>,
    installed: Option<bool>,
    path: Option<SharedString>,
    cx: &App,
) -> Div {
    row()
        .gap_2()
        .min_w_0()
        .child(
            Icon::default()
                .path(harness::icon_path(id))
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

/// An install or update in the DD6 area: confirm, run with live output, recheck.
enum Install {
    /// Asking for the commands this machine would run.
    Planning,
    Confirm(Target, Vec<Planned>),
    Running(Vec<String>),
    Failed(Target, Vec<String>, String),
    /// Unavailable, with the reason (no package manager for Node.js).
    Unavailable(String),
}

pub(super) struct AgentForm {
    /// The machine the agent belongs to, whose engine probes it.
    pub(super) machine: String,
    pub(super) id: Option<String>,
    pub(super) original: AgentDefinition,
    pub(super) external_changed: bool,
    pub(super) status: Option<String>,
    name: Entity<InputState>,
    harness: Entity<HarnessPicker>,
    command: Entity<InputState>,
    arguments: Rc<RefCell<Vec<Entity<InputState>>>>,
    model: Entity<ModelPicker>,
    effort: Entity<ChoicePicker>,
    mode: Entity<ChoicePicker>,
    model_text: Entity<InputState>,
    effort_text: Entity<InputState>,
    instructions: Entity<TextareaState>,
    instructions_mode: InstructionsMode,
    mcp_servers: Vec<McpServer>,
    probing: Probing,
    /// The probe's lists; kept while a model change re-probes.
    offered: Option<Probed>,
    probe: Option<client::Probe>,
    probe_generation: u64,
    install: Option<Install>,
    /// A login or logout running from this form, and what it last said.
    login: Option<String>,
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

/// Choices of one of the probe's options, for a picker.
fn choices(probed: &Probed, category: Category) -> Vec<ChoiceItem> {
    conversation::option(&probed.options, category)
        .map(|option| {
            option
                .choices()
                .iter()
                .map(|choice| ChoiceItem {
                    value: choice.value.clone().into(),
                    name: choice.name.clone().into(),
                    detail: if category == Category::Model {
                        choice.value.clone().into()
                    } else {
                        choice.description.clone().into()
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

impl AgentForm {
    pub(super) fn new(
        machine: String,
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
        let mode = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
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
                    this.install = None;
                    this.login = None;
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
            cx.subscribe(&mode, |this, _, _: &SelectEvent<_>, cx| {
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
            machine,
            id,
            instructions_mode: definition.instructions_mode,
            mcp_servers: definition.mcp_servers.clone(),
            original: definition,
            external_changed: false,
            status: None,
            name,
            harness,
            command,
            arguments,
            model,
            effort,
            mode,
            model_text,
            effort_text,
            instructions,
            probing: Probing::Idle,
            offered: None,
            probe: None,
            probe_generation: 0,
            install: None,
            login: None,
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

    /// The agent's display name for sentences: its profile's, else the harness label.
    fn agent_label(&self, cx: &App) -> String {
        let id = self.harness_id(cx).unwrap_or_default();
        profiles::find(&id, &self.identity(cx)).map_or_else(
            || cx.global::<Catalog>().label(&id, &self.identity(cx)),
            |profile| profile.name.to_owned(),
        )
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
        if catalog.demo() {
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
                // Plan modes are already left out by the agent layer (scope R20).
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

    fn offers(&self, category: Category) -> bool {
        self.offered
            .as_ref()
            .is_some_and(|probed| conversation::option(&probed.options, category).is_some())
    }

    /// The value to save: the picked offer, the free text after a failed
    /// probe, or the saved value while nothing is known yet.
    fn chosen(&self, category: Category, cx: &App) -> String {
        let (picker, text, saved) = match category {
            Category::Model => (
                self.model.read(cx).selected_value().cloned(),
                Some(&self.model_text),
                &self.original.model,
            ),
            Category::Effort => (
                self.effort.read(cx).selected_value().cloned(),
                Some(&self.effort_text),
                &self.original.effort,
            ),
            _ => (
                self.mode.read(cx).selected_value().cloned(),
                None,
                &self.original.mode,
            ),
        };
        if matches!(self.probing, Probing::Failed(_)) {
            return text.map_or_else(
                || saved.clone(),
                |text| text.read(cx).value().trim().to_owned(),
            );
        }
        if self.offered.is_some() {
            return if self.offers(category) {
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
            model: self.chosen(Category::Model, cx),
            effort: self.chosen(Category::Effort, cx),
            mode: self.chosen(Category::Mode, cx),
            system_instructions: self.instructions.read(cx).value().to_string(),
            instructions_mode: self.instructions_mode,
            mcp_servers: self.mcp_servers.clone(),
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
            Probing::Loading => return Err("Wait for the agent's options to load.".into()),
            Probing::Idle if id == CUSTOM => return Err("Command is required.".into()),
            Probing::Failed(_) => return Ok(()),
            _ => {}
        }
        for (category, label, saved) in [
            (Category::Model, "model", &self.original.model),
            (Category::Effort, "effort", &self.original.effort),
        ] {
            if self.offers(category) && self.chosen(category, cx).is_empty() {
                return Err(if self.not_offered(category, cx) {
                    format!("Saved {label} {saved} is not offered by the agent. Choose another.")
                } else {
                    format!("Choose a {label}.")
                });
            }
        }
        Ok(())
    }

    /// The saved value of an unchanged harness is missing from its offers.
    fn not_offered(&self, category: Category, cx: &App) -> bool {
        let saved = match category {
            Category::Model => &self.original.model,
            Category::Effort => &self.original.effort,
            _ => &self.original.mode,
        };
        !saved.is_empty()
            && self.harness_id(cx).as_deref() == Some(self.original.harness.as_str())
            && self.offered.as_ref().is_some_and(|probed| {
                conversation::option(&probed.options, category)
                    .is_some_and(|option| !option.offers(saved))
            })
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
        for picker in [&self.effort, &self.mode] {
            picker.update(cx, |picker, cx| picker.set_selected_index(None, window, cx));
        }
        self.model
            .update(cx, |picker, cx| picker.set_selected_index(None, window, cx));
        self.instructions_mode = definition.instructions_mode;
        self.mcp_servers.clone_from(&definition.mcp_servers);
        self.original = definition;
        self.external_changed = false;
        self.status = None;
        self.offered = None;
        self.start_probe(window, cx);
    }

    /// Asks the engine which commands install `target` on its machine (DD6 step 2).
    fn plan_install(&mut self, target: Target, cx: &mut Context<Self>) {
        self.install = Some(Install::Planning);
        let view = cx.entity().downgrade();
        let request_target = target.clone();
        client::request(
            &self.machine,
            protocol::Command::PlanInstall {
                target: request_target,
            },
            Box::new(move |result, cx| {
                let _ = view.update(cx, |form, cx| {
                    form.install = Some(
                        match result.and_then(|value| {
                            serde_json::from_value(value).map_err(|e| e.to_string())
                        }) {
                            Ok(steps) => Install::Confirm(target, steps),
                            Err(error) => Install::Unavailable(error),
                        },
                    );
                    cx.notify();
                });
            }),
            cx,
        );
        cx.notify();
    }

    /// Runs the confirmed install with its output streaming in (DD6 step 3).
    fn run_install(&mut self, target: Target, cx: &mut Context<Self>) {
        self.install = Some(Install::Running(Vec::new()));
        let output_view = cx.entity().downgrade();
        let view = output_view.clone();
        let failed_target = target.clone();
        client::request_with_output(
            &self.machine,
            protocol::Command::Install { target },
            Box::new(move |line, cx| {
                let _ = output_view.update(cx, |form, cx| {
                    if let Some(Install::Running(lines)) = &mut form.install {
                        lines.push(line);
                    }
                    cx.notify();
                });
            }),
            Box::new(move |result, cx| {
                let _ = view.update(cx, |form, cx| {
                    let lines = match form.install.take() {
                        Some(Install::Running(lines)) => lines,
                        _ => Vec::new(),
                    };
                    // Success: the engine checks again, and the harness list follows.
                    form.install = result
                        .err()
                        .map(|error| Install::Failed(failed_target.clone(), lines, error));
                    cx.notify();
                });
            }),
            cx,
        );
        cx.notify();
    }

    /// Logs in with an agent's method, or out without one (DD16).
    fn run_login(
        &mut self,
        method: Option<AuthMethod>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(harness) = self.harness_id(cx) else {
            return;
        };
        let custom = harness == CUSTOM;
        let terminal = method.as_ref().is_some_and(|m| m.terminal.is_some());
        let logout = method.is_none();
        self.login = Some(
            if logout {
                "Logging out…"
            } else {
                "Logging in…"
            }
            .into(),
        );
        let view = cx.entity().downgrade();
        let window_handle = window.window_handle();
        client::request(
            &self.machine,
            protocol::Command::Login {
                harness,
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
                method,
            },
            Box::new(move |result, cx| {
                let _ = cx.update_window(window_handle, |_, window, cx| {
                    let _ = view.update(cx, |form, cx| {
                        form.login = Some(match result {
                            Ok(_) if terminal => {
                                "Finish logging in in the terminal, then check again.".into()
                            }
                            Ok(_) if logout => "Logged out".into(),
                            Ok(_) => "Logged in".into(),
                            Err(error) => error,
                        });
                        if !terminal {
                            form.start_probe(window, cx);
                        }
                    });
                });
            }),
            cx,
        );
        cx.notify();
    }

    /// The area under the harness picker: found on path, the install, update,
    /// Node.js and logout offers, and an install's live output (DD6, DD16).
    fn harness_status(&self, id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        if id == CUSTOM {
            return None;
        }
        if let Some(install) = &self.install {
            return Some(self.install_area(install, cx));
        }
        let catalog = cx.global::<Catalog>();
        let profile = profiles::find(id, "");
        let name = profile.map_or_else(|| catalog.label(id, ""), |p| p.name.to_owned());
        let logout = self
            .offered
            .as_ref()
            .is_some_and(|probed| probed.features.logout);
        let login_note = self.login.clone().map(|note| muted(note, cx));
        match catalog.is_installed(id) {
            Some(true) => {
                let update = catalog
                    .update(id)
                    .map(|(installed, latest)| (installed.to_owned(), latest.to_owned()));
                let path = catalog
                    .installed
                    .get(id)
                    .map(|path| path.display().to_string());
                Some(
                    col()
                        .gap_2()
                        .child(
                            row()
                                .gap_2()
                                .min_w_0()
                                .child(installed_dot(true))
                                .children(path.map(|path| found_on(&path, cx)))
                                .when(logout, |row| {
                                    row.child(
                                        Button::new("agent-logout")
                                            .ghost()
                                            .xsmall()
                                            .label("Log out")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.run_login(None, window, cx);
                                            })),
                                    )
                                }),
                        )
                        .when_some(
                            update.filter(|_| profile.is_some()),
                            |column, (installed, latest)| {
                                let target = Target::Agent {
                                    harness: id.to_owned(),
                                };
                                column.child(
                                    row()
                                        .gap_2()
                                        .text_sm()
                                        .child(format!(
                                            "Installed {installed}. Version {latest} is available."
                                        ))
                                        .child(
                                            Button::new("agent-update")
                                                .small()
                                                .label("Update…")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.plan_install(target.clone(), cx);
                                                })),
                                        ),
                                )
                            },
                        )
                        .children(login_note)
                        .into_any_element(),
                )
            }
            None => Some(muted("Checking whether this agent is installed…", cx)),
            Some(false) => {
                let website = catalog
                    .get(id)
                    .map(|harness| harness.website.clone())
                    .unwrap_or_default();
                let node_missing =
                    profile.is_some_and(|p| p.install.needs_node) && catalog.node.npm.is_none();
                let mut status = col().gap_2().child(
                    row()
                        .gap_2()
                        .text_sm()
                        .child(installed_dot(false))
                        .child("Not installed")
                        .when(profile.is_some() && !node_missing, |row| {
                            let target = Target::Agent {
                                harness: id.to_owned(),
                            };
                            row.child(
                                Button::new("agent-install")
                                    .small()
                                    .label("Install…")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.plan_install(target.clone(), cx);
                                    })),
                            )
                        })
                        .when(!website.is_empty(), |row| {
                            row.child(
                                Link::new("agent-harness-website")
                                    .href(website.clone())
                                    .child(website.clone()),
                            )
                        }),
                );
                if node_missing {
                    status = status.child(
                        row()
                            .gap_2()
                            .text_sm()
                            .child(format!("{name} needs Node.js."))
                            .when(catalog.node.manager.is_some(), |row| {
                                row.child(
                                    Button::new("node-install")
                                        .small()
                                        .label("Install Node.js…")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.plan_install(Target::Node, cx);
                                        })),
                                )
                            }),
                    );
                    if catalog.node.manager.is_none() {
                        status = status.child(
                            row()
                                .gap_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(install::node_missing())
                                .child(
                                    Link::new("nodejs-link")
                                        .href("https://nodejs.org")
                                        .child("nodejs.org"),
                                ),
                        );
                    }
                }
                Some(status.into_any_element())
            }
        }
    }

    fn install_area(&self, install: &Install, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let output = |lines: &[String]| {
            div()
                .id("install-output")
                .w_full()
                .max_h(rems(14.))
                .overflow_y_scroll()
                .p_2()
                .rounded(theme.radius)
                .bg(theme.muted)
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .child(lines.join("\n"))
        };
        let machine = (self.machine != machines::LOCAL).then(|| machines::name(&self.machine));
        match install {
            Install::Planning => row()
                .gap_2()
                .text_sm()
                .child(Spinner::new().small())
                .child("Checking what to run…")
                .into_any_element(),
            Install::Unavailable(reason) => col()
                .gap_2()
                .text_sm()
                .child(div().text_color(theme.danger).child(reason.clone()))
                .when(
                    reason.contains("Node.js") && reason.contains("nodejs.org"),
                    |column| {
                        column.child(
                            Link::new("nodejs-link")
                                .href("https://nodejs.org")
                                .child("nodejs.org"),
                        )
                    },
                )
                .child(
                    Button::new("install-close")
                        .small()
                        .ghost()
                        .label("Close")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.install = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
            Install::Confirm(target, steps) => {
                let target = target.clone();
                col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .child(div().text_sm().child(match machine {
                        Some(machine) => format!("Adeline will run this on {machine}:"),
                        None => "Adeline will run this:".to_owned(),
                    }))
                    .child(output(
                        &steps.iter().map(Planned::display).collect::<Vec<_>>(),
                    ))
                    .child(
                        row()
                            .gap_2()
                            .justify_end()
                            .child(
                                Button::new("install-cancel")
                                    .small()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.install = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("install-confirm")
                                    .small()
                                    .primary()
                                    .label(if matches!(target, Target::Node) {
                                        "Install Node.js"
                                    } else {
                                        "Install"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run_install(target.clone(), cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            Install::Running(lines) => col()
                .gap_2()
                .child(
                    row()
                        .gap_2()
                        .text_sm()
                        .child(Spinner::new().small())
                        .child("Installing"),
                )
                .child(output(lines))
                .into_any_element(),
            Install::Failed(target, lines, error) => {
                let target = target.clone();
                col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.danger)
                            .child(format!("The install failed: {error}")),
                    )
                    .child(output(lines))
                    .child(
                        row()
                            .gap_2()
                            .child(
                                Button::new("install-retry")
                                    .small()
                                    .label("Retry")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run_install(target.clone(), cx);
                                    })),
                            )
                            .child(
                                Button::new("install-dismiss")
                                    .small()
                                    .ghost()
                                    .label("Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.install = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        }
    }

    fn probe_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
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

    /// "Additional MCP servers": this agent's own, added to the global ones.
    fn mcp_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity().downgrade();
        let edit_view = view.clone();
        let edit: Rc<dyn Fn(Option<usize>, &mut Window, &mut App)> =
            Rc::new(move |ix, window, cx| {
                let Some(form) = edit_view.upgrade() else {
                    return;
                };
                let server = ix.and_then(|ix| form.read(cx).mcp_servers.get(ix).cloned());
                let saving = edit_view.clone();
                mcp_ui::open_editor(
                    server.as_ref(),
                    Rc::new(move |server, _, cx| {
                        saving
                            .update(cx, |form, cx| {
                                match ix {
                                    Some(ix) if ix < form.mcp_servers.len() => {
                                        form.mcp_servers[ix] = server;
                                    }
                                    _ => form.mcp_servers.push(server),
                                }
                                form.status = None;
                                cx.notify();
                            })
                            .map_err(|error| error.to_string())
                    }),
                    window,
                    cx,
                );
            });
        let remove: Rc<dyn Fn(usize, &mut Window, &mut App)> = Rc::new(move |ix, _, cx| {
            let _ = view.update(cx, |form, cx| {
                if ix < form.mcp_servers.len() {
                    form.mcp_servers.remove(ix);
                }
                cx.notify();
            });
        });
        Form::new()
            .child(
                Field::new()
                    .label("Additional MCP servers")
                    .description(
                        "Sent to this agent at session start, after the MCP servers in Settings.",
                    )
                    .child(mcp_ui::server_list(
                        "agent-mcp",
                        &self.mcp_servers,
                        &edit,
                        &remove,
                        cx,
                    )),
            )
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
            if self.offers(Category::Model) {
                let marker = self
                    .not_offered(Category::Model, cx)
                    .then(|| format!("{} — not offered by the agent", self.original.model));
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
            if self.offers(Category::Effort) {
                let marker = self
                    .not_offered(Category::Effort, cx)
                    .then(|| format!("{} — not offered by the agent", self.original.effort));
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
            // Absent when the agent offers no modes (scope R19).
            if self.offers(Category::Mode) {
                form = form.child(
                    Field::new()
                        .label("Default mode")
                        .description("New chats start in this mode. The agent's modes decide when it asks for permission.")
                        .child(
                            Select::new(&self.mode)
                                .placeholder("The agent's default")
                                .accessibility_label("Default mode"),
                        ),
                );
            }
        }
        body = body.child(form);
        if usable || custom {
            body = body.child(self.mcp_field(cx));
        }
        // The field is absent for agents without a mechanism (scope R29).
        if let Some(id) = id.as_deref()
            && profiles::instructions(id, &self.identity(cx)) != profiles::Instructions::None
        {
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
                                .children(["Append to the agent's guidance", "Overwrite it"])
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
        }
        body
    }
}
