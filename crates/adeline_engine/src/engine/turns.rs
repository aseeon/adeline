//! Turns: sending, queueing, starting, stopping and retrying prompts.
use super::{ALREADY_PROCESSING, Engine, history_text, live_for};
use crate::acp;
use adeline_core::{
    conversation::{self, Attachment, MessageRole, ThreadStatus},
    data::Message,
    protocol::{Delta, Queued},
};
use serde_json::{Value, json};
use tokio::task::block_in_place;

impl Engine {
    pub(super) fn send(
        &mut self,
        project_id: &str,
        conversation_id: Option<String>,
        agent_id: Option<&str>,
        prompt: &str,
        attachments: Vec<Attachment>,
        now: bool,
    ) -> Result<String, String> {
        if self.stop_all.is_some() {
            return Err("The conversation engine is stopping its agents.".into());
        }
        if self.deleting.contains_key(project_id) {
            return Err("This project is being deleted.".into());
        }
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() && attachments.is_empty() {
            return Err("Enter a message to send.".into());
        }
        // Files picked on this machine are read here, so their size is known here.
        let attachments: Vec<Attachment> = attachments
            .into_iter()
            .map(|mut file| {
                if let Some(path) = &file.path
                    && file.data.is_empty()
                    && let Ok(metadata) = block_in_place(|| std::fs::metadata(path))
                {
                    file.size = metadata.len();
                }
                file
            })
            .collect();
        if let Some(error) = self.refuse_attachments(conversation_id.as_deref(), &attachments) {
            return Err(error);
        }
        let id = if let Some(id) = conversation_id {
            let Some(thread) = self.locate(&id) else {
                return Err("This conversation no longer exists.".into());
            };
            if thread.status == ThreadStatus::Archived {
                return Err("This conversation is archived.".into());
            }
            let live = self.live.entry(id.clone()).or_default();
            if live.storage_failed || live.recovering_storage {
                return Err("Retry storage before sending.".into());
            }
            if live.replacement {
                return Err("Start a replacement session before sending.".into());
            }
            // The composer is never blocked by a running turn.
            if live.processing {
                if now {
                    self.send_now(&id, prompt, attachments);
                } else {
                    self.queue(&id, prompt, attachments, false);
                }
                self.send_live(&id);
                return Ok(id);
            }
            if live.shutting_down {
                return Err("Wait for the agent to stop before sending.".into());
            }
            id
        } else {
            let agent = agent_id
                .and_then(|id| self.agents.entries.iter().find(|entry| entry.id == id))
                .map(|entry| entry.definition.clone())
                .ok_or("Create or select an agent before sending.")?;
            let launch = self
                .harnesses
                .launch(&agent.harness, &agent.command, &agent.arguments)
                .map_err(|e| format!("{e:#}"))?;
            let mut servers = self.settings.mcp_servers.clone();
            servers.extend(agent.mcp_servers.iter().cloned());
            let title = conversation::title_from(&prompt);
            let store = &mut self.store;
            let (id, thread, live) = block_in_place(|| {
                let id = store
                    .create_conversation(project_id, &agent, launch, servers, &title)
                    .map_err(|e| format!("{e:#}"))?;
                let saved = store
                    .conversation(&id)
                    .ok_or_else(|| "Created conversation is missing.".to_owned())?;
                Ok::<_, String>((id, saved.to_thread(), live_for(saved)))
            })?;
            self.live.insert(id.clone(), live);
            self.broadcast(Delta::Thread {
                project_id: project_id.to_owned(),
                thread,
            });
            id
        };
        if let Some(error) = self.cannot_start(&id) {
            self.send_live(&id);
            return Err(error);
        }
        self.deliver(&id, prompt, attachments);
        self.send_live(&id);
        Ok(id)
    }

    fn deliver(&mut self, id: &str, prompt: String, attachments: Vec<Attachment>) {
        prompt.clone_into(&mut self.live.entry(id.to_owned()).or_default().last_prompt);
        let saved = self.user_message(id, prompt.clone(), attachments.clone());
        self.attachments.insert(id.to_owned(), attachments.clone());
        if saved {
            self.start_prompt(id, prompt, attachments, false);
        }
    }

    pub(super) fn queue(&mut self, id: &str, text: String, files: Vec<Attachment>, first: bool) {
        self.next_queued += 1;
        let queued = Queued {
            id: self.next_queued,
            text,
            files: files.iter().map(|file| file.name.clone()).collect(),
        };
        self.queued_files.insert(queued.id, files);
        let list = &mut self.live.entry(id.to_owned()).or_default().queued;
        if first {
            list.insert(0, queued);
        } else {
            list.push(queued);
        }
    }

