//! Turns: sending and steering prompts, streamed updates and tool calls, and how
//! a turn ends.
use super::*;

impl Worker {
    pub(super) fn send_prompt(&mut self) {
        if self.closing || !self.configured {
            return;
        }
        let Some(session_id) = self.session_id.clone() else {
            return;
        };
        let Some(turn) = &self.active else {
            return;
        };
        if turn.cancelled
            || turn.prompt_request.is_some()
            || turn.retry_at.is_some_and(|when| Instant::now() < when)
        {
            return;
        }
        let blocks = match self.content(&turn.prompt) {
            Ok(blocks) => blocks,
            Err(error) => {
                self.fail(format!("{error:#}"), FailureKind::Configuration);
                return;
            }
        };
        let request = self.request(
            Outgoing::Prompt(wire::PromptRequest::new(session_id, blocks)),
            Request::Prompt,
        );
        if let Some(turn) = &mut self.active {
            turn.prompt_request = request;
            turn.retry_at = None;
        }
    }

    pub(super) fn content(&self, parts: &[Part]) -> anyhow::Result<Vec<wire::ContentBlock>> {
        let features = self.features.clone().unwrap_or_default();
        let mut blocks = Vec::new();
        for part in parts {
            match part {
                Part::Text(text) => {
                    blocks.push(wire::ContentBlock::Text(wire::TextContent::new(
                        text.clone(),
                    )));
                }
                Part::File(file) if file.image() => {
                    if !features.images {
                        anyhow::bail!("{} can't receive images.", self.config.name);
                    }
                    let data = attachment_data(file)?;
                    blocks.push(wire::ContentBlock::Image(wire::ImageContent::new(
                        data,
                        file.mime.clone(),
                    )));
                }
                Part::File(file) if features.embedded_files => {
                    let bytes = attachment_bytes(file)?;
                    let uri = file_uri(file);
                    let resource = match String::from_utf8(bytes) {
                        Ok(text) => wire::EmbeddedResourceResource::TextResourceContents(
                            wire::TextResourceContents::new(text, uri).mime_type(file.mime.clone()),
                        ),
                        Err(error) => wire::EmbeddedResourceResource::BlobResourceContents(
                            wire::BlobResourceContents::new(
                                crate::conversation::base64(error.as_bytes()),
                                uri,
                            )
                            .mime_type(file.mime.clone()),
                        ),
                    };
                    blocks.push(wire::ContentBlock::Resource(wire::EmbeddedResource::new(
                        resource,
                    )));
                }
                Part::File(file) => {
                    let path = linked_path(file)?;
                    blocks.push(wire::ContentBlock::ResourceLink(
                        wire::ResourceLink::new(file.name.clone(), path_uri(&path))
                            .mime_type(file.mime.clone())
                            .size(i64::try_from(file.size).ok()),
                    ));
                }
            }
        }
        Ok(blocks)
    }

    pub(super) fn steer(&mut self, prompt: &[Part]) {
        let steerable = self
            .features
            .as_ref()
            .is_some_and(|features| features.steering)
            && self
                .active
                .as_ref()
                .is_some_and(|turn| turn.prompt_request.is_some() && !turn.cancelled);
        let Some(session) = self.session_id.clone().filter(|_| steerable) else {
            self.emit(EventKind::Steered { delivered: false });
            return;
        };
        let blocks = match self.content(prompt) {
            Ok(blocks) => blocks,
            Err(error) => {
                self.emit(EventKind::Error {
                    message: format!("{error:#}"),
                    kind: FailureKind::Configuration,
                });
                self.emit(EventKind::Steered { delivered: false });
                return;
            }
        };
        let params = json!({
            "sessionId": session,
            "prompt": blocks,
            "_meta": {"steering": {"idleBehavior": "promptRequired"}},
        });
        if self
            .request(Outgoing::Steer(params), Request::Steer)
            .is_none()
        {
            self.emit(EventKind::Steered { delivered: false });
        }
    }

