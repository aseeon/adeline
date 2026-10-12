//! Which settings pages match the search box.
use super::*;

pub(super) fn agent_matches(entry: &agents::AgentEntry, query: &str) -> bool {
    matches_query(
        query,
        &[
            "Agents",
            &entry.definition.name,
            &entry.definition.model,
            &entry.definition.command,
        ],
    )
}

pub(super) fn matches_query(query: &str, parts: &[&str]) -> bool {
    query
        .split_whitespace()
        .all(|word| parts.iter().any(|part| part.to_lowercase().contains(word)))
}

pub(super) fn general_matches(child: usize, query: &str) -> bool {
    match child {
        0 => MODES.iter().any(|(_, name)| {
            matches_query(
                query,
                &["General", "Features", name, "Enable mode in the main view"],
            )
        }),
        1 => [
            "Theme",
            "Interface font",
            "Code font",
            "Interface font size",
            "Code font size",
        ]
        .iter()
        .any(|label| matches_query(query, &["General", "Appearance", label])),
        2 => KEYMAP
            .iter()
            .any(|label| matches_query(query, &["General", "Keymap", label])),
        _ => false,
    }
}

pub(super) fn engine_matches(query: &str) -> bool {
    [KEEP_RUNNING, RETRY_LABEL, "daemon"]
        .iter()
        .chain(&ENGINE_STATUS)
        .any(|label| matches_query(query, &["Engine", label]))
}
