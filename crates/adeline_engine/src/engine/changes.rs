//! Changes for clients: a project's uncommitted changes against `HEAD`, read
//! with the `git` command line on the engine's machine, a file's patch or
//! text, and fresh changes for the clients watching a project.

use super::{Engine, Input};
use adeline_core::protocol::{Change, ChangeFile, ChangeStatus, Changes, Command, Delta, Repo};
use serde_json::Value;
use std::{
    fmt::Write as _,
    io,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};

const LIMIT: usize = 2 * 1024 * 1024;
const BINARY_PROBE: usize = 8000;

#[expect(
    clippy::disallowed_methods,
    reason = "runs on a blocking engine thread, never the UI thread"
)]
fn git(root: &Path, args: &[&str]) -> io::Result<Output> {
    let mut command = std::process::Command::new("git");
    command
        .args(["-c", "core.quotepath=false", "--literal-pathspecs"])
        .args(args)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
    };
    command.output()
}

fn run(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = git(root, args).map_err(|e| format!("git: {e}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn base(root: &Path) -> Result<Option<String>, String> {
    if !root.is_dir() {
        return Err(format!("The folder {} no longer exists.", root.display()));
    }
    match git(root, &["rev-parse", "--is-inside-work-tree"]) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("git: {error}")),
        Ok(output) if !output.status.success() => return Ok(Some(String::new())),
        Ok(_) => {}
    }
    if git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok_and(|o| o.status.success()) {
        return Ok(Some("HEAD".into()));
    }
    let empty = run(root, &["hash-object", "-t", "tree", "--stdin"])?;
    Ok(Some(String::from_utf8_lossy(&empty).trim().to_owned()))
}

fn fields(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect()
}

fn diff_args<'a>(base: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "diff",
        base,
        "-M",
        "--relative",
        "--no-ext-diff",
        "--no-color",
    ];
    args.extend_from_slice(extra);
    args
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_PROBE)].contains(&0)
}

fn read_capped(path: &Path) -> io::Result<Option<Vec<u8>>> {
    if std::fs::metadata(path)?.len() > LIMIT as u64 {
        return Ok(None);
    }
    std::fs::read(path).map(Some)
}

fn statuses(root: &Path, base: &str) -> Result<Vec<Change>, String> {
    let statuses = fields(&run(root, &diff_args(base, &["--name-status", "-z"]))?);
    let mut changes = Vec::new();
    let mut at = 0;
    while at < statuses.len() {
        let letter = statuses[at].chars().next().unwrap_or('M');
        let renamed = matches!(letter, 'R' | 'C');
        let (old_path, path) = if renamed {
            let old = statuses.get(at + 1).cloned();
            (old, statuses.get(at + 2).cloned().unwrap_or_default())
        } else {
            (None, statuses.get(at + 1).cloned().unwrap_or_default())
        };
        at += if renamed { 3 } else { 2 };
        changes.push(Change {
            path,
            old_path: old_path.filter(|_| letter == 'R'),
            status: match letter {
                'A' | 'C' => ChangeStatus::Added,
                'D' => ChangeStatus::Deleted,
                'R' => ChangeStatus::Renamed,
                _ => ChangeStatus::Modified,
            },
            added: 0,
            removed: 0,
        });
    }
    Ok(changes)
}

fn tracked(root: &Path, base: &str) -> Result<Vec<Change>, String> {
    let mut changes = statuses(root, base)?;
    let counts = fields(&run(root, &diff_args(base, &["--numstat", "-z"]))?);
    let mut at = 0;
    while at < counts.len() {
        let mut parts = counts[at].splitn(3, '\t');
        let added = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let removed = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let path = match parts.next() {
            Some(path) if !path.is_empty() => {
                at += 1;
                path.to_owned()
            }
            _ => {
                at += 3;
                counts.get(at - 1).cloned().unwrap_or_default()
            }
        };
        if let Some(change) = changes.iter_mut().find(|change| change.path == path) {
            change.added = added;
            change.removed = removed;
        }
    }
    Ok(changes)
}

