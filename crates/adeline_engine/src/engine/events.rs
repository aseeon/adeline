//! Handling events from agent workers.
use super::{After, Engine, INTERRUPTED, PERMISSION_STOPPED, Stop, phase};
use crate::{acp, storage};
use adeline_core::{
    conversation::{PermissionOption, StopReason, ThreadStatus},
    data::ToolReport,
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
        if let acp::EventKind::Traffic(entry) = event.kind {
            self.traffic(&id, &entry);
            return;
        }
        let Some(agent) = self
            .lifecycles
            .get_mut(&id)
            .and_then(|lifecycle| lifecycle.phase.agent_mut())
        else {
            return;
        };
        if event.turn < agent.turn {
            return;
        }
        agent.turn = event.turn;
        let agent_closing = agent.closing.is_some();
        let was_processing = self.processing(&id);
        let closing = agent_closing
            || self.stop_all.is_some()
            || self.status_of(&id).is_some_and(ThreadStatus::closed);
        match event.kind {
            acp::EventKind::Traffic(_) | acp::EventKind::Probed => return,
            acp::EventKind::Session {
                session_id,
                replaced,
            } => self.session_ready(&id, &session_id, replaced),
            acp::EventKind::Agent { features, .. } => {
                self.live_mut(&id).features = features.clone();
                self.save_session_state(&id, |settings| settings.features = features);
            }
            acp::EventKind::Options(options) => {
                let live = self.live_mut(&id);
                if live.options != options {
                    live.options.clone_from(&options);
                    self.save_session_state(&id, |settings| settings.options = options);
                }
            }
            acp::EventKind::Commands(commands) => {
                self.live_mut(&id).commands.clone_from(&commands);
                self.save_session_state(&id, |settings| settings.commands = commands);
            }
            acp::EventKind::Todo(todo) => {
                self.live_mut(&id).todo.clone_from(&todo);
                self.save_session_state(&id, |settings| settings.todo = todo);
            }
            kind @ (acp::EventKind::Title(_)
            | acp::EventKind::Text { .. }
            | acp::EventKind::Thought { .. }
            | acp::EventKind::Tool { .. }
            | acp::EventKind::Usage { .. }
            | acp::EventKind::OpenUrl(_)) => {
                self.agent_output(id, kind);
                return;
            }
            acp::EventKind::Permission { .. }
            | acp::EventKind::Retrying { .. }
            | acp::EventKind::ReplacementRequired(_)
                if closing =>
            {
                return;
            }
            acp::EventKind::Permission {
                request_id,
                title,
                options,
            } => self.permission_requested(&id, request_id, title, options),
            acp::EventKind::Steered { delivered } => self.steered(&id, delivered),
            acp::EventKind::SkippedServers(names) => self.skipped_servers(&id, names),
            acp::EventKind::Auth(_) => {}
            acp::EventKind::Crashed(stderr) => self.live_mut(&id).stderr = stderr,
            acp::EventKind::Retrying {
                attempt,
                limit,
                error,
            } => self.retrying(&id, attempt, limit, error),
            acp::EventKind::Error { message, kind } => {
                self.turn_failed(&id, message, kind, closing);
            }
            acp::EventKind::Finished { stop_reason } => {
                self.turn_finished(&id, stop_reason, closing);
            }
            acp::EventKind::Stopped => self.turn_stopped(&id, closing),
            acp::EventKind::ShutdownStuck => self.shutdown_stuck(&id),
            acp::EventKind::ShutdownComplete => {
                self.shutdown_complete(&id);
                return;
            }
            acp::EventKind::TextCopy => self.text_copy(&id),
            acp::EventKind::ReplacementRequired(error) => self.replacement_required(&id, &error),
        }
        self.send_live(&id);
        if was_processing && !self.processing(&id) && self.subscribers() == 0 && !self.daemon() {
            self.shutdown_conversation(&id);
            self.send_live(&id);
        }
    }

    fn session_ready(&mut self, id: &str, session_id: &str, replaced: bool) {
        let saved = self.conversation_settings(id).is_some_and(|mut settings| {
            if replaced && let Some(old) = settings.session_id.take() {
                settings.previous_session_ids.push(old);
            }
            settings.session_id = Some(session_id.to_owned());
            self.save_conversation_settings(id, settings)
        });
        if saved {
            self.record_visible(
                id,
                "lifecycle",
                json!({"event":"session_ready","session_id":session_id,"replacement":replaced}),
            );
        }
    }

    fn agent_output(&mut self, id: String, kind: acp::EventKind) {
        if matches!(
            kind,
            acp::EventKind::Text { .. }
                | acp::EventKind::Thought { .. }
                | acp::EventKind::Tool { .. }
        ) && self
            .lifecycle(&id)
            .phase
            .turn_mut()
            .is_some_and(|turn| turn.retry.take().is_some())
        {
            self.send_live(&id);
        }
        match kind {
            acp::EventKind::Title(title) => {
                self.save_session_state(&id, |settings| title.clone_into(&mut settings.title));
                self.broadcast(Delta::Title { id, title });
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
            }
            acp::EventKind::Thought { message, text } => {
                self.queue_visible(&id, "thought_chunk", json!({"text":text,"message":message}));
                self.broadcast(Delta::Thought {
                    id,
                    text,
                    message,
                    at: adeline_core::recency::now_ms(),
                });
            }
            acp::EventKind::Tool {
                id: tool_id,
                title,
                status,
                detail,
                kind,
                paths,
            } => {
                let report = ToolReport {
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
                    id: id.clone(),
                    report: report.clone(),
                    at: adeline_core::recency::now_ms(),
                });
                self.tool_cards(&id, &report);
            }
            acp::EventKind::Usage { used, size } => {
                self.queue_visible(&id, "usage", json!({"used":used,"size":size}));
                self.broadcast(Delta::Usage { id, used, size });
            }
            acp::EventKind::OpenUrl(url) => {
                self.broadcast_message(&EngineMessage::Delta(Delta::OpenUrl { url }));
            }
            _ => unreachable!("agent_output only gets streamed agent output"),
        }
    }

    fn shutdown_stuck(&mut self, id: &str) {
        self.shutdown_conversation(id);
        if let Some(agent) = self.lifecycle(id).phase.agent_mut() {
            agent.closing.get_or_insert_default().stuck = true;
        }
        self.live_mut(id).error =
            Some("Agent shutdown is stuck. Force Stop terminates this process.".into());
    }

    fn text_copy(&mut self, id: &str) {
        self.lifecycle(id).text_copy = true;
        if self.record_visible(id, "fork_text_copy", json!({})) {
            self.broadcast_thread(id);
        }
    }

    fn skipped_servers(&mut self, id: &str, names: Vec<String>) {
        let agent = self
            .live
            .get(id)
            .and_then(|live| live.execution.as_ref())
            .map_or_else(|| "This agent".to_owned(), |e| e.name.clone());
        for name in names {
            let message = format!("Skipped {name}: {agent} doesn't support HTTP servers.");
            self.record_visible(id, "note", json!({"message":message}));
        }
        self.broadcast_thread(id);
    }

    fn retrying(&mut self, id: &str, attempt: u32, limit: u32, error: String) {
        if self.record_visible(
            id,
            "error",
            json!({"message":error,"attempt":attempt,"limit":limit}),
        ) && let Some(turn) = self.lifecycle(id).phase.turn_mut()
        {
            turn.retry = Some(phase::Retry {
                attempt,
                limit,
                error,
            });
        }
    }

    fn replacement_required(&mut self, id: &str, error: &str) {
        if self.record_visible(id, "error", json!({"message":error,"restoration":false})) {
            let lifecycle = self.lifecycle(id);
            lifecycle.end();
            lifecycle.block(After::Replacement);
            self.live_mut(id).error = Some(format!(
                "{error} Start a new session using saved messages only if you want to continue."
            ));
        }
    }

    fn permission_requested(
        &mut self,
        id: &str,
        request_id: u64,
        title: String,
        options: Vec<PermissionOption>,
    ) {
        let asking = |engine: &Self| {
            engine
                .lifecycles
                .get(id)
                .and_then(|lifecycle| lifecycle.phase.turn())
                .is_some_and(|turn| turn.stop.is_none())
        };
        if !asking(self)
            || !self.record_visible(
                id,
                "permission_request",
                json!({"request_id":request_id,"title":title,"options":options}),
            )
        {
            return;
        }
        if let Some(turn) = self.lifecycle(id).phase.turn_mut() {
            turn.permission.push(PendingPermission {
                request_id,
                title,
                options,
            });
        }
        if self.subscribers() == 0 {
            self.stop_for_permission(id);
        }
    }

    fn turn_failed(&mut self, id: &str, message: String, kind: acp::FailureKind, closing: bool) {
        let saved = self.record_visible(
            id,
            "error",
            json!({"message":message,"kind":format!("{kind:?}")}),
        );
        let lifecycle = self.lifecycle(id);
        if lifecycle.end().is_some() || !closing {
            lifecycle.block(if kind == acp::FailureKind::Authentication {
                After::LoginRequired
            } else {
                After::Blocked
            });
        }
        if saved {
            self.live_mut(id).error = Some(message);
        }
        if std::mem::take(&mut self.lifecycle(id).text_copy) {
            self.drop_undelivered_fork_session(id);
        }
    }

    fn turn_finished(&mut self, id: &str, stop_reason: StopReason, closing: bool) {
        self.lifecycle(id).text_copy = false;
        self.record_visible(
            id,
            "lifecycle",
            json!({"event":"turn_finished","reason":stop_reason}),
        );
        let lifecycle = self.lifecycle(id);
        let recovered = lifecycle.recovering();
        lifecycle.end();
        if recovered {
            self.live_mut(id).error = None;
        }
        if !closing && !self.lifecycle(id).storage_failed {
            self.send_queue(id);
        }
    }

    fn turn_stopped(&mut self, id: &str, closing: bool) {
        let saved = self.record_visible(id, "lifecycle", json!({"event":"turn_cancelled"}));
        let Some(turn) = self.lifecycle(id).end() else {
            return;
        };
        let stop = turn.stop.unwrap_or(Stop::User);
        self.turn_cut(id, stop, saved, false);
        if stop == Stop::SendQueued && saved && !closing && !self.lifecycle(id).storage_failed {
            self.send_queue(id);
        }
    }

    fn turn_cut(&mut self, id: &str, stop: Stop, saved: bool, gone: bool) {
        let closed = self.status_of(id).is_some_and(ThreadStatus::closed);
        let lifecycle = self.lifecycle(id);
        let recovered = saved && !lifecycle.storage_failed;
        if !matches!(stop, Stop::User | Stop::SendQueued) {
            lifecycle.block(After::Blocked);
        }
        let live = self.live_mut(id);
        match stop {
            Stop::Permission => {
                live.worked = true;
                live.error = Some(PERMISSION_STOPPED.into());
            }
            Stop::Storage if recovered && gone && !closed => {
                live.error =
                    Some("Agent stopped. Retry to continue from the saved session.".into());
            }
            Stop::Storage if recovered && !gone => {
                live.error = (!live.last_prompt.is_empty())
                    .then(|| "History is saved. Retry to continue the interrupted turn.".into());
            }
            Stop::Shutdown if !closed => live.error = Some(INTERRUPTED.into()),
            _ => {}
        }
    }

    fn shutdown_complete(&mut self, id: &str) {
        let saved = self.record_visible(id, "lifecycle", json!({"event":"process_stopped"}));
        self.agent_gone(id, saved);
        self.send_live(id);
        self.finish_deletions();
        self.finish_stop_all();
    }

    pub(super) fn agent_gone(&mut self, id: &str, saved: bool) {
        let Some((agent, turn)) = self.lifecycle(id).gone() else {
            return;
        };
        if let Some((text, files)) = agent.steering {
            self.queue(id, text, files, true);
        }
        self.last_traffic.remove(id);
        let closing = agent.closing.unwrap_or_default();
        if closing.stuck && saved && !self.lifecycle(id).storage_failed {
            self.live_mut(id).error = None;
        }
        let stop = turn.map(|turn| turn.stop.unwrap_or(Stop::Shutdown));
        if let Some(stop) = stop {
            self.turn_cut(id, stop, saved, true);
        }
        if self.cannot_turn(id).is_some() || self.lifecycle(id).storage_failed {
            return;
        }
        if let Some(prompt) = closing.restart {
            let files = self.attachments.get(id).cloned().unwrap_or_default();
            self.start_prompt(id, prompt, files, true);
        } else if stop == Some(Stop::SendQueued) {
            self.send_queue(id);
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
                self.live_mut(id).quiet_since = None;
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
        if self
            .lifecycles
            .get(id)
            .and_then(|lifecycle| lifecycle.phase.turn())
            .is_none_or(|turn| turn.permission.is_empty())
        {
            return;
        }
        tracing::event!(
            name: "engine.permission.unanswerable",
            tracing::Level::INFO,
            conversation.id = id,
            "Stopping conversation {{conversation.id}}: it needs a permission and no client is connected"
        );
        self.record_visible(
            id,
            "error",
            json!({"message":PERMISSION_STOPPED,"permission":true}),
        );
        self.stop(id, Stop::Permission);
    }
}