    pub(super) fn send_now(&mut self, id: &str, text: String, files: Vec<Attachment>) {
        let steering = self
            .live
            .get(id)
            .is_some_and(|live| live.features.steering && live.processing);
        if steering
            && !self.steering.contains_key(id)
            && let Some(slot) = self.drivers.get(id)
        {
            let mut prompt = vec![acp::Part::Text(text.clone())];
            prompt.extend(files.iter().cloned().map(acp::Part::File));
            if slot.driver.send(acp::Command::Steer { prompt }).is_ok() {
                self.steering.insert(id.to_owned(), (text, files));
                return;
            }
        }
        // The message goes first, then the cancel settles and the queue is sent.
        self.queue(id, text, files, true);
        self.send_after_stop.insert(id.to_owned());
        self.stop(id);
    }

    pub(super) fn user_message(
        &mut self,
        id: &str,
        text: String,
        attachments: Vec<Attachment>,
    ) -> bool {
        let message = Message {
            role: MessageRole::User,
            text,
            read: true,
            created_at: adeline_core::recency::now().to_string(),
            attachments,
            ..Default::default()
        };
        let saved = self.record_visible(
            id,
            "message",
            serde_json::to_value(&message).unwrap_or_default(),
        );
        self.broadcast(Delta::Message {
            id: id.to_owned(),
            message,
        });
        saved
    }

    pub(super) fn take_queued_parts(
        &mut self,
        id: &str,
        queued: u64,
    ) -> Result<(String, Vec<Attachment>), String> {
        let live = self.live.entry(id.to_owned()).or_default();
        let index = live
            .queued
            .iter()
            .position(|item| item.id == queued)
            .ok_or("That message was already sent.")?;
        let item = live.queued.remove(index);
        let files = self.queued_files.remove(&queued).unwrap_or_default();
        Ok((item.text, files))
    }

    pub(super) fn take_queued(&mut self, id: &str, queued: u64) -> Result<Value, String> {
        let (text, _) = self.take_queued_parts(id, queued)?;
        self.send_live(id);
        Ok(Value::String(text))
    }

    pub(super) fn send_queue(&mut self, id: &str) {
        let queued = std::mem::take(&mut self.live.entry(id.to_owned()).or_default().queued);
        if queued.is_empty() {
            return;
        }
        let text = queued
            .iter()
            .map(|item| item.text.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let files = queued
            .iter()
            .flat_map(|item| self.queued_files.remove(&item.id).unwrap_or_default())
            .collect();
        self.deliver(id, text, files);
    }

    pub(super) fn start_prompt(
        &mut self,
        id: &str,
        prompt: String,
        attachments: Vec<Attachment>,
        retry: bool,
    ) {
        if self.cannot_start(id).is_some() || !self.set_runtime_status(id, ThreadStatus::Processing)
        {
            return;
        }
        if !self.ensure_driver(id) {
            let error = "Could not load this conversation's saved agent settings.".to_owned();
            self.record_visible(id, "error", json!({"message":error}));
            self.live.entry(id.to_owned()).or_default().error = Some(error);
            self.set_runtime_status(id, ThreadStatus::Blocked);
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
        let live = self.live.entry(id.to_owned()).or_default();
        if live.storage_failed {
            return;
        }
        live.processing = true;
        live.error = None;
        live.stderr.clear();
        live.auth_required = false;
        live.progress = None;
        live.assistant = None;
        live.quiet_since = None;
        live.permission.clear();
        if !retry {
            live.worked = false;
            live.last_prompt.clone_from(&prompt);
        }
        self.last_traffic
            .insert(id.to_owned(), adeline_core::recency::now_ms());
        if let Some((session, context)) = self.fork_history(id)
            && let Some(slot) = self.drivers.get(id)
        {
            let _ = slot.driver.send(acp::Command::Fork { session, context });
        }
        let retries = u32::try_from(self.settings.retry_limit).unwrap_or(u32::MAX);
        let mut parts = Vec::new();
        if !prompt.is_empty() {
            parts.push(acp::Part::Text(prompt));
        }
        parts.extend(attachments.into_iter().map(acp::Part::File));
        // The worker numbers turns; a prompt it rejects keeps the old number.
        let Some(slot) = self.drivers.get_mut(id) else {
            return;
        };
        if let Err(error) = slot.driver.send(acp::Command::Prompt {
            prompt: parts,
            retries,
        }) {
            self.drivers.remove(id);
            let live = self.live.entry(id.to_owned()).or_default();
            live.processing = false;
            live.error = Some(error.clone());
            self.record_visible(id, "error", json!({"message":error}));
            self.set_runtime_status(id, ThreadStatus::Blocked);
        }
    }

    pub(super) fn stop(&mut self, id: &str) {
        let Some(live) = self.live.get_mut(id) else {
            return;
        };
        if !live.processing {
            return;
        }
        live.permission.clear();
        live.progress = Some("Stopping…".into());
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::Cancel);
        }
        self.send_live(id);
    }

