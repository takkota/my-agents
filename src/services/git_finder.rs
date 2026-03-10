use crate::error::AppResult;
use std::path::PathBuf;
use std::process::Command;

/// Find git repositories under common directories using `find` with depth limits.
pub fn find_git_repos() -> AppResult<Vec<PathBuf>> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let search_dirs = vec![
        home.clone(),
    ];

    let mut repos = Vec::new();
    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }
        let output = Command::new("find")
            .args([
                dir.to_string_lossy().as_ref(),
                "-maxdepth",
                "4",
                "-name",
                ".git",
                "-type",
                "d",
                "-not",
                "-path",
                "*/node_modules/*",
                "-not",
                "-path",
                "*/.my-agents/*",
            ])
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let git_dir = PathBuf::from(line);
                if let Some(parent) = git_dir.parent() {
                    repos.push(parent.to_path_buf());
                }
            }
        }
    }

    repos.sort();
    repos.dedup();
    Ok(repos)
}

