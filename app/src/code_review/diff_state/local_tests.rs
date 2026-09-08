use super::*;
use crate::util::git::{
    BranchEntry, parse_range, parse_unified_diff_header, sort_branches_main_first,
};

#[test]
fn test_parse_range_with_comma() {
    let (start, count) =
        parse_range("10,5").expect("parse_range should succeed for range with count");
    assert_eq!(start, 10);
    assert_eq!(count, 5);
}

#[test]
fn test_parse_range_without_comma() {
    let (start, count) =
        parse_range("10").expect("parse_range should succeed for range without count");
    assert_eq!(start, 10);
    assert_eq!(count, 1);
}

#[test]
fn test_parse_unified_diff_header_basic() {
    let header = "@@ -10,5 +12,7 @@";
    let parsed = parse_unified_diff_header(header)
        .expect("parse_unified_diff_header should succeed for basic header");
    assert_eq!(parsed.old_start_line, 10);
    assert_eq!(parsed.old_line_count, 5);
    assert_eq!(parsed.new_start_line, 12);
    assert_eq!(parsed.new_line_count, 7);
}

#[test]
fn test_parse_unified_diff_header_with_context() {
    let header = "@@ -4978,33 +4978,43 @@ impl TerminalView {";
    let parsed = parse_unified_diff_header(header)
        .expect("parse_unified_diff_header should succeed for header with context");
    assert_eq!(parsed.old_start_line, 4978);
    assert_eq!(parsed.old_line_count, 33);
    assert_eq!(parsed.new_start_line, 4978);
    assert_eq!(parsed.new_line_count, 43);
}

#[test]
fn test_parse_unified_diff_header_single_line() {
    let header = "@@ -10 +12,3 @@";
    let parsed = parse_unified_diff_header(header)
        .expect("parse_unified_diff_header should succeed for single line header");
    assert_eq!(parsed.old_start_line, 10);
    assert_eq!(parsed.old_line_count, 1);
    assert_eq!(parsed.new_start_line, 12);
    assert_eq!(parsed.new_line_count, 3);
}

#[test]
fn test_sort_branches_main_first_empty() {
    let branches: Vec<BranchEntry> = vec![];
    let result: Vec<_> = sort_branches_main_first(&branches).collect();
    assert!(result.is_empty());
}

#[test]
fn test_sort_branches_main_first_no_main() {
    let branches = vec![
        BranchEntry {
            name: "feature-a".to_string(),
            is_main: false,
        },
        BranchEntry {
            name: "feature-b".to_string(),
            is_main: false,
        },
        BranchEntry {
            name: "feature-c".to_string(),
            is_main: false,
        },
    ];
    let result: Vec<_> = sort_branches_main_first(&branches).collect();
    // No main branches — order should be unchanged.
    assert_eq!(result, branches.iter().collect::<Vec<_>>());
}

#[test]
fn test_sort_branches_main_first_promotes_main() {
    let branches = vec![
        BranchEntry {
            name: "feature-a".to_string(),
            is_main: false,
        },
        BranchEntry {
            name: "main".to_string(),
            is_main: true,
        },
        BranchEntry {
            name: "feature-b".to_string(),
            is_main: false,
        },
    ];
    let result: Vec<_> = sort_branches_main_first(&branches)
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(result, vec!["main", "feature-a", "feature-b"]);
}

#[test]
fn test_sort_branches_main_first_main_already_first() {
    let branches = vec![
        BranchEntry {
            name: "main".to_string(),
            is_main: true,
        },
        BranchEntry {
            name: "feature-a".to_string(),
            is_main: false,
        },
        BranchEntry {
            name: "feature-b".to_string(),
            is_main: false,
        },
    ];
    let result: Vec<_> = sort_branches_main_first(&branches)
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(result, vec!["main", "feature-a", "feature-b"]);
}

#[test]
fn test_sort_branches_main_first_preserves_recency_order_for_non_main() {
    // Non-main branches should remain in their original (recency) order.
    let branches = vec![
        BranchEntry {
            name: "recent-feature".to_string(),
            is_main: false,
        },
        BranchEntry {
            name: "main".to_string(),
            is_main: true,
        },
        BranchEntry {
            name: "older-feature".to_string(),
            is_main: false,
        },
        BranchEntry {
            name: "oldest-feature".to_string(),
            is_main: false,
        },
    ];
    let result: Vec<_> = sort_branches_main_first(&branches)
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(
        result,
        vec!["main", "recent-feature", "older-feature", "oldest-feature"]
    );
}

