//! What the window shows when an engine isn't usable: the engine and remote
//! machine screens, and the banner over a read-only machine.
use super::*;

impl Adeline {
    pub(crate) fn engine_screen(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let checked = machines::checked();
        // With several machines, each one's state shows in the machine selector.
        let [machine] = checked.as_slice() else {
            return None;
        };
        let connection = client::get(machine, cx)?;
        if machine != machines::LOCAL {
            return self.remote_screen(machine, connection, cx);
        }
        let (title, detail, buttons): (String, Option<String>, Vec<Button>) =
            match &connection.state {
                client::State::Connecting | client::State::Starting if !connection.loaded => {
                    ("Starting conversation engine…".into(), None, Vec::new())
                }
                client::State::Unavailable(reason) => (
                    "Conversation engine unavailable".into(),
                    Some(format!("{reason}\nLogs: {}", ipc::log_path())),
                    vec![
                        self.button("engine-retry", "Retry", Action::EngineRetry, cx)
                            .primary(),
                    ],
                ),
                client::State::Mismatch(status) => {
                    let count = status.conversations.len();
                    let mut buttons = Vec::new();
                    if status.daemon && count == 0 {
                        buttons.push(
                            self.button(
                                "engine-restart",
                                "Restart engine",
                                Action::EngineStopOld,
                                cx,
                            )
                            .primary(),
                        );
                    } else {
                        buttons.push(self.button("engine-wait", "Wait", Action::EngineWait, cx));
                        buttons.push(
                            self.button(
                                "engine-stop-old",
                                "Stop them now",
                                Action::EngineStopOld,
                                cx,
                            )
                            .danger(),
                        );
                    }
                    (
                        format!(
                            "An older engine is finishing {}",
                            chat::count(count, "conversation")
                        ),
                        Some(format!(
                            "Version {} is still running. This Adeline needs a newer engine.",
                            status.version
                        )),
                        buttons,
                    )
                }
                client::State::Waiting => (
                    "Waiting for the older engine to finish…".into(),
                    Some("Adeline connects as soon as it exits.".into()),
                    vec![
                        self.button(
                            "engine-stop-old",
                            "Stop them now",
                            Action::EngineStopOld,
                            cx,
                        )
                        .danger(),
                    ],
                ),
                _ => return None,
            };
        Some(Self::state_screen(title, detail, buttons, cx))
    }

    fn remote_screen(
        &self,
        machine: &str,
        connection: &client::Connection,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let name = machines::name(machine);
        let retry = || {
            vec![
                self.button(
                    "machine-retry",
                    "Retry",
                    Action::MachineRetry(machine.to_owned()),
                    cx,
                )
                .primary(),
            ]
        };
        let (title, buttons) = match &connection.state {
            client::State::Connecting | client::State::Starting if !connection.loaded => {
                (format!("Connecting to {name}…"), Vec::new())
            }
            client::State::Disconnected(_) if !connection.loaded => {
                (format!("Can't reach {name}"), retry())
            }
            client::State::SignInFailed(_) => (format!("Sign-in to {name} failed"), retry()),
            client::State::Unavailable(_) if !connection.loaded => {
                (format!("{name} is unavailable"), retry())
            }
            client::State::UpgradeNeeded { .. } if !connection.loaded => (
                format!("{name} runs an older Adeline"),
                vec![
                    self.button(
                        "machine-upgrade",
                        "Upgrade now",
                        Action::MachineUpgrade(machine.to_owned()),
                        cx,
                    )
                    .danger(),
                ],
            ),
            client::State::LocalUpdateNeeded(_) if !connection.loaded => {
                ("Update Adeline on this computer".into(), retry())
            }
            client::State::Unsupported(_) => (format!("{name} isn't supported"), Vec::new()),
            _ => return None,
        };
        Some(Self::state_screen(
            title,
            connection.state.detail(),
            buttons,
            cx,
        ))
    }

    fn state_screen(
        title: String,
        detail: Option<String>,
        buttons: Vec<Button>,
        cx: &Context<Self>,
    ) -> AnyElement {
        col()
            .id("engine-state")
            .role(Role::Status)
            .aria_label(title.clone())
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_6()
            .child(div().text_lg().child(title))
            .children(detail.map(|detail| {
                div()
                    .max_w(px(560.))
                    .text_sm()
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(detail)
            }))
            .child(row().gap_2().children(buttons))
            .into_any_element()
    }

    pub(crate) fn engine_banner(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let machine = self.current_machine();
        let connection = client::get(&machine, cx)?;
        let name = machines::name(&machine);
        let retry = || {
            Some(self.button(
                "machine-retry",
                "Retry",
                Action::MachineRetry(machine.clone()),
                cx,
            ))
        };
        let (message, button) = match &connection.state {
            client::State::Stopped { unexpected } => (
                self.about_machine(
                    &machine,
                    if *unexpected {
                        "Conversation engine stopped unexpectedly"
                    } else {
                        "Conversation engine stopped"
                    },
                ),
                Some(self.button("start-engine", "Start engine", Action::StartEngine, cx)),
            ),
            client::State::Connecting | client::State::Starting if connection.loaded => {
                if machine == machines::LOCAL {
                    (
                        self.about_machine(&machine, "Starting conversation engine…"),
                        None,
                    )
                } else {
                    (
                        format!("Reconnecting to {name}… Showing what was last received."),
                        None,
                    )
                }
            }
            state if machine != machines::LOCAL && !state.usable() && connection.loaded => (
                format!(
                    "{}. Showing what was last received; changes wait until it reconnects.",
                    match state.label() {
                        "Disconnected" => format!("{name} is disconnected"),
                        label => format!("{name} isn't connected: {}", label.to_lowercase()),
                    }
                ),
                if matches!(state, client::State::UpgradeNeeded { .. }) {
                    Some(self.button(
                        "machine-upgrade",
                        "Upgrade…",
                        Action::MachineUpgrade(machine.clone()),
                        cx,
                    ))
                } else {
                    retry()
                },
            ),
            _ => return None,
        };
        Some(
            row()
                .id("engine-banner")
                .role(Role::Status)
                .aria_label(message.clone())
                .w_full()
                .gap_3()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().group_box)
                .text_sm()
                .child(div().flex_1().child(message))
                .children(button)
                .into_any_element(),
        )
    }
}
