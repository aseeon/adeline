//! Failed requests, retries, a lost agent, and telling agent errors from
//! temporary and authentication failures.
use super::*;

impl Worker {
    fn step_back(&mut self, request: Request) {
        let back = match (request, self.setup()) {
            (Request::Initialize, Some(Setup::Initializing)) => Setup::Uninitialized,
            (Request::Setup, Some(Setup::Opening { .. }))
            | (Request::Config, Some(Setup::Configuring(_))) => Setup::Initialized,
            _ => return,
        };
        self.set_setup(back);
    }

    pub(super) fn failed(&mut self, request: Request, error: &sdk::Error) {
        let forked =
            request == Request::Setup && self.setup() == Some(Setup::Opening { fork: true });
        self.step_back(request);
        if sdk::is_incoming_transport_closed(error) {
            match request {
                Request::Steer => self.emit(EventKind::Steered { delivered: false }),
                Request::Authenticate | Request::Logout => {
                    self.emit(EventKind::Auth(Err("The agent stopped.".into())));
                }
                _ => {}
            }
            return;
        }
        if forked {
            if let Some((session, _)) = &mut self.fork {
                *session = None;
            }
            self.setup_session();
            return;
        }
        let text = error_text(error);
        let code = i64::from(i32::from(error.code));
        match request {
            Request::Steer => {
                self.emit(EventKind::Steered { delivered: false });
                return;
            }
            Request::Authenticate | Request::Logout => {
                self.emit(EventKind::Auth(Err(text)));
                return;
            }
            Request::Switch => {
                self.emit(EventKind::Error {
                    message: format!("The agent did not change the setting: {text}"),
                    kind: FailureKind::Configuration,
                });
                self.emit(EventKind::Options(self.options.clone()));
                return;
            }
            _ => {}
        }
        let kind =
            if request == Request::Prompt && self.active.as_ref().is_some_and(|turn| turn.denied) {
                FailureKind::Denied
            } else {
                classify_error(&text, Some(code))
            };
        if request == Request::Setup
            && self.session_id().is_some()
            && kind != FailureKind::Authentication
            && (code == -32601
                || [
                    "session not found",
                    "unknown session",
                    "no such session",
                    "cannot resume session",
                    "cannot load session",
                ]
                .iter()
                .any(|needle| contains_ascii(&text, needle)))
        {
            self.restore_failed(&format!("Cannot restore the saved agent session: {text}"));
        } else if request == Request::Close {
            self.emit(EventKind::Error {
                message: format!("Closing the agent session failed: {text}"),
                kind,
            });
            self.exit = Exit::Closing;
            self.watch.arm();
        } else {
            self.fail(format!("ACP {request:?} failed: {text}"), kind);
        }
    }

    pub(super) fn fail(&mut self, message: String, kind: FailureKind) {
        if kind == FailureKind::Temporary
            && self.exit == Exit::Running
            && let Some(turn) = &mut self.active
            && !matches!(turn.step, Step::Cancelling(_))
            && !turn.denied
            && turn.attempt < turn.retries
        {
            turn.attempt += 1;
            let attempt = turn.attempt;
            let limit = turn.retries;
            turn.step = Step::Backoff(
                Instant::now() + Duration::from_secs(u64::from(attempt.min(MAX_RETRY_DELAY_SECS))),
            );
            self.emit(EventKind::Retrying {
                attempt,
                limit,
                error: message,
            });
            return;
        }
        self.give_up(message, kind);
    }

    pub(super) fn give_up(&mut self, message: String, kind: FailureKind) {
        let message = if kind == FailureKind::Authentication {
            format!("{message} Log in, then retry.")
        } else {
            message
        };
        self.emit(EventKind::Error { message, kind });
        self.end_turn();
        self.cancel_permissions();
    }

    pub(super) fn retry_now(&mut self) {
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.step = Step::Waiting;
        let worked = turn.worked;
        if worked {
            turn.prompt = vec![Part::Text(CONTINUE_TURN.into())];
        }
        match self.setup() {
            _ if matches!(self.connection, Connection::Down) => self.start(),
            Some(Setup::Ready) if !worked => self.send_prompt(),
            setup if worked || setup.is_some_and(Setup::initialized) => self.setup_session(),
            _ => self.initialize(),
        }
    }

    pub(super) fn restore_failed(&mut self, reason: &str) {
        if let AgentSession::Saved(id) = &self.session {
            self.session = AgentSession::Unrestorable(id.clone());
        }
        self.set_setup(Setup::Initialized);
        self.emit(EventKind::ReplacementRequired(format!("{reason} You can start a replacement session with saved conversation content after confirming.")));
        if let Some(turn) = &mut self.active {
            turn.step = Step::Parked;
        }
    }

    pub(super) fn clear_pending(&mut self) {
        if self
            .pending
            .values()
            .any(|request| *request == Request::Steer)
        {
            self.emit(EventKind::Steered { delivered: false });
        }
        self.pending.clear();
    }

    pub(super) fn initializing(&self) -> bool {
        match &self.connection {
            Connection::Connecting => true,
            Connection::Up(_, setup) => !setup.initialized(),
            Connection::Lost(lost) => lost.initializing,
            Connection::Down | Connection::Dropped => false,
        }
    }

