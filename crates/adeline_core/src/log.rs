//! The engine's log file, rotated at `LOG_CAP` and kept to three old copies.
//! Code logs through `tracing` events with dot-separated names and named fields;
//! a process that never calls `open` drops them.
use std::{fmt, io, path::PathBuf};
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, MakeWriter, format::Writer},
    registry::LookupSpan,
};

const LOG_CAP: u64 = 1024 * 1024;

struct LogFile(PathBuf);

impl LogFile {
    fn rotate(&self, path: &std::path::Path) {
        if std::fs::metadata(path).is_ok_and(|meta| meta.len() > LOG_CAP) {
            for index in (1..3).rev() {
                let _ = std::fs::rename(
                    self.0.join(format!("engine.{index}.log")),
                    self.0.join(format!("engine.{}.log", index + 1)),
                );
            }
            let _ = std::fs::rename(path, self.0.join("engine.1.log"));
        }
    }
}

impl<'a> MakeWriter<'a> for LogFile {
    type Writer = Box<dyn io::Write>;

    fn make_writer(&'a self) -> Self::Writer {
        let path = self.0.join("engine.log");
        self.rotate(&path);
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_or_else(
                |_| Box::new(io::sink()) as Self::Writer,
                |file| Box::new(file),
            )
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
        .with_writer(LogFile(dir))
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
}
