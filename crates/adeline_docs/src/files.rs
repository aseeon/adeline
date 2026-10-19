//! Which files in a project are docs, and turning the paths clients and agents
//! name into checked paths inside the project.
//!
//! With `respect_gitignore`, files the project's `.gitignore` files (and
//! `.git/info/exclude`) ignore and files in hidden folders are not docs.
//! `.git` itself never holds docs.

// Rust guideline compliant 2026-10-09

use ignore::{
    Match, WalkBuilder,
    gitignore::{Gitignore, GitignoreBuilder},
};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

const EXTENSIONS: [&str; 6] = ["md", "markdown", "yaml", "yml", "toml", "txt"];

fn extension(path: &str) -> Option<&str> {
    let (stem, extension) = file_name(path).rsplit_once('.')?;
    (!stem.is_empty()).then_some(extension)
}

pub fn has_doc_extension(path: &str) -> bool {
    extension(path).is_some_and(|ext| EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

pub fn is_markdown(path: &str) -> bool {
    extension(path)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn skipped_folder(name: &OsStr, respect_gitignore: bool) -> bool {
    name == ".git" || (respect_gitignore && name.to_string_lossy().starts_with('.'))
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path
        .strip_prefix(root)
        .ok()?
        .iter()
        .map(OsStr::to_str)
        .collect();
    let parts = parts?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Every doc under `root`, sorted.
pub fn list(root: &Path, respect_gitignore: bool) -> Vec<String> {
    let mut docs: Vec<String> = WalkBuilder::new(root)
        .standard_filters(false)
        .git_ignore(respect_gitignore)
        .git_exclude(respect_gitignore)
        .require_git(false)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !skipped_folder(entry.file_name(), respect_gitignore)
        })
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .filter_map(|entry| relative(root, entry.path()))
        .filter(|path| has_doc_extension(path))
        .collect();
    docs.sort();
    docs
}

fn matcher(folder: &Path, file: &Path) -> Option<Gitignore> {
    if !file.is_file() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(folder);
    builder.add(file);
    builder.build().ok()
}

fn ignored(root: &Path, path: &str) -> bool {
    // The deepest `.gitignore` with a matching rule decides, as in git.
    let decide = |levels: &[Gitignore], path: &Path, is_dir: bool| {
        for matcher in levels.iter().rev() {
            match matcher.matched(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    };
    let mut levels: Vec<Gitignore> = [
        root.join(".git").join("info").join("exclude"),
        root.join(".gitignore"),
    ]
    .iter()
    .filter_map(|file| matcher(root, file))
    .collect();
    // An ignored folder hides everything in it, as git never looks inside.
    let mut folder = root.to_path_buf();
    for part in parent(path).split('/').filter(|part| !part.is_empty()) {
        folder.push(part);
        if decide(&levels, &folder, true) {
            return true;
        }
        levels.extend(matcher(&folder, &folder.join(".gitignore")));
    }
    decide(&levels, &root.join(path), false)
}

/// Whether `path` names a doc. It agrees with [`list`], but doesn't check that the file exists.
pub fn is_doc(root: &Path, path: &str, respect_gitignore: bool) -> bool {
    if !has_doc_extension(path) || clean(path).is_err() {
        return false;
    }
    let hidden = parent(path)
        .split('/')
        .any(|folder| skipped_folder(OsStr::new(folder), respect_gitignore));
    !(hidden || respect_gitignore && ignored(root, path))
}

fn clean(path: &str) -> Result<(), String> {
    let refused = || Err(format!("{path} is not a path inside the project."));
    if path.is_empty() {
        return refused();
    }
    for part in path.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains(['\\', ':'])
            || part.chars().any(char::is_control)
        {
            return refused();
        }
    }
    Ok(())
}

/// The file `path` names inside `root`, refusing paths that leave it, also through links.
pub fn resolve(root: &Path, path: &str) -> Result<PathBuf, String> {
    clean(path)?;
    let full = root.join(path);
    let root = root
        .canonicalize()
        .map_err(|error| format!("{}: {error}", root.display()))?;
    // The deepest part that exists shows where a link leads. A dangling link
    // can't show it, and writing through one could leave the project.
    let refused = || Err(format!("{path} is not a path inside the project."));
    for ancestor in full.ancestors() {
        if let Ok(existing) = ancestor.canonicalize() {
            return if existing.starts_with(&root) {
                Ok(full)
            } else {
                refused()
            };
        }
        if ancestor.symlink_metadata().is_ok() {
            return refused();
        }
    }
    refused()
}

/// The project path of an existing file an agent reported, absolute or relative to `root`.
pub fn project_path(root: &Path, reported: &str) -> Option<String> {
    let reported = Path::new(reported);
    let full = if reported.is_absolute() {
        reported.to_path_buf()
    } else {
        root.join(reported)
    };
    relative(&root.canonicalize().ok()?, &full.canonicalize().ok()?)
}

/// `target` from a link in the doc at `from`, as a project path. `None` when it leaves the project.
pub fn join(from: &str, target: &str) -> Option<String> {
    let mut parts: Vec<String> = if target.starts_with('/') {
        Vec::new()
    } else {
        parent(from)
            .split('/')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    };
    for part in target.split('/') {
        match percent_decoded(part)?.as_str() {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part.contains('/') => return None,
            part => parts.push(part.to_owned()),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn percent_decoded(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|hex| bytes[i] == b'%' && hex.iter().all(u8::is_ascii_hexdigit));
        match hex {
            Some(hex) => {
                decoded.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                i += 3;
            }
            None => {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

/// Checks a file name the user chose for a doc.
pub fn check_name(name: &str) -> Result<(), String> {
    const RESERVED: [&str; 4] = ["con", "prn", "aux", "nul"];
    let invalid = |reason: &str| Err(format!("\"{name}\" can't be used: {reason}."));
    if name.trim().is_empty() {
        return invalid("the name is empty");
    }
    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || name.chars().any(char::is_control)
    {
        return invalid("names can't contain / \\ : * ? \" < > |");
    }
    if name.starts_with(' ') || name.ends_with([' ', '.']) {
        return invalid("names can't start with a space or end with a space or a dot");
    }
    let stem = name.split('.').next().unwrap_or(name).to_lowercase();
    let numbered = (stem.starts_with("com") || stem.starts_with("lpt"))
        && stem.len() == 4
        && stem.ends_with(|c: char| c.is_ascii_digit());
    if RESERVED.contains(&stem.as_str()) || numbered {
        return invalid("Windows reserves that name");
    }
    if !has_doc_extension(name) {
        return invalid("a doc ends in .md, .markdown, .yaml, .yml, .toml or .txt");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "adeline-docs-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    #[test]
    fn listing_and_single_checks_agree_on_which_files_are_docs() {
        let root = project(&[
            (".gitignore", "target/\n*.log.md\n"),
            ("a.md", ""),
            ("b.yaml", ""),
            ("c.toml", ""),
            ("d.txt", ""),
            ("e.rs", ""),
            ("target/f.md", ""),
            ("target/.gitignore", "!f.md\n"),
            (".hidden/g.md", ""),
            (".git/h.md", ""),
            ("notes/run.log.md", ""),
            ("notes/.gitignore", "!keep.log.md\nlocal.md\n"),
            ("notes/keep.log.md", ""),
            ("notes/local.md", ""),
            ("notes/Plan.MD", ""),
        ]);
        let respected = list(&root, true);
        assert_eq!(
            respected,
            [
                "a.md",
                "b.yaml",
                "c.toml",
                "d.txt",
                "notes/Plan.MD",
                "notes/keep.log.md"
            ]
        );
        let all = list(&root, false);
        assert_eq!(
            all,
            [
                ".hidden/g.md",
                "a.md",
                "b.yaml",
                "c.toml",
                "d.txt",
                "notes/Plan.MD",
                "notes/keep.log.md",
                "notes/local.md",
                "notes/run.log.md",
                "target/f.md"
            ]
        );
        for respect in [true, false] {
            let listed = list(&root, respect);
            for path in &all {
                assert_eq!(
                    is_doc(&root, path, respect),
                    listed.contains(path),
                    "{path} with respect {respect}"
                );
            }
        }
        assert!(!is_doc(&root, "e.rs", false));
        assert!(!is_doc(&root, ".git/h.md", false));
        assert!(!is_doc(&root, "../outside.md", false));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_paths_inside_the_project_resolve() {
        let root = project(&[("docs/a.md", "")]);
        let outside = project_path(&root, root.join("..").to_str().unwrap());
        assert_eq!(outside, None);
        assert_eq!(
            project_path(&root, root.join("docs/a.md").to_str().unwrap()).as_deref(),
            Some("docs/a.md")
        );
        assert_eq!(
            project_path(&root, "docs/../docs/a.md").as_deref(),
            Some("docs/a.md")
        );
        assert_eq!(project_path(&root, "docs/missing.md"), None);
        resolve(&root, "docs/new.md").expect("a new doc resolves");
        resolve(&root, "new/deeper/file.md").expect("a doc in new folders resolves");
        for refused in [
            "",
            "../a.md",
            "docs/../../a.md",
            "/etc/a.md",
            "C:/a.md",
            "a\\b.md",
        ] {
            assert!(resolve(&root, refused).is_err(), "{refused}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn links_join_relative_to_the_linking_doc() {
        assert_eq!(join("docs/a.md", "b.md").as_deref(), Some("docs/b.md"));
        assert_eq!(
            join("docs/a.md", "../img/x.png").as_deref(),
            Some("img/x.png")
        );
        assert_eq!(
            join("docs/a.md", "./c/d.md").as_deref(),
            Some("docs/c/d.md")
        );
        assert_eq!(join("docs/a.md", "/top.md").as_deref(), Some("top.md"));
        assert_eq!(join("a.md", "../escape.md"), None);
        assert_eq!(
            join("docs/a.md", "Notes%202.yaml").as_deref(),
            Some("docs/Notes 2.yaml")
        );
        assert_eq!(join("a.md", "%2E%2E/escape.md"), None);
        assert_eq!(join("a.md", "100%.md").as_deref(), Some("100%.md"));
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_does_not_resolve() {
        let root = project(&[("docs/a.md", "")]);
        std::os::unix::fs::symlink("/tmp/adeline-missing-target.md", root.join("docs/link.md"))
            .unwrap();
        std::os::unix::fs::symlink("/tmp/adeline-missing-folder", root.join("gone")).unwrap();
        assert!(resolve(&root, "docs/link.md").is_err());
        assert!(resolve(&root, "gone/new.md").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn names_that_would_break_or_overwrite_something_are_refused() {
        for good in ["plan.md", "Notes 2.yaml", "a.b.toml", "README.MARKDOWN"] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in [
            "", "  ", "a/b.md", "a\\b.md", "x:y.md", "what?.md", "end.md.", " lead.md", "con.md",
            "COM1.txt", "notes", "main.rs",
        ] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }
}
