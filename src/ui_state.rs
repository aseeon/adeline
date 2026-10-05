//! What this UI remembers for itself and doesn't share with other clients:
//! when it last opened each project, by machine and project ID.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct UiState {
    /// The `opened_at` values from older project files have been copied.
    migrated: bool,
    /// Local projects by ID, remote ones by `machine:ID`.
    opened_at: BTreeMap<String, i64>,
}

static STATE: Mutex<Option<UiState>> = Mutex::new(None);

fn path() -> Result<PathBuf, String> {
    Ok(crate::config::directory()?.join("ui-state.yml"))
}

/// Local projects keep the bare IDs older versions wrote.
fn key(machine: &str, id: &str) -> String {
    if machine == crate::machines::LOCAL {
        id.to_owned()
    } else {
        format!("{machine}:{id}")
    }
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
    let path = path()?;
    crate::config::seed_yaml(&path, state).and_then(|()| crate::config::write_yaml(&path, state))
}

/// Gives a machine's projects this UI's recency. The first time, it adopts
/// the times the local engine still reports from older project files.
pub fn apply(machine: &str, projects: &mut [crate::data::Workspace]) {
    with(|state| {
        if !state.migrated && machine == crate::machines::LOCAL {
            for project in projects.iter() {
                if let Some(at) = project.config.opened_at {
                    state.opened_at.insert(project.config.id.clone(), at);
                }
            }
            state.migrated = true;
            let _ = save(state);
        }
        for project in projects {
            project.config.opened_at = state
                .opened_at
                .get(&key(machine, &project.config.id))
                .copied();
        }
    });
}

pub fn opened_at(machine: &str, id: &str) -> Option<i64> {
    with(|state| state.opened_at.get(&key(machine, id)).copied())
}

pub fn record_opened(machine: &str, id: &str, at: i64) -> Result<(), String> {
    with(|state| {
        state.opened_at.insert(key(machine, id), at);
        save(state)
    })
}

/// Keeps a renamed project's recency.
pub fn rename(machine: &str, old: &str, new: &str) {
    with(|state| {
        if let Some(at) = state.opened_at.remove(&key(machine, old)) {
            state.opened_at.insert(key(machine, new), at);
            let _ = save(state);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_projects_keep_bare_ids_and_remote_ones_add_their_machine() {
        assert_eq!(super::key(crate::machines::LOCAL, "app"), "app");
        assert_eq!(super::key("m-1", "app"), "m-1:app");
    }
}
