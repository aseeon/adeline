//! Harnesses Adeline can run: the ACP registry plus built-in OMP, where their
//! executables are installed and in which version, and whether Node.js is there.
#[cfg(feature = "gui")]
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
pub const OMP: &str = "omp";
pub const CUSTOM: &str = "custom";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Harness {
    pub id: String,
    pub name: String,
    pub website: String,
    /// Executable names to look for, in order.
    executables: Vec<String>,
    /// The npm or uv package the executables must come from.
    #[serde(default)]
    package: Option<String>,
    pub arguments: Vec<String>,
    /// Environment variables the registry sets for the agent.
    #[serde(default)]
    pub environment: Vec<(String, String)>,
    /// The registry's current version, for update offers (scope R12).
    #[serde(default)]
    pub version: String,
}

fn omp() -> Harness {
    Harness {
        id: OMP.into(),
        name: "OMP".into(),
        website: "https://github.com/can1357/oh-my-pi".into(),
        executables: vec![crate::profiles::OMP.command.into()],
        package: None,
        arguments: crate::profiles::OMP
            .arguments
            .iter()
            .map(|&arg| arg.to_owned())
            .collect(),
        environment: Vec::new(),
        version: String::new(),
    }
}

/// Supported agents launch the way their profile says, whatever the
/// registry guesses from package names.
fn with_profile(mut harness: Harness) -> Harness {
    if let Some(profile) = crate::profiles::find(&harness.id, "") {
        harness.executables = vec![profile.command.to_owned()];
        harness.arguments = profile.arguments.iter().map(|&a| a.to_owned()).collect();
        if let Some(package) = profile.install.package {
            harness.package = Some(package.to_owned());
        }
    }
    harness
}

/// Whether Node.js is there for npm installs, and which package manager
/// could install it (scope R11).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub npm: Option<PathBuf>,
    /// `winget` on Windows or `brew` on macOS, when found.
    pub manager: Option<PathBuf>,
}

/// Every known harness and where each installed one lives. The engine
/// detects them; clients get a copy.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub harnesses: Vec<Harness>,
    pub installed: HashMap<String, PathBuf>,
    /// The installed version of npm-installed agents.
    #[serde(default)]
    pub versions: HashMap<String, String>,
    #[serde(default)]
    pub node: Node,
    /// Detection has finished at least once.
    pub detected: bool,
    #[serde(skip)]
    pub demo: bool,
}

#[cfg(feature = "gui")]
impl Global for Catalog {}

impl Catalog {
    pub fn demo(&self) -> bool {
        self.demo
    }

    pub fn get(&self, id: &str) -> Option<&Harness> {
        self.harnesses.iter().find(|harness| harness.id == id)
    }

    /// Installed harnesses first, each group by name.
    pub fn sorted(&self) -> Vec<&Harness> {
        let mut list: Vec<_> = self.harnesses.iter().collect();
        list.sort_by_cached_key(|harness| {
            (
                !self.installed.contains_key(&harness.id),
                harness.name.to_lowercase(),
            )
        });
        list
    }

    /// `None` while detection has not answered yet.
    pub fn is_installed(&self, id: &str) -> Option<bool> {
        if id == CUSTOM {
            return Some(true);
        }
        (self.detected || self.installed.contains_key(id)).then(|| self.installed.contains_key(id))
    }

    pub fn label(&self, id: &str, identity: &str) -> String {
        if id == CUSTOM {
            if identity.is_empty() {
                "Custom".into()
            } else {
                format!("Custom: {identity}")
            }
        } else {
            self.get(id)
                .map_or_else(|| id.to_owned(), |h| h.name.clone())
        }
    }

    /// The executable and arguments a new conversation starts with.
    pub fn launch(
        &self,
        id: &str,
        command: &str,
        arguments: &[String],
    ) -> Result<crate::storage::Launch, String> {
        if id == CUSTOM {
            let path = resolve(command)
                .ok_or_else(|| format!("Command {command} was not found on this machine."))?;
            return Ok(crate::storage::Launch {
                command: path.to_string_lossy().into_owned(),
                arguments: arguments.to_vec(),
                environment: Vec::new(),
            });
        }
        let harness = self
            .get(id)
            .ok_or_else(|| format!("Harness {id} is not in the harness list."))?;
        let path = self
            .installed
            .get(id)
            .cloned()
            .or_else(|| locate(harness))
            .ok_or_else(|| format!("{} is not installed.", harness.name))?;
        Ok(crate::storage::Launch {
            command: path.to_string_lossy().into_owned(),
            arguments: harness.arguments.clone(),
            environment: harness.environment.clone(),
        })
    }

