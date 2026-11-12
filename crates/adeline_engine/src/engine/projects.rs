//! Projects, the agent catalog, harness commands, engine settings and the watched files.
use super::{Engine, Input, TRAFFIC_BACKLOG, disk_icons, lifecycle_for, live_for, settings_path};
use crate::{
    acp,
    storage::{self, ProjectStore},
};
use adeline_core::{
    agents, config,
    conversation::AuthMethod,
    data::Workspace,
    harness, install,
    protocol::{self, Delta, EngineMessage, EngineSettings},
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::block_in_place};

const OWN_WRITE: Duration = Duration::from_secs(1);

impl Engine {
    pub(super) fn save_project(
        &mut self,
        original: Option<&str>,
        name: &str,
        directory: &Path,
    ) -> Result<Value, String> {
        if original.is_some_and(|id| self.deleting.contains_key(id)) {
            return Err("This project is being deleted.".into());
        }
        let store = &mut self.store;
        let (id, workspace) = block_in_place(|| {
            let id = store
                .save_project(original, name, directory)
                .map_err(|e| format!("{e:#}"))?;
            let workspace = store
                .projects
                .iter()
                .find(|project| project.id == id)
                .map(storage::ProjectRecord::to_workspace)
                .ok_or_else(|| "Saved project is missing.".to_owned())?;
            Ok::<_, String>((id, workspace))
        })?;
        self.upsert_project(original.map(str::to_owned), workspace);
        Ok(Value::String(id))
    }

    fn upsert_project(&mut self, previous: Option<String>, workspace: Workspace) {
        let key = previous
            .as_deref()
            .unwrap_or(&workspace.config.id)
            .to_owned();
        if let Some(project) = self.projects.iter_mut().find(|p| p.config.id == key) {
            project.config = workspace.config.clone();
        } else {
            self.projects.push(workspace.clone());
        }
        self.broadcast_message(&EngineMessage::Delta(Delta::Project {
            previous,
            workspace,
        }));
    }

    fn forget_conversation(&mut self, id: &str) {
        if let Some(live) = self.live.remove(id) {
            for queued in live.queued {
                self.queued_files.remove(&queued.id);
            }
        }
        self.attachments.remove(id);
        self.lifecycles.remove(id);
    }

    pub(super) fn finish_deletions(&mut self) {
        let ready: Vec<_> = self
            .deleting
            .keys()
            .filter(|id| {
                self.project_conversations(id)
                    .iter()
                    .all(|c| self.agent(c).is_none())
            })
            .cloned()
            .collect();
        for id in ready {
            let waiters = self.deleting.remove(&id).unwrap_or_default();
            let result =
                block_in_place(|| self.store.delete_project(&id)).map_err(|e| format!("{e:#}"));
            if result.is_ok() {
                for conversation in self.project_conversations(&id) {
                    self.forget_conversation(&conversation);
                }
                self.projects.retain(|p| p.config.id != id);
                self.broadcast_message(&EngineMessage::Delta(Delta::ProjectRemoved {
                    id: id.clone(),
                }));
            }
            for (client, request) in waiters {
                self.reply(client, request, result.clone().map(|()| Value::Null));
            }
        }
    }

    pub(super) fn cancel_delete_project(&mut self, id: &str) {
        for (client, request) in self.deleting.remove(id).unwrap_or_default() {
            self.reply(client, request, Err("Deletion cancelled.".into()));
        }
    }

    pub(super) fn force_project(&mut self, id: &str) {
        for conversation in self.project_conversations(id) {
            self.force_stop(&conversation);
        }
    }

    pub(super) fn save_agent(
        &mut self,
        original: Option<&str>,
        definition: agents::AgentDefinition,
        expected: Option<&agents::AgentDefinition>,
        overwrite: bool,
    ) -> Result<Value, String> {
        let saved = block_in_place(|| self.agents.save(original, definition, expected, overwrite));
        self.agents_changed();
        saved.map(Value::String).map_err(|e| format!("{e:#}"))
    }

    pub(super) fn delete_agent(&mut self, id: &str) -> Result<Value, String> {
        let deleted = block_in_place(|| self.agents.delete(id));
        self.agents_changed();
        deleted.map(|()| Value::Null).map_err(|e| format!("{e:#}"))
    }

    pub(super) fn agents_changed(&mut self) {
        self.broadcast(Delta::Agents {
            entries: self.agents.entries.clone(),
            errors: self.agents.errors.clone(),
        });
        self.refresh_icons();
    }

    pub(super) fn refresh_icons(&mut self) {
        let icons = block_in_place(disk_icons);
        if icons != self.icons {
            self.icons = icons;
            self.broadcast(Delta::Icons(self.icons.clone()));
        }
    }

    pub(super) fn detect_harnesses(&self, fetch: bool) {
        let known = self.harnesses.harnesses.clone();
        let input = self.input.clone();
        tokio::task::spawn_blocking(move || {
            let _ = input.send(Input::Harnesses(harness::detect(fetch, known)));
        });
    }

