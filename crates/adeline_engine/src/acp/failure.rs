//! Failed requests, retries, a lost agent, and telling agent errors from
//! temporary and authentication failures.
use super::*;

impl Worker {
    pub(super) fn failed(&mut self, request: Request, error: &sdk::Error) {
        // The connection ended: the process exit reports it, with its status.
        if sdk::is_incoming_transport_closed(error) {
            match request {
                Request::Setup => self.setup = None,
                Request::Steer => self.emit(EventKind::Steered { delivered: false }),
                Request::Authenticate | Request::Logout => {
                    self.emit(EventKind::Auth(Err("The agent stopped.".into())));
                }
                _ => {}
            }
            return;
        }
        // A failed native fork falls back to a new session with the text copy.
        if request == Request::Setup && self.forking {
            self.setup = None;
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
                // The agent keeps its own value; the menus show it again.
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
            && self.session_id.is_some()
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
            self.setup = None;
            self.restore_failed(&format!("Cannot restore the saved agent session: {text}"));
        } else if request == Request::Close {
            self.emit(EventKind::Error {
                message: format!("Closing the agent session failed: {text}"),
                kind,
            });
            self.shutdown_deadline
                .get_or_insert_with(|| Instant::now() + SHUTDOWN_GRACE);
        } else {
            if request == Request::Setup {
                self.setup = None;
            }
            self.fail(format!("ACP {request:?} failed: {text}"), kind);
        }
    }

    pub(super) fn fail(&mut self, message: String, kind: FailureKind) {
        let message = if kind == FailureKind::Authentication {
            format!("{message} Log in, then retry.")
        } else {
            message
        };
        if kind == FailureKind::Temporary
            && !self.closing
            && let Some(turn) = &mut self.active
            && !turn.cancelled
            && !turn.denied
            && turn.attempt < turn.retries
        {
            turn.attempt += 1;
            let attempt = turn.attempt;
            let limit = turn.retries;
            turn.prompt_request = None;
            turn.retry_at = Some(
                Instant::now() + Duration::from_secs(u64::from(attempt.min(MAX_RETRY_DELAY_SECS))),
            );
            self.emit(EventKind::Retrying {
                attempt,
                limit,
                error: message,
            });
            return;
        }
        self.emit(EventKind::Error { message, kind });
        self.active = None;
        self.cancel_permissions();
    }

    pub(super) fn retry_now(&mut self) {
        let Some(turn) = &mut self.active else {
            return;
        };
        turn.retry_at = None;
        if turn.worked {
            turn.prompt = vec![Part::Text(CONTINUE_TURN.into())];
            if self.child.is_none() {
                self.start();
            } else {
                self.setup_session();
            }
        } else if self.child.is_none() {
            self.start();
        } else if !self.configured {
            if self.features.is_none() {
                self.initialize();
            } else {
                self.setup_session();
            }
        } else {
            self.send_prompt();
        }
    }

    pub(super) fn restore_failed(&mut self, reason: &str) {
        self.restore_required = true;
        self.emit(EventKind::ReplacementRequired(format!("{reason} You can start a replacement session with saved conversation content after confirming.")));
        if let Some(turn) = &mut self.active {
            turn.prompt_request = None;
            turn.retry_at = None;
        }
    }

    // Replies to these will never come; a steered message goes back to the queue.
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

    pub(super) fn lost(&mut self, message: String) {
        let initializing = self.lost_during_initialize
            || self
                .pending
                .values()
                .any(|request| *request == Request::Initialize);
        self.lost_during_initialize = initializing;
        let invalid_initialize =
            !self.closing && initializing && message.starts_with("ACP connection failed");
        self.link = None;
        self.connecting = false;
        self.clear_pending();
        self.cancel_permissions();
        self.setup = None;
        self.configured = false;
        self.handshake = None;
        self.lost_pending = Some(message.clone());
        self.lost_at = Some(Instant::now());
        self.shutdown_deadline
            .get_or_insert_with(|| Instant::now() + SHUTDOWN_GRACE);
        if invalid_initialize && self.config.harness == harness::OMP {
            self.timeout_reported = true;
            self.fail(
                format!("{message} The configured OMP Arguments must include `acp`."),
                FailureKind::Configuration,
            );
        } else if invalid_initialize {
            self.timeout_reported = true;
            self.fail(message, FailureKind::Configuration);
        } else if self.child.is_none() && !self.closing {
            self.fail(message, FailureKind::Temporary);
        }
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

// A write to an agent that already exited fails with a broken pipe before its exit is seen.
// The SDK keeps only the error's text, so the OS error code is read back from it.
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

// A match starts a word and ends one, or runs into a capital as in `AuthenticationError`.
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
    // Some agents pass on their provider's HTTP 401.
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
