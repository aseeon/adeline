//! A conversation's agent: starting and stopping its driver, forks, option
//! changes, permission answers and read markers.
use super::*;

impl Engine {
    pub(super) fn thread_from_store(&self, id: &str) -> Option<(String, Thread)> {
        self.store.projects.iter().find_map(|p| {
            p.conversations
                .iter()
                .find(|c| c.id == id)
                .map(|c| (p.id.clone(), c.to_thread()))
        })
    }

    pub(super) fn ensure_driver(&mut self, id: &str) -> bool {
        if self.drivers.contains_key(id) {
            return true;
        }
        let Some(settings) = self.conversation_settings(id) else {
            return false;
        };
        let driver = acp::Driver::spawn(
            id.to_owned(),
            settings.execution,
            settings.session_id,
            self.events.clone(),
        );
        self.drivers.insert(id.to_owned(), Slot { driver, turn: 0 });
        self.live.entry(id.to_owned()).or_default().running = true;
        true
    }

    pub(super) fn cannot_start(&self, id: &str) -> Option<String> {
        let execution = self.live.get(id)?.execution.as_ref()?;
        let command = Path::new(&execution.command);
        (command.is_absolute() && !command.is_file()).then(|| {
            format!(
                "The {} harness is not installed.",
                self.harnesses
                    .label(&execution.harness, &execution.identity)
            )
        })
    }

    pub(super) fn refuse_attachments(
        &self,
        id: Option<&str>,
        attachments: &[Attachment],
    ) -> Option<String> {
        for file in attachments {
            if file.size > crate::conversation::ATTACHMENT_LIMIT {
                return Some(format!(
                    "{} is {} MB. Files over {} MB can't be attached.",
                    file.name,
                    file.size.div_ceil(1024 * 1024),
                    crate::conversation::ATTACHMENT_LIMIT / (1024 * 1024)
                ));
            }
        }
        let live = id.and_then(|id| self.live.get(id))?;
        if attachments.iter().any(Attachment::image) && live.features.known && !live.features.images
        {
            let name = live
                .execution
                .as_ref()
                .map_or("This agent", |execution| execution.name.as_str());
            return Some(format!("{name} can't receive images."));
        }
        None
    }

    pub(super) fn drop_undelivered_fork_session(&mut self, id: &str) {
        // Retry then starts a fresh session and sends the copied history again.
        let Some(mut settings) = self.conversation_settings(id) else {
            return;
        };
        if let Some(old) = settings.session_id.take() {
            settings.previous_session_ids.push(old);
        }
        if self.save_conversation_settings(id, settings) {
            self.shutdown_conversation(id);
        }
    }

    pub(super) fn shutdown_conversation(&mut self, id: &str) {
        let live = self.live.entry(id.to_owned()).or_default();
        live.permission.clear();
        live.progress = None;
        if let Some(slot) = self.drivers.get(id)
            && !live.shutting_down
        {
            live.shutting_down = true;
            if let Err(error) = slot.driver.send(acp::Command::Shutdown) {
                live.error = Some(error);
            }
        }
    }

    pub(super) fn fork(&mut self, id: &str, message: usize) -> Result<Value, String> {
        if self
            .project_of(id)
            .is_some_and(|project| self.deleting.contains_key(&project))
        {
            return Err("This project is being deleted.".into());
        }
        let running = self.live.get(id).is_some_and(|live| live.processing)
            && self.locate(id).is_some_and(|thread| {
                thread
                    .messages
                    .iter()
                    .rposition(|m| m.role == MessageRole::User)
                    .is_some_and(|last| message > last)
            });
        if running {
            return Err("Wait for this reply to finish before forking.".into());
        }
        let store = &mut self.store;
        let (fork, live) = block_in_place(|| {
            let fork = store
                .fork_conversation(id, message)
                .map_err(|e| format!("{e:#}"))?;
            let live = store.conversation(&fork).map(live_for).unwrap_or_default();
            Ok::<_, String>((fork, live))
        })?;
        self.touched.insert(fork.clone(), Instant::now());
        self.live.insert(fork.clone(), live);
        if let Some((project_id, thread)) = self.thread_from_store(&fork) {
            self.broadcast(Delta::Thread { project_id, thread });
        }
        self.send_live(&fork);
        Ok(Value::String(fork))
    }