    /// The installed and the newer registry version, when an update exists.
    pub fn update(&self, id: &str) -> Option<(&str, &str)> {
        let installed = self.versions.get(id)?;
        let latest = self.get(id)?.version.as_str();
        newer(latest, installed).then_some((installed.as_str(), latest))
    }
}

/// Whether version `a` is newer than `b`, comparing dotted numbers.
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['.', '-', '+'])
            .map_while(|part| part.parse().ok())
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    !a.is_empty() && !b.is_empty() && a > b
}

/// The client's catalog until the engine sends its own. Demo mode, which has
/// no engine, lists the bundled harnesses as not installed.
#[cfg(feature = "gui")]
pub fn init(demo: bool, cx: &mut App) {
    cx.set_global(Catalog {
        harnesses: if demo { load() } else { Vec::new() },
        detected: demo,
        demo,
        ..Default::default()
    });
}

/// Finds installed harnesses, after fetching a fresh registry when `fetch`
/// is set. Blocking: the engine runs it on its blocking pool.
pub fn detect(fetch: bool, known: Vec<Harness>) -> Catalog {
    let harnesses = if fetch && download().is_ok() {
        load()
    } else {
        known
    };
    let dirs = search_path();
    let installed: HashMap<String, PathBuf> = harnesses
        .iter()
        .filter_map(|harness| Some((harness.id.clone(), locate(harness)?)))
        .collect();
    let versions = harnesses
        .iter()
        .filter(|harness| installed.contains_key(&harness.id))
        .filter_map(|harness| {
            let package = harness.package.as_deref()?;
            Some((harness.id.clone(), package_version(package, &dirs)?))
        })
        .collect();
    let node = Node {
        npm: find("npm", &dirs),
        manager: find(if cfg!(windows) { "winget" } else { "brew" }, &dirs)
            .filter(|_| cfg!(any(windows, target_os = "macos"))),
    };
    Catalog {
        harnesses,
        installed,
        versions,
        node,
        detected: true,
        demo: false,
    }
}

fn cache() -> Option<PathBuf> {
    crate::config::directory()
        .ok()
        .map(|path| path.join("cache"))
}

/// Icons the engine sent, by asset path.
static ICONS: std::sync::RwLock<BTreeMap<String, Vec<u8>>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Adds icons an engine sent. Every machine's engine adds its own.
pub fn set_icons(icons: &BTreeMap<String, String>) {
    if let Ok(mut map) = ICONS.write() {
        map.extend(
            icons
                .iter()
                .map(|(path, svg)| (path.clone(), svg.clone().into_bytes())),
        );
    }
}

/// A registry icon or agent avatar from the engine, for the asset loader.
pub fn icon(path: &str) -> Option<Vec<u8>> {
    ICONS.read().ok()?.get(path).cloned()
}

/// Whether the engine sent an icon for `path`, without copying it.
pub fn has_icon(path: &str) -> bool {
    ICONS.read().is_ok_and(|icons| icons.contains_key(path))
}

/// A harness's icon, read by the engine: bundled for OMP, else the cached registry icon.
pub fn icon_svg(id: &str) -> Option<Vec<u8>> {
    if id == OMP {
        crate::embedded("omp.svg").map(<[u8]>::to_vec)
    } else if id.contains(['/', '\\']) || id.contains("..") {
        None
    } else {
        fs::read(cache()?.join("registry-icons").join(format!("{id}.svg"))).ok()
    }
}

/// A harness's icon asset path: bundled for OMP, the registry icon
/// otherwise, else the generic robot.
pub fn icon_path(id: &str) -> String {
    let registry = format!("registry-icons/{id}.svg");
    if id == OMP {
        "omp.svg".into()
    } else if has_icon(&registry) {
        registry
    } else {
        "robot.svg".into()
    }
}

/// The cached registry, else the bundled copy, else OMP alone.
pub fn load() -> Vec<Harness> {
    let cached = cache()
        .and_then(|cache| fs::read_to_string(cache.join("registry.json")).ok())
        .and_then(|text| parse_registry(&text).ok());
    let bundled = || {
        crate::embedded("registry.json")
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|text| parse_registry(text).ok())
    };
    let mut harnesses: Vec<_> = cached
        .or_else(bundled)
        .unwrap_or_default()
        .into_iter()
        .map(with_profile)
        .collect();
    if !harnesses.iter().any(|harness| harness.id == OMP) {
        harnesses.push(omp());
    }
    harnesses
}

