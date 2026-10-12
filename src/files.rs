//! Filesystem helpers shared by the settings, agent and project stores.
//! Writes are synced before they replace anything, and a failed `replace`
//! restores the original.
use anyhow::{Context as _, Result, bail};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn error(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

pub fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn random_id() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    // RandomState's keys are random per process, so no rand crate is needed.
    let half = || {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

pub fn checked_id(id: &str) -> Result<()> {
    if crate::agents::normalize_name(id).ok().as_deref() != Some(id) {
        bail!("Invalid folder name: {id}");
    }
    Ok(())
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| path.display().to_string())?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(e).with_context(|| path.display().to_string());
    }
    Ok(())
}

pub fn seed(path: &Path, bytes: &[u8]) -> Result<()> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file
            .write_all(bytes)
            .with_context(|| path.display().to_string()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e).with_context(|| path.display().to_string()),
    }
}

pub fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension(unique("tmp"));
    write_new(&temp, bytes)?;
    let backup = path.with_extension(unique("bak"));
    if let Err(e) = fs::rename(path, &backup) {
        let _ = fs::remove_file(&temp);
        return Err(e).with_context(|| path.display().to_string());
    }
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temp);
        return Err(e).with_context(|| path.display().to_string());
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

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
