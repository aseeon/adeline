use super::*;

use crate::files::TempDir;

fn working(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir(&path).unwrap();
    path
}
fn open_store(root: &Path) -> ProjectStore {
    ProjectStore::with_root(root.join("projects"))
}

fn agent() -> AgentDefinition {
    AgentDefinition {
        name: "Josh".into(),
        harness: crate::harness::OMP.into(),
        model: "openai-codex/gpt-6-sol".into(),
        effort: "high".into(),
        system_instructions: "Keep edits small".into(),
        ..Default::default()
    }
}

fn launch() -> (String, Vec<String>) {
    ("C:/tools/omp.exe".into(), vec!["acp".into()])
}

#[test]
fn conversations_snapshot_resolved_launch_and_switch_only_model_and_effort() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let project = store.save_project(None, "Project", &work).unwrap();
    let id = store
        .create_conversation(&project, &agent(), launch(), "Hello")
        .unwrap();
    let restored = open_store(&test);
    let settings = restored.conversation(&id).unwrap().settings.clone();
    assert!(!settings.execution.legacy());
    assert_eq!(settings.execution.command, "C:/tools/omp.exe");
    assert_eq!(settings.execution.arguments, ["acp"]);
    assert_eq!(settings.execution.harness, "omp");
    let mut switched = settings;
    switched.execution.model = "xai/grok".into();
    switched.execution.effort = "low".into();
    switched.config_options = vec![serde_json::json!({"id":"model"})];
    store.update_conversation(&project, &id, switched).unwrap();
    let saved = open_store(&test)
        .conversation(&id)
        .unwrap()
        .settings
        .clone();
    assert_eq!(saved.execution.model, "xai/grok");
    assert_eq!(saved.config_options.len(), 1);
    let mut moved = saved;
    moved.execution.command = "elsewhere.exe".into();
    assert!(store.update_conversation(&project, &id, moved).is_err());
}

#[test]
fn old_snapshots_stay_readable_but_are_marked_legacy() {
    let restored: ExecutionConfig = serde_yaml_ng::from_str(
        "name: Josh\ncommand: omp.exe\narguments: [acp]\nmodel: openai-codex/gpt-6-sol\neffort: High\neffort_parameter_name: thinking\nsystem_instructions: ''\ndirectory: C:/work\n",
    )
    .unwrap();
    assert!(restored.legacy());
    assert_eq!(restored.harness, "OMP");
}

#[test]
fn project_rename_preserves_history_and_directory_snapshot() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let next = working(&test, "next");
    let mut store = open_store(&test);
    assert!(
        store
            .save_project(None, "Bad", Path::new("relative"))
            .is_err()
    );
    assert!(
        store
            .save_project(None, "Bad", &test.join("missing"))
            .is_err()
    );
    let id = store
        .save_project(None, "Example--Project!", &work)
        .unwrap();
    assert_eq!(id, "example-project");
    assert!(store.save_project(None, "Example Project", &work).is_err());
    let project_file = test.join("projects/example-project/project.yml");
    let yaml = fs::read_to_string(&project_file).unwrap();
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
    assert_eq!(document.as_mapping().unwrap().len(), 2);
    assert_eq!(document["name"], "Example--Project!");
    assert_eq!(document["directory"], work.to_string_lossy().as_ref());

    let conversation = store
        .create_conversation(&id, &agent(), launch(), "Hello")
        .unwrap();
    for status in ["active", "idle", "processing", "blocked"] {
        let mut settings = store.conversation(&conversation).unwrap().settings.clone();
        settings.status = status.into();
        store
            .update_conversation(&id, &conversation, settings)
            .unwrap();
        assert!(
            store
                .save_project(Some(&id), "Changed Project", &next)
                .is_err()
        );
    }
    let snapshot = store
        .conversation(&conversation)
        .unwrap()
        .settings
        .execution
        .clone();
    assert_eq!(snapshot.directory, work);
    assert_eq!(snapshot.arguments, vec!["acp"]);
    let mut settings = store.conversation(&conversation).unwrap().settings.clone();
    let mut changed_execution = settings.clone();
    changed_execution.execution.command = "someone-else".into();
    assert!(
        store
            .update_conversation(&id, &conversation, changed_execution)
            .is_err()
    );
    settings.status = "archived".into();
    settings.permission_mode = PermissionMode::AllowEverything;
    settings.session_id = Some("saved-session".into());
    store
        .update_conversation(&id, &conversation, settings)
        .unwrap();
    let renamed = store
        .save_project(Some(&id), "Changed Project", &next)
        .unwrap();
    assert_eq!(renamed, "changed-project");
    assert!(test.join("projects/changed-project/conversations").is_dir());
    assert!(!test.join("projects/example-project").exists());
    assert!(
        store
            .create_conversation(&renamed, &agent(), launch(), "New")
            .unwrap()
            .len()
            > 8
    );
    drop(store);

    let mut reloaded = open_store(&test);
    assert!(reloaded.errors.is_empty(), "{:?}", reloaded.errors);
    assert_eq!(reloaded.projects[0].name, "Changed Project");
    assert_eq!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .settings
            .execution,
        snapshot
    );
    assert_eq!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .settings
            .session_id
            .as_deref(),
        Some("saved-session")
    );
    assert_eq!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .settings
            .permission_mode,
        PermissionMode::AllowEverything
    );
    fs::write(
        test.join("projects/changed-project/foreign.txt"),
        b"leave alone",
    )
    .unwrap();
    assert!(reloaded.delete_project(&renamed).is_err());
    fs::remove_file(test.join("projects/changed-project/foreign.txt")).unwrap();
    reloaded.delete_project(&renamed).unwrap();
    assert!(work.is_dir());
    assert!(next.is_dir());
}

