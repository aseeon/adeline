//! What this UI remembers for itself and doesn't share with other clients:
//! when it last opened each project, by project ID.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct UiState {
    /// The `opened_at` values from older project files have been copied.
    migrated: bool,
    opened_at: BTreeMap<String, i64>,
}

static STATE: Mutex<Option<UiState>> = Mutex::new(None);

fn path() -> Result<PathBuf, String> {
    Ok(crate::config::directory()?.join("ui-state.yml"))
}

fn with<T>(change: impl FnOnce(&mut UiState) -> T) -> T {
    let mut guard = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let state = guard.get_or_insert_with(|| {
        path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_yaml_ng::from_str(&text).ok())
            .unwrap_or_default()
    });
    change(state)
}

fn save(state: &UiState) -> Result<(), String> {
    crate::config::write_yaml(&path()?, state)
}

/// Gives projects this UI's recency. The first time, it adopts the times the
/// engine still reports from older project files.
pub fn apply(projects: &mut [crate::data::Workspace]) {
    with(|state| {
        if !state.migrated {
            for project in projects.iter() {
                if let Some(at) = project.config.opened_at {
                    state.opened_at.insert(project.config.id.clone(), at);
                }
            }
            state.migrated = true;
            let _ = save(state);
        }
        for project in projects {
            project.config.opened_at = state.opened_at.get(&project.config.id).copied();
        }
    });
}

pub fn opened_at(id: &str) -> Option<i64> {
    with(|state| state.opened_at.get(id).copied())
}

pub fn record_opened(id: &str, at: i64) -> Result<(), String> {
    with(|state| {
        state.opened_at.insert(id.to_owned(), at);
        save(state)
    })
}

/// Keeps a renamed project's recency.
pub fn rename(old: &str, new: &str) {
    with(|state| {
        if let Some(at) = state.opened_at.remove(old) {
            state.opened_at.insert(new.to_owned(), at);
            let _ = save(state);
        }
    });
}