    fn cannot_turn(&self, id: &str) -> Option<String> {
        if self.stop_all.is_some() {
            return Some("The conversation engine is stopping its agents.".into());
        }
        if self
            .project_of(id)
            .is_some_and(|project| self.deleting.contains_key(&project))
        {
            return Some("This project is being deleted.".into());
        }
        match self.status_of(id)? {
            ThreadStatus::Completed => Some("Already completed".into()),
            ThreadStatus::Archived => Some("This conversation is archived.".into()),
            _ => None,
        }
    }

    pub(super) fn retry(&mut self, id: &str) -> Result<Value, String> {
        if let Some(error) = self.cannot_turn(id) {
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        if live.processing {
            return Err(ALREADY_PROCESSING.into());
        }
        if live.storage_failed || live.recovering_storage || live.replacement {
            return Err("This conversation can't retry right now.".into());
        }
        let worked = live.worked;
        let prompt = if worked {
            acp::CONTINUE_TURN.to_owned()
        } else {
            live.last_prompt.clone()
        };
        let files = if worked {
            Vec::new()
        } else {
            self.attachments.get(id).cloned().unwrap_or_default()
        };
        if prompt.is_empty() && files.is_empty() {
            return Err("There is nothing to retry.".into());
        }
        if let Some(error) = self.cannot_start(id) {
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        if live.shutting_down && !live.shutdown_stuck {
            // An agent closed in the background; the turn starts once it has exited.
            live.processing = true;
            live.progress = Some("Restarting the agent…".into());
            self.restart_prompt.insert(id.to_owned(), prompt);
        } else if live.shutting_down {
            return Err("Agent shutdown is stuck. Use Force Stop first.".into());
        } else {
            self.start_prompt(id, prompt, files, true);
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    pub(super) fn retry_storage(&mut self, id: &str) -> Result<Value, String> {
        if self.live.get(id).is_none_or(|live| !live.storage_failed) {
            return Err("History is already saved.".into());
        }
        let store = &mut self.store;
        let result = block_in_place(|| {
            store.retry_unsaved(id).map_err(|e| format!("{e:#}"))?;
            store
                .conversation(id)
                .map(|conversation| {
                    let worked = conversation
                        .events
                        .iter()
                        .chain(&conversation.unsaved_events)
                        .rev()
                        .take_while(|event| event.kind != "message" || event.data["role"] != "user")
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
        let (settings, thread, recovered_work) = match result {
            Ok(result) => result,
            Err(error) => {
                self.live.entry(id.to_owned()).or_default().error = Some(error.clone());
                self.send_live(id);
                return Err(error);
            }
        };
        if let Some(project_id) = self.project_of(id) {
            self.broadcast(Delta::Thread { project_id, thread });
        }
        let live = self.live.entry(id.to_owned()).or_default();
        live.storage_failed = false;
        live.worked |= recovered_work;
        live.recovering_storage = live.processing;
        live.error = if live.recovering_storage {
            Some("History is saved. Waiting for the stopped turn to settle.".into())
        } else if !settings.status.closed() && !live.last_prompt.is_empty() {
            Some("History is saved. Retry to continue the interrupted turn.".into())
        } else {
            None
        };
        // The cancelled turn settles by itself; with no agent there is nothing to wait for.
        if !self.drivers.contains_key(id) {
            let live = self.live.entry(id.to_owned()).or_default();
            live.processing = false;
            live.recovering_storage = false;
        }
        if !settings.status.closed() {
            self.set_runtime_status(id, ThreadStatus::Blocked);
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    pub(super) fn replace_session(&mut self, id: &str) -> Result<Value, String> {
        if let Some(error) = self.cannot_turn(id) {
            return Err(error);
        }
        if !self
            .live
            .get(id)
            .is_some_and(|live| live.replacement && !live.storage_failed && !live.shutting_down)
        {
            return Err("This conversation doesn't need a replacement session.".into());
        }
        let context = self
            .locate(id)
            .map(|thread| history_text(&thread.messages))
            .unwrap_or_default();
        if !self.set_runtime_status(id, ThreadStatus::Processing) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        if !self.ensure_driver(id) {
            self.set_runtime_status(id, ThreadStatus::Blocked);
            return Err("Could not load this conversation's saved agent settings.".into());
        }
        if !self.record_visible(
            id,
            "lifecycle",
            json!({"event":"replacement_session_approved"}),
        ) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        if let Some(slot) = self.drivers.get(id)
            && let Err(error) = slot.driver.send(acp::Command::ReplaceSession { context })
        {
            self.drivers.remove(id);
            self.live.entry(id.to_owned()).or_default().error = Some(error.clone());
            self.set_runtime_status(id, ThreadStatus::Blocked);
            self.send_live(id);
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        live.replacement = false;
        live.error = None;
        live.processing = true;
        live.assistant = None;
        self.send_live(id);
        Ok(Value::Null)
    }
}