#[test]
fn legacy_opened_time_is_read_and_survives_a_rename() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let id = store.save_project(None, "Opened", &work).unwrap();
    assert_eq!(store.projects[0].opened_at, None);
    // Older versions recorded the time in project.yml; clients copy it once.
    let definition = test.join("projects").join(&id).join("project.yml");
    let text = fs::read_to_string(&definition).unwrap();
    fs::write(
        &definition,
        format!(
            "{text}opened_at: 1700000000
"
        ),
    )
    .unwrap();
    let mut store = open_store(&test);
    assert_eq!(store.projects[0].opened_at, Some(1_700_000_000));
    let renamed = store
        .save_project(Some(&id), "Opened Again", &work)
        .unwrap();
    let reloaded = open_store(&test);
    let project = reloaded.projects.iter().find(|p| p.id == renamed).unwrap();
    assert_eq!(project.opened_at, Some(1_700_000_000));
    assert_eq!(project.to_workspace().config.opened_at, Some(1_700_000_000));
}

#[test]
fn reconcile_takes_in_outside_changes_but_not_busy_conversations() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let project = store.save_project(None, "Outside", &work).unwrap();
    let quiet = store
        .create_conversation(&project, &agent(), launch(), "Quiet")
        .unwrap();
    let busy = store
        .create_conversation(&project, &agent(), launch(), "Busy")
        .unwrap();
    let none = std::collections::HashSet::new();
    assert!(
        store
            .reconcile(open_store(&test), &none, &none)
            .conversations
            .is_empty()
    );
    for id in [&quiet, &busy] {
        let mut writer = open_store(&test);
        writer
            .record_event(
                id,
                &TranscriptEvent::new("message", serde_json::json!({"role":"user","text":"hi"})),
            )
            .unwrap();
    }
    fs::create_dir_all(work.join("other")).unwrap();
    let mut outside = open_store(&test);
    outside
        .save_project(None, "Second", &work.join("other"))
        .unwrap();
    let running: std::collections::HashSet<_> = [busy.clone()].into();
    let changes = store.reconcile(open_store(&test), &running, &none);
    assert_eq!(changes.conversations, std::slice::from_ref(&quiet));
    assert_eq!(changes.projects.len(), 1);
    assert_eq!(changes.projects[0].config.name, "Second");
    assert_eq!(store.conversation(&quiet).unwrap().events.len(), 1);
    assert!(store.conversation(&busy).unwrap().events.is_empty());
}

