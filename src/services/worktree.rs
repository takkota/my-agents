use crate::domain::task::WorktreeInfo;
use crate::error::AppResult;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Maximum number of retry attempts for git commands that may fail due to
/// transient issues (e.g. lock files held by concurrent git processes).
const MAX_RETRIES: u32 = 3;

/// Delay between retry attempts in milliseconds.
const RETRY_DELAY_MS: u64 = 1000;

/// Run a git command with stdout/stderr captured (not leaked to the TUI).
/// Returns the captured stderr on failure for diagnostics.
fn run_git(upstream_repo: &Path, args: &[&str]) -> AppResult<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(upstream_repo)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git {} failed in {:?}: {}",
            args.first().unwrap_or(&""),
            upstream_repo,
            stderr.trim()
        );
    }
    Ok(())
}

/// Run a git command with retries for transient failures (e.g. lock contention).
fn run_git_with_retry(upstream_repo: &Path, args: &[&str]) -> AppResult<()> {
    let mut last_err = None;
    for attempt in 0..MAX_RETRIES {
        match run_git(upstream_repo, args) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let err_msg = e.to_string();
                // Retry on lock-related errors
                if err_msg.contains("Unable to create")
                    && err_msg.contains(".lock")
                    || err_msg.contains("Another git process seems to be running")
                {
                    if attempt + 1 < MAX_RETRIES {
                        std::thread::sleep(std::time::Duration::from_millis(
                            RETRY_DELAY_MS * (attempt as u64 + 1),
                        ));
                        last_err = Some(e);
                        continue;
                    }
                }
                last_err = Some(e);
                break;
            }
        }
    }
    Err(last_err.unwrap())
}

/// Run a git command and capture stdout.
fn run_git_output(upstream_repo: &Path, args: &[&str]) -> AppResult<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(upstream_repo)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git {} failed in {:?}: {}",
            args.first().unwrap_or(&""),
            upstream_repo,
            stderr.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Detect the remote default branch (origin/main or origin/master).
fn detect_remote_default_branch(upstream_repo: &Path) -> AppResult<String> {
    // Try `git symbolic-ref refs/remotes/origin/HEAD` first
    if let Ok(symbolic) = run_git_output(
        upstream_repo,
        &["symbolic-ref", "refs/remotes/origin/HEAD"],
    ) {
        // Returns something like "refs/remotes/origin/main"
        if let Some(branch) = symbolic.strip_prefix("refs/remotes/") {
            return Ok(branch.to_string());
        }
    }
    // Fallback: check if origin/main exists, then origin/master
    if run_git_output(upstream_repo, &["rev-parse", "--verify", "origin/main"]).is_ok() {
        return Ok("origin/main".to_string());
    }
    if run_git_output(upstream_repo, &["rev-parse", "--verify", "origin/master"]).is_ok() {
        return Ok("origin/master".to_string());
    }
    anyhow::bail!(
        "Could not detect remote default branch in {:?}",
        upstream_repo
    );
}

/// Log a worktree error to a diagnostic file for post-mortem analysis.
pub fn log_worktree_error(task_dir: &Path, message: &str) {
    let log_path = task_dir.join(".worktree_error.log");
    let timestamp = chrono::Utc::now().to_rfc3339();
    let entry = format!("[{}] {}\n", timestamp, message);
    // Append to existing log file or create new one
    if let Ok(existing) = std::fs::read_to_string(&log_path) {
        let _ = std::fs::write(&log_path, format!("{}{}", existing, entry));
    } else {
        let _ = std::fs::write(&log_path, entry);
    }
}

pub struct WorktreeService;

impl WorktreeService {
    pub fn new() -> Self {
        Self
    }