#[expect(
    clippy::disallowed_methods,
    reason = "Runs on the engine's blocking pool, never on the UI thread"
)]
fn download() -> Result<(), String> {
    let cache = cache().ok_or("No configuration directory.")?;
    fs::create_dir_all(cache.join("registry-icons")).map_err(|e| e.to_string())?;
    let partial = cache.join(crate::files::unique("registry.json"));
    let status = hidden(Command::new("curl"))
        .args(["-fsSL", "--max-time", "20", "-o"])
        .arg(&partial)
        .arg(REGISTRY_URL)
        .status()
        .map_err(|e| e.to_string())?;
    let text = fs::read_to_string(&partial).unwrap_or_default();
    let parsed = status
        .success()
        .then(|| serde_json::from_str::<Value>(&text).ok())
        .flatten();
    let Some(registry) = parsed.filter(|_| parse_registry(&text).is_ok()) else {
        let _ = fs::remove_file(&partial);
        return Err("Registry download failed.".into());
    };
    fs::rename(&partial, cache.join("registry.json")).map_err(|e| e.to_string())?;
    let mut icons = hidden(Command::new("curl"));
    icons.args(["-fsSL", "--max-time", "30"]);
    for agent in registry["agents"].as_array().into_iter().flatten() {
        if let (Some(id), Some(url)) = (agent["id"].as_str(), agent["icon"].as_str())
            && !id.contains(['/', '\\', '.'])
            && url.starts_with("https://")
        {
            icons
                .arg("-o")
                .arg(cache.join("registry-icons").join(format!("{id}.svg")))
                .arg(url);
        }
    }
    // Icons are decoration; a missing one falls back to the generic robot.
    let _ = icons.status();
    Ok(())
}

fn hidden(mut command: Command) -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "cmd",
    "powershell",
    "pwsh",
    "wscript",
    "cscript",
    "mshta",
    "rundll32",
    "env",
    "python",
    "python3",
    "py",
    "node",
    "deno",
    "bun",
    "perl",
    "ruby",
    "php",
    "osascript",
];

fn parse_registry(text: &str) -> Result<Vec<Harness>, String> {
    let registry: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let agents = registry["agents"]
        .as_array()
        .ok_or("Registry has no agents.")?;
    let platform = format!(
        "{}-{}",
        match std::env::consts::OS {
            "macos" => "darwin",
            os => os,
        },
        std::env::consts::ARCH
    );
    let mut harnesses = Vec::new();
    for agent in agents {
        let (Some(id), Some(name)) = (agent["id"].as_str(), agent["name"].as_str()) else {
            continue;
        };
        // The registry must not replace Adeline's own entries.
        if [OMP, CUSTOM].contains(&id) {
            continue;
        }
        let distribution = &agent["distribution"];
        let mut executables = Vec::new();
        let mut arguments = Vec::new();
        let mut environment = Vec::new();
        let mut source = None;
        if let Some(binaries) = distribution["binary"].as_object() {
            let binary = binaries
                .get(&platform)
                .or_else(|| binaries.values().next())
                .unwrap_or(&Value::Null);
            if let Some(cmd) = binary["cmd"].as_str() {
                let file = cmd.rsplit(['/', '\\']).next().unwrap_or(cmd);
                executables.push(file.strip_suffix(".exe").unwrap_or(file).to_owned());
            }
            arguments = strings(&binary["args"]);
            environment = pairs(&binary["env"]);
        }
        for runner in ["npx", "uvx"] {
            let Some(package) = distribution[runner]["package"].as_str() else {
                continue;
            };
            // `@scope/name@1.2.3` installs a `name` executable, sometimes without `-cli`.
            let unversioned = package
                .get(1..)
                .and_then(|rest| rest.find('@'))
                .map_or(package, |at| &package[..=at]);
            let unversioned = unversioned
                .split_once("==")
                .map_or(unversioned, |(name, _)| name);
            source = Some(unversioned.to_owned());
            let base = unversioned.rsplit('/').next().unwrap_or(unversioned);
            executables.push(base.to_owned());
            if let Some(short) = base.strip_suffix("-cli") {
                executables.push(short.to_owned());
            }
            if arguments.is_empty() {
                arguments = strings(&distribution[runner]["args"]);
            }
            if environment.is_empty() {
                environment = pairs(&distribution[runner]["env"]);
            }
        }
        executables.push(id.to_owned());
        executables.dedup();
        // A general interpreter would run whatever arguments the registry supplies.
        executables.retain(|name| {
            !INTERPRETERS
                .iter()
                .any(|shell| name.eq_ignore_ascii_case(shell))
        });
        let link = |key: &str| agent[key].as_str().unwrap_or_default().to_owned();
        harnesses.push(Harness {
            id: id.to_owned(),
            name: name.to_owned(),
            website: Some(link("website"))
                .filter(|url| !url.is_empty())
                .unwrap_or_else(|| link("repository")),
            executables,
            package: source,
            arguments,
            environment,
            version: agent["version"].as_str().unwrap_or_default().to_owned(),
        });
    }
    Ok(harnesses)
}

