//! Starting the agent process, the ACP handshake, session setup, authentication
//! and shutdown.
use super::*;

impl Worker {
    pub(super) fn start(&mut self) {
        if self.closing {
            return;
        }
        if !self.config.directory.is_absolute() || !self.config.directory.is_dir() {
            self.fail("The saved working directory is missing or not absolute. Restore it or change the project for a new conversation.".into(), FailureKind::Configuration);
            return;
        }
        let command = &self.config.command;
        let existing_path = command.is_file() || self.config.directory.join(command).is_file();
        let words = command
            .to_str()
            .map_or(1, |command| command.split_whitespace().count());
        if words != 1 && !existing_path {
            self.fail("Agent command must name one executable; edit the agent definition for new conversations.".into(), FailureKind::Configuration);
            return;
        }
        let flags = self.instructions() == Instructions::Flags;
        if flags
            && self.config.arguments.iter().any(|arg| {
                arg == "--system-prompt"
                    || arg.starts_with("--system-prompt=")
                    || arg == "--system-prompt-template"
                    || arg.starts_with("--system-prompt-template=")
            })
        {
            self.fail("Agent arguments replace OMP's default prompt. Remove system-prompt override to keep OMP defaults.".into(), FailureKind::Configuration);
            return;
        }
        let mut process = ProcessCommand::new(&self.config.command);
        process
            .args(&self.config.arguments)
            .envs(self.config.environment.iter().map(|(k, v)| (k, v)))
            .current_dir(&self.config.directory);
        if flags {
            let flag = match self.config.instructions_mode {
                InstructionsMode::Append => "--append-system-prompt",
                InstructionsMode::Overwrite => "--system-prompt",
            };
            // A trailing newline forces OMP's literal-text route, not its single-line file lookup.
            process.arg(format!("{flag}={}\n", self.guidance()));
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            process.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        };
        match block_in_place(|| adeline_core::platform::spawn_piped(&mut process, true)) {
            Ok((child, stdin, stdout, stderr)) => {
                self.configured = false;
                self.features = None;
                self.raw_options.clear();
                self.modes = None;
                self.options.clear();
                self.handshake = None;
                self.timeout_reported = false;
                self.lost_during_initialize = true;
                self.epoch += 1;
                self.stderr.clear();
                match tokio::process::ChildStdin::from_std(stdin) {
                    Ok(stdin) => {
                        self.connect(stdin, stdout);
                        if let Some(stderr) = stderr {
                            self.read_stderr(stderr);
                        }
                        self.child = Some(child);
                        self.connecting = true;
                    }
                    Err(error) => {
                        let mut child = child;
                        let _ = adeline_core::platform::kill_tree(&mut child);
                        self.fail(
                            format!("Starting {} failed: {error}", self.config.command.display()),
                            FailureKind::Configuration,
                        );
                    }
                }
            }
            Err(error) => self.fail(
                format!("Starting {} failed: {error}", self.config.command.display()),
                FailureKind::Configuration,
            ),
        }
    }

    fn connect(&self, stdin: tokio::process::ChildStdin, stdout: adeline_core::platform::Reader) {
        let outgoing = futures::sink::unfold(stdin, move |mut stdin, line: String| async move {
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            Ok::<_, std::io::Error>(stdin)
        });
        let lines = BufReader::new(stdout).lines();
        let incoming = futures::stream::unfold(lines, |mut lines| async move {
            match lines.next_line().await {
                Ok(Some(line)) => Some((Ok(line), lines)),
                Ok(None) => None,
                Err(error) => Some((Err(error), lines)),
            }
        });
        self.connect_lines(outgoing, incoming);
    }