fn untracked(root: &Path) -> Result<Vec<Change>, String> {
    let paths = fields(&run(
        root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?);
    Ok(paths
        .into_iter()
        .filter(|path| !path.ends_with('/'))
        .map(|path| {
            let added = match read_capped(&root.join(&path)) {
                Ok(Some(bytes)) if !is_binary(&bytes) => {
                    String::from_utf8_lossy(&bytes).lines().count()
                }
                _ => 0,
            };
            Change {
                path,
                old_path: None,
                status: ChangeStatus::Untracked,
                added,
                removed: 0,
            }
        })
        .collect())
}

pub(super) fn changes(root: &Path) -> Result<Changes, String> {
    let base = match base(root)? {
        None => {
            return Ok(Changes {
                repo: Repo::NoGit,
                changes: Vec::new(),
            });
        }
        Some(base) if base.is_empty() => {
            return Ok(Changes {
                repo: Repo::NotGit,
                changes: Vec::new(),
            });
        }
        Some(base) => base,
    };
    let mut changes = tracked(root, &base)?;
    changes.extend(untracked(root)?);
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Changes {
        repo: Repo::Git,
        changes,
    })
}

fn added_patch(path: &str, text: &str) -> String {
    let mut patch = format!("diff --git a/{path} b/{path}\nnew file mode 100644\n");
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return patch;
    }
    let _ = write!(
        patch,
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        patch.push('+');
        patch.push_str(line.strip_suffix('\r').unwrap_or(line));
        patch.push('\n');
    }
    if !text.ends_with('\n') {
        patch.push_str("\\ No newline at end of file\n");
    }
    patch
}

fn too_large() -> ChangeFile {
    ChangeFile {
        too_large: true,
        ..ChangeFile::default()
    }
}

fn binary() -> ChangeFile {
    ChangeFile {
        binary: true,
        ..ChangeFile::default()
    }
}

pub(super) fn read(root: &Path, path: &str) -> Result<ChangeFile, String> {
    let full = adeline_docs::resolve(root, path)?;
    let base = base(root)?.filter(|base| !base.is_empty());
    let change = match &base {
        Some(base) => statuses(root, base)?
            .into_iter()
            .find(|change| change.path == path),
        None => None,
    };
    let (Some(base), Some(change)) = (&base, &change) else {
        let Some(bytes) = read_capped(&full).map_err(|e| super::docs::io_error(path, &e))? else {
            return Ok(too_large());
        };
        if is_binary(&bytes) {
            return Ok(binary());
        }
        let others = [
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            path,
        ];
        if base.is_some() && !run(root, &others)?.is_empty() {
            return Ok(ChangeFile {
                patch: Some(added_patch(path, &String::from_utf8_lossy(&bytes))),
                ..ChangeFile::default()
            });
        }
        let (text, _) = adeline_docs::decode(bytes);
        return Ok(ChangeFile {
            text: Some(text),
            ..ChangeFile::default()
        });
    };
    let mut args = diff_args(base, &["--unified=1000000", "--"]);
    args.extend(change.old_path.as_deref());
    args.push(path);
    let patch = run(root, &args)?;
    if patch.len() > LIMIT {
        return Ok(too_large());
    }
    let patch = String::from_utf8_lossy(&patch).into_owned();
    if !patch.contains("\n@@ ")
        && (patch.contains("\nBinary files ") || patch.contains("\nGIT binary patch"))
    {
        return Ok(binary());
    }
    Ok(ChangeFile {
        patch: Some(patch),
        ..ChangeFile::default()
    })
}

impl Engine {
    fn reply_later(
        &self,
        client: u64,
        request: u64,
        work: impl FnOnce() -> Result<Value, String> + Send + 'static,
    ) {
        let input = self.input.clone();
        tokio::task::spawn_blocking(move || {
            let _ = input.send(Input::Done {
                client,
                request,
                result: work(),
                detect: false,
            });
        });
    }

