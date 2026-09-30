//! Filesystem helpers shared by the settings, agent and project stores.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// `{path}: {error}`.
pub fn error(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

/// A name no other call in this process has produced: `{prefix}-{pid}-{n}`.
pub fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// A folder ID as `agents::normalize_name` would produce it.
pub fn checked_id(id: &str) -> Result<(), String> {
    if crate::agents::normalize_name(id).as_deref() == Ok(id) {
        Ok(())
    } else {
        Err(format!("Invalid folder name: {id}"))
    }
}

/// Creates a file that must not exist yet, synced to disk; nothing is left
/// behind on failure.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| error(path, e))?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error(path, e));
    }
    Ok(())
}

/// Creates a file unless one already exists, which is left untouched.
pub fn seed(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(bytes).map_err(|e| error(path, e)),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(error(path, e)),
    }
}

/// Replaces an existing file's contents. The new contents are fully synced
/// before they take the file's place, and the original is restored if that fails.
pub fn replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(unique("tmp"));
    write_new(&temp, bytes)?;
    let backup = path.with_extension(unique("bak"));
    if let Err(e) = fs::rename(path, &backup) {
        let _ = fs::remove_file(&temp);
        return Err(error(path, e));
    }
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temp);
        return Err(error(path, e));
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

/// A fresh directory under the system temp folder, removed on drop.
#[cfg(test)]
pub struct TempDir(std::path::PathBuf);
#[cfg(test)]
impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(unique(prefix));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
#[cfg(test)]
impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}
#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
