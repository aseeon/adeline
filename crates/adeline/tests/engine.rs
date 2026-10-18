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

/// A temporary home with one project and the fake agent, and the engine running in it.
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

    /// A new client; the first one starts the engine.
    fn connect(&mut self) -> Client {
        self.connect_from(None)
    }

    fn connect_from(&mut self, resume: Option<Resume>) -> Client {
        let mut bridge = Process::new(env!("CARGO_BIN_EXE_adeline"))
            .arg("bridge")
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root)
            .env("ADELINE_FAKE_STATE", self.root.join("fake-state"))
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
        #[cfg(windows)]
        let killed = Process::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
        #[cfg(not(windows))]
        let killed = Process::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output();
        killed.unwrap();
        // Windows ends a killed process after taskkill returns, and until then
        // its pipe still accepts the next client.
        #[cfg(windows)]
        {
            let deadline = Instant::now() + WAIT;
            while alive(pid) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        // The engine's pipe or socket closes with it.
        std::thread::sleep(Duration::from_millis(500));
    }

    /// What the engine sent the agent with this method, from the saved traffic.
    fn sent(&self, conversation: &str, method: &str) -> Vec<Value> {
        let path = self
            .root
            .join(".config/adeline/projects")
            .join(PROJECT)
            .join("conversations")
            .join(conversation)
            .join("transcript.jsonl");
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| event["kind"] == "traffic" && event["data"]["direction"] == "to_agent")
            .filter_map(|event| serde_json::from_str::<Value>(event["data"]["text"].as_str()?).ok())
            .filter(|message| message["method"] == method)
            .collect()
    }

    /// Waits for the batched transcript to hold `count` requests of this method.
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

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let output = Process::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!(" {pid} "))
}

