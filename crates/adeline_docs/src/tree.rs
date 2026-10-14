//! The docs tree: the docs and the folders that hold them, folders first and
//! each level sorted alphabetically, and its fuzzy search by path.

// Rust guideline compliant 2026-10-09

use nucleo_matcher::{
    Config, Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<String>,
}

/// One visible line of the tree: a folder or a doc, by its project path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub depth: usize,
    pub folder: bool,
    pub open: bool,
}

#[derive(Debug, Default)]
pub struct Tree {
    root: Folder,
    paths: Vec<String>,
}

fn sort_key(name: &str) -> (String, &str) {
    (name.to_lowercase(), name)
}

impl Folder {
    fn rows(&self, prefix: &str, depth: usize, open: &dyn Fn(&str) -> bool, rows: &mut Vec<Row>) {
        let mut folders: Vec<_> = self.folders.iter().collect();
        folders.sort_by(|(a, _), (b, _)| sort_key(a).cmp(&sort_key(b)));
        for (name, folder) in folders {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let expanded = open(&path);
            rows.push(Row {
                path: path.clone(),
                depth,
                folder: true,
                open: expanded,
            });
            if expanded {
                folder.rows(&path, depth + 1, open, rows);
            }
        }
        let mut files: Vec<_> = self.files.iter().collect();
        files.sort_by(|a, b| sort_key(crate::file_name(a)).cmp(&sort_key(crate::file_name(b))));
        rows.extend(files.into_iter().map(|path| Row {
            path: path.clone(),
            depth,
            folder: false,
            open: false,
        }));
    }
}

impl Tree {
    pub fn new(paths: Vec<String>) -> Self {
        let mut root = Folder::default();
        for path in &paths {
            let mut folder = &mut root;
            for part in crate::parent(path).split('/').filter(|p| !p.is_empty()) {
                folder = folder.folders.entry(part.to_owned()).or_default();
            }
            folder.files.push(path.clone());
        }
        Self { root, paths }
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// The visible rows, with the folders in `expanded` open.
    pub fn rows(&self, expanded: &HashSet<String>) -> Vec<Row> {
        let mut rows = Vec::new();
        self.root
            .rows("", 0, &|path| expanded.contains(path), &mut rows);
        rows
    }

    /// The docs whose path fuzzily matches `query`, with every folder that holds one open.
    pub fn search(&self, query: &str) -> Vec<Row> {
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let found = pattern
            .match_list(self.paths.iter(), &mut matcher)
            .into_iter()
            .map(|(path, _)| path.clone())
            .collect();
        let mut rows = Vec::new();
        Self::new(found).root.rows("", 0, &|_| true, &mut rows);
        rows
    }

    /// Every folder that holds a doc, sorted.
    pub fn folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = self
            .paths
            .iter()
            .flat_map(|path| {
                let parent = crate::parent(path);
                parent
                    .match_indices('/')
                    .map(|(at, _)| &parent[..at])
                    .chain((!parent.is_empty()).then_some(parent))
            })
            .map(str::to_owned)
            .collect();
        folders.sort();
        folders.dedup();
        folders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|&p| p.to_owned()).collect()
    }

    fn shown(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| {
                let mark = if row.folder { "/" } else { "" };
                format!(
                    "{}{}{mark}",
                    "  ".repeat(row.depth),
                    crate::file_name(&row.path)
                )
            })
            .collect()
    }

    #[test]
    fn folders_come_first_and_each_level_is_alphabetical() {
        let tree = Tree::new(paths(&[
            "zeta.md",
            "Alpha.md",
            "docs/b.md",
            "docs/archive/old.md",
            "docs/A.md",
            "beta/x.toml",
        ]));
        let expanded: HashSet<String> = ["docs".to_owned()].into();
        assert_eq!(
            shown(&tree.rows(&expanded)),
            [
                "beta/",
                "docs/",
                "  archive/",
                "  A.md",
                "  b.md",
                "Alpha.md",
                "zeta.md"
            ]
        );
        assert_eq!(shown(&tree.rows(&HashSet::new()))[..2], ["beta/", "docs/"]);
        assert_eq!(tree.folders(), ["beta", "docs", "docs/archive"]);
    }

    #[test]
    fn search_finds_abbreviations_and_opens_their_folders() {
        let tree = Tree::new(paths(&[
            "docs/scope-docs-mode.md",
            "docs/shortcuts.md",
            "README.md",
            "notes/plan.md",
        ]));
        let found = tree.search("scpdoc");
        assert_eq!(shown(&found), ["docs/", "  scope-docs-mode.md"]);
        assert!(found[0].open);
        assert!(tree.search("zzzz").is_empty());
        assert_eq!(tree.search("").len(), tree.rows(&tree_folders(&tree)).len());
    }

    fn tree_folders(tree: &Tree) -> HashSet<String> {
        tree.folders().into_iter().collect()
    }

    #[test]
    fn ten_thousand_docs_filter_quickly() {
        let many: Vec<String> = (0..10_000)
            .map(|i| format!("area{}/topic{}/note-{i}.md", i % 50, i % 7))
            .collect();
        let tree = Tree::new(many);
        let started = std::time::Instant::now();
        for query in ["n", "no", "not", "note", "note-9", "area4/note-99"] {
            assert!(!tree.search(query).is_empty());
        }
        // Six keystrokes, well under a frame each on any machine that runs tests.
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
