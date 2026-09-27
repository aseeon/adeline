//! Opt-in render counters for checking cache boundaries in the actual native UI.
#[derive(Clone, Copy)]
pub(crate) enum Region {
    Shell,
    Header,
    Sidebar,
    Transcript,
    Composer,
    ChatRow,
    MessageRow,
}

#[inline]
pub(crate) fn record(region: Region) {
    #[cfg(feature = "ui-profiling")]
    profiling::record(region);
    #[cfg(not(feature = "ui-profiling"))]
    let _ = region;
}

/// Optional synthetic data for native list checks. Never compiled into the
/// normal executable. Delete the control file to use the regular fixtures.
#[cfg(feature = "ui-profiling")]
pub(crate) fn stress_fixture(mut projects: Vec<crate::Workspace>) -> Vec<crate::Workspace> {
    let count = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/artifacts/ui-stress-rows.txt"
    ))
    .ok()
    .and_then(|s| s.trim().parse::<usize>().ok())
    .unwrap_or(0)
    .min(10_000);
    if count == 0 {
        return projects;
    }
    let template = projects[0].threads[0].clone();
    projects[0].threads = (0..count)
        .map(|i| {
            let mut thread = template.clone();
            thread.id = format!("stress-{i}");
            thread.title = format!("Stress chat {:05}", i + 1);
            thread
        })
        .collect();
    projects[0].threads[0].messages = (0..count)
        .map(|i| {
            let mut message = template.messages[i % template.messages.len()].clone();
            message.text = format!("Message {:05}\n\n{}", i + 1, message.text);
            message
        })
        .collect();
    for thread in &mut projects[0].threads {
        thread.prepare_search();
    }
    projects
}

#[cfg(feature = "ui-profiling")]
mod profiling {
    use super::Region;
    use std::{
        io::Write,
        sync::{
            Once,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };
    static START: Once = Once::new();
    static COUNTS: [AtomicU64; 7] = [const { AtomicU64::new(0) }; 7];

    #[expect(
        clippy::disallowed_methods,
        reason = "the sleep runs on a dedicated sampling thread, not the UI thread"
    )]
    pub fn record(region: Region) {
        COUNTS[region as usize].fetch_add(1, Ordering::Relaxed);
        START.call_once(|| {
            std::thread::spawn(|| {
                let path = std::path::Path::new(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/artifacts/ui-profile.csv"
                ));
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let Ok(mut file) = std::fs::File::create(path) else {
                    return;
                };
                let _ = writeln!(
                    file,
                    "shell,header,sidebar,transcript,composer,chat_rows,message_rows"
                );
                loop {
                    let line = COUNTS
                        .iter()
                        .map(|c| c.load(Ordering::Relaxed).to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    if writeln!(file, "{line}").is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            });
        });
    }
}
