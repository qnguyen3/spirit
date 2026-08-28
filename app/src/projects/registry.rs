use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;

use anyhow::{Result, bail};
use chrono::Utc;
use persistence::model::{Project as ProjectRow, ProjectWorktree as WorktreeRow};
use warp_errors::report_error;
use warpui::{Entity, ModelContext, SingletonEntity};

use super::{Project, ProjectId, ProjectKind, Worktree, WorktreeId, WorktreeKind};
use crate::persistence::ModelEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRegistryEvent {
    ProjectAdded(ProjectId),
    ProjectUpdated(ProjectId),
    ProjectRemoved(ProjectId),
    WorktreeAdded(WorktreeId),
    WorktreeUpdated(WorktreeId),
    WorktreeRemoved(WorktreeId),
}

pub struct ProjectRegistryModel {
    projects: HashMap<ProjectId, Project>,
    worktrees: HashMap<WorktreeId, Worktree>,
    model_event_sender: Option<SyncSender<ModelEvent>>,
}

impl Entity for ProjectRegistryModel {
    type Event = ProjectRegistryEvent;
}

impl SingletonEntity for ProjectRegistryModel {}

pub fn now_ts() -> i64 {
    Utc::now().timestamp()
}

fn normalized(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl ProjectRegistryModel {
    pub fn new(model_event_sender: Option<SyncSender<ModelEvent>>) -> Self {
        Self {
            projects: HashMap::new(),
            worktrees: HashMap::new(),
            model_event_sender,
        }
    }

    pub fn from_persisted(
        projects: Vec<Project>,
        worktrees: Vec<Worktree>,
        model_event_sender: Option<SyncSender<ModelEvent>>,
    ) -> Self {
        let mut model = Self::new(model_event_sender);
        model.load(projects, worktrees);
        model
    }

    pub fn load(&mut self, projects: Vec<Project>, worktrees: Vec<Worktree>) {
        log::debug!(
            "Loading {} persisted projects and {} worktrees",
            projects.len(),
            worktrees.len()
        );
        self.projects = projects
            .into_iter()
            .map(|project| (project.id, project))
            .collect();
        self.worktrees = worktrees
            .into_iter()
            .map(|worktree| (worktree.id, worktree))
            .collect();
        self.repair_invariants();
    }

    fn repair_invariants(&mut self) {
        let known_projects: HashSet<ProjectId> = self.projects.keys().copied().collect();
        let orphans: Vec<WorktreeId> = self
            .worktrees
            .values()
            .filter(|worktree| !known_projects.contains(&worktree.project_id))
            .map(|worktree| worktree.id)
            .collect();
        for id in orphans {
            log::warn!("Dropping worktree {id} whose project is unknown");
            self.worktrees.remove(&id);
            self.send(ModelEvent::RemoveWorktree {
                worktree_id: id.to_string(),
            });
        }

        let mut primaries: HashMap<ProjectId, Vec<WorktreeId>> = HashMap::new();
        for worktree in self.worktrees.values() {
            if worktree.is_primary() {
                primaries
                    .entry(worktree.project_id)
                    .or_default()
                    .push(worktree.id);
            }
        }

        for (project_id, project) in self.projects.clone() {
            match primaries.get(&project_id) {
                None => {
                    let worktree = Worktree {
                        id: WorktreeId::new(),
                        project_id,
                        name: project.display_name.clone(),
                        kind: WorktreeKind::Primary,
                        created_ts: project.created_ts,
                    };
                    log::warn!("Synthesizing a Primary worktree for project {project_id}");
                    self.send(ModelEvent::UpsertWorktree {
                        worktree: WorktreeRow::from(&worktree),
                    });
                    self.worktrees.insert(worktree.id, worktree);
                }
                Some(ids) if ids.len() > 1 => {
                    let mut ids = ids.clone();
                    ids.sort_by_key(|id| id.to_string());
                    for duplicate in ids.into_iter().skip(1) {
                        log::warn!("Dropping duplicate Primary worktree {duplicate}");
                        self.worktrees.remove(&duplicate);
                        self.send(ModelEvent::RemoveWorktree {
                            worktree_id: duplicate.to_string(),
                        });
                    }
                }
                Some(_) => {}
            }
        }
    }

    fn send(&self, event: ModelEvent) {
        let Some(sender) = &self.model_event_sender else {
            return;
        };
        if let Err(err) = sender.send(event) {
            report_error!(anyhow::Error::new(err).context("Failed to persist project registry"));
        }
    }

    fn persist_project(&self, project: &Project) {
        self.send(ModelEvent::UpsertProject {
            project: ProjectRow::from(project),
        });
    }

    fn persist_worktree(&self, worktree: &Worktree) {
        self.send(ModelEvent::UpsertWorktree {
            worktree: WorktreeRow::from(worktree),
        });
    }

    pub fn register_project(
        &mut self,
        root_path: PathBuf,
        display_name: String,
        kind: ProjectKind,
        primary_branch: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) -> ProjectId {
        let root_path = normalized(&root_path);
        if let Some(existing) = self.project_by_root(&root_path) {
            return existing.id;
        }

        let now = now_ts();
        let project = Project {
            id: ProjectId::new(),
            root_path,
            display_name,
            kind,
            primary_branch,
            created_ts: now,
            last_opened_ts: now,
        };
        let project_id = project.id;
        let worktree = Worktree {
            id: WorktreeId::new(),
            project_id,
            name: project.display_name.clone(),
            kind: WorktreeKind::Primary,
            created_ts: now,
        };

        self.persist_project(&project);
        self.persist_worktree(&worktree);
        self.projects.insert(project_id, project);
        self.worktrees.insert(worktree.id, worktree);

        ctx.emit(ProjectRegistryEvent::ProjectAdded(project_id));
        ctx.notify();
        project_id
    }

    pub fn remove_project(&mut self, id: ProjectId, ctx: &mut ModelContext<Self>) {
        if self.projects.remove(&id).is_none() {
            return;
        }
        self.worktrees
            .retain(|_, worktree| worktree.project_id != id);
        self.send(ModelEvent::RemoveProject {
            project_id: id.to_string(),
        });
        ctx.emit(ProjectRegistryEvent::ProjectRemoved(id));
        ctx.notify();
    }

    pub fn rename_project(
        &mut self,
        id: ProjectId,
        display_name: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(project) = self.projects.get_mut(&id) else {
            return;
        };
        project.display_name = display_name;
        let project = project.clone();
        self.persist_project(&project);
        ctx.emit(ProjectRegistryEvent::ProjectUpdated(id));
        ctx.notify();
    }

    pub fn touch_opened(&mut self, id: ProjectId, now: i64, ctx: &mut ModelContext<Self>) {
        let Some(project) = self.projects.get_mut(&id) else {
            return;
        };
        project.last_opened_ts = now;
        let project = project.clone();
        self.persist_project(&project);
        ctx.emit(ProjectRegistryEvent::ProjectUpdated(id));
        ctx.notify();
    }

    pub fn set_primary_branch(
        &mut self,
        id: ProjectId,
        branch: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(project) = self.projects.get_mut(&id) else {
            return;
        };
        if project.primary_branch == branch {
            return;
        }
        project.primary_branch = branch;
        let project = project.clone();
        self.persist_project(&project);
        ctx.emit(ProjectRegistryEvent::ProjectUpdated(id));
        ctx.notify();
    }

    pub fn add_linked_worktree(
        &mut self,
        project_id: ProjectId,
        name: String,
        path: PathBuf,
        branch: String,
        base_branch: String,
        ctx: &mut ModelContext<Self>,
    ) -> WorktreeId {
        let worktree = Worktree {
            id: WorktreeId::new(),
            project_id,
            name,
            kind: WorktreeKind::Linked {
                path,
                branch,
                base_branch,
            },
            created_ts: now_ts(),
        };
        let worktree_id = worktree.id;
        self.persist_worktree(&worktree);
        self.worktrees.insert(worktree_id, worktree);
        ctx.emit(ProjectRegistryEvent::WorktreeAdded(worktree_id));
        ctx.notify();
        worktree_id
    }

    pub fn remove_worktree(&mut self, id: WorktreeId, ctx: &mut ModelContext<Self>) -> Result<()> {
        let Some(worktree) = self.worktrees.get(&id) else {
            bail!("Unknown worktree {id}");
        };
        if worktree.is_primary() {
            bail!("The Primary worktree cannot be removed");
        }
        self.worktrees.remove(&id);
        self.send(ModelEvent::RemoveWorktree {
            worktree_id: id.to_string(),
        });
        ctx.emit(ProjectRegistryEvent::WorktreeRemoved(id));
        ctx.notify();
        Ok(())
    }

    pub fn rename_worktree(&mut self, id: WorktreeId, name: String, ctx: &mut ModelContext<Self>) {
        let Some(worktree) = self.worktrees.get_mut(&id) else {
            return;
        };
        worktree.name = name;
        let worktree = worktree.clone();
        self.persist_worktree(&worktree);
        ctx.emit(ProjectRegistryEvent::WorktreeUpdated(id));
        ctx.notify();
    }

    pub fn set_worktree_branch(
        &mut self,
        id: WorktreeId,
        branch: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(worktree) = self.worktrees.get_mut(&id) else {
            return;
        };
        let WorktreeKind::Linked {
            branch: current, ..
        } = &mut worktree.kind
        else {
            return;
        };
        if *current == branch {
            return;
        }
        *current = branch;
        let worktree = worktree.clone();
        self.persist_worktree(&worktree);
        ctx.emit(ProjectRegistryEvent::WorktreeUpdated(id));
        ctx.notify();
    }

    pub fn projects_mru(&self) -> Vec<&Project> {
        let mut projects: Vec<&Project> = self.projects.values().collect();
        projects.sort_by(|left, right| {
            right
                .last_opened_ts
                .cmp(&left.last_opened_ts)
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        projects
    }

    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    pub fn project(&self, id: ProjectId) -> Option<&Project> {
        self.projects.get(&id)
    }

    pub fn project_by_root(&self, root_path: &Path) -> Option<&Project> {
        let target = normalized(root_path);
        self.projects
            .values()
            .find(|project| normalized(&project.root_path) == target)
    }

    pub fn worktrees_for_project(&self, id: ProjectId) -> Vec<&Worktree> {
        let mut worktrees: Vec<&Worktree> = self
            .worktrees
            .values()
            .filter(|worktree| worktree.project_id == id)
            .collect();
        worktrees.sort_by(|left, right| {
            right
                .is_primary()
                .cmp(&left.is_primary())
                .then_with(|| left.created_ts.cmp(&right.created_ts))
                .then_with(|| left.name.cmp(&right.name))
        });
        worktrees
    }

    pub fn linked_worktree_count(&self, id: ProjectId) -> usize {
        self.worktrees
            .values()
            .filter(|worktree| worktree.project_id == id && !worktree.is_primary())
            .count()
    }

    pub fn worktree(&self, id: WorktreeId) -> Option<&Worktree> {
        self.worktrees.get(&id)
    }

    pub fn primary_worktree_id(&self, project_id: ProjectId) -> Option<WorktreeId> {
        self.worktrees
            .values()
            .find(|worktree| worktree.project_id == project_id && worktree.is_primary())
            .map(|worktree| worktree.id)
    }

    pub fn worktree_names_for_project(&self, project_id: ProjectId) -> HashSet<String> {
        self.worktrees
            .values()
            .filter(|worktree| worktree.project_id == project_id)
            .map(|worktree| worktree.name.clone())
            .collect()
    }

    pub fn worktree_directory(&self, worktree_id: WorktreeId) -> Option<PathBuf> {
        let worktree = self.worktree(worktree_id)?;
        let project = self.project(worktree.project_id)?;
        Some(worktree.directory(project).to_path_buf())
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
