use super::*;
use crate::conversation::Selections;
use std::sync::Mutex;

fn config() -> ExecutionConfig {
    ExecutionConfig {
        name: "Josh".into(),
        harness: harness::OMP.into(),
        command: "omp.exe".into(),
        arguments: vec!["acp".into()],
        selections: Selections {
            model: "openai-codex/gpt-6-sol".into(),
            effort: "high".into(),
            ..Default::default()
        },
        directory: std::env::current_dir().expect("cwd"),
        ..Default::default()
    }
}

type Sent = Arc<Mutex<Vec<Outgoing>>>;

/// A worker with a ready session whose requests land in `Sent`.
fn worker() -> (Worker, async_channel::Receiver<Event>, Sent) {
    let (events, received) = async_channel::unbounded();
    let (sender, receiver) = mpsc::unbounded_channel();
    let mut worker = Worker::new(
        "conversation-1".into(),
        config(),
        Some("session-1".into()),
        events,
        sender,
        receiver,
    );
    let sent = Sent::default();
    worker.link = Some(Link::Test(sent.clone()));
    worker.features = Some(Features {
        known: true,
        ..Default::default()
    });
    worker.configured = true;
    worker.turn = 1;
    (worker, received, sent)
}

fn events(received: &async_channel::Receiver<Event>) -> Vec<EventKind> {
    std::iter::from_fn(|| received.try_recv().ok().map(|event| event.kind)).collect()
}

fn methods(sent: &Sent) -> Vec<&'static str> {
    sent.lock()
        .unwrap()
        .drain(..)
        .map(|outgoing| match outgoing {
            Outgoing::Initialize(_) => "initialize",
            Outgoing::New(_) => "session/new",
            Outgoing::Load(_) => "session/load",
            Outgoing::Resume(_) => "session/resume",
            Outgoing::Fork(_) => "session/fork",
            Outgoing::SetConfig(_) => "session/set_config_option",
            Outgoing::SetMode(_) => "session/set_mode",
            Outgoing::Prompt(_) => "session/prompt",
            Outgoing::Steer(_) => "_session/steering",
            Outgoing::Close(_) => "session/close",
            Outgoing::Authenticate(_) => "authenticate",
            Outgoing::Logout(_) => "logout",
        })
        .collect()
}

fn options(value: Value) -> Vec<wire::SessionConfigOption> {
    serde_json::from_value(value).expect("config options")
}

fn turn(retries: u32) -> Turn {
    let mut turn = Turn::new(vec![Part::Text("original user request".into())], retries);
    turn.prompt_request = Some(1);
    turn
}

fn error(code: i32, message: &str) -> sdk::Error {
    sdk::Error::new(code, message)
}

#[test]
fn forks_use_the_native_fork_and_fall_back_to_the_text_copy() {
    for native_works in [true, false] {
        let (mut worker, received, sent) = worker();
        worker.session_id = None;
        worker.configured = false;
        worker.config.selections = Selections::default();
        worker.features.as_mut().unwrap().fork = true;
        worker.command(Command::Fork {
            session: Some("source".into()),
            context: "user: one".into(),
        });
        worker.active = Some(Turn::new(vec![Part::Text("two".into())], 0));
        worker.setup_session();
        let id = worker.next_request;
        if native_works {
            worker.reply(
                id,
                Ok(Reply::Session(SessionReply {
                    session_id: Some("forked".into()),
                    ..Default::default()
                })),
            );
        } else {
            worker.reply(id, Err(error(-32601, "no")));
            let id = worker.next_request;
            worker.reply(
                id,
                Ok(Reply::Session(SessionReply {
                    session_id: Some("new".into()),
                    ..Default::default()
                })),
            );
        }
        let prompt = text_of(&worker.active.as_ref().unwrap().prompt);
        let copied = events(&received)
            .iter()
            .any(|event| matches!(event, EventKind::TextCopy));
        if native_works {
            assert_eq!(methods(&sent), ["session/fork", "session/prompt"]);
            assert_eq!(prompt, "two");
            assert!(!copied);
        } else {
            assert_eq!(
                methods(&sent),
                ["session/fork", "session/new", "session/prompt"]
            );
            assert!(prompt.contains("user: one") && prompt.ends_with("two"));
            assert!(copied);
        }
        assert!(worker.fork.is_none());
    }
}

