//! Cards: a chat reply's links to what a mode shows, such as written docs.
//!
//! The engine makes them from agent output it has checked, and they carry no
//! commands: a client can only open the card's route or copy its path.
//!
//! A route is `adeline://<mode>/<path>`, relative to the chat's project. A
//! chat holds one card per route, so a later change updates the card in place.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

const SCHEME: &str = "adeline://";

/// An `adeline://<mode>/<path>` route.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityRef(String);

impl EntityRef {
    pub fn doc(path: &str) -> Self {
        Self(format!("{SCHEME}docs/{path}"))
    }

    pub fn parse(route: &str) -> Option<Self> {
        let rest = route.strip_prefix(SCHEME)?;
        let (mode, path) = rest.split_once('/')?;
        (!mode.is_empty() && !path.is_empty()).then(|| Self(route.to_owned()))
    }

    pub fn mode(&self) -> &str {
        self.rest().split_once('/').map_or("", |(mode, _)| mode)
    }

    pub fn path(&self) -> &str {
        self.rest().split_once('/').map_or("", |(_, path)| path)
    }

    fn rest(&self) -> &str {
        self.0.strip_prefix(SCHEME).unwrap_or_default()
    }
}

impl std::fmt::Display for EntityRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Card {
    pub entity: EntityRef,
    pub title: String,
    pub subtitle: String,
    // The user message that started the turn the card belongs to.
    pub turn: Option<usize>,
    // Epoch milliseconds of the entity's last change.
    pub edited: u64,
    // The engine noticed the entity is gone.
    pub gone: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_name_their_mode_and_keep_the_path_opaque() {
        let route = EntityRef::doc("notes/plan v2.md");
        assert_eq!(route.to_string(), "adeline://docs/notes/plan v2.md");
        assert_eq!((route.mode(), route.path()), ("docs", "notes/plan v2.md"));
        assert_eq!(EntityRef::parse(&route.to_string()), Some(route));
        for invalid in [
            "https://x/y",
            "adeline://docs",
            "adeline:///a",
            "adeline://docs/",
        ] {
            assert_eq!(EntityRef::parse(invalid), None, "{invalid}");
        }
    }
}
