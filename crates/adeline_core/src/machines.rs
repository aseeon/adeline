//! The machines this client knows: the local machine and the remote machines saved
//! here, which ones are checked, and which one "Open folder…" used last.
//!
//! Kept in `machines.yml`; other clients never see it. Demo mode keeps fake
//! machines in memory and never touches the file. The last checked machine can't be
//! unchecked, and removing a machine leaves its engine and data untouched. A file
//! that can't be read or parsed is never overwritten.
use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
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
    // Tried in this order on every connect.
    pub destinations: Vec<String>,
    // The engine it was first connected to.
    pub engine: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Store {
    machines: Vec<Machine>,
    checked: Vec<String>,
    last_folder: Option<String>,
    #[serde(skip)]
    unreadable: Option<String>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static DEMO: AtomicBool = AtomicBool::new(false);

pub fn init(demo: bool) {
    DEMO.store(demo, Ordering::Relaxed);
}

pub fn demo() -> bool {
    DEMO.load(Ordering::Relaxed)
}

fn path() -> anyhow::Result<PathBuf> {
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
        unreadable: None,
    }
}

fn load(path: &Path) -> Store {
    let unreadable = |error: String| Store {
        unreadable: Some(error),
        ..Store::default()
    };
    match std::fs::read_to_string(path) {
        Ok(text) => serde_yaml_ng::from_str(&text)
            .unwrap_or_else(|e| unreadable(crate::files::error(path, e))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
        Err(e) => unreadable(crate::files::error(path, e)),
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
        path().map_or_else(
            |e| Store {
                unreadable: Some(format!("{e:#}")),
                ..Store::default()
            },
            |path| load(&path),
        )
    });
    // At least one machine stays checked, and only known ones count.
    let known: Vec<String> = store.machines.iter().map(|m| m.id.clone()).collect();
    store.checked.retain(|id| id == LOCAL || known.contains(id));
    if store.checked.is_empty() {
        store.checked.push(LOCAL.into());
    }
    change(store)
}

// Changes a copy, so a failed save leaves the machines as they were.
fn update<T>(change: impl FnOnce(&mut Store) -> anyhow::Result<T>) -> anyhow::Result<T> {
    with(|store| {
        let mut changed = store.clone();
        let value = change(&mut changed)?;
        save(&changed)?;
        *store = changed;
        Ok(value)
    })
}

fn save(store: &Store) -> anyhow::Result<()> {
    if demo() {
        return Ok(());
    }
    if let Some(error) = &store.unreadable {
        bail!("{error}");
    }
    let path = path()?;
    crate::config::seed_yaml(&path, store).and_then(|()| crate::config::write_yaml(&path, store))
}

pub fn remotes() -> Vec<Machine> {
    with(|store| store.machines.clone())
}

pub fn remote(id: &str) -> Option<Machine> {
    with(|store| store.machines.iter().find(|m| m.id == id).cloned())
}

pub fn all() -> Vec<String> {
    let mut ids = vec![LOCAL.to_owned()];
    ids.extend(remotes().into_iter().map(|m| m.id));
    ids
}

pub fn name(id: &str) -> String {
    if id == LOCAL {
        if demo() {
            return "Nexus".into();
        }
        // macOS reports names like "Mac.local"; the label wants just "Mac".
        let host = gethostname::gethostname().to_string_lossy().into_owned();
        return match host.split('.').next() {
            Some(host) if !host.is_empty() => host.into(),
            _ => "Local machine".into(),
        };
    }
    remote(id).map_or_else(|| id.to_owned(), |m| m.name)
}

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

pub fn set_checked(id: &str, checked: bool) -> anyhow::Result<()> {
    update(|store| {
        if checked {
            if !store.checked.iter().any(|c| c == id) {
                store.checked.push(id.to_owned());
            }
        } else {
            if store.checked.len() == 1 && store.checked[0] == id {
                bail!("At least one machine stays checked.");
            }
            store.checked.retain(|c| c != id);
        }
        Ok(())
    })
}

