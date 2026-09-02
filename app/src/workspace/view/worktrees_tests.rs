use std::path::PathBuf;

use warpui::App;

use super::*;
use crate::projects::{ProjectId, ProjectKind};

fn test_project(root: &str) -> Project {
    Project {
        id: ProjectId::new(),
        root_path: PathBuf::from(root),
        display_name: "repo".to_owned(),
        kind: ProjectKind::Git,
        primary_branch: Some("main".to_owned()),
        created_ts: 1,
        last_opened_ts: 1,
    }
}

fn linked_worktree(project: &Project, path: &str, branch: &str) -> Worktree {
    Worktree {
        id: WorktreeId::new(),
        project_id: project.id,
        name: branch.to_owned(),
        kind: WorktreeKind::Linked {
            path: PathBuf::from(path),
            branch: branch.to_owned(),
            base_branch: "main".to_owned(),
        },
        created_ts: 2,
    }
}

fn entry(path: &str, branch: Option<&str>, prunable: bool) -> WorktreeListEntry {
    WorktreeListEntry {
        path: PathBuf::from(path),
        head: Some("abc123".to_owned()),
        branch: branch.map(str::to_owned),
        is_main: false,
        locked: false,
        prunable,
    }
}

#[test]
fn a_primary_worktree_is_always_kept() {
    let project = test_project("/tmp/spirit-reconcile");
    let primary = Worktree {
        id: WorktreeId::new(),
        project_id: project.id,
        name: "repo".to_owned(),
        kind: WorktreeKind::Primary,
        created_ts: 1,
    };
    assert_eq!(
        reconcile_worktree(&primary, &project, &[]),
        WorktreeReconciliation::Keep
    );
}

#[test]
fn a_worktree_git_forgot_and_disk_lost_is_removed() {
    let project = test_project("/tmp/spirit-reconcile");
    let worktree = linked_worktree(&project, "/tmp/spirit-reconcile-wt/gone", "gone");
    assert_eq!(
        reconcile_worktree(&worktree, &project, &[]),
        WorktreeReconciliation::Remove
    );
}

#[test]
fn a_prunable_worktree_is_removed() {
    let project = test_project("/tmp/spirit-reconcile");
    let worktree = linked_worktree(&project, "/tmp/spirit-reconcile-wt/stale", "stale");
    let entries = vec![entry("/tmp/spirit-reconcile-wt/stale", Some("stale"), true)];
    assert_eq!(
        reconcile_worktree(&worktree, &project, &entries),
        WorktreeReconciliation::Remove
    );
}

#[test]
fn a_branch_switched_outside_spirit_is_recorded() {
    let project = test_project("/tmp/spirit-reconcile");
    let worktree = linked_worktree(&project, "/tmp/spirit-reconcile-wt/auth", "auth");
    let entries = vec![entry(
        "/tmp/spirit-reconcile-wt/auth",
        Some("auth-v2"),
        false,
    )];
    assert_eq!(
        reconcile_worktree(&worktree, &project, &entries),
        WorktreeReconciliation::UpdateBranch("auth-v2".to_owned())
    );
}

#[test]
fn a_listed_worktree_on_its_recorded_branch_is_kept() {
    let project = test_project("/tmp/spirit-reconcile");
    let worktree = linked_worktree(&project, "/tmp/spirit-reconcile-wt/auth", "auth");
    let entries = vec![entry("/tmp/spirit-reconcile-wt/auth", Some("auth"), false)];
    assert_eq!(
        reconcile_worktree(&worktree, &project, &entries),
        WorktreeReconciliation::Keep
    );
}

#[test]
fn run_partition_collapses_consecutive_bindings() {
    let alpha = WorktreeId::new();
    let beta = WorktreeId::new();
    let bindings = [
        None,
        Some(alpha),
        Some(alpha),
        Some(beta),
        None,
        Some(alpha),
    ];
    assert_eq!(
        worktree_run_partition(&bindings),
        vec![
            (None, 1),
            (Some(alpha), 2),
            (Some(beta), 1),
            (None, 1),
            (Some(alpha), 1),
        ]
    );
}

#[test]
fn run_partition_of_unbound_tabs_is_a_single_run() {
    assert_eq!(worktree_run_partition(&[None, None, None]), vec![(None, 3)]);
}

#[test]
fn run_partition_of_no_tabs_is_empty() {
    assert!(worktree_run_partition(&[]).is_empty());
}

#[test]
fn worktree_repo_root_resolves_primary_and_linked_directories_only_for_git_projects() {
    App::test((), |mut app| async move {
        let root = std::env::temp_dir().join(format!(
            "spirit-worktree-root-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let registry = app.add_singleton_model(|_| ProjectRegistryModel::new(None));

        registry.update(&mut app, |registry, ctx| {
            let git_project = registry.register_project(
                root.clone(),
                "repo".to_owned(),
                ProjectKind::Git,
                Some("main".to_owned()),
                ctx,
            );
            let primary = registry
                .primary_worktree_id(git_project)
                .expect("git project has a primary worktree");
            let linked = registry.add_linked_worktree(
                git_project,
                "auth".to_owned(),
                root.join("auth"),
                "auth".to_owned(),
                "main".to_owned(),
                ctx,
            );
            let folder_project = registry.register_project(
                root.join("plain"),
                "plain".to_owned(),
                ProjectKind::Folder,
                None,
                ctx,
            );
            let folder_primary = registry
                .primary_worktree_id(folder_project)
                .expect("folder project has a primary worktree");

            assert_eq!(
                worktree_repo_root(registry, primary),
                Some(dunce::canonicalize(&root).unwrap())
            );
            assert_eq!(
                worktree_repo_root(registry, linked),
                Some(root.join("auth"))
            );
            assert_eq!(worktree_repo_root(registry, folder_primary), None);
            assert_eq!(worktree_repo_root(registry, WorktreeId::new()), None);
        });

        std::fs::remove_dir_all(&root).ok();
    })
}