fn pairs(value: &Value) -> Vec<(String, String)> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
        .collect()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn search_path() -> Vec<PathBuf> {
    let mut dirs: Vec<_> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    // Default install locations that are often missing from PATH.
    for (variable, folder) in [
        ("LOCALAPPDATA", "omp"),
        ("APPDATA", "npm"),
        ("USERPROFILE", ".local/bin"),
        ("USERPROFILE", ".bun/bin"),
        ("USERPROFILE", ".cargo/bin"),
        ("HOME", ".local/bin"),
        ("HOME", ".bun/bin"),
        ("HOME", ".cargo/bin"),
        ("HOME", ".opencode/bin"),
        ("ProgramFiles", "nodejs"),
        ("LOCALAPPDATA", "Microsoft/WindowsApps"),
    ] {
        if let Some(base) = std::env::var_os(variable) {
            dirs.push(PathBuf::from(base).join(folder));
        }
    }
    // Homebrew, which an app started from Finder has no PATH entry for.
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    dirs
}

/// A program found on PATH or in the usual install folders.
pub fn program(name: &str) -> Option<PathBuf> {
    find(name, &search_path())
}

/// The installed version of an npm package, from its `package.json`.
fn package_version(package: &str, dirs: &[PathBuf]) -> Option<String> {
    dirs.iter().find_map(|dir| {
        [dir.join("node_modules"), dir.join("../lib/node_modules")]
            .iter()
            .find_map(|modules| {
                let text = fs::read(modules.join(package).join("package.json")).ok()?;
                let manifest: Value = serde_json::from_slice(&text).ok()?;
                manifest["version"].as_str().map(str::to_owned)
            })
    })
}

fn find(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    // npm also writes an extensionless shell script on Windows; only these run.
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ".com"]
    } else {
        &[""]
    };
    dirs.iter().find_map(|dir| {
        extensions
            .iter()
            .map(|extension| dir.join(format!("{name}{extension}")))
            .find(|path| path.is_file())
    })
}

fn locate(harness: &Harness) -> Option<PathBuf> {
    let dirs = search_path();
    if let Some(path) = harness
        .package
        .as_deref()
        .and_then(|package| from_package(package, &dirs))
    {
        return Some(path);
    }
    harness
        .executables
        .iter()
        .filter_map(|name| find(name, &dirs))
        .find(|path| {
            harness
                .package
                .as_deref()
                .is_none_or(|package| comes_from(path, package))
        })
}

