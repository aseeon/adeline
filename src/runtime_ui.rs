use super::*;
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Default)]
pub(super) struct Runtime {
    conversations: HashMap<String, LiveConversation>,
    pub expanded_tools: HashSet<String>,
    exiting: bool,
}

#[derive(Default)]
struct LiveConversation {
    driver: Option<acp::Driver>,
    processing: bool,
    shutting_down: bool,
    shutdown_stuck: bool,
    storage_failed: bool,
    replacement: bool,
    recovering_storage: bool,
    error: Option<String>,
    progress: Option<String>,
    permission: VecDeque<PendingPermission>,
    execution: Option<storage::ExecutionConfig>,
    permission_mode: Option<agents::PermissionMode>,
    last_read_through: Option<usize>,
    assistant: Option<usize>,
    last_prompt: String,
    worked: bool,
    generation: u64,
    turn: u64,
}

struct PendingPermission {
    request_id: u64,
    title: String,
    options: Vec<acp::PermissionChoice>,
}

impl Adeline {
    pub(super) fn watch_runtime(&mut self, cx: &mut Context<Self>) {
        if self.demo_mode {
            return;
        }
        // A receiver is created for each driver so events from an old process
        // cannot be mistaken for turns from its replacement.
        if let Ok(store) = self.project_store.lock() {
            if !store.errors.is_empty() {
                self.notify_toast(&store.errors.join("\n"), cx);
            }
            for conversation in store.projects.iter().flat_map(|p| &p.conversations) {
                let thread = conversation.to_thread();
                let last_user = thread.messages.iter().rposition(|m| m.role == "user");
                let interrupted = matches!(
                    conversation.settings.status.as_str(),
                    "processing" | "blocked"
                );
                let last_error = conversation
                    .events
                    .iter()
                    .chain(&conversation.unsaved_events)
                    .rev()
                    .take_while(|event| event.kind != "message" || event.data["role"] != "user")
                    .find_map(|event| {
                        if event.kind == "error" {
                            event
                                .data
                                .get("message")
                                .and_then(serde_json::Value::as_str)
                        } else if event.kind == "lifecycle" {
                            event.data.get("error").and_then(serde_json::Value::as_str)
                        } else {
                            None
                        }
                    });
                let error = conversation.storage_error.clone().map(|error| format!(
                    "History could not be saved: {error}. Restore storage access, then Retry storage."
                )).or_else(|| {
                    interrupted.then(|| last_error.map_or_else(
                        || "The previous turn was interrupted. Retry continues from the saved session.".to_owned(),
                        str::to_owned,
                    ))
                });
                let execution = Some(conversation.settings.execution.clone());
                let permission_mode = Some(conversation.settings.permission_mode);
                let last_read_through = thread
                    .messages
                    .iter()
                    .rposition(|message| message.role == "assistant" && message.read);
                self.runtime.conversations.insert(
                    conversation.id.clone(),
                    LiveConversation {
                        storage_failed: conversation.storage_error.is_some(),
                        execution,
                        permission_mode,
                        last_read_through,
                        error,
                        last_prompt: last_user
                            .map(|i| thread.messages[i].text.clone())
                            .unwrap_or_default(),
                        worked: interrupted
                            || last_user.is_some_and(|i| i + 1 < thread.messages.len()),
                        ..Default::default()
                    },
                );
            }
        }
        cx.notify();
    }

    fn current_id(&self) -> Option<String> {
        self.selected
            .and_then(|i| self.workspace().threads.get(i))
            .map(|t| t.id.clone())
    }

    fn locate_conversation(&self, id: &str) -> Option<(usize, usize)> {
        self.projects.iter().enumerate().find_map(|(p, project)| {
            project
                .threads
                .iter()
                .position(|thread| thread.id == id)
                .map(|t| (p, t))
        })
    }

    fn conversation_settings(&self, id: &str) -> Option<storage::ConversationSettings> {
        let store = self.project_store.lock().ok()?;
        store
            .projects
            .iter()
            .flat_map(|p| &p.conversations)
            .find(|c| c.id == id)
            .map(|c| c.settings.clone())
    }

    pub(super) fn bound_definition(&self) -> Option<storage::ExecutionConfig> {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .and_then(|live| live.execution.clone())
    }

    /// The open chat's permission mode, or the one a new chat will start with.
    pub(super) fn current_permission_mode(&self) -> Option<agents::PermissionMode> {
        if self.selected.is_none() {
            return self.new_chat_permission.or_else(|| {
                self.selected_definition()
                    .map(|agent| agent.permission_mode)
            });
        }
        if self.demo_mode {
            return Some(if self.permission == 0 {
                agents::PermissionMode::Ask
            } else {
                agents::PermissionMode::AllowEverything
            });
        }
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .and_then(|live| live.permission_mode)
    }