    pub(super) fn fork_history(&self, id: &str) -> Option<(Option<String>, String)> {
        // The source's session only when a native fork would give exactly the copied history.
        let settings = self.conversation_settings(id)?;
        if settings.session_id.is_some() {
            return None;
        }
        let origin = settings.forked_from?;
        let context = history_text(self.locate(id)?.messages.get(..=origin.message)?);
        let source = &origin.conversation_id;
        let unchanged = self.live.get(source).is_none_or(|live| !live.processing)
            && self
                .locate(source)
                .is_some_and(|thread| thread.messages.len() == origin.message + 1);
        let session = self
            .conversation_settings(source)
            .and_then(|source| source.session_id)
            .filter(|_| unchanged);
        Some((session, context))
    }

    pub(super) fn set_option(
        &mut self,
        id: &str,
        category: Category,
        option: &str,
        value: String,
    ) -> Result<Value, String> {
        if self.live.get(id).is_some_and(|l| l.processing) {
            return Err("Switch after this turn finishes.".into());
        }
        let mut settings = self
            .conversation_settings(id)
            .ok_or("This conversation no longer exists.")?;
        settings.execution.selections.set(category, option, &value);
        let execution = settings.execution.clone();
        if !self.save_conversation_settings(id, settings) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        self.record_visible(
            id,
            "lifecycle",
            json!({"event":"setting_switched","category":category,"option":option,"value":value}),
        );
        self.live.entry(id.to_owned()).or_default().execution = Some(execution);
        if let Some(slot) = self.drivers.get(id) {
            let _ = slot.driver.send(acp::Command::SetOption {
                category,
                id: option.to_owned(),
                value,
            });
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    pub(super) fn answer_permission(
        &mut self,
        id: &str,
        request_id: u64,
        option: &str,
    ) -> Result<Value, String> {
        let Some((index, request)) = self.live.get(id).and_then(|live| {
            live.permission
                .iter()
                .enumerate()
                .find(|(_, request)| request.request_id == request_id)
        }) else {
            return Err("Permission already answered".into());
        };
        let Some(choice) = request.options.iter().find(|choice| choice.id == option) else {
            return Err("That choice is no longer offered.".into());
        };
        let denied = choice.kind == PermissionKind::RejectOnce;
        let decision = json!({"request_id":request_id,"option_id":option,"kind":choice.kind,"name":choice.name,"denied":denied});
        if !self.record_visible(id, "permission_decision", decision) {
            self.send_live(id);
            return Err("History could not be saved.".into());
        }
        let result = self
            .drivers
            .get(id)
            .ok_or_else(|| "Agent is no longer running.".to_owned())
            .and_then(|slot| {
                slot.driver.send(acp::Command::Permission {
                    request_id,
                    option_id: option.to_owned(),
                })
            });
        if let Err(error) = result {
            self.drivers.remove(id);
            let live = self.live.entry(id.to_owned()).or_default();
            live.processing = false;
            live.permission.clear();
            live.error = Some(error.clone());
            self.record_visible(id, "error", json!({"message":error}));
            self.set_runtime_status(id, ThreadStatus::Blocked);
            self.send_live(id);
            return Err(error);
        }
        let live = self.live.entry(id.to_owned()).or_default();
        live.permission.remove(index);
        if live.permission.is_empty() {
            self.set_runtime_status(id, ThreadStatus::Processing);
        }
        self.send_live(id);
        Ok(Value::Null)
    }

    pub(super) fn mark_read(&mut self, id: &str, through: usize) -> Value {
        if self
            .live
            .get(id)
            .and_then(|live| live.last_read_through)
            .is_some_and(|last| last >= through)
        {
            return Value::Null;
        }
        if self.record_visible(id, "message_read", json!({"through":through})) {
            self.broadcast(Delta::Read {
                id: id.to_owned(),
                through,
            });
        } else {
            self.send_live(id);
        }
        Value::Null
    }
}
