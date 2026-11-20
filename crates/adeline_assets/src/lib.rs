//! Files from `assets/` and `bundled-themes/`, embedded at build time, plus the
//! titlebar icon rendered at every size it's drawn at.
include!(concat!(env!("OUT_DIR"), "/assets.rs"));
include!(concat!(env!("OUT_DIR"), "/themes.rs"));

pub fn embedded(path: &str) -> Option<&'static [u8]> {
    EMBEDDED
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}