    pub(super) fn update(&mut self, notification: wire::SessionNotification) {
        if Some(notification.session_id.0.as_ref()) != self.session_id.as_deref() {
            return;
        }
        let ends_turn = self.profile.is_some_and(|profile| {
            profile.turn_end.iter().any(|key| {
                notification
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get(*key))
                    .and_then(Value::as_bool)
                    == Some(true)
            })
        });
        let in_turn = self.setup.is_none()
            && self
                .active
                .as_ref()
                .is_some_and(|turn| !turn.cancelled && turn.prompt_request.is_some());
        match notification.update {
            // Session state counts between turns too.
            wire::SessionUpdate::UsageUpdate(usage) => {
                if self.setup.is_none() {
                    self.emit(EventKind::Usage {
                        used: usage.used,
                        size: usage.size,
                    });
                }
            }
            wire::SessionUpdate::AvailableCommandsUpdate(update) => {
                self.emit(EventKind::Commands(
                    update
                        .available_commands
                        .into_iter()
                        .map(|command| AgentCommand {
                            name: command.name,
                            description: command.description,
                            hint: match command.input {
                                Some(wire::AvailableCommandInput::Unstructured(input)) => {
                                    input.hint
                                }
                                _ => String::new(),
                            },
                        })
                        .collect(),
                ));
            }
            wire::SessionUpdate::CurrentModeUpdate(update) => {
                if let Some(modes) = &mut self.modes {
                    modes.current_mode_id = update.current_mode_id;
                    self.refresh_options();
                }
            }
            wire::SessionUpdate::ConfigOptionUpdate(update) => {
                self.raw_options = update.config_options;
                self.refresh_options();
            }
            wire::SessionUpdate::SessionInfoUpdate(update) => {
                if let MaybeUndefined::Value(title) = update.title
                    && !title.trim().is_empty()
                {
                    self.emit(EventKind::Title(title));
                }
            }
            wire::SessionUpdate::Plan(plan) if self.setup.is_none() => {
                self.emit(EventKind::Todo(
                    plan.entries
                        .into_iter()
                        .map(|entry| TodoStep {
                            text: entry.content,
                            status: match entry.status {
                                wire::PlanEntryStatus::InProgress => StepStatus::InProgress,
                                wire::PlanEntryStatus::Completed => StepStatus::Completed,
                                _ => StepStatus::Pending,
                            },
                        })
                        .collect(),
                ));
            }
            wire::SessionUpdate::AgentMessageChunk(chunk) if in_turn => {
                if let Some(text) = chunk_text(&chunk)
                    && !text.is_empty()
                {
                    if let Some(turn) = &mut self.active {
                        turn.worked = true;
                        if turn.observed_text.len() < OBSERVED_TEXT {
                            turn.observed_text.push_str(&text);
                        }
                    }
                    self.emit(EventKind::Text {
                        message: chunk.message_id.map(|id| id.0.to_string()),
                        text,
                    });
                }
            }
            wire::SessionUpdate::AgentThoughtChunk(chunk) if in_turn => {
                if let Some(text) = chunk_text(&chunk)
                    && !text.is_empty()
                {
                    if let Some(turn) = &mut self.active {
                        turn.worked = true;
                    }
                    self.emit(EventKind::Thought {
                        message: chunk.message_id.map(|id| id.0.to_string()),
                        text,
                    });
                }
            }
            // Tool calls outside a turn are background work the agent reports.
            wire::SessionUpdate::ToolCall(call) if self.setup.is_none() => {
                let update = wire::ToolCallUpdate::new(
                    call.tool_call_id,
                    wire::ToolCallUpdateFields::new()
                        .kind(call.kind)
                        .status(call.status)
                        .title(call.title)
                        .content(call.content)
                        .locations(call.locations)
                        .raw_output(call.raw_output),
                );
                self.tool(update, in_turn);
            }
            wire::SessionUpdate::ToolCallUpdate(update) if self.setup.is_none() => {
                self.tool(update, in_turn);
            }
            // Replay, user echoes and notices stay in the raw traffic.
            _ => {}
        }
        if ends_turn && in_turn {
            self.finish(StopReason::Signal);
        }
    }

    fn tool(&mut self, update: wire::ToolCallUpdate, in_turn: bool) {
        if in_turn && let Some(turn) = &mut self.active {
            turn.worked = true;
        }
        let id = update.tool_call_id.0.to_string();
        let fields = update.fields;
        // Updates may omit fields they don't change.
        let entry = self.tools.entry(id.clone()).or_default();
        if let Some(title) = fields.title {
            entry.title = title;
        }
        if let Some(status) = fields.status {
            entry.status = tool_status(status);
        }
        let detail = tool_detail(fields.raw_output.as_ref(), fields.content.as_deref());
        if !detail.is_empty() {
            entry.detail = detail;
        }
        if let Some(kind) = fields.kind {
            entry.kind = tool_kind(kind);
        }
        if let Some(locations) = fields.locations {
            entry.paths = locations
                .iter()
                .map(|location| location.path.to_string_lossy().into_owned())
                .collect();
        }
        let ToolState {
            title,
            status,
            detail,
            kind,
            paths,
        } = entry.clone();
        self.emit(EventKind::Tool {
            id,
            title,
            status,
            detail,
            kind,
            paths,
        });
    }

    pub(super) fn cancel(&mut self) {
        self.cancel_permissions();
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.cancelled = true;
        turn.retry_at = None;
        if turn.prompt_request.is_some() {
            if let (Some(link), Some(session)) = (&self.link, &self.session_id) {
                link.cancel(session);
            }
        } else {
            self.active = None;
            self.emit(EventKind::Stopped);
        }
    }

    fn finish(&mut self, stop_reason: StopReason) {
        self.active = None;
        self.cancel_permissions();
        self.emit(EventKind::Finished { stop_reason });
    }

    pub(super) fn prompt_result(&mut self, id: u64, reason: wire::StopReason) {
        let Some(turn) = &mut self.active else {
            return;
        };
        // A late answer to a turn a profile signal already ended.
        if turn.prompt_request != Some(id) {
            return;
        }
        turn.prompt_request = None;
        if turn.cancelled {
            self.active = None;
            self.emit(EventKind::Stopped);
            if self.closing {
                self.close_session();
            }
            return;
        }
        let reason = match reason {
            wire::StopReason::EndTurn => StopReason::EndTurn,
            wire::StopReason::MaxTokens => StopReason::MaxTokens,
            wire::StopReason::MaxTurnRequests => StopReason::MaxTurnRequests,
            wire::StopReason::Refusal => StopReason::Refusal,
            _ => StopReason::Cancelled,
        };
        if reason == StopReason::Cancelled {
            self.active = None;
            self.emit(EventKind::Stopped);
        } else if self.active.as_ref().is_some_and(|turn| turn.denied) {
            self.fail("Permission denied by the user.".into(), FailureKind::Denied);
        } else if reason == StopReason::EndTurn
            && self
                .active
                .as_ref()
                .is_some_and(|turn| looks_like_agent_error(&turn.observed_text))
        {
            let error = self
                .active
                .as_ref()
                .map_or(String::new(), |turn| turn.observed_text.clone());
            // The model can write this text too, so report it without retrying.
            if let Some(turn) = &mut self.active {
                turn.retries = turn.attempt;
            }
            self.fail(error.clone(), classify_error(&error, None));
        } else {
            self.finish(reason);
        }
        if self.closing {
            self.close_session();
        }
    }
}

