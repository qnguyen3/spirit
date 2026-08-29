use super::*;

#[test]
fn sanitize_keeps_safe_characters() {
    assert_eq!(sanitize_worktree_name("fix auth bug"), "fix-auth-bug");
    assert_eq!(sanitize_worktree_name("feature/login"), "feature-login");
    assert_eq!(sanitize_worktree_name("  spaced  out  "), "spaced-out");
    assert_eq!(sanitize_worktree_name("my_branch.v2"), "my_branch.v2");
    assert_eq!(sanitize_worktree_name("café-naïve"), "café-naïve");
}

#[test]
fn sanitize_collapses_dot_runs_and_trims() {
    assert_eq!(sanitize_worktree_name("..hidden.."), "hidden");
    assert_eq!(sanitize_worktree_name("a...b"), "a.b");
    assert_eq!(sanitize_worktree_name("---"), "worktree");
    assert_eq!(sanitize_worktree_name(""), "worktree");
    assert_eq!(sanitize_worktree_name("!!!"), "worktree");
}

#[test]
fn sanitize_never_emits_a_path_separator() {
    for raw in ["a/b", "a\\b", "../../etc/passwd", "nested/deep/name"] {
        let sanitized = sanitize_worktree_name(raw);
        assert!(!sanitized.contains('/'), "{sanitized}");
        assert!(!sanitized.contains('\\'), "{sanitized}");
    }
}

#[test]
fn next_available_suffixes_on_collision() {
    let taken: std::collections::HashSet<String> = ["auth".to_owned(), "auth-2".to_owned()]
        .into_iter()
        .collect();
    let is_taken = |name: &str| taken.contains(name);
    assert_eq!(next_available("auth", &is_taken), "auth-3");
    assert_eq!(next_available("login", &is_taken), "login");
}

#[test]
fn parses_porcelain_stanzas() {
    let porcelain = "worktree /repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /wt/feature\nHEAD def456\nbranch refs/heads/feature\n\nworktree /wt/detached\nHEAD 999999\ndetached\n\n";
    let entries = parse_worktree_list(porcelain);

    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_main);
    assert_eq!(entries[0].branch.as_deref(), Some("main"));
    assert!(!entries[1].is_main);
    assert_eq!(entries[1].path, std::path::PathBuf::from("/wt/feature"));
    assert_eq!(entries[2].branch, None);
    assert_eq!(entries[2].head.as_deref(), Some("999999"));
}

#[test]
fn parses_locked_and_prunable_entries() {
    let porcelain = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /wt/locked\nHEAD def\nbranch refs/heads/locked-one\nlocked keeping this around\n\nworktree /wt/gone\nHEAD ghi\nbranch refs/heads/gone\nprunable gitdir file points to non-existent location\n\n";
    let entries = parse_worktree_list(porcelain);

    assert_eq!(entries.len(), 3);
    assert!(entries[1].locked);
    assert!(!entries[1].prunable);
    assert!(entries[2].prunable);
    assert!(!entries[2].locked);
}

#[test]
fn parses_clone_progress_lines() {
    assert_eq!(
        parse_clone_progress("Receiving objects:  42% (420/1000)"),
        Some(CloneProgress {
            phase: ClonePhase::Receiving,
            percent: Some(42)
        })
    );
    assert_eq!(
        parse_clone_progress("Resolving deltas: 100% (10/10), done."),
        Some(CloneProgress {
            phase: ClonePhase::Resolving,
            percent: Some(100)
        })
    );
    assert_eq!(
        parse_clone_progress("remote: Counting objects: 7% (1/14)"),
        Some(CloneProgress {
            phase: ClonePhase::Counting,
            percent: Some(7)
        })
    );
    assert_eq!(parse_clone_progress("Cloning into 'repo'..."), None);
}

