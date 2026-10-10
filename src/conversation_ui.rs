//! The conversation's live surfaces: the activity row and silence notice,
//! queued messages, the permission and login cards, a crashed agent's output,
//! attachment chips, the `/` command list, the TODO list in the chat header,
//! thoughts in replies and the ACP traffic tab.
use super::*;
use crate::conversation::{
    Attachment, AuthMethod, Direction, PermissionKind, StepStatus, TrafficNote,
};
use crate::protocol::Live;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, PopupMenuItem},
    popover::Popover,
    scroll::Scrollbar,
    spinner::Spinner,
    tab::{Tab, TabBar},
};
use std::sync::Arc;

/// `320 KB`, `1.2 MB`.
pub(super) fn file_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    } else {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    }
}

/// A composer shortcut as the tooltips show it: `Ctrl+Shift+M`, `⌘⇧M` on macOS.
pub(super) fn shortcut(keys: &str) -> String {
    if cfg!(target_os = "macos") {
        // The option menus' Alt shortcuts are Cmd+Shift on macOS (bind_keys).
        keys.replace("Ctrl+", "Cmd+").replace("Alt+", "Cmd+Shift+")
    } else {
        keys.to_owned()
    }
}

impl Adeline {
    /// What the agent is doing right now, from real signals (scope R32).
    fn activity_label(&self, thread: &Thread, live: &Live) -> Option<String> {
        if !live.permission.is_empty() {
            return Some("Waiting for your permission".into());
        }
        let turn = thread.messages.iter().rposition(|m| m.role == "user");
        if let Some(tool) =
            turn.and_then(|turn| thread.turn_tools(turn).filter(|t| t.running).last())
        {
            // The kind, not the title: titles can be whole command lines.
            use conversation::ToolKind;
            let label = match tool.tool {
                ToolKind::Think => "Thinking",
                ToolKind::Read | ToolKind::Search | ToolKind::Fetch => "Reading",
                ToolKind::Edit | ToolKind::Delete | ToolKind::Move => "Writing",
                ToolKind::Execute => "Running commands",
                ToolKind::SwitchMode | ToolKind::Other => "Using tools",
            };
            return Some(format!("{label}…"));
        }
        if live.quiet_since.is_some() {
            return Some("Quiet".into());
        }
        live.progress.clone()
    }

    /// Minutes the agent has been silent with nothing open, once past the
    /// configured notice time (scope R34).
    fn silent_minutes(&self, thread: &Thread, live: &Live) -> Option<u64> {
        let limit = u64::from(config::with(|s| s.modes.chats.silence_notice_minutes));
        let since = live.quiet_since?;
        let open_tool = thread
            .activity
            .iter()
            .any(|a| a.running && a.kind.starts_with("tool:"));
        if limit == 0 || !live.processing || !live.permission.is_empty() || open_tool {
            return None;
        }
        let minutes = recency::now_ms().saturating_sub(since) / 60_000;
        (minutes >= limit).then_some(minutes)
    }

