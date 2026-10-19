//! The engine's log file, rotated at `LOG_CAP` and kept to three old copies.
//! Code logs through `tracing` events with dot-separated names and named fields;
//! a process that never calls `open` drops them.
use std::{
    fmt,
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, MakeWriter, format::Writer},
    registry::LookupSpan,
};

const LOG_CAP: u64 = 1024 * 1024;

// The open `engine.log` and its size. Only the engine holding the
// single-instance lock writes it, so the size is counted here, not read back.
#[derive(Default)]
struct Open {
    file: Option<File>,
    size: u64,
}

struct LogFile {
    dir: PathBuf,
    open: Mutex<Open>,
}

struct LogWriter<'a> {
    dir: &'a Path,
    open: MutexGuard<'a, Open>,
}

impl io::Write for LogWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let path = self.dir.join("engine.log");
        if self.open.size > LOG_CAP {
            self.open.file = None;
            self.open.size = 0;
            for index in (1..3).rev() {
                let _ = std::fs::rename(
                    self.dir.join(format!("engine.{index}.log")),
                    self.dir.join(format!("engine.{}.log", index + 1)),
                );
            }
            let _ = std::fs::rename(&path, self.dir.join("engine.1.log"));
        }
        if self.open.file.is_none() {
            // Without a log file, events are dropped.
            let Ok(file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            else {
                return Ok(buf.len());
            };
            self.open.size = file.metadata().map_or(0, |meta| meta.len());
            self.open.file = Some(file);
        }
        let Some(file) = self.open.file.as_mut() else {
            return Ok(buf.len());
        };
        let written = file.write(buf)?;
        self.open.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.open.file.as_mut().map_or(Ok(()), File::flush)
    }
}

impl<'a> MakeWriter<'a> for LogFile {
    type Writer = LogWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter {
            dir: &self.dir,
            open: self.open.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }
}

// One line per event: local time, process, level, event name, message, fields.
struct Line;

impl<S, N> FormatEvent<S, N> for Line
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        write!(
            writer,
            "{} [{}] {} {}: ",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            std::process::id(),
            meta.level(),
            meta.name()
        )?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

pub fn open(dir: PathBuf) {
    let _ = tracing_subscriber::fmt()
        .with_writer(LogFile {
            dir,
            open: Mutex::default(),
        })
        .with_ansi(false)
        .event_format(Line)
        .try_init();
}

#[cfg(test)]
mod tests {
    #[test]
    fn events_reach_the_log_file_with_their_fields() {
        let dir = crate::files::TempDir::new("adeline-log");
        super::open(dir.to_path_buf());
        tracing::event!(
            name: "test.log.written",
            tracing::Level::INFO,
            client.id = 7,
            "Client {{client.id}} connected"
        );
        let text = std::fs::read_to_string(dir.join("engine.log")).unwrap();
        assert!(
            text.contains("INFO test.log.written: Client {client.id} connected client.id=7"),
            "{text}"
        );
        assert!(
            text.contains(&format!("[{}]", std::process::id())),
            "{text}"
        );
    }

    #[test]
    fn a_full_log_rotates_and_keeps_three_old_copies() {
        let dir = crate::files::TempDir::new("adeline-log");
        super::open(dir.to_path_buf());
        let line = "x".repeat(1024);
        for _ in 0..(5 * 1024) {
            tracing::event!(name: "test.log.filler", tracing::Level::INFO, "{line}");
        }
        let mut names: Vec<String> = std::fs::read_dir(&*dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["engine.1.log", "engine.2.log", "engine.3.log", "engine.log"]
        );
        for name in &names {
            let size = std::fs::metadata(dir.join(name)).unwrap().len();
            assert!(size <= super::LOG_CAP + 2 * 1024, "{name} is {size} bytes");
        }
    }
}
