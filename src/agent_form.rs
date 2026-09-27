use super::*;
use agents::{AgentDefinition, EffortParameterName};
use gpui_kit::component::{
    Disableable as _, IndexPath, Sizable as _,
    button::{Button, ButtonVariants},
    form::{Field, Form},
    input::{Input, InputState, Textarea, TextareaState},
    radio::RadioGroup,
    select::{Select, SelectEvent, SelectState},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const FIELDS: [(&str, &str); 5] = [
    ("Name", "Josh"),
    ("Harness", "OMP"),
    ("Driver", "ACP"),
    ("Command", "omp.exe"),
    ("Provider/model", "openai-codex/gpt-6-luna"),
];

pub(super) struct AgentForm {
    pub(super) id: Option<String>,
    pub(super) original: AgentDefinition,
    pub(super) inputs: [Entity<InputState>; 5],
    pub(super) instructions: Entity<TextareaState>,
    arguments: Rc<RefCell<Vec<Entity<InputState>>>>,
    effort: Rc<Cell<Option<usize>>>,
    effort_parameter_name: Entity<SelectState<Vec<SharedString>>>,
    _effort_parameter_subscription: Subscription,
    permission_mode: Rc<Cell<agents::PermissionMode>>,
    pub(super) external_changed: bool,
    pub(super) status: Option<String>,
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

impl AgentForm {
    pub(super) fn new(
        id: Option<String>,
        definition: AgentDefinition,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let values = [
            &definition.name,
            &definition.harness,
            &definition.driver,
            &definition.command,
            &definition.model,
        ];
        let inputs = std::array::from_fn(|i| input(values[i], FIELDS[i].1, window, cx));
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
        let permission_mode = Rc::new(Cell::new(definition.permission_mode));
        let effort_parameter_name = cx.new(|cx| {
            SelectState::new(
                EffortParameterName::ALL
                    .into_iter()
                    .map(|name| name.as_str().into())
                    .collect::<Vec<SharedString>>(),
                EffortParameterName::ALL
                    .iter()
                    .position(|name| *name == definition.effort_parameter_name)
                    .map(|index| IndexPath::default().row(index)),
                window,
                cx,
            )
        });
        let effort_parameter_subscription = cx.subscribe(
            &effort_parameter_name,
            |_, _: &SelectEvent<Vec<SharedString>>, cx| cx.refresh_windows(),
        );
        let effort = Rc::new(Cell::new(
            agents::EFFORTS
                .iter()
                .position(|choice| *choice == definition.effort),
        ));
        Self {
            id,
            original: definition,
            inputs,
            instructions,
            arguments,
            effort,
            effort_parameter_name,
            _effort_parameter_subscription: effort_parameter_subscription,
            permission_mode,
            external_changed: false,
            status: None,
        }
    }

    pub(super) fn values(&self, cx: &App) -> AgentDefinition {
        let value = |i: usize| self.inputs[i].read(cx).text().to_string();
        AgentDefinition {
            name: value(0),
            harness: value(1),
            driver: value(2),
            command: value(3),
            arguments: self
                .arguments
                .borrow()
                .iter()
                .map(|input| input.read(cx).text().to_string())
                .collect(),
            permission_mode: self.permission_mode.get(),
            model: value(4),
            effort: self.effort.get().map_or_else(
                || self.original.effort.clone(),
                |i| agents::EFFORTS[i].into(),
            ),
            effort_parameter_name: self.selected_effort_parameter(cx),
            system_instructions: self.instructions.read(cx).text().to_string(),
        }
    }

    pub(super) fn dirty(&self, cx: &App) -> bool {
        let originals = [
            &self.original.name,
            &self.original.harness,
            &self.original.driver,
            &self.original.command,
            &self.original.model,
        ];
        self.inputs
            .iter()
            .zip(originals)
            .any(|(input, original)| !input.read(cx).text().chars().eq(original.chars()))
            || self
                .effort
                .get()
                .is_some_and(|i| agents::EFFORTS[i] != self.original.effort)
            || !self
                .instructions
                .read(cx)
                .text()
                .chars()
                .eq(self.original.system_instructions.chars())
            || self.permission_mode.get() != self.original.permission_mode
            || self.selected_effort_parameter(cx) != self.original.effort_parameter_name
            || {
                let arguments = self.arguments.borrow();
                arguments.len() != self.original.arguments.len()
                    || arguments
                        .iter()
                        .zip(&self.original.arguments)
                        .any(|(input, original)| {
                            !input.read(cx).text().chars().eq(original.chars())
                        })
            }
    }

    pub(super) fn reload(
        &mut self,
        definition: AgentDefinition,
        window: &mut Window,
        cx: &mut App,
    ) {
        let values = [
            &definition.name,
            &definition.harness,
            &definition.driver,
            &definition.command,
            &definition.model,
        ];
        for (input, value) in self.inputs.iter().zip(values) {
            input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        }
        self.instructions.update(cx, |input, cx| {
            input.set_value(definition.system_instructions.clone(), window, cx);
        });
        *self.arguments.borrow_mut() = definition
            .arguments
            .iter()
            .map(|value| input(value, "Argument", window, cx))
            .collect();
        self.effort.set(
            agents::EFFORTS
                .iter()
                .position(|choice| *choice == definition.effort),
        );
        self.permission_mode.set(definition.permission_mode);
        self.effort_parameter_name.update(cx, |picker, cx| {
            picker.set_selected_value(
                &SharedString::from(definition.effort_parameter_name.as_str()),
                window,
                cx,
            );
        });
        self.original = definition;
        self.external_changed = false;
        self.status = None;
    }

    fn selected_effort_parameter(&self, cx: &App) -> EffortParameterName {
        let picker = self.effort_parameter_name.read(cx);
        EffortParameterName::ALL
            .into_iter()
            .find(|name| {
                picker
                    .selected_value()
                    .is_some_and(|value| value.as_ref() == name.as_str())
            })
            .unwrap_or(self.original.effort_parameter_name)
    }

    pub(super) fn fields(&self) -> Div {
        let selected_effort = self.effort.get();
        let selected_permission = match self.permission_mode.get() {
            agents::PermissionMode::Ask => Some(0),
            agents::PermissionMode::AllowEverything => Some(1),
        };
        let effort = self.effort.clone();
        let permission = self.permission_mode.clone();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                Form::new()
                    .child(
                        Field::new()
                            .label(FIELDS[0].0)
                            .child(Input::new(&self.inputs[0]).aria_label(FIELDS[0].0)),
                    )
                    .child(
                        Field::new()
                            .label(FIELDS[1].0)
                            .child(Input::new(&self.inputs[1]).aria_label(FIELDS[1].0)),
                    )
                    .child(
                        Field::new()
                            .label(FIELDS[2].0)
                            .child(Input::new(&self.inputs[2]).aria_label(FIELDS[2].0)),
                    )
                    .child(
                        Field::new()
                            .label(FIELDS[3].0)
                            .child(Input::new(&self.inputs[3]).aria_label(FIELDS[3].0)),
                    ),
            )
            .child(self.argument_fields())
            .child(
                Form::new()
                    .child(
                        Field::new()
                            .label(FIELDS[4].0)
                            .child(Input::new(&self.inputs[4]).aria_label(FIELDS[4].0)),
                    )
                    .child(
                        Field::new().label("Effort parameter name").child(
                            Select::new(&self.effort_parameter_name)
                                .accessibility_label("Effort parameter name"),
                        ),
                    )
                    .child(
                        Field::new().label("Effort").child(
                            RadioGroup::horizontal("agent-effort")
                                .children(agents::EFFORTS)
                                .selected_index(selected_effort)
                                .on_change(move |index, _, cx| {
                                    effort.set(Some(*index));
                                    cx.refresh_windows();
                                }),
                        ),
                    )
                    .child(
                        Field::new().label("Default permission mode").child(
                            RadioGroup::horizontal("agent-permission")
                                .children(["Ask", "Allow everything"])
                                .selected_index(selected_permission)
                                .on_change(move |index, _, cx| {
                                    permission.set(if *index == 0 {
                                        agents::PermissionMode::Ask
                                    } else {
                                        agents::PermissionMode::AllowEverything
                                    });
                                    cx.refresh_windows();
                                }),
                        ),
                    )
                    .child(
                        Field::new().label("System instructions (optional)").child(
                            Textarea::new(&self.instructions)
                                .aria_label("System instructions")
                                .h_32(),
                        ),
                    ),
            )
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
}