#[test]
fn effort_offered_only_after_the_model_is_set_is_still_applied() {
    let (mut worker, received, sent) = worker();
    worker.configured = false;
    worker.config.selections.model = "gpt-6-astra".into();
    // Codex: the model is already current and effort is not offered yet.
    worker.raw_options = options(json!([{"id":"model","name":"Model","category":"model",
        "type":"select","currentValue":"gpt-6-astra","options":[{"value":"gpt-6-astra","name":"A"}]}]));
    worker.refresh_options();
    worker.configure_next();
    let first = worker.next_request;
    worker.reply(
        first,
        Ok(Reply::Options(options(json!([
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":"gpt-6-astra",
             "options":[{"value":"gpt-6-astra","name":"A"}]},
            {"id":"reasoning_effort","name":"Effort","category":"thought_level","type":"select",
             "currentValue":"medium","options":[{"value":"medium","name":"M"},{"value":"high","name":"H"}]}
        ])))),
    );
    let requests = std::mem::take(&mut *sent.lock().unwrap());
    assert_eq!(requests.len(), 2);
    let Outgoing::SetConfig(effort) = &requests[1] else {
        panic!("effort request missing");
    };
    assert_eq!(effort.config_id.0.as_ref(), "reasoning_effort");
    let second = worker.next_request;
    worker.reply(second, Ok(Reply::Options(Vec::new())));
    assert!(worker.configured);
    assert!(
        events(&received)
            .iter()
            .any(|event| matches!(event, EventKind::Options(options) if options.len() == 2))
    );
}

#[test]
fn agent_without_model_or_effort_options_keeps_its_defaults() {
    let (mut worker, _, sent) = worker();
    worker.configured = false;
    worker.config.selections = Selections::default();
    worker.configure_next();
    assert!(worker.configured);
    assert!(methods(&sent).is_empty());
}