pub fn add(name: &str, destination: &str) -> anyhow::Result<String> {
    let name = name.trim();
    let destination = destination.trim();
    if name.is_empty() {
        bail!("Enter a name for the machine.");
    }
    check_destination(destination)?;
    update(|store| {
        if store
            .machines
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(name))
            || name.eq_ignore_ascii_case(&self::name(LOCAL))
        {
            bail!("A machine named \"{name}\" already exists.");
        }
        let id = format!("m-{}", &crate::files::random_id()[..12]);
        store.machines.push(Machine {
            id: id.clone(),
            name: name.to_owned(),
            destinations: vec![destination.to_owned()],
            engine: None,
        });
        store.checked.push(id.clone());
        Ok(id)
    })
}

pub fn edit(id: &str, edit: impl FnOnce(&mut Machine) -> anyhow::Result<()>) -> anyhow::Result<()> {
    update(|store| {
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
            .context("That machine is no longer saved.")?;
        let mut changed = machine.clone();
        edit(&mut changed)?;
        changed.name = changed.name.trim().to_string();
        if changed.name.is_empty() {
            bail!("Enter a name for the machine.");
        }
        if others.contains(&changed.name.to_lowercase()) {
            bail!("A machine named \"{}\" already exists.", changed.name);
        }
        if changed.destinations.is_empty() {
            bail!("A machine needs at least one destination.");
        }
        *machine = changed;
        Ok(())
    })
}

pub fn remove(id: &str) -> anyhow::Result<()> {
    update(|store| {
        store.machines.retain(|m| m.id != id);
        store.checked.retain(|c| c != id);
        if store.checked.is_empty() {
            store.checked.push(LOCAL.into());
        }
        if store.last_folder.as_deref() == Some(id) {
            store.last_folder = None;
        }
        Ok(())
    })
}

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

pub fn split_port(destination: &str) -> (&str, Option<&str>) {
    match destination.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty()
                && !host.contains(':')
                && !port.is_empty()
                && port.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (host, Some(port))
        }
        _ => (destination, None),
    }
}

pub fn check_destination(destination: &str) -> anyhow::Result<()> {
    let (host, _) = split_port(destination);
    if host.is_empty()
        || host.starts_with('-')
        || host.contains(char::is_whitespace)
        || host.ends_with('@')
        || host.starts_with('@')
    {
        anyhow::bail!(
            "\"{destination}\" isn't a destination. Use user@host, user@host:port, or a Host alias from ~/.ssh/config."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destinations_split_off_a_numeric_port() {
        assert_eq!(
            split_port("me@home.example.com:2222"),
            ("me@home.example.com", Some("2222"))
        );
        assert_eq!(split_port("desktop.lan"), ("desktop.lan", None));
        assert_eq!(split_port("alias"), ("alias", None));
        check_destination("me@host:22").unwrap();
        check_destination("desktop").unwrap();
        assert!(check_destination("-oProxyCommand=x").is_err());
        assert!(check_destination("me@").is_err());
        assert!(check_destination("two words").is_err());
    }

    #[test]
    fn an_unparsable_file_blocks_saving() {
        let dir = crate::files::TempDir::new("adeline-machines");
        let path = dir.join("machines.yml");
        assert!(load(&path).unreadable.is_none());
        std::fs::write(&path, "machines: [unclosed").unwrap();
        let store = load(&path);
        assert!(store.unreadable.is_some());
        assert!(save(&store).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "machines: [unclosed"
        );
    }

    #[test]
    fn a_failed_save_leaves_the_machines_unchanged() {
        *STORE.lock().unwrap() = Some(Store {
            unreadable: Some("machines.yml: unreadable".into()),
            ..Store::default()
        });
        add("Box", "me@box").unwrap_err();
        assert!(remotes().is_empty());
        set_checked("other", true).unwrap_err();
        assert_eq!(checked(), [LOCAL]);
    }
}