    pub(super) fn connect_lines(
        &self,
        outgoing: impl futures::Sink<String, Error = std::io::Error> + Send + 'static,
        incoming: impl futures::Stream<Item = std::io::Result<String>> + Send + 'static,
    ) {
        use futures::SinkExt as _;
        let epoch = self.epoch;
        let inputs = self.sender.clone();
        let traffic = self.traffic_sink();
        let outgoing_traffic = Arc::clone(&traffic);
        let outgoing = outgoing.with(move |line: String| {
            outgoing_traffic(Direction::ToAgent, &line, TrafficNote::None);
            futures::future::ready(Ok::<_, std::io::Error>(line))
        });
        let incoming = incoming.filter_map(move |line| {
            let kept = match line {
                Ok(line) if line.trim().is_empty() => None,
                Ok(line) => {
                    let note = classify_line(&line);
                    traffic(Direction::FromAgent, &line, note);
                    (note != TrafficNote::NotJson).then_some(Ok(line))
                }
                Err(error) => Some(Err(error)),
            };
            futures::future::ready(kept)
        });
        let handler_inputs = inputs.clone();
        tokio::spawn(async move {
            let (close, closed) = oneshot::channel::<()>();
            let mut close = Some(close);
            let connected_inputs = inputs.clone();
            let result = sdk::Client
                .builder()
                .name("adeline")
                .on_receive_dispatch(
                    async move |dispatch: Dispatch, _connection: ConnectionTo<Agent>| {
                        Ok(route(dispatch, &handler_inputs, epoch))
                    },
                    sdk::on_receive_dispatch!(),
                )
                .connect_with(
                    Lines::new(Box::pin(outgoing), Box::pin(incoming)),
                    async move |connection: ConnectionTo<Agent>| {
                        let link = Link::Sdk {
                            connection: connection.clone(),
                            _close: close.take().expect("connected once"),
                        };
                        let _ = connected_inputs.send(Input::Connected { epoch, link });
                        // Either the agent closes its output or the worker drops the link.
                        tokio::select! {
                            () = connection.incoming_closed() => {}
                            _ = closed => {}
                        }
                        Ok(())
                    },
                )
                .await;
            let error = match result {
                Ok(()) => "Agent closed protocol output.".to_owned(),
                Err(error) if sdk::is_incoming_transport_closed(&error) || broken_pipe(&error) => {
                    "Agent closed protocol output.".to_owned()
                }
                Err(error) => format!("ACP connection failed: {error}"),
            };
            let _ = inputs.send(Input::Lost { epoch, error });
        });
    }

