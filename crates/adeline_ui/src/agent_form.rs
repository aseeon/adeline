//! The Add Agent and Edit Agent form. Model, effort and mode come from the
//! agent itself through a background probe, and the last probe's lists stay
//! while a model change re-probes. Under the harness picker it installs,
//! updates and logs in supported agents.
use super::*;
use crate::Harnesses;
use crate::views::text_input;
use adeline_core::conversation::{AuthMethod, Category, McpServer};
use adeline_core::install::{Planned, Target};
use adeline_core::protocol::{ProbeError, Probed};
use agents::{AgentDefinition, InstructionsMode};
use gpui_kit::component::{
    Disableable as _, Sizable as _,
    form::{Field, Form},
    input::Textarea,
    link::Link,
    radio::RadioGroup,
    select::{SearchableVec, Select, SelectEvent, SelectGroup, SelectItem, SelectState},
    spinner::Spinner,
    text::TextView,
};
use profiles::CUSTOM;
use std::rc::Rc;

mod installer;
mod probe;

#[derive(Clone)]
struct HarnessItem {
    id: SharedString,
    name: SharedString,
    installed: Option<bool>,
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

fn found_on(path: &str, cx: &App) -> Div {
    div()
        .min_w_0()
        .truncate()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(format!("Found on {path}"))
}

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
    Idle,
    Loading,
    Ready(Probed),
    Failed(ProbeError),
}

enum Install {
    Planning,
    Confirm(Target, Vec<Planned>),
    Running(Vec<String>),
    Failed(Target, Vec<String>, String),
    Unavailable(String),
}

pub(super) struct AgentForm {
    pub(super) machine: String,
    pub(super) id: Option<String>,
    pub(super) original: AgentDefinition,
    pub(super) external_changed: bool,
    pub(super) status: Option<String>,
    name: Entity<InputState>,
    harness: Entity<HarnessPicker>,
    command: Entity<InputState>,
    arguments: Vec<Entity<InputState>>,
    model: Entity<ModelPicker>,
    effort: Entity<ChoicePicker>,
    mode: Entity<ChoicePicker>,
    model_text: Entity<InputState>,
    effort_text: Entity<InputState>,
    instructions: Entity<TextareaState>,
    instructions_mode: InstructionsMode,
    mcp_servers: Vec<McpServer>,
    probing: Probing,
    offered: Option<Probed>,
    probe: Option<client::Probe>,
    probe_generation: u64,
    install: Option<Install>,
    login: Option<String>,
    _subscriptions: Vec<Subscription>,
}

fn harness_items(cx: &App) -> SearchableVec<HarnessItem> {
    let catalog = cx.global::<Harnesses>();
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
        let name = text_input(&definition.name, "Name", window, cx);
        let harness = cx.new(|cx| {
            let mut picker = SelectState::new(harness_items(cx), None, window, cx).searchable(true);
            picker.set_selected_value(&definition.harness.clone().into(), window, cx);
            picker
        });
        let command = text_input(&definition.command, "omp.exe", window, cx);
        let model = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let effort = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let mode = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let model_text = text_input(&definition.model, "Model ID", window, cx);
        let effort_text = text_input(&definition.effort, "Effort", window, cx);
        let instructions = cx.new(|cx| {
            let mut state =
                TextareaState::new(window, cx).placeholder("System instructions (optional)");
            state.set_value(definition.system_instructions.clone(), window, cx);
            state
        });
        let arguments = definition
            .arguments
            .iter()
            .map(|value| text_input(value, "Argument", window, cx))
            .collect();
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
            cx.observe_global_in::<Harnesses>(window, |this, window, cx| {
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
            .iter()
            .map(|input| input.read(cx).value().to_string())
            .collect()
    }

    fn agent_label(&self, cx: &App) -> String {
        let id = self.harness_id(cx).unwrap_or_default();
        profiles::find(&id, &self.identity(cx)).map_or_else(
            || cx.global::<Harnesses>().label(&id, &self.identity(cx)),
            |profile| profile.name.to_owned(),
        )
    }

    fn offers(&self, category: Category) -> bool {
        self.offered
            .as_ref()
            .is_some_and(|probed| conversation::option(&probed.options, category).is_some())
    }

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

    pub(super) fn blocked(&self, cx: &App) -> bool {
        let Some(id) = self.harness_id(cx) else {
            return true;
        };
        cx.global::<Harnesses>().is_installed(&id) != Some(true)
            || matches!(self.probing, Probing::Loading)
    }

    pub(super) fn check(&self, cx: &App) -> Result<()> {
        if self.name(cx).trim().is_empty() {
            anyhow::bail!("Name is required.");
        }
        let Some(id) = self.harness_id(cx) else {
            anyhow::bail!("Choose a harness.");
        };
        if cx.global::<Harnesses>().is_installed(&id) != Some(true) {
            anyhow::bail!("This harness is not installed.");
        }
        match &self.probing {
            Probing::Loading => anyhow::bail!("Wait for the agent's options to load."),
            Probing::Idle if id == CUSTOM => anyhow::bail!("Command is required."),
            Probing::Failed(_) => return Ok(()),
            _ => {}
        }
        for (category, label, saved) in [
            (Category::Model, "model", &self.original.model),
            (Category::Effort, "effort", &self.original.effort),
        ] {
            if self.offers(category) && self.chosen(category, cx).is_empty() {
                if self.not_offered(category, cx) {
                    anyhow::bail!(
                        "Saved {label} {saved} is not offered by the agent. Choose another."
                    );
                }
                anyhow::bail!("Choose a {label}.");
            }
        }
        Ok(())
    }

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
        self.arguments = definition
            .arguments
            .iter()
            .map(|value| text_input(value, "Argument", window, cx))
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

    fn argument_fields(&self, cx: &mut Context<Self>) -> Div {
        let mut fields = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child("Arguments (optional, one literal value per entry)");
        let count = self.arguments.len();
        for (i, input) in self.arguments.iter().enumerate() {
            let key = input.entity_id();
            let mut row = div().w_full().flex().items_center().gap_2().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(input).aria_label(format!("Argument {}", i + 1))),
            );
            for (action, label) in ["Up", "Down", "Remove"].into_iter().enumerate() {
                row = row.child(
                    Button::new(format!("argument-{key:?}-{action}"))
                        .small()
                        .ghost()
                        .label(label)
                        .accessibility_label(format!("{label} argument {}", i + 1))
                        .disabled((action == 0 && i == 0) || (action == 1 && i + 1 == count))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let inputs = &mut this.arguments;
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
                            cx.notify();
                        })),
                );
            }
            fields = fields.child(row);
        }
        fields.child(
            Button::new("agent-add-argument")
                .small()
                .label("Add argument")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.arguments.push(text_input("", "Argument", window, cx));
                    cx.notify();
                })),
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
                        saving.update(cx, |form, cx| {
                            mcp_ui::upsert(&mut form.mcp_servers, ix, server);
                            form.status = None;
                            cx.notify();
                        })
                    }),
                    window,
                    cx,
                );
            });
        let remove: Rc<dyn Fn(usize, &mut Window, &mut App)> = Rc::new(move |ix, _, cx| {
            let _ = view.update(cx, |form, cx| {
                mcp_ui::remove(&mut form.mcp_servers, ix);
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
            .is_some_and(|id| cx.global::<Harnesses>().is_installed(id) == Some(true));
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
                .child(self.argument_fields(cx));
            let identity = self.identity(cx);
            if !identity.is_empty() {
                body = body.child(muted(cx.global::<Harnesses>().label(CUSTOM, &identity), cx));
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
