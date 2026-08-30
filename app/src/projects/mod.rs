pub mod agent_status;
pub mod create_worktree_modal;
pub mod delete_worktree_dialog;
pub mod git_ops;
pub mod host;
pub mod new_workspace_modal;
pub mod overview;
pub mod registry;
pub mod remove_workspace_dialog;
pub mod settings;

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::BoxFuture;
use persistence::model::{Project as ProjectRow, ProjectWorktree as WorktreeRow};
use uuid::Uuid;
use warpui::AppContext;

// A GUI launch inherits only `/usr/bin:/bin:/usr/sbin:/sbin`, so git cannot find the binaries
// its repo-configured filters and hooks need: a checkout in a Git LFS repository dies with
// "git-lfs: command not found" unless the commands run with this PATH.
pub fn interactive_path_env(ctx: &mut AppContext) -> BoxFuture<'static, Option<String>> {
    #[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
    {
        use warpui::SingletonEntity;

        use crate::terminal::local_shell::LocalShellState;

        if ctx.has_singleton_model::<LocalShellState>() {
            return LocalShellState::handle(ctx).update(ctx, |shell_state, ctx| {
                shell_state.get_interactive_path_env_var(ctx)
            });
        }
    }
    let _ = ctx;
    futures::future::ready(None).boxed()
}

const ERROR_SUMMARY_MAX_CHARS: usize = 220;

pub fn error_summary(err: &anyhow::Error) -> String {
    let flattened = format!("{err:#}");
    let mut summary = flattened.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.chars().count() > ERROR_SUMMARY_MAX_CHARS {
        summary = summary
            .chars()
            .take(ERROR_SUMMARY_MAX_CHARS)
            .collect::<String>()
            + "\u{2026}";
    }
    summary
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorktreeId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectKind {
    Git,
    Folder,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: ProjectId,
    pub root_path: PathBuf,
    pub display_name: String,
    pub kind: ProjectKind,
    pub primary_branch: Option<String>,
    pub created_ts: i64,
    pub last_opened_ts: i64,
}

#[derive(Debug, Clone)]
pub enum WorktreeKind {
    Primary,
    Linked {
        path: PathBuf,
        branch: String,
        base_branch: String,
    },
}

#[derive(Debug, Clone)]
pub struct Worktree {
    pub id: WorktreeId,
    pub project_id: ProjectId,
    pub name: String,
    pub kind: WorktreeKind,
    pub created_ts: i64,
}

impl ProjectId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl WorktreeId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl fmt::Display for WorktreeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl FromStr for ProjectId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

impl FromStr for WorktreeId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

impl ProjectKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            ProjectKind::Git => "git",
            ProjectKind::Folder => "folder",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "git" => Some(ProjectKind::Git),
            "folder" => Some(ProjectKind::Folder),
            _ => None,
        }
    }
}

impl Project {
    pub fn display_name_for_root(root_path: &Path) -> String {
        root_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| root_path.display().to_string())
    }
}

impl Worktree {
    pub fn directory<'a>(&'a self, project: &'a Project) -> &'a Path {
        match &self.kind {
            WorktreeKind::Primary => project.root_path.as_path(),
            WorktreeKind::Linked { path, .. } => path.as_path(),
        }
    }

    pub fn is_primary(&self) -> bool {
        matches!(self.kind, WorktreeKind::Primary)
    }

    pub fn branch(&self) -> Option<&str> {
        match &self.kind {
            WorktreeKind::Primary => None,
            WorktreeKind::Linked { branch, .. } => Some(branch.as_str()),
        }
    }
}

impl From<&Project> for ProjectRow {
    fn from(project: &Project) -> Self {
        Self {
            id: project.id.to_string(),
            root_path: project.root_path.to_string_lossy().to_string(),
            display_name: project.display_name.clone(),
            kind: project.kind.as_db_str().to_owned(),
            primary_branch: project.primary_branch.clone(),
            created_ts: project.created_ts,
            last_opened_ts: project.last_opened_ts,
        }
    }
}

impl TryFrom<ProjectRow> for Project {
    type Error = anyhow::Error;

    fn try_from(row: ProjectRow) -> Result<Self> {
        let kind = ProjectKind::from_db_str(&row.kind)
            .ok_or_else(|| anyhow!("unknown project kind {:?}", row.kind))?;
        Ok(Self {
            id: row.id.parse()?,
            root_path: PathBuf::from(row.root_path),
            display_name: row.display_name,
            kind,
            primary_branch: row.primary_branch,
            created_ts: row.created_ts,
            last_opened_ts: row.last_opened_ts,
        })
    }
}

impl From<&Worktree> for WorktreeRow {
    fn from(worktree: &Worktree) -> Self {
        let (kind, path, branch, base_branch) = match &worktree.kind {
            WorktreeKind::Primary => ("primary", None, None, None),
            WorktreeKind::Linked {
                path,
                branch,
                base_branch,
            } => (
                "linked",
                Some(path.to_string_lossy().to_string()),
                Some(branch.clone()),
                Some(base_branch.clone()),
            ),
        };
        Self {
            id: worktree.id.to_string(),
            project_id: worktree.project_id.to_string(),
            name: worktree.name.clone(),
            kind: kind.to_owned(),
            path,
            branch,
            base_branch,
            created_ts: worktree.created_ts,
        }
    }
}

impl TryFrom<WorktreeRow> for Worktree {
    type Error = anyhow::Error;

    fn try_from(row: WorktreeRow) -> Result<Self> {
        let kind = match row.kind.as_str() {
            "primary" => WorktreeKind::Primary,
            "linked" => WorktreeKind::Linked {
                path: PathBuf::from(
                    row.path
                        .ok_or_else(|| anyhow!("linked worktree row is missing its path"))?,
                ),
                branch: row
                    .branch
                    .ok_or_else(|| anyhow!("linked worktree row is missing its branch"))?,
                base_branch: row
                    .base_branch
                    .ok_or_else(|| anyhow!("linked worktree row is missing its base branch"))?,
            },
            other => return Err(anyhow!("unknown worktree kind {other:?}")),
        };
        Ok(Self {
            id: row.id.parse()?,
            project_id: row.project_id.parse()?,
            name: row.name,
            kind,
            created_ts: row.created_ts,
        })
    }
}