    /// The footer under a chat: the running turn's steps, its activity, the
    /// queue, the permission and login cards, errors and recovery buttons.
    pub(super) fn runtime_footer(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[index];
        let mut content = col().w_full().gap_3().pb_4();
        let empty = Live::default();
        let live = self.runtime.conversations.get(&thread.id).unwrap_or(&empty);
        let turn = thread.messages.iter().rposition(|m| m.role == "user");
        // The running turn's steps show live; finished turns fold them into
        // the summary under their reply.
        if live.processing
            && let Some(turn) = turn
            && !config::current().modes.chats.hide_tool_calls
        {
            let steps: Vec<_> = thread.turn_tools(turn).collect();
            if !steps.is_empty() {
                content = content.child(chat_render::tool_steps(steps, cx));
            }
        }
        // Only this turn's errors: a later turn means the agent recovered.
        // Errors without a turn (an interrupted engine, storage) are current.
        let dismissed = |message: &str| {
            self.dismissed_errors
                .contains(&(thread.id.clone(), turn, message.to_owned()))
        };
        for activity in thread.activity.iter().filter(|a| {
            a.kind == "error" && (a.turn.is_none() || a.turn == turn) && !dismissed(&a.title)
        }) {
            content = content.child(self.error_line(&activity.title, cx));
        }
        let writing = live
            .assistant
            .and_then(|i| thread.messages.get(i))
            .is_some_and(|m| {
                !m.text.is_empty()
                    || (config::with(|s| s.modes.chats.show_thinking) && !m.thought.is_empty())
            });
        let label = self.activity_label(thread, live);
        if live.processing && live.storage_failed {
            content = content.child(text(
                "Stopping the turn before storage can be retried…",
                13.,
                theme::muted_foreground(),
            ));
        } else if let Some(minutes) = self.silent_minutes(thread, live) {
            content = content.child(self.hang_notice(minutes, cx));
        } else if live.processing && (!writing || label.is_some()) {
            content = content.child(
                chat_render::agent_header(
                    self.agent_icon(&thread.provider, cx),
                    thread.provider.clone(),
                    cx,
                )
                .child(chat_render::thinking_label(label, cx)),
            );
        } else if !live.processing
            && turn.is_some_and(|turn| thread.turn_tools(turn).any(|t| t.running))
        {
            content = content.child(
                row()
                    .gap_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(Spinner::new().xsmall())
                    .child("Background tasks running"),
            );
        }
        if !live.queued.is_empty() {
            content = content.child(self.queued_entries(live, cx));
        }
        if let Some(request) = live.permission.first() {
            content = content.child(self.permission_card(request, cx));
        }
        if live.auth_required {
            let name = live
                .execution
                .as_ref()
                .map_or_else(|| thread.provider.clone(), |e| e.name.clone());
            content = content.child(self.login_card(&thread.id, &name, &live.features.auth, cx));
        }
        if let Some(error) = live.error.as_ref().filter(|error| {
            !dismissed(error)
                && !thread
                    .activity
                    .iter()
                    .any(|a| a.kind == "error" && a.title == **error && a.turn == turn)
        }) {
            content = content.child(self.error_line(error, cx));
        }
        if !live.stderr.is_empty() {
            content = content.child(self.crash_output(&thread.id, &live.stderr, cx));
        }
        if live.storage_failed {
            content = content.child(self.button(
                "retry-storage",
                "Retry storage",
                Action::RetryStorage,
                cx,
            ));
        } else if live.replacement && !live.shutting_down {
            content = content.child(self.button(
                "replace-session",
                "Start new session with saved context",
                Action::ReplaceSession,
                cx,
            ));
        } else if live.error.is_some()
            && !live.auth_required
            && !live.shutting_down
            && !live.processing
            && !live.recovering_storage
            && !live.last_prompt.is_empty()
            && !matches!(thread.status.as_str(), "completed" | "archived")
        {
            content = content.child(self.button("retry-prompt", "Retry", Action::RetryPrompt, cx));
        }
        if live.shutting_down {
            content = content.child(text(
                "Waiting for the agent to stop…",
                13.,
                theme::muted_foreground(),
            ));
        }
        if live.shutdown_stuck {
            content = content.child(self.button("force-stop", "Force Stop", Action::ForceStop, cx));
        }
        content.into_any_element()
    }

