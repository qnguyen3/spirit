use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRoot {
    pub root: PathBuf,
    pub resolved_from_linked_worktree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeListEntry {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub is_main: bool,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchDeleteOutcome {
    Deleted,
    KeptUnmerged,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncludeCopyReport {
    pub copied: usize,
    pub skipped_over_budget: usize,
    pub errors: usize,
}

impl IncludeCopyReport {
    pub fn summary(&self) -> Option<String> {
        if self.skipped_over_budget == 0 && self.errors == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if self.skipped_over_budget > 0 {
            parts.push(format!(
                "{} file(s) skipped (copy budget exceeded)",
                self.skipped_over_budget
            ));
        }
        if self.errors > 0 {
            parts.push(format!("{} file(s) failed to copy", self.errors));
        }
        Some(format!(".worktreeinclude: {}", parts.join(", ")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludePattern {
    pub directory: String,
    pub file_prefix: String,
    pub has_wildcard: bool,
}

pub fn parse_worktree_include(contents: &str) -> Vec<IncludePattern> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let normalized = line.replace('\\', "/");
            if normalized.starts_with('/')
                || normalized.starts_with("~")
                || normalized.split('/').any(|part| part == "..")
            {
                return None;
            }
            let (directory, file) = match normalized.rsplit_once('/') {
                Some((directory, file)) => (directory.to_owned(), file.to_owned()),
                None => (String::new(), normalized),
            };
            let has_wildcard = file.ends_with('*');
            let file_prefix = file.trim_end_matches('*').to_owned();
            if file_prefix.is_empty() && !has_wildcard {
                return None;
            }
            Some(IncludePattern {
                directory,
                file_prefix,
                has_wildcard,
            })
        })
        .collect()
}

impl IncludePattern {
    pub fn matches(&self, file_name: &str) -> bool {
        if self.has_wildcard {
            file_name.starts_with(&self.file_prefix)
        } else {
            file_name == self.file_prefix
        }
    }
}

pub const INCLUDE_COPY_MAX_FILES: usize = 5_000;
pub const INCLUDE_COPY_MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClonePhase {
    Counting,
    Compressing,
    Receiving,
    Resolving,
    CheckingOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloneProgress {
    pub phase: ClonePhase,
    pub percent: Option<u8>,
}

impl ClonePhase {
    pub fn label(self) -> &'static str {
        match self {
            ClonePhase::Counting => "Counting objects",
            ClonePhase::Compressing => "Compressing objects",
            ClonePhase::Receiving => "Receiving objects",
            ClonePhase::Resolving => "Resolving deltas",
            ClonePhase::CheckingOut => "Checking out files",
        }
    }
}

// Two repos with the same folder name share this directory; existing users'
// worktrees live here, so collisions are resolved at the leaf, not by changing
// the convention.
pub fn generated_worktree_repo_dir(repo_path: &Path) -> PathBuf {
    let repo_name = repo_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("untitled");
    warp_core::paths::data_dir()
        .join("worktrees")
        .join(repo_name)
}

pub fn generated_worktree_path(repo_path: &Path, worktree_name: &str) -> PathBuf {
    generated_worktree_repo_dir(repo_path).join(worktree_name)
}

pub fn sanitize_worktree_name(raw: &str) -> String {
    let mut sanitized = String::with_capacity(raw.len());
    let mut pending_separator = false;
    for character in raw.chars() {
        if character.is_alphanumeric() || character == '.' || character == '_' || character == '-' {
            if pending_separator && !sanitized.is_empty() {
                sanitized.push('-');
            }
            pending_separator = false;
            sanitized.push(character);
        } else {
            pending_separator = true;
        }
    }

    while sanitized.contains("..") {
        sanitized = sanitized.replace("..", ".");
    }

    let trimmed = sanitized.trim_matches(['.', '-']).to_owned();
    if trimmed.is_empty() {
        "worktree".to_owned()
    } else {
        trimmed
    }
}

pub fn next_available(base: &str, taken: &dyn Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !taken(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

pub fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeListEntry> {
    let mut entries = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut head: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut locked = false;
    let mut prunable = false;

    let flush = |path: &mut Option<PathBuf>,
                 head: &mut Option<String>,
                 branch: &mut Option<String>,
                 locked: &mut bool,
                 prunable: &mut bool,
                 entries: &mut Vec<WorktreeListEntry>| {
        if let Some(path) = path.take() {
            let is_main = entries.is_empty();
            entries.push(WorktreeListEntry {
                path,
                head: head.take(),
                branch: branch.take(),
                is_main,
                locked: std::mem::take(locked),
                prunable: std::mem::take(prunable),
            });
        }
    };

    for line in porcelain.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            flush(
                &mut path,
                &mut head,
                &mut branch,
                &mut locked,
                &mut prunable,
                &mut entries,
            );
            continue;
        }
        if let Some(value) = line.strip_prefix("worktree ") {
            flush(
                &mut path,
                &mut head,
                &mut branch,
                &mut locked,
                &mut prunable,
                &mut entries,
            );
            path = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("HEAD ") {
            head = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("branch ") {
            branch = Some(
                value
                    .strip_prefix("refs/heads/")
                    .unwrap_or(value)
                    .to_owned(),
            );
        } else if line == "detached" {
            branch = None;
        } else if line == "locked" || line.starts_with("locked ") {
            locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            prunable = true;
        }
    }
    flush(
        &mut path,
        &mut head,
        &mut branch,
        &mut locked,
        &mut prunable,
        &mut entries,
    );

    entries
}

pub fn parse_clone_progress(line: &str) -> Option<CloneProgress> {
    let (phase, rest) = if let Some(rest) = line.strip_prefix("Receiving objects:") {
        (ClonePhase::Receiving, rest)
    } else if let Some(rest) = line.strip_prefix("Resolving deltas:") {
        (ClonePhase::Resolving, rest)
    } else if let Some(rest) = line.strip_prefix("Compressing objects:") {
        (ClonePhase::Compressing, rest)
    } else if let Some(rest) = line.strip_prefix("remote: Compressing objects:") {
        (ClonePhase::Compressing, rest)
    } else if let Some(rest) = line.strip_prefix("remote: Counting objects:") {
        (ClonePhase::Counting, rest)
    } else if let Some(rest) = line.strip_prefix("Counting objects:") {
        (ClonePhase::Counting, rest)
    } else if let Some(rest) = line.strip_prefix("Updating files:") {
        (ClonePhase::CheckingOut, rest)
    } else {
        return None;
    };

    let percent = rest
        .trim_start()
        .split('%')
        .next()
        .and_then(|digits| digits.trim().parse::<u8>().ok())
        .filter(|percent| *percent <= 100);

    Some(CloneProgress { phase, percent })
}

pub fn derive_clone_directory_name(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed
        .rsplit(['/', ':'])
        .find(|segment| !segment.is_empty())?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(feature = "local_fs")]
mod imp {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result, anyhow, bail};
    use warp_util::git::{run_git_command, run_git_command_with_env};

    use super::{
        BranchDeleteOutcome, CloneProgress, INCLUDE_COPY_MAX_BYTES, INCLUDE_COPY_MAX_FILES,
        IncludeCopyReport, RepoRoot, WorktreeListEntry, derive_clone_directory_name,
        parse_clone_progress, parse_worktree_include, parse_worktree_list,
    };

    pub async fn discover_repo_root(path: &Path) -> Result<Option<RepoRoot>> {
        let Ok(toplevel) = run_git_command(path, &["rev-parse", "--show-toplevel"]).await else {
            return Ok(None);
        };
        let toplevel = PathBuf::from(toplevel.trim());
        if toplevel.as_os_str().is_empty() {
            return Ok(None);
        }

        let common_dir = run_git_command(path, &["rev-parse", "--git-common-dir"])
            .await
            .map(|output| PathBuf::from(output.trim()))
            .unwrap_or_else(|_| toplevel.join(".git"));
        let common_dir = if common_dir.is_absolute() {
            common_dir
        } else {
            path.join(common_dir)
        };
        let common_dir = std::fs::canonicalize(&common_dir).unwrap_or(common_dir);

        let main_root = common_dir
            .parent()
            .filter(|_| common_dir.file_name().is_some_and(|name| name == ".git"))
            .map(Path::to_path_buf);

        match main_root {
            Some(main_root) if !same_path(&main_root, &toplevel) => Ok(Some(RepoRoot {
                root: main_root,
                resolved_from_linked_worktree: true,
            })),
            _ => Ok(Some(RepoRoot {
                root: toplevel,
                resolved_from_linked_worktree: false,
            })),
        }
    }

    pub fn same_path(left: &Path, right: &Path) -> bool {
        let canonical = |path: &Path| std::fs::canonicalize(path).unwrap_or(path.to_path_buf());
        canonical(left) == canonical(right)
    }

    pub async fn detect_primary_branch(root: &Path) -> Result<String> {
        if let Ok(output) =
            run_git_command(root, &["symbolic-ref", "refs/remotes/origin/HEAD"]).await
            && let Some(branch) = output.trim().strip_prefix("refs/remotes/origin/")
            && !branch.is_empty()
        {
            return Ok(branch.to_owned());
        }

        for candidate in ["origin/main", "origin/master", "main", "master"] {
            let verified = run_git_command(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{candidate}^{{commit}}"),
                ],
            )
            .await;
            if verified.is_ok() {
                return Ok(candidate
                    .strip_prefix("origin/")
                    .unwrap_or(candidate)
                    .to_owned());
            }
        }

        let current = crate::util::git::detect_current_branch(root).await?;
        if current.is_empty() || current == "HEAD" {
            bail!(
                "Could not determine a primary branch for {}",
                root.display()
            );
        }
        Ok(current)
    }

    pub async fn worktree_add(
        root: &Path,
        branch: &str,
        path: &Path,
        base: &str,
        path_env: Option<&str>,
    ) -> Result<()> {
        let path = path.to_string_lossy().to_string();
        run_git_command_with_env(
            root,
            &["worktree", "add", "--no-track", "-b", branch, &path, base],
            path_env,
        )
        .await
        .map(|_| ())
        .with_context(|| format!("Failed to create worktree for branch {branch}"))
    }

    pub async fn worktree_add_or_rollback(
        root: &Path,
        branch: &str,
        path: &Path,
        base: &str,
        path_env: Option<&str>,
    ) -> Result<()> {
        let added = worktree_add(root, branch, path, base, path_env).await;
        let failure = match added {
            Err(err) => err,
            Ok(()) if !path.exists() => {
                anyhow!("git created no worktree at {}", path.display())
            }
            Ok(()) => return Ok(()),
        };

        let _ = worktree_remove(root, path, true, path_env).await;
        let _ = worktree_prune(root).await;
        let _ = force_delete_branch(root, branch).await;
        Err(failure)
    }

    pub async fn worktree_list(root: &Path) -> Result<Vec<WorktreeListEntry>> {
        let output = run_git_command(root, &["worktree", "list", "--porcelain"])
            .await
            .context("Failed to list worktrees")?;
        Ok(parse_worktree_list(&output))
    }

    pub async fn worktree_remove(
        root: &Path,
        path: &Path,
        force: bool,
        path_env: Option<&str>,
    ) -> Result<()> {
        let path_arg = path.to_string_lossy().to_string();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path_arg);

        let Err(err) = run_git_command_with_env(root, &args, path_env).await else {
            return Ok(());
        };

        let _ = worktree_prune(root).await;
        let still_listed = worktree_list(root)
            .await
            .map(|entries| entries.iter().any(|entry| same_path(&entry.path, path)))
            .unwrap_or(true);
        if still_listed {
            Err(err.context(format!("Failed to remove worktree at {path_arg}")))
        } else {
            Ok(())
        }
    }

    pub async fn worktree_prune(root: &Path) -> Result<()> {
        run_git_command(root, &["worktree", "prune"])
            .await
            .map(|_| ())
            .context("Failed to prune worktrees")
    }

    pub async fn delete_branch_safe(root: &Path, branch: &str) -> Result<BranchDeleteOutcome> {
        match run_git_command(root, &["branch", "-d", branch]).await {
            Ok(_) => Ok(BranchDeleteOutcome::Deleted),
            Err(err) => {
                let message = err.to_string();
                if message.contains("not fully merged") {
                    Ok(BranchDeleteOutcome::KeptUnmerged)
                } else {
                    Err(err.context(format!("Failed to delete branch {branch}")))
                }
            }
        }
    }

    pub async fn force_delete_branch(root: &Path, branch: &str) -> Result<()> {
        run_git_command(root, &["branch", "-D", branch])
            .await
            .map(|_| ())
            .with_context(|| format!("Failed to force-delete branch {branch}"))
    }

    pub async fn status_is_dirty(worktree_path: &Path, path_env: Option<&str>) -> Result<bool> {
        let output = run_git_command_with_env(
            worktree_path,
            &["status", "--porcelain", "--untracked-files=all", "-z"],
            path_env,
        )
        .await
        .context("Failed to read worktree status")?;
        Ok(!output.trim_matches(['\0', ' ', '\n', '\r']).is_empty())
    }

    pub async fn current_branch(worktree_path: &Path) -> Result<String> {
        crate::util::git::detect_current_branch(worktree_path).await
    }

    pub async fn local_branches(root: &Path) -> HashSet<String> {
        run_git_command(root, &["branch", "--list", "--format=%(refname:short)"])
            .await
            .map(|output| {
                output
                    .lines()
                    .map(|line| line.trim().to_owned())
                    .filter(|line| !line.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn init_new_project(path: &Path, path_env: Option<&str>) -> Result<()> {
        let created_dir = prepare_new_project_dir(path)?;

        let result = async {
            run_git_command(path, &["init"])
                .await
                .context("Failed to run git init")?;
            run_git_command_with_env(
                path,
                &["commit", "--allow-empty", "-m", "Initial commit"],
                path_env,
            )
            .await
            .map_err(missing_git_identity_hint)?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        if result.is_err() {
            if created_dir {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_dir_all(path.join(".git"));
            }
        }
        result
    }

    fn missing_git_identity_hint(err: anyhow::Error) -> anyhow::Error {
        let message = err.to_string();
        if message.contains("Please tell me who you are")
            || message.contains("unable to auto-detect email address")
            || message.contains("empty ident name")
        {
            anyhow!(
                "git has no author identity configured. Set one with:\n  git config --global user.name \"Your Name\"\n  git config --global user.email \"you@example.com\""
            )
        } else {
            err.context("Failed to create the initial commit")
        }
    }

    fn prepare_new_project_dir(path: &Path) -> Result<bool> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        match std::fs::read_dir(path) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    bail!("{} already exists and is not empty", path.display());
                }
                Ok(false)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(path)
                    .with_context(|| format!("Failed to create {}", path.display()))?;
                Ok(true)
            }
            Err(err) => {
                Err(anyhow::Error::new(err)
                    .context(format!("Failed to inspect {}", path.display())))
            }
        }
    }

    pub async fn clone(
        url: &str,
        dest_parent: &Path,
        dir_name: Option<&str>,
        path_env: Option<&str>,
        mut progress: impl FnMut(CloneProgress),
        cancel: impl Fn() -> bool,
    ) -> Result<PathBuf> {
        use command::Stdio;
        use command::r#async::Command;
        use futures::io::{AsyncBufReadExt, BufReader};

        let dir_name = dir_name
            .map(str::to_owned)
            .or_else(|| derive_clone_directory_name(url))
            .ok_or_else(|| anyhow!("Could not derive a directory name from {url}"))?;
        let dest = dest_parent.join(&dir_name);

        std::fs::create_dir_all(dest_parent)
            .with_context(|| format!("Failed to create {}", dest_parent.display()))?;
        let created_dest = match std::fs::read_dir(&dest) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    bail!("{} already exists and is not empty", dest.display());
                }
                false
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&dest)
                    .with_context(|| format!("Failed to create {}", dest.display()))?;
                true
            }
            Err(err) => {
                return Err(anyhow::Error::new(err)
                    .context(format!("Failed to inspect {}", dest.display())));
            }
        };

        let cleanup = || {
            if created_dest {
                let _ = std::fs::remove_dir_all(&dest);
            }
        };

        let mut command = Command::new("git");
        command
            .args([
                "clone",
                "--progress",
                "--",
                url,
                dest.to_string_lossy().as_ref(),
            ])
            .current_dir(dest_parent)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "")
            .env("GIT_OPTIONAL_LOCKS", "0");
        if let Some(path_env) = path_env {
            command.env("PATH", path_env);
        }
        let mut child = command
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn git clone")?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("git clone produced no progress stream"))?;

        let mut tail: Vec<String> = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut cancelled = false;
        loop {
            let mut chunk = Vec::new();
            match reader.read_until(b'\r', &mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(update) = parse_clone_progress(line) {
                    progress(update);
                } else {
                    tail.push(line.to_owned());
                    if tail.len() > 8 {
                        tail.remove(0);
                    }
                }
            }
            if cancel() {
                cancelled = true;
                let _ = child.kill();
                break;
            }
        }

        let status = child.status().await.context("git clone failed")?;
        if cancelled {
            cleanup();
            bail!("Clone cancelled");
        }
        if !status.success() {
            cleanup();
            let detail = tail.join("\n");
            let host = clone_url_host(url);
            log::warn!("git clone from {host} failed");
            bail!(if detail.is_empty() {
                "git clone failed".to_owned()
            } else {
                detail
            });
        }

        Ok(dest)
    }

    pub async fn copy_worktree_includes(
        root: &Path,
        worktree_path: &Path,
    ) -> Result<IncludeCopyReport> {
        let include_file = root.join(".worktreeinclude");
        let Ok(contents) = std::fs::read_to_string(&include_file) else {
            return Ok(IncludeCopyReport::default());
        };

        let mut report = IncludeCopyReport::default();
        let mut copied_bytes = 0u64;

        for pattern in parse_worktree_include(&contents) {
            let source_dir = root.join(&pattern.directory);
            if !source_dir.starts_with(root) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&source_dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let Some(file_name) = file_name.to_str() else {
                    continue;
                };
                if !pattern.matches(file_name) {
                    continue;
                }
                if !entry.path().is_file() {
                    continue;
                }
                let destination = worktree_path.join(&pattern.directory).join(file_name);
                if destination.exists() {
                    continue;
                }

                let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                if report.copied >= INCLUDE_COPY_MAX_FILES
                    || copied_bytes.saturating_add(size) > INCLUDE_COPY_MAX_BYTES
                {
                    report.skipped_over_budget += 1;
                    continue;
                }

                if let Some(parent) = destination.parent()
                    && std::fs::create_dir_all(parent).is_err()
                {
                    report.errors += 1;
                    continue;
                }
                match std::fs::copy(entry.path(), &destination) {
                    Ok(_) => {
                        report.copied += 1;
                        copied_bytes = copied_bytes.saturating_add(size);
                    }
                    Err(_) => report.errors += 1,
                }
            }
        }

        Ok(report)
    }

    fn clone_url_host(url: &str) -> String {
        let without_scheme = match url.split_once("://") {
            Some((_, rest)) => rest,
            None => url,
        };
        let without_credentials = match without_scheme.rsplit_once('@') {
            Some((_, rest)) => rest,
            None => without_scheme,
        };
        without_credentials
            .split(['/', ':'])
            .next()
            .unwrap_or("unknown host")
            .to_owned()
    }
}

