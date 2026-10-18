//! The machine selector in the title bar and its menu of machines and their
//! connection states.
use super::*;

impl Adeline {
    pub(super) fn machine_selector(&self, cx: &Context<Self>) -> Popover {
        let owner = cx.weak_entity();
        let content_owner = owner.clone();
        let checked = machines::checked();
        let label = match checked.as_slice() {
            [one] => machines::name(one),
            several => format!("{} machines", several.len()),
        };
        let attention = client::connections(cx).iter().any(|c| failing(&c.state));
        let trigger = Button::new("machines")
            .ghost()
            .icon(Icon::default().path("devices.svg"))
            .label(label)
            .dropdown_caret(true)
            .accessibility_label("Machines")
            .h_full()
            .rounded_none()
            .px(rems(0.875))
            .when(attention, |button| button.text_color(cx.theme().danger));
        Popover::new("machine-selector")
            .anchor(Anchor::TopRight)
            .trigger(trigger)
            .open(self.menu == Some("machines"))
            .p_0()
            .shadow(views::menu_shadow(cx))
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if *open {
                        app.act(Action::Machines, window, cx);
                    } else {
                        app.close_menu("machines", cx);
                    }
                });
            })
            .content(move |_, _, cx| {
                let owner = content_owner.clone();
                machine_rows(&owner, cx)
            })
    }
}

fn failing(state: &client::State) -> bool {
    !state.usable() && !matches!(state, client::State::Connecting | client::State::Starting)
}

fn machine_rows(owner: &WeakEntity<Adeline>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let checked = machines::checked();
    let mut list = col()
        .id("machine-list")
        .test_support()
        .role(Role::List)
        .aria_label("Machines")
        .w(rems(24.))
        .p_1()
        .gap_1();
    for (ix, machine) in machines::all().into_iter().enumerate() {
        let name = machines::name(&machine);
        let on = checked.contains(&machine);
        let state = client::state(&machine, cx).cloned();
        let status = match &state {
            Some(state) => state.label().to_owned(),
            None if on => "Connecting…".to_owned(),
            None => "Not checked".to_owned(),
        };
        let detail = state.as_ref().and_then(client::State::detail);
        let action = match &state {
            Some(client::State::UpgradeNeeded { .. }) => {
                Some(("Upgrade…", Action::MachineUpgrade(machine.clone())))
            }
            Some(state)
                if failing(state)
                    && !matches!(
                        state,
                        client::State::Waiting | client::State::Unsupported(_)
                    ) =>
            {
                Some(("Retry", Action::MachineRetry(machine.clone())))
            }
            _ => None,
        };
        let toggle_owner = owner.clone();
        let toggle = machine.clone();
        let failed = state
            .as_ref()
            .is_some_and(|s| failing(s) && !matches!(s, client::State::Waiting));
        list = list.child(
            col()
                .id(("machine-row", ix))
                .test_support()
                .role(Role::ListItem)
                .aria_label(format!("{name}, {status}"))
                .px_2()
                .py_1p5()
                .gap_1()
                .rounded(theme.radius)
                .hover(|style| style.bg(theme.accent))
                .child(
                    row()
                        .gap_2()
                        .child(
                            Checkbox::new(("machine-check", ix))
                                .checked(on)
                                .label(name.clone())
                                .accessibility_label(format!("Show {name}"))
                                .on_click(move |_, window, cx| {
                                    let _ = toggle_owner.update(cx, |app, cx| {
                                        app.act(Action::Machine(toggle.clone()), window, cx);
                                    });
                                }),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_xs()
                                .text_color(if failed {
                                    theme.danger
                                } else {
                                    theme.muted_foreground
                                })
                                .child(status),
                        )
                        .children(action.map(|(label, action)| {
                            Button::new(("machine-action", ix))
                                .xsmall()
                                .label(label)
                                .on_click(owner_action(owner, action))
                        })),
                )
                .children(detail.filter(|_| on).map(|detail| {
                    div()
                        .id(("machine-detail", ix))
                        .role(Role::Status)
                        .aria_label(detail.clone())
                        .pl(rems(1.75))
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail)
                })),
        );
    }
    list.child(
        div().border_t_1().border_color(theme.border).pt_1().child(
            Button::new("manage-machines")
                .ghost()
                .small()
                .w_full()
                .icon(Icon::default().path("settings.svg"))
                .label("Manage machines…")
                .on_click(owner_action(owner, Action::ManageMachines)),
        ),
    )
    .into_any_element()
}
