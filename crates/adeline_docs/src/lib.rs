//! Docs mode without a UI: which project files are docs, checking the paths
//! clients and agents name, reading and saving doc text, and the docs tree
//! with its fuzzy search. The engine and the UI both use it.
//!
//! A doc's path is relative to the project root, with `/` separators.

// Rust guideline compliant 2026-10-09

mod files;
mod format;
mod text;
mod tree;

pub use files::{
    check_name, file_name, has_doc_extension, is_doc, is_markdown, join, list, parent,
    project_path, resolve,
};
pub use format::{Edit, Format, apply};
pub use text::{MAX_EDITABLE, conform, decode};
pub use tree::{Row, Tree};