    pub(super) fn changes_command(
        &mut self,
        client: u64,
        request: u64,
        command: Command,
    ) -> Option<Result<Value, String>> {
        let respect = self.settings.docs.respect_gitignore;
        let root = match &command {
            Command::ListFiles { project_id }
            | Command::ListChanges { project_id }
            | Command::ReadChange { project_id, .. } => match self.project_root(project_id) {
                Ok(root) => root,
                Err(error) => return Some(Err(error)),
            },
            _ => PathBuf::new(),
        };
        let work: Box<dyn FnOnce() -> Result<Value, String> + Send> = match command {
            Command::ListFiles { .. } => {
                Box::new(move || super::docs::json(adeline_docs::list_files(&root, respect)))
            }
            Command::ListChanges { .. } => Box::new(move || super::docs::json(changes(&root)?)),
            Command::ReadChange { path, .. } => {
                Box::new(move || super::docs::json(read(&root, &path)?))
            }
            Command::WatchChanges { project_id } => {
                match project_id {
                    Some(id) => self.changes_watching.insert(client, id),
                    None => self.changes_watching.remove(&client),
                };
                self.watch_docs();
                return Some(Ok(Value::Null));
            }
            _ => unreachable!("changes_command only gets changes commands"),
        };
        self.reply_later(client, request, work);
        None
    }

    pub(super) fn changes_forget_unwatched(&mut self) {
        let watched: std::collections::HashSet<&String> = self.changes_watching.values().collect();
        self.changes_files.retain(|id, _| watched.contains(id));
        for (id, generation) in &mut self.changes_generation {
            if !watched.contains(id) {
                *generation += 1;
            }
        }
    }

    pub(super) fn changes_settled(&mut self, paths: &[PathBuf]) {
        let watched: std::collections::HashSet<String> =
            self.changes_watching.values().cloned().collect();
        let respect = self.settings.docs.respect_gitignore;
        for project_id in watched {
            let Ok(root) = self.project_root(&project_id) else {
                continue;
            };
            let real = root.canonicalize().unwrap_or_else(|_| root.clone());
            let mut folders = std::collections::HashSet::new();
            let touched = tokio::task::block_in_place(|| {
                paths.iter().any(|path| {
                    let Ok(inside) = path
                        .strip_prefix(&root)
                        .or_else(|_| path.strip_prefix(&real))
                    else {
                        return false;
                    };
                    let folder = inside.parent().unwrap_or(Path::new(""));
                    !respect
                        || folders.insert(folder.to_path_buf())
                            && !adeline_docs::ignored_folder(&root, folder)
                })
            });
            if !touched {
                continue;
            }
            let generation = self
                .changes_generation
                .entry(project_id.clone())
                .or_default();
            *generation += 1;
            let generation = *generation;
            let input = self.input.clone();
            tokio::task::spawn_blocking(move || {
                let found = changes(&root).map(|changes| {
                    let files = adeline_docs::list_files(&root, respect);
                    (changes, files)
                });
                let _ = input.send(Input::Changes {
                    project_id,
                    generation,
                    found,
                });
            });
        }
    }

