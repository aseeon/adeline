//! The engine end to end, on every OS. Each test starts the real binary's
//! `adeline bridge` in its own temporary home, which starts an engine there,
//! and `scripts/engine-check/fake_agent.py` plays the agent (its keywords
//! script each turn). A test's name says the failure it guards against.
//! Needs Python 3 on PATH.
#![expect(
    clippy::disallowed_methods,
    reason = "tests block on processes and sleep off any UI thread"
)]

mod common;

use adeline_core::conversation::Attachment;
use adeline_core::protocol::{ClientMessage, Command, EngineSettings, Live, PROTOCOL, Resume};
use common::PROJECT;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead as _, BufReader, Write as _},
    path::PathBuf,
    process::{Child, ChildStdin, Command as Process, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(20);

struct Home {
    root: PathBuf,
    engine: Option<u32>,
}

impl Home {
    fn new(name: &str) -> Self {
        Self {
            root: common::home(name),
            engine: None,
        }
    }

    fn connect(&mut self) -> Client {
        self.connect_from(None)
    }

    fn connect_from(&mut self, resume: Option<Resume>) -> Client {
        let mut bridge = Process::new(env!("CARGO_BIN_EXE_adeline"))
            .arg("bridge")
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root)
            .env("ADELINE_FAKE_STATE", self.root.join("fake-state"))
            .env("ADELINE_FAKE_PIDS", self.root.join("fake-pids"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = bridge.stdin.take().unwrap();
        let output = BufReader::new(bridge.stdout.take().unwrap());
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines() {
                let Ok(line) = line else { break };
                if sender.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut client = Client {
            bridge,
            input,
            messages,
            next: 0,
            live: HashMap::new(),
            deltas: Vec::new(),
            snapshot: None,
            resumed: false,
        };
        client.write(&ClientMessage::Hello {
            protocol: PROTOCOL,
            cli: false,
            resume,
        });
        let welcome = client.next_message();
        assert_eq!(welcome["type"], "welcome", "{welcome}");
        self.engine = welcome["status"]["pid"]
            .as_u64()
            .and_then(|pid| pid.try_into().ok());
        let first = client.next_message();
        client.handle(&first);
        client
    }

    fn kill_engine(&mut self) {
        let Some(pid) = self.engine.take() else {
            return;
        };
        kill(pid);
        #[cfg(windows)]
        {
            let deadline = Instant::now() + WAIT;
            while alive(pid) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    fn kill_agent(&self) {
        let deadline = Instant::now() + WAIT;
        loop {
            let pid = fs::read_dir(self.root.join("fake-pids"))
                .into_iter()
                .flatten()
                .flatten()
                .find_map(|entry| entry.file_name().to_str()?.parse().ok());
            if let Some(pid) = pid {
                return kill(pid);
            }
            assert!(Instant::now() < deadline, "no agent started");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn transcript(&self, conversation: &str) -> PathBuf {
        self.root
            .join(".config/adeline/projects")
            .join(PROJECT)
            .join("conversations")
            .join(conversation)
            .join("transcript.jsonl")
    }

    fn sent(&self, conversation: &str, method: &str) -> Vec<Value> {
        fs::read_to_string(self.transcript(conversation))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| event["kind"] == "traffic" && event["data"]["direction"] == "to_agent")
            .filter_map(|event| serde_json::from_str::<Value>(event["data"]["text"].as_str()?).ok())
            .filter(|message| message["method"] == method)
            .collect()
    }

    fn wait_sent(&self, conversation: &str, method: &str, count: usize) -> Vec<Value> {
        let deadline = Instant::now() + WAIT;
        loop {
            let sent = self.sent(conversation, method);
            if sent.len() >= count {
                return sent;
            }
            assert!(
                Instant::now() < deadline,
                "{method} sent {} times",
                sent.len()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        self.kill_engine();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn kill(pid: u32) {
    #[cfg(windows)]
    let killed = Process::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output();
    #[cfg(not(windows))]
    let killed = Process::new("kill")
        .args(["-KILL", &pid.to_string()])
        .output();
    killed.unwrap();
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let output = Process::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!(" {pid} "))
}

struct Client {
    bridge: Child,
    input: ChildStdin,
    messages: mpsc::Receiver<Value>,
    next: u64,
    live: HashMap<String, Live>,
    deltas: Vec<Value>,
    snapshot: Option<Value>,
    resumed: bool,
}

impl Client {
    fn write(&mut self, message: &ClientMessage) {
        let line = serde_json::to_string(message).unwrap();
        writeln!(self.input, "{line}").unwrap();
        self.input.flush().unwrap();
    }

    fn next_message(&self) -> Value {
        self.messages
            .recv_timeout(WAIT)
            .expect("the engine went quiet")
    }

    fn handle(&mut self, message: &Value) {
        match message["type"].as_str() {
            Some("snapshot") => {
                self.live = serde_json::from_value(message["live"].clone()).unwrap();
                self.snapshot = Some(message.clone());
            }
            Some("resumed") => self.resumed = true,
            Some("delta") => {
                if message["delta"] == "live" {
                    let id = message["id"].as_str().unwrap().to_owned();
                    let live = serde_json::from_value(message["live"].clone()).unwrap();
                    self.live.insert(id, live);
                }
                self.deltas.push(message.clone());
            }
            _ => {}
        }
    }

    fn call(&mut self, command: Command) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        self.write(&ClientMessage::Request { id, command });
        loop {
            let message = self.next_message();
            self.handle(&message);
            if message["type"] == "reply" && message["id"] == id {
                return serde_json::from_value(message["result"].clone()).unwrap();
            }
        }
    }

    fn send(&mut self, conversation: Option<&str>, prompt: &str) -> String {
        let reply = self.call(Command::Send {
            project_id: PROJECT.into(),
            conversation_id: conversation.map(Into::into),
            agent_id: conversation.is_none().then(|| "fake".into()),
            prompt: prompt.into(),
            attachments: Vec::new(),
            now: false,
        });
        reply.unwrap().as_str().unwrap().to_owned()
    }

    fn live(&self, conversation: &str) -> Live {
        self.live.get(conversation).cloned().unwrap_or_default()
    }

    fn wait_live(&mut self, conversation: &str, test: impl Fn(&Live) -> bool) -> Live {
        let deadline = Instant::now() + WAIT;
        while !test(&self.live(conversation)) {
            let left = deadline.saturating_duration_since(Instant::now());
            let message = self
                .messages
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("waited for {:?}", self.live(conversation)));
            self.handle(&message);
        }
        self.live(conversation)
    }

    fn wait_turn(&mut self, conversation: &str) -> Live {
        self.wait_live(conversation, |live| live.processing);
        self.wait_live(conversation, |live| !live.processing)
    }

    fn position(&self) -> Resume {
        let snapshot = self.snapshot.as_ref().unwrap();
        Resume {
            epoch: snapshot["epoch"].as_str().unwrap().to_owned(),
            seq: snapshot["seq"].as_u64().unwrap() + self.deltas.len() as u64,
        }
    }

    fn settle(&mut self) {
        while let Ok(message) = self.messages.recv_timeout(Duration::from_millis(500)) {
            self.handle(&message);
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.bridge.kill();
        let _ = self.bridge.wait();
    }
}

#[test]
fn an_engine_killed_mid_turn_marks_the_turn_interrupted_and_retry_finishes_it() {
    let mut home = Home::new("killed");
    let mut client = home.connect();
    let id = client.send(None, "SLOW 30");
    client.wait_live(&id, |live| live.processing);
    home.kill_engine();
    drop(client);
    let mut client = home.connect();
    let error = client.live(&id).error.unwrap_or_default();
    assert!(error.contains("interrupted"), "{error:?}");
    client.call(Command::Retry { id: id.clone() }).unwrap();
    let live = client.wait_turn(&id);
    assert_eq!(live.error, None);
}

#[test]
fn a_permission_answered_from_two_windows_counts_once() {
    let mut home = Home::new("permission-race");
    let mut first = home.connect();
    let mut second = home.connect();
    let id = first.send(None, "PERMISSION");
    let live = first.wait_live(&id, |live| !live.permission.is_empty());
    second.wait_live(&id, |live| !live.permission.is_empty());
    let request = &live.permission[0];
    let answer = Command::AnswerPermission {
        id: id.clone(),
        request_id: request.request_id,
        option_id: "allow".into(),
    };
    first.call(answer.clone()).unwrap();
    assert_eq!(
        second.call(answer),
        Err("Permission already answered".into())
    );
    assert_eq!(first.wait_live(&id, |live| !live.processing).error, None);
}

#[test]
fn a_permission_asked_with_no_window_open_stops_the_turn_and_retry_continues_it() {
    let mut home = Home::new("permission-alone");
    let mut client = home.connect();
    let id = client.send(None, "PERMISSION");
    client.wait_live(&id, |live| !live.permission.is_empty());
    drop(client);
    let deadline = Instant::now() + WAIT;
    let mut client = loop {
        let client = home.connect();
        if !client.live(&id).processing {
            break client;
        }
        assert!(Instant::now() < deadline, "the turn kept waiting");
        drop(client);
        std::thread::sleep(Duration::from_millis(500));
    };
    let error = client.live(&id).error.unwrap_or_default();
    assert!(error.contains("waiting for permission"), "{error:?}");
    client.call(Command::Retry { id: id.clone() }).unwrap();
    let live = client.wait_live(&id, |live| !live.permission.is_empty() || !live.processing);
    if let Some(request) = live.permission.first() {
        let request_id = request.request_id;
        client
            .call(Command::AnswerPermission {
                id: id.clone(),
                request_id,
                option_id: "allow".into(),
            })
            .unwrap();
    }
    assert_eq!(client.wait_live(&id, |live| !live.processing).error, None);
}

#[test]
fn a_message_sent_during_a_turn_waits_in_the_queue_and_runs_next() {
    let mut home = Home::new("queue");
    let mut client = home.connect();
    let id = client.send(None, "SLOW 2");
    client.wait_live(&id, |live| live.processing);
    client.send(Some(&id), "and then this");
    client.wait_live(&id, |live| live.queued.len() == 1);
    client.wait_live(&id, |live| live.queued.is_empty() && !live.processing);
    let prompts: Vec<_> = home
        .wait_sent(&id, "session/prompt", 2)
        .iter()
        .map(|message| message["params"]["prompt"][0]["text"].clone())
        .collect();
    assert_eq!(prompts, [json!("SLOW 2"), json!("and then this")]);
}

#[test]
fn a_queued_message_taken_back_keeps_its_attachments() {
    let mut home = Home::new("take-queued");
    let mut client = home.connect();
    let id = client.send(None, "SLOW 2");
    client.wait_live(&id, |live| live.processing);
    let file = Attachment {
        name: "note.txt".into(),
        mime: "text/plain".into(),
        size: 2,
        data: adeline_core::conversation::base64(b"hi"),
        path: None,
    };
    client
        .call(Command::Send {
            project_id: PROJECT.into(),
            conversation_id: Some(id.clone()),
            agent_id: None,
            prompt: "with a file".into(),
            attachments: vec![file.clone()],
            now: false,
        })
        .unwrap();
    let queued = client.wait_live(&id, |live| live.queued.len() == 1).queued[0].id;
    let taken = client
        .call(Command::TakeQueued {
            id: id.clone(),
            queued,
        })
        .unwrap();
    assert_eq!(taken, json!(["with a file", [file]]));
}

#[test]
fn a_reconnect_gets_only_the_deltas_it_missed_in_order() {
    let mut home = Home::new("resume");
    let mut watcher = home.connect();
    let id = watcher.send(None, "hello");
    watcher.wait_turn(&id);
    let dropped = home.connect();
    let left_at = dropped.position();
    drop(dropped);
    watcher.send(Some(&id), "SLOW 2");
    watcher.wait_live(&id, |live| live.processing);
    let mut back = home.connect_from(Some(left_at.clone()));
    assert!(back.resumed && back.snapshot.is_none());
    watcher.wait_live(&id, |live| !live.processing);
    watcher.settle();
    back.settle();
    let since = usize::try_from(left_at.seq).unwrap()
        - usize::try_from(watcher.snapshot.as_ref().unwrap()["seq"].as_u64().unwrap()).unwrap();
    let shared = back.deltas.len().min(watcher.deltas.len() - since);
    let text = back
        .deltas
        .iter()
        .position(|delta| delta["delta"] == "text")
        .expect("the missed text");
    let turn_end = back
        .deltas
        .iter()
        .rposition(|delta| {
            delta["delta"] == "live"
                && delta["id"] == id.as_str()
                && delta["live"]["processing"] == false
        })
        .expect("the missed turn end");
    assert!(
        text < turn_end && turn_end < shared,
        "{text} {turn_end} {shared}"
    );
    assert_eq!(
        back.deltas[..shared],
        watcher.deltas[since..since + shared],
        "nothing earlier, nothing twice, nothing out of order"
    );
    let elsewhere = home.connect_from(Some(Resume {
        epoch: "another engine".into(),
        seq: 3,
    }));
    assert!(!elsewhere.resumed && elsewhere.snapshot.is_some());
}

#[test]
fn a_silent_agent_is_flagged_quiet_and_force_stop_frees_the_conversation() {
    let mut home = Home::new("silent");
    let mut client = home.connect();
    let id = client.send(None, "SILENT");
    client.wait_live(&id, |live| live.quiet_since.is_some());
    client.call(Command::Stop { id: id.clone() }).unwrap();
    client.call(Command::ForceStop { id: id.clone() }).unwrap();
    client.wait_live(&id, |live| !live.processing);
    client.send(Some(&id), "are you back?");
    assert_eq!(client.wait_turn(&id).error, None);
}

#[test]
fn an_agent_that_exits_mid_turn_is_restarted_and_without_retries_its_stderr_is_shown() {
    let mut home = Home::new("exit");
    let mut client = home.connect();
    let id = client.send(None, "EXIT");
    client.wait_live(&id, |live| {
        live.progress
            .as_deref()
            .is_some_and(|progress| progress.contains("Agent exited"))
    });
    assert_eq!(client.wait_live(&id, |live| !live.processing).error, None);
    let prompts = home.wait_sent(&id, "session/prompt", 2);
    assert_ne!(prompts[1]["params"]["prompt"][0]["text"], "EXIT");
    client
        .call(Command::SetSettings {
            settings: EngineSettings {
                retry_limit: 0,
                ..EngineSettings::default()
            },
        })
        .unwrap();
    client.send(Some(&id), "EXIT");
    let live = client.wait_turn(&id);
    let error = live.error.unwrap_or_default();
    assert!(error.contains("exited"), "{error:?}");
    assert!(
        live.stderr.contains("crashing on purpose"),
        "{:?}",
        live.stderr
    );
}

#[test]
fn a_lost_session_waits_for_consent_and_the_replacement_carries_the_history() {
    let mut home = Home::new("lost-session");
    let mut client = home.connect();
    let id = client.send(None, "LOSE_SESSION remember the parrot");
    client.wait_turn(&id);
    home.kill_engine();
    drop(client);
    let mut client = home.connect();
    client.send(Some(&id), "what did I ask you to remember?");
    client.wait_live(&id, |live| live.replacement);
    assert_eq!(home.sent(&id, "session/prompt").len(), 1);
    client
        .call(Command::ReplaceSession { id: id.clone() })
        .unwrap();
    client.wait_live(&id, |live| !live.replacement && !live.processing);
    let prompts = home.wait_sent(&id, "session/prompt", 2);
    let text = prompts[1]["params"]["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("remember the parrot"), "{text}");
}

#[test]
fn a_replacement_approved_after_its_agent_stopped_runs_the_turn_to_its_end() {
    let mut home = Home::new("replace-later");
    let mut client = home.connect();
    let id = client.send(None, "LOSE_SESSION remember the parrot");
    client.wait_turn(&id);
    home.kill_engine();
    drop(client);
    let mut client = home.connect();
    client.send(Some(&id), "what did I ask you to remember?");
    client.wait_live(&id, |live| live.replacement);
    drop(client);
    let deadline = Instant::now() + WAIT;
    let mut client = loop {
        let client = home.connect();
        if !client.live(&id).running {
            break client;
        }
        assert!(Instant::now() < deadline, "the idle agent kept running");
        drop(client);
        std::thread::sleep(Duration::from_millis(500));
    };
    client
        .call(Command::ReplaceSession { id: id.clone() })
        .unwrap();
    let live = client.wait_live(&id, |live| !live.replacement && !live.processing);
    assert_eq!(live.error, None);
    let prompts = home.wait_sent(&id, "session/prompt", 2);
    let text = prompts[1]["params"]["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("remember the parrot"), "{text}");
}

#[test]
fn a_turn_that_fails_while_storage_recovers_lets_the_next_message_send() {
    let mut home = Home::new("recovering");
    let mut client = home.connect();
    let id = client.send(None, "SILENT");
    client.wait_delta(|delta| delta["delta"] == "text" && delta["id"] == id.as_str());
    let transcript = home.transcript(&id);
    let writable = fs::metadata(&transcript).unwrap().permissions();
    let mut read_only = writable.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&transcript, read_only).unwrap();
    client
        .call(Command::MarkRead {
            id: id.clone(),
            through: 0,
        })
        .unwrap();
    client.wait_live(&id, |live| live.storage_failed);
    fs::set_permissions(&transcript, writable).unwrap();
    client
        .call(Command::RetryStorage { id: id.clone() })
        .unwrap();
    client.wait_live(&id, |live| live.recovering_storage);
    home.kill_agent();
    let live = client.wait_live(&id, |live| !live.processing);
    assert!(!live.recovering_storage);
    client.send(Some(&id), "are you back?");
}

#[test]
fn a_force_stopped_turn_leaves_the_conversation_idle() {
    let mut home = Home::new("force-stop");
    let mut client = home.connect();
    let id = client.send(None, "SILENT");
    client.wait_live(&id, |live| live.processing);
    client.call(Command::ForceStop { id: id.clone() }).unwrap();
    let live = client.wait_live(&id, |live| !live.running);
    assert!(!live.processing && !live.shutting_down);
    assert_eq!(home.connect().thread_status(&id), "idle");
}

#[test]
fn stop_all_mid_turn_marks_the_turn_interrupted_and_retry_continues_it() {
    let mut home = Home::new("stop-all");
    let mut client = home.connect();
    let id = client.send(None, "SLOW 30");
    client.wait_delta(|delta| delta["delta"] == "text" && delta["id"] == id.as_str());
    client.call(Command::StopAll).unwrap();
    let live = client.live(&id);
    assert!(!live.running && !live.processing && !live.shutting_down);
    let error = live.error.unwrap_or_default();
    assert!(error.contains("interrupted"), "{error:?}");
    assert_eq!(home.connect().thread_status(&id), "blocked");
    client.call(Command::Retry { id: id.clone() }).unwrap();
    assert_eq!(client.wait_turn(&id).error, None);
}

#[test]
fn a_send_now_on_an_idle_conversation_waits_and_a_later_stop_does_not_send_it() {
    let mut home = Home::new("send-now-idle");
    let mut client = home.connect();
    let id = client.send(None, "SLOW 30");
    client.wait_live(&id, |live| live.processing);
    client.send(Some(&id), "queued");
    client.wait_live(&id, |live| live.queued.len() == 1);
    client.call(Command::Stop { id: id.clone() }).unwrap();
    let queued = client.wait_live(&id, |live| !live.processing).queued[0].id;
    client
        .call(Command::SendQueuedNow {
            id: id.clone(),
            queued,
        })
        .unwrap();
    client.send(Some(&id), "SLOW 30");
    client.wait_live(&id, |live| live.processing);
    client.call(Command::Stop { id: id.clone() }).unwrap();
    let live = client.wait_live(&id, |live| !live.processing);
    assert_eq!(live.queued.len(), 1, "the stop sent the queue");
}

#[test]
fn a_retry_notice_clears_once_the_retried_attempt_streams() {
    let mut home = Home::new("retry-notice");
    let mut client = home.connect();
    let id = client.send(None, "FAIL 1 SLOW 2");
    client.wait_live(&id, |live| {
        live.progress
            .as_deref()
            .is_some_and(|progress| progress.starts_with("Retry 1 of"))
    });
    let status = client.call(Command::Status).unwrap();
    assert_eq!(status["conversations"][0]["state"], "retrying");
    let live = client.wait_live(&id, |live| live.progress.is_none() || !live.processing);
    assert!(live.processing, "the notice stayed for the whole turn");
}

impl Client {
    fn wait_delta(&mut self, test: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(delta) = self.deltas.iter().find(|delta| test(delta)) {
                return delta.clone();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let message = self
                .messages
                .recv_timeout(left)
                .expect("the delta never came");
            self.handle(&message);
        }
    }

    fn thread_status(&self, conversation: &str) -> Value {
        self.snapshot.as_ref().unwrap()["projects"][0]["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|thread| thread["id"] == conversation)
            .map(|thread| thread["status"].clone())
            .unwrap()
    }

    fn cards(&self, conversation: &str) -> Vec<Value> {
        self.deltas
            .iter()
            .filter(|delta| delta["delta"] == "card" && delta["id"] == conversation)
            .map(|delta| delta["card"].clone())
            .collect()
    }
}

#[test]
fn docs_an_agent_writes_get_one_card_each_that_follows_the_file() {
    let mut home = Home::new("doc-cards");
    let work = home.root.join("work");
    fs::write(work.join(".gitignore"), "target/\n").unwrap();
    let mut client = home.connect();
    client
        .call(Command::WatchDocs {
            project_id: Some(PROJECT.to_owned()),
        })
        .unwrap();
    let id = client.send(
        None,
        "WRITE notes/plan.md WRITE main.rs WRITE ../outside.md WRITE target/build.md",
    );
    client.wait_turn(&id);
    client.settle();
    let cards = client.cards(&id);
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert_eq!(cards[0]["entity"], "adeline://docs/notes/plan.md");
    assert_eq!(cards[0]["title"], "plan.md");
    assert_eq!(cards[0]["turn"], 0);
    assert!(cards[0]["edited"].as_u64().unwrap() > 0);

    client.send(Some(&id), "WRITE notes/plan.md WRITE a.md");
    client.wait_turn(&id);
    client.settle();
    let fresh = home.connect();
    let thread = fresh.snapshot.as_ref().unwrap()["projects"][0]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|thread| thread["id"] == id.as_str())
        .cloned()
        .unwrap();
    let saved: Vec<(String, Value)> = thread["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| {
            (
                card["subtitle"].as_str().unwrap().to_owned(),
                card["turn"].clone(),
            )
        })
        .collect();
    assert_eq!(
        saved,
        [
            ("notes/plan.md".to_owned(), json!(0)),
            ("a.md".to_owned(), json!(2))
        ]
    );

    fs::remove_file(work.join("notes/plan.md")).unwrap();
    client.wait_delta(|delta| {
        delta["delta"] == "card"
            && delta["card"]["subtitle"] == "notes/plan.md"
            && delta["card"]["gone"] == true
    });

    let before = client.cards(&id).len();
    client
        .call(Command::SetSettings {
            settings: EngineSettings {
                retry_limit: 5,
                docs: adeline_core::protocol::DocsSettings {
                    enabled: false,
                    respect_gitignore: true,
                },
                ..EngineSettings::default()
            },
        })
        .unwrap();
    client.send(Some(&id), "WRITE c.md");
    client.wait_turn(&id);
    client.settle();
    assert_eq!(client.cards(&id).len(), before);
}

#[test]
fn watching_clients_see_disk_changes_and_doc_commands_never_overwrite() {
    let mut home = Home::new("doc-watch");
    let work = home.root.join("work");
    fs::create_dir(work.join("notes")).unwrap();
    fs::write(work.join("a.md"), "mine\n").unwrap();
    fs::write(work.join("notes/plan.md"), "plan\n").unwrap();
    let mut client = home.connect();
    let project = || PROJECT.to_owned();
    client
        .call(Command::WatchDocs {
            project_id: Some(project()),
        })
        .unwrap();
    let listed = client.call(Command::ListDocs {
        project_id: project(),
    });
    assert_eq!(listed, Ok(json!(["a.md", "notes/plan.md"])));

    let error = client
        .call(Command::WriteDoc {
            project_id: project(),
            path: "a.md".into(),
            text: "replaced".into(),
            create: true,
        })
        .unwrap_err();
    assert!(error.contains("already exists"), "{error}");
    assert_eq!(fs::read_to_string(work.join("a.md")).unwrap(), "mine\n");
    let error = client
        .call(Command::RenameDoc {
            project_id: project(),
            from: "a.md".into(),
            to: "notes/plan.md".into(),
        })
        .unwrap_err();
    assert!(error.contains("already exists"), "{error}");
    client
        .call(Command::RenameDoc {
            project_id: project(),
            from: "a.md".into(),
            to: "notes/b.md".into(),
        })
        .unwrap();
    assert!(work.join("notes/b.md").is_file() && !work.join("a.md").exists());
    let error = client
        .call(Command::ReadDoc {
            project_id: project(),
            path: "a.md".into(),
        })
        .unwrap_err();
    assert!(error.contains("no longer exists"), "{error}");

    fs::write(work.join("outside-change.md"), "new").unwrap();
    let change = client.wait_delta(|delta| {
        delta["delta"] == "docs"
            && delta["changes"]
                .as_array()
                .is_some_and(|changes| changes.iter().any(|c| c["path"] == "outside-change.md"))
    });
    assert_eq!(change["project_id"], PROJECT);
}
