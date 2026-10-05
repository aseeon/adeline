//! The machines this client knows: the local machine and the remote machines
//! saved here, which ones are checked, and which one "Open folder…" used last.
//! Kept in `machines.yml`; other clients never see it.
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub const LOCAL: &str = "local";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Machine {
    pub id: String,
    pub name: String,
    /// Tried in this order on every connect.
    pub destinations: Vec<String>,
    /// The engine it was first connected to.
    pub engine: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Store {
    machines: Vec<Machine>,
    checked: Vec<String>,
    last_folder: Option<String>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static DEMO: AtomicBool = AtomicBool::new(false);

/// Demo mode keeps fake machines in memory and never touches the file.
pub fn init(demo: bool) {
    DEMO.store(demo, Ordering::Relaxed);
}

pub fn demo() -> bool {
    DEMO.load(Ordering::Relaxed)
}

fn path() -> Result<PathBuf, String> {
    Ok(crate::config::directory()?.join("machines.yml"))
}

fn demo_store() -> Store {
    let machine = |id: &str, name: &str| Machine {
        id: id.into(),
        name: name.into(),
        destinations: vec![format!("{id}.example.com")],
        engine: Some(id.into()),
    };
    Store {
        machines: vec![machine("matrix", "Matrix"), machine("vortex", "Vortex")],
        checked: vec![LOCAL.into(), "matrix".into(), "vortex".into()],
        last_folder: None,
    }
}

fn with<T>(change: impl FnOnce(&mut Store) -> T) -> T {
    let mut guard = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let store = guard.get_or_insert_with(|| {
        if demo() {
            return demo_store();
        }
        path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_yaml_ng::from_str(&text).ok())
            .unwrap_or_default()
    });
    // At least one machine stays checked, and only known ones count.
    let known: Vec<String> = store.machines.iter().map(|m| m.id.clone()).collect();
    store.checked.retain(|id| id == LOCAL || known.contains(id));
    if store.checked.is_empty() {
        store.checked.push(LOCAL.into());
    }
    change(store)
}

fn save(store: &Store) -> Result<(), String> {
    if demo() {
        return Ok(());
    }
    let path = path()?;
    crate::config::seed_yaml(&path, store).and_then(|()| crate::config::write_yaml(&path, store))
}

/// The saved remote machines, in the order they were added.
pub fn remotes() -> Vec<Machine> {
    with(|store| store.machines.clone())
}

pub fn remote(id: &str) -> Option<Machine> {
    with(|store| store.machines.iter().find(|m| m.id == id).cloned())
}

/// Every machine's ID, the local machine first.
pub fn all() -> Vec<String> {
    let mut ids = vec![LOCAL.to_owned()];
    ids.extend(remotes().into_iter().map(|m| m.id));
    ids
}

pub fn name(id: &str) -> String {
    if id == LOCAL {
        return if demo() { "Nexus" } else { "Local machine" }.into();
    }
    remote(id).map_or_else(|| id.to_owned(), |m| m.name)
}

/// The checked machines, in `all` order.
pub fn checked() -> Vec<String> {
    let checked = with(|store| store.checked.clone());
    all()
        .into_iter()
        .filter(|id| checked.contains(id))
        .collect()
}

pub fn is_checked(id: &str) -> bool {
    with(|store| store.checked.iter().any(|c| c == id))
}

/// Checks or unchecks a machine. The last checked one can't be unchecked.
pub fn set_checked(id: &str, checked: bool) -> Result<(), String> {
    with(|store| {
        if checked {
            if !store.checked.iter().any(|c| c == id) {
                store.checked.push(id.to_owned());
            }
        } else {
            if store.checked.len() == 1 && store.checked[0] == id {
                return Err("At least one machine stays checked.".into());
            }
            store.checked.retain(|c| c != id);
        }
        save(store)
    })
}

/// Saves a new machine, checked. Returns its ID.
pub fn add(name: &str, destination: &str) -> Result<String, String> {
    let name = name.trim();
    let destination = destination.trim();
    if name.is_empty() {
        return Err("Enter a name for the machine.".into());
    }
    crate::remote::check_destination(destination)?;
    with(|store| {
        if store
            .machines
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(name))
            || name.eq_ignore_ascii_case(&self::name(LOCAL))
        {
            return Err(format!("A machine named \"{name}\" already exists."));
        }
        let id = format!("m-{}", &crate::files::random_id()[..12]);
        store.machines.push(Machine {
            id: id.clone(),
            name: name.to_owned(),
            destinations: vec![destination.to_owned()],
            engine: None,
        });
        store.checked.push(id.clone());
        save(store).map(|()| id)
    })
}

/// Changes a saved machine with `edit`, which may refuse.
pub fn edit(id: &str, edit: impl FnOnce(&mut Machine) -> Result<(), String>) -> Result<(), String> {
    with(|store| {
        let others: Vec<String> = store
            .machines
            .iter()
            .filter(|m| m.id != id)
            .map(|m| m.name.to_lowercase())
            .collect();
        let machine = store
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or("That machine is no longer saved.")?;
        let mut changed = machine.clone();
        edit(&mut changed)?;
        changed.name = changed.name.trim().to_string();
        if changed.name.is_empty() {
            return Err("Enter a name for the machine.".into());
        }
        if others.contains(&changed.name.to_lowercase()) {
            return Err(format!(
                "A machine named \"{}\" already exists.",
                changed.name
            ));
        }
        if changed.destinations.is_empty() {
            return Err("A machine needs at least one destination.".into());
        }
        *machine = changed;
        save(store)
    })
}

/// Forgets a machine on this client. Its engine and data are untouched.
pub fn remove(id: &str) -> Result<(), String> {
    with(|store| {
        store.machines.retain(|m| m.id != id);
        store.checked.retain(|c| c != id);
        if store.checked.is_empty() {
            store.checked.push(LOCAL.into());
        }
        if store.last_folder.as_deref() == Some(id) {
            store.last_folder = None;
        }
        save(store)
    })
}

/// The saved machine whose engine this is, other than `except`.
pub fn with_engine(engine: &str, except: &str) -> Option<Machine> {
    with(|store| {
        store
            .machines
            .iter()
            .find(|m| m.id != except && m.engine.as_deref() == Some(engine))
            .cloned()
    })
}

pub fn last_folder() -> Option<String> {
    with(|store| store.last_folder.clone())
}

pub fn set_last_folder(id: &str) {
    with(|store| {
        if store.last_folder.as_deref() != Some(id) {
            store.last_folder = Some(id.to_owned());
            let _ = save(store);
        }
    });
}
