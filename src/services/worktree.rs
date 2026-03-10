use crate::domain::task::WorktreeInfo;
use crate::error::AppResult;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct WorktreeService;

impl WorktreeService {
    pub fn new() -> Self {
        Self
    }

    pub fn add_worktree(
        &self,
        upstream_repo: &Path,
        target_dir: &Path,
        branch: &str,
    ) -> AppResult<()> {
        let status = Command::new("git")
            .args([
                "-C",
                &upstream_repo.to_string_lossy(),
                "worktree",
                "add",
                &target_dir.to_string_lossy(),
                "-b",
                branch,
            ])
            .status()?;
        if !status.success() {
            anyhow::bail!(
                "Failed to create worktree at {:?} from {:?}",
                target_dir,
                upstream_repo
            );
        }
        Ok(())
    }

    pub fn remove_worktree(&self, wt: &WorktreeInfo) -> AppResult<()> {
        // First try to remove gracefully
        let status = Command::new("git")
            .args([
                "-C",
                &wt.upstream_path.to_string_lossy(),
                "worktree",
                "remove",
                &wt.worktree_path.to_string_lossy(),
                "--force",
            ])
            .status()?;
        if !status.success() {
            // Prune if remove fails
            Command::new("git")
                .args([
                    "-C",
                    &wt.upstream_path.to_string_lossy(),
                    "worktree",
                    "prune",
                ])
                .status()?;
        }

        // Delete the branch
        Command::new("git")
            .args([
                "-C",
                &wt.upstream_path.to_string_lossy(),
                "branch",
                "-D",
                &wt.branch,
            ])
            .output()
            .ok();

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
            let branch = format!("task/{}/{}", &task_id[..task_id.len().min(6)], repo_name);

            if let Err(e) = self.add_worktree(upstream_path, &worktree_path, &branch) {
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
