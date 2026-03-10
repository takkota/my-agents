use crate::error::AppResult;
use std::path::PathBuf;
use std::process::Command;

/// Find git repositories under common directories.
/// Uses `fd` if available (much faster), otherwise falls back to `find` with pruning.
pub fn find_git_repos() -> AppResult<Vec<PathBuf>> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    let output = if has_fd() {
        find_with_fd(&home)?
    } else {
        find_with_find(&home)?
    };

    let mut repos: Vec<PathBuf> = output
        .lines()
        .filter_map(|line| {
            let git_dir = PathBuf::from(line);
            git_dir.parent().map(|p| p.to_path_buf())
        })
        .collect();

    repos.sort();
    repos.dedup();
    Ok(repos)
}

fn has_fd() -> bool {
    Command::new("fd")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn find_with_fd(home: &PathBuf) -> AppResult<String> {
    let output = Command::new("fd")
        .args([
            "--hidden",
            "--no-ignore",
            "--type",
            "d",
            "--max-depth",
            "4",
            "^\\.git$",
            &home.to_string_lossy(),
            "--exclude",
            "node_modules",
            "--exclude",
            ".my-agents",
            "--exclude",
            "Library",
            "--exclude",
            ".cache",
            "--exclude",
            ".local",
            "--exclude",
            ".npm",
            "--exclude",
            ".cargo",
            "--exclude",
            ".rustup",
            "--exclude",
            "Applications",
        ])
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn find_with_find(home: &PathBuf) -> AppResult<String> {
    // Use -prune to skip heavy directories entirely (not just filter output).
    // find $HOME \( -name node_modules -o -name .my-agents -o ... \) -prune \
    //   -o -name .git -type d -maxdepth 4 -print -prune
    // The trailing -prune after -print stops descending into .git dirs.
    let output = Command::new("find")
        .args([
            &*home.to_string_lossy(),
            "-maxdepth",
            "4",
            "(",
            "-name",
            "node_modules",
            "-o",
            "-name",
            ".my-agents",
            "-o",
            "-name",
            "Library",
            "-o",
            "-name",
            ".cache",
            "-o",
            "-name",
            ".local",
            "-o",
            "-name",
            ".npm",
            "-o",
            "-name",
            ".cargo",
            "-o",
            "-name",
            ".rustup",
            "-o",
            "-name",
            "Applications",
            ")",
            "-prune",
            "-o",
            "-name",
            ".git",
            "-type",
            "d",
            "-print",
            "-prune",
        ])
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
