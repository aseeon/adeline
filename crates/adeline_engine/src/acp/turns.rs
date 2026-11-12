//! Turns: sending and steering prompts, streamed updates and tool calls, and how
//! a turn ends.
use super::*;

impl Worker {
    pub(super) fn send_prompt(&mut self) {
        if self.exit != Exit::Running || self.setup() != Some(Setup::Ready) {
            return;
        }
        let Some(session_id) = self.session_id().map(str::to_owned) else {
            return;
        };
        let Some(turn) = &self.active else {
            return;
        };
        match turn.step {
            Step::Waiting => {}
            Step::Backoff(when) if Instant::now() >= when => {}
            _ => return,
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
            turn.step = request.map_or(Step::Waiting, Step::Prompting);
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
                                adeline_core::conversation::base64(error.as_bytes()),
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
                    let path = self.linked_path(file)?;
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

    fn linked_path(&self, file: &Attachment) -> anyhow::Result<std::path::PathBuf> {
        if let Some(path) = &file.path {
            return Ok(path.clone());
        }
        let bytes = attachment_bytes(file)?;
        if self.attachments.get().is_none() {
            let folder = tempfile::Builder::new()
                .prefix("adeline-attachments-")
                .tempdir()
                .context("Could not make a folder for attachments")?;
            let _ = self.attachments.set(folder);
        }
        let root = self
            .attachments
            .get()
            .context("No folder for attachments")?;
        let folder = root.path().join(adeline_core::files::unique("file"));
        let name = Path::new(&file.name)
            .file_name()
            .map_or_else(|| "attachment".into(), |name| name.to_os_string());
        let path = folder.join(name);
        block_in_place(|| {
            std::fs::create_dir(&folder).with_context(|| folder.display().to_string())?;
            adeline_core::files::write_new(&path, &bytes)
        })?;
        Ok(path)
    }

    pub(super) fn steer(&mut self, prompt: &[Part]) {
        let steerable = self
            .features
            .as_ref()
            .is_some_and(|features| features.steering)
            && self
                .active
                .as_ref()
                .is_some_and(|turn| matches!(turn.step, Step::Prompting(_)));
        let Some(session) = self.session_id().filter(|_| steerable).map(str::to_owned) else {
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
        if Some(notification.session_id.0.as_ref()) != self.session_id() {
            return;
        }
        let opening = matches!(self.setup(), Some(Setup::Opening { .. }));
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
        let in_turn = !opening
            && self
                .active
                .as_ref()
                .is_some_and(|turn| matches!(turn.step, Step::Prompting(_)));
        match notification.update {
            wire::SessionUpdate::UsageUpdate(usage) => {
                if !opening {
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
                        .map(agent_command)
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
            wire::SessionUpdate::Plan(plan) if !opening => {
                self.emit(EventKind::Todo(
                    plan.entries.into_iter().map(todo_step).collect(),
                ));
            }
            wire::SessionUpdate::AgentMessageChunk(chunk) if in_turn => self.chunk(chunk, false),
            wire::SessionUpdate::AgentThoughtChunk(chunk) if in_turn => self.chunk(chunk, true),
            wire::SessionUpdate::ToolCall(call) if !opening => {
                self.tool(call.into(), in_turn);
            }
            wire::SessionUpdate::ToolCallUpdate(update) if !opening => {
                self.tool(update, in_turn);
            }
            _ => {}
        }
        if ends_turn && in_turn {
            self.finish(StopReason::Signal);
        }
    }

    fn chunk(&mut self, chunk: wire::ContentChunk, thought: bool) {
        let Some(text) = chunk_text(&chunk).filter(|text| !text.is_empty()) else {
            return;
        };
        if let Some(turn) = &mut self.active {
            turn.worked = true;
            if !thought && turn.observed_text.len() < OBSERVED_TEXT {
                turn.observed_text.push_str(&text);
            }
        }
        let message = chunk.message_id.map(|id| id.0.to_string());
        self.emit(if thought {
            EventKind::Thought { message, text }
        } else {
            EventKind::Text { message, text }
        });
    }

    fn tool(&mut self, update: wire::ToolCallUpdate, in_turn: bool) {
        if in_turn && let Some(turn) = &mut self.active {
            turn.worked = true;
        }
        let id = update.tool_call_id.0.to_string();
        let fields = update.fields;
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
        if let Step::Prompting(id) | Step::Cancelling(id) = turn.step {
            turn.step = Step::Cancelling(id);
            if let (Connection::Up(link, _), Some(session)) = (&self.connection, self.session_id())
            {
                link.cancel(session);
            }
        } else {
            self.end_turn();
            self.emit(EventKind::Stopped);
        }
    }

    fn finish(&mut self, stop_reason: StopReason) {
        self.end_turn();
        self.cancel_permissions();
        self.emit(EventKind::Finished { stop_reason });
    }

    pub(super) fn prompt_result(&mut self, id: u64, reason: wire::StopReason) {
        let Some(turn) = &mut self.active else {
            return;
        };
        let cancelled = match turn.step {
            Step::Prompting(request) if request == id => false,
            Step::Cancelling(request) if request == id => true,
            _ => return,
        };
        turn.step = Step::Waiting;
        if cancelled {
            self.end_turn();
            self.emit(EventKind::Stopped);
            if self.exit != Exit::Running {
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
            self.end_turn();
            self.emit(EventKind::Stopped);
        } else if self.active.as_ref().is_some_and(|turn| turn.denied) {
            self.fail("Permission denied by the user.".into(), FailureKind::Denied);
        } else if reason == StopReason::EndTurn
            && let Some(error) = self
                .active
                .as_ref()
                .map(|turn| turn.observed_text.clone())
                .filter(|text| looks_like_agent_error(text))
        {
            let kind = classify_error(&error, None);
            self.give_up(error, kind);
        } else {
            self.finish(reason);
        }
        if self.exit != Exit::Running {
            self.close_session();
        }
    }
}

fn agent_command(command: wire::AvailableCommand) -> AgentCommand {
    AgentCommand {
        name: command.name,
        description: command.description,
        hint: match command.input {
            Some(wire::AvailableCommandInput::Unstructured(input)) => input.hint,
            _ => String::new(),
        },
    }
}

fn todo_step(entry: wire::PlanEntry) -> TodoStep {
    TodoStep {
        text: entry.content,
        status: match entry.status {
            wire::PlanEntryStatus::InProgress => StepStatus::InProgress,
            wire::PlanEntryStatus::Completed => StepStatus::Completed,
            _ => StepStatus::Pending,
        },
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
    if file.path.is_none() && file.size == 0 {
        return Ok(Vec::new());
    }
    if !file.data.is_empty() {
        return adeline_core::conversation::unbase64(&file.data)
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
        attachment_bytes(file).map(|bytes| adeline_core::conversation::base64(&bytes))
    } else {
        Ok(file.data.clone())
    }
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