#[test]
fn switching_while_idle_sets_the_option_and_while_stopped_waits_for_setup() {
    let (mut worker, _, sent) = worker();
    worker.raw_options = options(json!([{"id":"thinking","name":"Thinking",
        "category":"thought_level","type":"select","currentValue":"high",
        "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}]));
    worker.refresh_options();
    worker.command(Command::SetOption {
        category: Category::Effort,
        id: String::new(),
        value: "low".into(),
    });
    assert_eq!(worker.config.selections.effort, "low");
    assert_eq!(methods(&sent), ["session/set_config_option"]);
    worker.link = None;
    worker.command(Command::SetOption {
        category: Category::Model,
        id: String::new(),
        value: "xai/grok".into(),
    });
    assert_eq!(worker.config.selections.model, "xai/grok");
    assert!(methods(&sent).is_empty());
}

#[test]
fn session_modes_hide_the_profiles_plan_modes_and_switch_by_set_mode() {
    let (mut worker, _, sent) = worker();
    worker.profile = Some(&profiles::CLAUDE);
    worker.modes = Some(
        serde_json::from_value(json!({"currentModeId":"default","availableModes":[
            {"id":"default","name":"Default"},
            {"id":"plan","name":"Plan"},
            {"id":"acceptEdits","name":"Accept edits","description":"Theirs"}
        ]}))
        .unwrap(),
    );
    worker.refresh_options();
    let mode = crate::conversation::option(&worker.options, Category::Mode).unwrap();
    let values: Vec<_> = mode.choices().iter().map(|c| c.value.as_str()).collect();
    assert_eq!(values, ["default", "acceptEdits"]);
    assert_eq!(
        mode.choices()[0].description,
        "Asks before editing files or running commands."
    );
    assert_eq!(mode.choices()[1].description, "Theirs");
    worker.command(Command::SetOption {
        category: Category::Mode,
        id: String::new(),
        value: "acceptEdits".into(),
    });
    assert_eq!(methods(&sent), ["session/set_mode"]);
    let mode = crate::conversation::option(&worker.options, Category::Mode).unwrap();
    assert_eq!(mode.current(), "acceptEdits");
}

#[test]
fn cancel_discards_pending_retry_and_permission() {
    let (mut worker, received, _) = worker();
    let mut pending = turn(5);
    pending.prompt_request = None;
    pending.retry_at = Some(Instant::now() + Duration::from_secs(30));
    worker.active = Some(pending);
    worker.permissions.insert(
        7,
        PendingPermission {
            turn: 1,
            options: Vec::new(),
            responder: None,
        },
    );
    worker.cancel();
    assert!(worker.active.is_none());
    assert!(worker.permissions.is_empty());
    assert!(matches!(events(&received)[..], [EventKind::Stopped]));
}

#[test]
fn denied_permission_settles_without_retry_even_when_retries_remain() {
    let (mut worker, received, _) = worker();
    worker.active = Some(turn(5));
    worker.permissions.insert(
        7,
        PendingPermission {
            turn: 1,
            options: vec![PermissionOption {
                id: "deny-once".into(),
                name: "Deny once".into(),
                kind: PermissionKind::RejectOnce,
            }],
            responder: None,
        },
    );
    worker.permission(7, "deny-once");
    assert!(worker.active.as_ref().unwrap().denied);
    worker.prompt_result(1, wire::StopReason::EndTurn);
    assert!(worker.active.is_none());
    assert!(matches!(
        events(&received)[..],
        [EventKind::Error {
            kind: FailureKind::Denied,
            ..
        }]
    ));
}

#[test]
fn error_looking_reply_is_reported_without_retry() {
    let (mut worker, received, _) = worker();
    let mut active = turn(5);
    active.observed_text = "Error: Provider timeout".into();
    worker.active = Some(active);
    worker.prompt_result(1, wire::StopReason::EndTurn);
    assert!(worker.active.is_none());
    assert!(matches!(
        events(&received)[..],
        [EventKind::Error {
            kind: FailureKind::Temporary,
            ..
        }]
    ));
}

#[test]
fn temporary_failure_obeys_retry_limit_and_preserves_original_until_work() {
    let (mut worker, received, _) = worker();
    worker.active = Some(turn(2));
    for expected in 1..=2 {
        worker.fail("Provider timed out".into(), FailureKind::Temporary);
        let active = worker.active.as_ref().unwrap();
        assert_eq!(active.attempt, expected);
        assert_eq!(text_of(&active.prompt), "original user request");
        assert!(active.retry_at.is_some());
        assert!(
            matches!(events(&received)[..], [EventKind::Retrying { attempt, .. }] if attempt == expected)
        );
    }
    worker.fail("Provider timed out".into(), FailureKind::Temporary);
    assert!(worker.active.is_none());
    assert!(matches!(
        events(&received)[..],
        [EventKind::Error {
            kind: FailureKind::Temporary,
            ..
        }]
    ));
    assert!(continue_interrupted_turn().contains("Do not repeat completed work"));
}

#[test]
fn changing_retries_affects_the_running_turn() {
    let (mut worker, received, _) = worker();
    worker.active = Some(turn(0));
    worker.command(Command::SetRetries(2));
    worker.fail("Provider timed out".into(), FailureKind::Temporary);
    assert!(matches!(
        events(&received)[..],
        [EventKind::Retrying {
            attempt: 1,
            limit: 2,
            ..
        }]
    ));
}

#[test]
fn unavailable_saved_model_fails_before_prompt() {
    let (mut worker, received, sent) = worker();
    worker.configured = false;
    worker.active = Some(turn(0));
    worker.raw_options = options(json!([{"id":"model","name":"Model","category":"model",
        "type":"select","currentValue":"other/model","options":[{"value":"other/model","name":"Other"}]}]));
    worker.refresh_options();
    events(&received);
    worker.send_prompt();
    assert!(methods(&sent).is_empty());
    worker.configure_next();
    assert!(worker.active.is_none());
    assert!(matches!(
        events(&received)[..],
        [EventKind::Error {
            kind: FailureKind::Configuration,
            ..
        }]
    ));
}

#[test]
fn shutdown_during_setup_never_starts_configuration_or_prompt() {
    let (mut worker, received, sent) = worker();
    worker.session_id = None;
    let mut active = turn(0);
    active.prompt_request = None;
    worker.active = Some(active);
    worker.pending.insert(2, Request::Setup);
    worker.shutdown();
    assert!(matches!(events(&received)[..], [EventKind::Stopped]));
    worker.reply(
        2,
        Ok(Reply::Session(SessionReply {
            session_id: Some("new".into()),
            ..Default::default()
        })),
    );
    assert!(
        events(&received)
            .iter()
            .any(|event| matches!(event, EventKind::Session { .. }))
    );
    assert!(!methods(&sent).contains(&"session/prompt"));
    assert!(worker.active.is_none());
}

#[test]
fn unavailable_session_needs_consent_but_missing_auth_needs_login() {
    let (mut worker, received, _) = worker();
    worker.pending.insert(1, Request::Setup);
    worker.active = Some(turn(0));
    worker.reply(1, Err(error(-32602, "ACP session not found")));
    assert!(worker.restore_required);
    assert!(matches!(
        events(&received)[..],
        [EventKind::ReplacementRequired(_)]
    ));
    let (mut worker, received, _) = self::worker();
    worker.pending.insert(1, Request::Setup);
    worker.active = Some(turn(5));
    worker.reply(1, Err(error(-32000, "Authentication required")));
    assert!(!worker.restore_required);
    assert!(worker.active.is_none());
    assert!(matches!(&events(&received)[..], [EventKind::Error {
        kind: FailureKind::Authentication, message
    }] if message.contains("Log in")));
}

#[test]
fn a_profile_turn_end_signal_ends_the_turn_and_its_late_answer_is_ignored() {
    static SIGNALLING: Profile = Profile {
        turn_end: &["turnEnded"],
        ..profiles::OMP
    };
    let (mut worker, received, _) = worker();
    worker.profile = Some(&SIGNALLING);
    worker.active = Some(turn(0));
    let mut notification: wire::SessionNotification = serde_json::from_value(json!({
        "sessionId":"session-1",
        "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}},
    }))
    .unwrap();
    let mut meta = wire::Meta::new();
    meta.insert("turnEnded".into(), Value::Bool(true));
    notification.meta = Some(meta);
    worker.update(notification);
    assert!(worker.active.is_none());
    assert!(events(&received).iter().any(|event| matches!(
        event,
        EventKind::Finished {
            stop_reason: StopReason::Signal
        }
    )));
    worker.active = Some(Turn::new(vec![Part::Text("next".into())], 0));
    worker.active.as_mut().unwrap().prompt_request = Some(2);
    worker.prompt_result(1, wire::StopReason::EndTurn);
    assert!(
        worker.active.is_some(),
        "an old turn's answer ended the new turn"
    );
}

#[test]
fn failure_classification_avoids_retrying_auth_and_configuration() {
    assert_eq!(
        classify_error("Missing API key; run omp login", None),
        FailureKind::Authentication
    );
    assert_eq!(
        classify_error("anything", Some(-32000)),
        FailureKind::Authentication
    );
    assert_eq!(
        classify_error("Unknown argument --append-system-prompt", None),
        FailureKind::Configuration
    );
    assert_eq!(
        classify_error("Permission denied by user", None),
        FailureKind::Denied
    );
    assert_eq!(
        classify_error("Provider timeout", None),
        FailureKind::Temporary
    );
    assert!(looks_like_agent_error("Error: Provider timeout"));
    assert!(!looks_like_agent_error("I can explain a provider timeout."));
}

#[test]
fn claude_gets_instructions_in_session_meta_with_the_name_sentence_first() {
    let (mut worker, _, _) = worker();
    worker.profile = Some(&profiles::CLAUDE);
    worker.config.system_instructions = "Be brief.".into();
    let meta = worker.session_meta().unwrap();
    assert!(
        meta["systemPrompt"]["append"]
            .as_str()
            .unwrap()
            .starts_with("You are an agent named Josh")
    );
    worker.config.instructions_mode = InstructionsMode::Overwrite;
    assert!(worker.session_meta().unwrap()["systemPrompt"].is_string());
    worker.profile = Some(&profiles::CODEX);
    assert!(worker.session_meta().is_none());
}

#[test]
fn files_are_embedded_or_linked_and_images_need_support() {
    let (mut worker, _, _) = worker();
    let file = Attachment {
        name: "notes.txt".into(),
        mime: "text/plain".into(),
        size: 5,
        data: crate::conversation::base64(b"hello"),
        path: None,
    };
    let image = Attachment {
        name: "shot.png".into(),
        mime: "image/png".into(),
        size: 3,
        data: crate::conversation::base64(b"png"),
        path: None,
    };
    assert!(
        worker
            .content(&[Part::File(image.clone())])
            .unwrap_err()
            .contains("can't receive images")
    );
    let linked = worker.content(&[Part::File(file.clone())]).unwrap();
    assert!(
        matches!(&linked[0], wire::ContentBlock::ResourceLink(link) if link.name == "notes.txt")
    );
    let features = worker.features.as_mut().unwrap();
    features.images = true;
    features.embedded_files = true;
    let blocks = worker
        .content(&[Part::File(image), Part::File(file)])
        .unwrap();
    assert!(
        matches!(&blocks[0], wire::ContentBlock::Image(image) if image.mime_type == "image/png")
    );
    assert!(matches!(&blocks[1], wire::ContentBlock::Resource(_)));
}

// ---------------------------------------------------------------------------
// Through the SDK: a scripted agent on the other end of line channels.

/// A worker connected to a fake agent: lines the worker writes arrive on the
/// returned receiver, and lines sent on the returned sender reach the worker.
fn connected() -> (
    Worker,
    async_channel::Receiver<Event>,
    UnboundedReceiver<String>,
    UnboundedSender<String>,
) {
    let (events, received) = async_channel::unbounded();
    let (sender, receiver) = mpsc::unbounded_channel();
    let mut worker = Worker::new("c".into(), config(), None, events, sender, receiver);
    worker.config.selections = Selections::default();
    let (to_agent, from_worker) = mpsc::unbounded_channel::<String>();
    let (to_worker, from_agent) = mpsc::unbounded_channel::<String>();
    let outgoing = futures::sink::unfold(to_agent, |to_agent, line: String| async move {
        let _ = to_agent.send(line);
        Ok::<_, std::io::Error>(to_agent)
    });
    let incoming = futures::stream::unfold(from_agent, |mut from_agent| async move {
        from_agent.recv().await.map(|line| (Ok(line), from_agent))
    });
    worker.epoch = 1;
    worker.connect_lines(outgoing, incoming);
    worker.connecting = true;
    (worker, received, from_worker, to_worker)
}

/// The next request the worker sent, as (id, method).
async fn next_request(lines: &mut UnboundedReceiver<String>) -> (Value, Value) {
    loop {
        let line = tokio::time::timeout(Duration::from_secs(10), lines.recv())
            .await
            .expect("worker sent nothing")
            .expect("worker closed");
        let message: Value = serde_json::from_str(&line).unwrap();
        if message.get("method").is_some() || message.get("result").is_some() {
            return (message["id"].clone(), message);
        }
    }
}

fn answer(to: &UnboundedSender<String>, id: &Value, result: &Value) {
    to.send(json!({"jsonrpc":"2.0","id":id,"result":result}).to_string())
        .unwrap();
}

async fn wait_for(
    received: &async_channel::Receiver<Event>,
    seen: &mut Vec<EventKind>,
    found: impl Fn(&EventKind) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let kind = received.recv().await.unwrap().kind;
            let done = found(&kind);
            seen.push(kind);
            if done {
                break;
            }
        }
    })
    .await
    .expect("event never came");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_noisy_agent_with_string_ids_and_unknown_updates_keeps_working() {
    let (worker, received, mut from_worker, to_worker) = connected();
    let commands = worker.sender.clone();
    tokio::spawn(worker.run());
    commands
        .send(Input::Command(Command::Prompt {
            prompt: vec![Part::Text("hi".into())],
            retries: 0,
        }))
        .unwrap();
    to_worker.send("not json at all".into()).unwrap();
    let (id, message) = next_request(&mut from_worker).await;
    assert_eq!(message["method"], "initialize");
    assert_eq!(message["params"]["protocolVersion"], 1);
    answer(
        &to_worker,
        &id,
        &json!({"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}),
    );
    let (id, message) = next_request(&mut from_worker).await;
    assert_eq!(message["method"], "session/new");
    answer(&to_worker, &id, &json!({"sessionId":"s1"}));
    let (prompt_id, message) = next_request(&mut from_worker).await;
    assert_eq!(message["method"], "session/prompt");
    to_worker
        .send(
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1",
            "update":{"sessionUpdate":"brand_new_thing","stuff":1}}})
            .to_string(),
        )
        .unwrap();
    to_worker
        .send(
            json!({"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{
            "sessionId":"s1","toolCall":{"toolCallId":"t1","title":"Edit a file"},
            "options":[{"optionId":"yes","name":"Allow","kind":"allow_once"},
                       {"optionId":"never","name":"Never","kind":"reject_always"}]}})
            .to_string(),
        )
        .unwrap();
    let mut seen = Vec::new();
    wait_for(&received, &mut seen, |kind| {
        matches!(kind, EventKind::Permission { .. })
    })
    .await;
    let Some(EventKind::Permission {
        request_id,
        options,
        ..
    }) = seen.last().cloned()
    else {
        unreachable!()
    };
    assert_eq!(options.len(), 1, "reject always is hidden");
    commands
        .send(Input::Command(Command::Permission {
            request_id,
            option_id: "yes".into(),
        }))
        .unwrap();
    let (id, message) = next_request(&mut from_worker).await;
    assert_eq!(id, "perm-1");
    assert_eq!(message["result"]["outcome"]["optionId"], "yes");
    answer(&to_worker, &prompt_id, &json!({"stopReason":"end_turn"}));
    wait_for(&received, &mut seen, |kind| {
        matches!(kind, EventKind::Finished { .. })
    })
    .await;
    let notes: Vec<_> = seen
        .iter()
        .filter_map(|kind| match kind {
            EventKind::Traffic(entry) => Some(entry.note),
            _ => None,
        })
        .collect();
    assert!(notes.contains(&TrafficNote::NotJson));
    assert!(notes.contains(&TrafficNote::Unknown));
}