    pub(super) fn conversation_processing(&self) -> bool {
        self.current_id()
            .and_then(|id| self.runtime.conversations.get(&id))
            .is_some_and(|r| r.processing)
    }

    pub(super) fn mark_conversation_read(&mut self, _cx: &mut Context<Self>) {
        if self.demo_mode {
            return;
        }
        let Some(id) = self.current_id() else {
            return;
        };
        let Some((p, t)) = self.locate_conversation(&id) else {
            return;
        };
        let Some(through) = self.projects[p].threads[t]
            .messages
            .iter()
            .rposition(|message| message.role == "assistant")
        else {
            return;
        };
        if self
            .runtime
            .conversations
            .get(&id)
            .and_then(|live| live.last_read_through)
            .is_some_and(|last| last >= through)
        {
            return;
        }
        if self.record_visible(&id, "message_read", json!({"through":through})) {
            self.runtime
                .conversations
                .entry(id)
                .or_default()
                .last_read_through = Some(through);
            self.projects[p].threads[t].mark_read();
        }
    }

    fn storage_failure(&mut self, id: &str, error: &str) {
        let live = self.runtime.conversations.entry(id.to_owned()).or_default();
        live.storage_failed = true;
        live.recovering_storage = false;
        live.error = Some(format!(
            "History could not be saved: {error}. Restore storage access, then Retry storage."
        ));
        live.permission.clear();
        if let Some(driver) = &live.driver {
            let _ = driver.send(acp::Command::Cancel);
        }
        if let Some((p, t)) = self.locate_conversation(id)
            && !matches!(
                self.projects[p].threads[t].status.as_str(),
                "completed" | "archived"
            )
        {
            self.projects[p].threads[t].status = "blocked".into();
            self.projects[p].rebuild_counts();
        }
    }