#[test]
fn transcript_replays_messages_tool_updates_errors_and_raw_traffic() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store
        .create_conversation(&id, &agent(), launch(), "Hello")
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "message",
                serde_json::json!({
                    "role":"user", "text":"hello", "read":true,
                }),
            ),
        )
        .unwrap();
    store
        .record_raw(
            &conversation,
            "incoming",
            &serde_json::json!({"jsonrpc":"2.0", "method":"session/update"}),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "message",
                serde_json::json!({
                    "role":"assistant", "text":"pa", "read":true,
                }),
            ),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "message_update",
                serde_json::json!({"index":1,"text":"partial"}),
            ),
        )
        .unwrap();
    // The completed update omits the kind and paths the first one carried.
    for update in [
        serde_json::json!({
            "id":"tool-1", "title":"Read", "status":"pending", "detail":"",
            "kind":"read", "paths":["/work/a.md"],
        }),
        serde_json::json!({"id":"tool-1", "title":"Read", "status":"completed", "detail":"done"}),
    ] {
        store
            .record_event(&conversation, &TranscriptEvent::new("tool", update))
            .unwrap();
    }
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new("usage", serde_json::json!({"used":38_000,"size":200_000})),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "error",
                serde_json::json!({"message":"provider unavailable"}),
            ),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new("message", serde_json::json!({"role":"user","text":"next"})),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "assistant_chunk",
                serde_json::json!({"turn":2,"text":"new "}),
            ),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "assistant_chunk",
                serde_json::json!({"turn":2,"text":"answer"}),
            ),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "lifecycle",
                serde_json::json!({"event":"turn_started","retry":false}),
            ),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "assistant_chunk",
                serde_json::json!({"turn":2,"text":" resumed"}),
            ),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new("message_read", serde_json::json!({"through":3})),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "lifecycle",
                serde_json::json!({"event":"turn_started","retry":true}),
            ),
        )
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "assistant_chunk",
                serde_json::json!({"turn":3,"text":"retry answer"}),
            ),
        )
        .unwrap();
    let transcript = test
        .join("projects/project/conversations")
        .join(&conversation)
        .join("transcript.jsonl");
    let lines = fs::read_to_string(&transcript).unwrap();
    assert_eq!(lines.lines().count(), 16);
    assert!(lines.lines().all(|line| {
        serde_json::from_str::<TranscriptEvent>(line)
            .unwrap()
            .timestamp
            > 0
    }));
    drop(store);

    let reloaded = open_store(&test);
    assert!(reloaded.errors.is_empty(), "{:?}", reloaded.errors);
    let thread = reloaded.conversation(&conversation).unwrap().to_thread();
    assert_eq!(
        thread
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        [
            "hello",
            "partial",
            "next",
            "new answer resumed",
            "retry answer"
        ]
    );
    assert!(thread.messages[3].read);
    assert!(!thread.messages[4].read);
    assert_eq!(thread.activity.len(), 2);
    assert_eq!(thread.activity[0].kind, "tool:tool-1");
    assert_eq!(thread.activity[0].title, "Read (completed)");
    assert_eq!(thread.activity[0].detail, "done");
    assert_eq!(thread.activity[0].tool, "read");
    assert_eq!(thread.activity[0].paths, ["/work/a.md"]);
    // Recorded during the first turn, before the next user message.
    assert_eq!(thread.activity[0].turn, Some(0));
    assert_eq!(thread.context, Some((38_000, 200_000)));
    assert_eq!(thread.activity[1].title, "provider unavailable");
    fs::remove_dir(&work).unwrap();
    let mut reloaded = open_store(&test);
    assert_eq!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .to_thread()
            .messages[1]
            .text,
        "partial"
    );
    assert!(
        reloaded
            .create_conversation(&id, &agent(), launch(), "No start")
            .is_err()
    );
}

#[test]
fn write_failure_retains_events_and_explicit_retry_appends_once() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store
        .create_conversation(&id, &agent(), launch(), "Hello")
        .unwrap();
    let saved = TranscriptEvent::new("message", serde_json::json!({"role":"user","text":"saved"}));
    store.record_event(&conversation, &saved).unwrap();
    let transcript = test
        .join("projects/project/conversations")
        .join(&conversation)
        .join("transcript.jsonl");
    let backup = transcript.with_extension("backup");
    fs::rename(&transcript, &backup).unwrap();
    fs::create_dir(&transcript).unwrap();
    assert!(
        store
            .record_event(
                &conversation,
                &TranscriptEvent::new(
                    "message",
                    serde_json::json!({"role":"assistant","text":"unsaved"})
                )
            )
            .is_err()
    );
    assert!(
        store
            .record_raw(&conversation, "incoming", &serde_json::json!({"ok":true}))
            .is_err()
    );
    assert_eq!(
        store
            .conversation(&conversation)
            .unwrap()
            .unsaved_events
            .len(),
        2
    );
    assert!(
        store
            .conversation(&conversation)
            .unwrap()
            .storage_error
            .is_some()
    );
    assert_eq!(fs::read_to_string(&backup).unwrap().lines().count(), 1);
    fs::remove_dir(&transcript).unwrap();
    fs::rename(&backup, &transcript).unwrap();
    store.retry_unsaved(&conversation).unwrap();
    assert!(
        store
            .conversation(&conversation)
            .unwrap()
            .unsaved_events
            .is_empty()
    );
    assert!(
        store
            .conversation(&conversation)
            .unwrap()
            .storage_error
            .is_none()
    );
    let lines = fs::read_to_string(&transcript).unwrap();
    assert_eq!(lines.lines().count(), 3);
    assert_eq!(
        serde_json::from_str::<TranscriptEvent>(lines.lines().next().unwrap()).unwrap(),
        saved
    );
    assert_eq!(
        open_store(&test)
            .conversation(&conversation)
            .unwrap()
            .to_thread()
            .messages[1]
            .text,
        "unsaved"
    );
}

