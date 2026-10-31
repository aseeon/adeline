//! What this UI remembers for itself and doesn't share with other clients:
//! when it last opened each project and how it last left Docs mode there, by
//! machine and project ID, and the widths the user dragged the right island's
//! panes to. Local projects keep the bare IDs older versions
//! wrote. `migrated` records that the first local snapshot's times, kept by
//! older project files, were adopted.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct UiState {
    migrated: bool,
    opened_at: BTreeMap<String, i64>,
    docs: BTreeMap<String, Docs>,
    panes: Panes,
}

/// Widths of the right island's pane, in design pixels: the side pane in
/// Chats and the chat beside every other mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Panes {
    pub side: Option<f32>,
    pub chat: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Docs {
    pub open: Option<String>,
    pub editor: bool,
    // The Viewer's top block and offset into it, or the Editor's offset.
    pub scroll: (usize, f32),
    pub expanded: Vec<String>,
    pub chat: Option<String>,
}

static STATE: Mutex<Option<UiState>> = Mutex::new(None);

fn path() -> anyhow::Result<PathBuf> {
    Ok(adeline_core::config::directory()?.join("ui-state.yml"))
}

fn key(machine: &str, id: &str) -> String {
    if machine == adeline_core::machines::LOCAL {
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

fn save(state: &UiState) -> anyhow::Result<()> {
    let path = path()?;
    adeline_core::config::seed_yaml(&path, state)
        .and_then(|()| adeline_core::config::write_yaml(&path, state))
}

pub fn apply(machine: &str, projects: &mut [adeline_core::data::Workspace]) {
    with(|state| {
        if !state.migrated && machine == adeline_core::machines::LOCAL {
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

pub fn record_opened(machine: &str, id: &str, at: i64) -> anyhow::Result<()> {
    with(|state| {
        state.opened_at.insert(key(machine, id), at);
        save(state)
    })
}

pub fn rename(machine: &str, old: &str, new: &str) {
    with(|state| {
        let docs = state.docs.remove(&key(machine, old));
        if let Some(docs) = docs {
            state.docs.insert(key(machine, new), docs);
        }
        if let Some(at) = state.opened_at.remove(&key(machine, old)) {
            state.opened_at.insert(key(machine, new), at);
        }
        let _ = save(state);
    });
}

pub fn panes() -> Panes {
    with(|state| state.panes)
}

pub fn record_panes(panes: Panes) -> anyhow::Result<()> {
    with(|state| {
        if state.panes == panes {
            return Ok(());
        }
        state.panes = panes;
        save(state)
    })
}

pub fn docs(machine: &str, id: &str) -> Docs {
    with(|state| {
        state
            .docs
            .get(&key(machine, id))
            .cloned()
            .unwrap_or_default()
    })
}

pub fn record_docs(machine: &str, id: &str, docs: Docs) -> anyhow::Result<()> {
    with(|state| {
        if state.docs.get(&key(machine, id)) == Some(&docs) {
            return Ok(());
        }
        state.docs.insert(key(machine, id), docs);
        save(state)
    })
}