#[tokio::test(flavor = "multi_thread")]
async fn session_state_between_turns_is_reported() {
    let (worker, received, mut from_worker, to_worker) = connected();
    let commands = worker.sender.clone();
    tokio::spawn(worker.run());
    commands.send(Input::Command(Command::Probe)).unwrap();
    let (id, _) = next_request(&mut from_worker).await;
    answer(
        &to_worker,
        &id,
        &json!({"protocolVersion":1,"agentCapabilities":{
        "promptCapabilities":{"image":true},"sessionCapabilities":{"close":{}}},
        "authMethods":[{"id":"browser","name":"Log in with a browser"}],
        "_meta":{"steering":{"supported":true}}}),
    );
    let (id, _) = next_request(&mut from_worker).await;
    answer(&to_worker, &id, &json!({"sessionId":"s1"}));
    let mut seen = Vec::new();
    wait_for(&received, &mut seen, |kind| {
        matches!(kind, EventKind::Probed)
    })
    .await;
    let update = |update: Value| {
        to_worker
            .send(
                json!({"jsonrpc":"2.0","method":"session/update",
                "params":{"sessionId":"s1","update":update}})
                .to_string(),
            )
            .unwrap();
    };
    update(
        json!({"sessionUpdate":"available_commands_update","availableCommands":[
        {"name":"review","description":"Review the diff","input":{"hint":"focus"}}]}),
    );
    update(json!({"sessionUpdate":"session_info_update","title":"Fix the parser"}));
    update(json!({"sessionUpdate":"plan","entries":[
        {"content":"Read","priority":"high","status":"completed"},
        {"content":"Write","priority":"high","status":"in_progress"}]}));
    update(json!({"sessionUpdate":"usage_update","used":10,"size":100}));
    wait_for(&received, &mut seen, |kind| {
        matches!(kind, EventKind::Usage { .. })
    })
    .await;
    let features = seen.iter().find_map(|kind| match kind {
        EventKind::Agent { features, .. } => Some(features.clone()),
        _ => None,
    });
    let features = features.unwrap();
    assert!(features.images && features.close && features.steering && !features.fork);
    assert_eq!(features.auth[0].name, "Log in with a browser");
    assert!(seen.iter().any(|kind| matches!(kind,
        EventKind::Commands(commands) if commands[0].hint == "focus")));
    assert!(
        seen.iter()
            .any(|kind| matches!(kind, EventKind::Title(title) if title == "Fix the parser"))
    );
    assert!(seen.iter().any(|kind| matches!(kind,
        EventKind::Todo(steps) if steps[1].status == StepStatus::InProgress)));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_on_another_protocol_version_gets_a_configuration_error() {
    let (worker, received, mut from_worker, to_worker) = connected();
    let commands = worker.sender.clone();
    tokio::spawn(worker.run());
    commands.send(Input::Command(Command::Probe)).unwrap();
    let (id, _) = next_request(&mut from_worker).await;
    answer(
        &to_worker,
        &id,
        &json!({"protocolVersion":2,"agentCapabilities":{},"authMethods":[]}),
    );
    let mut seen = Vec::new();
    wait_for(&received, &mut seen, |kind| {
        matches!(kind, EventKind::Error { .. })
    })
    .await;
    assert!(matches!(seen.last(), Some(EventKind::Error {
        kind: FailureKind::Configuration, message
    }) if message.contains("Adeline supports version 1")));
}

#[tokio::test(flavor = "multi_thread")]
async fn driver_reports_an_agent_that_exits_at_once() {
    let mut config = config();
    config.harness = "Other".into();
    #[cfg(windows)]
    let (command, arguments) = ("cmd.exe", ["/C", "echo boom 1>&2 & exit 1"]);
    #[cfg(not(windows))]
    let (command, arguments) = ("sh", ["-c", "echo boom >&2; exit 1"]);
    config.command = command.into();
    config.arguments = arguments.map(Into::into).to_vec();
    let (events, received) = async_channel::unbounded();
    let driver = Driver::spawn("conversation-1".into(), config, None, events);
    driver
        .send(Command::Prompt {
            prompt: vec![Part::Text("hello".into())],
            retries: 0,
        })
        .unwrap();
    let mut seen = Vec::new();
    wait_for(&received, &mut seen, |kind| {
        matches!(kind, EventKind::Error { .. })
    })
    .await;
    assert!(
        matches!(seen.last(), Some(EventKind::Error { message, .. }) if message.contains("Agent exited")),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|kind| matches!(kind, EventKind::Crashed(stderr) if stderr.contains("boom")))
    );
}