#[test]
fn failed_settings_write_preserves_pending_state_until_retry() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store
        .create_conversation(&id, &agent(), launch(), "Hello")
        .unwrap();
    let settings_path = test
        .join("projects/project/conversations")
        .join(&conversation)
        .join("conversation.yml");
    let backup = settings_path.with_extension("backup");
    fs::rename(&settings_path, &backup).unwrap();
    fs::create_dir(&settings_path).unwrap();
    let mut settings = store.conversation(&conversation).unwrap().settings.clone();
    settings.session_id = Some("new-session".into());
    assert!(
        store
            .update_conversation(&id, &conversation, settings)
            .is_err()
    );
    assert!(
        store
            .conversation(&conversation)
            .unwrap()
            .storage_error
            .is_some()
    );
    assert_eq!(
        store
            .conversation(&conversation)
            .unwrap()
            .settings
            .session_id
            .as_deref(),
        Some("new-session")
    );
    fs::remove_dir(&settings_path).unwrap();
    fs::rename(&backup, &settings_path).unwrap();
    store.retry_unsaved(&conversation).unwrap();
    assert!(
        store
            .conversation(&conversation)
            .unwrap()
            .storage_error
            .is_none()
    );
    assert_eq!(
        open_store(&test)
            .conversation(&conversation)
            .unwrap()
            .settings
            .session_id
            .as_deref(),
        Some("new-session")
    );
}

#[test]
fn bad_transcript_keeps_prior_history_and_exposes_error() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store
        .create_conversation(&id, &agent(), launch(), "Hello")
        .unwrap();
    store
        .record_event(
            &conversation,
            &TranscriptEvent::new(
                "message",
                serde_json::json!({"role":"user","text":"still readable"}),
            ),
        )
        .unwrap();
    let mut settings = store.conversation(&conversation).unwrap().settings.clone();
    settings.status = "processing".into();
    store
        .update_conversation(&id, &conversation, settings)
        .unwrap();
    let path = test
        .join("projects/project/conversations")
        .join(&conversation)
        .join("transcript.jsonl");
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"broken json\n")
        .unwrap();
    drop(store);

    let mut reloaded = open_store(&test);
    assert!(
        reloaded
            .errors
            .iter()
            .any(|error| error.contains("transcript.jsonl:2"))
    );
    assert_eq!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .settings
            .status,
        "blocked"
    );
    assert!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .to_thread()
            .activity
            .iter()
            .any(|activity| activity.title.contains("interrupted"))
    );
    assert_eq!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .to_thread()
            .messages[0]
            .text,
        "still readable"
    );
    assert!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .storage_error
            .is_some()
    );
    assert!(
        reloaded
            .record_event(
                &conversation,
                &TranscriptEvent::new(
                    "message",
                    serde_json::json!({
                        "role":"assistant","text":"waiting",
                    })
                )
            )
            .is_err()
    );
    assert!(reloaded.retry_unsaved(&conversation).is_err());
    assert_eq!(
        reloaded
            .conversation(&conversation)
            .unwrap()
            .unsaved_events
            .len(),
        1
    );
}