#[cfg(not(feature = "local_fs"))]
mod imp {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use anyhow::{Result, anyhow};

    use super::{
        BranchDeleteOutcome, CloneProgress, IncludeCopyReport, RepoRoot, WorktreeListEntry,
    };

    fn unsupported<T>() -> Result<T> {
        Err(anyhow!("Git operations are not supported on this platform"))
    }

    pub async fn discover_repo_root(_path: &Path) -> Result<Option<RepoRoot>> {
        Ok(None)
    }

    pub fn same_path(left: &Path, right: &Path) -> bool {
        left == right
    }

    pub async fn detect_primary_branch(_root: &Path) -> Result<String> {
        unsupported()
    }

    pub async fn worktree_add(
        _root: &Path,
        _branch: &str,
        _path: &Path,
        _base: &str,
        _path_env: Option<&str>,
    ) -> Result<()> {
        unsupported()
    }

    pub async fn worktree_add_or_rollback(
        _root: &Path,
        _branch: &str,
        _path: &Path,
        _base: &str,
        _path_env: Option<&str>,
    ) -> Result<()> {
        unsupported()
    }

    pub async fn worktree_list(_root: &Path) -> Result<Vec<WorktreeListEntry>> {
        unsupported()
    }

    pub async fn worktree_remove(
        _root: &Path,
        _path: &Path,
        _force: bool,
        _path_env: Option<&str>,
    ) -> Result<()> {
        unsupported()
    }

