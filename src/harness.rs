//! Harnesses Adeline can run: the ACP registry plus built-in OMP, where their
//! executables are installed, and short prompt-free probes of their options.
use gpui_kit::{App, Global};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
pub const OMP: &str = "omp";
pub const CUSTOM: &str = "custom";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Harness {
    pub id: String,
    pub name: String,
    pub website: String,
    /// Executable names to look for, in order.
    executables: Vec<String>,
    pub arguments: Vec<String>,
}

fn omp() -> Harness {
    Harness {
        id: OMP.into(),
        name: "OMP".into(),
        website: "https://github.com/can1357/oh-my-pi".into(),
        executables: vec!["omp".into()],
        arguments: vec!["acp".into()],
    }
}

/// Every known harness and where each installed one lives.
#[derive(Default)]
pub struct Catalog {
    pub harnesses: Vec<Harness>,
    pub installed: HashMap<String, PathBuf>,
    /// Detection has finished at least once.
    pub detected: bool,
    demo: bool,
}

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
    ) -> Result<(String, Vec<String>), String> {
        if id == CUSTOM {
            let path = resolve(command)
                .ok_or_else(|| format!("Command {command} was not found on this machine."))?;
            return Ok((path.to_string_lossy().into_owned(), arguments.to_vec()));
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
        Ok((
            path.to_string_lossy().into_owned(),
            harness.arguments.clone(),
        ))
    }
}

/// Loads the cached or bundled harness list and starts background detection.
pub fn init(demo: bool, cx: &mut App) {
    cx.set_global(Catalog {
        harnesses: load(),
        demo,
        ..Default::default()
    });
    refresh(false, cx);
}

/// Detects installed harnesses in the background, after fetching a fresh
/// registry when `fetch` is set. Windows redraw as results arrive.
pub fn refresh(fetch: bool, cx: &mut App) {
    let fetch = fetch && !cx.global::<Catalog>().demo;
    let known = cx.global::<Catalog>().harnesses.clone();
    let task = cx.background_executor().spawn(async move {
        let harnesses = if fetch && download().is_ok() {
            load()
        } else {
            known
        };
        let installed: HashMap<_, _> = harnesses
            .iter()
            .filter_map(|harness| Some((harness.id.clone(), locate(harness)?)))
            .collect();
        (harnesses, installed)
    });
    cx.spawn(async move |cx| {
        let (harnesses, installed) = task.await;
        cx.update(|cx| {
            let catalog = cx.global_mut::<Catalog>();
            catalog.harnesses = harnesses;
            catalog.installed = installed;
            catalog.detected = true;
            cx.refresh_windows();
        });
    })
    .detach();
}

fn cache() -> Option<PathBuf> {
    crate::config::directory()
        .ok()
        .map(|path| path.join("cache"))
}

/// A cached registry icon, served to the asset loader as `registry-icons/<id>.svg`.
pub fn icon(path: &str) -> Option<Vec<u8>> {
    let name = path.strip_prefix("registry-icons/")?;
    if name.contains(['/', '\\']) || name.contains("..") {
        return None;
    }
    fs::read(cache()?.join("registry-icons").join(name)).ok()
}

pub fn has_icon(id: &str) -> bool {
    cache().is_some_and(|cache| {
        cache
            .join("registry-icons")
            .join(format!("{id}.svg"))
            .is_file()
    })
}

/// The cached registry, else the bundled copy, else OMP alone.
fn load() -> Vec<Harness> {
    let cached = cache()
        .and_then(|cache| fs::read_to_string(cache.join("registry.json")).ok())
        .and_then(|text| parse_registry(&text).ok());
    let bundled = || {
        crate::embedded("registry.json")
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|text| parse_registry(text).ok())
    };
    let mut harnesses = cached.or_else(bundled).unwrap_or_default();
    if !harnesses.iter().any(|harness| harness.id == OMP) {
        harnesses.push(omp());
    }
    harnesses
}