/// One connection to the engine, seeing what a UI sees.
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

    /// Waits until the conversation's live state passes `test`, and returns it.
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

    /// Waits for a turn to start and end, and returns the final live state.
    fn wait_turn(&mut self, conversation: &str) -> Live {
        self.wait_live(conversation, |live| live.processing);
        self.wait_live(conversation, |live| !live.processing)
    }

    /// Where this client is in the engine's delta stream.
    fn position(&self) -> Resume {
        let snapshot = self.snapshot.as_ref().unwrap();
        Resume {
            epoch: snapshot["epoch"].as_str().unwrap().to_owned(),
            seq: snapshot["seq"].as_u64().unwrap() + self.deltas.len() as u64,
        }
    }

    /// Reads whatever arrives in the next moment.
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
    back.wait_live(&id, |live| !live.processing);
    watcher.settle();
    back.settle();
    let since = usize::try_from(left_at.seq).unwrap()
        - usize::try_from(watcher.snapshot.as_ref().unwrap()["seq"].as_u64().unwrap()).unwrap();
    // Either may hold a trailing status update the other hasn't read yet.
    let shared = back.deltas.len().min(watcher.deltas.len() - since);
    let text = back
        .deltas
        .iter()
        .position(|delta| delta["delta"] == "text");
    let turn_end = back.deltas.iter().rposition(|delta| {
        delta["delta"] == "live"
            && delta["id"] == id.as_str()
            && delta["live"]["processing"] == false
    });
    assert!(
        text < turn_end && turn_end < Some(shared),
        "{text:?} {turn_end:?} {shared}"
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
fn text_for_another_session_never_reaches_the_conversation() {
    let mut home = Home::new("stray");
    let mut client = home.connect();
    let id = client.send(None, "STRAY");
    assert_eq!(client.wait_turn(&id).error, None);
    let texts: String = client
        .deltas
        .iter()
        .filter(|delta| delta["delta"] == "text" && delta["id"] == id.as_str())
        .filter_map(|delta| delta["text"].as_str())
        .collect();
    assert_eq!(texts, "Hello from fake agent");
}

impl Client {
    /// Waits for a delta that passes `test`, reading on from what arrived so far.
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

    fn cards(&self, conversation: &str) -> Vec<Value> {
        self.deltas
            .iter()
            .filter(|delta| delta["delta"] == "card" && delta["id"] == conversation)
            .map(|delta| delta["card"].clone())
            .collect()
    }

    fn docs(&mut self, command: Command) -> Result<Value, String> {
        self.call(command)
    }
}

#[test]
fn docs_an_agent_writes_get_one_card_each_and_watching_clients_see_disk_changes() {
    let mut home = Home::new("docs");
    let work = home.root.join("work");
    fs::write(work.join(".gitignore"), "target/\n").unwrap();
    let mut client = home.connect();
    let project = || PROJECT.to_owned();
    client
        .docs(Command::WatchDocs {
            project_id: Some(project()),
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

    // A later turn updates the card in place; a new doc gets its own.
    client.send(Some(&id), "WRITE notes/plan.md WRITE a.md");
    client.wait_turn(&id);
    client.settle();
    let mut fresh = home.connect();
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

    let listed = fresh.docs(Command::ListDocs {
        project_id: project(),
    });
    assert_eq!(listed, Ok(json!(["a.md", "notes/plan.md"])));

    // A save keeps the file's line endings and missing final newline.
    fs::write(work.join("crlf.md"), "one\r\ntwo\r\nthree").unwrap();
    let read = fresh
        .docs(Command::ReadDoc {
            project_id: project(),
            path: "crlf.md".into(),
        })
        .unwrap();
    assert_eq!(read["text"], "one\r\ntwo\r\nthree");
    assert_eq!(read["read_only"], Value::Null);
    fresh
        .docs(Command::WriteDoc {
            project_id: project(),
            path: "crlf.md".into(),
            text: "one\nTWO\nthree\n".into(),
            create: false,
        })
        .unwrap();
    assert_eq!(
        fs::read(work.join("crlf.md")).unwrap(),
        b"one\r\nTWO\r\nthree"
    );

    // Nothing is overwritten, and names that aren't docs are refused.
    let refused = |result: Result<Value, String>| result.unwrap_err();
    let error = refused(fresh.docs(Command::WriteDoc {
        project_id: project(),
        path: "a.md".into(),
        text: "replaced".into(),
        create: true,
    }));
    assert!(error.contains("already exists"), "{error}");
    assert_eq!(
        fs::read_to_string(work.join("a.md")).unwrap(),
        "written by the fake agent\n"
    );
    let error = refused(fresh.docs(Command::RenameDoc {
        project_id: project(),
        from: "a.md".into(),
        to: "notes/plan.md".into(),
    }));
    assert!(error.contains("already exists"), "{error}");
    for bad in ["a.rs", "../a.md", "notes/x?.md"] {
        assert!(
            fresh
                .docs(Command::RenameDoc {
                    project_id: project(),
                    from: "a.md".into(),
                    to: bad.into(),
                })
                .is_err(),
            "{bad}"
        );
    }
    fresh
        .docs(Command::RenameDoc {
            project_id: project(),
            from: "a.md".into(),
            to: "notes/b.md".into(),
        })
        .unwrap();
    assert!(work.join("notes/b.md").is_file() && !work.join("a.md").exists());

    // Changes on disk reach the watching client, and a deleted doc's card turns gone.
    fs::write(work.join("outside-change.md"), "new").unwrap();
    let change = client.wait_delta(|delta| {
        delta["delta"] == "docs"
            && delta["changes"]
                .as_array()
                .is_some_and(|changes| changes.iter().any(|c| c["path"] == "outside-change.md"))
    });
    assert_eq!(change["project_id"], PROJECT);
    fs::remove_file(work.join("notes/plan.md")).unwrap();
    client.wait_delta(|delta| {
        delta["delta"] == "card"
            && delta["card"]["subtitle"] == "notes/plan.md"
            && delta["card"]["gone"] == true
    });
    let error = refused(fresh.docs(Command::ReadDoc {
        project_id: project(),
        path: "notes/plan.md".into(),
    }));
    assert!(error.contains("no longer exists"), "{error}");

    // With Docs disabled, no new cards are made.
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
