use warpui::{App, ModelHandle};

use super::*;

fn registry(app: &mut App) -> ModelHandle<ProjectRegistryModel> {
    app.add_singleton_model(|_| ProjectRegistryModel::new(None))
}

fn scratch_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "spirit-registry-{name}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn register_dedupes_by_root_and_creates_a_primary_worktree() {
    App::test((), |mut app| async move {
        let root = scratch_dir("dedupe");
        let handle = registry(&mut app);

        let first = handle.update(&mut app, |model, ctx| {
            model.register_project(
                root.clone(),
                "repo".to_owned(),
                ProjectKind::Git,
                Some("main".to_owned()),
                ctx,
            )
        });
        let second = handle.update(&mut app, |model, ctx| {
            model.register_project(
                root.clone(),
                "repo-again".to_owned(),
                ProjectKind::Git,
                None,
                ctx,
            )
        });

        assert_eq!(first, second);
        handle.update(&mut app, |model, _| {
            assert_eq!(model.projects_mru().len(), 1);
            let worktrees = model.worktrees_for_project(first);
            assert_eq!(worktrees.len(), 1);
            assert!(worktrees[0].is_primary());
            assert_eq!(model.primary_worktree_id(first), Some(worktrees[0].id));
        });

        std::fs::remove_dir_all(&root).ok();
    })
}

#[test]
fn primary_worktree_directory_is_the_project_root() {
    App::test((), |mut app| async move {
        let root = scratch_dir("primary-dir");
        let handle = registry(&mut app);

        let project_id = handle.update(&mut app, |model, ctx| {
            model.register_project(
                root.clone(),
                "repo".to_owned(),
                ProjectKind::Git,
                Some("main".to_owned()),
                ctx,
            )
        });

        handle.update(&mut app, |model, _| {
            let primary = model.primary_worktree_id(project_id).unwrap();
            let directory = model.worktree_directory(primary).unwrap();
            assert_eq!(directory, model.project(project_id).unwrap().root_path);
        });

        std::fs::remove_dir_all(&root).ok();
    })
}

#[test]
fn remove_project_cascades_worktrees() {
    App::test((), |mut app| async move {
        let root = scratch_dir("cascade");
        let handle = registry(&mut app);

        let project_id = handle.update(&mut app, |model, ctx| {
            model.register_project(
                root.clone(),
                "repo".to_owned(),
                ProjectKind::Git,
                Some("main".to_owned()),
                ctx,
            )
        });
        handle.update(&mut app, |model, ctx| {
            model.add_linked_worktree(
                project_id,
                "feature".to_owned(),
                root.join("wt"),
                "feature".to_owned(),
                "main".to_owned(),
                ctx,
            )
        });

        handle.update(&mut app, |model, _| {
            assert_eq!(model.worktrees_for_project(project_id).len(), 2);
            assert_eq!(model.linked_worktree_count(project_id), 1);
        });

        handle.update(&mut app, |model, ctx| model.remove_project(project_id, ctx));
        handle.update(&mut app, |model, _| {
            assert!(model.project(project_id).is_none());
            assert!(model.worktrees_for_project(project_id).is_empty());
        });

        std::fs::remove_dir_all(&root).ok();
    })
}

#[test]
fn remove_worktree_refuses_primary() {
    App::test((), |mut app| async move {
        let root = scratch_dir("refuse-primary");
        let handle = registry(&mut app);

        let project_id = handle.update(&mut app, |model, ctx| {
            model.register_project(
                root.clone(),
                "repo".to_owned(),
                ProjectKind::Git,
                Some("main".to_owned()),
                ctx,
            )
        });

        handle.update(&mut app, |model, ctx| {
            let primary = model.primary_worktree_id(project_id).unwrap();
            assert!(model.remove_worktree(primary, ctx).is_err());
            assert!(model.worktree(primary).is_some());
        });

        std::fs::remove_dir_all(&root).ok();
    })
}

#[test]
fn remove_worktree_drops_linked_worktrees() {
    App::test((), |mut app| async move {
        let root = scratch_dir("remove-linked");
        let handle = registry(&mut app);

        let project_id = handle.update(&mut app, |model, ctx| {
            model.register_project(
                root.clone(),
                "repo".to_owned(),
                ProjectKind::Git,
                Some("main".to_owned()),
                ctx,
            )
        });
        let worktree_id = handle.update(&mut app, |model, ctx| {
            model.add_linked_worktree(
                project_id,
                "feature".to_owned(),
                root.join("wt"),
                "feature".to_owned(),
                "main".to_owned(),
                ctx,
            )
        });

        handle.update(&mut app, |model, ctx| {
            assert!(model.remove_worktree(worktree_id, ctx).is_ok());
            assert!(model.worktree(worktree_id).is_none());
            assert_eq!(model.linked_worktree_count(project_id), 0);
        });

        std::fs::remove_dir_all(&root).ok();
    })
}

#[test]
fn load_repairs_missing_primary_and_orphan_rows() {
    App::test((), |mut app| async move {
        let handle = registry(&mut app);
        let project_id = ProjectId::new();
        let orphan_project_id = ProjectId::new();

        let projects = vec![Project {
            id: project_id,
            root_path: PathBuf::from("/tmp/spirit-load-repair"),
            display_name: "repo".to_owned(),
            kind: ProjectKind::Git,
            primary_branch: Some("main".to_owned()),
            created_ts: 1,
            last_opened_ts: 1,
        }];
        let worktrees = vec![
            Worktree {
                id: WorktreeId::new(),
                project_id,
                name: "feature".to_owned(),
                kind: WorktreeKind::Linked {
                    path: PathBuf::from("/tmp/spirit-load-repair-wt"),
                    branch: "feature".to_owned(),
                    base_branch: "main".to_owned(),
                },
                created_ts: 2,
            },
            Worktree {
                id: WorktreeId::new(),
                project_id: orphan_project_id,
                name: "orphan".to_owned(),
                kind: WorktreeKind::Primary,
                created_ts: 3,
            },
        ];

        handle.update(&mut app, |model, _| model.load(projects, worktrees));

        handle.update(&mut app, |model, _| {
            let worktrees = model.worktrees_for_project(project_id);
            assert_eq!(worktrees.len(), 2);
            assert!(worktrees[0].is_primary());
            assert_eq!(worktrees[1].name, "feature");
            assert!(model.worktrees_for_project(orphan_project_id).is_empty());
        });
    })
}

#[test]
fn load_drops_duplicate_primary_worktrees() {
    App::test((), |mut app| async move {
        let handle = registry(&mut app);
        let project_id = ProjectId::new();

        let primary = |created_ts| Worktree {
            id: WorktreeId::new(),
            project_id,
            name: "repo".to_owned(),
            kind: WorktreeKind::Primary,
            created_ts,
        };

        let projects = vec![Project {
            id: project_id,
            root_path: PathBuf::from("/tmp/spirit-load-duplicate"),
            display_name: "repo".to_owned(),
            kind: ProjectKind::Folder,
            primary_branch: None,
            created_ts: 1,
            last_opened_ts: 1,
        }];

        handle.update(&mut app, |model, _| {
            model.load(projects, vec![primary(1), primary(2), primary(3)])
        });
        handle.update(&mut app, |model, _| {
            assert_eq!(model.worktrees_for_project(project_id).len(), 1);
        });
    })
}
