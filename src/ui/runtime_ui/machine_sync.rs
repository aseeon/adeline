//! Each machine's projects in the UI: placing a snapshot's projects among the
//! other machines', checking and unchecking machines, and asking to upgrade one.
use super::*;

impl Adeline {
    fn place(&self) -> (Option<String>, Option<String>) {
        (
            self.has_open_project().then(|| self.workspace().key()),
            self.current_id(),
        )
    }

    fn restore(&mut self, (current, selected): (Option<String>, Option<String>)) {
        let found = current.and_then(|key| self.projects.iter().position(|p| p.key() == key));
        self.project = found
            .or_else(|| self.open_projects.iter().position(|open| *open))
            .unwrap_or(0);
        self.selected = if found.is_some() {
            selected.and_then(|id| {
                self.workspace()
                    .threads
                    .iter()
                    .position(|thread| thread.id == id)
            })
        } else {
            None
        };
    }

    pub(super) fn machine_range(&self, machine: &str) -> std::ops::Range<usize> {
        let start = self.projects.iter().position(|p| p.machine == machine);
        let Some(start) = start else {
            let order = machines::all();
            let rank = |m: &str| order.iter().position(|o| o == m).unwrap_or(order.len());
            let at = self
                .projects
                .iter()
                .position(|p| rank(&p.machine) > rank(machine))
                .unwrap_or(self.projects.len());
            return at..at;
        };
        let end = start
            + self.projects[start..]
                .iter()
                .take_while(|p| p.machine == machine)
                .count();
        start..end
    }

    pub(crate) fn insert_project(&mut self, project: Workspace, open: bool) -> usize {
        let at = self.machine_range(&project.machine).end;
        self.projects.insert(at, project);
        self.open_projects.insert(at, open);
        self.project_tints
            .insert(at, at % theme::project_colors().len());
        if self.project >= at && self.projects.len() > 1 {
            self.project += 1;
        }
        at
    }

    fn replace_machine_projects(
        &mut self,
        machine: &str,
        mut projects: Vec<Workspace>,
        open_all: bool,
    ) {
        let place = self.place();
        let open: HashSet<String> = self
            .projects
            .iter()
            .zip(&self.open_projects)
            .filter(|(_, open)| **open)
            .map(|(project, _)| project.key())
            .collect();
        let tints: HashMap<String, usize> = self
            .projects
            .iter()
            .zip(&self.project_tints)
            .map(|(project, tint)| (project.key(), *tint))
            .collect();
        let range = self.machine_range(machine);
        for project in &mut projects {
            machine.clone_into(&mut project.machine);
        }
        let gone: Vec<String> = self.projects[range.clone()]
            .iter()
            .flat_map(|p| &p.threads)
            .map(|t| t.id.clone())
            .collect();
        for id in gone {
            self.runtime.conversations.remove(&id);
        }
        let start = range.start;
        let opened: Vec<bool> = projects
            .iter()
            .map(|project| open_all || open.contains(&project.key()))
            .collect();
        let tinted: Vec<usize> = projects
            .iter()
            .enumerate()
            .map(|(i, project)| {
                tints
                    .get(&project.key())
                    .copied()
                    .unwrap_or([0, 2, 3][(start + i).min(2)])
            })
            .collect();
        self.projects.splice(range.clone(), projects);
        self.open_projects.splice(range.clone(), opened);
        self.project_tints.splice(range, tinted);
        self.restore(place);
    }

    pub(crate) fn apply_snapshot(
        &mut self,
        machine: &str,
        snapshot: Snapshot,
        cx: &mut Context<Self>,
    ) {
        let first = self.loaded_machines.insert(machine.to_owned());
        let mut projects = snapshot.projects;
        ui_state::apply(machine, &mut projects);
        self.replace_machine_projects(machine, projects, first);
        self.runtime.conversations.extend(snapshot.live);
        let catalog = self.catalog_mut(machine);
        catalog.entries = snapshot.agents;
        catalog.errors = snapshot.agent_errors;
        if self.selected_agent.is_none() && self.agent_catalog().entries.len() == 1 {
            self.selected_agent = Some(self.agent_catalog().entries[0].id.clone());
        }
        if !snapshot.errors.is_empty() {
            self.notify_toast(
                &self.about_machine(machine, &snapshot.errors.join("\n")),
                cx,
            );
        }
        self.agents_changed(None, None, false, cx);
        self.refresh_runtime_views(cx);
    }

    pub(super) fn about_machine(&self, machine: &str, message: &str) -> String {
        if machines::checked().len() > 1 {
            format!("{}: {message}", machines::name(machine))
        } else {
            message.to_owned()
        }
    }

    pub(crate) fn machine_unchecked(&mut self, machine: &str, cx: &mut Context<Self>) {
        self.replace_machine_projects(machine, Vec::new(), false);
        self.catalogs.remove(machine);
        self.loaded_machines.remove(machine);
        self.agents_changed(None, None, false, cx);
        self.sync_regions(&Action::Project(self.project), cx);
        self.refresh_runtime_views(cx);
    }

    pub(crate) fn machine_checked(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.demo_mode {
            let projects = load()
                .into_iter()
                .filter(|project| project.machine == machine)
                .collect();
            self.replace_machine_projects(machine, projects, true);
            self.catalogs
                .insert(machine.to_owned(), agents::AgentCatalog::new(true));
        }
        self.sync_regions(&Action::Project(self.project), cx);
        self.refresh_runtime_views(cx);
    }

    pub(crate) fn ask_upgrade(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.modal.is_some() {
            return;
        }
        self.upgrade_machine = Some(machine.to_owned());
        self.open_modal_later("upgrade", cx);
    }
}