/// The executable an installed `package` provides, read from what npm and uv
/// record: npm's `package.json` next to its shims, uv's tool receipt.
fn from_package(package: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let unscoped = package.rsplit('/').next().unwrap_or(package);
    let uv_name = package.to_lowercase().replace(['_', '.'], "-");
    for tools in uv_tool_dirs() {
        let Ok(receipt) = fs::read_to_string(tools.join(&uv_name).join("uv-receipt.toml")) else {
            continue;
        };
        let paths: Vec<PathBuf> = receipt
            .split("install-path = \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(PathBuf::from)
            .collect();
        let names: Vec<String> = paths
            .iter()
            .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
            .collect();
        if let Some(name) = pick(&names, &uv_name) {
            return paths
                .into_iter()
                .find(|path| {
                    path.file_stem()
                        .is_some_and(|stem| stem.to_string_lossy() == name)
                })
                .filter(|path| path.is_file());
        }
    }
    // Windows npm keeps packages in `<shims>/node_modules`, Unix in `<prefix>/lib/node_modules`.
    dirs.iter().find_map(|dir| {
        [dir.join("node_modules"), dir.join("../lib/node_modules")]
            .iter()
            .find_map(|modules| {
                let text = fs::read(modules.join(package).join("package.json")).ok()?;
                let manifest: Value = serde_json::from_slice(&text).ok()?;
                let names = match &manifest["bin"] {
                    Value::String(_) => vec![unscoped.to_owned()],
                    Value::Object(bins) => bins.keys().cloned().collect(),
                    _ => Vec::new(),
                };
                find(&pick(&names, unscoped)?, std::slice::from_ref(dir))
            })
    })
}

/// The executable `npx` or `uvx` would run: the one named after the package,
/// else the only one.
/// ponytail: else the shortest name, as in `mcode` over `mcode-tools`; let
/// the agent definition name it if a package ever needs another.
fn pick(names: &[String], package: &str) -> Option<String> {
    names
        .iter()
        .find(|name| *name == package)
        .or_else(|| names.iter().min_by_key(|name| name.len()))
        .cloned()
}

/// Where `uv tool install` puts its tools, by its documented defaults.
fn uv_tool_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("UV_TOOL_DIR")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data).join("uv/tools"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("uv/tools"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/uv/tools"));
    }
    dirs
}

/// Whether `path` runs `package`: it links into the package's folder, or is a
/// small shim or launcher that names it, as npm, bun and uv install them.
/// Names are guessed from the package, so `@minimax-ai/code` must not find VS Code's `code`.
fn comes_from(path: &Path, package: &str) -> bool {
    let needles: Vec<String> = ['/', '\\']
        .iter()
        .map(|sep| format!("{sep}{}{sep}", package.replace('/', &sep.to_string())))
        .collect();
    let mentions = |bytes: &[u8]| {
        needles
            .iter()
            .any(|needle| contains_ascii_bytes(bytes, needle.as_bytes()))
    };
    fs::canonicalize(path).is_ok_and(|real| mentions(real.as_os_str().as_encoded_bytes()))
        || fs::metadata(path).is_ok_and(|meta| meta.len() <= 4 << 20)
            && fs::read(path).is_ok_and(|bytes| mentions(&bytes))
}

fn contains_ascii_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// A Custom command as a path: itself when it names a file, else found on PATH.
pub fn resolve(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_file() {
        // Absolute, so the conversation's working directory cannot change its meaning.
        return std::path::absolute(path).ok();
    }
    if command.contains(['/', '\\']) || command.trim().is_empty() {
        return None;
    }
    let name = Path::new(command)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(command);
    let dirs = search_path();
    if Path::new(command).extension().is_some() {
        dirs.iter()
            .map(|dir| dir.join(command))
            .find(|path| path.is_file())
            .or_else(|| find(name, &dirs))
    } else {
        find(name, &dirs)
    }
}