#[expect(
    clippy::disallowed_methods,
    reason = "Runs on the background executor, never on the UI thread"
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
        let distribution = &agent["distribution"];
        let mut executables = Vec::new();
        let mut arguments = Vec::new();
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
        }
        for runner in ["npx", "uvx"] {
            let Some(package) = distribution[runner]["package"].as_str() else {
                continue;
            };
            // `@scope/name@1.2.3` installs a `name` executable, sometimes without `-cli`.
            // ponytail: name heuristic; packages whose binary is named otherwise stay red.
            let unversioned = package
                .get(1..)
                .and_then(|rest| rest.find('@'))
                .map_or(package, |at| &package[..=at]);
            let base = unversioned.rsplit('/').next().unwrap_or(unversioned);
            executables.push(base.to_owned());
            if let Some(short) = base.strip_suffix("-cli") {
                executables.push(short.to_owned());
            }
            if arguments.is_empty() {
                arguments = strings(&distribution[runner]["args"]);
            }
        }
        executables.push(id.to_owned());
        executables.dedup();
        let link = |key: &str| agent[key].as_str().unwrap_or_default().to_owned();
        harnesses.push(Harness {
            id: id.to_owned(),
            name: name.to_owned(),
            website: Some(link("website"))
                .filter(|url| !url.is_empty())
                .unwrap_or_else(|| link("repository")),
            executables,
            arguments,
        });
    }
    Ok(harnesses)
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
    ] {
        if let Some(base) = std::env::var_os(variable) {
            dirs.push(PathBuf::from(base).join(folder));
        }
    }
    dirs
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
    harness
        .executables
        .iter()
        .find_map(|name| find(name, &dirs))
}

