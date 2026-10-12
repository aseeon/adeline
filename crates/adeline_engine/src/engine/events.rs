//! Handling events from agent workers.
use super::{Engine, PERMISSION_STOPPED};
use crate::{acp, storage};
use adeline_core::{
    conversation::ThreadStatus,
    protocol::{Delta, EngineMessage, PendingPermission},
};
use serde_json::json;

impl Engine {
    fn save_session_state(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut storage::ConversationSettings),
    ) {
        if let Some(mut settings) = self.conversation_settings(id) {
            let before = settings.clone();
            change(&mut settings);
            if settings != before {
                self.save_conversation_settings(id, settings);
            }
        }
    }

    pub(super) fn driver_event(&mut self, event: acp::Event) {
        let id = event.conversation_id.clone();
        if self.locate(&id).is_none() {
            return;
        }
        // Traffic is the agent's, whatever turn it belongs to.
        if let acp::EventKind::Traffic(entry) = event.kind {
            self.traffic(&id, &entry);
            return;
        }
        let Some(slot) = self.drivers.get_mut(&id) else {
            return;
        };
        if event.turn < slot.turn {
            return;
        }
        slot.turn = event.turn;
        let was_processing = self.live.get(&id).is_some_and(|l| l.processing);
        let closing = self.live.get(&id).is_some_and(|l| l.shutting_down)
            || self.stop_all.is_some()
            || self.status_of(&id).is_some_and(ThreadStatus::closed);
        match event.kind {
            acp::EventKind::Traffic(_) | acp::EventKind::Probed => return,
            acp::EventKind::Session {
                session_id,
                replaced,
            } => {
                let saved = self.conversation_settings(&id).is_some_and(|mut settings| {
                    if replaced && let Some(old) = settings.session_id.take() {
                        settings.previous_session_ids.push(old);
                    }
                    settings.session_id = Some(session_id.clone());
                    self.save_conversation_settings(&id, settings)
                });
                if saved {
                    self.record_visible(
                        &id,
                        "lifecycle",
                        json!({"event":"session_ready","session_id":session_id,"replacement":replaced}),
                    );
                }
            }
            acp::EventKind::Agent { features, .. } => {
                self.live.entry(id.clone()).or_default().features = features.clone();
                self.save_session_state(&id, |settings| settings.features = features);
            }
            acp::EventKind::Options(options) => {
                let live = self.live.entry(id.clone()).or_default();
                if live.options != options {
                    live.options.clone_from(&options);
                    self.save_session_state(&id, |settings| settings.options = options);
                }
            }
            acp::EventKind::Commands(commands) => {
                self.live
                    .entry(id.clone())
                    .or_default()
                    .commands
                    .clone_from(&commands);
                self.save_session_state(&id, |settings| settings.commands = commands);
            }
            acp::EventKind::Todo(todo) => {
                self.live
                    .entry(id.clone())
                    .or_default()
                    .todo
                    .clone_from(&todo);
                self.save_session_state(&id, |settings| settings.todo = todo);
            }
            acp::EventKind::Title(title) => {
                self.save_session_state(&id, |settings| title.clone_into(&mut settings.title));
                self.broadcast(Delta::Title {
                    id: id.clone(),
                    title,
                });
                return;
            }
            acp::EventKind::Text { message, text } => {
                self.queue_visible(
                    &id,
                    "assistant_chunk",
                    json!({"text":text,"message":message}),
                );
                self.broadcast(Delta::Text {
                    id,
                    text,
                    message,
                    at: adeline_core::recency::now_ms(),
                });
                return;
            }
            acp::EventKind::Thought { message, text } => {
                self.queue_visible(&id, "thought_chunk", json!({"text":text,"message":message}));
                self.broadcast(Delta::Thought {
                    id,
                    text,
                    message,
                    at: adeline_core::recency::now_ms(),
                });
                return;
            }
            acp::EventKind::Tool {
                id: tool_id,
                title,
                status,
                detail,
                kind,
                paths,
            } => {
                let report = adeline_core::data::ToolReport {
                    id: tool_id,
                    title,
                    status,
                    detail,
                    kind,
                    paths,
                };
                self.queue_visible(
                    &id,
                    "tool",
                    serde_json::to_value(&report).unwrap_or_default(),
                );
                self.broadcast(Delta::Tool {
                    id,
                    report,
                    at: adeline_core::recency::now_ms(),
                });
                return;
            }
            acp::EventKind::Usage { used, size } => {
                self.queue_visible(&id, "usage", json!({"used":used,"size":size}));
                self.broadcast(Delta::Usage { id, used, size });
                return;
            }
            acp::EventKind::Permission {
                request_id,
                title,
                options,
            } => {
                if closing {
                    return;
                }
                if self.record_visible(
                    &id,
                    "permission_request",
                    json!({"request_id":request_id,"title":title,"options":options}),
                ) {
                    self.live
                        .entry(id.clone())
                        .or_default()
                        .permission
                        .push(PendingPermission {
                            request_id,
                            title,
                            options,
                        });
                    self.set_runtime_status(&id, ThreadStatus::Blocked);
                    if self.subscribers() == 0 {
                        self.stop_for_permission(&id);
                    }
                }
            }
            acp::EventKind::Steered { delivered } => {
                if let Some((text, files)) = self.steering.remove(&id) {
                    if delivered {
                        self.live.entry(id.clone()).or_default().assistant = None;
                        self.user_message(&id, text, files);
                    } else {
                        // The agent didn't take it: it goes out when the turn ends.
                        self.queue(&id, text, files, true);
                        if !self.live.get(&id).is_some_and(|live| live.processing) {
                            self.send_queue(&id);
                        }
                    }
                }
            }
            acp::EventKind::SkippedServers(names) => {
                let agent = self
                    .live
                    .get(&id)
                    .and_then(|live| live.execution.as_ref())
                    .map_or_else(|| "This agent".to_owned(), |e| e.name.clone());
                for name in names {
                    let message = format!("Skipped {name}: {agent} doesn't support HTTP servers.");
                    self.record_visible(&id, "note", json!({"message":message}));
                }
                if let Some((project_id, thread)) = self.thread_from_store(&id) {
                    self.broadcast(Delta::Thread { project_id, thread });
                }
            }
            acp::EventKind::OpenUrl(url) => {
                self.broadcast_message(&EngineMessage::Delta(Delta::OpenUrl { url }));
                return;
            }
            acp::EventKind::Auth(_) => {}
            acp::EventKind::Crashed(stderr) => {
                self.live.entry(id.clone()).or_default().stderr = stderr;
            }
            acp::EventKind::Retrying {
                attempt,
                limit,
                error,
            } => {
                if closing {
                    return;
                }
                if self.record_visible(
                    &id,
                    "error",
                    json!({"message":error,"attempt":attempt,"limit":limit}),
                ) {
                    self.live.entry(id.clone()).or_default().progress =
                        Some(format!("Retry {attempt} of {limit}: {error}"));
                }
            }
            acp::EventKind::Error { message, kind } => {
                let saved = self.record_visible(
                    &id,
                    "error",
                    json!({"message":message,"kind":format!("{kind:?}")}),
                );
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.progress = None;
                live.permission.clear();
                live.auth_required = kind == acp::FailureKind::Authentication;
                if saved {
                    live.error = Some(message);
                    if !closing {
                        self.set_runtime_status(&id, ThreadStatus::Blocked);
                    }
                }
                if self.text_copy_pending.remove(&id) {
                    self.drop_undelivered_fork_session(&id);
                }
            }
            acp::EventKind::Finished { stop_reason } => {
                self.text_copy_pending.remove(&id);
                self.record_visible(
                    &id,
                    "lifecycle",
                    json!({"event":"turn_finished","reason":stop_reason}),
                );
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.progress = None;
                live.quiet_since = None;
                live.permission.clear();
                if !closing && !live.storage_failed {
                    self.set_runtime_status(&id, ThreadStatus::Idle);
                    self.send_queue(&id);
                }
            }
            acp::EventKind::Stopped => {
                let saved =
                    self.record_visible(&id, "lifecycle", json!({"event":"turn_cancelled"}));
                let for_permission = self.permission_stopped.remove(&id);
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.permission.clear();
                live.progress = None;
                live.quiet_since = None;
                if for_permission {
                    live.worked = true;
                    live.error = Some(PERMISSION_STOPPED.into());
                    if !closing && !live.storage_failed {
                        self.set_runtime_status(&id, ThreadStatus::Blocked);
                    }
                } else if live.recovering_storage {
                    live.recovering_storage = false;
                    if saved {
                        live.error = (!live.last_prompt.is_empty()).then(|| {
                            "History is saved. Retry to continue the interrupted turn.".into()
                        });
                        if !closing && !live.storage_failed {
                            self.set_runtime_status(&id, ThreadStatus::Blocked);
                        }
                    }
                } else if saved && !closing && !live.storage_failed {
                    self.set_runtime_status(&id, ThreadStatus::Idle);
                    if self.send_after_stop.remove(&id) {
                        self.send_queue(&id);
                    }
                }
            }
            acp::EventKind::ShutdownStuck => {
                let live = self.live.entry(id.clone()).or_default();
                live.shutdown_stuck = true;
                live.error =
                    Some("Agent shutdown is stuck. Force Stop terminates this process.".into());
            }
            acp::EventKind::ShutdownComplete => {
                let saved =
                    self.record_visible(&id, "lifecycle", json!({"event":"process_stopped"}));
                self.drivers.remove(&id);
                self.steering.remove(&id);
                self.last_traffic.remove(&id);
                let finished = self.status_of(&id).is_some_and(ThreadStatus::closed);
                let live = self.live.entry(id.clone()).or_default();
                live.processing = false;
                live.shutting_down = false;
                live.quiet_since = None;
                if live.shutdown_stuck && saved && !live.storage_failed {
                    live.error = None;
                }
                live.shutdown_stuck = false;
                live.permission.clear();
                if live.recovering_storage {
                    live.recovering_storage = false;
                    if saved && !live.storage_failed && !finished {
                        live.error =
                            Some("Agent stopped. Retry to continue from the saved session.".into());
                    }
                }
                if let Some(prompt) = self.restart_prompt.remove(&id)
                    && self.stop_all.is_none()
                {
                    let files = self.attachments.get(&id).cloned().unwrap_or_default();
                    self.start_prompt(&id, prompt, files, true);
                } else if self.send_after_stop.remove(&id) && self.stop_all.is_none() {
                    self.send_queue(&id);
                }
                self.send_live(&id);
                self.finish_deletions();
                self.finish_stop_all();
                return;
            }
            acp::EventKind::TextCopy => {
                self.text_copy_pending.insert(id.clone());
                if self.record_visible(&id, "fork_text_copy", json!({}))
                    && let Some((project_id, thread)) = self.thread_from_store(&id)
                {
                    self.broadcast(Delta::Thread { project_id, thread });
                }
            }
            acp::EventKind::ReplacementRequired(error) => {
                if closing {
                    return;
                }
                if self.record_visible(&id, "error", json!({"message":error,"restoration":false})) {
                    let live = self.live.entry(id.clone()).or_default();
                    live.replacement = true;
                    live.processing = false;
                    live.error = Some(format!(
                        "{error} Start a new session using saved messages only if you want to continue."
                    ));
                    self.set_runtime_status(&id, ThreadStatus::Blocked);
                }
            }
        }
        self.send_live(&id);
        // With no client connected, a finished turn's agent closes.
        let processing = self.live.get(&id).is_some_and(|l| l.processing);
        if was_processing && !processing && self.subscribers() == 0 && !self.daemon() {
            self.shutdown_conversation(&id);
            self.send_live(&id);
        }
    }

    fn traffic(&mut self, id: &str, entry: &adeline_core::conversation::TrafficEntry) {
        self.queue_visible(
            id,
            "traffic",
            serde_json::to_value(entry).unwrap_or_default(),
        );
        if entry.direction != adeline_core::conversation::Direction::ToAgent {
            self.last_traffic.insert(id.to_owned(), entry.at);
            if self
                .live
                .get(id)
                .is_some_and(|live| live.quiet_since.is_some())
            {
                self.live.entry(id.to_owned()).or_default().quiet_since = None;
                self.send_live(id);
            }
        }
        let watchers: Vec<u64> = self
            .watching
            .iter()
            .filter(|(_, watched)| *watched == id)
            .map(|(client, _)| *client)
            .collect();
        for client in watchers {
            self.send_to(
                client,
                &EngineMessage::Traffic {
                    id: id.to_owned(),
                    entry: entry.clone(),
                },
            );
        }
    }

    pub(super) fn stop_for_permission(&mut self, id: &str) {
        let Some(live) = self.live.get_mut(id) else {
            return;
        };
        if live.permission.is_empty() {
            return;
        }
        live.permission.clear();
        live.progress = Some("Stopping…".into());
        tracing::event!(
            name: "engine.permission.unanswerable",
            tracing::Level::INFO,
            conversation.id = id,
            "Stopping conversation {{conversation.id}}: it needs a permission and no client is connected"
        );
        self.permission_stopped.insert(id.to_owned());
        self.record_visible(
            id,
            "error",
            json!({"message":PERMISSION_STOPPED,"permission":true}),
        );
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::Cancel);
        }
        self.send_live(id);
    }
}
