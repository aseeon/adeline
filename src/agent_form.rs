use super::*;
use agents::AgentDefinition;

const FIELDS: [(&str, &str); 7] = [
    ("Name", "Josh"),
    ("Harness", "OMP"),
    ("Driver", "ACP"),
    ("Startup command", "omp.exe acp"),
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
        Self {
            id,
            original: definition,
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
        }
        fields
    }
}