fn chunk_text(chunk: &wire::ContentChunk) -> Option<String> {
    match &chunk.content {
        wire::ContentBlock::Text(text) => Some(text.text.clone()),
        wire::ContentBlock::ResourceLink(link) => Some(format!("[{}]({})", link.name, link.uri)),
        _ => None,
    }
}

fn tool_kind(kind: wire::ToolKind) -> ToolKind {
    match kind {
        wire::ToolKind::Read => ToolKind::Read,
        wire::ToolKind::Edit => ToolKind::Edit,
        wire::ToolKind::Delete => ToolKind::Delete,
        wire::ToolKind::Move => ToolKind::Move,
        wire::ToolKind::Search => ToolKind::Search,
        wire::ToolKind::Execute => ToolKind::Execute,
        wire::ToolKind::Think => ToolKind::Think,
        wire::ToolKind::Fetch => ToolKind::Fetch,
        wire::ToolKind::SwitchMode => ToolKind::SwitchMode,
        _ => ToolKind::Other,
    }
}

fn tool_status(status: wire::ToolCallStatus) -> ToolStatus {
    match status {
        wire::ToolCallStatus::InProgress => ToolStatus::InProgress,
        wire::ToolCallStatus::Completed => ToolStatus::Completed,
        wire::ToolCallStatus::Failed => ToolStatus::Failed,
        _ => ToolStatus::Pending,
    }
}

pub(super) fn tool_detail(
    raw_output: Option<&Value>,
    content: Option<&[wire::ToolCallContent]>,
) -> String {
    if let Some(raw) = raw_output.filter(|value| !value.is_null()) {
        return raw.as_str().map_or_else(|| raw.to_string(), str::to_owned);
    }
    let mut detail = String::new();
    for item in content.into_iter().flatten() {
        let (path, text) = match item {
            wire::ToolCallContent::Content(content) => match &content.content {
                wire::ContentBlock::Text(text) => (None, text.text.clone()),
                _ => continue,
            },
            wire::ToolCallContent::Diff(diff) => (
                Some(diff.path.to_string_lossy().into_owned()),
                diff.new_text.clone(),
            ),
            _ => continue,
        };
        if !detail.is_empty() {
            detail.push('\n');
        }
        if let Some(path) = path {
            detail.push_str(&path);
            detail.push_str(":\n");
        }
        detail.push_str(&text);
    }
    detail
}

fn attachment_bytes(file: &Attachment) -> anyhow::Result<Vec<u8>> {
    if !file.data.is_empty() {
        return crate::conversation::unbase64(&file.data)
            .with_context(|| format!("{} could not be read.", file.name));
    }
    let path = file
        .path
        .as_ref()
        .with_context(|| format!("{} has no content.", file.name))?;
    block_in_place(|| std::fs::read(path)).with_context(|| path.display().to_string())
}

fn attachment_data(file: &Attachment) -> anyhow::Result<String> {
    if file.data.is_empty() {
        attachment_bytes(file).map(|bytes| crate::conversation::base64(&bytes))
    } else {
        Ok(file.data.clone())
    }
}

fn linked_path(file: &Attachment) -> anyhow::Result<std::path::PathBuf> {
    if let Some(path) = &file.path {
        return Ok(path.clone());
    }
    let bytes = attachment_bytes(file)?;
    let folder = std::env::temp_dir().join(crate::files::unique("adeline-attachment"));
    let name = Path::new(&file.name)
        .file_name()
        .map_or_else(|| "attachment".into(), |name| name.to_os_string());
    let path = folder.join(name);
    block_in_place(|| std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, bytes)))
        .with_context(|| path.display().to_string())?;
    Ok(path)
}

fn file_uri(file: &Attachment) -> String {
    file.path
        .as_deref()
        .map_or_else(|| format!("file:///{}", file.name), path_uri)
}

fn path_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}