    fn record_visible(&mut self, id: &str, kind: &str, data: serde_json::Value) -> bool {
        let result = self
            .project_store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut store| {
                store.record_event(id, &storage::TranscriptEvent::new(kind, data))
            });
        if let Err(error) = result {
            self.storage_failure(id, &error);
            false
        } else {
            true
        }
    }

    fn save_conversation_settings(
        &mut self,
        id: &str,
        settings: storage::ConversationSettings,
    ) -> bool {
        let Some((p, _)) = self.locate_conversation(id) else {
            return false;
        };
        let project_id = self.projects[p].config.id.clone();
        let result = self
            .project_store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut store| store.update_conversation(&project_id, id, settings));
        if let Err(error) = result {
            self.storage_failure(id, &error);
            false
        } else {
            true
        }
    }

    fn set_runtime_status(&mut self, id: &str, status: &str) -> bool {
        let Some(mut settings) = self.conversation_settings(id) else {
            return false;
        };
        status.clone_into(&mut settings.status);
        if !self.save_conversation_settings(id, settings) {
            return false;
        }
        if let Some((p, t)) = self.locate_conversation(id) {
            status.clone_into(&mut self.projects[p].threads[t].status);
            self.projects[p].rebuild_counts();
        }
        true
    }

    fn refresh_runtime_views(&self, cx: &mut Context<Self>) {
        self.sync_sidebar(cx);
        self.transcript
            .update(cx, |view, cx| view.sync(self, false, cx));
        self.composer_region.update(cx, |_, cx| cx.notify());
        self.header_region.update(cx, |_, cx| cx.notify());
        self.control_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn ensure_driver(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        if self
            .runtime
            .conversations
            .get(id)
            .is_some_and(|r| r.driver.is_some())
        {
            return true;
        }
        let Some(settings) = self.conversation_settings(id) else {
            return false;
        };
        let (sender, receiver) = async_channel::unbounded();
        let store = self.project_store.clone();
        let record = std::sync::Arc::new(
            move |id: &str, direction: &str, value: &serde_json::Value| {
                let mut store = store.lock().map_err(|e| e.to_string())?;
                if direction == "session" {
                    let (project_id, mut settings) = store
                        .projects
                        .iter()
                        .find_map(|p| {
                            p.conversations
                                .iter()
                                .find(|c| c.id == id)
                                .map(|c| (p.id.clone(), c.settings.clone()))
                        })
                        .ok_or_else(|| "Conversation is missing from storage.".to_owned())?;
                    if value["replaced"].as_bool() == Some(true) {
                        let old = value["old_session_id"]
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| settings.session_id.take());
                        if let Some(old) = old {
                            settings.previous_session_ids.push(old);
                        }
                    }
                    settings.session_id = value["session_id"].as_str().map(str::to_owned);
                    let event_result = store
                        .record_event(id, &storage::TranscriptEvent::new("session", value.clone()));
                    let settings_result = store.update_conversation(&project_id, id, settings);
                    event_result.and(settings_result)
                } else if matches!(direction, "visible_text" | "visible_tool" | "visible_usage") {
                    let kind = match direction {
                        "visible_text" => "assistant_chunk",
                        "visible_tool" => "tool",
                        _ => "usage",
                    };
                    store.record_event(id, &storage::TranscriptEvent::new(kind, value.clone()))
                } else {
                    store.record_raw(id, direction, value)
                }
            },
        );
        let driver = acp::Driver::spawn(
            id.to_owned(),
            settings.execution,
            settings.session_id,
            settings.permission_mode,
            sender,
            record,
        );
        let live = self.runtime.conversations.entry(id.to_owned()).or_default();
        live.generation = live.generation.wrapping_add(1);
        live.turn = 0;
        let generation = live.generation;
        live.driver = Some(driver);
        cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                if this
                    .update(cx, |app, cx| app.runtime_event(generation, event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        true
    }

    pub(super) fn send_real(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_open_project() || self.runtime.exiting {
            return;
        }
        let prompt = self.composer.read(cx).value().trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        let id = if let Some(id) = self.current_id() {
            id
        } else {
            if self.selected_definition().is_none() && self.agent_catalog.entries.len() == 1 {
                self.selected_agent = Some(self.agent_catalog.entries[0].id.clone());
            }
            let Some(mut agent) = self.selected_definition().cloned() else {
                self.notify_toast("Create or select an agent before sending.", cx);
                self.open_commands("agent", window, cx);
                return;
            };
            if let Some(mode) = self.new_chat_permission.take() {
                agent.permission_mode = mode;
            }
            let project_id = self.workspace().config.id.clone();
            let result = self
                .project_store
                .lock()
                .map_err(|e| e.to_string())
                .and_then(|mut store| {
                    let id =
                        store.create_conversation(&project_id, &agent, &short(&prompt, 100))?;
                    let saved = store
                        .projects
                        .iter()
                        .flat_map(|p| &p.conversations)
                        .find(|c| c.id == id)
                        .ok_or_else(|| "Created conversation is missing.".to_owned())?;
                    Ok((
                        id,
                        saved.to_thread(),
                        saved.settings.execution.clone(),
                        saved.settings.permission_mode,
                    ))
                });
            match result {
                Ok((id, thread, execution, permission_mode)) => {
                    self.projects[self.project].threads.insert(0, thread);
                    self.selected = Some(0);
                    self.runtime.conversations.insert(
                        id.clone(),
                        LiveConversation {
                            execution: Some(execution),
                            permission_mode: Some(permission_mode),
                            ..Default::default()
                        },
                    );
                    id
                }
                Err(error) => {
                    self.notify_toast(&error, cx);
                    return;
                }
            }
        };
        let live = self.runtime.conversations.entry(id.clone()).or_default();
        if live.processing
            || live.shutting_down
            || live.storage_failed
            || live.recovering_storage
            || live.replacement
        {
            return;
        }
        let message = Message {
            role: "user".into(),
            text: prompt.clone(),
            read: true,
            created_at: recency::now().to_string(),
            ..Default::default()
        };
        self.runtime
            .conversations
            .entry(id.clone())
            .or_default()
            .last_prompt
            .clone_from(&prompt);
        let saved = self.record_visible(
            &id,
            "message",
            json!({"role":"user","text":prompt,"read":true}),
        );
        if let Some((p, t)) = self.locate_conversation(&id) {
            self.projects[p].threads[t].push_message(message);
        }
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
        if saved {
            self.start_prompt(&id, prompt, false, cx);
        }
        self.refresh_runtime_views(cx);
    }

    fn start_prompt(&mut self, id: &str, prompt: String, retry: bool, cx: &mut Context<Self>) {
        if !self.set_runtime_status(id, "processing") {
            return;
        }
        if !self.ensure_driver(id, cx) {
            let error = "Could not load this conversation's saved agent settings.".to_owned();
            self.record_visible(id, "error", json!({"message":error}));
            self.runtime
                .conversations
                .entry(id.to_owned())
                .or_default()
                .error = Some(error);
            self.set_runtime_status(id, "blocked");
            return;
        }
        if retry
            && !self.record_visible(
                id,
                "lifecycle",
                json!({"event":"turn_started","retry":true}),
            )
        {
            return;
        }
        let live = self.runtime.conversations.entry(id.to_owned()).or_default();
        if live.storage_failed {
            return;
        }
        live.processing = true;
        live.error = None;
        live.progress = None;
        live.assistant = None;
        live.permission.clear();
        if !retry {
            live.worked = false;
            live.last_prompt.clone_from(&prompt);
        }
        live.turn = live.turn.saturating_add(1);
        let retries = u32::try_from(config::current().modes.chats.retry_limit).unwrap_or(u32::MAX);
        if let Some(driver) = &live.driver
            && let Err(error) = driver.send(acp::Command::Prompt {
                text: prompt,
                retries,
            })
        {
            live.processing = false;
            live.error = Some(error.clone());
            live.driver = None;
            self.record_visible(id, "error", json!({"message":error}));
            self.set_runtime_status(id, "blocked");
        }
    }

    fn runtime_event(&mut self, generation: u64, event: acp::Event, cx: &mut Context<Self>) {
        let id = event.conversation_id;
        let Some((p, t)) = self.locate_conversation(&id) else {
            return;
        };
        let Some(live) = self.runtime.conversations.get_mut(&id) else {
            return;
        };
        if live.generation != generation || live.driver.is_none() || event.turn < live.turn {
            return;
        }
        live.turn = event.turn;
        let closing = live.shutting_down
            || self.runtime.exiting
            || matches!(
                self.projects[p].threads[t].status.as_str(),
                "completed" | "archived"
            );
        match event.kind {
            acp::EventKind::Session {
                session_id,
                replaced,
            } => {
                self.record_visible(
                    &id,
                    "lifecycle",
                    json!({"event":"session_ready","session_id":session_id,"replacement":replaced}),
                );
            }
            acp::EventKind::Text(delta) => {
                let index = self
                    .runtime
                    .conversations
                    .entry(id.clone())
                    .or_default()
                    .assistant;
                if let Some(index) = index {
                    self.projects[p].threads[t].messages[index]
                        .text
                        .push_str(&delta);
                } else {
                    let index = self.projects[p].threads[t].messages.len();
                    self.projects[p].threads[t].push_message(Message {
                        role: "assistant".into(),
                        text: delta,
                        read: self.project == p && self.selected == Some(t),
                        created_at: recency::now().to_string(),
                        ..Default::default()
                    });
                    self.runtime
                        .conversations
                        .entry(id.clone())
                        .or_default()
                        .assistant = Some(index);
                }
                self.runtime
                    .conversations
                    .entry(id.clone())
                    .or_default()
                    .worked = true;
                self.projects[p].threads[t].prepare_search();
            }
            acp::EventKind::Tool {
                id: tool_id,
                title,
                status,
                detail,
                kind,
                paths,
            } => {
                // Streamed tool state is durably recorded by the worker before dispatch.
                let thread = &mut self.projects[p].threads[t];
                let turn = thread.messages.iter().rposition(|m| m.role == "user");
                let key = format!("tool:{tool_id}");
                let item = if let Some(index) = thread.activity.iter().position(|a| a.kind == key) {
                    index
                } else {
                    thread.activity.push(Activity {
                        kind: key,
                        ..Default::default()
                    });
                    thread.activity.len() - 1
                };
                thread.activity[item].title = format!("{title} ({status})");
                thread.activity[item].detail = detail;
                thread.activity[item].running =
                    matches!(status.as_str(), "pending" | "in_progress");
                if !kind.is_empty() {
                    thread.activity[item].tool = kind;
                }
                if !paths.is_empty() {
                    thread.activity[item].paths = paths;
                }
                thread.activity[item].turn = thread.activity[item].turn.or(turn);
                self.runtime
                    .conversations
                    .entry(id.clone())
                    .or_default()
                    .worked = true;
            }
            acp::EventKind::Permission {
                request_id,
                title,
                options,
            } => {
                if closing {
                    return;
                }
                let choices: Vec<_> = options
                    .iter()
                    .map(|option| {
                        json!({
                            "id":option.option_id,"name":option.name,"kind":option.kind
                        })
                    })
                    .collect();
                if !self.record_visible(
                    &id,
                    "permission_request",
                    json!({
                        "request_id":request_id,"title":title,"options":choices
                    }),
                ) {
                    self.refresh_runtime_views(cx);
                    return;
                }
                self.runtime
                    .conversations
                    .entry(id.clone())
                    .or_default()
                    .permission
                    .push_back(PendingPermission {
                        request_id,
                        title,
                        options,
                    });
                self.set_runtime_status(&id, "blocked");
            }
            acp::EventKind::Retrying {
                attempt,
                limit,
                error,
            } => {
                if closing {
                    return;
                }
                if !self.record_visible(
                    &id,
                    "error",
                    json!({"message":error,"attempt":attempt,"limit":limit}),
                ) {
                    self.refresh_runtime_views(cx);
                    return;
                }
                self.runtime
                    .conversations
                    .entry(id.clone())
                    .or_default()
                    .progress = Some(format!("Retry {attempt} of {limit}: {error}"));
            }
            acp::EventKind::Error { message, kind } => {
                let saved = self.record_visible(
                    &id,
                    "error",
                    json!({"message":message,"kind":format!("{kind:?}")}),
                );
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.processing = false;
                live.progress = None;
                live.permission.clear();
                if saved {
                    live.error = Some(message);
                    if !closing {
                        self.set_runtime_status(&id, "blocked");
                    }
                }
            }
            acp::EventKind::Finished { stop_reason } => {
                self.record_visible(
                    &id,
                    "lifecycle",
                    json!({"event":"turn_finished","reason":stop_reason}),
                );
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.processing = false;
                live.progress = None;
                live.permission.clear();
                if !closing && !live.storage_failed {
                    self.set_runtime_status(&id, "idle");
                }
            }
            acp::EventKind::Stopped => {
                let saved =
                    self.record_visible(&id, "lifecycle", json!({"event":"turn_cancelled"}));
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.processing = false;
                live.permission.clear();
                live.progress = None;
                if live.recovering_storage {
                    live.recovering_storage = false;
                    if saved {
                        live.error = (!live.last_prompt.is_empty()).then(|| {
                            "History is saved. Retry to continue the interrupted turn.".into()
                        });
                        if !closing && !live.storage_failed {
                            self.set_runtime_status(&id, "blocked");
                        }
                    }
                } else if saved && !closing && !live.storage_failed {
                    self.set_runtime_status(&id, "idle");
                }
            }
            acp::EventKind::ShutdownStuck => {
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.shutdown_stuck = true;
                live.error =
                    Some("Agent shutdown is stuck. Force Stop terminates this process.".into());
            }
            acp::EventKind::ShutdownComplete => {
                let saved =
                    self.record_visible(&id, "lifecycle", json!({"event":"process_stopped"}));
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.driver = None;
                live.processing = false;
                live.shutting_down = false;
                if live.shutdown_stuck && saved && !live.storage_failed {
                    live.error = None;
                }
                live.shutdown_stuck = false;
                live.permission.clear();
                if live.recovering_storage {
                    live.recovering_storage = false;
                    if saved
                        && !live.storage_failed
                        && !matches!(
                            self.projects[p].threads[t].status.as_str(),
                            "completed" | "archived"
                        )
                    {
                        live.error =
                            Some("Agent stopped. Retry to continue from the saved session.".into());
                    }
                }
                self.finish_project_deletion(cx);
                if self.runtime.exiting
                    && self
                        .runtime
                        .conversations
                        .values()
                        .all(|r| r.driver.is_none())
                {
                    let handle = self.main_window;
                    let owner = cx.weak_entity();
                    cx.defer(move |cx| settings::request_close(handle, owner, cx));
                }
            }
            acp::EventKind::Usage { used, size } => {
                self.projects[p].threads[t].context = Some((used, size));
            }
            acp::EventKind::StorageError(error) => {
                self.storage_failure(&id, &error);
            }
            acp::EventKind::ReplacementRequired(error) => {
                if closing {
                    return;
                }
                if !self.record_visible(&id, "error", json!({"message":error,"restoration":false}))
                {
                    self.refresh_runtime_views(cx);
                    return;
                }
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.replacement = true;
                live.processing = false;
                live.error = Some(format!(
                    "{error} Start a new session using saved messages only if you want to continue."
                ));
                self.set_runtime_status(&id, "blocked");
            }
        }
        if self.current_id().as_deref() == Some(id.as_str())
            && self
                .runtime
                .conversations
                .get(&id)
                .is_some_and(|live| !live.processing && !live.storage_failed)
        {
            self.mark_conversation_read(cx);
            if let Some((project, _)) = self.locate_conversation(&id) {
                self.projects[project].rebuild_counts();
            }
        }
        self.refresh_runtime_views(cx);
    }

    pub(super) fn stop_conversation(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id()
            && let Some(live) = self.runtime.conversations.get_mut(&id)
        {
            live.permission.clear();
            live.progress = Some("Stopping…".into());
            if let Some(driver) = &live.driver {
                let _ = driver.send(acp::Command::Cancel);
            }
        }
        self.refresh_runtime_views(cx);
    }

    pub(super) fn complete_conversation(&mut self, archive: bool, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id() {
            self.finish_conversation(&id, archive, cx);
        }
    }

    /// Completes or archives a chat, stopping its agent. Completing a finished
    /// chat reopens it as idle.
    pub(super) fn finish_conversation(&mut self, id: &str, archive: bool, cx: &mut Context<Self>) {
        let id = id.to_owned();
        let Some((p, t)) = self.locate_conversation(&id) else {
            return;
        };
        if self.demo_mode {
            self.projects[p].threads[t].status =
                if archive { "archived" } else { "completed" }.into();
        } else if !archive
            && matches!(
                self.projects[p].threads[t].status.as_str(),
                "completed" | "archived"
            )
        {
            self.set_runtime_status(&id, "idle");
        } else {
            self.set_runtime_status(&id, if archive { "archived" } else { "completed" });
            self.shutdown_conversation(&id);
        }
        self.menu = None;
        self.refresh_runtime_views(cx);
    }

    fn shutdown_conversation(&mut self, id: &str) {
        let live = self.runtime.conversations.entry(id.to_owned()).or_default();
        live.permission.clear();
        live.progress = None;
        if let Some(driver) = &live.driver
            && !live.shutting_down
        {
            live.shutting_down = true;
            if let Err(error) = driver.send(acp::Command::Shutdown) {
                live.error = Some(error);
            }
        }
    }

    pub(super) fn stop_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let ids: Vec<_> = self
            .projects
            .iter()
            .filter(|p| p.config.id == project_id)
            .flat_map(|p| p.threads.iter().map(|t| t.id.clone()))
            .collect();
        for id in ids {
            self.shutdown_conversation(&id);
        }
        self.refresh_runtime_views(cx);
    }

    pub(super) fn project_agents_stopped(&self, project_id: &str) -> bool {
        self.projects
            .iter()
            .filter(|p| p.config.id == project_id)
            .flat_map(|p| &p.threads)
            .all(|t| {
                self.runtime
                    .conversations
                    .get(&t.id)
                    .is_none_or(|r| r.driver.is_none())
            })
    }

    pub(super) fn forget_project_runtime(&mut self, ids: &[String]) {
        for id in ids {
            if self
                .runtime
                .conversations
                .get(id)
                .is_none_or(|live| live.driver.is_none())
            {
                self.runtime.conversations.remove(id);
                let prefix = format!("{id}:");
                self.runtime
                    .expanded_tools
                    .retain(|key| !key.starts_with(&prefix));
            }
        }
    }

    pub(super) fn force_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        for thread in self
            .projects
            .iter()
            .filter(|p| p.config.id == project_id)
            .flat_map(|p| &p.threads)
        {
            if let Some(driver) = self
                .runtime
                .conversations
                .get(&thread.id)
                .and_then(|r| r.driver.as_ref())
            {
                let _ = driver.send(acp::Command::ForceStop);
            }
        }
        cx.notify();
    }

    pub(super) fn force_conversation(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.current_id()
            && let Some(driver) = self
                .runtime
                .conversations
                .get(&id)
                .and_then(|r| r.driver.as_ref())
        {
            let _ = driver.send(acp::Command::ForceStop);
        }
        cx.notify();
    }

    pub(super) fn force_all(&mut self, cx: &mut Context<Self>) {
        for live in self.runtime.conversations.values() {
            if let Some(driver) = &live.driver {
                let _ = driver.send(acp::Command::ForceStop);
            }
        }
        cx.notify();
    }

    pub(super) fn request_runtime_exit(&mut self, cx: &mut Context<Self>) -> bool {
        if self
            .runtime
            .conversations
            .values()
            .all(|r| r.driver.is_none())
        {
            return true;
        }
        self.runtime.exiting = true;
        let ids: Vec<_> = self.runtime.conversations.keys().cloned().collect();
        for id in ids {
            self.shutdown_conversation(&id);
        }
        self.modal = Some("shutdown");
        self.refresh_runtime_views(cx);
        false
    }

    pub(super) fn retry_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current_id() else {
            return;
        };
        let Some(live) = self.runtime.conversations.get(&id) else {
            return;
        };
        if live.processing
            || live.shutting_down
            || live.storage_failed
            || live.recovering_storage
            || live.replacement
        {
            return;
        }
        let prompt = if live.worked {
            "Continue the interrupted turn from the saved session. Preserve completed work; do not repeat completed tool actions.".to_owned()
        } else {
            live.last_prompt.clone()
        };
        if !prompt.is_empty() {
            self.start_prompt(&id, prompt, true, cx);
        }
        self.refresh_runtime_views(cx);
    }

    pub(super) fn retry_storage(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current_id() else {
            return;
        };
        if self
            .runtime
            .conversations
            .get(&id)
            .is_none_or(|live| !live.storage_failed)
        {
            return;
        }
        let result = self
            .project_store
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut store| {
                store.retry_unsaved(&id)?;
                store
                    .conversation(&id)
                    .map(|conversation| {
                        let worked = conversation
                            .events
                            .iter()
                            .chain(&conversation.unsaved_events)
                            .rev()
                            .take_while(|event| {
                                event.kind != "message" || event.data["role"] != "user"
                            })
                            .any(|event| {
                                matches!(
                                    event.kind.as_str(),
                                    "assistant_chunk" | "tool" | "message_update"
                                ) || (event.kind == "message" && event.data["role"] == "assistant")
                            });
                        (
                            conversation.settings.clone(),
                            conversation.to_thread(),
                            worked,
                        )
                    })
                    .ok_or_else(|| "Conversation is missing from storage.".to_owned())
            });
        match result {
            Ok((settings, mut thread, recovered_work)) => {
                if let Some((p, t)) = self.locate_conversation(&id) {
                    if self.project == p && self.selected == Some(t) {
                        thread.mark_read();
                    }
                    self.projects[p].threads[t] = thread;
                    self.projects[p].rebuild_counts();
                }
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.storage_failed = false;
                live.worked |= recovered_work;
                live.recovering_storage = live.processing;
                live.permission_mode = Some(settings.permission_mode);
                live.error = if live.recovering_storage {
                    Some("History is saved. Waiting for the stopped turn to settle.".into())
                } else if !matches!(settings.status.as_str(), "completed" | "archived")
                    && !live.last_prompt.is_empty()
                {
                    Some("History is saved. Retry to continue the interrupted turn.".into())
                } else {
                    None
                };
                if let Some(driver) = &live.driver {
                    if driver.send(acp::Command::ResumeStorage).is_ok() {
                        let _ =
                            driver.send(acp::Command::SetPermissionMode(settings.permission_mode));
                    } else {
                        live.driver = None;
                        live.processing = false;
                        live.recovering_storage = false;
                    }
                }
                if !matches!(settings.status.as_str(), "completed" | "archived") {
                    self.set_runtime_status(&id, "blocked");
                }
                if !self
                    .runtime
                    .conversations
                    .get(&id)
                    .is_some_and(|live| live.storage_failed)
                {
                    self.mark_conversation_read(cx);
                }
            }
            Err(error) => {
                self.runtime.conversations.entry(id).or_default().error = Some(error);
            }
        }
        self.refresh_runtime_views(cx);
    }

    pub(super) fn replace_session(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current_id() else {
            return;
        };
        let Some((p, t)) = self.locate_conversation(&id) else {
            return;
        };
        if !self
            .runtime
            .conversations
            .get(&id)
            .is_some_and(|live| live.replacement && !live.storage_failed && !live.shutting_down)
        {
            return;
        }
        let context = self.projects[p].threads[t]
            .messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.text))
            .collect::<Vec<_>>()
            .join("\n\n");
        if !self.set_runtime_status(&id, "processing") {
            self.refresh_runtime_views(cx);
            return;
        }
        if !self.ensure_driver(&id, cx) {
            self.set_runtime_status(&id, "blocked");
            return;
        }
        if !self.record_visible(
            &id,
            "lifecycle",
            json!({"event":"replacement_session_approved"}),
        ) {
            self.refresh_runtime_views(cx);
            return;
        }
        let live = self.runtime.conversations.entry(id.clone()).or_default();
        if let Some(driver) = &live.driver
            && let Err(error) = driver.send(acp::Command::ReplaceSession { context })
        {
            live.driver = None;
            live.error = Some(error);
            self.set_runtime_status(&id, "blocked");
            self.refresh_runtime_views(cx);
            return;
        }
        live.replacement = false;
        live.error = None;
        live.processing = true;
        live.assistant = None;
        self.refresh_runtime_views(cx);
    }

    pub(super) fn set_conversation_permission(&mut self, mode: usize, cx: &mut Context<Self>) {
        let Some(id) = self.current_id() else {
            return;
        };
        let Some(mut settings) = self.conversation_settings(&id) else {
            return;
        };
        let permission = if mode == 0 {
            agents::PermissionMode::Ask
        } else {
            agents::PermissionMode::AllowEverything
        };
        settings.permission_mode = permission;
        if self.save_conversation_settings(&id, settings) {
            if let Some(live) = self.runtime.conversations.get_mut(&id) {
                live.permission_mode = Some(permission);
            }
            if self.record_visible(&id, "permission_mode", json!({"mode":permission}))
                && let Some(driver) = self
                    .runtime
                    .conversations
                    .get(&id)
                    .and_then(|r| r.driver.as_ref())
                && let Err(error) = driver.send(acp::Command::SetPermissionMode(permission))
            {
                let live = self.runtime.conversations.entry(id.clone()).or_default();
                live.driver = None;
                live.error = Some(error.clone());
                self.record_visible(&id, "error", json!({"message":error}));
                self.set_runtime_status(&id, "blocked");
            }
        }
        self.refresh_runtime_views(cx);
    }

    pub(super) fn answer_permission(&mut self, option: String, cx: &mut Context<Self>) {
        let Some(id) = self.current_id() else {
            return;
        };
        let Some(request) = self
            .runtime
            .conversations
            .get(&id)
            .and_then(|r| r.permission.front())
        else {
            return;
        };
        let Some(choice) = request.options.iter().find(|choice| {
            choice.option_id == option
                && matches!(
                    choice.kind.as_str(),
                    "allow_once" | "allow_always" | "reject_once"
                )
        }) else {
            return;
        };
        let request_id = request.request_id;
        let decision = json!({"request_id":request_id,"option_id":option,"kind":choice.kind,"name":choice.name});
        if !self.record_visible(&id, "permission_decision", decision) {
            self.refresh_runtime_views(cx);
            return;
        }
        if let Some(live) = self.runtime.conversations.get_mut(&id) {
            let result = live
                .driver
                .as_ref()
                .ok_or_else(|| "Agent is no longer running.".to_owned())
                .and_then(|driver| {
                    driver.send(acp::Command::Permission {
                        request_id,
                        option_id: option,
                    })
                });
            if let Err(error) = result {
                live.driver = None;
                live.processing = false;
                live.permission.clear();
                live.error = Some(error.clone());
                self.record_visible(&id, "error", json!({"message":error}));
                self.set_runtime_status(&id, "blocked");
                self.refresh_runtime_views(cx);
                return;
            }
            live.permission.pop_front();
            if live.permission.is_empty() {
                self.set_runtime_status(&id, "processing");
            }
        }
        self.refresh_runtime_views(cx);
    }

    pub(super) fn runtime_footer(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let thread = &self.workspace().threads[index];
        let mut content = col().w_full().gap_3().pb_4();
        let processing = self
            .runtime
            .conversations
            .get(&thread.id)
            .is_some_and(|live| live.processing);
        // The running turn's steps show live; finished turns fold them into
        // the summary under their reply.
        let turn = thread.messages.iter().rposition(|m| m.role == "user");
        if processing
            && let Some(turn) = turn
            && !config::current().modes.chats.hide_tool_calls
        {
            let steps: Vec<_> = thread.turn_tools(turn).collect();
            if !steps.is_empty() {
                content = content.child(chat_render::tool_steps(steps, cx));
            }
        }
        for activity in thread.activity.iter().filter(|a| a.kind == "error") {
            content = content.child(text(activity.title.clone(), 13., theme::foreground()));
        }
        if let Some(live) = self.runtime.conversations.get(&thread.id) {
            if live.processing && live.storage_failed {
                content = content.child(text(
                    "Stopping the turn before storage can be retried…",
                    13.,
                    theme::muted_foreground(),
                ));
            } else if live.processing && live.permission.is_empty() {
                content = content.child(text(
                    live.progress.clone().unwrap_or_else(|| "Thinking…".into()),
                    13.,
                    theme::muted_foreground(),
                ));
            }
            if let Some(request) = live.permission.front() {
                let mut options = row().flex_wrap().gap_2();
                for option in &request.options {
                    if option.kind == "reject_always" {
                        continue;
                    }
                    let label = match option.kind.as_str() {
                        "allow_once" => format!("Allow once: {}", option.name),
                        "allow_always" => {
                            format!("{} (harness remembers this choice)", option.name)
                        }
                        "reject_once" => format!("Deny once: {}", option.name),
                        _ => continue,
                    };
                    let action = Action::PermissionResponse(option.option_id.clone());
                    let button = Button::new(SharedString::from(format!(
                        "permission-{}",
                        option.option_id
                    )))
                    .small()
                    .label(label)
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.act(action.clone(), window, cx);
                    }));
                    options = options.child(if option.kind == "allow_once" {
                        button.primary()
                    } else {
                        button.outline()
                    });
                }
                content = content.child(
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
                        .child(options),
                );
            }
            if let Some(error) = &live.error {
                content = content.child(text(error.clone(), 13., theme::foreground()));
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
                && !live.shutting_down
                && !live.processing
                && !live.recovering_storage
                && !live.last_prompt.is_empty()
                && !matches!(thread.status.as_str(), "completed" | "archived")
            {
                content =
                    content.child(self.button("retry-prompt", "Retry", Action::RetryPrompt, cx));
            }
            if live.shutting_down {
                content = content.child(text(
                    "Waiting for the agent to stop…",
                    13.,
                    theme::muted_foreground(),
                ));
            }
            if live.shutdown_stuck {
                content =
                    content.child(self.button("force-stop", "Force Stop", Action::ForceStop, cx));
            }
        }
        content.into_any_element()
    }
}
