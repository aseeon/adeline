use super::*;
use agents::AgentDefinition;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const FIELDS: [(&str, &str); 7] = [
    ("Name", "Josh"),
    ("Harness", "OMP"),
    ("Driver", "ACP"),
    ("Command", "omp.exe"),
    ("Provider/model", "openai-codex/gpt-6-luna"),
    ("Effort", "Low, Medium, High, Extra High or Max"),
    (
        "System instructions (optional)",
        "You are a helpful coding assistant.",
    ),
];

pub(super) struct AgentForm {
    pub(super) id: Option<String>,
    pub(super) original: AgentDefinition,
    pub(super) inputs: [Entity<TextInput>; 7],
    arguments: Rc<RefCell<Vec<Entity<TextInput>>>>,
    permission_mode: Rc<Cell<agents::PermissionMode>>,
    pub(super) external_changed: bool,
    pub(super) status: Option<String>,
}

impl AgentForm {
    pub(super) fn new(id: Option<String>, definition: AgentDefinition, cx: &mut App) -> Self {
        let values = [
            &definition.name,
            &definition.harness,
            &definition.driver,
            &definition.command,
            &definition.model,
            &definition.effort,
            &definition.system_instructions,
        ];
        let inputs = std::array::from_fn(|i| {
            cx.new(|cx| {
                let mut input = TextInput::new(FIELDS[i].1, cx);
                input.set(values[i].clone(), cx);
                input
            })
        });
        let arguments = Rc::new(RefCell::new(
            definition
                .arguments
                .iter()
                .map(|value| {
                    cx.new(|cx| {
                        let mut input = TextInput::new("Argument", cx);
                        input.set(value.clone(), cx);
                        input
                    })
                })
                .collect(),
        ));
        let permission_mode = Rc::new(Cell::new(definition.permission_mode));
        Self {
            id,
            original: definition,
            arguments,
            permission_mode,
            inputs,
            external_changed: false,
            status: None,
        }
    }

    pub(super) fn values(&self, cx: &App) -> AgentDefinition {
        let value = |i: usize| self.inputs[i].read(cx).content.to_string();
        AgentDefinition {
            name: value(0),
            harness: value(1),
            driver: value(2),
            command: value(3),
            arguments: self
                .arguments
                .borrow()
                .iter()
                .map(|input| input.read(cx).content.to_string())
                .collect(),
            permission_mode: self.permission_mode.get(),
            model: value(4),
            effort: value(5),
            system_instructions: value(6),
        }
    }

    pub(super) fn dirty(&self, cx: &App) -> bool {
        let originals = [
            &self.original.name,
            &self.original.harness,
            &self.original.driver,
            &self.original.command,
            &self.original.model,
            &self.original.effort,
            &self.original.system_instructions,
        ];
        self.inputs
            .iter()
            .zip(originals)
            .any(|(input, original)| input.read(cx).content.as_ref() != original.as_str())
            || self.permission_mode.get() != self.original.permission_mode
            || {
                let arguments = self.arguments.borrow();
                arguments.len() != self.original.arguments.len()
                    || arguments
                        .iter()
                        .zip(&self.original.arguments)
                        .any(|(input, original)| {
                            input.read(cx).content.as_ref() != original.as_str()
                        })
            }
    }

    pub(super) fn reload(&mut self, definition: AgentDefinition, cx: &mut App) {
        let values = [
            &definition.name,
            &definition.harness,
            &definition.driver,
            &definition.command,
            &definition.model,
            &definition.effort,
            &definition.system_instructions,
        ];
        for (input, value) in self.inputs.iter().zip(values) {
            input.update(cx, |input, cx| input.set(value.clone(), cx));
        }
        *self.arguments.borrow_mut() = definition
            .arguments
            .iter()
            .map(|value| {
                cx.new(|cx| {
                    let mut input = TextInput::new("Argument", cx);
                    input.set(value.clone(), cx);
                    input
                })
            })
            .collect();
        self.permission_mode.set(definition.permission_mode);
        self.original = definition;
        self.external_changed = false;
        self.status = None;
    }

