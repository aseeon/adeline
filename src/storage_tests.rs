use super::*;

struct TestRoot(PathBuf);
impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "adeline-storage-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn working(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir(&path).unwrap();
        path
    }
    fn store(&self) -> ProjectStore {
        ProjectStore::with_root(self.0.join("projects"))
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn agent() -> AgentDefinition {
    AgentDefinition {
        name: "Josh".into(),
        harness: "OMP".into(),
        driver: "ACP".into(),
        command: "omp.exe".into(),
        arguments: vec!["acp".into()],
        model: "openai-codex/gpt-6-sol".into(),
        effort: "High".into(),
        system_instructions: "Keep edits small".into(),
        ..Default::default()
    }
}

#[test]
fn conversations_snapshot_effort_parameter_and_restore_legacy_defaults() {
    let test = TestRoot::new();
    let work = test.working("work");
    let mut store = test.store();
    let project = store.save_project(None, "Project", &work).unwrap();
    for parameter in agents::EffortParameterName::ALL {
        let mut definition = agent();
        definition.effort_parameter_name = parameter;
        let id = store
            .create_conversation(&project, &definition, "Hello")
            .unwrap();
        let restored = test.store();
        let execution = &restored.conversation(&id).unwrap().settings.execution;
        assert_eq!(execution.effort_parameter_name, parameter);
        let mut legacy = serde_yaml_ng::to_value(execution).unwrap();
        legacy
            .as_mapping_mut()
            .unwrap()
            .remove("effort_parameter_name");
        let restored: ExecutionConfig = serde_yaml_ng::from_value(legacy).unwrap();
        assert_eq!(
            restored.effort_parameter_name,
            agents::EffortParameterName::Thinking
        );
    }
}

#[test]
fn non_omp_conversation_restores_its_harness_and_legacy_snapshots_use_omp() {
    let test = TestRoot::new();
    let work = test.working("work");
    let mut store = test.store();
    let project = store.save_project(None, "Project", &work).unwrap();
    let mut definition = agent();
    definition.harness = "Other".into();
    definition.command = "other-agent.exe".into();
    let id = store
        .create_conversation(&project, &definition, "Hello")
        .unwrap();
    assert_eq!(
        store.conversation(&id).unwrap().settings.execution.harness,
        "Other"
    );
    drop(store);
    assert_eq!(
        test.store()
            .conversation(&id)
            .unwrap()
            .settings
            .execution
            .harness,
        "Other"
    );

    let mut snapshot =
        serde_yaml_ng::to_value(&test.store().conversation(&id).unwrap().settings.execution)
            .unwrap();
    snapshot.as_mapping_mut().unwrap().remove("harness");
    let restored: ExecutionConfig = serde_yaml_ng::from_value(snapshot).unwrap();
    assert_eq!(restored.harness, "OMP");
}

#[test]
fn project_rename_preserves_history_and_directory_snapshot() {
    let test = TestRoot::new();
    let work = test.working("work");
    let next = test.working("next");
    let mut store = test.store();
    assert!(
        store
            .save_project(None, "Bad", Path::new("relative"))
            .is_err()
    );
    assert!(
        store
            .save_project(None, "Bad", &test.0.join("missing"))
            .is_err()
    );
    let id = store
        .save_project(None, "Example--Project!", &work)
        .unwrap();
    assert_eq!(id, "example-project");
    assert!(store.save_project(None, "Example Project", &work).is_err());
    let project_file = test.0.join("projects/example-project/project.yml");
    let yaml = fs::read_to_string(&project_file).unwrap();
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
    assert_eq!(document.as_mapping().unwrap().len(), 2);
    assert_eq!(document["name"], "Example--Project!");
    assert_eq!(document["directory"], work.to_string_lossy().as_ref());

    let conversation = store.create_conversation(&id, &agent(), "Hello").unwrap();
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
    changed_execution.execution.model = "someone-else".into();
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
    assert!(
        test.0
            .join("projects/changed-project/conversations")
            .is_dir()
    );
    assert!(!test.0.join("projects/example-project").exists());
    assert!(
        store
            .create_conversation(&renamed, &agent(), "New")
            .unwrap()
            .len()
            > 8
    );
    drop(store);

    let mut reloaded = test.store();
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
        test.0.join("projects/changed-project/foreign.txt"),
        b"leave alone",
    )
    .unwrap();
    assert!(reloaded.delete_project(&renamed).is_err());
    fs::remove_file(test.0.join("projects/changed-project/foreign.txt")).unwrap();
    reloaded.delete_project(&renamed).unwrap();
    assert!(work.is_dir());
    assert!(next.is_dir());
}

#[test]
fn opened_time_persists_and_survives_a_rename() {
    let test = TestRoot::new();
    let work = test.working("work");
    let mut store = test.store();
    let id = store.save_project(None, "Opened", &work).unwrap();
    assert_eq!(store.projects[0].opened_at, None);
    assert!(store.mark_opened("missing", 1).is_err());
    store.mark_opened(&id, 1_700_000_000).unwrap();
    assert_eq!(test.store().projects[0].opened_at, Some(1_700_000_000));
    let renamed = store
        .save_project(Some(&id), "Opened Again", &work)
        .unwrap();
    let reloaded = test.store();
    let project = reloaded.projects.iter().find(|p| p.id == renamed).unwrap();
    assert_eq!(project.opened_at, Some(1_700_000_000));
    assert_eq!(project.to_workspace().config.opened_at, Some(1_700_000_000));
}

#[test]
fn transcript_replays_messages_tool_updates_errors_and_raw_traffic() {
    let test = TestRoot::new();
    let work = test.working("work");
    let mut store = test.store();
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store.create_conversation(&id, &agent(), "Hello").unwrap();
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
        .0
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

    let reloaded = test.store();
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
    let mut reloaded = test.store();
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
            .create_conversation(&id, &agent(), "No start")
            .is_err()
    );
}

#[test]
fn write_failure_retains_events_and_explicit_retry_appends_once() {
    let test = TestRoot::new();
    let work = test.working("work");
    let mut store = test.store();
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store.create_conversation(&id, &agent(), "Hello").unwrap();
    let saved = TranscriptEvent::new("message", serde_json::json!({"role":"user","text":"saved"}));
    store.record_event(&conversation, &saved).unwrap();
    let transcript = test
        .0
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
        test.store()
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
    let test = TestRoot::new();
    let work = test.working("work");
    let mut store = test.store();
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store.create_conversation(&id, &agent(), "Hello").unwrap();
    let settings_path = test
        .0
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
        test.store()
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
    let test = TestRoot::new();
    let work = test.working("work");
    let mut store = test.store();
    let id = store.save_project(None, "Project", &work).unwrap();
    let conversation = store.create_conversation(&id, &agent(), "Hello").unwrap();
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
        .0
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

    let mut reloaded = test.store();
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
    let test = TestRoot::new();
    let work = test.working("work");
    let sentinel = work.join("never-delete.txt");
    fs::write(&sentinel, b"important").unwrap();
    let mut store = test.store();
    let id = store.save_project(None, "Project", &work).unwrap();
    let project_folder = test.0.join("projects/project");
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
    let mut loaded = test.store();
    assert!(
        loaded
            .errors
            .iter()
            .any(|error| error.contains("inside Adeline"))
    );
    assert!(loaded.delete_project(&id).is_err());
    assert!(store.delete_project(&id).is_err());
    fs::write(&config_file, original_yaml).unwrap();
    let folder = test.0.join("projects/project/conversations");
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
