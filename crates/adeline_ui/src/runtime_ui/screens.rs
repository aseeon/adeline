//! What the window shows when an engine isn't usable: the engine and remote
//! machine screens, and the banner over a read-only machine. With several
//! machines checked, a local engine that would get a screen gets a banner.
use super::*;

impl Adeline {
    pub(crate) fn engine_screen(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let checked = machines::checked();
        let [machine] = checked.as_slice() else {
            return None;
        };
        let connection = client::get(machine, cx)?;
        if machine != machines::LOCAL {
            return self.remote_screen(machine, connection, cx);
        }
        let (title, detail, buttons) = match &connection.state {
            client::State::Connecting { .. } if !connection.loaded() => {
                ("Starting conversation engine…".into(), None, Vec::new())
            }
            _ => self.local_trouble(connection, cx)?,
        };
        Some(Self::state_screen(title, detail, buttons, cx))
    }

    fn local_trouble(
        &self,
        connection: &client::Connection,
        cx: &Context<Self>,
    ) -> Option<(String, Option<String>, Vec<Button>)> {
        let stop_old = || {
            self.button(
                "engine-stop-old",
                "Stop them now",
                Action::EngineStopOld,
                cx,
            )
            .danger()
        };
        Some(match &connection.state {
            client::State::Unavailable(reason) => (
                "Conversation engine unavailable".into(),
                Some(format!("{reason}\nLogs: {}", ipc::log_path())),
                vec![
                    self.button("engine-retry", "Retry", Action::EngineRetry, cx)
                        .primary(),
                ],
            ),
            client::State::Mismatch { status, .. } => {
                let count = status.conversations.len();
                let buttons = if status.daemon && count == 0 {
                    vec![
                        self.button(
                            "engine-restart",
                            "Restart engine",
                            Action::EngineStopOld,
                            cx,
                        )
                        .primary(),
                    ]
                } else {
                    vec![
                        self.button("engine-wait", "Wait", Action::EngineWait, cx),
                        stop_old(),
                    ]
                };
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
            client::State::Waiting(_) => (
                "Waiting for the older engine to finish…".into(),
                Some("Adeline connects as soon as it exits.".into()),
                vec![stop_old()],
            ),
            _ => return None,
        })
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
        let loaded = connection.loaded();
        let (title, buttons) = match &connection.state {
            client::State::Connecting { .. } if !loaded => {
                (format!("Connecting to {name}…"), Vec::new())
            }
            client::State::Disconnected(_) if !loaded => (format!("Can't reach {name}"), retry()),
            client::State::SignInFailed(_) => (format!("Sign-in to {name} failed"), retry()),
            client::State::Unavailable(_) if !loaded => (format!("{name} is unavailable"), retry()),
            client::State::UpgradeNeeded { .. } if !loaded => (
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
            client::State::LocalUpdateNeeded(_) if !loaded => {
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
            .test_support()
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
        let local = client::get(machines::LOCAL, cx)
            .filter(|_| machines::checked().len() > 1)
            .and_then(|local| self.local_trouble(local, cx))
            .map(|(title, _, buttons)| (self.about_machine(machines::LOCAL, &title), buttons));
        let (message, buttons) = match local {
            Some(banner) => banner,
            None => self.machine_banner(cx)?,
        };
        Some(
            row()
                .id("engine-banner")
                .test_support()
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
                .children(buttons)
                .into_any_element(),
        )
    }

    fn machine_banner(&self, cx: &Context<Self>) -> Option<(String, Vec<Button>)> {
        let machine = self.current_machine();
        let connection = client::get(&machine, cx)?;
        let name = machines::name(&machine);
        Some(match &connection.state {
            client::State::Stopped { unexpected } => (
                self.about_machine(
                    &machine,
                    if *unexpected {
                        "Conversation engine stopped unexpectedly"
                    } else {
                        "Conversation engine stopped"
                    },
                ),
                vec![self.button("start-engine", "Start engine", Action::StartEngine, cx)],
            ),
            client::State::Connecting { .. } if connection.loaded() => {
                if machine == machines::LOCAL {
                    (
                        self.about_machine(&machine, "Starting conversation engine…"),
                        Vec::new(),
                    )
                } else {
                    (
                        format!("Reconnecting to {name}… Showing what was last received."),
                        Vec::new(),
                    )
                }
            }
            state if machine != machines::LOCAL && !state.usable() && connection.loaded() => (
                format!(
                    "{}. Showing what was last received; changes can't be sent until it reconnects.",
                    match state.label() {
                        "Disconnected" => format!("{name} is disconnected"),
                        label => format!("{name} isn't connected: {}", label.to_lowercase()),
                    }
                ),
                vec![if matches!(state, client::State::UpgradeNeeded { .. }) {
                    self.button(
                        "machine-upgrade",
                        "Upgrade…",
                        Action::MachineUpgrade(machine.clone()),
                        cx,
                    )
                } else {
                    self.button(
                        "machine-retry",
                        "Retry",
                        Action::MachineRetry(machine.clone()),
                        cx,
                    )
                }],
            ),
            _ => return None,
        })
    }
}
