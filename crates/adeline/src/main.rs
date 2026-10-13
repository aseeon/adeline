//! Entry point. `adeline engine` runs the conversation engine and `adeline bridge`
//! joins an SSH session to it. Anything else opens the UI (`--demo` uses bundled
//! data, `--verbose` shows debugging views such as ACP traffic). Without the `gui` feature, the headless build has no UI and prints its
//! usage instead.
#![cfg_attr(
    all(target_os = "windows", feature = "gui"),
    windows_subsystem = "windows"
)]
use adeline_core::{askpass, platform, remote};
use adeline_engine::engine;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `ssh` runs this executable to ask for a password or a host key.
    if let Some(address) = std::env::var_os("ADELINE_ASKPASS") {
        let prompt = args.first().map_or("", String::as_str);
        std::process::exit(askpass::askpass_main(&address.to_string_lossy(), prompt));
    }
    match args.first().map(String::as_str) {
        Some("engine") => std::process::exit(engine::main(&args[1..])),
        Some("bridge") => std::process::exit(remote::bridge_main()),
        Some("--version") => {
            platform::attach_parent_console();
            let headless = if cfg!(feature = "gui") {
                ""
            } else {
                " (headless)"
            };
            println!("{}{headless}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        _ => {}
    }
    #[cfg(feature = "gui")]
    adeline_ui::open(
        args.iter().any(|arg| arg == "--demo"),
        args.iter().any(|arg| arg == "--verbose"),
    );
    #[cfg(not(feature = "gui"))]
    {
        platform::attach_parent_console();
        eprintln!(
            "Usage: adeline engine [status | start [--daemon] | stop] | adeline bridge | adeline --version"
        );
        std::process::exit(2);
    }
}