#[test]
fn unsafe_config_contents_block_delete_without_touching_workdir() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let sentinel = work.join("never-delete.txt");
    fs::write(&sentinel, b"important").unwrap();
    let mut store = open_store(&test);
    let id = store.save_project(None, "Project", &work).unwrap();
    let project_folder = test.join("projects/project");
    assert!(
        store
            .save_project(Some(&id), "Project", &project_folder)
            .is_err()
    );
    let config_file = project_folder.join("project.yml");
    let original_yaml = fs::read_to_string(&config_file).unwrap();
    let malicious_yaml = serde_yaml_ng::to_string(&ProjectDefinition {
        name: "Project".into(),
        directory: project_folder.clone(),
        opened_at: None,
    })
    .unwrap();
    fs::write(&config_file, malicious_yaml).unwrap();
    let mut loaded = open_store(&test);
    assert!(
        loaded
            .errors
            .iter()
            .any(|error| error.contains("inside Adeline"))
    );
    assert!(loaded.delete_project(&id).is_err());
    assert!(store.delete_project(&id).is_err());
    fs::write(&config_file, original_yaml).unwrap();
    let folder = test.join("projects/project/conversations");
    fs::write(folder.join("extra-file"), b"keep").unwrap();
    assert!(store.delete_project(&id).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"important");
    fs::remove_file(folder.join("extra-file")).unwrap();
    let link = folder.join("linked-folder");
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&work, &link);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&work, &link);
    if linked.is_ok() {
        assert!(store.delete_project(&id).is_err());
        assert_eq!(fs::read(&sentinel).unwrap(), b"important");
        #[cfg(windows)]
        fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
    }
    store.delete_project(&id).unwrap();
    assert_eq!(fs::read(&sentinel).unwrap(), b"important");
}

#[test]
fn forks_copy_visible_history_through_the_fork_point_only() {
    let test = TempDir::new("adeline-storage");
    let work = working(&test, "work");
    let mut store = open_store(&test);
    let project = store.save_project(None, "Project", &work).unwrap();
    let source = store
        .create_conversation(&project, &agent(), launch(), "Plan")
        .unwrap();
    let event = |kind: &str, data: Value| TranscriptEvent::new(kind, data);
    for event in [
        event("message", serde_json::json!({"role":"user","text":"one"})),
        event("raw", serde_json::json!({"direction":"outgoing"})),
        event("assistant_chunk", serde_json::json!({"text":"first "})),
        event(
            "tool",
            serde_json::json!({"id":"t1","title":"Read","status":"completed"}),
        ),
        event("error", serde_json::json!({"message":"overloaded"})),
        event(
            "lifecycle",
            serde_json::json!({"event":"turn_started","retry":true}),
        ),
        event("assistant_chunk", serde_json::json!({"text":"retried"})),
        event(
            "permission_decision",
            serde_json::json!({"option_id":"allow"}),
        ),
        event("lifecycle", serde_json::json!({"event":"turn_finished"})),
        event("message", serde_json::json!({"role":"user","text":"two"})),
        event("assistant_chunk", serde_json::json!({"text":"later"})),
    ] {
        store.record_event(&source, &event).unwrap();
    }
    let mut settings = store.conversation(&source).unwrap().settings.clone();
    settings.permission_mode = PermissionMode::AllowEverything;
    settings.execution.model = "xai/grok".into();
    settings.session_id = Some("source-session".into());
    store
        .update_conversation(&project, &source, settings)
        .unwrap();
    // Message 1 ends its turn's first reply; message 2 is the retried reply.
    assert!(store.fork_conversation(&source, 1).is_err());
    assert!(store.fork_conversation(&source, 3).is_err());
    let fork = store.fork_conversation(&source, 2).unwrap();
    let restored = open_store(&test);
    let saved = restored.conversation(&fork).unwrap();
    assert_eq!(saved.settings.title, "Plan (fork)");
    assert_eq!(saved.settings.execution.model, "xai/grok");
    assert_eq!(
        saved.settings.permission_mode,
        PermissionMode::AllowEverything
    );
    assert_eq!(saved.settings.session_id, None);
    assert_eq!(
        saved.settings.forked_from.as_ref().unwrap().conversation_id,
        source
    );
    assert!(saved.events.iter().all(|event| !matches!(
        event.kind.as_str(),
        "raw" | "lifecycle" | "permission_decision" | "session"
    )));
    let thread = saved.to_thread();
    let texts: Vec<_> = thread.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["one", "first ", "retried"]);
    assert!(thread.messages.iter().all(|m| m.read));
    assert_eq!(thread.activity.len(), 2);
    assert_eq!(thread.fork.as_ref().unwrap().title, "Plan");
    assert!(!thread.fork.as_ref().unwrap().text_copy);
    // Forking a fork points at the first fork; the source is untouched.
    let again = store.fork_conversation(&fork, 2).unwrap();
    let again = store.conversation(&again).unwrap().to_thread();
    assert_eq!(again.title, "Plan (fork) (fork)");
    assert_eq!(again.fork.unwrap().id, fork);
    assert_eq!(
        store
            .conversation(&source)
            .unwrap()
            .to_thread()
            .messages
            .len(),
        5
    );
}
