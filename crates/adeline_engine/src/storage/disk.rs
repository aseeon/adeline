//! A conversation's files: writing its snapshot, flushing settings and queued
//! events, and loading it back.
use super::*;

pub(super) fn write_conversation(
    base: &Path,
    settings: ConversationSettings,
    events: Vec<TranscriptEvent>,
) -> Result<StoredConversation> {
    let id = loop {
        let id = files::unique(&format!("{:x}", adeline_core::recency::now_ms()));
        match fs::create_dir(base.join(&id)) {
            Ok(()) => break id,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).with_context(|| base.display().to_string()),
        }
    };
    let folder = base.join(&id);
    let mut transcript = Vec::new();
    for event in &events {
        serde_json::to_writer(&mut transcript, event)?;
        transcript.push(b'\n');
    }
    let result = serde_yaml_ng::to_string(&settings)
        .map_err(anyhow::Error::from)
        .and_then(|text| files::write_new(&folder.join("conversation.yml"), text.as_bytes()))
        .and_then(|()| files::write_new(&folder.join("transcript.jsonl"), &transcript));
    if let Err(error) = result {
        let _ = fs::remove_file(folder.join("conversation.yml"));
        let _ = fs::remove_file(folder.join("transcript.jsonl"));
        let _ = fs::remove_dir(&folder);
        return Err(error);
    }
    Ok(StoredConversation {
        id,
        settings,
        events,
        unsaved_events: Vec::new(),
        storage_error: None,
        unsaved_since: None,
        persisted_len: transcript.len() as u64,
        needs_rollback: false,
        pending_settings: None,
        load_error: false,
        interrupted: false,
    })
}

pub(super) fn flush_settings(conversation: &mut StoredConversation, path: &Path) -> Result<()> {
    let Some(settings) = &conversation.pending_settings else {
        return Ok(());
    };
    let result = (|| {
        safe_conversation_path(path)?;
        ensure_regular_file(path)?;
        let text =
            serde_yaml_ng::to_string(settings).with_context(|| path.display().to_string())?;
        files::replace(path, text.as_bytes())
    })();
    match result {
        Ok(()) => {
            conversation.pending_settings = None;
            Ok(())
        }
        Err(error) => {
            conversation.storage_error = Some(format!("{error:#}"));
            Err(error)
        }
    }
}

pub(super) fn flush_events(conversation: &mut StoredConversation, path: &Path) -> Result<()> {
    let result = (|| {
        safe_conversation_path(path)?;
        ensure_regular_file(path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| path.display().to_string())?;
        let len = file
            .metadata()
            .with_context(|| path.display().to_string())?
            .len();
        if len < conversation.persisted_len {
            bail!(
                "{}: saved transcript was truncated; reload before writing.",
                path.display()
            );
        }
        if conversation.needs_rollback {
            file.set_len(conversation.persisted_len)
                .with_context(|| path.display().to_string())?;
            file.sync_all()
                .with_context(|| path.display().to_string())?;
            conversation.needs_rollback = false;
        } else if len != conversation.persisted_len {
            bail!(
                "{}: transcript changed on disk; reload before writing.",
                path.display()
            );
        }
        file.seek(SeekFrom::Start(conversation.persisted_len))
            .with_context(|| path.display().to_string())?;
        let write_result = (|| {
            for event in &conversation.unsaved_events {
                serde_json::to_writer(&mut file, event)
                    .with_context(|| path.display().to_string())?;
                file.write_all(b"\n")
                    .with_context(|| path.display().to_string())?;
            }
            file.sync_all()
                .with_context(|| path.display().to_string())?;
            file.stream_position()
                .with_context(|| path.display().to_string())
        })();
        match write_result {
            Ok(len) => conversation.persisted_len = len,
            Err(error) => {
                conversation.needs_rollback = true;
                return Err(error);
            }
        }
        conversation.events.append(&mut conversation.unsaved_events);
        conversation.storage_error = None;
        Ok(())
    })();
    if let Err(error) = &result {
        conversation.storage_error = Some(format!("{error:#}"));
    }
    result
}

pub(super) fn load_conversation(folder: &Path) -> Result<Option<StoredConversation>> {
    let metadata = fs::symlink_metadata(folder).with_context(|| folder.display().to_string())?;
    if is_link(&metadata) {
        bail!(
            "{}: linked conversation folders are not loaded.",
            folder.display()
        );
    }
    if !metadata.file_type().is_dir() {
        return Ok(None);
    }
    let id = folder
        .file_name()
        .with_context(|| format!("{}: missing folder name.", folder.display()))?
        .to_string_lossy()
        .into_owned();
    checked_id(&id)?;
    let settings_path = folder.join("conversation.yml");
    ensure_regular_file(&settings_path)?;
    let text =
        fs::read_to_string(&settings_path).with_context(|| settings_path.display().to_string())?;
    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).with_context(|| settings_path.display().to_string())?;
    // An earlier format stays on disk untouched and is not shown.
    if yaml
        .get("version")
        .and_then(serde_yaml_ng::Value::as_u64)
        .is_none_or(|version| version < u64::from(VERSION))
    {
        return Ok(None);
    }
    let mut settings: ConversationSettings =
        serde_yaml_ng::from_value(yaml).with_context(|| settings_path.display().to_string())?;
    let interrupted = settings.status == ThreadStatus::Processing;
    if interrupted {
        settings.status = ThreadStatus::Blocked;
    }
    if !settings.execution.directory.is_absolute() {
        bail!(
            "{}: saved working directory is not absolute.",
            settings_path.display()
        );
    }
    let transcript_path = folder.join("transcript.jsonl");
    ensure_regular_file(&transcript_path)?;
    let file =
        File::open(&transcript_path).with_context(|| transcript_path.display().to_string())?;
    let persisted_len = file
        .metadata()
        .with_context(|| transcript_path.display().to_string())?
        .len();
    let mut events = Vec::new();
    let mut load_error = None;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        match line
            .map_err(|e| file_error(&transcript_path, e))
            .and_then(|line| {
                serde_json::from_str(&line)
                    .map_err(|e| format!("{}:{}: {e}", transcript_path.display(), index + 1))
            }) {
            Ok(event) => events.push(event),
            Err(error) => {
                load_error = Some(error);
                break;
            }
        }
    }
    Ok(Some(StoredConversation {
        id,
        settings,
        events,
        unsaved_events: Vec::new(),
        load_error: load_error.is_some(),
        storage_error: load_error,
        unsaved_since: None,
        persisted_len,
        needs_rollback: false,
        pending_settings: None,
        interrupted,
    }))
}
