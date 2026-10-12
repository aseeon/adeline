//! Files from `assets/` and `bundled-themes/`, embedded at build time, plus the
//! titlebar icon rendered at every size it's drawn at.
include!(concat!(env!("OUT_DIR"), "/assets.rs"));
include!(concat!(env!("OUT_DIR"), "/themes.rs"));