    fn read_stderr(&self, stderr: adeline_core::platform::Reader) {
        let epoch = self.epoch;
        let inputs = self.sender.clone();
        let traffic = self.traffic_sink();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(mut line)) = lines.next_line().await {
                // Keeps a runaway stderr line from flooding the traffic log.
                line.truncate(line.floor_char_boundary(1024));
                traffic(Direction::Stderr, &line, TrafficNote::None);
                if inputs.send(Input::Stderr { epoch, line }).is_err() {
                    break;
                }
            }
        });
    }

    fn traffic_sink(&self) -> Arc<dyn Fn(Direction, &str, TrafficNote) + Send + Sync> {
        let events = self.events.clone();
        let id = self.id.clone();
        Arc::new(move |direction, line, note| {
            let _ = events.try_send(Event {
                conversation_id: id.clone(),
                turn: 0,
                kind: EventKind::Traffic(TrafficEntry {
                    at: adeline_core::recency::now_ms(),
                    direction,
                    text: trimmed_traffic(line),
                    note,
                }),
            });
        })
    }

    fn instructions(&self) -> Instructions {
        self.profile
            .map_or(Instructions::None, |profile| profile.instructions)
    }

    fn guidance(&self) -> String {
        harness::guidance(&self.config.name, &self.config.system_instructions)
    }

    pub(super) fn initialize(&mut self) {
        let capabilities = wire::ClientCapabilities::new()
            .auth(wire::AuthCapabilities::new().terminal(true))
            .elicitation(
                wire::ElicitationCapabilities::new().url(wire::ElicitationUrlCapabilities::new()),
            )
            .session(
                wire::ClientSessionCapabilities::new().config_options(
                    wire::SessionConfigOptionsCapabilities::new()
                        .boolean(wire::BooleanConfigOptionCapabilities::new()),
                ),
            );
        let request = wire::InitializeRequest::new(ProtocolVersion::V1)
            .client_capabilities(capabilities)
            .client_info(
                wire::Implementation::new("adeline", env!("CARGO_PKG_VERSION"))
                    .title("Adeline".to_owned()),
            );
        self.request(Outgoing::Initialize(request), Request::Initialize);
    }

    pub(super) fn handshake_timeout(&mut self) {
        let Some((id, request, _)) = self.handshake.take() else {
            return;
        };
        self.pending.remove(&id);
        self.setup = None;
        let (message, kind) = if request == Request::Initialize {
            ("Agent did not respond to ACP initialize. Check that its configured Arguments include `acp` and that the command supports ACP.".to_owned(), FailureKind::Configuration)
        } else {
            (
                format!(
                    "Agent did not respond to ACP {request:?} within 30 seconds. Check the agent and provider connection."
                ),
                FailureKind::Temporary,
            )
        };
        self.timeout_reported = true;
        self.close_link();
        self.lost_pending = Some(message.clone());
        self.lost_at = Some(Instant::now());
        self.shutdown_deadline = Some(Instant::now() + SHUTDOWN_GRACE);
        self.fail(message, kind);
    }

    fn mcp_servers(&self) -> Vec<wire::McpServer> {
        let features = self.features.clone().unwrap_or_default();
        let mut skipped = Vec::new();
        let mut servers = Vec::new();
        for server in &self.config.mcp_servers {
            match &server.transport {
                McpTransport::Stdio {
                    command,
                    arguments,
                    environment,
                } => servers.push(wire::McpServer::Stdio(
                    wire::McpServerStdio::new(server.name.clone(), command.clone())
                        .args(arguments.clone())
                        .env(
                            environment
                                .iter()
                                .map(|(name, value)| wire::EnvVariable::new(name, value))
                                .collect(),
                        ),
                )),
                McpTransport::Http { url, headers } if features.mcp_http => {
                    servers.push(wire::McpServer::Http(
                        wire::McpServerHttp::new(server.name.clone(), url.clone()).headers(
                            headers
                                .iter()
                                .map(|(name, value)| wire::HttpHeader::new(name, value))
                                .collect(),
                        ),
                    ));
                }
                McpTransport::Http { .. } => skipped.push(server.name.clone()),
            }
        }
        if !skipped.is_empty() {
            self.emit(EventKind::SkippedServers(skipped));
        }
        servers
    }

    pub(super) fn session_meta(&self) -> Option<wire::Meta> {
        if self.instructions() != Instructions::SessionMeta {
            return None;
        }
        let guidance = self.guidance();
        let prompt = match self.config.instructions_mode {
            InstructionsMode::Append => json!({"append": guidance}),
            InstructionsMode::Overwrite => Value::String(guidance),
        };
        let mut meta = wire::Meta::new();
        meta.insert("systemPrompt".into(), prompt);
        Some(meta)
    }

    pub(super) fn setup_session(&mut self) {
        self.configured = false;
        self.config_step = 0;
        self.forking = false;
        let features = self.features.clone().unwrap_or_default();
        let cwd = self.config.directory.clone();
        let servers = self.mcp_servers();
        let meta = self.session_meta();
        let outgoing = if let Some(id) = &self.session_id {
            if features.resume {
                Outgoing::Resume(
                    wire::ResumeSessionRequest::new(id.clone(), cwd)
                        .mcp_servers(servers)
                        .meta(meta),
                )
            } else if features.load {
                Outgoing::Load(
                    wire::LoadSessionRequest::new(id.clone(), cwd)
                        .mcp_servers(servers)
                        .meta(meta),
                )
            } else {
                self.restore_failed("The agent does not support restoring sessions.");
                return;
            }
        } else if let Some((Some(source), _)) = &self.fork
            && features.fork
        {
            self.forking = true;
            Outgoing::Fork(
                wire::ForkSessionRequest::new(source.clone(), cwd)
                    .mcp_servers(servers)
                    .meta(meta),
            )
        } else {
            Outgoing::New(
                wire::NewSessionRequest::new(cwd)
                    .mcp_servers(servers)
                    .meta(meta),
            )
        };
        if self.request(outgoing, Request::Setup).is_some() {
            self.setup = Some(Request::Setup);
        }
    }

    pub(super) fn initialized(&mut self, response: &wire::InitializeResponse) {
        self.lost_during_initialize = false;
        if response.protocol_version != ProtocolVersion::V1 {
            let name = &self.config.name;
            self.fail(
                format!(
                    "{name} uses ACP version {}. Adeline supports version 1.",
                    response.protocol_version
                ),
                FailureKind::Version,
            );
            return;
        }
        let capabilities = &response.agent_capabilities;
        let steering = response
            .meta
            .as_ref()
            .and_then(|meta| meta.get("steering"))
            .and_then(|steering| steering.get("supported"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && self.profile.is_none_or(|profile| profile.steering);
        let features = Features {
            images: capabilities.prompt_capabilities.image,
            embedded_files: capabilities.prompt_capabilities.embedded_context,
            load: capabilities.load_session,
            resume: capabilities.session_capabilities.resume.is_some(),
            fork: capabilities.session_capabilities.fork.is_some(),
            close: capabilities.session_capabilities.close.is_some(),
            steering,
            logout: capabilities.auth.logout.is_some(),
            mcp_http: capabilities.mcp_capabilities.http,
            mcp_sse: capabilities.mcp_capabilities.sse,
            auth: response.auth_methods.iter().map(auth_method).collect(),
            known: true,
        };
        let (name, version) = response
            .agent_info
            .as_ref()
            .map(|info| (info.name.clone(), info.version.clone()))
            .unwrap_or_default();
        self.features = Some(features.clone());
        self.emit(EventKind::Agent {
            name,
            version,
            features,
        });
        if self.closing && !self.probing {
            self.close_link();
            return;
        }
        if let Some(method) = self.auth.take() {
            self.send_auth(method);
            return;
        }
        if self.active.is_some() || self.probing || self.replacing {
            self.setup_session();
        }
    }

    pub(super) fn session_ready(&mut self, session: SessionReply) {
        self.restore_required = false;
        self.setup = None;
        let session_id = if let Some(id) = &self.session_id {
            id.clone()
        } else if let Some(id) = session.session_id {
            id
        } else {
            self.fail(
                "ACP session/new returned no session ID.".into(),
                FailureKind::Configuration,
            );
            return;
        };
        let was_replacement = self.replacing;
        self.session_id = Some(session_id.clone());
        self.raw_options = session.options.unwrap_or_default();
        self.modes = session.modes;
        self.previous_session.take();
        if let Some((_, context)) = self.fork.take()
            && !std::mem::take(&mut self.forking)
        {
            if let Some(turn) = &mut self.active {
                turn.prompt = prefixed(&turn.prompt, |text| with_context(&context, text));
            } else {
                self.pending_context = Some(context);
            }
            self.emit(EventKind::TextCopy);
        }
        self.emit(EventKind::Session {
            session_id,
            replaced: was_replacement,
        });
        self.refresh_options();
        if self.closing && !self.probing {
            self.close_session();
            return;
        }
        self.configure_next();
    }

    pub(super) fn authenticate(&mut self, method: Option<String>) {
        if self.link.is_some() && self.features.is_some() {
            self.send_auth(method);
        } else {
            self.auth = Some(method);
            if self.child.is_none() {
                self.start();
            }
        }
    }

    fn send_auth(&mut self, method: Option<String>) {
        let outgoing = match method {
            Some(method) => (
                Outgoing::Authenticate(wire::AuthenticateRequest::new(method)),
                Request::Authenticate,
            ),
            None => (
                Outgoing::Logout(wire::LogoutRequest::new()),
                Request::Logout,
            ),
        };
        if self.request(outgoing.0, outgoing.1).is_none() {
            self.emit(EventKind::Auth(Err("The agent is not running.".into())));
        }
    }

    pub(super) fn shutdown(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.probing = false;
        self.replacement = None;
        self.cancel();
        self.shutdown_deadline = Some(Instant::now() + SHUTDOWN_GRACE);
        if self.active.is_none() {
            self.close_session();
        }
    }

    pub(super) fn close_session(&mut self) {
        if self
            .pending
            .values()
            .any(|request| *request == Request::Close)
        {
            return;
        }
        if let Some(session_id) = self.session_id.clone()
            && self
                .features
                .as_ref()
                .is_some_and(|features| features.close)
            && self.link.is_some()
        {
            self.request(
                Outgoing::Close(wire::CloseSessionRequest::new(session_id)),
                Request::Close,
            );
            return;
        }
        self.close_link();
    }

    pub(super) fn close_link(&mut self) {
        self.link = None;
        self.connecting = false;
    }
}

fn route(dispatch: Dispatch, inputs: &UnboundedSender<Input>, epoch: u64) -> Handled<Dispatch> {
    match dispatch {
        Dispatch::Request(message, responder) => {
            match message.method() {
                "session/request_permission" => {
                    match serde_json::from_value::<wire::RequestPermissionRequest>(
                        message.params().clone(),
                    ) {
                        Ok(request) => {
                            let _ = inputs.send(Input::Permission {
                                epoch,
                                request: Box::new(request),
                                responder: responder.cast(),
                            });
                        }
                        Err(error) => {
                            let _ = responder.respond_with_error(
                                sdk::Error::invalid_params().data(error.to_string()),
                            );
                        }
                    }
                }
                "elicitation/create" => {
                    let url = serde_json::from_value::<wire::CreateElicitationRequest>(
                        message.params().clone(),
                    )
                    .ok()
                    .and_then(|request| match request.mode {
                        wire::ElicitationMode::Url(url) => Some(url.url),
                        _ => None,
                    });
                    let action = if let Some(url) = url {
                        let _ = inputs.send(Input::OpenUrl(url));
                        wire::ElicitationAction::Accept(wire::ElicitationAcceptAction::new())
                    } else {
                        wire::ElicitationAction::Decline
                    };
                    let _ = responder
                        .cast::<wire::CreateElicitationResponse>()
                        .respond(wire::CreateElicitationResponse::new(action));
                }
                // Adeline offers no `fs/*` or `terminal/*`.
                method => {
                    let _ = responder
                        .respond_with_error(sdk::Error::method_not_found().data(method.to_owned()));
                }
            }
            Handled::Yes
        }
        Dispatch::Notification(message) => {
            if message.method() == "session/update"
                && let Ok(notification) =
                    serde_json::from_value::<wire::SessionNotification>(message.params().clone())
            {
                let _ = inputs.send(Input::Update {
                    epoch,
                    notification: Box::new(notification),
                });
            }
            Handled::Yes
        }
        dispatch @ Dispatch::Response(..) => Handled::No {
            message: dispatch,
            retry: false,
        },
    }
}

fn classify_line(line: &str) -> TrafficNote {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return TrafficNote::NotJson;
    };
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return TrafficNote::None;
    };
    let known = match method {
        "session/update" => value.pointer("/params/update").is_some_and(|update| {
            serde_json::from_value::<wire::SessionUpdate>(update.clone()).is_ok()
        }),
        "session/request_permission" | "elicitation/create" | "elicitation/complete" => true,
        _ => false,
    };
    if known {
        TrafficNote::None
    } else {
        TrafficNote::Unknown
    }
}

fn trimmed_traffic(line: &str) -> String {
    const LIMIT: usize = 64 * 1024;
    if line.len() <= LIMIT {
        line.to_owned()
    } else {
        let end = line.floor_char_boundary(LIMIT);
        format!("{}… [{} more bytes]", &line[..end], line.len() - end)
    }
}

fn auth_method(method: &wire::AuthMethod) -> AuthMethod {
    match method {
        wire::AuthMethod::Terminal(terminal) => AuthMethod {
            id: terminal.id.0.to_string(),
            name: terminal.name.clone(),
            description: terminal.description.clone().unwrap_or_default(),
            terminal: Some(TerminalLogin {
                arguments: terminal.args.clone(),
                environment: terminal
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            }),
        },
        wire::AuthMethod::Agent(agent) => AuthMethod {
            id: agent.id.0.to_string(),
            name: agent.name.clone(),
            description: agent.description.clone().unwrap_or_default(),
            terminal: None,
        },
        _ => AuthMethod {
            id: method.id().0.to_string(),
            name: method.id().0.to_string(),
            description: String::new(),
            terminal: None,
        },
    }
}