    pub async fn worktree_prune(_root: &Path) -> Result<()> {
        unsupported()
    }

    pub async fn delete_branch_safe(_root: &Path, _branch: &str) -> Result<BranchDeleteOutcome> {
        unsupported()
    }

    pub async fn force_delete_branch(_root: &Path, _branch: &str) -> Result<()> {
        unsupported()
    }

    pub async fn status_is_dirty(_worktree_path: &Path, _path_env: Option<&str>) -> Result<bool> {
        unsupported()
    }

    pub async fn current_branch(_worktree_path: &Path) -> Result<String> {
        unsupported()
    }

    pub async fn local_branches(_root: &Path) -> HashSet<String> {
        HashSet::new()
    }

    pub async fn init_new_project(_path: &Path, _path_env: Option<&str>) -> Result<()> {
        unsupported()
    }

    pub async fn clone(
        _url: &str,
        _dest_parent: &Path,
        _dir_name: Option<&str>,
        _path_env: Option<&str>,
        _progress: impl FnMut(CloneProgress),
        _cancel: impl Fn() -> bool,
    ) -> Result<PathBuf> {
        unsupported()
    }

    pub async fn copy_worktree_includes(
        _root: &Path,
        _worktree_path: &Path,
    ) -> Result<IncludeCopyReport> {
        Ok(IncludeCopyReport::default())
    }
}

#[allow(unused_imports)]
pub use imp::{
    clone, copy_worktree_includes, current_branch, delete_branch_safe, detect_primary_branch,
    discover_repo_root, force_delete_branch, init_new_project, local_branches, same_path,
    status_is_dirty, worktree_add, worktree_add_or_rollback, worktree_list, worktree_prune,
    worktree_remove,
};

#[cfg(test)]
#[path = "git_ops_tests.rs"]
mod tests;