    fn standalone(
        &self,
        harness_id: &str,
        command: &str,
        arguments: Vec<String>,
    ) -> Result<agents::ExecutionConfig, String> {
        let launch =
            if harness_id == adeline_core::profiles::CUSTOM || Path::new(command).is_absolute() {
                block_in_place(|| harness::resolve(command))
                    .map(|path| harness::Launch {
                        command: path,
                        arguments,
                        environment: Vec::new(),
                    })
                    .ok_or_else(|| format!("Command {command} was not found on this machine."))?
            } else if self.harnesses.installed.contains_key(harness_id) {
                self.harnesses
                    .launch(harness_id, "", &[])
                    .map_err(|e| format!("{e:#}"))?
            } else {
                return Err(format!(
                    "{} is not installed.",
                    self.harnesses.label(harness_id, "")
                ));
            };
        Ok(agents::ExecutionConfig {
            name: self.harnesses.label(harness_id, ""),
            harness: harness_id.to_owned(),
            command: launch.command,
            arguments: launch.arguments,
            environment: launch.environment,
            directory: std::env::temp_dir(),
            ..Default::default()
        })
    }

    pub(super) fn probe(
        &mut self,
        client: u64,
        request: u64,
        probe: u64,
        harness_id: &str,
        command: &str,
        arguments: Vec<String>,
        model: Option<String>,
    ) -> Option<Result<Value, String>> {
        let mut config = match self.standalone(harness_id, command, arguments) {
            Ok(config) => config,
            Err(message) => {
                let failed: Result<protocol::Probed, _> = Err(protocol::ProbeError {
                    message,
                    auth: Vec::new(),
                    version: false,
                });
                return Some(super::docs::json(failed));
            }
        };
        config.selections.model = model.unwrap_or_default();
        let (session, results) = acp::probe(config);
        self.probes.insert((client, probe), session);
        let input = self.input.clone();
        tokio::spawn(async move {
            let result = results.recv().await.ok();
            let _ = input.send(Input::ProbeDone {
                client,
                request,
                probe,
                result,
            });
        });
        None
    }

    pub(super) fn login(
        &mut self,
        client: u64,
        request: u64,
        harness_id: &str,
        command: &str,
        arguments: Vec<String>,
        method: Option<AuthMethod>,
    ) -> Option<Result<Value, String>> {
        let config = match self.standalone(harness_id, command, arguments) {
            Ok(config) => config,
            Err(error) => return Some(Err(error)),
        };
        if let Some(terminal) = method.as_ref().and_then(|method| method.terminal.clone()) {
            let result = block_in_place(|| {
                install::open_terminal(
                    Path::new(&config.command),
                    &terminal.arguments,
                    &terminal.environment,
                )
            });
            return Some(result.map(|()| Value::Null).map_err(|e| format!("{e:#}")));
        }
        let (session, results, urls) = acp::login(config, method.map(|method| method.id));
        self.logins.insert((client, request), session);
        let input = self.input.clone();
        tokio::spawn(async move {
            let url_input = input.clone();
            let opener = tokio::spawn(async move {
                while let Ok(url) = urls.recv().await {
                    let _ = url_input.send(Input::OpenUrl(url));
                }
            });
            let result = results
                .recv()
                .await
                .unwrap_or_else(|_| Err("The login stopped.".into()))
                .map(|()| Value::Null);
            opener.abort();
            let _ = input.send(Input::Done {
                client,
                request,
                result,
                detect: false,
            });
        });
        None
    }

    pub(super) fn restart(&mut self, id: &str) {
        let Some(driver) = self.lifecycle(id).restart(acp::CONTINUE_TURN.into()) else {
            return;
        };
        let _ = driver.send(acp::Command::ForceStop);
        self.live_mut(id).quiet_since = None;
        self.send_live(id);
    }

    pub(super) fn begin_delete_project(
        &mut self,
        client: u64,
        request: u64,
        id: &str,
    ) -> Option<Result<Value, String>> {
        if !self.projects.iter().any(|p| p.config.id == id) {
            return Some(Err(format!("Project {id} no longer exists.")));
        }
        self.deleting
            .entry(id.to_owned())
            .or_default()
            .push((client, request));
        for conversation in self.project_conversations(id) {
            self.shutdown_conversation(&conversation);
            self.send_live(&conversation);
        }
        self.finish_deletions();
        None
    }

    pub(super) fn install(
        &mut self,
        client: u64,
        request: u64,
        target: &install::Target,
    ) -> Option<Result<Value, String>> {
        let steps = match install::plan(target, &self.harnesses) {
            Ok(steps) => steps,
            Err(error) => return Some(Err(format!("{error:#}"))),
        };
        let input = self.input.clone();
        tokio::spawn(async move {
            let output_input = input.clone();
            let output = Arc::new(move |line: String| {
                let _ = output_input.send(Input::Output {
                    client,
                    request,
                    line,
                });
            });
            let result = install::run(steps, output)
                .await
                .map(|()| Value::Null)
                .map_err(|e| format!("{e:#}"));
            let _ = input.send(Input::Done {
                client,
                request,
                result,
                detect: true,
            });
        });
        None
    }