#[test]
fn test_sort_branches_main_first_multiple_main_flags() {
    // Defensive: both flagged as main (shouldn't happen in practice, but
    // sort_branches_main_first should handle it gracefully).
    let branches = vec![
        BranchEntry {
            name: "feature".to_string(),
            is_main: false,
        },
        BranchEntry {
            name: "main".to_string(),
            is_main: true,
        },
        BranchEntry {
            name: "master".to_string(),
            is_main: true,
        },
    ];
    let result: Vec<_> = sort_branches_main_first(&branches)
        .map(|entry| entry.name.as_str())
        .collect();
    // Both main-flagged entries appear first, non-main last.
    assert_eq!(result, vec!["main", "master", "feature"]);
}

#[test]
fn test_parse_unified_diff_header_malformed() {
    let header = "not a diff header";
    let result = parse_unified_diff_header(header);
    assert!(result.is_err());

    let header2 = "@@ incomplete";
    let result2 = parse_unified_diff_header(header2);
    assert!(result2.is_err());
}

#[test]
fn test_parse_git_status_modified_file_with_spaces() {
    // Porcelain v2 output for a modified file with spaces in the name.
    // Format: 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
    let status_output = "1 .M N... 100644 100644 100644 abc1234 def5678 test file.txt";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "test file.txt");
    assert_eq!(result[0].1, GitFileStatus::Modified);
}

#[test]
fn test_parse_git_status_modified_file_with_multiple_spaces() {
    // Filename with multiple spaces.
    let status_output = "1 .M N... 100644 100644 100644 abc1234 def5678 path to/my test file.txt";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "path to/my test file.txt");
    assert_eq!(result[0].1, GitFileStatus::Modified);
}

#[test]
fn test_parse_git_status_new_file_with_spaces() {
    let status_output = "1 A. N... 000000 100644 100644 0000000 abc1234 new file name.rs";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "new file name.rs");
    assert_eq!(result[0].1, GitFileStatus::New);
}

#[test]
fn test_parse_git_status_renamed_file_with_spaces() {
    // Porcelain v2 renamed entry (type 2) with spaces in the new path.
    // Format: 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>\0<origPath>
    let status_output =
        "2 R. N... 100644 100644 100644 abc1234 def5678 R100 new name.txt\0old name.txt";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "new name.txt");
    assert!(matches!(
        &result[0].1,
        GitFileStatus::Renamed { old_path } if old_path == "old name.txt"
    ));
}

#[test]
fn test_parse_git_status_untracked_file_with_spaces() {
    let status_output = "? my untracked file.txt";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "my untracked file.txt");
    assert_eq!(result[0].1, GitFileStatus::Untracked);
}

#[test]
fn test_parse_git_status_unmerged_file_with_spaces() {
    // Porcelain v2 unmerged entry (type u) with spaces in the path.
    // Format: u <xy> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
    let status_output =
        "u UU N... 100644 100644 100644 100644 abc1234 def5678 ghi9012 conflict file.txt";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "conflict file.txt");
    assert_eq!(result[0].1, GitFileStatus::Conflicted);
}

#[test]
fn test_parse_git_status_mixed_entries_with_spaces() {
    // Multiple entries separated by NUL, mixing files with and without spaces.
    let status_output = "1 .M N... 100644 100644 100644 abc1234 def5678 test file.txt\0\
         1 .M N... 100644 100644 100644 abc1234 def5678 normal.txt\0\
         ? another file with spaces.rs";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].0, "test file.txt");
    assert_eq!(result[1].0, "normal.txt");
    assert_eq!(result[2].0, "another file with spaces.rs");
}

#[test]
fn test_parse_git_status_file_without_spaces_still_works() {
    // Ensure the splitn change doesn't break files without spaces.
    let status_output = "1 .M N... 100644 100644 100644 abc1234 def5678 simple.txt";
    let result = LocalDiffStateModel::parse_git_status(status_output).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "simple.txt");
    assert_eq!(result[0].1, GitFileStatus::Modified);
}

#[tokio::test]
async fn untracked_directory_diff_is_empty_and_non_binary() {
    let repo_dir = tempfile::tempdir().expect("create temp repo dir");
    std::fs::create_dir(repo_dir.path().join("nested-repo")).expect("create nested dir");

    // `git status` reports a nested repo/worktree as a single untracked
    // directory entry (with a trailing slash). It must short-circuit to an
    // empty non-binary diff — the error fallback would otherwise mislabel it
    // as binary and the view would render "Binary file - no diff available"
    // instead of "New empty file".
    let diff = LocalDiffStateModel::get_file_diff(
        repo_dir.path(),
        "nested-repo/",
        &GitFileStatus::Untracked,
        false,
        None,
    )
    .await
    .expect("get_file_diff should succeed for an untracked directory");

    assert!(!diff.is_binary);
    assert_eq!(diff.hunks.len(), 0);
    assert_eq!(diff.status, GitFileStatus::Untracked);
}