/// A Custom command as a path: itself when it names a file, else found on PATH.
pub fn resolve(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_file() {
        return Some(path.to_owned());
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

/// Whether Adeline knows how to give this harness system instructions.
/// A Custom harness counts by the identity it reports in its handshake.
pub fn supports_instructions(harness: &str, identity: &str) -> bool {
    harness == OMP || ["omp", "oh-my-pi"].contains(&identity.to_lowercase().as_str())
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Model,
    Effort,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub name: String,
    /// The server's group, else the `provider/` prefix, else empty.
    pub group: String,
}

/// One select-type config option a harness offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    pub id: String,
    pub current: String,
    pub choices: Vec<Choice>,
}

impl Setting {
    pub fn offers(&self, value: &str) -> bool {
        self.choices.iter().any(|choice| choice.value == value)
    }

    pub fn name_of<'a>(&'a self, value: &'a str) -> &'a str {
        self.choices
            .iter()
            .find(|choice| choice.value == value)
            .map_or(value, |choice| choice.name.as_str())
    }
}

/// Finds the model or effort option among ACP `configOptions`.
pub fn setting(options: &[Value], kind: Kind) -> Option<Setting> {
    let option = options.iter().find(|option| {
        let id = option["id"].as_str().unwrap_or_default();
        let category = option["category"].as_str().unwrap_or_default();
        option["type"].as_str().is_none_or(|kind| kind == "select")
            && match kind {
                Kind::Model => category == "model" || (category.is_empty() && id == "model"),
                Kind::Effort => {
                    category == "thought_level"
                        || (category.is_empty()
                            && ["thinking", "effort", "reasoning_effort", "thought_level"]
                                .contains(&id))
                }
            }
    })?;
    let mut choices = Vec::new();
    let mut grouped = false;
    for entry in option["options"].as_array().into_iter().flatten() {
        if let Some(items) = entry["options"].as_array() {
            grouped = true;
            let group = entry["name"]
                .as_str()
                .or_else(|| entry["group"].as_str())
                .unwrap_or_default();
            choices.extend(items.iter().filter_map(|item| choice(item, group)));
        } else {
            choices.extend(choice(entry, ""));
        }
    }
    if !grouped && choices.iter().all(|choice| choice.value.contains('/')) {
        for choice in &mut choices {
            choice.group = choice
                .value
                .split('/')
                .next()
                .unwrap_or_default()
                .to_owned();
        }
    }
    Some(Setting {
        id: option["id"].as_str()?.to_owned(),
        current: option["currentValue"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        choices,
    })
}

fn choice(item: &Value, group: &str) -> Option<Choice> {
    let value = item["value"].as_str()?.to_owned();
    Some(Choice {
        name: item["name"].as_str().unwrap_or(&value).to_owned(),
        value,
        group: group.to_owned(),
    })
}

/// What a probe learned: the reported identity and offered options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Probed {
    pub identity: String,
    pub options: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeError {
    pub message: String,
    /// Advertised ACP login methods, when authentication is missing.
    pub login: Vec<String>,
}

/// A running probe. Dropping it stops the probe's process.
pub struct Probe {
    cancelled: Arc<AtomicBool>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

/// Starts `command` in a temporary folder, runs the ACP handshake and opens a
/// session without prompting, sets `model` when given, then stops it.
pub fn probe(
    command: PathBuf,
    arguments: Vec<String>,
    model: Option<String>,
) -> (Probe, async_channel::Receiver<Result<Probed, ProbeError>>) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = async_channel::bounded(1);
    let flag = cancelled.clone();
    thread::spawn(move || {
        let folder = std::env::temp_dir().join(crate::files::unique("adeline-probe"));
        let result = fs::create_dir_all(&folder)
            .map_err(|e| failure(format!("Could not create a probe folder: {e}")))
            .and_then(|()| run_probe(&command, &arguments, model.as_deref(), &folder, &flag));
        let _ = fs::remove_dir_all(&folder);
        if !flag.load(Ordering::SeqCst) {
            let _ = sender.send_blocking(result);
        }
    });
    (Probe { cancelled }, receiver)
}

fn failure(message: String) -> ProbeError {
    ProbeError {
        message,
        login: Vec::new(),
    }
}

struct Session<'a> {
    child: Child,
    lines: mpsc::Receiver<String>,
    cancelled: &'a AtomicBool,
    next: u64,
    command: String,
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Session<'_> {
    fn request(&mut self, method: &str, params: &Value) -> Result<Value, ProbeError> {
        self.next += 1;
        let id = self.next;
        let message = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| failure("No stdin.".into()))?;
        writeln!(stdin, "{message}")
            .and_then(|()| stdin.flush())
            .map_err(|_| self.silent())?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                return Err(failure("Probe cancelled.".into()));
            }
            if Instant::now() >= deadline {
                return Err(if id == 1 {
                    self.silent()
                } else {
                    failure(format!(
                        "{} did not answer ACP {method} within 30 seconds.",
                        self.command
                    ))
                });
            }
            let line = match self.lines.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => line,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(self.silent()),
            };
            let Ok(reply) = serde_json::from_str::<Value>(&line) else {
                if id == 1 {
                    return Err(self.silent());
                }
                continue;
            };
            if reply["id"].as_u64() != Some(id) || reply.get("method").is_some() {
                continue;
            }
            if let Some(error) = reply.get("error") {
                return Err(failure(
                    error["message"]
                        .as_str()
                        .unwrap_or("Unknown ACP error")
                        .to_owned(),
                ));
            }
            return Ok(reply["result"].clone());
        }
    }

    fn silent(&self) -> ProbeError {
        failure(format!(
            "{} did not answer the ACP handshake. Check that it speaks ACP over stdio, for example with its `acp` argument.",
            self.command
        ))
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "Runs on the probe's own thread, never on the UI thread"
)]
fn run_probe(
    command: &Path,
    arguments: &[String],
    model: Option<&str>,
    folder: &Path,
    cancelled: &AtomicBool,
) -> Result<Probed, ProbeError> {
    let shown = command.file_name().map_or_else(
        || command.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut process = hidden(Command::new(command));
    process
        .args(arguments)
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = process
        .spawn()
        .map_err(|e| failure(format!("Could not start {shown}: {e}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let (lines, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let mut session = Session {
        child,
        lines: receiver,
        cancelled,
        next: 0,
        command: shown,
    };
    let info = session.request(
        "initialize",
        &json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"adeline","title":"Adeline","version":env!("CARGO_PKG_VERSION")}}),
    )?;
    let login: Vec<String> = info["authMethods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|method| {
            let name = method["name"].as_str()?;
            Some(match method["description"].as_str() {
                Some(description) if !description.is_empty() => format!("{name}: {description}"),
                _ => name.to_owned(),
            })
        })
        .collect();
    let identity = info["agentInfo"]["name"]
        .as_str()
        .or_else(|| info["agentInfo"]["title"].as_str())
        .unwrap_or_default()
        .to_owned();
    let session_result = session
        .request(
            "session/new",
            &json!({"cwd":folder.to_string_lossy(),"mcpServers":[]}),
        )
        .map_err(|mut error| {
            if crate::acp::needs_login(&error.message) {
                error.login = login;
            }
            error
        })?;
    let mut options = session_result["configOptions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Some harnesses (Codex) list effort only once a model has been set, so
    // always set one: the chosen model, else the harness's current one.
    if let (Some(setting), Some(session_id)) = (
        setting(&options, Kind::Model),
        session_result["sessionId"].as_str(),
    ) && let Some(model) = model
        .filter(|model| setting.offers(model))
        .or_else(|| {
            setting
                .offers(&setting.current)
                .then_some(setting.current.as_str())
        })
        .or_else(|| setting.choices.first().map(|choice| choice.value.as_str()))
    {
        let result = session.request(
            "session/set_config_option",
            &json!({"sessionId":session_id,"configId":setting.id,"value":model}),
        )?;
        if let Some(updated) = result["configOptions"].as_array() {
            options.clone_from(updated);
        }
    }
    Ok(Probed { identity, options })
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
        assert!(parse_registry("not json").is_err());
        let bundled = std::str::from_utf8(crate::embedded("registry.json").unwrap()).unwrap();
        assert!(parse_registry(bundled).unwrap().len() > 10);
    }

    #[test]
    fn options_group_by_server_groups_or_provider_prefix() {
        let options = vec![
            json!({"id":"mode","category":"mode","type":"select","options":[{"value":"plan"}]}),
            json!({"id":"model","category":"model","type":"select","currentValue":"openai/gpt",
                   "options":[{"value":"openai/gpt","name":"GPT"},{"value":"xai/grok","name":"Grok"}]}),
            json!({"id":"effort","category":"thought_level","type":"select","currentValue":"low",
                   "options":[{"group":"fast","name":"Fast","options":[{"value":"low","name":"Low"}]}]}),
        ];
        let model = setting(&options, Kind::Model).unwrap();
        assert_eq!(model.current, "openai/gpt");
        assert_eq!(model.choices[1].group, "xai");
        assert_eq!(model.name_of("xai/grok"), "Grok");
        let effort = setting(&options, Kind::Effort).unwrap();
        assert_eq!(effort.id, "effort");
        assert_eq!(effort.choices[0].group, "Fast");
        assert!(setting(&options[..1], Kind::Model).is_none());
        let legacy = [json!({"id":"thinking","options":[{"value":"high"}]})];
        assert!(setting(&legacy, Kind::Effort).unwrap().offers("high"));
    }

    #[test]
    fn instruction_support_follows_harness_or_reported_identity() {
        assert!(supports_instructions(OMP, ""));
        assert!(supports_instructions(CUSTOM, "omp"));
        assert!(!supports_instructions("gemini", "gemini-cli"));
        assert!(
            guidance("Josh", "Be brief.").starts_with(
                "You are an agent named Josh, running inside Adeline ADE (Agentic Development Environment)\nBe brief."
            )
        );
    }

    #[test]
    fn missing_custom_command_fails_the_probe_with_a_clear_error() {
        let (_probe, results) = probe(PathBuf::from("adeline-no-such-command"), Vec::new(), None);
        let error = results.recv_blocking().unwrap().unwrap_err();
        assert!(
            error.message.contains("Could not start"),
            "{}",
            error.message
        );
    }
}