    pub(super) fn lost(&mut self, message: String) {
        let initializing = self.initializing();
        let reported = matches!(&self.connection, Connection::Lost(lost) if lost.reported);
        let invalid_initialize = self.exit == Exit::Running
            && initializing
            && message.starts_with("ACP connection failed");
        self.connection = Connection::Lost(Lost {
            error: message.clone(),
            at: Instant::now(),
            initializing,
            reported: reported || invalid_initialize,
        });
        self.clear_pending();
        self.cancel_permissions();
        self.handshake = None;
        self.watch.arm();
        if invalid_initialize && self.config.harness == harness::OMP {
            self.fail(
                format!("{message} The configured OMP Arguments must include `acp`."),
                FailureKind::Configuration,
            );
        } else if invalid_initialize {
            self.fail(message, FailureKind::Configuration);
        } else if self.child.is_none() && self.exit == Exit::Running {
            self.fail(message, FailureKind::Temporary);
        }
    }

    pub(super) fn exited(&mut self, status: std::process::ExitStatus) -> bool {
        let Connection::Lost(lost) = std::mem::replace(&mut self.connection, Connection::Down)
        else {
            return false;
        };
        let stderr = self.stderr.make_contiguous().join("\n");
        self.child = None;
        self.clear_pending();
        self.handshake = None;
        self.cancel_permissions();
        self.epoch += 1;
        self.watch = ExitWatch::Idle;
        if self.exit != Exit::Running {
            self.emit(EventKind::ShutdownComplete);
            return true;
        }
        if !lost.reported {
            if !stderr.is_empty() {
                self.emit(EventKind::Crashed(stderr));
            }
            let error = format!("{} Agent exited: {status}.", lost.error);
            let kind = classify_error(&error, None);
            if lost.initializing
                && kind == FailureKind::Temporary
                && self.config.harness == harness::OMP
            {
                self.fail(
                    format!("{error} The configured OMP Arguments must include `acp`."),
                    FailureKind::Configuration,
                );
            } else {
                self.fail(error, kind);
            }
        }
        false
    }
}

fn error_text(error: &sdk::Error) -> String {
    match error.data.as_ref().and_then(Value::as_str) {
        Some(data) if !data.is_empty() && !error.message.contains(data) => {
            format!("{} ({data})", error.message)
        }
        _ => error.message.clone(),
    }
}

pub(super) fn broken_pipe(error: &sdk::Error) -> bool {
    error
        .data
        .as_ref()
        .and_then(Value::as_str)
        .and_then(|text| {
            text.rsplit_once("(os error ")?
                .1
                .strip_suffix(')')?
                .parse()
                .ok()
        })
        .is_some_and(|code| {
            std::io::Error::from_raw_os_error(code).kind() == std::io::ErrorKind::BrokenPipe
        })
}

pub(super) fn with_context(context: &str, text: &str) -> String {
    format!(
        "Saved conversation context (prior history; do not repeat completed tool actions):\n{context}\n\nNew user request:\n{text}"
    )
}

fn contains_ascii(text: &str, needle: &str) -> bool {
    text.as_bytes()
        .windows(needle.len())
        .any(|bytes| bytes.eq_ignore_ascii_case(needle.as_bytes()))
}

fn contains_word(text: &str, needle: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(needle.len() - 1)).any(|start| {
        let end = start + needle.len();
        bytes[start..end].eq_ignore_ascii_case(needle.as_bytes())
            && (start == 0 || !bytes[start - 1].is_ascii_alphanumeric())
            && bytes
                .get(end)
                .is_none_or(|next| !next.is_ascii_lowercase() && !next.is_ascii_digit())
    })
}

pub(super) fn classify_error(message: &str, code: Option<i64>) -> FailureKind {
    use sdk::ErrorCode;
    let code = code
        .and_then(|code| i32::try_from(code).ok())
        .map(ErrorCode::from);
    if matches!(code, Some(ErrorCode::AuthRequired | ErrorCode::Other(401)))
        || [
            "authentication",
            "unauthenticated",
            "unauthorized",
            "not logged in",
            "log in",
            "login",
            "api key",
            "401",
        ]
        .iter()
        .any(|needle| contains_word(message, needle))
    {
        FailureKind::Authentication
    } else if contains_ascii(message, "permission denied")
        || contains_ascii(message, "denied by user")
    {
        FailureKind::Denied
    } else if matches!(
        code,
        Some(ErrorCode::InvalidRequest | ErrorCode::MethodNotFound | ErrorCode::InvalidParams)
    ) || [
        "invalid model",
        "unsupported",
        "unknown acp",
        "unknown option",
        "unknown argument",
        "unrecognized option",
    ]
    .iter()
    .any(|needle| contains_ascii(message, needle))
    {
        FailureKind::Configuration
    } else {
        FailureKind::Temporary
    }
}

pub(super) fn looks_like_agent_error(text: &str) -> bool {
    let text = text.trim_start();
    [
        "error:",
        "api error:",
        "authentication failed",
        "missing api key",
        "no api key",
        "authentication required",
        "not logged in",
        "you need to log in",
        "rate limit exceeded",
        "provider error:",
    ]
    .iter()
    .any(|prefix| {
        text.get(..prefix.len())
            .is_some_and(|begin| begin.eq_ignore_ascii_case(prefix))
    })
}