    /// An error under the chat, with a button that hides it.
    fn error_line(&self, message: &str, cx: &Context<Self>) -> Div {
        let message = message.to_owned();
        row()
            .w_full()
            .items_start()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(text(message.clone(), 13., theme::foreground())),
            )
            .child(
                Button::new(SharedString::from(format!("dismiss-error:{message}")))
                    .ghost()
                    .xsmall()
                    .flex_shrink_0()
                    .icon(Icon::default().path("close.svg"))
                    .accessibility_label("Dismiss error")
                    .tooltip("Dismiss")
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(Action::DismissError(message.clone()), window, cx);
                    })),
            )
    }

    /// "Agent silent for N min", with Stop and Restart (DD20).
    fn hang_notice(&self, minutes: u64, cx: &Context<Self>) -> Stateful<Div> {
        let title = format!("Agent silent for {minutes} min");
        col()
            .id("hang-notice")
            .role(Role::Alert)
            .aria_label(title.clone())
            .gap_2()
            .child(
                Alert::warning(
                    "hang-notice-alert",
                    "No ACP traffic and nothing open. The agent may be stuck, or still working without reporting.",
                )
                .title(title),
            )
            .child(
                row()
                    .gap_2()
                    .child(
                        self.button("hang-stop", "Stop", Action::Stop, cx)
                            .tooltip(format!("Stop ({})", shortcut("Ctrl+."))),
                    )
                    .child(
                        self.button("hang-restart", "Restart", Action::Restart, cx)
                            .tooltip(format!("Restart the agent ({})", shortcut("Ctrl+Shift+R"))),
                    ),
            )
    }

    /// Queued messages under the running turn, each with Edit, Remove and Send now.
    fn queued_entries(&self, live: &Live, cx: &Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let steering = live.features.steering;
        let send_now_help = if steering {
            format!("Send now ({})", shortcut("Ctrl+Shift+Enter"))
        } else {
            "Stops the turn, then sends".to_owned()
        };
        let mut list = col()
            .id("queued-messages")
            .role(Role::List)
            .aria_label("Queued messages")
            .w_full()
            .gap_2();
        for item in &live.queued {
            let id = item.id;
            let action = |name: &str, label: &'static str, action: Action| {
                Button::new(SharedString::from(format!("queued-{name}:{id}")))
                    .ghost()
                    .xsmall()
                    .label(label)
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.act(action.clone(), window, cx)),
                    )
            };
            list = list.child(
                row()
                    .id(SharedString::from(format!("queued:{id}")))
                    .role(Role::ListItem)
                    .aria_label(format!("Queued: {}", item.text))
                    .w_full()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.group_box)
                    .child(
                        col()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("Queued"),
                            )
                            .child(div().text_sm().line_clamp(3).child(item.text.clone()))
                            .when(!item.files.is_empty(), |column| {
                                column.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(item.files.join(", ")),
                                )
                            }),
                    )
                    .child(action("edit", "Edit", Action::EditQueued(id)))
                    .child(action("remove", "Remove", Action::RemoveQueued(id)))
                    .child(
                        action("send", "Send now", Action::SendQueuedNow(id))
                            .tooltip(send_now_help.clone()),
                    )
                    .context_menu({
                        let owner = cx.weak_entity();
                        move |menu, _, _| {
                            let owner = owner.clone();
                            menu.item(PopupMenuItem::new("Remove").on_click(
                                move |_, window, cx| {
                                    let _ = owner.update(cx, |app, cx| {
                                        app.act(Action::RemoveQueued(id), window, cx);
                                    });
                                },
                            ))
                        }
                    }),
            );
        }
        list
    }

    /// The agent's own permission options, without "reject always" (DD19).
    fn permission_card(
        &self,
        request: &protocol::PendingPermission,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let mut options = row().flex_wrap().gap_2();
        let primary = request.options.iter().position(|o| {
            matches!(
                o.kind,
                PermissionKind::AllowOnce | PermissionKind::AllowAlways
            )
        });
        for (ix, option) in request.options.iter().enumerate() {
            let action = Action::PermissionResponse(option.id.clone());
            let button = Button::new(SharedString::from(format!("permission-{}", option.id)))
                .small()
                .label(option.name.clone())
                .on_click(
                    cx.listener(move |app, _, window, cx| app.act(action.clone(), window, cx)),
                );
            options = options.child(if Some(ix) == primary {
                button.primary()
            } else {
                button.outline()
            });
        }
        col()
            .id("permission-request")
            .role(Role::Group)
            .aria_label("Permission request")
            .w_full()
            .gap_3()
            .p_4()
            .bg(cx.theme().group_box)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(request.title.clone()),
            )
            .child(options)
    }

    /// "Claude Code needs you to log in.", a button per method, and Retry (DD12).
    fn login_card(
        &self,
        id: &str,
        name: &str,
        methods: &[AuthMethod],
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let logged_in = self.logged_in.contains(id);
        let mut buttons = row().flex_wrap().gap_2();
        if !logged_in {
            for method in methods {
                let label = if method.terminal.is_some() {
                    format!("{} in terminal…", method.name)
                } else {
                    method.name.clone()
                };
                let action = Action::Login(method.id.clone());
                buttons = buttons.child(
                    Button::new(SharedString::from(format!("login-{}", method.id)))
                        .small()
                        .label(label)
                        .when(!method.description.is_empty(), |b| {
                            b.tooltip(method.description.clone())
                        })
                        .on_click(cx.listener(move |app, _, window, cx| {
                            app.act(action.clone(), window, cx);
                        })),
                );
            }
        }
        buttons = buttons.child(self.button("login-retry", "Retry", Action::RetryPrompt, cx));
        col()
            .id("login-required")
            .role(Role::Group)
            .aria_label("Login required")
            .w_full()
            .gap_3()
            .p_4()
            .bg(cx.theme().group_box)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(if logged_in {
                "Logged in".to_owned()
            } else {
                format!("{name} needs you to log in.")
            }))
            .when(methods.is_empty() && !logged_in, |card| {
                card.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Log in with the agent's own tools, then retry."),
                )
            })
            .child(buttons)
    }

    /// The crashed agent's stderr in a collapsed "Agent output" disclosure (DD18).
    fn crash_output(&self, id: &str, stderr: &str, cx: &Context<Self>) -> Div {
        let key = format!("stderr:{id}");
        let open = self.open_thoughts.contains(&key);
        let copy = stderr.to_owned();
        col()
            .gap_2()
            .child(
                Button::new("agent-output")
                    .ghost()
                    .small()
                    .label("Agent output")
                    .icon(Icon::default().path(if open {
                        "chevron.svg"
                    } else {
                        "caret-right.svg"
                    }))
                    .accessibility_label(if open {
                        "Hide agent output"
                    } else {
                        "Show agent output"
                    })
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(Action::ToggleThought(key.clone()), window, cx);
                    })),
            )
            .when(open, |column| {
                column.child(
                    col()
                        .gap_2()
                        .p_3()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .child(stderr.to_owned())
                        .child(
                            Button::new("agent-output-copy")
                                .ghost()
                                .xsmall()
                                .label("Copy")
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                }),
                        ),
                )
            })
    }

    /// An image attachment's picture, decoded once.
    pub(super) fn picture(&self, file: &Attachment) -> Option<Arc<Image>> {
        if !file.image() || file.data.is_empty() {
            return None;
        }
        let key = format!(
            "{}:{}:{}",
            file.name,
            file.size,
            &file.data[..file.data.len().min(64)]
        );
        if let Some(image) = self.pictures.borrow().get(&key) {
            return Some(image.clone());
        }
        let format = ImageFormat::from_mime_type(&file.mime)?;
        let bytes = conversation::unbase64(&file.data)?;
        let image = Arc::new(Image::from_bytes(format, bytes));
        self.pictures.borrow_mut().insert(key, image.clone());
        Some(image)
    }

    /// Files as chips: a miniature for images, else the file icon, the name
    /// and the size. Composer chips can be removed; sent images open a preview.
    pub(super) fn attachment_chips(
        &self,
        files: &[Attachment],
        message: Option<usize>,
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let mut chips = row().flex_wrap().gap_1p5();
        for (ix, file) in files.iter().enumerate() {
            let picture = self.picture(file);
            let thumb = match picture {
                Some(picture) => img(ImageSource::Image(picture))
                    .size(rems(1.5))
                    .rounded(theme.radius / 2.)
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => icon("file").size(rems(1.)).into_any_element(),
            };
            let label = format!("{}, {}", file.name, file_size(file.size));
            let mut chip = row()
                .id(SharedString::from(format!(
                    "attachment:{}:{ix}",
                    message.map_or_else(|| "composer".to_owned(), |m| m.to_string())
                )))
                .role(Role::Group)
                .aria_label(label)
                .h(rems(2.))
                .pl_1()
                .pr_2()
                .gap_1p5()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .text_xs()
                .child(thumb)
                .child(div().max_w(rems(12.)).truncate().child(file.name.clone()))
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(file_size(file.size)),
                );
            if let Some(message) = message {
                if file.image() {
                    chip =
                        chip.cursor_pointer()
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.act(Action::PreviewImage(message, ix), window, cx);
                            }));
                }
            } else {
                chip = chip.child(
                    Button::new(SharedString::from(format!("remove-attachment:{ix}")))
                        .ghost()
                        .xsmall()
                        .icon(Icon::default().path("close.svg"))
                        .accessibility_label(format!("Remove {}", file.name))
                        .tooltip(format!("Remove {}", file.name))
                        .on_click(cx.listener(move |app, _, window, cx| {
                            app.act(Action::RemoveAttachment(ix), window, cx);
                        })),
                );
            }
            chips = chips.child(chip);
        }
        chips
    }

    /// Adds a file to the composer, unless it is too big or an image the
    /// agent can't receive (scope R23).
    pub(super) fn attach(&mut self, file: Attachment, cx: &mut Context<Self>) {
        self.attachment_error = None;
        if file.size > conversation::ATTACHMENT_LIMIT {
            self.attachment_error = Some(format!(
                "{} is {}. Files over 20 MB can't be attached.",
                file.name,
                file_size(file.size)
            ));
        } else if file.image()
            && let Some(live) = self.current_live()
            && live.features.known
            && !live.features.images
        {
            let name = live
                .execution
                .as_ref()
                .map_or("This agent", |e| e.name.as_str());
            self.attachment_error = Some(format!("{name} can't receive images."));
        } else {
            self.attachments.push(file);
        }
        self.composer_region.update(cx, |_, cx| cx.notify());
    }

    /// Reads a local file into an attachment, on a background thread.
    pub(super) fn attach_local(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async move {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let mime = conversation::mime_for(&name).to_owned();
            let size = std::fs::metadata(&path)
                .map_err(|e| files::error(&path, e))?
                .len();
            // Too big to read; `attach` reports it.
            let data = if size > conversation::ATTACHMENT_LIMIT {
                String::new()
            } else {
                conversation::base64(&std::fs::read(&path).map_err(|e| files::error(&path, e))?)
            };
            Ok::<_, String>(Attachment {
                name,
                mime,
                size,
                data,
                path: None,
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |app, cx| match result {
                Ok(file) => app.attach(file, cx),
                Err(error) => {
                    app.attachment_error = Some(error);
                    app.composer_region.update(cx, |_, cx| cx.notify());
                }
            });
        })
        .detach();
    }

    /// A pasted image, or files copied in a file manager.
    pub(super) fn paste(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
        let mut taken = false;
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(image) => {
                    taken = true;
                    let format = image.format();
                    let count = self.attachments.iter().filter(|a| a.image()).count() + 1;
                    self.attach(
                        Attachment {
                            name: format!("Pasted image {count}.{}", format.extension()),
                            mime: format.mime_type().to_owned(),
                            size: image.bytes().len() as u64,
                            data: conversation::base64(image.bytes()),
                            path: None,
                        },
                        cx,
                    );
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    taken = true;
                    for path in paths.paths() {
                        self.attach_local(path.clone(), cx);
                    }
                }
                ClipboardEntry::String(_) => {}
            }
        }
        taken
    }

    /// The agent's commands matching a `/` typed at the start of the composer.
    pub(super) fn slash_matches(&self, cx: &App) -> Vec<conversation::AgentCommand> {
        if self.slash_dismissed {
            return Vec::new();
        }
        let text = self.composer.read(cx).value().to_string();
        let Some(typed) = text.strip_prefix('/') else {
            return Vec::new();
        };
        if typed.contains(char::is_whitespace) {
            return Vec::new();
        }
        let typed = typed.to_lowercase();
        self.current_live()
            .map(|live| {
                live.commands
                    .iter()
                    .filter(|command| command.name.to_lowercase().contains(&typed))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `/` list above the composer (DD10): command, input hint, description.
    pub(super) fn slash_list(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let commands = self.slash_matches(cx);
        if commands.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let highlight = self.slash.unwrap_or(0).min(commands.len() - 1);
        let mut list = col()
            .id("slash-commands")
            .role(Role::ListBox)
            .aria_label("Commands")
            .w_full()
            .max_h(rems(18.))
            .overflow_y_scroll()
            .p_1()
            .rounded(rems(0.625))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow(project_bar::menu_shadow(cx));
        for (ix, command) in commands.iter().enumerate() {
            let name = command.name.clone();
            list = list.child(
                col()
                    .id(SharedString::from(format!("slash:{}", command.name)))
                    .role(Role::ListBoxOption)
                    .aria_selected(ix == highlight)
                    .aria_label(format!("/{} {}", command.name, command.description))
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .when(ix == highlight, |row| row.bg(theme.accent))
                    .hover(|style| style.bg(theme.accent))
                    .cursor_pointer()
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.insert_command(&name, window, cx);
                    }))
                    .child(
                        row()
                            .gap_2()
                            .font_family(theme.mono_font_family.clone())
                            .text_sm()
                            .child(format!("/{}", command.name))
                            .when(!command.hint.is_empty(), |row| {
                                row.child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child(command.hint.clone()),
                                )
                            }),
                    )
                    .when(!command.description.is_empty(), |item| {
                        item.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(command.description.clone()),
                        )
                    }),
            );
        }
        Some(list.into_any_element())
    }

    /// Puts `/name ` in the composer and closes the list. It never sends.
    pub(super) fn insert_command(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.update(cx, |state, cx| {
            state.set_value(format!("/{name} "), window, cx);
        });
        self.slash = None;
        window.focus(&self.composer.focus_handle(cx), cx);
        self.composer_region.update(cx, |_, cx| cx.notify());
    }

    /// Keys for the `/` list and the queue while the composer has focus.
    /// Returns whether the key was used.
    pub(super) fn composer_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let commands = self.slash_matches(cx);
        if !commands.is_empty() {
            let last = commands.len() - 1;
            let current = self.slash.unwrap_or(0).min(last);
            match key {
                "up" => self.slash = Some(current.saturating_sub(1)),
                "down" => self.slash = Some((current + 1).min(last)),
                "enter" | "tab" => {
                    self.insert_command(&commands[current].name.clone(), window, cx);
                    return true;
                }
                "escape" => {
                    self.slash_dismissed = true;
                    self.slash = None;
                }
                _ => return false,
            }
            self.composer_region.update(cx, |_, cx| cx.notify());
            return true;
        }
        if key == "up"
            && self.composer.read(cx).value().is_empty()
            && self
                .current_live()
                .is_some_and(|live| !live.queued.is_empty())
        {
            self.act(Action::EditLastQueued, window, cx);
            return true;
        }
        false
    }

    /// The header's TODO button and its list (DD8).
    pub(super) fn todo_button(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let steps = self
            .current_live()
            .map(|live| live.todo.clone())
            .filter(|steps| !steps.is_empty())?;
        let theme = cx.theme();
        let done = steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        let current = conversation::current_step(&steps);
        let count = if current.is_none() {
            format!("{done} of {} done", steps.len())
        } else {
            format!("{} of {}", done + 1, steps.len())
        };
        let muted = theme.muted_foreground;
        let glyph = move |status| step_glyph(status, muted);
        let label = current.map_or_else(|| "All steps done".to_owned(), |step| step.text.clone());
        let trigger = Button::new("chat-todo")
            .ghost()
            .small()
            .min_w_0()
            .flex_shrink(1.)
            .max_w(rems(22.))
            .dropdown_caret(true)
            .accessibility_label(format!("TODO: {label}, {count}"))
            .tooltip(format!("TODO ({})", shortcut("Ctrl+Shift+T")))
            .child(
                row()
                    .min_w_0()
                    .gap_1p5()
                    .child(glyph(current.map_or(StepStatus::Completed, |s| s.status)))
                    .child(div().min_w_0().truncate().child(label))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(theme.muted_foreground)
                            .child(count),
                    ),
            );
        let owner = cx.weak_entity();
        Some(
            Popover::new("todo")
                .anchor(Anchor::TopRight)
                .trigger(trigger)
                .open(self.menu == Some("todo"))
                .on_open_change(move |open, window, cx| {
                    let _ = owner.update(cx, |app, cx| {
                        if *open != (app.menu == Some("todo")) {
                            app.act(Action::TodoList, window, cx);
                        }
                    });
                })
                .content(move |_, _, cx| {
                    let theme = cx.theme();
                    col()
                        .id("todo-list")
                        .role(Role::List)
                        .aria_label("TODO")
                        .w(rems(22.))
                        .gap_1()
                        .p_1()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("TODO"),
                        )
                        .children(steps.iter().map(|step| {
                            row()
                                .items_start()
                                .gap_2()
                                .px_2()
                                .py_1()
                                .text_sm()
                                .when(step.status == StepStatus::Completed, |row| {
                                    row.text_color(theme.muted_foreground)
                                })
                                .child(div().pt_0p5().child(glyph(step.status)))
                                .child(div().flex_1().min_w_0().child(step.text.clone()))
                        }))
                })
                .into_any_element(),
        )
    }

    /// A reply's thought: streaming muted text, then "Thought for 12s" (DD4).
    pub(super) fn thought_block(
        &self,
        thread: &Thread,
        i: usize,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let message = &thread.messages[i];
        if message.thought.is_empty() || !config::with(|s| s.modes.chats.show_thinking) {
            return None;
        }
        let theme = cx.theme();
        let key = format!("thought:{}:{i}", thread.id);
        let streaming = message.text.is_empty();
        let open = streaming || self.open_thoughts.contains(&key);
        let seconds = message
            .thought_ended
            .saturating_sub(message.thought_started)
            .div_ceil(1000)
            .max(1);
        let thought = div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(message.thought.clone());
        if streaming {
            return Some(thought.into_any_element());
        }
        Some(
            col()
                .gap_1()
                .child(
                    Button::new(SharedString::from(format!(
                        "thought-toggle:{}:{i}",
                        thread.id
                    )))
                    .ghost()
                    .xsmall()
                    .ml(rems(-0.375))
                    .text_color(theme.muted_foreground)
                    .label(format!("Thought for {seconds}s"))
                    .icon(Icon::default().path(if open {
                        "chevron.svg"
                    } else {
                        "caret-right.svg"
                    }))
                    .accessibility_label(if open { "Hide thought" } else { "Show thought" })
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(Action::ToggleThought(key.clone()), window, cx);
                    })),
                )
                .when(open, |column| {
                    column.child(thought.pl_2().border_l_1().border_color(theme.border))
                })
                .into_any_element(),
        )
    }

    /// The right panel's tabs: Agent activity and ACP traffic (DD7).
    pub(super) fn right_panel(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let tabs = TabBar::new("right-panel-tabs")
            .underline()
            .selected_index(usize::from(self.traffic_tab))
            .child(Tab::new().label("Agent activity"))
            .child(Tab::new().label("ACP traffic"))
            .on_click(move |ix, window, cx| {
                let ix = *ix;
                let _ = owner.update(cx, |app, cx| app.act(Action::PanelTab(ix), window, cx));
            });
        let body = if self.traffic_tab {
            self.traffic_panel(cx)
        } else {
            self.activity_panel(cx).into_any_element()
        };
        col()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(div().px_2().pt_1().child(tabs))
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// The open chat's raw ACP traffic and the agent's stderr (DD21).
    fn traffic_panel(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        if self.selected.is_none() {
            return col()
                .p_3()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Start a chat to see its ACP traffic.")
                .into_any_element();
        }
        let toolbar = row()
            .px_3()
            .py_1()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} messages", self.traffic.len())),
            )
            .child(
                self.button("traffic-copy", "Copy all", Action::CopyTraffic, cx)
                    .ghost(),
            );
        let mono = theme.mono_font_family.clone();
        let rows = self.traffic.iter().enumerate().map(|(ix, entry)| {
            let (marker, color) = match entry.direction {
                Direction::ToAgent => ("→ agent", theme.foreground),
                Direction::FromAgent => ("← agent", theme.foreground),
                Direction::Stderr => ("stderr", theme.muted_foreground),
            };
            let note = match entry.note {
                TrafficNote::NotJson => Some("not JSON"),
                TrafficNote::Unknown => Some("unknown"),
                TrafficNote::None => None,
            };
            let copy = entry.text.clone();
            row()
                .id(SharedString::from(format!("traffic:{ix}")))
                .items_start()
                .gap_2()
                .px_3()
                .py_0p5()
                .text_xs()
                .font_family(mono.clone())
                .child(
                    div()
                        .w(rems(4.5))
                        .flex_shrink_0()
                        .text_color(theme.muted_foreground)
                        .child(clock(entry.at)),
                )
                .child(
                    div()
                        .w(rems(4.))
                        .flex_shrink_0()
                        .text_color(theme.muted_foreground)
                        .child(marker),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(color)
                        .when_some(note, |text, note| {
                            text.child(div().text_color(theme.warning).child(note))
                        })
                        .child(excerpt(&entry.text, 12, 2000)),
                )
                .context_menu(move |menu, _, _| {
                    let copy = copy.clone();
                    menu.item(
                        PopupMenuItem::new("Copy message").on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                        }),
                    )
                })
        });
        col()
            .id("acp-traffic")
            .role(Role::Log)
            .aria_label("ACP traffic")
            .size_full()
            .child(toolbar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("traffic-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.traffic_scroll)
                            .child(col().w_full().pb_2().children(rows)),
                    )
                    .child(Scrollbar::vertical(&self.traffic_scroll)),
            )
            .into_any_element()
    }

    /// A sent image, large, in a dialog. Escape closes it.
    pub(super) fn preview_image(
        &mut self,
        message: usize,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self
            .selected
            .and_then(|thread| self.workspace().threads[thread].messages.get(message))
            .and_then(|message| message.attachments.get(ix))
            .cloned()
        else {
            return;
        };
        let Some(picture) = self.picture(&file) else {
            return;
        };
        window.open_dialog(cx, move |dialog, _, cx| {
            views::styled_dialog(dialog, cx)
                .w(px(760.))
                .title(views::dialog_title(file.name.clone()))
                .child(
                    img(ImageSource::Image(picture.clone()))
                        .w_full()
                        .max_h(rems(36.))
                        .object_fit(ObjectFit::Contain),
                )
        });
    }

    /// Every traffic line as text, for Copy all.
    pub(super) fn traffic_text(&self) -> String {
        self.traffic
            .iter()
            .map(|entry| {
                let direction = match entry.direction {
                    Direction::ToAgent => "->",
                    Direction::FromAgent => "<-",
                    Direction::Stderr => "stderr",
                };
                format!("{} {direction} {}", clock(entry.at), entry.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A TODO step's status, by shape: spinner, check or empty circle (DD8).
fn step_glyph(status: StepStatus, muted: Hsla) -> AnyElement {
    match status {
        StepStatus::InProgress => Spinner::new().xsmall().into_any_element(),
        StepStatus::Completed => icon("check")
            .size(rems(0.875))
            .text_color(muted)
            .into_any_element(),
        StepStatus::Pending => icon("circle")
            .size(rems(0.875))
            .text_color(muted)
            .into_any_element(),
    }
}

/// Local time to the second: `2:43:05 PM`.
fn clock(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%-I:%M:%S %p")
                .to_string()
        })
        .unwrap_or_default()
}

/// At most `lines` lines and `chars` characters of `text`, marked when cut.
fn excerpt(text: &str, lines: usize, chars: usize) -> String {
    let kept: Vec<&str> = text.lines().take(lines).collect();
    let mut kept = kept.join("\n");
    let mut cut = kept.len() < text.len();
    if kept.chars().count() > chars {
        kept = kept.chars().take(chars).collect();
        cut = true;
    }
    if cut {
        kept.push('…');
    }
    kept
}
