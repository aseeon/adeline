//! Profiling builds only (`--features profiler`): with `ADELINE_FRAME_LOG` set to a
//! file path, every drawn and presented frame is appended to it as a CSV row,
//! timestamped in Unix milliseconds so a driver script can slice it by scenario.
use gpui_kit::{FrameEvent, FrameTimingCollector, set_trace_enabled};
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn start() {
    let Some(path) = std::env::var_os("ADELINE_FRAME_LOG") else {
        return;
    };
    let Ok(mut file) = std::fs::File::create(&path) else {
        return;
    };
    set_trace_enabled(true);
    let anchor = Instant::now();
    let anchor_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0;
    let ms =
        move |at: Instant| anchor_ms + at.saturating_duration_since(anchor).as_secs_f64() * 1000.0;
    let us = |duration: Duration| duration.as_secs_f64() * 1_000_000.0;
    let _ = writeln!(
        file,
        "kind,at_ms,duration_us,dirty_to_end_us,invalidations,interval_us"
    );
    std::thread::spawn(move || {
        let mut collector = FrameTimingCollector::new();
        loop {
            #[expect(
                clippy::disallowed_methods,
                reason = "the logger's own thread, not the UI thread"
            )]
            std::thread::sleep(Duration::from_millis(100));
            for event in collector.collect_unseen() {
                let _ = match event {
                    FrameEvent::Draw(frame) => writeln!(
                        file,
                        "draw,{:.3},{:.1},{},{},",
                        ms(frame.draw_start),
                        us(frame.draw_duration()),
                        frame
                            .dirty_to_draw_duration()
                            .map_or(String::new(), |d| format!("{:.1}", us(d))),
                        frame.invalidations,
                    ),
                    FrameEvent::Present(present) => writeln!(
                        file,
                        "present,{:.3},{:.1},,,{}",
                        ms(present.present_start),
                        us(present.present_duration()),
                        present
                            .animation_interval
                            .map_or(String::new(), |d| format!("{:.1}", us(d))),
                    ),
                };
            }
            let _ = file.flush();
        }
    });
}