    pub(super) fn watch_traffic(&mut self, client: u64, id: Option<String>) -> Value {
        let recorded = id
            .as_ref()
            .map(|id| block_in_place(|| self.store.traffic(id, TRAFFIC_BACKLOG)))
            .unwrap_or_default();
        match id {
            Some(id) => self.watching.insert(client, id),
            None => self.watching.remove(&client),
        };
        Value::Array(recorded)
    }

    pub(super) fn set_settings(&mut self, settings: EngineSettings) -> Result<Value, String> {
        let path = settings_path().map_err(|e| format!("{e:#}"))?;
        block_in_place(|| {
            config::seed_yaml(&path, &settings).and_then(|()| config::write_yaml(&path, &settings))
        })
        .map_err(|e| format!("{e:#}"))?;
        self.settings = settings;
        let retries = u32::try_from(self.settings.retry_limit).unwrap_or(u32::MAX);
        for agent in self.lifecycles.values().filter_map(|l| l.phase.agent()) {
            let _ = agent.driver.send(acp::Command::SetRetries(retries));
        }
        self.broadcast(Delta::Settings(self.settings.clone()));
        Ok(Value::Null)
    }

    pub(super) fn watch(input: mpsc::UnboundedSender<Input>) -> Option<notify::RecommendedWatcher> {
        use notify::Watcher as _;
        let root = config::directory().ok()?;
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event
                && !matches!(event.kind, notify::EventKind::Access(_))
            {
                let _ = input.send(Input::Watch(event.paths));
            }
        });
        let result = watcher.and_then(|mut watcher| {
            for folder in ["agents", "projects"] {
                let path = root.join(folder);
                let _ = std::fs::create_dir_all(&path);
                watcher.watch(&path, notify::RecursiveMode::Recursive)?;
            }
            Ok(watcher)
        });
        result
            .map_err(|error| {
                tracing::event!(
                    name: "engine.watch.failed",
                    tracing::Level::WARN,
                    error.message = %error,
                    "Cannot watch the data folders: {{error.message}}"
                );
            })
            .ok()
    }

    pub(super) fn reload_watched(&mut self) {
        let Ok(root) = config::directory() else {
            return;
        };
        let paths = std::mem::take(&mut self.watched);
        let agents_root = root.join("agents");
        let projects_root = root.join("projects");
        if paths.iter().any(|path| path.starts_with(&agents_root)) {
            let changed = block_in_place(|| self.agents.refresh());
            if changed {
                self.agents_changed();
            }
        }
        let outside = paths.iter().any(|path| {
            let Ok(relative) = path.strip_prefix(&projects_root) else {
                return false;
            };
            let mut parts = relative.iter().map(|part| part.to_string_lossy());
            let conversation = match (parts.next(), parts.next(), parts.next()) {
                (Some(_), Some(folder), Some(id)) if folder == "conversations" => Some(id),
                _ => None,
            };
            conversation.is_none_or(|id| {
                self.agent(id.as_ref()).is_none()
                    && self
                        .touched
                        .get(id.as_ref())
                        .is_none_or(|at| at.elapsed() > OWN_WRITE)
            })
        });
        if outside {
            let input = self.input.clone();
            let started = Instant::now();
            tokio::task::spawn_blocking(move || {
                let store = ProjectStore::with_root(projects_root);
                let _ = input.send(Input::ProjectsLoaded { store, started });
            });
        }
    }

    pub(super) fn reconcile(&mut self, fresh: ProjectStore, started: Instant) {
        let busy: HashSet<String> = self
            .agent_ids()
            .into_iter()
            .chain(
                self.touched
                    .iter()
                    .filter(|(_, at)| **at + OWN_WRITE >= started)
                    .map(|(id, _)| id.clone()),
            )
            .chain(
                self.deleting
                    .keys()
                    .flat_map(|p| self.project_conversations(p)),
            )
            .collect();
        let deleting: HashSet<String> = self.deleting.keys().cloned().collect();
        let changes = self.store.reconcile(fresh, &busy, &deleting);
        for error in &changes.errors {
            tracing::event!(
                name: "engine.store.reconcile_failed",
                tracing::Level::WARN,
                error.message = %error,
                "{{error.message}}"
            );
            self.broadcast_message(&EngineMessage::Delta(Delta::Notice {
                message: error.clone(),
            }));
        }
        for workspace in changes.projects {
            self.upsert_project(None, workspace);
        }
        for id in changes.removed {
            for conversation in self.project_conversations(&id) {
                self.forget_conversation(&conversation);
            }
            self.projects.retain(|p| p.config.id != id);
            self.broadcast_message(&EngineMessage::Delta(Delta::ProjectRemoved { id }));
        }
        for id in changes.conversations {
            let Some((project_id, thread)) = self.thread_from_store(&id) else {
                continue;
            };
            if let Some(conversation) = self.store.conversation(&id) {
                let (live, lifecycle) = (live_for(conversation), lifecycle_for(conversation));
                self.live.insert(id.clone(), live);
                self.lifecycles.insert(id.clone(), lifecycle);
            }
            self.broadcast(Delta::Thread { project_id, thread });
            self.send_live(&id);
        }
    }
}
