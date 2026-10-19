//! The headless build's command line. Runs in the headless test step
//! (`--no-default-features`), where `CARGO_BIN_EXE_adeline` is that build.
#![cfg(not(feature = "gui"))]
#![expect(
    clippy::disallowed_methods,
    reason = "tests block on processes off any UI thread"
)]

mod common;

use std::path::Path;
use std::process::Command;

#[test]
fn headless_cli() {
    let exe = Path::new(env!("CARGO_BIN_EXE_adeline"));
    if cfg!(windows) {
        assert_eq!(common::windows_subsystem(exe), 3, "not a console program");
    }
    // Without a window to open, anything but a role prints the usage.
    for args in [&[][..], &["--demo"], &["frobnicate"]] {
        let output = Command::new(exe).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let usage = String::from_utf8_lossy(&output.stderr);
        for word in ["engine", "bridge", "--version"] {
            assert!(usage.contains(word), "{usage}");
        }
    }
    let version = Command::new(exe).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("{} (headless)", env!("CARGO_PKG_VERSION"))
    );
    // A short home: macOS limits the engine's socket path to 104 bytes.
    let home = common::home("headless-cli");
    let status = || {
        String::from_utf8_lossy(&common::adeline(&home, &["engine", "status"]).stdout).into_owned()
    };
    assert!(status().contains("not running"), "{}", status());
    let started = common::adeline(&home, &["engine", "start"]);
    assert!(started.status.success(), "{started:?}");
    let running = status();
    common::adeline(&home, &["engine", "stop"]);
    assert!(running.contains("Build: headless"), "{running}");
}
