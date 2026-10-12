//! Installing, updating and logging in a supported agent from under the harness
//! picker, and the status line that offers it.
use super::*;

impl AgentForm {
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

    pub(super) fn run_login(
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

    pub(super) fn harness_status(&self, id: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
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
}
