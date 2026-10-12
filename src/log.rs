//! The engine's log file, rotated at `LOG_CAP` and kept to three old copies.
use std::{io::Write as _, path::PathBuf, sync::Mutex};

static LOG: Mutex<Option<PathBuf>> = Mutex::new(None);
const LOG_CAP: u64 = 1024 * 1024;

pub fn open(dir: PathBuf) {
    if let Ok(mut guard) = LOG.lock() {
        *guard = Some(dir);
    }
}

pub fn log(message: impl std::fmt::Display) {
    let Ok(guard) = LOG.lock() else {
        return;
    };
    let Some(dir) = guard.as_ref() else {
        return;
    };
    let path = dir.join("engine.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > LOG_CAP) {
        for index in (1..3).rev() {
            let _ = std::fs::rename(
                dir.join(format!("engine.{index}.log")),
                dir.join(format!("engine.{}.log", index + 1)),
            );
        }
        let _ = std::fs::rename(&path, dir.join("engine.1.log"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(
            file,
            "{} [{}] {message}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            std::process::id()
        );
    }
}