#[test]
fn derives_clone_directory_names() {
    assert_eq!(
        derive_clone_directory_name("https://github.com/owner/repo.git").as_deref(),
        Some("repo")
    );
    assert_eq!(
        derive_clone_directory_name("git@github.com:owner/repo.git").as_deref(),
        Some("repo")
    );
    assert_eq!(
        derive_clone_directory_name("https://github.com/owner/repo/").as_deref(),
        Some("repo")
    );
    assert_eq!(derive_clone_directory_name("   "), None);
}

#[cfg(feature = "local_fs")]
mod with_git {
    use std::path::{Path, PathBuf};

    use super::super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = command::blocking::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Spirit Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Spirit Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git is available");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    struct TempRepo {
        root: PathBuf,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "spirit-git-ops-{name}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let root = std::fs::canonicalize(&root).unwrap();
            Self { root }
        }

        fn init(name: &str, branch: &str) -> Self {
            let repo = Self::new(name);
            git(&repo.root, &["init", "--initial-branch", branch]);
            git(
                &repo.root,
                &["commit", "--allow-empty", "-m", "Initial commit"],
            );
            repo
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        futures::executor::block_on(future)
    }

    #[test]
    fn init_new_project_creates_a_worktree_capable_repo() {
        let scratch = TempRepo::new("init");
        let project = scratch.root.join("brand-new");
        block_on(init_new_project(&project, None)).unwrap();

        assert!(project.join(".git").exists());
        let branch = block_on(current_branch(&project)).unwrap();
        assert!(!branch.is_empty());

        let worktree = scratch.root.join("wt");
        block_on(worktree_add(&project, "spun-off", &worktree, &branch, None)).unwrap();
        assert!(worktree.join(".git").exists());
    }

    #[test]
    fn init_new_project_rejects_a_non_empty_directory() {
        let scratch = TempRepo::new("init-nonempty");
        let project = scratch.root.join("occupied");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("file.txt"), "hello").unwrap();

        assert!(block_on(init_new_project(&project, None)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn worktree_add_runs_checkout_filters_from_the_supplied_path() {
        use std::os::unix::fs::PermissionsExt;

        let repo = TempRepo::init("filter-path", "main");
        let filter_bin = repo.root.join("bin");
        std::fs::create_dir_all(&filter_bin).unwrap();
        let filter = filter_bin.join("spirit-probe-filter");
        std::fs::write(&filter, "#!/bin/sh\ncat\n").unwrap();
        std::fs::set_permissions(&filter, std::fs::Permissions::from_mode(0o755)).unwrap();

        std::fs::write(repo.root.join(".gitattributes"), "*.probe filter=probe\n").unwrap();
        std::fs::write(repo.root.join("payload.probe"), "contents\n").unwrap();
        git(&repo.root, &["config", "filter.probe.clean", "cat"]);
        git(
            &repo.root,
            &["config", "filter.probe.smudge", "spirit-probe-filter"],
        );
        git(&repo.root, &["config", "filter.probe.required", "true"]);
        git(&repo.root, &["add", "-A"]);
        git(&repo.root, &["commit", "-m", "Add filtered file"]);

        let without_path = repo.root.join("wt-without-path");
        assert!(
            block_on(worktree_add_or_rollback(
                &repo.root,
                "without-path",
                &without_path,
                "main",
                None
            ))
            .is_err(),
            "the filter is not on the inherited PATH, so the checkout must fail"
        );
        assert!(
            !git(&repo.root, &["branch", "--list", "without-path"]).contains("without-path"),
            "a failed checkout must not leave its branch behind"
        );

        let with_path = repo.root.join("wt-with-path");
        let path_env = format!(
            "{}:{}",
            filter_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        block_on(worktree_add_or_rollback(
            &repo.root,
            "with-path",
            &with_path,
            "main",
            Some(&path_env),
        ))
        .unwrap();
        assert!(with_path.join("payload.probe").exists());
    }

    #[test]
    fn worktree_add_list_remove_round_trip() {
        let repo = TempRepo::init("round-trip", "main");
        let worktree_path = repo
            .root
            .parent()
            .unwrap()
            .join(format!("spirit-wt-{}", uuid::Uuid::new_v4().simple()));

        block_on(worktree_add(
            &repo.root,
            "feature",
            &worktree_path,
            "main",
            None,
        ))
        .unwrap();

        let entries = block_on(worktree_list(&repo.root)).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_main);
        assert!(
            entries
                .iter()
                .any(|entry| entry.branch.as_deref() == Some("feature"))
        );

        block_on(worktree_remove(&repo.root, &worktree_path, false, None)).unwrap();
        let entries = block_on(worktree_list(&repo.root)).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!worktree_path.exists());
    }

    #[test]
    fn dirty_detection_sees_modified_and_untracked_files() {
        let repo = TempRepo::init("dirty", "main");
        assert!(!block_on(status_is_dirty(&repo.root, None)).unwrap());

        std::fs::write(repo.root.join("untracked.txt"), "hi").unwrap();
        assert!(block_on(status_is_dirty(&repo.root, None)).unwrap());

        git(&repo.root, &["add", "untracked.txt"]);
        git(&repo.root, &["commit", "-m", "add file"]);
        assert!(!block_on(status_is_dirty(&repo.root, None)).unwrap());

        std::fs::write(repo.root.join("untracked.txt"), "changed").unwrap();
        assert!(block_on(status_is_dirty(&repo.root, None)).unwrap());
    }

    #[test]
    fn safe_branch_delete_keeps_unmerged_work() {
        let repo = TempRepo::init("branch-delete", "main");

        git(&repo.root, &["checkout", "-b", "merged"]);
        git(&repo.root, &["checkout", "main"]);
        assert_eq!(
            block_on(delete_branch_safe(&repo.root, "merged")).unwrap(),
            BranchDeleteOutcome::Deleted
        );

        git(&repo.root, &["checkout", "-b", "unmerged"]);
        std::fs::write(repo.root.join("work.txt"), "work").unwrap();
        git(&repo.root, &["add", "work.txt"]);
        git(&repo.root, &["commit", "-m", "unmerged work"]);
        git(&repo.root, &["checkout", "main"]);

        assert_eq!(
            block_on(delete_branch_safe(&repo.root, "unmerged")).unwrap(),
            BranchDeleteOutcome::KeptUnmerged
        );
        assert!(block_on(local_branches(&repo.root)).contains("unmerged"));

        block_on(force_delete_branch(&repo.root, "unmerged")).unwrap();
        assert!(!block_on(local_branches(&repo.root)).contains("unmerged"));
    }

    #[test]
    fn discover_repo_root_resolves_subdirectories_and_linked_worktrees() {
        let repo = TempRepo::init("discover", "main");
        let nested = repo.root.join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();

        let from_nested = block_on(discover_repo_root(&nested)).unwrap().unwrap();
        assert!(same_path(&from_nested.root, &repo.root));
        assert!(!from_nested.resolved_from_linked_worktree);

        let worktree_path = repo
            .root
            .parent()
            .unwrap()
            .join(format!("spirit-wt-{}", uuid::Uuid::new_v4().simple()));
        block_on(worktree_add(
            &repo.root,
            "side",
            &worktree_path,
            "main",
            None,
        ))
        .unwrap();

        let from_worktree = block_on(discover_repo_root(&worktree_path))
            .unwrap()
            .unwrap();
        assert!(same_path(&from_worktree.root, &repo.root));
        assert!(from_worktree.resolved_from_linked_worktree);

        block_on(worktree_remove(&repo.root, &worktree_path, true, None)).unwrap();
    }

    #[test]
    fn discover_repo_root_returns_none_outside_a_repo() {
        let scratch = TempRepo::new("not-a-repo");
        assert!(
            block_on(discover_repo_root(&scratch.root))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn primary_branch_detection_reads_the_local_branch() {
        for branch in ["main", "master", "trunk"] {
            let repo = TempRepo::init("primary", branch);
            let detected = block_on(detect_primary_branch(&repo.root)).unwrap();
            assert_eq!(detected, branch);
        }
    }
}