#[tokio::test]
async fn untracked_directory_has_no_baseline_content() {
    let repo_dir = tempfile::tempdir().expect("create temp repo dir");
    std::fs::create_dir(repo_dir.path().join("nested-repo")).expect("create nested dir");
    std::fs::write(repo_dir.path().join("new-file.txt"), "hello\n").expect("write file");

    // No baseline for a directory entry, so no editor is constructed for it.
    let dir_content = LocalDiffStateModel::get_file_content_at_head(
        repo_dir.path(),
        "nested-repo/",
        &GitFileStatus::Untracked,
    )
    .await;
    assert_eq!(dir_content, None);

    // Regular untracked files keep their empty baseline.
    let file_content = LocalDiffStateModel::get_file_content_at_head(
        repo_dir.path(),
        "new-file.txt",
        &GitFileStatus::Untracked,
    )
    .await;
    assert_eq!(file_content, Some(String::new()));
}

#[tokio::test]
async fn renamed_file_content_at_head_reads_old_path() {
    let repo_dir = tempfile::tempdir().expect("create temp repo dir");
    let repo_path = repo_dir.path();

    // Set up a real git repo with one committed file, then rename it in the working tree
    // (without committing the rename) so HEAD only knows about the old path.
    run_git_command(repo_path, &["init", "-b", "main"])
        .await
        .expect("git init");
    run_git_command(repo_path, &["config", "user.email", "test@test.com"])
        .await
        .expect("git config email");
    run_git_command(repo_path, &["config", "user.name", "Test"])
        .await
        .expect("git config name");
    std::fs::write(repo_path.join("old.txt"), "hello world\n").expect("write old.txt");
    run_git_command(repo_path, &["add", "old.txt"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");

    // Rename in the working tree only — `old.txt` no longer exists at this path, so `git
    // show HEAD:new.txt` would fail (the bug in APP-5111).
    std::fs::rename(repo_path.join("old.txt"), repo_path.join("new.txt"))
        .expect("rename old.txt to new.txt");

    let content = LocalDiffStateModel::get_file_content_at_head(
        repo_path,
        "new.txt",
        &GitFileStatus::Renamed {
            old_path: "old.txt".to_string(),
        },
    )
    .await;

    // The baseline content at HEAD must come from the old path, not the new one, so the code
    // review pane can render a diff instead of "Unable to load file content".
    assert_eq!(content, Some("hello world\n".to_string()));
}

#[tokio::test]
async fn staged_rename_and_modify_produces_non_empty_diff() {
    let repo_dir = tempfile::tempdir().expect("create temp repo dir");
    let repo_path = repo_dir.path();

    run_git_command(repo_path, &["init", "-b", "main"])
        .await
        .expect("git init");
    run_git_command(repo_path, &["config", "user.email", "test@test.com"])
        .await
        .expect("git config email");
    run_git_command(repo_path, &["config", "user.name", "Test"])
        .await
        .expect("git config name");
    std::fs::write(
        repo_path.join("old.txt"),
        "line one\nline two\nline three\n",
    )
    .expect("write old.txt");
    run_git_command(repo_path, &["add", "old.txt"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");

    // Stage both the rename and a content edit, so nothing is left unstaged (git status
    // reports this as a plain "R " entry with no unstaged component).
    run_git_command(repo_path, &["mv", "old.txt", "new.txt"])
        .await
        .expect("git mv");
    std::fs::write(
        repo_path.join("new.txt"),
        "line one\nline two changed\nline three\n",
    )
    .expect("write new.txt");
    run_git_command(repo_path, &["add", "new.txt"])
        .await
        .expect("git add new.txt");

    let diff = LocalDiffStateModel::get_file_diff(
        repo_path,
        "new.txt",
        &GitFileStatus::Renamed {
            old_path: "old.txt".to_string(),
        },
        false,
        None,
    )
    .await
    .expect("get_file_diff should succeed for a fully staged rename+modify");

    // A fully staged rename with a staged content edit must still render an inline diff
    // instead of falling through to "File renamed without changes": comparing only the
    // index against the working tree (as before the fix) produced an empty diff here,
    // since both changes were already staged.
    assert!(
        !diff.is_empty(),
        "expected a non-empty diff for a fully staged rename+modify"
    );
}

#[tokio::test]
async fn num_lines_in_file_if_non_binary_counts_lines_in_text_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let file_path = dir.path().join("file.txt");
    std::fs::write(&file_path, "one\ntwo\nthree\n").expect("write file");

    let num_lines = LocalDiffStateModel::num_lines_in_file_if_non_binary(&file_path)
        .await
        .expect("counting a regular file should succeed");
    assert_eq!(num_lines, Some(3));
}

#[tokio::test]
async fn num_lines_in_file_if_non_binary_errors_for_directory() {
    let dir = tempfile::tempdir().expect("create temp dir");

    // Directories aren't countable. The metadata callers degrade this error
    // to a 0-line contribution per entry instead of failing the whole
    // metadata computation.
    let result = LocalDiffStateModel::num_lines_in_file_if_non_binary(dir.path()).await;
    assert!(result.is_err());
}

#[cfg(feature = "local_fs")]
async fn await_repo_detection(
    app: &mut warpui::App,
    model: &warpui::ModelHandle<LocalDiffStateModel>,
) {
    let completion = model.update(app, |model, ctx| {
        let future_id = model
            .repo_detection_handle
            .as_ref()
            .expect("detection is in flight")
            .future_id();
        ctx.await_spawned_future(future_id)
    });
    completion.await;
}

#[cfg(feature = "local_fs")]
#[test]
fn not_in_repository_recovers_when_repo_appears_on_reload() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());

        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let project = temp_dir.path().join("project");
        std::fs::create_dir_all(&project).expect("create project dir");

        let model = app.add_model(|ctx| {
            LocalDiffStateModel::new(Some(project.to_string_lossy().into_owned()), ctx)
        });
        await_repo_detection(&mut app, &model).await;
        model.read(&app, |model, _| {
            assert!(model.repository.is_none());
            assert!(matches!(model.state, InternalDiffState::NotInRepository));
        });

        let git_dir = project.join(".git");
        std::fs::create_dir_all(git_dir.join("objects")).expect("create objects");
        std::fs::create_dir_all(git_dir.join("refs")).expect("create refs");
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("write HEAD");
        std::fs::write(git_dir.join("config"), "").expect("write config");

        model.update(&mut app, |model, ctx| {
            model.add_refresh_consumer(DiffRefreshConsumer::RemoteSubscribers, ctx);
            model.load_diffs_for_current_repo(false, false, ctx);
            assert!(matches!(model.state, InternalDiffState::Detecting));
        });
        await_repo_detection(&mut app, &model).await;
        model.read(&app, |model, _| {
            assert!(
                model.repository.is_some(),
                "reloading after the folder became a repository picks the repository up"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
async fn init_discard_repo() -> tempfile::TempDir {
    let repo_dir = tempfile::tempdir().expect("create temp repo dir");
    let repo_path = repo_dir.path();
    run_git_command(repo_path, &["init", "-b", "main"])
        .await
        .expect("git init");
    run_git_command(repo_path, &["config", "user.email", "test@test.com"])
        .await
        .expect("git config email");
    run_git_command(repo_path, &["config", "user.name", "Test"])
        .await
        .expect("git config name");
    repo_dir
}

#[cfg(feature = "local_fs")]
async fn git_status_short(repo_path: &Path) -> String {
    run_git_command(repo_path, &["status", "--short"])
        .await
        .expect("git status")
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_tracked_and_untracked_together_restores_the_tracked_file() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("tracked.txt"), "original\n").expect("write tracked.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    std::fs::write(repo_path.join("tracked.txt"), "edited\n").expect("edit tracked.txt");
    std::fs::write(repo_path.join("new.txt"), "new\n").expect("write new.txt");

    LocalDiffStateModel::git_restore_and_clean(
        repo_path,
        &["tracked.txt".to_string(), "new.txt".to_string()],
        "HEAD",
    )
    .await
    .expect("discard should succeed");

    assert_eq!(
        std::fs::read_to_string(repo_path.join("tracked.txt")).expect("tracked.txt should exist"),
        "original\n"
    );
    assert!(!repo_path.join("new.txt").exists());
    assert_eq!(git_status_short(repo_path).await, "");
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_a_staged_new_file_alongside_a_tracked_edit_removes_only_the_new_file() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("tracked.txt"), "original\n").expect("write tracked.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    std::fs::write(repo_path.join("tracked.txt"), "edited\n").expect("edit tracked.txt");
    std::fs::write(repo_path.join("staged_new.txt"), "staged\n").expect("write staged_new.txt");
    run_git_command(repo_path, &["add", "staged_new.txt"])
        .await
        .expect("git add staged_new.txt");

    LocalDiffStateModel::git_restore_and_clean(
        repo_path,
        &["tracked.txt".to_string(), "staged_new.txt".to_string()],
        "HEAD",
    )
    .await
    .expect("discard should succeed");

    assert_eq!(
        std::fs::read_to_string(repo_path.join("tracked.txt")).expect("tracked.txt should exist"),
        "original\n"
    );
    assert!(!repo_path.join("staged_new.txt").exists());
    assert_eq!(git_status_short(repo_path).await, "");
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_against_another_branch_restores_only_paths_that_branch_has() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("on_base.txt"), "base\n").expect("write on_base.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    run_git_command(repo_path, &["checkout", "-b", "feature"])
        .await
        .expect("git checkout -b");
    std::fs::write(repo_path.join("on_base.txt"), "changed\n").expect("edit on_base.txt");
    std::fs::write(repo_path.join("only_on_feature.txt"), "feature\n")
        .expect("write only_on_feature.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add feature files");

    LocalDiffStateModel::git_restore_and_clean(
        repo_path,
        &["on_base.txt".to_string(), "only_on_feature.txt".to_string()],
        "main",
    )
    .await
    .expect("discard should succeed");

    assert_eq!(
        std::fs::read_to_string(repo_path.join("on_base.txt")).expect("on_base.txt should exist"),
        "base\n"
    );
    assert!(!repo_path.join("only_on_feature.txt").exists());
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_with_an_unborn_head_removes_the_selected_paths_only() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("staged.txt"), "staged\n").expect("write staged.txt");
    std::fs::write(repo_path.join("untracked.txt"), "untracked\n").expect("write untracked.txt");
    std::fs::write(repo_path.join("keep.txt"), "keep\n").expect("write keep.txt");
    run_git_command(repo_path, &["add", "staged.txt"])
        .await
        .expect("git add staged.txt");

    LocalDiffStateModel::git_restore_and_clean(
        repo_path,
        &["staged.txt".to_string(), "untracked.txt".to_string()],
        "HEAD",
    )
    .await
    .expect("discard should succeed for an unborn HEAD");

    assert!(!repo_path.join("staged.txt").exists());
    assert!(!repo_path.join("untracked.txt").exists());
    assert!(
        repo_path.join("keep.txt").exists(),
        "a path that was not selected must be left alone"
    );
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_a_deleted_tracked_file_restores_it() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("gone.txt"), "content\n").expect("write gone.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    run_git_command(repo_path, &["rm", "gone.txt"])
        .await
        .expect("git rm");

    LocalDiffStateModel::git_restore_and_clean(repo_path, &["gone.txt".to_string()], "HEAD")
        .await
        .expect("discard should succeed");

    assert_eq!(
        std::fs::read_to_string(repo_path.join("gone.txt")).expect("gone.txt should be restored"),
        "content\n"
    );
    assert_eq!(git_status_short(repo_path).await, "");
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_a_path_with_wildcards_leaves_similarly_named_files_alone() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("seed.txt"), "seed\n").expect("write seed.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    std::fs::write(repo_path.join("wild*card.txt"), "literal\n").expect("write wildcard file");
    std::fs::write(repo_path.join("wildXcard.txt"), "decoy\n").expect("write decoy file");

    LocalDiffStateModel::git_restore_and_clean(repo_path, &["wild*card.txt".to_string()], "HEAD")
        .await
        .expect("discard should succeed");

    assert!(!repo_path.join("wild*card.txt").exists());
    assert!(
        repo_path.join("wildXcard.txt").exists(),
        "the wildcard must be matched literally, not as a glob"
    );
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_a_unicode_path_with_spaces_restores_it() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::create_dir(repo_path.join("dir with space")).expect("create dir");
    std::fs::write(repo_path.join("dir with space/café.txt"), "original\n").expect("write file");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    std::fs::write(repo_path.join("dir with space/café.txt"), "edited\n").expect("edit file");

    LocalDiffStateModel::git_restore_and_clean(
        repo_path,
        &["dir with space/café.txt".to_string()],
        "HEAD",
    )
    .await
    .expect("discard should succeed");

    assert_eq!(
        std::fs::read_to_string(repo_path.join("dir with space/café.txt")).expect("file exists"),
        "original\n"
    );
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn discarding_a_rename_restores_the_old_path_and_removes_the_new_one() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("old.txt"), "content\n").expect("write old.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    run_git_command(repo_path, &["mv", "old.txt", "new.txt"])
        .await
        .expect("git mv");

    LocalDiffStateModel::discard_files_impl(
        &StandardizedPath::from_local_absolute_unchecked(repo_path),
        vec![FileStatusInfo {
            path: StandardizedPath::from_local_absolute_unchecked(&repo_path.join("new.txt")),
            status: GitFileStatus::Renamed {
                old_path: "old.txt".to_string(),
            },
        }],
        false, /* should_stash */
        "HEAD",
    )
    .await
    .expect("discard should succeed");

    assert_eq!(
        std::fs::read_to_string(repo_path.join("old.txt")).expect("old.txt should be restored"),
        "content\n"
    );
    assert!(!repo_path.join("new.txt").exists());
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn stashing_discard_keeps_working_tree_recoverable() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    std::fs::write(repo_path.join("tracked.txt"), "original\n").expect("write tracked.txt");
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", "initial"])
        .await
        .expect("git commit");
    std::fs::write(repo_path.join("tracked.txt"), "edited\n").expect("edit tracked.txt");
    std::fs::write(repo_path.join("new.txt"), "new\n").expect("write new.txt");

    LocalDiffStateModel::discard_files_impl(
        &StandardizedPath::from_local_absolute_unchecked(repo_path),
        vec![
            FileStatusInfo {
                path: StandardizedPath::from_local_absolute_unchecked(
                    &repo_path.join("tracked.txt"),
                ),
                status: GitFileStatus::Modified,
            },
            FileStatusInfo {
                path: StandardizedPath::from_local_absolute_unchecked(&repo_path.join("new.txt")),
                status: GitFileStatus::Untracked,
            },
        ],
        true, /* should_stash */
        "HEAD",
    )
    .await
    .expect("stashing discard should succeed");

    assert_eq!(
        std::fs::read_to_string(repo_path.join("tracked.txt")).expect("tracked.txt should exist"),
        "original\n"
    );
    assert!(!repo_path.join("new.txt").exists());
    let stash_list = run_git_command(repo_path, &["stash", "list"])
        .await
        .expect("git stash list");
    assert!(
        !stash_list.trim().is_empty(),
        "the discarded changes must be recoverable from the stash"
    );
}

#[cfg(feature = "local_fs")]
fn modified_files_update(paths: &[&str], is_ignored: bool) -> DiffStateRepositoryUpdate {
    modified_files_update_with_lock(paths, is_ignored, false)
}

#[cfg(feature = "local_fs")]
fn modified_files_update_with_lock(
    paths: &[&str],
    is_ignored: bool,
    index_lock_held: bool,
) -> DiffStateRepositoryUpdate {
    DiffStateRepositoryUpdate {
        update: RepositoryUpdate {
            modified: paths
                .iter()
                .map(|path| {
                    repo_metadata::watcher::TargetFile::new(PathBuf::from(path), is_ignored)
                })
                .collect(),
            ..Default::default()
        },
        index_lock_held,
    }
}

#[cfg(feature = "local_fs")]
fn index_lock_update(index_lock_held: bool) -> DiffStateRepositoryUpdate {
    DiffStateRepositoryUpdate {
        update: RepositoryUpdate {
            index_lock_detected: true,
            ..Default::default()
        },
        index_lock_held,
    }
}

#[cfg(feature = "local_fs")]
fn empty_diffs() -> DiffsWithBaseContent {
    DiffsWithBaseContent {
        changes: Ok(GitDiffWithBaseContent {
            files: Vec::new(),
            total_additions: 0,
            total_deletions: 0,
            files_changed: 0,
        }),
    }
}

#[cfg(feature = "local_fs")]
async fn model_watching_a_repo(
    app: &mut warpui::App,
    temp_dir: &tempfile::TempDir,
) -> warpui::ModelHandle<LocalDiffStateModel> {
    let project = temp_dir.path().join("project");
    let git_dir = project.join(".git");
    std::fs::create_dir_all(git_dir.join("objects")).expect("create objects");
    std::fs::create_dir_all(git_dir.join("refs")).expect("create refs");
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("write HEAD");
    std::fs::write(git_dir.join("config"), "").expect("write config");

    let model = app.add_model(|ctx| {
        LocalDiffStateModel::new(Some(project.to_string_lossy().into_owned()), ctx)
    });
    await_repo_detection(app, &model).await;
    model.update(app, |model, ctx| {
        model.add_refresh_consumer(DiffRefreshConsumer::RemoteSubscribers, ctx);
        assert!(
            model.repository.is_some(),
            "test setup should resolve a repository"
        );
    });
    model
}

#[cfg(feature = "local_fs")]
fn in_flight_load_id(model: &LocalDiffStateModel) -> warpui::r#async::FutureId {
    model
        .computing_diffs_abort_handle
        .as_ref()
        .expect("a full load should be in flight")
        .future_id()
}

#[cfg(feature = "local_fs")]
#[test]
fn file_updates_during_a_full_load_are_accumulated_instead_of_restarting_it() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            model.load_diffs_for_current_repo(false, false, ctx);
            let original_load = in_flight_load_id(model);

            model.handle_file_update(modified_files_update(&["a.txt"], false), ctx);
            model.handle_file_update(modified_files_update(&["b.txt"], false), ctx);
            model.handle_file_update(modified_files_update(&["c.txt"], false), ctx);

            assert_eq!(
                in_flight_load_id(model),
                original_load,
                "ordinary file writes must not cancel and restart the running snapshot"
            );
            assert!(
                model.file_invalidation.queued_full_reload.is_none(),
                "ordinary file writes must not queue a follow-up full reload either"
            );
            let pending = model
                .pending_file_updates
                .as_ref()
                .expect("edits during the load should be held for later");
            assert_eq!(pending.pending_file_edits.len(), 3);
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn edits_deferred_during_a_full_load_are_applied_once_it_completes() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            model.load_diffs_for_current_repo(false, false, ctx);
            model.handle_file_update(modified_files_update(&["a.txt"], false), ctx);
            let target = model
                .file_invalidation
                .full_load_target
                .clone()
                .expect("the in-flight load records what it is computing against");

            model.handle_updated_state_for_repo((target, empty_diffs()), ctx);

            assert!(
                !model.file_invalidation.full_load_in_flight,
                "completing the load must release ownership so the queue can progress"
            );
            assert!(
                model.pending_file_updates.is_none(),
                "the deferred edit should have been drained into the invalidation queue"
            );
            assert!(matches!(model.state, InternalDiffState::Loaded(_)));
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn ignored_file_updates_during_a_full_load_do_nothing() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            model.load_diffs_for_current_repo(false, false, ctx);
            let original_load = in_flight_load_id(model);

            let needs_metadata_refresh =
                model.handle_file_update(modified_files_update(&["target/build.log"], true), ctx);

            assert!(
                !needs_metadata_refresh,
                "an update containing only ignored files is not a change worth refreshing for"
            );
            assert_eq!(in_flight_load_id(model), original_load);
            assert!(model.pending_file_updates.is_none());
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn a_reload_requested_during_a_full_load_is_coalesced_into_one_follow_up() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            model.load_diffs_for_current_repo(false, false, ctx);
            let original_load = in_flight_load_id(model);

            model.load_diffs_for_current_repo(false, false, ctx);
            model.load_diffs_for_current_repo(true, false, ctx);

            assert_eq!(
                in_flight_load_id(model),
                original_load,
                "the running snapshot is allowed to finish"
            );
            let queued = model
                .file_invalidation
                .queued_full_reload
                .as_ref()
                .expect("a single follow-up reload should be queued");
            assert!(
                queued.should_fetch_base,
                "the follow-up must keep the strongest requested options"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn changing_diff_mode_during_a_full_load_starts_a_new_load() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            model.load_diffs_for_current_repo(false, false, ctx);
            let original_load = in_flight_load_id(model);

            model.set_diff_mode(DiffMode::MainBranch, false, false, ctx);

            assert_ne!(
                in_flight_load_id(model),
                original_load,
                "a new comparison base makes the running snapshot obsolete"
            );
            assert!(model.file_invalidation.queued_full_reload.is_none());
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn a_snapshot_computed_against_a_stale_diff_mode_is_not_published() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            model.load_diffs_for_current_repo(false, false, ctx);
            let stale_target = model
                .file_invalidation
                .full_load_target
                .clone()
                .expect("in-flight target");
            model.set_diff_mode(DiffMode::MainBranch, false, false, ctx);

            model.handle_updated_state_for_repo((stale_target, empty_diffs()), ctx);

            assert!(
                matches!(model.state, InternalDiffState::Loading),
                "the obsolete snapshot must not be published as the current state"
            );
            assert!(
                model.file_invalidation.full_load_in_flight,
                "the load started for the new mode still owns the diff state"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn a_held_index_lock_defers_work_until_it_clears() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            assert!(!model.handle_file_update(index_lock_update(true), ctx));
            assert!(model.file_invalidation.index_lock_held);
            assert!(
                model.computing_diffs_abort_handle.is_none(),
                "no snapshot should be read while the index is locked"
            );

            model.handle_file_update(
                modified_files_update_with_lock(&["a.txt"], false, true /* index_lock_held */),
                ctx,
            );
            assert!(
                model.computing_diffs_abort_handle.is_none(),
                "the lock is still held, so per-file work stays deferred too"
            );

            assert!(model.handle_file_update(modified_files_update(&["a.txt"], false), ctx));

            assert!(!model.file_invalidation.index_lock_held);
            assert!(
                model.file_invalidation.full_load_in_flight,
                "releasing the lock triggers exactly one full reload"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn dropping_the_last_refresh_consumer_stops_in_flight_work() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;

        model.update(&mut app, |model, ctx| {
            model.load_diffs_for_current_repo(false, false, ctx);
            model.handle_file_update(modified_files_update(&["a.txt"], false), ctx);

            model.remove_refresh_consumer(DiffRefreshConsumer::RemoteSubscribers);

            assert!(!model.file_invalidation.full_load_in_flight);
            assert!(model.computing_diffs_abort_handle.is_none());
            assert!(model.pending_file_updates.is_none());
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn one_consumer_leaving_keeps_the_model_refreshing_for_the_others() {
    warpui::App::test((), |mut app| async move {
        app.add_singleton_model(repo_metadata::watcher::DirectoryWatcher::new_for_testing);
        app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let model = model_watching_a_repo(&mut app, &temp_dir).await;
        let other_panel = DiffRefreshConsumer::CodeReviewView(model.id());

        model.update(&mut app, |model, ctx| {
            model.add_refresh_consumer(other_panel, ctx);

            model.remove_refresh_consumer(other_panel);

            assert!(
                model.refresh_enabled(),
                "the remaining consumer still needs this repository's diffs"
            );
            assert!(
                model.handle_file_update(modified_files_update(&["a.txt"], false), ctx),
                "watcher updates must keep being processed for the remaining consumer"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
async fn commit_all(repo_path: &Path, message: &str) {
    run_git_command(repo_path, &["add", "-A"])
        .await
        .expect("git add");
    run_git_command(repo_path, &["commit", "-m", message])
        .await
        .expect("git commit");
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn an_oversized_text_diff_is_unrenderable_and_carries_no_base_content() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();

    let padding = "x".repeat(40);
    let huge_before: String = (0..60_000)
        .map(|i| format!("original line {i} {padding}\n"))
        .collect();
    let huge_after: String = (0..60_000)
        .map(|i| format!("rewritten line {i} {padding}\n"))
        .collect();
    assert!(huge_before.len() + huge_after.len() > MAX_DIFF_SIZE);
    std::fs::write(repo_path.join("huge.txt"), &huge_before).expect("write huge.txt");
    std::fs::write(repo_path.join("small.txt"), "one\n").expect("write small.txt");
    commit_all(repo_path, "initial").await;
    std::fs::write(repo_path.join("huge.txt"), &huge_after).expect("rewrite huge.txt");
    std::fs::write(repo_path.join("small.txt"), "one\ntwo\n").expect("edit small.txt");

    let diffs = LocalDiffStateModel::diff_state_against_head(repo_path)
        .await
        .expect("diff state");

    let huge = diffs
        .files
        .iter()
        .find(|f| f.file_diff.file_path == "huge.txt")
        .expect("huge.txt should still be listed");
    assert!(huge.file_diff.is_unrenderable());
    assert_eq!(
        huge.content_at_head, None,
        "base content for a file the panel cannot render should not be read or retained"
    );

    let small = diffs
        .files
        .iter()
        .find(|f| f.file_diff.file_path == "small.txt")
        .expect("small.txt should be listed");
    assert!(!small.file_diff.is_unrenderable());
    assert_eq!(small.content_at_head.as_deref(), Some("one\n"));
}

#[cfg(feature = "local_fs")]
#[tokio::test]
async fn a_deletion_beyond_the_render_limit_is_unrenderable_and_carries_no_base_content() {
    let repo_dir = init_discard_repo().await;
    let repo_path = repo_dir.path();
    let many_lines: String = (0..9_000).map(|i| format!("line {i}\n")).collect();
    std::fs::write(repo_path.join("deleted.txt"), &many_lines).expect("write deleted.txt");
    std::fs::write(repo_path.join("kept.txt"), "one\n").expect("write kept.txt");
    commit_all(repo_path, "initial").await;
    std::fs::remove_file(repo_path.join("deleted.txt")).expect("delete file");
    std::fs::write(repo_path.join("kept.txt"), "one\ntwo\n").expect("edit kept.txt");

    let diffs = LocalDiffStateModel::diff_state_against_head(repo_path)
        .await
        .expect("diff state");

    let deleted = diffs
        .files
        .iter()
        .find(|f| f.file_diff.file_path == "deleted.txt")
        .expect("deleted.txt should still be listed");
    assert!(deleted.file_diff.is_unrenderable());
    assert_eq!(deleted.content_at_head, None);

    let kept = diffs
        .files
        .iter()
        .find(|f| f.file_diff.file_path == "kept.txt")
        .expect("kept.txt should be listed");
    assert_eq!(kept.content_at_head.as_deref(), Some("one\n"));
}