/// The system guidance a supported harness receives.
pub fn guidance(name: &str, instructions: &str) -> String {
    let lead = format!(
        "You are an agent named {name}, running inside Adeline ADE (Agentic Development Environment)"
    );
    if instructions.trim().is_empty() {
        lead
    } else {
        format!("{lead}\n{instructions}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entries_become_local_launch_commands() {
        let harnesses = parse_registry(
            r#"{"agents":[
            {"id":"gemini","name":"Gemini CLI","repository":"https://github.com/g/gemini",
             "distribution":{"npx":{"package":"@google/gemini-cli@0.62.0","args":["--acp"]}}},
            {"id":"opencode","name":"OpenCode","website":"https://opencode.ai",
             "distribution":{"binary":{"windows-x86_64":{"cmd":"./opencode.exe","args":["acp"]},
                                       "linux-x86_64":{"cmd":"./opencode","args":["acp"]}}}},
            {"id":"codex-acp","name":"Codex",
             "distribution":{"npx":{"package":"@agentclientprotocol/codex-acp@2.1.1"}}}
        ]}"#,
        )
        .unwrap();
        assert_eq!(harnesses[0].executables, ["gemini-cli", "gemini"]);
        assert_eq!(harnesses[0].arguments, ["--acp"]);
        assert_eq!(harnesses[0].website, "https://github.com/g/gemini");
        assert_eq!(harnesses[1].executables, ["opencode"]);
        assert_eq!(harnesses[1].arguments, ["acp"]);
        assert_eq!(harnesses[2].executables, ["codex-acp"]);
        assert!(harnesses[2].arguments.is_empty());
        assert_eq!(
            harnesses[2].package.as_deref(),
            Some("@agentclientprotocol/codex-acp")
        );
        assert_eq!(harnesses[1].package, None);
        let hostile = parse_registry(
            r#"{"agents":[
            {"id":"omp","name":"Fake OMP","distribution":{}},
            {"id":"evil","name":"Evil","distribution":{"binary":{"x":{"cmd":"PowerShell.exe","args":["-c","x"]}}}}
        ]}"#,
        )
        .unwrap();
        assert_eq!(hostile.len(), 1);
        assert_eq!(hostile[0].executables, ["evil"]);
        assert!(resolve("Cargo.toml").unwrap().is_absolute());
        assert!(parse_registry("not json").is_err());
        let bundled = std::str::from_utf8(crate::embedded("registry.json").unwrap()).unwrap();
        assert!(parse_registry(bundled).unwrap().len() > 10);
    }

    #[test]
    fn guessed_executables_must_come_from_their_package() {
        let dir = std::env::temp_dir().join(format!("adeline-shims-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let vs_code = dir.join("code.cmd");
        fs::write(
            &vs_code,
            r#"@"%~dp0..\Code.exe" "%~dp0..\resources\app\out\cli.js" %*"#,
        )
        .unwrap();
        let npm = dir.join("mcode.cmd");
        fs::write(
            &npm,
            r#""%_prog%" "%dp0%\node_modules\@minimax-ai\code\cli.js" %*"#,
        )
        .unwrap();
        assert!(!comes_from(&vs_code, "@minimax-ai/code"));
        assert!(comes_from(&npm, "@minimax-ai/code"));
        assert!(!comes_from(&npm, "@minimax-ai/co"));
        let uv = parse_registry(
            r#"{"agents":[{"id":"fast-agent","name":"fast-agent",
             "distribution":{"uvx":{"package":"fast-agent-acp==0.10.1"}}}]}"#,
        )
        .unwrap();
        assert_eq!(uv[0].package.as_deref(), Some("fast-agent-acp"));
        assert_eq!(uv[0].executables, ["fast-agent-acp", "fast-agent"]);
    }

    #[test]
    fn package_metadata_names_the_executable() {
        let dir = std::env::temp_dir().join(format!("adeline-npm-{}", std::process::id()));
        let package = dir.join("node_modules/@minimax-ai/code");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            r#"{"bin":{"mcode":"cli.js","mcode-tools":"tools.js"}}"#,
        )
        .unwrap();
        let shim = dir.join(if cfg!(windows) { "mcode.cmd" } else { "mcode" });
        fs::write(&shim, "").unwrap();
        assert_eq!(from_package("@minimax-ai/code", &[dir]), Some(shim));
        let names = ["gcm".to_owned(), "fast-agent-acp".to_owned()];
        assert_eq!(
            pick(&names, "fast-agent-acp").as_deref(),
            Some("fast-agent-acp")
        );
        assert_eq!(pick(&names, "other").as_deref(), Some("gcm"));
        assert_eq!(pick(&[], "other"), None);
    }

    #[test]
    fn registry_env_version_and_profiles_shape_the_launch() {
        let harnesses: Vec<_> = parse_registry(
            r#"{"agents":[
            {"id":"claude-acp","name":"Claude Agent","version":"0.85.1",
             "distribution":{"npx":{"package":"@agentclientprotocol/claude-agent-acp@0.85.1","env":{"FOO":"1"}}}}
        ]}"#,
        )
        .unwrap()
        .into_iter()
        .map(with_profile)
        .collect();
        assert_eq!(harnesses[0].version, "0.85.1");
        assert_eq!(
            harnesses[0].environment,
            [("FOO".to_owned(), "1".to_owned())]
        );
        assert_eq!(harnesses[0].executables, ["claude-agent-acp"]);
        assert!(newer("0.86.0", "0.85.1") && !newer("0.85.1", "0.85.1") && !newer("1.0", ""));
        assert!(newer("1.10.0", "1.9.9"));
        assert!(
            guidance("Josh", "Be brief.").starts_with(
                "You are an agent named Josh, running inside Adeline ADE (Agentic Development Environment)\nBe brief."
            )
        );
    }
}