    /// Create a worktree directory without checking out files.
    /// This avoids triggering post-checkout hooks, allowing files to be
    /// copied into the worktree before checkout.
    /// Call `checkout_worktree` afterwards to complete the setup.
    pub fn add_worktree(
        &self,
        upstream_repo: &Path,
        target_dir: &Path,
        branch: &str,
    ) -> AppResult<()> {
        // Fetch latest from origin so worktree starts from up-to-date remote state.
        // If fetch fails (offline, auth error, etc.), fall back to HEAD to avoid
        // using stale remote-tracking refs.
        let fetch_ok = run_git(upstream_repo, &["fetch", "origin"]).is_ok();

        let start_point = if fetch_ok {
            // Only use remote branch when fetch succeeded (refs are up-to-date)
            detect_remote_default_branch(upstream_repo).ok()
        } else {
            // Fetch failed (offline, auth error, etc.) — don't use potentially
            // stale remote-tracking refs; fall back to HEAD instead.
            None
        };
        let start_point = start_point.as_deref().unwrap_or("HEAD");

        run_git_with_retry(
            upstream_repo,
            &[
                "worktree",
                "add",
                "--no-checkout",
                &target_dir.to_string_lossy(),
                "-b",
                branch,
                start_point,
            ],
        )
    }

    /// Checkout files in an already-created worktree.
    /// Uses `-f` to overwrite untracked files (e.g. files copied before
    /// checkout). This triggers post-checkout hooks, so any files copied
    /// into the worktree beforehand (e.g. `.env`) can be updated by hooks.
    pub fn checkout_worktree(worktree_path: &Path, branch: &str) -> AppResult<()> {
        let output = Command::new("git")
            .arg("-C")
            .arg(worktree_path)
            .args(["checkout", "-f", branch])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git checkout failed in {:?}: {}",
                worktree_path,
                stderr.trim()
            );
        }
        Ok(())
    }

    pub fn remove_worktree(&self, wt: &WorktreeInfo) -> AppResult<()> {
        // First try to remove gracefully
        let remove_result = run_git(
            &wt.upstream_path,
            &[
                "worktree",
                "remove",
                &wt.worktree_path.to_string_lossy(),
                "--force",
            ],
        );
        if remove_result.is_err() {
            // Prune if remove fails
            run_git(&wt.upstream_path, &["worktree", "prune"])?;
        }

        // Delete the branch (best-effort: ignore failure e.g. branch not found)
        let _ = run_git(&wt.upstream_path, &["branch", "-D", &wt.branch]);

        Ok(())
    }

    /// Copy specified files from upstream repo to worktree directory.
    /// Files that don't exist in the upstream repo are silently skipped.
    pub fn copy_files_to_worktree(
        upstream_repo: &Path,
        worktree_path: &Path,
        files: &[String],
    ) -> AppResult<()> {
        for file in files {
            let src = upstream_repo.join(file);
            let dst = worktree_path.join(file);
            if src.exists() {
                // Ensure parent directory exists
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&src, &dst)?;
            }
        }
        Ok(())
    }

    pub fn create_worktrees_for_task(
        &self,
        task_dir: &Path,
        task_id: &str,
        repos: &[(String, PathBuf)],
    ) -> AppResult<Vec<WorktreeInfo>> {
        let mut worktrees = Vec::new();
        for (repo_name, upstream_path) in repos {
            let worktree_path = task_dir.join(repo_name);
            let branch = task_id[..task_id.len().min(6)].to_string();

            if let Err(e) = self.add_worktree(upstream_path, &worktree_path, &branch) {
                log_worktree_error(
                    task_dir,
                    &format!(
                        "add_worktree failed for repo={}, branch={}, upstream={:?}, target={:?}: {}",
                        repo_name, branch, upstream_path, worktree_path, e
                    ),
                );
                // Rollback previously created worktrees
                for wt in &worktrees {
                    let _ = self.remove_worktree(wt);
                }
                return Err(e);
            }

            worktrees.push(WorktreeInfo {
                repo_name: repo_name.clone(),
                upstream_path: upstream_path.clone(),
                worktree_path,
                branch,
            });
        }
        Ok(worktrees)
    }
}