    pub(super) fn fields(&self, cx: &App) -> Div {
        let mut fields = col().w_full().gap_4();
        for (i, (label, _)) in FIELDS.into_iter().enumerate() {
            let mut field = col().gap_2().child(text(label, 13., theme::foreground()));
            if i == 5 {
                let selected = &self.inputs[5].read(cx).content;
                let mut choices = row().gap_2().flex_wrap();
                for (index, &effort) in agents::EFFORTS.iter().enumerate() {
                    let input = self.inputs[5].clone();
                    choices = choices.child(
                        row()
                            .id(("agent-effort", index))
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .px_3()
                            .py_2()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .bg(rgb(if selected.as_ref() == effort {
                                theme::primary()
                            } else {
                                theme::secondary()
                            }))
                            .focus(|s| s.border_color(rgb(theme::ring())))
                            .child(text(
                                effort,
                                13.,
                                if selected.as_ref() == effort {
                                    theme::primary_foreground()
                                } else {
                                    theme::secondary_foreground()
                                },
                            ))
                            .on_click(move |_, _, cx| {
                                input.update(cx, |input, cx| input.set(effort, cx));
                            }),
                    );
                }
                field = field.child(choices);
            } else {
                field = field.child(
                    div()
                        .w_full()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .bg(rgb(theme::input()))
                        .child(self.inputs[i].clone()),
                );
            }
            fields = fields.child(field);
            if i == 3 {
                fields = fields.child(self.argument_fields());
            }
            if i == 5 {
                fields = fields.child(self.permission_fields());
            }
        }
        fields
    }

    fn argument_fields(&self) -> Div {
        let mut fields = col().w_full().gap_2().child(text(
            "Arguments (optional, one literal value per entry)",
            13.,
            theme::foreground(),
        ));
        for (i, input) in self.arguments.borrow().iter().enumerate() {
            let mut row = row().gap_2().child(
                div()
                    .flex_1()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .bg(rgb(theme::input()))
                    .child(input.clone()),
            );
            for (action, label) in ["Up", "Down", "Remove"].into_iter().enumerate() {
                if (action == 0 && i == 0)
                    || (action == 1 && i + 1 == self.arguments.borrow().len())
                {
                    continue;
                }
                let arguments = self.arguments.clone();
                row = row.child(
                    div()
                        .id(("agent-argument", i * 3 + action))
                        .focusable()
                        .tab_stop(true)
                        .cursor_pointer()
                        .px_2()
                        .py_2()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .focus(|s| s.border_color(rgb(theme::ring())))
                        .child(text(label, 12., theme::foreground()))
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
            div()
                .id("agent-add-argument")
                .focusable()
                .tab_stop(true)
                .cursor_pointer()
                .px_3()
                .py_2()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(theme::border()))
                .focus(|s| s.border_color(rgb(theme::ring())))
                .child(text("Add argument", 13., theme::foreground()))
                .on_click(move |_, _, cx| {
                    arguments
                        .borrow_mut()
                        .push(cx.new(|cx| TextInput::new("Argument", cx)));
                    cx.refresh_windows();
                }),
        )
    }

    fn permission_fields(&self) -> Div {
        let fields = col()
            .gap_2()
            .child(text("Default permission mode", 13., theme::foreground()));
        let mut choices = row().gap_2().flex_wrap();
        for (i, (mode, label)) in [
            (agents::PermissionMode::Ask, "Ask"),
            (agents::PermissionMode::AllowEverything, "Allow everything"),
        ]
        .into_iter()
        .enumerate()
        {
            let selected = self.permission_mode.get() == mode;
            let permission_mode = self.permission_mode.clone();
            choices = choices.child(
                row()
                    .id(("agent-default-permissions", i))
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .px_3()
                    .py_2()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .bg(rgb(if selected {
                        theme::primary()
                    } else {
                        theme::secondary()
                    }))
                    .focus(|s| s.border_color(rgb(theme::ring())))
                    .child(text(
                        label,
                        13.,
                        if selected {
                            theme::primary_foreground()
                        } else {
                            theme::secondary_foreground()
                        },
                    ))
                    .on_click(move |_, _, cx| {
                        permission_mode.set(mode);
                        cx.refresh_windows();
                    }),
            );
        }
        fields.child(choices)
    }

    pub(super) fn argument_inputs(&self) -> Vec<Entity<TextInput>> {
        self.arguments.borrow().clone()
    }
}
