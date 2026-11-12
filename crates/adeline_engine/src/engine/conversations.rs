//! A conversation's agent: starting and stopping its driver, forks, option
//! changes, permission answers and read markers.
use super::*;

impl Engine {
    pub(super) fn conversation_command(&mut self, command: Command) -> Result<Value, String> {
        let conversation = |engine: &Self, id: &str| {
            engine
                .locate(id)
                .map(|_| ())
                .ok_or_else(|| "This conversation no longer exists.".to_owned())
        };
        match command {
            Command::Send {
                project_id,
                conversation_id,
                agent_id,
                prompt,
                attachments,
                now,
            } => self
                .send(
                    &project_id,
                    conversation_id,
                    agent_id.as_deref(),
                    &prompt,
                    attachments,
                    now,
                )
                .map(Value::String),
            Command::TakeQueued { id, queued } => {
                conversation(self, &id).and_then(|()| self.take_queued(&id, queued))
            }
            Command::SendQueuedNow { id, queued } => {
                conversation(self, &id).and_then(|()| self.send_queued_now(&id, queued))
            }
            Command::Stop { id } => conversation(self, &id).map(|()| {
                self.stop(&id, Stop::User);
                Value::Null
            }),
            Command::ForceStop { id } => conversation(self, &id).map(|()| {
                self.force_stop(&id);
                Value::Null
            }),
            Command::Retry { id } => conversation(self, &id).and_then(|()| self.retry(&id)),
            Command::Restart { id } => conversation(self, &id).map(|()| {
                self.restart(&id);
                Value::Null
            }),
            Command::RetryStorage { id } => {
                conversation(self, &id).and_then(|()| self.retry_storage(&id))
            }
            Command::ReplaceSession { id } => {
                conversation(self, &id).and_then(|()| self.replace_session(&id))
            }
            Command::Fork { id, message } => {
                conversation(self, &id).and_then(|()| self.fork(&id, message))
            }
            Command::SetOption {
                id,
                category,
                option,
                value,
            } => conversation(self, &id)
                .and_then(|()| self.set_option(&id, category, &option, value)),
            Command::AnswerPermission {
                id,
                request_id,
                option_id,
            } => conversation(self, &id)
                .and_then(|()| self.answer_permission(&id, request_id, &option_id)),
            Command::SetStatus { id, status } => {
                conversation(self, &id).and_then(|()| self.set_status(&id, status))
            }
            Command::MarkRead { id, through } => {
                conversation(self, &id).map(|()| self.mark_read(&id, through))
            }
            _ => unreachable!("conversation_command only gets conversation commands"),
        }
    }

    pub(super) fn thread_from_store(&self, id: &str) -> Option<(String, Thread)> {
        self.store.projects.iter().find_map(|p| {
            p.conversations
                .iter()
                .find(|c| c.id == id)
                .map(|c| (p.id.clone(), c.to_thread()))
        })
    }

    pub(super) fn ensure_driver(&mut self, id: &str) -> bool {
        if self.agent(id).is_some() {
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
        self.lifecycle(id).spawned(phase::Agent {
            driver,
            turn: 0,
            steering: None,
            closing: None,
        });
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
            if file.size > adeline_core::conversation::ATTACHMENT_LIMIT {
                return Some(format!(
                    "{} is {} MB. Files over {} MB can't be attached.",
                    file.name,
                    file.size.div_ceil(1024 * 1024),
                    adeline_core::conversation::ATTACHMENT_LIMIT / (1024 * 1024)
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
        let Some(driver) = self.lifecycle(id).close() else {
            return;
        };
        if let Err(error) = driver.send(acp::Command::Shutdown) {
            self.live_mut(id).error = Some(error);
        }
    }

    pub(super) fn fork(&mut self, id: &str, message: usize) -> Result<Value, String> {
        if self
            .project_of(id)
            .is_some_and(|project| self.deleting.contains_key(&project))
        {
            return Err("This project is being deleted.".into());
        }
        let running = self.processing(id)
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
        let (fork, live, lifecycle) = block_in_place(|| {
            let fork = store
                .fork_conversation(id, message)
                .map_err(|e| format!("{e:#}"))?;
            let saved = store.conversation(&fork);
            let live = saved.map(live_for).unwrap_or_default();
            let lifecycle = saved.map(lifecycle_for).unwrap_or_default();
            Ok::<_, String>((fork, live, lifecycle))
        })?;
        self.touched.insert(fork.clone(), Instant::now());
        self.live.insert(fork.clone(), live);
        self.lifecycles.insert(fork.clone(), lifecycle);
        self.broadcast_thread(&fork);
        self.send_live(&fork);
        Ok(Value::String(fork))
    }

    pub(super) fn fork_history(&self, id: &str) -> Option<(Option<String>, String)> {
        let settings = self.conversation_settings(id)?;
        if settings.session_id.is_some() {
            return None;
        }
        let origin = settings.forked_from?;
        let context = history_text(self.locate(id)?.messages.get(..=origin.message)?);
        let source = &origin.conversation_id;
        let unchanged = !self.processing(source)
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
        if self.processing(id) {
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
        self.live_mut(id).execution = Some(execution);
        if let Some(agent) = self.agent(id) {
            let _ = agent.driver.send(acp::Command::SetOption {
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
        let Some((index, request)) = self
            .lifecycles
            .get(id)
            .and_then(|lifecycle| lifecycle.phase.turn())
            .and_then(|turn| {
                turn.permission
                    .iter()
                    .enumerate()
                    .find(|(_, request)| request.request_id == request_id)
            })
        else {
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
            .agent(id)
            .ok_or_else(|| "Agent is no longer running.".to_owned())
            .and_then(|agent| {
                agent.driver.send(acp::Command::Permission {
                    request_id,
                    option_id: option.to_owned(),
                })
            });
        if let Err(error) = result {
            let lifecycle = self.lifecycle(id);
            lifecycle.gone();
            lifecycle.block(After::Blocked);
            self.live_mut(id).error = Some(error.clone());
            self.record_visible(id, "error", json!({"message":error}));
            self.send_live(id);
            return Err(error);
        }
        if let Some(turn) = self.lifecycle(id).phase.turn_mut() {
            turn.permission.remove(index);
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
