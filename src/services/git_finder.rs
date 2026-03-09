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

/// Fuzzy match repo paths against a query string
pub fn fuzzy_match_repos(repos: &[PathBuf], query: &str) -> Vec<PathBuf> {
    if query.is_empty() {
        return repos.to_vec();
    }
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(usize, &PathBuf)> = repos
        .iter()
        .filter_map(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let full = path.to_string_lossy().to_lowercase();

            if name == query_lower {
                Some((100, path))
            } else if name.starts_with(&query_lower) {
                Some((80, path))
            } else if name.contains(&query_lower) {
                Some((60, path))
            } else if full.contains(&query_lower) {
                Some((40, path))
            } else {
                // Simple subsequence match
                let mut qi = query_lower.chars().peekable();
                for c in name.chars() {
                    if qi.peek() == Some(&c) {
                        qi.next();
                    }
                }
                if qi.peek().is_none() {
                    Some((20, path))
                } else {
                    None
                }
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, p)| p.clone()).collect()
}