    pub(super) fn changes_found(
        &mut self,
        project_id: String,
        generation: u64,
        found: Result<(Changes, Vec<String>), String>,
    ) {
        if self.changes_generation.get(&project_id) != Some(&generation) {
            return;
        }
        let Ok((changes, files)) = found else {
            return;
        };
        let same_files = self.changes_files.get(&project_id) == Some(&files);
        if !same_files {
            self.changes_files.insert(project_id.clone(), files.clone());
        }
        self.broadcast(Delta::Changes {
            project_id,
            changes,
            files: (!same_files).then_some(files),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adeline_core::files::TempDir;

    fn sh(root: &Path, args: &[&str]) {
        let output = git(root, args).unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }

    fn repo(name: &str) -> TempDir {
        let root = TempDir::new(name);
        sh(&root, &["init", "-q"]);
        sh(&root, &["config", "user.email", "test@example.com"]);
        sh(&root, &["config", "user.name", "Test"]);
        sh(&root, &["config", "core.autocrlf", "false"]);
        root
    }

    fn status(changes: &Changes, path: &str) -> (ChangeStatus, usize, usize) {
        let change = changes
            .changes
            .iter()
            .find(|change| change.path == path)
            .unwrap_or_else(|| panic!("{path} in {changes:?}"));
        (change.status, change.added, change.removed)
    }

    #[test]
    fn staged_unstaged_untracked_and_renamed_files_are_changes() {
        let root = repo("adeline-changes-kinds");
        std::fs::write(root.join("staged.txt"), "a\nb\n").unwrap();
        std::fs::write(root.join("edited.txt"), "a\nb\n").unwrap();
        std::fs::write(root.join("gone.txt"), "x\n").unwrap();
        std::fs::write(root.join("old.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        sh(&root, &["add", "."]);
        sh(&root, &["commit", "-qm", "first"]);
        std::fs::write(root.join("staged.txt"), "a\nB\n").unwrap();
        sh(&root, &["add", "staged.txt"]);
        std::fs::write(root.join("edited.txt"), "a\nb\nc\n").unwrap();
        std::fs::write(root.join("new.txt"), "1\n2\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        sh(&root, &["mv", "old.txt", "renamed.txt"]);
        let found = changes(&root).unwrap();
        assert_eq!(found.repo, Repo::Git);
        assert_eq!(status(&found, "staged.txt"), (ChangeStatus::Modified, 1, 1));
        assert_eq!(status(&found, "edited.txt"), (ChangeStatus::Modified, 1, 0));
        assert_eq!(status(&found, "new.txt"), (ChangeStatus::Untracked, 2, 0));
        assert_eq!(status(&found, "gone.txt"), (ChangeStatus::Deleted, 0, 1));
        assert_eq!(status(&found, "renamed.txt").0, ChangeStatus::Renamed);
        let renamed = found.changes.iter().find(|c| c.path == "renamed.txt");
        assert_eq!(renamed.and_then(|c| c.old_path.as_deref()), Some("old.txt"));

        let patch = read(&root, "edited.txt").unwrap().patch.unwrap();
        assert!(patch.contains("+c"), "{patch}");
        let added = read(&root, "new.txt").unwrap().patch.unwrap();
        assert!(added.contains("+1\n+2\n"), "{added}");
        let deleted = read(&root, "gone.txt").unwrap().patch.unwrap();
        assert!(deleted.contains("-x"), "{deleted}");
        sh(&root, &["commit", "-qam", "second"]);
        let file = read(&root, "staged.txt").unwrap();
        assert_eq!(file.text.as_deref(), Some("a\nB\n"));
    }

    #[test]
    fn a_repository_without_commits_shows_every_file_as_added() {
        let root = repo("adeline-changes-empty");
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        sh(&root, &["add", "a.txt"]);
        let found = changes(&root).unwrap();
        assert_eq!(status(&found, "a.txt"), (ChangeStatus::Added, 1, 0));
        assert_eq!(status(&found, "b.txt"), (ChangeStatus::Untracked, 1, 0));
    }

    #[test]
    fn a_folder_outside_git_says_so_and_binary_files_have_no_patch() {
        let plain = TempDir::new("adeline-changes-plain");
        assert_eq!(changes(&plain).unwrap().repo, Repo::NotGit);
        let gone = changes(&plain.join("gone")).unwrap_err();
        assert!(gone.contains("no longer exists"), "{gone}");
        let root = repo("adeline-changes-binary");
        std::fs::write(root.join("big.txt"), vec![b'a'; LIMIT + 1]).unwrap();
        assert!(read(&root, "big.txt").unwrap().too_large);
        assert_eq!(status(&changes(&root).unwrap(), "big.txt").1, 0);
        std::fs::remove_file(root.join("big.txt")).unwrap();
        std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 0]).unwrap();
        assert!(read(&root, "blob.bin").unwrap().binary);
        sh(&root, &["add", "."]);
        sh(&root, &["commit", "-qm", "blob"]);
        std::fs::write(root.join("blob.bin"), [0u8, 9, 9, 0]).unwrap();
        let file = read(&root, "blob.bin").unwrap();
        assert!(file.binary && file.patch.is_none(), "{file:?}");
    }
}
